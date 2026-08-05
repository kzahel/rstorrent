#!/usr/bin/env python3
"""Prove durable client settings, restarted seeding, and bind recovery."""

from __future__ import annotations

import gc
import json
import os
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request
from pathlib import Path
from typing import Any

import libtorrent as lt

from application_surface_harness import (
    TOKEN,
    application_metrics,
    build_gateway,
    connection_metrics,
    start_gateway,
    stop_gateway,
    verify_payload,
)
from browser_peer_inspection_surface import build_and_start_production_web
from browser_surface_harness import reserve_loopback_port, stop_process
from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    compare_payloads,
    create_session,
    wait_for_listener,
)
from incoming_seeding import create_outbound_only_session
from magnet_metadata import ROOT_NAME, Fixture, create_fixture, magnet_uri


OWNER = "0123456789abcdef0123456789abcdef"
PAYLOAD_SIZE = 2 * 1024 * 1024
PIECE_SIZE = 64 * 1024
TRANSFER_TIMEOUT_SECONDS = 30


def run_playwright(
    repository: Path,
    origin: str,
    gateway_address: str,
    phase: str,
    fixture: Fixture,
    seed_port: int,
) -> dict[str, object]:
    environment = os.environ.copy()
    environment.update(
        {
            "NO_COLOR": "1",
            "RSTORRENT_PLAYWRIGHT_BASE_URL": origin,
            "RSTORRENT_LIVE_GATEWAY_URL": f"http://{gateway_address}",
            "RSTORRENT_LIVE_GATEWAY_TOKEN": TOKEN,
            "RSTORRENT_LIVE_CLIENT_SETTINGS_PHASE": phase,
        }
    )
    if phase == "configure":
        environment.update(
            {
                "RSTORRENT_LIVE_MAGNET": magnet_uri(
                    fixture.info_hash, f"127.0.0.1:{seed_port}"
                ),
                "RSTORRENT_LIVE_TORRENT_ID": fixture.info_hash,
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
            "live client settings persist across restart and recover bind failure",
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
            f"client-settings Playwright phase {phase} failed\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    prefix = "client_settings_live_milestone "
    observations = [
        json.loads(line[len(prefix) :])
        for line in completed.stdout.splitlines()
        if line.startswith(prefix)
    ]
    if len(observations) != 1 or not isinstance(observations[0], dict):
        raise ScenarioFailure(
            f"Playwright phase {phase} did not emit exactly one milestone"
        )
    observation = observations[0]
    if observation.get("phase") != phase or observation.get("axeViolations") != 0:
        raise ScenarioFailure(f"unexpected Playwright observation: {observation}")
    return observation


def command(
    gateway_address: str, request_id: str, command_value: dict[str, Any], origin: str
) -> dict[str, Any]:
    envelope = {
        "version": 1,
        "request_id": request_id,
        "command": command_value,
    }
    request = urllib.request.Request(
        f"http://{gateway_address}/api/v1/commands",
        data=json.dumps(envelope, separators=(",", ":")).encode(),
        headers={
            "Authorization": f"Bearer {TOKEN}",
            "Content-Type": "application/json",
            "Origin": origin,
            "X-RSTorrent-Owner": OWNER,
        },
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        result = json.loads(response.read())
    if result.get("status") != "success":
        raise ScenarioFailure(f"command {request_id} failed: {result}")
    return result


def set_fixed_settings(
    gateway_address: str, origin: str, request_id: str, port: int
) -> None:
    command(
        gateway_address,
        request_id,
        {
            "type": "set_client_settings",
            "settings": {
                "listener": {"type": "fixed_loopback", "port": port},
                "peer_connection_limit": 37,
                "upload_slots": 1,
            },
        },
        origin,
    )


def leech_from_rstorrent(fixture: Fixture, listener_port: int, output: Path) -> str:
    session = create_outbound_only_session()
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    try:
        output.mkdir(parents=True)
        parameters = lt.add_torrent_params()
        parameters.ti = fixture.torrent_info
        parameters.save_path = str(output)
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        handle = session.add_torrent(parameters)
        handle.connect_peer(("127.0.0.1", listener_port))
        deadline = time.monotonic() + TRANSFER_TIMEOUT_SECONDS
        while time.monotonic() < deadline:
            diagnostics.extend(alert.message() for alert in session.pop_alerts())
            status = handle.status()
            if status.errc.value() != 0:
                raise ScenarioFailure(
                    f"libtorrent leech failed: {status.errc.message()}"
                )
            if status.is_seeding:
                payload = output / ROOT_NAME / "payload.bin"
                actual_hash = compare_payloads(fixture.payload_path, payload)
                if actual_hash != fixture.payload_hash:
                    raise ScenarioFailure("libtorrent leech produced the wrong payload hash")
                return actual_hash
            time.sleep(0.02)
        raise ScenarioFailure(
            "libtorrent did not complete from the restarted RSTorrent listener\n"
            + "\n".join(diagnostics[-30:])
        )
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def integer_field(observation: dict[str, object], field: str) -> int:
    value = observation.get(field)
    if not isinstance(value, int):
        raise ScenarioFailure(f"resource observation {field} is not an integer")
    return value


def assert_shutdown_metrics(
    gateway_metrics: dict[str, object], application: dict[str, object]
) -> None:
    if gateway_metrics.get("active_connections") != 0:
        raise ScenarioFailure("gateway retained an application connection at shutdown")
    terminal_zero = (
        "incoming_owner_after_shutdown",
        "storage_owned_after_shutdown",
        "storage_cached_after_shutdown",
        "platform_pending_after_shutdown",
    )
    for field in terminal_zero:
        value = application.get(field)
        if value not in (0, False):
            raise ScenarioFailure(f"application shutdown left {field}={value}")
    storage_limit = integer_field(application, "storage_limit")
    if integer_field(application, "storage_owned_high_water") > storage_limit:
        raise ScenarioFailure("storage ownership exceeded its configured limit")
    if integer_field(application, "storage_cached_before_shutdown") > storage_limit:
        raise ScenarioFailure("storage cache exceeded its configured limit")


def assert_seed_metrics(
    application: dict[str, object], expected_port: int, expected_payload: int
) -> None:
    incoming = application.get("incoming")
    if not isinstance(incoming, dict):
        raise ScenarioFailure("restarted seeding generation lacks incoming metrics")
    if incoming.get("listen") != f"127.0.0.1:{expected_port}":
        raise ScenarioFailure(f"incoming listener metrics differ: {incoming}")
    exact_limits = {
        "configured_connection_limit": 37,
        "incoming_connection_slack": 10,
    }
    for field, expected in exact_limits.items():
        if integer_field(incoming, field) != expected:
            raise ScenarioFailure(f"{field} did not retain {expected}")
    upper_bounds = {
        "pending_high_water": 8,
        "established_high_water": 47,
        "connection_high_water": 47,
        "queued_requests_high_water": 2_000,
        "queued_bytes_high_water": 2_000 * 16 * 1024,
        "read_high_water": 10,
        "read_bytes_high_water": 10 * 16 * 1024,
        "writer_send_buffer_high_water": 528_396,
        "upload_regular_high_water": 1,
        "upload_optimistic_high_water": 1,
        "upload_slots_high_water": 1,
    }
    for field, maximum in upper_bounds.items():
        actual = integer_field(incoming, field)
        if actual > maximum:
            raise ScenarioFailure(f"{field} reached {actual}, exceeding {maximum}")
    for field in (
        "established_high_water",
        "connection_high_water",
        "upload_slots_high_water",
    ):
        if integer_field(incoming, field) < 1:
            raise ScenarioFailure(f"{field} did not observe the controlled leecher")
    if integer_field(incoming, "payload_bytes_sent") < expected_payload:
        raise ScenarioFailure("RSTorrent did not report the complete seeded payload")


def stop_and_observe(
    gateway: subprocess.Popen[str],
) -> tuple[dict[str, object], dict[str, object]]:
    stderr = stop_gateway(gateway)
    gateway_metrics = connection_metrics(stderr)
    application = application_metrics(stderr)
    assert_shutdown_metrics(gateway_metrics, application)
    return gateway_metrics, application


def close_libtorrent_seed(
    session: lt.session | None, handle: lt.torrent_handle | None
) -> None:
    if session is not None:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
    gc.collect()


def run() -> None:
    repository = Path(__file__).resolve().parents[2]
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-client-settings-restart-"))
    profile = run_path / "profile"
    storage = run_path / "downloads"
    gateway: subprocess.Popen[str] | None = None
    vite: subprocess.Popen[str] | None = None
    seed_session: lt.session | None = None
    seed_handle: lt.torrent_handle | None = None
    conflict_listener: socket.socket | None = None
    failure: BaseException | None = None
    observations: list[dict[str, object]] = []
    try:
        fixture = create_fixture(
            run_path,
            payload_size=PAYLOAD_SIZE,
            piece_size=PIECE_SIZE,
        )
        seed_diagnostics: list[str] = []
        seed_session = create_session()
        seed_port = wait_for_listener(seed_session, seed_diagnostics)
        seed_handle = add_seed(
            seed_session, fixture.torrent_info, fixture.seed_directory, seed_diagnostics
        )

        binary = build_gateway(repository)
        vite_port = reserve_loopback_port()
        origin = f"http://127.0.0.1:{vite_port}"
        vite = build_and_start_production_web(repository, origin, vite_port)

        gateway, address = start_gateway(binary, profile, storage, origin)
        observations.append(
            run_playwright(
                repository, origin, address, "configure", fixture, seed_port
            )
        )
        _, first_application = stop_and_observe(gateway)
        gateway = None
        if first_application.get("incoming") is not None:
            raise ScenarioFailure("default generation unexpectedly owned a listener")
        verify_payload(storage, ROOT_NAME, fixture.payload_hash)
        close_libtorrent_seed(seed_session, seed_handle)
        seed_session = None
        seed_handle = None

        gateway, address = start_gateway(binary, profile, storage, origin)
        active = run_playwright(
            repository, origin, address, "observe", fixture, seed_port
        )
        observations.append(active)
        active_port = active.get("listenerPort")
        if not isinstance(active_port, int) or active_port == 0:
            raise ScenarioFailure("restarted automatic listener did not expose a port")
        seeded_hash = leech_from_rstorrent(
            fixture, active_port, run_path / "libtorrent-leech"
        )

        conflict_port = reserve_loopback_port()
        set_fixed_settings(address, origin, "client-settings-fixed-conflict", conflict_port)
        _, seed_application = stop_and_observe(gateway)
        gateway = None
        assert_seed_metrics(seed_application, active_port, PAYLOAD_SIZE)

        conflict_listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        conflict_listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        conflict_listener.bind(("127.0.0.1", conflict_port))
        conflict_listener.listen()
        gateway, address = start_gateway(binary, profile, storage, origin)
        observations.append(
            run_playwright(repository, origin, address, "recover", fixture, seed_port)
        )
        _, conflict_application = stop_and_observe(gateway)
        gateway = None
        if conflict_application.get("incoming") is not None:
            raise ScenarioFailure("failed fixed bind unexpectedly created incoming owners")
        conflict_listener.close()
        conflict_listener = None

        gateway, address = start_gateway(binary, profile, storage, origin)
        repaired = run_playwright(
            repository, origin, address, "observe", fixture, seed_port
        )
        observations.append(repaired)
        repaired_port = repaired.get("listenerPort")
        if not isinstance(repaired_port, int) or repaired_port == 0:
            raise ScenarioFailure("repaired automatic listener did not expose a port")
        _, repaired_application = stop_and_observe(gateway)
        gateway = None
        incoming = repaired_application.get("incoming")
        if not isinstance(incoming, dict) or incoming.get("listen") != (
            f"127.0.0.1:{repaired_port}"
        ):
            raise ScenarioFailure("repaired listener metrics differ from the product view")

        print(
            "client_settings_restart_milestone "
            + json.dumps(
                {
                    "configured": {
                        "listener": "automatic_loopback",
                        "peer_connection_limit": 37,
                        "upload_slots": 1,
                    },
                    "phases": observations,
                    "fixed_conflict_port": conflict_port,
                    "payload_bytes": PAYLOAD_SIZE,
                    "payload_sha1": seeded_hash,
                    "seeding_resources": seed_application,
                    "terminal_owners": 0,
                    "gateway_shutdown": "joined",
                    "axe_serious_or_critical": 0,
                    "cleanup": "ok",
                },
                separators=(",", ":"),
            )
        )
    except BaseException as error:
        failure = error
    finally:
        if gateway is not None:
            try:
                stop_gateway(gateway)
            except BaseException as cleanup_error:
                failure = failure or cleanup_error
        if vite is not None:
            try:
                stop_process(vite, "production web preview")
            except BaseException as cleanup_error:
                failure = failure or cleanup_error
        if conflict_listener is not None:
            conflict_listener.close()
        close_libtorrent_seed(seed_session, seed_handle)
        shutil.rmtree(run_path, ignore_errors=True)
    if failure is not None:
        raise failure


if __name__ == "__main__":
    try:
        run()
    except ScenarioFailure as error:
        print(f"scenario_failed={error}", file=os.sys.stderr)
        raise SystemExit(1) from error
