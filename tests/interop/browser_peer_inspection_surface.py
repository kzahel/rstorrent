#!/usr/bin/env python3
"""Drive the live React peer table against a controlled libtorrent seed."""

from __future__ import annotations

import argparse
import gc
import os
import selectors
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

import libtorrent as lt

from browser_surface_harness import reserve_loopback_port, stop_process
from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    compare_payloads,
    create_session,
    wait_for_listener,
)
from application_surface_harness import TOKEN, build_gateway, verify_payload
from magnet_metadata import ROOT_NAME, create_fixture, magnet_uri
from udp_tracker_magnet import OneShotUdpTracker, tracker_magnet


BROWSER_PREFIX_SIZE = 7_000
BROWSER_PREFIX_PATH = Path("nested/prefix.bin")


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--screenshot-dir",
        type=Path,
        help="retain redacted loopback-only wide, compact, and phone PNGs",
    )
    parser.add_argument(
        "--disk-pressure",
        action="store_true",
        help="run the bounded slow-storage Disk view proof",
    )
    parser.add_argument(
        "--piece-map",
        action="store_true",
        help="run the selected-torrent Pieces canvas proof",
    )
    parser.add_argument(
        "--file-selection",
        action="store_true",
        help="run metadata-only add plus live Skip/Normal storage proof",
    )
    arguments = parser.parse_args()
    if sum((arguments.disk_pressure, arguments.piece_map, arguments.file_selection)) > 1:
        parser.error(
            "--disk-pressure, --piece-map, and --file-selection are mutually exclusive"
        )
    return arguments


def start_development_gateway(
    binary: Path,
    profile: Path,
    storage: Path,
    origin: str,
    *,
    disk_pressure: bool,
    bearer: bool,
    lease_millis: int = 500,
    bind: str | None = None,
) -> tuple[subprocess.Popen[str], str]:
    profile.mkdir()
    storage.mkdir()
    environment = os.environ.copy()
    environment.update(
        {
            "RSTORRENT_PROFILE_ROOT": str(profile),
            "RSTORRENT_STORAGE_ROOT": str(storage),
            "RSTORRENT_GATEWAY_ORIGIN": origin,
            "RSTORRENT_NETWORK_POLICY": "loopback_only",
        }
    )
    if bearer:
        environment.update(
            {
                "RSTORRENT_GATEWAY_AUTH": "bearer",
                "RSTORRENT_GATEWAY_BIND": bind or "127.0.0.1:0",
                "RSTORRENT_GATEWAY_TOKEN": TOKEN,
            }
        )
        environment.pop("RSTORRENT_TEST_VIEW_SET_LEASE_MILLIS", None)
    else:
        environment.update(
            {
                "RSTORRENT_GATEWAY_AUTH": "unauthenticated_loopback_development",
                "RSTORRENT_TEST_VIEW_SET_LEASE_MILLIS": str(lease_millis),
            }
        )
        if bind is None:
            environment.pop("RSTORRENT_GATEWAY_BIND", None)
        else:
            environment["RSTORRENT_GATEWAY_BIND"] = bind
        environment.pop("RSTORRENT_GATEWAY_TOKEN", None)
    if disk_pressure:
        environment["RSTORRENT_TEST_STORAGE_WRITE_DELAY_MILLIS"] = "150"
        environment["RSTORRENT_TEST_BUFFERED_PAYLOAD_BYTES"] = str(128 * 1024)
    process = subprocess.Popen(
        [str(binary)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=environment,
    )
    if process.stderr is None:
        raise ScenarioFailure("gateway stderr is unavailable")
    selector = selectors.DefaultSelector()
    selector.register(process.stderr, selectors.EVENT_READ)
    deadline = time.monotonic() + 10
    diagnostics: list[str] = []
    try:
        while time.monotonic() < deadline:
            if not selector.select(deadline - time.monotonic()):
                break
            line = process.stderr.readline()
            if not line:
                break
            diagnostics.append(line.rstrip())
            prefix = "gateway listening on "
            if line.startswith(prefix):
                return process, line[len(prefix) :].strip()
    finally:
        selector.close()
    terminate_gateway(process)
    raise ScenarioFailure(
        "development gateway did not announce its listener\n"
        + "\n".join(diagnostics)
    )


def terminate_gateway(process: subprocess.Popen[str]) -> str:
    if process.poll() is None:
        process.send_signal(signal.SIGINT)
    try:
        stdout, stderr = process.communicate(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        stdout, stderr = process.communicate(timeout=5)
        raise ScenarioFailure(
            "gateway did not join after SIGINT\n"
            f"stdout:\n{stdout}\nstderr:\n{stderr}"
        )
    if process.returncode != 0:
        raise ScenarioFailure(
            f"gateway exited with status {process.returncode}\n"
            f"stdout:\n{stdout}\nstderr:\n{stderr}"
        )
    return stderr


def run_playwright(
    repository: Path,
    origin: str,
    gateway_address: str,
    magnet: str,
    torrent_id: str,
    file_count: int,
    tracker_url: str,
    screenshot_directory: Path | None,
    *,
    disk_pressure: bool,
    piece_map: bool,
    file_selection: bool,
    storage: Path,
) -> str:
    environment = os.environ.copy()
    environment.update(
        {
            "RSTORRENT_PLAYWRIGHT_BASE_URL": origin,
            "RSTORRENT_LIVE_GATEWAY_URL": f"http://{gateway_address}",
            "RSTORRENT_LIVE_MAGNET": magnet,
            "RSTORRENT_LIVE_TORRENT_ID": torrent_id,
            "RSTORRENT_LIVE_TORRENT_NAME": ROOT_NAME,
            "RSTORRENT_LIVE_FILE_COUNT": str(file_count),
            "RSTORRENT_LIVE_TRACKER_URL": tracker_url,
        }
    )
    if screenshot_directory is not None:
        screenshot_directory.mkdir(parents=True, exist_ok=True)
        environment["RSTORRENT_SCREENSHOT_DIR"] = str(screenshot_directory)
    if disk_pressure:
        environment["RSTORRENT_LIVE_EXPECT_DISK_PRESSURE"] = "1"
    if piece_map:
        environment["RSTORRENT_LIVE_EXPECT_PIECES"] = "1"
    if file_selection:
        environment.update(
            {
                "RSTORRENT_LIVE_FILE_SELECTION": "1",
                "RSTORRENT_LIVE_GATEWAY_TOKEN": TOKEN,
                "RSTORRENT_LIVE_STORAGE_PATH": str(storage),
            }
        )
    test_name = (
        "live disk inspection"
        if disk_pressure
        else "live piece inspection"
        if piece_map
        else "live metadata-only add and file selection"
        if file_selection
        else "live peer inspection"
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
            test_name,
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
            "live peer browser test failed\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    passed = next(
        (line.strip() for line in completed.stdout.splitlines() if "passed" in line),
        "playwright=passed",
    )
    milestone_prefix = (
        "disk_live_milestones "
        if disk_pressure
        else "piece_live_milestones "
        if piece_map
        else "file_selection_live_milestones "
        if file_selection
        else "file_live_milestones "
    )
    milestones = next(
        (
            line.strip()
            for line in completed.stdout.splitlines()
            if line.startswith(milestone_prefix)
        ),
        f"{milestone_prefix.strip()}=unavailable",
    )
    return f"{passed} {milestones}"


def build_and_start_production_web(
    repository: Path,
    origin: str,
    port: int,
    gateway_address: str,
) -> subprocess.Popen[str]:
    build_environment = os.environ.copy()
    build_environment["VITE_RSTORRENT_DEFAULT_LIVE"] = "same-origin"
    built = subprocess.run(
        ["npm", "run", "build", "--prefix", "clients/web"],
        cwd=repository,
        env=build_environment,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    if built.returncode != 0:
        raise ScenarioFailure(
            "production web build failed\n"
            f"stdout:\n{built.stdout}\nstderr:\n{built.stderr}"
        )
    preview_environment = os.environ.copy()
    preview_environment["RSTORRENT_WEBUI_PROXY_TARGET"] = (
        f"http://{gateway_address}"
    )
    process = subprocess.Popen(
        [
            str(repository / "clients/web/node_modules/.bin/vite"),
            "preview",
            "--host",
            "127.0.0.1",
            "--port",
            str(port),
            "--strictPort",
        ],
        cwd=repository / "clients/web",
        env=preview_environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(origin, timeout=0.2) as response:
                if response.status == 200:
                    return process
        except OSError:
            pass
        time.sleep(0.05)
    stop_process(process, "production web preview")
    raise ScenarioFailure("production web preview did not start")


def run(
    screenshot_directory: Path | None,
    *,
    disk_pressure: bool,
    piece_map: bool,
    file_selection: bool,
) -> None:
    repository = Path(__file__).resolve().parents[2]
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-browser-peer-inspection-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    tracker: OneShotUdpTracker | None = None
    gateway: subprocess.Popen[str] | None = None
    vite: subprocess.Popen[str] | None = None
    diagnostics: list[str] = []
    failure: BaseException | None = None
    try:
        direct_peer = disk_pressure or piece_map or file_selection
        fixture = create_fixture(
            run_path,
            payload_size=4 * 1024 * 1024 if direct_peer else 40_000,
            piece_size=256 * 1024 if direct_peer else 16 * 1024,
            prefix_payload_size=BROWSER_PREFIX_SIZE,
        )
        session = create_session()
        if not direct_peer:
            session.apply_settings({"upload_rate_limit": 4 * 1024})
        elif piece_map:
            session.apply_settings({"upload_rate_limit": 256 * 1024})
        port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            fixture.torrent_info,
            fixture.seed_directory,
            diagnostics,
        )
        if not direct_peer:
            handle.set_upload_limit(4 * 1024)
        elif piece_map:
            handle.set_upload_limit(256 * 1024)
        if not direct_peer:
            tracker = OneShotUdpTracker(
                fixture.info_hash,
                port,
                response_delay_seconds=3,
                seeders=37,
                leechers=11,
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
            disk_pressure=disk_pressure,
            bearer=file_selection,
        )
        vite = build_and_start_production_web(
            repository, origin, vite_port, address
        )
        result = run_playwright(
            repository,
            origin,
            address,
            (
                magnet_uri(fixture.info_hash, f"127.0.0.1:{port}")
                if direct_peer
                else (
                    f"{tracker_magnet(fixture.info_hash, tracker.port)}"
                    f"&x.pe=127.0.0.1%3A{port}"
                )
            ),
            fixture.info_hash,
            len(fixture.files),
            "" if direct_peer else f"udp://127.0.0.1:{tracker.port}",
            screenshot_directory,
            disk_pressure=disk_pressure,
            piece_map=piece_map,
            file_selection=file_selection,
            storage=storage,
        )
        if tracker is not None:
            tracker.join()
        verify_payload(storage, ROOT_NAME, fixture.payload_hash)
        compare_payloads(
            fixture.seed_directory / ROOT_NAME / BROWSER_PREFIX_PATH,
            storage / ROOT_NAME / BROWSER_PREFIX_PATH,
        )
        stop_process(vite, "Vite")
        vite = None
        terminate_gateway(gateway)
        gateway = None
        print(
            f"{result} info_hash={fixture.info_hash} metadata_size={len(fixture.info_bytes)} "
            f"pieces={fixture.torrent_info.num_pieces()} files={len(fixture.files)} "
            f"boundary_file_bytes={BROWSER_PREFIX_SIZE} "
            f"payload_sha1={fixture.payload_hash} "
            f"responsive={'wide' if direct_peer else 'wide,compact,phone'} "
            f"tracker_requests={0 if direct_peer else 2} "
            "peer_removal=ok swarm_source_merge=ok swarm_terminal_cleanup=ok "
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
                terminate_gateway(gateway)
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"gateway cleanup failed: {cleanup_error}", file=sys.stderr)
        if tracker is not None:
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
    arguments = parse_arguments()
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    try:
        run(
            arguments.screenshot_dir.resolve()
            if arguments.screenshot_dir is not None
            else None,
            disk_pressure=arguments.disk_pressure,
            piece_map=arguments.piece_map,
            file_selection=arguments.file_selection,
        )
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
