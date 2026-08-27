#!/usr/bin/env python3
"""Measure production browser application bytes across selected live views."""

from __future__ import annotations

import argparse
import gc
import json
import os
import platform
import signal
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request
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
from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    create_session,
    wait_for_listener,
)
from magnet_metadata import create_fixture, magnet_uri
from performance_profiles import collect_hardware_environment


MIB = 1024 * 1024
OWNER = "0123456789abcdef0123456789abcdef"
ACTIVE_ROOT_NAME = "bandwidth-active"


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library-rows", type=int, default=12)
    parser.add_argument("--payload-mib", type=int, default=64)
    parser.add_argument("--piece-size-kib", type=int, default=256)
    parser.add_argument("--source-rate-kib", type=int, default=256)
    parser.add_argument("--window-seconds", type=float, default=8.0)
    parser.add_argument("--timeout-seconds", type=int, default=240)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    if not 1 <= arguments.library_rows <= 32:
        parser.error("--library-rows must be between 1 and 32")
    if not 8 <= arguments.payload_mib <= 256:
        parser.error("--payload-mib must be between 8 and 256")
    if (
        arguments.piece_size_kib < 16
        or arguments.piece_size_kib > 16 * 1024
        or arguments.piece_size_kib & (arguments.piece_size_kib - 1)
    ):
        parser.error("--piece-size-kib must be a power of two from 16 to 16384")
    if not 16 <= arguments.source_rate_kib <= 16 * 1024:
        parser.error("--source-rate-kib must be between 16 and 16384")
    if not 1.0 <= arguments.window_seconds <= 60.0:
        parser.error("--window-seconds must be between 1 and 60")
    if not 60 <= arguments.timeout_seconds <= 900:
        parser.error("--timeout-seconds must be between 60 and 900")
    return arguments


def command_text(command: list[str], repository: Path) -> str | None:
    completed = subprocess.run(
        command,
        cwd=repository,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=10,
        check=False,
    )
    return completed.stdout.strip() if completed.returncode == 0 else None


def upload_stopped_torrent(
    gateway_address: str,
    origin: str,
    request_id: str,
    source: bytes,
) -> None:
    query = urllib.parse.urlencode(
        {
            "request_id": request_id,
            "storage_root": "downloads",
            "start_content": "false",
            "selection": "all",
        }
    )
    request = urllib.request.Request(
        f"http://{gateway_address}/api/v1/torrents?{query}",
        data=source,
        headers={
            "Authorization": f"Bearer {TOKEN}",
            "Content-Type": "application/x-bittorrent",
            "Origin": origin,
            "X-RSTorrent-Owner": OWNER,
        },
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        result = json.loads(response.read())
    if result.get("status") != "success":
        raise ScenarioFailure(f"stopped torrent preload failed: {result}")


def run_browser(
    repository: Path,
    origin: str,
    gateway_address: str,
    magnet: str,
    file_count: int,
    library_rows: int,
    window_millis: int,
    timeout_seconds: int,
    gateway_process: subprocess.Popen[str],
) -> dict[str, Any]:
    environment = os.environ.copy()
    environment.pop("FORCE_COLOR", None)
    environment.update(
        {
            "NO_COLOR": "1",
            "RSTORRENT_PLAYWRIGHT_BASE_URL": origin,
            "RSTORRENT_LIVE_GATEWAY_URL": f"http://{gateway_address}",
            "RSTORRENT_LIVE_GATEWAY_TOKEN": TOKEN,
            "RSTORRENT_LIVE_MAGNET": magnet,
            "RSTORRENT_LIVE_FILE_COUNT": str(file_count),
            "RSTORRENT_LIVE_BANDWIDTH_BASELINE": "1",
            "RSTORRENT_BANDWIDTH_ACTIVE_NAME": ACTIVE_ROOT_NAME,
            "RSTORRENT_BANDWIDTH_LIBRARY_ROWS": str(library_rows),
            "RSTORRENT_BANDWIDTH_WINDOW_MILLIS": str(window_millis),
        }
    )
    process = subprocess.Popen(
        [
            "npm",
            "run",
            "test:e2e",
            "--prefix",
            "clients/web",
            "--",
            "bandwidth-baseline.spec.ts",
        ],
        cwd=repository,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=environment,
        start_new_session=True,
    )
    deadline = time.monotonic() + timeout_seconds
    while process.poll() is None:
        if gateway_process.poll() is not None:
            stop_process(process, "Playwright UI bandwidth baseline")
            gateway_stdout, gateway_stderr = gateway_process.communicate()
            raise ScenarioFailure(
                f"gateway exited with {gateway_process.returncode} during baseline\n"
                f"stdout:\n{gateway_stdout}\nstderr:\n{gateway_stderr}"
            )
        if time.monotonic() >= deadline:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                stdout, stderr = process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                stdout, stderr = process.communicate(timeout=5)
            raise ScenarioFailure(
                "browser UI bandwidth baseline timed out\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}"
            )
        time.sleep(0.1)
    stdout, stderr = process.communicate()
    if process.returncode != 0:
        raise ScenarioFailure(
            "browser UI bandwidth baseline failed\n"
            f"stdout:\n{stdout}\nstderr:\n{stderr}"
        )
    marker = "bandwidth_baseline_result "
    matches = [
        json.loads(line.split(marker, maxsplit=1)[1])
        for line in stdout.splitlines()
        if marker in line
    ]
    if len(matches) != 1 or not isinstance(matches[0], dict):
        raise ScenarioFailure(
            f"browser emitted {len(matches)} bandwidth baseline results"
        )
    return matches[0]


def frame_metric_totals(metrics: dict[str, Any], direction: str) -> tuple[int, int]:
    families = metrics.get(direction)
    if not isinstance(families, dict):
        raise ScenarioFailure(f"gateway metrics omit {direction}")
    messages = 0
    payload_bytes = 0
    for family, value in families.items():
        if not isinstance(family, str) or not isinstance(value, dict):
            raise ScenarioFailure(f"gateway {direction} contains an invalid family")
        family_messages = value.get("messages")
        family_bytes = value.get("bytes")
        if not isinstance(family_messages, int) or not isinstance(family_bytes, int):
            raise ScenarioFailure(f"gateway {direction}.{family} is invalid")
        messages += family_messages
        payload_bytes += family_bytes
    return messages, payload_bytes


def validate_cross_check(browser: dict[str, Any], gateway: dict[str, Any]) -> None:
    if browser.get("schemaVersion") != 1 or browser.get("applicationUpgrades") != 1:
        raise ScenarioFailure("browser did not report one versioned application connection")
    if browser.get("semanticHttpRequests") != []:
        raise ScenarioFailure("browser baseline used the semantic HTTP adapter")
    total = browser.get("total")
    if not isinstance(total, dict):
        raise ScenarioFailure("browser baseline omits total frame metrics")
    pairs = (
        ("client_frames", "client_to_server"),
        ("server_frames", "server_to_client"),
    )
    for gateway_direction, browser_direction in pairs:
        expected_messages, expected_bytes = frame_metric_totals(
            gateway, gateway_direction
        )
        observed = total.get(browser_direction)
        if not isinstance(observed, dict):
            raise ScenarioFailure(f"browser baseline omits {browser_direction}")
        if observed.get("messages") != expected_messages:
            raise ScenarioFailure(
                f"{browser_direction} message cross-check differs: "
                f"browser={observed.get('messages')} gateway={expected_messages}"
            )
        if observed.get("payload_bytes") != expected_bytes:
            raise ScenarioFailure(
                f"{browser_direction} byte cross-check differs: "
                f"browser={observed.get('payload_bytes')} gateway={expected_bytes}"
            )
    if gateway.get("accepted_connections") != 1:
        raise ScenarioFailure("gateway did not accept exactly one application connection")
    if gateway.get("active_connections") != 0:
        raise ScenarioFailure("application connection remained active after browser exit")
    if gateway.get("heartbeat_timeouts") != 0:
        raise ScenarioFailure("application connection encountered a heartbeat timeout")


def run(arguments: argparse.Namespace) -> dict[str, Any]:
    repository = Path(__file__).resolve().parents[2]
    payload_bytes = arguments.payload_mib * MIB
    required_free = payload_bytes * 3 + 1024 * MIB
    available = shutil.disk_usage(tempfile.gettempdir()).free
    if available < required_free:
        raise ScenarioFailure(
            f"insufficient temporary disk: need {required_free}, have {available}"
        )
    gateway_binary = build_gateway(repository)
    started = time.monotonic()
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    gateway: subprocess.Popen[str] | None = None
    vite: subprocess.Popen[str] | None = None
    diagnostics: list[str] = []
    browser: dict[str, Any] | None = None
    gateway_observation: dict[str, Any] | None = None
    with tempfile.TemporaryDirectory(
        prefix="rstorrent-application-ui-bandwidth-"
    ) as temporary:
        owned_root = Path(temporary)
        try:
            active_fixture = create_fixture(
                owned_root / "active",
                payload_size=payload_bytes,
                piece_size=arguments.piece_size_kib * 1024,
                root_name=ACTIVE_ROOT_NAME,
            )
            session = create_session()
            source_rate = arguments.source_rate_kib * 1024
            session.apply_settings({"upload_rate_limit": source_rate})
            peer_port = wait_for_listener(session, diagnostics)
            handle = add_seed(
                session,
                active_fixture.torrent_info,
                active_fixture.seed_directory,
                diagnostics,
            )
            handle.set_upload_limit(source_rate)

            vite_port = reserve_loopback_port()
            origin = f"http://127.0.0.1:{vite_port}"
            gateway, address = start_gateway(
                gateway_binary,
                owned_root / "profile",
                owned_root / "downloads",
                origin,
            )
            for index in range(arguments.library_rows - 1):
                fixture = create_fixture(
                    owned_root / f"stopped-{index:02}",
                    payload_size=16 * 1024,
                    piece_size=16 * 1024,
                    root_name=f"bandwidth-stopped-{index:02}",
                )
                upload_stopped_torrent(
                    address,
                    origin,
                    f"baseline-stopped-{index:02}",
                    fixture.torrent_path.read_bytes(),
                )
            vite = build_and_start_production_web(
                repository,
                origin,
                vite_port,
                address,
            )
            browser = run_browser(
                repository,
                origin,
                address,
                magnet_uri(
                    active_fixture.info_hash,
                    f"127.0.0.1:{peer_port}",
                ),
                len(active_fixture.files),
                arguments.library_rows - 1,
                round(arguments.window_seconds * 1_000),
                arguments.timeout_seconds,
                gateway,
            )
            stop_process(vite, "production web preview")
            vite = None
            gateway_stderr = stop_gateway(gateway)
            gateway = None
            gateway_observation = connection_metrics(gateway_stderr)
            validate_cross_check(browser, gateway_observation)
        finally:
            if vite is not None:
                stop_process(vite, "production web preview")
            if gateway is not None:
                stop_gateway(gateway)
            if session is not None:
                if handle is not None and handle.is_valid():
                    session.remove_torrent(handle)
                session.pause()
                diagnostics.extend(alert.message() for alert in session.pop_alerts())
            handle = None
            session = None
            gc.collect()

    if browser is None or gateway_observation is None:
        raise ScenarioFailure("bandwidth baseline ended without observations")
    return {
        "schema_version": 1,
        "scenario": "production-websocket-ui-bandwidth",
        "status": "passed",
        "environment": {
            **collect_hardware_environment(Path(tempfile.gettempdir())),
            "repository_commit": command_text(["git", "rev-parse", "HEAD"], repository),
            "repository_dirty": bool(
                command_text(["git", "status", "--porcelain"], repository)
            ),
            "platform": platform.platform(),
            "python": platform.python_version(),
            "libtorrent": lt.version,
            "browser": "Playwright Chrome",
        },
        "config": {
            "library_rows_before_active": arguments.library_rows - 1,
            "library_rows_during_active": arguments.library_rows,
            "payload_mib": arguments.payload_mib,
            "piece_size_kib": arguments.piece_size_kib,
            "source_rate_kib": arguments.source_rate_kib,
            "window_seconds": arguments.window_seconds,
            "transport": "websocket",
            "encoding": "json",
        },
        "elapsed_seconds": time.monotonic() - started,
        "browser": browser,
        "gateway_connection_metrics": gateway_observation,
        "cleanup_succeeded": True,
    }


def main() -> int:
    arguments = parse_arguments()
    try:
        report = run(arguments)
    except (
        OSError,
        ScenarioFailure,
        subprocess.SubprocessError,
        json.JSONDecodeError,
    ) as error:
        print(f"application UI bandwidth baseline failed: {error}", file=sys.stderr)
        return 1
    rendered = json.dumps(report, indent=2, sort_keys=True)
    if arguments.output is not None:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(f"{rendered}\n", encoding="utf-8")
    print(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
