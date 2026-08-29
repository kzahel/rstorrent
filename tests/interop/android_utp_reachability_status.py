#!/usr/bin/env python3
"""Prove product uTP reachability status and lifecycle on an API 34 AVD."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path
from typing import Any

from android_utp_path_mtu import (
    ABI_TARGETS,
    APPLICATION_BINARY,
    AndroidUtpFailure,
    build_binaries,
    local_binary,
    parse_endpoint_port,
    push,
    start_remote_role,
)
from headless_avd import OwnedHeadlessAvd, adb_shell, default_adb, default_emulator
from utp_reference_oracle import PAYLOAD_NAME, create_fixture


ROOT = Path(__file__).resolve().parents[2]
CASE_TIMEOUT_SECONDS = 60.0


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--avd", default="jstorrent-tablet")
    parser.add_argument("--no-build", action="store_true")
    return parser.parse_args()


def mapping_status(value: object, field: str) -> dict[str, Any]:
    if not isinstance(value, dict) or value.get("type") != "disabled":
        raise AndroidUtpFailure(f"Android {field} is not cleanly disabled: {value}")
    return value


def run(arguments: argparse.Namespace) -> dict[str, Any]:
    if not arguments.no_build:
        build_binaries()
    adb = default_adb()
    emulator = default_emulator()
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="rstorrent-android-utp-status-") as temporary:
        root = Path(temporary)
        fixture_root = root / "fixture"
        fixture_root.mkdir()
        _torrent_info, seed_root, expected_sha1 = create_fixture(fixture_root)
        torrent = fixture_root / "forced-utp.torrent"
        payload = seed_root / PAYLOAD_NAME
        avd = OwnedHeadlessAvd.start(arguments.avd, adb, emulator, root)
        remote_root = f"/data/local/tmp/rstorrent-t140-{uuid.uuid4().hex[:12]}"
        if not remote_root.startswith("/data/local/tmp/rstorrent-t140-"):
            raise AndroidUtpFailure("unsafe Android temporary root")
        application = None
        try:
            sdk = adb_shell(adb, avd.serial, "getprop", "ro.build.version.sdk").stdout.strip()
            abi = adb_shell(adb, avd.serial, "getprop", "ro.product.cpu.abi").stdout.strip()
            if sdk != "34" or abi not in ABI_TARGETS:
                raise AndroidUtpFailure(
                    "expected a supported API 34 AVD, "
                    f"found API {sdk} {abi}"
                )
            application_binary = local_binary(abi, APPLICATION_BINARY)
            remote_application = f"{remote_root}/bin/{APPLICATION_BINARY}"
            remote_torrent = f"{remote_root}/forced-utp.torrent"
            remote_storage = f"{remote_root}/application-storage"
            remote_payload = f"{remote_storage}/{PAYLOAD_NAME}"
            adb_shell(
                adb,
                avd.serial,
                "mkdir",
                "-p",
                f"{remote_root}/bin",
                remote_storage,
            )
            push(adb, avd.serial, application_binary, remote_application)
            push(adb, avd.serial, torrent, remote_torrent)
            push(adb, avd.serial, payload, remote_payload)
            adb_shell(adb, avd.serial, "chmod", "700", remote_application)
            application = start_remote_role(
                adb,
                avd.serial,
                remote_application,
                [
                    "--profile-root",
                    f"{remote_root}/profile",
                    "--storage-root",
                    remote_storage,
                    "--metainfo",
                    remote_torrent,
                    "--controlled-local-network",
                    "--utp",
                ],
            )
            deadline = time.monotonic() + CASE_TIMEOUT_SECONDS
            ready = application.read_event(deadline)
            if ready.get("event") != "ready" or ready.get("registrations") != 1:
                raise AndroidUtpFailure(f"unexpected Android application readiness: {ready}")
            parse_endpoint_port(ready.get("utp_listen"), "application uTP listen")
            tcp_status = mapping_status(ready.get("tcp_mapping_status"), "TCP mapping status")
            udp_status = mapping_status(ready.get("udp_mapping_status"), "UDP mapping status")

            application.send_command("snapshot")
            snapshot = application.read_event(deadline)
            utp = snapshot.get("utp")
            summary = snapshot.get("summary")
            torrent_summary = summary.get("torrent") if isinstance(summary, dict) else None
            if not (
                isinstance(utp, dict)
                and utp.get("path_mtu_profile") == "dynamic_ipv4"
                and utp.get("active_connections") == 0
                and utp.get("worker_panics") == 0
                and isinstance(torrent_summary, dict)
                and torrent_summary.get("state") == "complete"
                and torrent_summary.get("storage_state") == "available"
            ):
                raise AndroidUtpFailure("Android product snapshot is not ready and bounded")

            application.send_stop()
            stopped = application.read_event(deadline)
            application.wait_success(deadline)
            application = None
            terminal_utp = stopped.get("utp_before_shutdown")
            if not (
                stopped.get("event") == "stopped"
                and isinstance(terminal_utp, dict)
                and terminal_utp.get("active_connections") == 0
                and terminal_utp.get("worker_panics") == 0
                and stopped.get("mapping_tasks_after_shutdown") == 0
                and stopped.get("mappings_after_shutdown") == 0
            ):
                raise AndroidUtpFailure("Android product lifecycle did not terminate cleanly")
            return {
                "schema_version": 1,
                "oracle": "android-product-utp-reachability-status",
                "target": {
                    "api": int(sdk),
                    "abi": abi,
                    "headless": True,
                },
                "application": {
                    "payload_sha1": expected_sha1,
                    "utp_listener_bound": True,
                    "path_mtu_profile": utp["path_mtu_profile"],
                    "tcp_mapping_status": tcp_status["type"],
                    "udp_mapping_status": udp_status["type"],
                    "active_connections": terminal_utp["active_connections"],
                    "worker_panics": terminal_utp["worker_panics"],
                    "terminal_mapping_tasks": stopped["mapping_tasks_after_shutdown"],
                    "terminal_mappings": stopped["mappings_after_shutdown"],
                },
                "cleanup": {
                    "remote_root_removed": True,
                    "owned_avd_stopped": True,
                    "temporary_directory_removed": True,
                },
                "seconds": round(time.monotonic() - started, 6),
            }
        finally:
            if application is not None:
                application.cleanup()
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
    except (AndroidUtpFailure, OSError, subprocess.SubprocessError) as error:
        print(f"Android uTP reachability status failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
