#!/usr/bin/env python3
"""Drive the live Library media detail against a controlled TV fixture."""

from __future__ import annotations

import gc
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

import libtorrent as lt

from application_surface_harness import build_gateway
from browser_peer_inspection_surface import (
    build_and_start_production_web,
    start_development_gateway,
    terminate_gateway,
)
from browser_surface_harness import reserve_loopback_port, stop_process
from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    compare_payloads,
    create_session,
    wait_for_listener,
    write_deterministic_range,
)
from udp_tracker_magnet import OneShotUdpTracker, tracker_magnet


ROOT_NAME = "North Shore Stories"
PIECE_SIZE = 256 * 1024
UPLOAD_RATE_LIMIT = 192 * 1024
FILES: tuple[tuple[str, int], ...] = (
    ("Season 01/North.Shore.Stories.S01E10.1080p.WEB-DL.mkv", 384 * 1024),
    ("poster.jpg", 4 * 1024),
    ("Season 01/North.Shore.Stories.S01E02.1080p.WEB-DL.mp4", 512 * 1024),
    ("Season 01/North.Shore.Stories.S01E01.1080p.WEB-DL.mkv", 640 * 1024),
    ("Season 01/North.Shore.Stories.S01E07E08.mkv", 768 * 1024),
    ("Season 02/North.Shore.Stories.S02E01.mkv", 896 * 1024),
    ("Extras/Behind the scenes.webm", 256 * 1024),
    ("README.nfo", 1024),
)


@dataclass(frozen=True)
class MediaFixture:
    seed_directory: Path
    torrent_info: lt.torrent_info
    info_hash: str
    info_bytes: bytes
    source_files: tuple[Path, ...]
    payload_sha1: str


def create_media_fixture(run_path: Path) -> MediaFixture:
    seed_directory = run_path / "seed"
    content_root = seed_directory / ROOT_NAME
    content_root.mkdir(parents=True)
    storage = lt.file_storage()
    source_files: list[Path] = []
    payload_digest = hashlib.sha1()
    logical_offset = 0
    for relative, length in FILES:
        source = content_root / relative
        write_deterministic_range(source, logical_offset, length)
        payload_digest.update(source.read_bytes())
        storage.add_file(f"{ROOT_NAME}/{relative}", length)
        source_files.append(source)
        logical_offset += length

    creator = lt.create_torrent(
        storage,
        piece_size=PIECE_SIZE,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(seed_directory))
    torrent_path = run_path / "media-library.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    torrent_info = lt.torrent_info(str(torrent_path))
    info_bytes = bytes(torrent_info.info_section())
    info_hash = str(torrent_info.info_hashes().v1)
    if torrent_info.files().num_files() != len(FILES):
        raise ScenarioFailure("media fixture file count changed during creation")
    if any(True for _ in torrent_info.trackers()):
        raise ScenarioFailure("media fixture unexpectedly contains a tracker")
    return MediaFixture(
        seed_directory=seed_directory,
        torrent_info=torrent_info,
        info_hash=info_hash,
        info_bytes=info_bytes,
        source_files=tuple(source_files),
        payload_sha1=payload_digest.hexdigest(),
    )


def run_playwright(
    repository: Path,
    origin: str,
    gateway_address: str,
    fixture: MediaFixture,
    tracker_port: int,
) -> str:
    environment = os.environ.copy()
    environment.update(
        {
            "RSTORRENT_PLAYWRIGHT_BASE_URL": origin,
            "RSTORRENT_LIVE_GATEWAY_URL": f"http://{gateway_address}",
            "RSTORRENT_LIVE_MAGNET": tracker_magnet(
                fixture.info_hash, tracker_port
            ),
            "RSTORRENT_LIVE_TORRENT_ID": fixture.info_hash,
            "RSTORRENT_LIVE_TORRENT_NAME": ROOT_NAME,
            "RSTORRENT_LIVE_FILE_COUNT": str(len(FILES)),
            "RSTORRENT_LIVE_MEDIA_LIBRARY": "1",
        }
    )
    completed = subprocess.run(
        [
            "npm",
            "run",
            "test:e2e",
            "--prefix",
            "clients/web",
            "--",
            "--grep",
            "live Library media detail",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        env=environment,
        timeout=120,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "live Library media browser test failed\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    return next(
        (
            line.strip()
            for line in completed.stdout.splitlines()
            if line.startswith("media_library_live_milestones ")
        ),
        "media_library_live_milestones unavailable",
    )


def verify_files(fixture: MediaFixture, storage: Path) -> None:
    payload_digest = hashlib.sha1()
    for (relative, _), source in zip(FILES, fixture.source_files, strict=True):
        output = storage / ROOT_NAME / relative
        compare_payloads(source, output)
        payload_digest.update(output.read_bytes())
    if payload_digest.hexdigest() != fixture.payload_sha1:
        raise ScenarioFailure("media fixture aggregate payload hash differs")


def run() -> None:
    repository = Path(__file__).resolve().parents[2]
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-browser-media-library-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    tracker: OneShotUdpTracker | None = None
    gateway: subprocess.Popen[str] | None = None
    vite: subprocess.Popen[str] | None = None
    failure: BaseException | None = None
    started = time.monotonic()
    try:
        fixture = create_media_fixture(run_path)
        diagnostics: list[str] = []
        session = create_session()
        session.apply_settings({"upload_rate_limit": UPLOAD_RATE_LIMIT})
        port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            fixture.torrent_info,
            fixture.seed_directory,
            diagnostics,
        )
        handle.set_upload_limit(UPLOAD_RATE_LIMIT)
        tracker = OneShotUdpTracker(
            fixture.info_hash,
            port,
            response_delay_seconds=1.5,
            expected_listen_port=None,
            accept_any_peer_id=True,
        )
        tracker.start()

        vite_port = reserve_loopback_port()
        origin = f"http://127.0.0.1:{vite_port}"
        storage = run_path / "downloads"
        gateway, address = start_development_gateway(
            build_gateway(repository),
            run_path / "profile",
            storage,
            origin,
            disk_pressure=False,
            bearer=False,
        )
        vite = build_and_start_production_web(
            repository, origin, vite_port, address
        )
        milestones = run_playwright(
            repository, origin, address, fixture, tracker.port
        )
        tracker.join()
        verify_files(fixture, storage)
        stop_process(vite, "Vite")
        vite = None
        terminate_gateway(gateway)
        gateway = None
        print(
            f"{milestones} info_hash={fixture.info_hash} "
            f"metadata_size={len(fixture.info_bytes)} "
            f"pieces={fixture.torrent_info.num_pieces()} files={len(FILES)} "
            f"payload_sha1={fixture.payload_sha1} "
            f"elapsed_seconds={time.monotonic() - started:.3f} "
            "gateway_shutdown=joined cleanup=ok"
        )
    except BaseException as error:
        failure = error
        raise
    finally:
        if vite is not None:
            try:
                stop_process(vite, "Vite")
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"Vite cleanup failed: {cleanup_error}", file=sys.stderr)
        if gateway is not None:
            try:
                gateway_diagnostics = terminate_gateway(gateway)
                if failure is not None:
                    print(
                        "gateway diagnostics after failure:\n"
                        + "\n".join(gateway_diagnostics.splitlines()[-80:]),
                        file=sys.stderr,
                    )
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"gateway cleanup failed: {cleanup_error}", file=sys.stderr)
        if tracker is not None:
            if failure is not None:
                print(
                    f"tracker requests={tracker.requests} failure={tracker.failure}",
                    file=sys.stderr,
                )
            tracker.close()
        if session is not None:
            if handle is not None:
                try:
                    session.remove_torrent(handle)
                except RuntimeError:
                    pass
            session.pause()
            session.pop_alerts()
        handle = None
        session = None
        gc.collect()
        shutil.rmtree(run_path, ignore_errors=True)


def main() -> int:
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    try:
        run()
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
