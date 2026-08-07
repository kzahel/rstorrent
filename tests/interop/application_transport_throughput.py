#!/usr/bin/env python3
"""Compare the browser HTTP and WebSocket adapters on one controlled torrent."""

from __future__ import annotations

import argparse
import gc
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

import libtorrent as lt

from application_surface_harness import build_gateway, connection_metrics
from browser_peer_inspection_surface import (
    build_and_start_production_web,
    start_development_gateway,
    terminate_gateway,
)
from browser_surface_harness import reserve_loopback_port, stop_process
from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    create_session,
    hash_file,
    wait_for_listener,
)
from magnet_metadata import create_fixture, magnet_uri
from performance_profiles import collect_hardware_environment


MIB = 1024 * 1024

def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--size-mib", type=int, default=1024)
    parser.add_argument("--piece-size-kib", type=int, default=1024)
    parser.add_argument("--timeout-seconds", type=int, default=240)
    parser.add_argument(
        "--order",
        nargs=2,
        choices=("http", "websocket"),
        default=("http", "websocket"),
        metavar=("FIRST", "SECOND"),
    )
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    if not 1 <= arguments.size_mib <= 10 * 1024:
        parser.error("--size-mib must be between 1 and 10240")
    if arguments.piece_size_kib < 16 or arguments.piece_size_kib & (
        arguments.piece_size_kib - 1
    ):
        parser.error("--piece-size-kib must be a power of two of at least 16")
    if set(arguments.order) != {"http", "websocket"}:
        parser.error("--order must contain http and websocket exactly once")
    if not 10 <= arguments.timeout_seconds <= 900:
        parser.error("--timeout-seconds must be between 10 and 900")
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


def run_browser_case(
    repository: Path,
    origin: str,
    gateway_address: str,
    magnet: str,
    torrent_id: str,
    transport: str,
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
            "RSTORRENT_LIVE_MAGNET": magnet,
            "RSTORRENT_LIVE_TORRENT_ID": torrent_id,
            "RSTORRENT_LIVE_TRANSPORT_BENCHMARK": "1",
            "RSTORRENT_LIVE_TRANSPORT": transport,
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
            "--grep",
            "paired application transport throughput",
        ],
        cwd=repository,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=environment,
        start_new_session=True,
    )
    deadline = time.monotonic() + timeout_seconds + 60
    while process.poll() is None:
        if gateway_process.poll() is not None:
            stop_process(process, "Playwright transport case")
            gateway_stdout, gateway_stderr = gateway_process.communicate()
            raise ScenarioFailure(
                f"gateway exited with {gateway_process.returncode} during {transport}\n"
                f"stdout:\n{gateway_stdout}\nstderr:\n{gateway_stderr}"
            )
        if time.monotonic() >= deadline:
            stop_process(process, "Playwright transport case")
            raise ScenarioFailure(f"{transport} browser transport case timed out")
        time.sleep(0.1)
    stdout, stderr = process.communicate()
    if process.returncode != 0:
        raise ScenarioFailure(
            f"{transport} browser transport case failed\n"
            f"stdout:\n{stdout}\nstderr:\n{stderr}"
        )
    marker = "transport_benchmark_result "
    lines = [line for line in stdout.splitlines() if marker in line]
    if len(lines) != 1:
        raise ScenarioFailure(
            f"{transport} browser case emitted {len(lines)} result lines"
        )
    result = json.loads(lines[0].split(marker, maxsplit=1)[1])
    if result.get("transport") != transport:
        raise ScenarioFailure(f"{transport} browser case returned the wrong adapter")
    return result


def run(arguments: argparse.Namespace) -> dict[str, Any]:
    repository = Path(__file__).resolve().parents[2]
    required_free = arguments.size_mib * MIB * 3 + 2 * 1024 * MIB
    available = shutil.disk_usage(tempfile.gettempdir()).free
    if available < required_free:
        raise ScenarioFailure(
            f"insufficient temporary disk: need {required_free}, have {available}"
        )
    gateway_binary = build_gateway(repository)
    piece_size = arguments.piece_size_kib * 1024
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    vite: subprocess.Popen[str] | None = None
    gateway: subprocess.Popen[str] | None = None
    diagnostics: list[str] = []
    results: list[dict[str, Any]] = []
    started = time.monotonic()
    try:
        with tempfile.TemporaryDirectory(
            prefix="rstorrent-application-transport-throughput-"
        ) as temporary:
            owned_root = Path(temporary)
            fixture = create_fixture(
                owned_root,
                payload_size=arguments.size_mib * MIB,
                piece_size=piece_size,
            )
            info = fixture.torrent_info
            torrent_id = fixture.info_hash
            session = create_session()
            peer_port = wait_for_listener(session, diagnostics)
            handle = add_seed(session, info, fixture.seed_directory, diagnostics)
            vite_port = reserve_loopback_port()
            origin = f"http://127.0.0.1:{vite_port}"
            gateway_bind = f"127.0.0.1:{reserve_loopback_port()}"
            vite = build_and_start_production_web(
                repository, origin, vite_port, gateway_bind
            )
            for order, transport in enumerate(arguments.order, start=1):
                case_root = owned_root / f"case-{order}-{transport}"
                case_root.mkdir()
                payload_root = case_root / "payload"
                gateway, address = start_development_gateway(
                    gateway_binary,
                    case_root / "profile",
                    payload_root,
                    origin,
                    disk_pressure=False,
                    lease_millis=60_000,
                    bind=gateway_bind,
                )
                case = run_browser_case(
                    repository,
                    origin,
                    address,
                    magnet_uri(torrent_id, f"127.0.0.1:{peer_port}"),
                    torrent_id,
                    transport,
                    arguments.timeout_seconds,
                    gateway,
                )
                candidates = list((payload_root / torrent_id).rglob("payload.bin"))
                if len(candidates) != 1:
                    raise ScenarioFailure(
                        f"{transport} published {len(candidates)} payload files"
                    )
                payload = candidates[0]
                if payload.stat().st_size != arguments.size_mib * MIB:
                    raise ScenarioFailure(f"{transport} payload is absent or truncated")
                payload_sha1 = hash_file(payload)
                if payload_sha1 != fixture.payload_hash:
                    raise ScenarioFailure(f"{transport} payload hash differs from the seed")
                gateway_stderr = terminate_gateway(gateway)
                gateway = None
                metrics = connection_metrics(gateway_stderr)
                transfer_seconds = float(case["transferSeconds"])
                result = {
                    "transport": transport,
                    "order": order,
                    "transfer_seconds": transfer_seconds,
                    "throughput_mib_s": arguments.size_mib / transfer_seconds,
                    "application_upgrades": int(case["applicationUpgrades"]),
                    "semantic_http_requests": int(case["semanticHttpRequests"]),
                    "payload_sha1": payload_sha1,
                    "gateway_connection_metrics": metrics,
                    "cleanup_succeeded": True,
                }
                results.append(result)
                print(
                    f"transport_case transport={transport} order={order} "
                    f"seconds={transfer_seconds:.3f} "
                    f"throughput_mib_s={result['throughput_mib_s']:.3f} "
                    f"upgrades={result['application_upgrades']} "
                    f"semantic_http_requests={result['semantic_http_requests']} "
                    f"sha1={payload_sha1}",
                    flush=True,
                )
                shutil.rmtree(case_root)
            stop_process(vite, "Vite")
            vite = None
    finally:
        if gateway is not None:
            terminate_gateway(gateway)
        if vite is not None:
            stop_process(vite, "Vite")
        if session is not None:
            if handle is not None and handle.is_valid():
                session.remove_torrent(handle)
            session.pause()
            diagnostics.extend(alert.message() for alert in session.pop_alerts())
        handle = None
        session = None
        gc.collect()
    by_transport = {result["transport"]: result for result in results}
    return {
        "schema_version": 1,
        "scenario": "browser-application-transport-throughput",
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
            "size_mib": arguments.size_mib,
            "piece_size_kib": arguments.piece_size_kib,
            "timeout_seconds": arguments.timeout_seconds,
            "order": list(arguments.order),
            "view": "general",
            "source_cache_policy": "warm-uncontrolled-os-page-cache",
        },
        "elapsed_seconds": time.monotonic() - started,
        "results": results,
        "comparison": {
            "websocket_over_http_throughput": (
                by_transport["websocket"]["throughput_mib_s"]
                / by_transport["http"]["throughput_mib_s"]
            )
        },
    }


def main() -> int:
    arguments = parse_arguments()
    try:
        report = run(arguments)
    except (OSError, ScenarioFailure, subprocess.SubprocessError, json.JSONDecodeError) as error:
        print(f"application transport throughput failed: {error}", file=sys.stderr)
        return 1
    rendered = json.dumps(report, indent=2, sort_keys=True)
    if arguments.output is not None:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(rendered + "\n", encoding="utf-8")
    print(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
