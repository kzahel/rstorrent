#!/usr/bin/env python3
"""Drive empty Add through a real chooser and WebSocket byte intake."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import libtorrent as lt

from application_surface_harness import (
    TOKEN,
    build_gateway,
    connection_metrics,
    start_gateway,
    stop_gateway,
)
from browser_peer_inspection_surface import build_and_start_production_web
from browser_surface_harness import reserve_loopback_port, stop_process
from first_verified_piece import ScenarioFailure
from bep52_metainfo_oracle import SourceFile
from first_verified_piece import add_seed, create_session, wait_for_listener
from http_tracker_application import ControlledHttpTracker
from pure_v2_runtime import (
    PlaintextBep52Proxy,
    RuntimeFixture,
    deterministic_bytes,
    make_fixture,
)


@dataclass(frozen=True)
class TorrentCase:
    format: str
    path: Path
    name: str
    info_hash: str
    complete: bool = False
    skip_name: str | None = None
    wanted_name: str | None = None


def create_torrent_files(root: Path) -> tuple[TorrentCase, RuntimeFixture]:
    payload = b"bounded browser picker fixture"
    piece_hash = hashlib.sha1(payload).digest()
    v1_name = "picker-fixture.bin"
    name = v1_name.encode()
    info = (
        f"d6:lengthi{len(payload)}e4:name{len(name)}:".encode()
        + name
        + f"12:piece lengthi16384e6:pieces{len(piece_hash)}:".encode()
        + piece_hash
        + b"e"
    )
    comment = b"independently generated for picker evidence"
    source = (
        f"d7:comment{len(comment)}:".encode()
        + comment
        + b"4:info"
        + info
        + b"e"
    )
    torrent = root / "picker-fixture.torrent"
    torrent.write_bytes(source)
    v1 = TorrentCase(
        format="v1",
        path=torrent,
        name=v1_name,
        info_hash=hashlib.sha1(info).hexdigest(),
    )
    fixture = make_fixture(
        root,
        "browser-pure-v2",
        (
            SourceFile((b"nested", b"final.bin"), deterministic_bytes(61, 17)),
            SourceFile((b"payload.bin",), deterministic_bytes(59, 40_000)),
            SourceFile((b"skip.bin",), deterministic_bytes(53, 9)),
        ),
        32 * 1024,
    )
    return v1, fixture


def torrent_case_with_tracker(fixture: RuntimeFixture, tracker_url: str) -> TorrentCase:
    metainfo = lt.bdecode(fixture.torrent_path.read_bytes())
    metainfo[b"announce"] = tracker_url.encode("utf-8")
    fixture.torrent_path.write_bytes(bytes(lt.bencode(metainfo)))
    identity = lt.torrent_info(str(fixture.torrent_path)).info_hashes()
    if identity.has_v1() or str(identity.v2) != fixture.full_info_hash:
        raise ScenarioFailure("browser tracker source changed pure-v2 identity")
    return TorrentCase(
        format="v2",
        path=fixture.torrent_path,
        name="root",
        info_hash=fixture.full_info_hash,
        complete=True,
        skip_name="skip.bin",
        wanted_name="payload.bin",
    )


def run_playwright(
    repository: Path,
    origin: str,
    gateway_address: str,
    case: TorrentCase,
    *,
    restart: bool = False,
    storage: Path | None = None,
) -> str:
    environment = os.environ.copy()
    environment.pop("FORCE_COLOR", None)
    environment.update(
        {
            "NO_COLOR": "1",
            "RSTORRENT_PLAYWRIGHT_BASE_URL": origin,
            "RSTORRENT_LIVE_GATEWAY_URL": f"http://{gateway_address}",
            "RSTORRENT_LIVE_GATEWAY_TOKEN": TOKEN,
            "RSTORRENT_LIVE_TORRENT_NAME": case.name,
        }
    )
    if restart:
        environment["RSTORRENT_LIVE_TORRENT_FILE_RESTART"] = "1"
    else:
        environment.update(
            {
                "RSTORRENT_LIVE_TORRENT_FILE_PICKER": "1",
                "RSTORRENT_LIVE_TORRENT_FILE": str(case.path),
            }
        )
    if case.complete:
        environment.update(
            {
                "RSTORRENT_LIVE_TORRENT_FILE_COMPLETE": "1",
                "RSTORRENT_LIVE_TORRENT_FILE_SKIP_NAME": case.skip_name or "",
                "RSTORRENT_LIVE_TORRENT_FILE_WANTED_NAME": case.wanted_name or "",
            }
        )
    if storage is not None:
        environment["RSTORRENT_LIVE_STORAGE_PATH"] = str(storage)
    completed = subprocess.run(
        [
            "npm",
            "run",
            "test:e2e",
            "--prefix",
            "clients/web",
            "--",
            "--grep",
            "live torrent file picker",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        env=environment,
        timeout=90,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "live torrent file picker failed\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    milestone = next(
        (
            line.strip()
            for line in completed.stdout.splitlines()
            if line.startswith("torrent_file_picker_live_milestones ")
        ),
        None,
    )
    if milestone is None:
        raise ScenarioFailure("Playwright omitted torrent picker milestones")
    return milestone


def run_v2_magnet_playwright(
    repository: Path,
    origin: str,
    gateway_address: str,
    torrent_name: str,
    magnet: str,
    phase: str,
    *,
    skip_name: str = "skip.bin",
    second_magnet: str | None = None,
    v1_info_hash: str | None = None,
    v2_info_hash: str | None = None,
    file_count: int = 4,
) -> str:
    environment = os.environ.copy()
    environment.pop("FORCE_COLOR", None)
    environment.update(
        {
            "NO_COLOR": "1",
            "RSTORRENT_PLAYWRIGHT_BASE_URL": origin,
            "RSTORRENT_LIVE_GATEWAY_URL": f"http://{gateway_address}",
            "RSTORRENT_LIVE_GATEWAY_TOKEN": TOKEN,
            "RSTORRENT_LIVE_TORRENT_NAME": torrent_name,
            "RSTORRENT_LIVE_V2_MAGNET_PHASE": phase,
            "RSTORRENT_LIVE_V2_MAGNET_SKIP_NAME": skip_name,
            "RSTORRENT_LIVE_V2_MAGNET_FILE_COUNT": str(file_count),
        }
    )
    if second_magnet is not None:
        environment["RSTORRENT_LIVE_SECOND_MAGNET"] = second_magnet
    if v1_info_hash is not None:
        environment["RSTORRENT_LIVE_V1_INFO_HASH"] = v1_info_hash
    if v2_info_hash is not None:
        environment["RSTORRENT_LIVE_V2_INFO_HASH"] = v2_info_hash
    if phase == "add":
        environment["RSTORRENT_LIVE_MAGNET"] = magnet
    completed = subprocess.run(
        [
            "npm",
            "run",
            "test:e2e",
            "--prefix",
            "clients/web",
            "--",
            "--grep",
            "live v2 magnet lifecycle",
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
            f"live v2 magnet {phase} failed\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    milestone = next(
        (
            line.strip()
            for line in completed.stdout.splitlines()
            if line.startswith("v2_magnet_live_milestone ")
        ),
        None,
    )
    if milestone is None:
        raise ScenarioFailure("Playwright omitted v2 magnet lifecycle milestone")
    return milestone


def verify_metrics(
    metrics: dict[str, object], expected_connections: int, expected_uploads: int
) -> None:
    if metrics.get("accepted_connections") != expected_connections:
        raise ScenarioFailure(
            f"gateway did not record {expected_connections} browser connections"
        )
    if metrics.get("active_connections") != 0:
        raise ScenarioFailure("gateway retained a browser connection after shutdown")
    client_frames = metrics.get("client_frames")
    server_frames = metrics.get("server_frames")
    if not isinstance(client_frames, dict) or not isinstance(server_frames, dict):
        raise ScenarioFailure("gateway omitted connection frame metrics")
    upload_begin = client_frames.get("begin_torrent_upload")
    upload_ready = server_frames.get("torrent_upload_ready")
    if (
        not isinstance(upload_begin, dict)
        or upload_begin.get("messages") != expected_uploads
    ):
        raise ScenarioFailure("gateway recorded the wrong upload declaration count")
    if (
        not isinstance(upload_ready, dict)
        or upload_ready.get("messages") != expected_uploads
    ):
        raise ScenarioFailure("gateway recorded the wrong upload admission count")


def run() -> None:
    repository = Path(__file__).resolve().parents[2]
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-browser-torrent-file-"))
    gateway: subprocess.Popen[str] | None = None
    vite: subprocess.Popen[str] | None = None
    session: Any | None = None
    handle: Any | None = None
    tracker: ControlledHttpTracker | None = None
    hash_proxy: PlaintextBep52Proxy | None = None
    failure: BaseException | None = None
    try:
        v1, v2_fixture = create_torrent_files(run_path)
        diagnostics: list[str] = []
        session = create_session()
        session.apply_settings(
            {
                "enable_incoming_utp": True,
                "enable_outgoing_utp": True,
            }
        )
        seed_port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            v2_fixture.torrent_info,
            v2_fixture.libtorrent_storage_root,
            diagnostics,
        )
        tracker = ControlledHttpTracker(v2_fixture.wire_info_hash, seed_port)
        tracker.start()
        v2 = torrent_case_with_tracker(v2_fixture, tracker.url)
        cases = (v1, v2)
        vite_port = reserve_loopback_port()
        origin = f"http://127.0.0.1:{vite_port}"
        gateway, address = start_gateway(
            build_gateway(repository),
            run_path / "profile",
            run_path / "downloads",
            origin,
            "loopback_only",
        )
        vite = build_and_start_production_web(
            repository, origin, vite_port, address
        )
        milestones = [
            run_playwright(repository, origin, address, case, storage=run_path / "downloads")
            for case in cases
        ]
        tracker.wait_for_event("started")
        output = run_path / "downloads" / "root"
        if (output / "skip.bin").exists():
            raise ScenarioFailure("browser pure-v2 selection published skipped file")
        sources = {
            "/".join(component.decode("utf-8") for component in source.path): source
            for source in v2_fixture.files
        }
        for relative in ("payload.bin", "nested/final.bin"):
            source = sources[relative]
            path = output / relative
            if not path.is_file() or path.read_bytes() != source.data:
                raise ScenarioFailure(f"browser pure-v2 output differs: {relative}")
        if any(path.name.endswith(".rstorrent-parts") for path in (run_path / "downloads").iterdir()):
            raise ScenarioFailure("browser pure-v2 selection created a part artifact")
        stop_process(vite, "Vite")
        vite = None
        diagnostics = stop_gateway(gateway)
        gateway = None
        metrics = connection_metrics(diagnostics)
        verify_metrics(metrics, len(cases), len(cases))

        gateway, address = start_gateway(
            build_gateway(repository),
            run_path / "profile",
            run_path / "downloads",
            origin,
            "loopback_only",
        )
        vite = build_and_start_production_web(repository, origin, vite_port, address)
        restart_milestone = run_playwright(
            repository,
            origin,
            address,
            v2,
            restart=True,
            storage=run_path / "downloads",
        )
        milestones.append(restart_milestone)
        stop_process(vite, "Vite")
        vite = None
        restart_diagnostics = stop_gateway(gateway)
        gateway = None
        restart_metrics = connection_metrics(restart_diagnostics)
        verify_metrics(restart_metrics, 1, 0)

        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = create_session()
        magnet_seed_diagnostics: list[str] = []
        seed_port = wait_for_listener(session, magnet_seed_diagnostics)
        handle = add_seed(
            session,
            v2_fixture.torrent_info,
            v2_fixture.libtorrent_storage_root,
            magnet_seed_diagnostics,
        )
        hash_proxy = PlaintextBep52Proxy(("127.0.0.1", seed_port))
        magnet_profile = run_path / "magnet-profile"
        magnet_downloads = run_path / "magnet-downloads"
        magnet = (
            "magnet:?xt=urn:btmh:1220"
            f"{v2_fixture.full_info_hash}"
            f"&x.pe={hash_proxy.endpoint[0]}:{hash_proxy.endpoint[1]}&so=0-1"
        )
        gateway, address = start_gateway(
            build_gateway(repository),
            magnet_profile,
            magnet_downloads,
            origin,
            "loopback_only",
        )
        vite = build_and_start_production_web(repository, origin, vite_port, address)
        milestones.append(
            run_v2_magnet_playwright(
                repository,
                origin,
                address,
                v2.name,
                magnet,
                "add",
            )
        )
        magnet_output = magnet_downloads / "root"
        if (magnet_output / "skip.bin").exists():
            raise ScenarioFailure("browser v2 magnet published skipped payload")
        for relative in ("payload.bin", "nested/final.bin"):
            source = sources[relative]
            path = magnet_output / relative
            if not path.is_file() or path.read_bytes() != source.data:
                raise ScenarioFailure(f"browser v2 magnet output differs: {relative}")
        before_magnet_restart = hash_proxy.snapshot()
        if (
            not before_magnet_restart["hash_requests"]
            or before_magnet_restart["hash_responses"] < 1
            or any(
                piece == 3
                for piece, _, _ in before_magnet_restart["piece_messages"]
            )
        ):
            raise ScenarioFailure(
                "browser v2 magnet wire evidence is incomplete: "
                f"{before_magnet_restart}"
            )
        stop_process(vite, "Vite")
        vite = None
        magnet_diagnostics = stop_gateway(gateway)
        gateway = None
        magnet_metrics = connection_metrics(magnet_diagnostics)
        verify_metrics(magnet_metrics, 1, 0)

        gateway, address = start_gateway(
            build_gateway(repository),
            magnet_profile,
            magnet_downloads,
            origin,
            "loopback_only",
        )
        vite = build_and_start_production_web(repository, origin, vite_port, address)
        milestones.append(
            run_v2_magnet_playwright(
                repository,
                origin,
                address,
                v2.name,
                magnet,
                "restart_remove",
            )
        )
        after_magnet_restart = hash_proxy.snapshot()
        if after_magnet_restart != before_magnet_restart:
            raise ScenarioFailure(
                "complete browser v2 magnet restart used the peer instead of local "
                f"tree reconstruction: before={before_magnet_restart} "
                f"after={after_magnet_restart}"
            )
        if magnet_output.exists():
            raise ScenarioFailure("browser v2 magnet removal retained managed payload")
        stop_process(vite, "Vite")
        vite = None
        magnet_restart_diagnostics = stop_gateway(gateway)
        gateway = None
        magnet_restart_metrics = connection_metrics(magnet_restart_diagnostics)
        verify_metrics(magnet_restart_metrics, 1, 0)
        print(
            f"{' '.join(milestones)} "
            f"formats={','.join(case.format for case in cases)} "
            f"info_hashes={','.join(case.info_hash for case in cases)} "
            f"source_bytes={sum(case.path.stat().st_size for case in cases)} "
            "v1_start_content=false v2_completion=complete_rechecked "
            "selection=file_0_skipped_without_part restart=complete "
            "v2_magnet_selection=files_0_1 v2_magnet_restart=local_reconstruction "
            "v2_magnet_export=canonical v2_magnet_removal=exact "
            "gateway_shutdown=joined "
            f"connection_metrics={json.dumps(metrics, sort_keys=True, separators=(',', ':'))} "
            f"restart_connection_metrics={json.dumps(restart_metrics, sort_keys=True, separators=(',', ':'))}"
            f" magnet_connection_metrics={json.dumps(magnet_metrics, sort_keys=True, separators=(',', ':'))}"
            f" magnet_restart_connection_metrics={json.dumps(magnet_restart_metrics, sort_keys=True, separators=(',', ':'))}"
        )
    except BaseException as error:
        failure = error
        if tracker is not None:
            print(
                "browser_tracker_debug "
                + json.dumps(
                    {
                        "events": tracker.events,
                        "requests": tracker.requests,
                    },
                    sort_keys=True,
                ),
                file=sys.stderr,
            )
        if hash_proxy is not None:
            print(
                "browser_v2_magnet_wire_debug "
                + json.dumps(hash_proxy.snapshot(), sort_keys=True),
                file=sys.stderr,
            )
        if handle is not None and handle.is_valid():
            status = handle.status()
            print(
                "browser_seed_debug "
                + json.dumps(
                    {
                        "download_rate": status.download_rate,
                        "is_seeding": status.is_seeding,
                        "num_peers": status.num_peers,
                        "upload_rate": status.upload_rate,
                        "uploaded": status.total_upload,
                        "peers": [
                            {
                                "client": str(peer.client),
                                "endpoint": str(peer.ip),
                                "flags": int(peer.flags),
                            }
                            for peer in handle.get_peer_info()
                        ],
                        "alerts": [
                            alert.message() for alert in session.pop_alerts()
                        ]
                        if session is not None
                        else [],
                    },
                    sort_keys=True,
                ),
                file=sys.stderr,
            )
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
                stop_gateway(gateway)
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"gateway cleanup failed: {cleanup_error}", file=sys.stderr)
        if tracker is not None:
            try:
                tracker.close()
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"tracker cleanup failed: {cleanup_error}", file=sys.stderr)
        if hash_proxy is not None:
            try:
                hash_proxy.close()
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"v2 magnet proxy cleanup failed: {cleanup_error}", file=sys.stderr)
        if handle is not None and session is not None:
            try:
                if handle.is_valid():
                    session.remove_torrent(handle)
            except Exception:
                pass
        if session is not None:
            session.pause()
        shutil.rmtree(run_path, ignore_errors=True)


def main() -> int:
    try:
        run()
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(f"browser torrent file intake failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
