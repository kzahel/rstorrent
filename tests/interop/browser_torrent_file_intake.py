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
from bep52_metainfo_oracle import SourceFile, pure_v2_fixture


@dataclass(frozen=True)
class TorrentCase:
    format: str
    path: Path
    name: str
    info_hash: str


def create_torrent_files(root: Path) -> tuple[TorrentCase, TorrentCase]:
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
    fixture = pure_v2_fixture(
        "browser-pure-v2",
        [SourceFile((b"payload.bin",), b"bounded pure-v2 browser picker fixture")],
        16 * 1024,
    )
    v2_path = root / "pure-v2-picker-fixture.torrent"
    v2_path.write_bytes(fixture.torrent)
    v2 = TorrentCase(
        format="v2",
        path=v2_path,
        name="root",
        info_hash=fixture.expected["v2_info_hash"],
    )
    return v1, v2


def run_playwright(
    repository: Path,
    origin: str,
    gateway_address: str,
    case: TorrentCase,
) -> str:
    environment = os.environ.copy()
    environment.pop("FORCE_COLOR", None)
    environment.update(
        {
            "NO_COLOR": "1",
            "RSTORRENT_PLAYWRIGHT_BASE_URL": origin,
            "RSTORRENT_LIVE_GATEWAY_URL": f"http://{gateway_address}",
            "RSTORRENT_LIVE_GATEWAY_TOKEN": TOKEN,
            "RSTORRENT_LIVE_TORRENT_FILE_PICKER": "1",
            "RSTORRENT_LIVE_TORRENT_FILE": str(case.path),
            "RSTORRENT_LIVE_TORRENT_NAME": case.name,
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


def verify_metrics(metrics: dict[str, object], expected_connections: int) -> None:
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
        or upload_begin.get("messages") != expected_connections
    ):
        raise ScenarioFailure("gateway recorded the wrong upload declaration count")
    if (
        not isinstance(upload_ready, dict)
        or upload_ready.get("messages") != expected_connections
    ):
        raise ScenarioFailure("gateway recorded the wrong upload admission count")


def run() -> None:
    repository = Path(__file__).resolve().parents[2]
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-browser-torrent-file-"))
    gateway: subprocess.Popen[str] | None = None
    vite: subprocess.Popen[str] | None = None
    failure: BaseException | None = None
    try:
        cases = create_torrent_files(run_path)
        vite_port = reserve_loopback_port()
        origin = f"http://127.0.0.1:{vite_port}"
        gateway, address = start_gateway(
            build_gateway(repository),
            run_path / "profile",
            run_path / "downloads",
            origin,
            "offline",
        )
        vite = build_and_start_production_web(
            repository, origin, vite_port, address
        )
        milestones = [
            run_playwright(repository, origin, address, case)
            for case in cases
        ]
        stop_process(vite, "Vite")
        vite = None
        diagnostics = stop_gateway(gateway)
        gateway = None
        metrics = connection_metrics(diagnostics)
        verify_metrics(metrics, len(cases))
        storage_entries = list((run_path / "downloads").iterdir())
        if storage_entries:
            raise ScenarioFailure(
                f"metadata-only picker created payload artifacts: {storage_entries}"
            )
        print(
            f"{' '.join(milestones)} "
            f"formats={','.join(case.format for case in cases)} "
            f"info_hashes={','.join(case.info_hash for case in cases)} "
            f"source_bytes={sum(case.path.stat().st_size for case in cases)} "
            "start_content=false payload_artifacts=0 gateway_shutdown=joined "
            f"connection_metrics={json.dumps(metrics, sort_keys=True, separators=(',', ':'))}"
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
                stop_gateway(gateway)
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"gateway cleanup failed: {cleanup_error}", file=sys.stderr)
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
