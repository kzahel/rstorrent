#!/usr/bin/env python3
"""Prove product uTP path-MTU and lifecycle behavior on one API 34 AVD."""

from __future__ import annotations

import argparse
import gc
import json
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path
from typing import Any

from first_verified_piece import ScenarioFailure
from headless_avd import OwnedHeadlessAvd, adb_shell, default_adb, default_emulator
from utp_reference_oracle import (
    PAYLOAD_NAME,
    POLL_SECONDS,
    add_torrent,
    collect_alerts,
    create_fixture,
    create_session,
    stats_snapshot,
    wait_until_ready,
)
from utp_rstorrent_interop import (
    OutputPump,
    RoleProcess,
    validate_libtorrent_stats,
)


ROOT = Path(__file__).resolve().parents[2]
ABI_TARGETS = {
    "x86_64": "x86_64-linux-android",
    "arm64-v8a": "aarch64-linux-android",
}
ROLE_BINARY = "rstorrent-utp-interop"
APPLICATION_BINARY = "rstorrent-incoming-seed"
CASE_TIMEOUT_SECONDS = 150.0
BUILD_TIMEOUT_SECONDS = 300.0
PLATFORM_PROBE_PAYLOAD_BYTES = 32 * 1024


class AndroidUtpFailure(ScenarioFailure):
    pass


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--avd", default="jstorrent-tablet")
    parser.add_argument("--no-build", action="store_true")
    return parser.parse_args()


def build_binaries() -> None:
    completed = subprocess.run(
        [
            "cargo",
            "ndk",
            "-t",
            "x86_64",
            "-t",
            "arm64-v8a",
            "-P",
            "28",
            "build",
            "--release",
            "-p",
            "rstorrent-engine",
            "--bin",
            ROLE_BINARY,
            "-p",
            "rstorrent-session",
            "--bin",
            APPLICATION_BINARY,
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=BUILD_TIMEOUT_SECONDS,
        check=False,
    )
    if completed.returncode != 0:
        raise AndroidUtpFailure(
            "failed to cross-build Android uTP diagnostics\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )


def local_binary(abi: str, name: str) -> Path:
    target = ABI_TARGETS.get(abi)
    if target is None:
        raise AndroidUtpFailure(f"unsupported Android AVD ABI: {abi}")
    path = ROOT / "target" / target / "release" / name
    if not path.is_file():
        raise AndroidUtpFailure(f"Android diagnostic binary is missing: {path}")
    return path


def adb_run(adb: Path, serial: str, *arguments: str) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        [str(adb), "-s", serial, *arguments],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    if completed.returncode != 0:
        raise AndroidUtpFailure(
            f"adb {' '.join(arguments)} failed\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    return completed


def push(adb: Path, serial: str, source: Path, destination: str) -> None:
    adb_run(adb, serial, "push", str(source), destination)


def start_remote_role(
    adb: Path, serial: str, binary: str, arguments: list[str]
) -> RoleProcess:
    process = subprocess.Popen(
        [str(adb), "-s", serial, "shell", binary, *arguments],
        cwd=ROOT,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    if process.stdout is None or process.stderr is None:
        process.kill()
        raise AndroidUtpFailure("failed to capture Android diagnostic output")
    stdout = OutputPump(process.stdout)
    stderr = OutputPump(process.stderr)
    return RoleProcess(process, stdout, stderr, (stdout.start(), stderr.start()))


def parse_endpoint_port(value: object, field: str) -> int:
    if not isinstance(value, str):
        raise AndroidUtpFailure(f"Android readiness lacks {field}")
    _, separator, port_text = value.rpartition(":")
    if separator != ":":
        raise AndroidUtpFailure(f"Android {field} is malformed: {value}")
    port = int(port_text)
    if not 1 <= port <= 65535:
        raise AndroidUtpFailure(f"Android {field} has an invalid port: {value}")
    return port


def validate_terminal_resources(resources: dict[str, Any]) -> None:
    for name in ("terminal_udp", "terminal_peer_udp"):
        snapshot = resources.get(name)
        if not isinstance(snapshot, dict) or snapshot.get("tasks") != 0:
            raise AndroidUtpFailure(f"Android platform probe retained {name}: {snapshot}")
        if snapshot.get("dht_queued") != 0 or snapshot.get("utp_queued") != 0:
            raise AndroidUtpFailure(f"Android platform probe retained queues: {snapshot}")
    for name in ("terminal_utp", "terminal_peer_utp"):
        snapshot = resources.get(name)
        if not isinstance(snapshot, dict) or snapshot.get("active_connections") != 0:
            raise AndroidUtpFailure(f"Android platform probe retained {name}: {snapshot}")


def run_platform_probe(
    adb: Path, serial: str, remote_binary: str, deadline: float
) -> dict[str, Any]:
    role = start_remote_role(adb, serial, remote_binary, ["platform-mtu-probe"])
    try:
        result = role.read_event(deadline)
        role.wait_success(deadline)
    finally:
        role.cleanup()
    capability = result.get("capability")
    payload = result.get("payload")
    resources = result.get("resources")
    if (
        result.get("event") != "complete"
        or result.get("role") != "platform-mtu-probe"
        or result.get("platform") != "android"
        or not isinstance(capability, dict)
        or capability.get("initial") != "Verified"
        or capability.get("replacement") != "Verified"
        or capability.get("generation_changed") is not True
        or capability.get("endpoint_changed") is not True
        or not isinstance(payload, dict)
        or payload.get("bytes") != PLATFORM_PROBE_PAYLOAD_BYTES
        or not isinstance(resources, dict)
    ):
        raise AndroidUtpFailure(f"invalid Android platform MTU evidence: {result}")
    utp = resources.get("live_utp")
    udp = resources.get("live_udp")
    if (
        not isinstance(utp, dict)
        or utp.get("path_mtu_profile") != "dynamic_ipv4"
        or not isinstance(utp.get("selected_mtu_max_bytes"), int)
        or utp["selected_mtu_max_bytes"] < 1_010
        or utp.get("mtu_probes_acknowledged_high_water", 0) <= 0
        or not isinstance(udp, dict)
        or udp.get("protected_sends_sent", 0) <= 0
        or udp.get("fragmentation_restore_failures") != 0
    ):
        raise AndroidUtpFailure(f"Android protected-send evidence failed: {resources}")
    validate_terminal_resources(resources)
    return result


def validate_application_snapshot(snapshot: dict[str, Any]) -> dict[str, Any]:
    summary = snapshot.get("summary")
    torrent = summary.get("torrent") if isinstance(summary, dict) else None
    if (
        not isinstance(torrent, dict)
        or torrent.get("state") != "complete"
        or torrent.get("storage_state") != "published"
    ):
        raise AndroidUtpFailure(f"Android application did not publish: {torrent}")
    utp = snapshot.get("utp")
    if (
        not isinstance(utp, dict)
        or utp.get("path_mtu_profile") != "dynamic_ipv4"
        or utp.get("active_connections") not in (0, 1)
        or utp.get("connection_high_water") != 1
        or utp.get("selected_mtu_min_bytes") != 548
        or not isinstance(utp.get("selected_mtu_max_bytes"), int)
        or not 548 <= utp["selected_mtu_max_bytes"] <= 1_472
        or utp.get("data_datagrams_sent", 0) <= 0
        or utp.get("worker_panics") != 0
    ):
        raise AndroidUtpFailure(f"Android application uTP evidence failed: {utp}")
    return utp


def run_application_transfer(
    adb: Path,
    serial: str,
    remote_root: str,
    application_binary: str,
    torrent_path: str,
    payload_path: str,
    torrent_info: Any,
    seed_root: Path,
    expected_sha1: str,
    deadline: float,
) -> dict[str, Any]:
    application: RoleProcess | None = None
    session = None
    handle = None
    diagnostics: list[str] = []
    try:
        session = create_session()
        handle = add_torrent(session, torrent_info, seed_root, seed=True)
        host_port = wait_until_ready(
            session,
            handle,
            seed=True,
            deadline=deadline,
            diagnostics=diagnostics,
        )
        application = start_remote_role(
            adb,
            serial,
            application_binary,
            [
                "--profile-root",
                f"{remote_root}/profile",
                "--storage-root",
                f"{remote_root}/application-storage",
                "--metainfo",
                torrent_path,
                "--controlled-local-network",
                "--fixture-payload",
                payload_path,
                "--initial-piece",
                "0",
                "--peer",
                f"10.0.2.2:{host_port}",
                "--encryption",
                "disabled",
            ],
        )
        ready = application.read_event(deadline)
        if ready.get("event") != "ready" or ready.get("registrations") != 1:
            raise AndroidUtpFailure(f"unexpected Android application readiness: {ready}")
        parse_endpoint_port(ready.get("utp_listen"), "application uTP listen")
        tcp_mapping = ready.get("tcp_mapping_status")
        udp_mapping = ready.get("udp_mapping_status")
        if not (
            isinstance(tcp_mapping, dict)
            and tcp_mapping.get("type") == "disabled"
            and isinstance(udp_mapping, dict)
            and udp_mapping.get("type") == "disabled"
        ):
            raise AndroidUtpFailure(
                "Android application mapping statuses did not preserve disabled "
                f"lifecycle semantics: tcp={tcp_mapping}, udp={udp_mapping}"
            )

        snapshot: dict[str, Any] | None = None
        observed_outgoing_utp = False
        while time.monotonic() < deadline:
            if application.process.poll() is not None:
                raise AndroidUtpFailure("Android application stopped during uTP transfer")
            collect_alerts(session, diagnostics)
            status = handle.status()
            if status.errc.value() != 0:
                raise AndroidUtpFailure(
                    f"pinned libtorrent seed failed: {status.errc.message()}"
                )
            application.send_command("snapshot")
            snapshot = application.read_event(deadline)
            peers = snapshot.get("peers")
            rows = peers.get("peers") if isinstance(peers, dict) else None
            observed_outgoing_utp |= isinstance(rows, list) and any(
                isinstance(row, dict)
                and row.get("direction") == "outgoing"
                and row.get("transport") == "utp"
                for row in rows
            )
            swarm = snapshot.get("swarm")
            counts = swarm.get("counts") if isinstance(swarm, dict) else None
            if isinstance(counts, dict) and counts.get("backed_off", 0) > 0:
                raise AndroidUtpFailure(
                    f"Android application peer backed off: {swarm}; "
                    f"libtorrent={diagnostics}"
                )
            summary = snapshot.get("summary")
            torrent = summary.get("torrent") if isinstance(summary, dict) else None
            if (
                isinstance(torrent, dict)
                and torrent.get("state") == "complete"
                and torrent.get("storage_state") == "published"
            ):
                break
            time.sleep(POLL_SECONDS)
        else:
            raise AndroidUtpFailure(
                "Android application uTP transfer exceeded its deadline: "
                f"swarm={snapshot.get('swarm') if snapshot else None}; "
                f"utp={snapshot.get('utp') if snapshot else None}; "
                f"libtorrent={diagnostics}"
            )
        if snapshot is None:
            raise AndroidUtpFailure("Android application emitted no transfer snapshot")
        application_utp = validate_application_snapshot(snapshot)

        remote_output = f"{remote_root}/application-storage/{PAYLOAD_NAME}"
        device_hash = adb_shell(
            adb,
            serial,
            "sha1sum",
            remote_output,
            timeout=15,
        ).stdout.split()[0]
        if device_hash != expected_sha1:
            raise AndroidUtpFailure(
                f"Android application payload hash {device_hash} != {expected_sha1}"
            )
        oracle_stats = stats_snapshot(session, diagnostics, deadline)
        validate_libtorrent_stats("seed", oracle_stats)
        if not observed_outgoing_utp:
            raise AndroidUtpFailure("Android application lacked outgoing uTP")

        application.send_command("stop")
        stopped = application.read_event(deadline)
        application.wait_success(deadline)
        application = None
        stopped_utp = stopped.get("utp_before_shutdown")
        if (
            stopped.get("event") != "stopped"
            or not isinstance(stopped_utp, dict)
            or stopped_utp.get("active_connections") != 0
            or stopped_utp.get("worker_panics") != 0
        ):
            raise AndroidUtpFailure(f"Android application shutdown failed: {stopped}")
        return {
            "payload_sha1": expected_sha1,
            "application_utp": application_utp,
            "mapping_status": {
                "tcp": tcp_mapping,
                "udp": udp_mapping,
            },
            "application_snapshot": snapshot,
            "application_stopped": stopped,
            "libtorrent_stats": oracle_stats,
            "libtorrent_diagnostics": diagnostics,
        }
    finally:
        if application is not None:
            application.cleanup()
        if session is not None and handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        if session is not None:
            session.pause()
        gc.collect()


def run(arguments: argparse.Namespace) -> dict[str, Any]:
    if not arguments.no_build:
        build_binaries()
    adb = default_adb()
    emulator = default_emulator()
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="rstorrent-android-utp-") as temporary:
        root = Path(temporary)
        fixture_root = root / "fixture"
        fixture_root.mkdir()
        torrent_info, seed_root, expected_sha1 = create_fixture(fixture_root)
        torrent = fixture_root / "forced-utp.torrent"
        payload = seed_root / PAYLOAD_NAME
        avd = OwnedHeadlessAvd.start(arguments.avd, adb, emulator, root)
        remote_root = f"/data/local/tmp/rstorrent-t137-{uuid.uuid4().hex[:12]}"
        if not remote_root.startswith("/data/local/tmp/rstorrent-t137-"):
            raise AndroidUtpFailure("unsafe Android temporary root")
        try:
            sdk = adb_shell(adb, avd.serial, "getprop", "ro.build.version.sdk").stdout.strip()
            abi = adb_shell(adb, avd.serial, "getprop", "ro.product.cpu.abi").stdout.strip()
            if sdk != "34" or abi not in ABI_TARGETS:
                raise AndroidUtpFailure(
                    "expected a supported API 34 AVD, "
                    f"found API {sdk} {abi}"
                )
            role_binary = local_binary(abi, ROLE_BINARY)
            application_binary = local_binary(abi, APPLICATION_BINARY)
            adb_shell(
                adb,
                avd.serial,
                "mkdir",
                "-p",
                f"{remote_root}/bin",
                f"{remote_root}/seed",
                f"{remote_root}/application-storage",
            )
            remote_role = f"{remote_root}/bin/{ROLE_BINARY}"
            remote_application = f"{remote_root}/bin/{APPLICATION_BINARY}"
            remote_torrent = f"{remote_root}/forced-utp.torrent"
            remote_payload = f"{remote_root}/seed/{PAYLOAD_NAME}"
            push(adb, avd.serial, role_binary, remote_role)
            push(adb, avd.serial, application_binary, remote_application)
            push(adb, avd.serial, torrent, remote_torrent)
            push(adb, avd.serial, payload, remote_payload)
            adb_shell(
                adb,
                avd.serial,
                "chmod",
                "700",
                remote_role,
                remote_application,
            )
            platform = run_platform_probe(
                adb,
                avd.serial,
                remote_role,
                time.monotonic() + CASE_TIMEOUT_SECONDS,
            )
            application = run_application_transfer(
                adb,
                avd.serial,
                remote_root,
                remote_application,
                remote_torrent,
                remote_payload,
                torrent_info,
                seed_root,
                expected_sha1,
                time.monotonic() + CASE_TIMEOUT_SECONDS,
            )
            return {
                "schema_version": 1,
                "oracle": "android-product-utp-path-mtu",
                "target": {
                    "avd": arguments.avd,
                    "api": int(sdk),
                    "abi": abi,
                    "headless": True,
                },
                "platform_probe": platform,
                "application_transfer": application,
                "cleanup": {
                    "remote_root_removed": True,
                    "owned_avd_stopped": True,
                    "temporary_directory_removed": True,
                },
                "seconds": round(time.monotonic() - started, 6),
            }
        finally:
            adb_shell(
                adb,
                avd.serial,
                "rm",
                "-rf",
                remote_root,
                timeout=30,
                check=False,
            )
            avd.close()


def main() -> int:
    print(json.dumps(run(parse_arguments()), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(f"Android uTP path-MTU failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
