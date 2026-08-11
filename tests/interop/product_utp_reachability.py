#!/usr/bin/env python3
"""Prove product-owned incoming uTP through one direct off-LAN path."""

from __future__ import annotations

import argparse
import json
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

from upnp_external_seeding import (
    GateFailure,
    build_seed,
    delete_mapping,
    discover_control,
    list_mappings,
    query_mapping,
    read_json_line,
    stop_seed,
    terminate,
)
from utp_reference_oracle import PAYLOAD_NAME, create_fixture
from utp_remote_seed import eligible_public_ipv4
from utp_rstorrent_wan import (
    SCENARIO_TIMEOUT_SECONDS,
    SSH_ALIAS_PATTERN,
    RemoteProcess,
    WanFailure,
    bounded_diagnostics,
    create_remote_run,
    remote_leecher_started_pid,
    stage_remote_leecher_fixture,
    validate_remote_leecher_complete,
    validate_remote_leecher_ready,
    verify_direct_route,
    verify_remote_leecher_cleanup,
)


ROOT = Path(__file__).resolve().parents[2]
MAPPING_DESCRIPTION = "RSTorrent"


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", required=True)
    return parser.parse_args()


def local_route_address() -> str:
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        probe.connect(("1.1.1.1", 53))
        address = probe.getsockname()[0]
    finally:
        probe.close()
    if address.startswith("127.") or address == "0.0.0.0":
        raise WanFailure("local route did not select a usable IPv4 address")
    return address


def gateway_preflight() -> tuple[str, str, str]:
    local_address = local_route_address()
    control, service = discover_control(local_address)
    owned = [
        entry
        for entry in list_mappings(control, service)
        if entry.get("NewInternalClient") == local_address
        and entry.get("NewPortMappingDescription") == MAPPING_DESCRIPTION
    ]
    if owned:
        raise WanFailure("gateway preflight found an existing local RSTorrent mapping")
    return local_address, control, service


def start_product_seed(
    binary: Path,
    profile_root: Path,
    storage_root: Path,
    metainfo: Path,
) -> tuple[subprocess.Popen[str], dict[str, Any]]:
    process = subprocess.Popen(
        [
            str(binary),
            "--profile-root",
            str(profile_root),
            "--storage-root",
            str(storage_root),
            "--metainfo",
            str(metainfo),
            "--upnp",
            "--utp",
            "--await-udp-mapping",
        ],
        cwd=ROOT,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    try:
        ready = read_json_line(process, 60)
        if ready.get("event") != "ready" or ready.get("registrations") != 1:
            raise WanFailure("product seed did not become registered and UDP-mapped")
        return process, ready
    except BaseException as error:
        terminate(process)
        stderr = process.stderr.read() if process.stderr is not None else ""
        detail = bounded_diagnostics(stderr.splitlines())
        raise WanFailure(
            "product seed failed before UDP-mapped readiness"
            + (f": {detail[-5:]}" if detail else "")
        ) from error


def parse_endpoint(value: object, field: str) -> tuple[str, int]:
    if not isinstance(value, str):
        raise WanFailure(f"product readiness omitted {field}")
    address, separator, port_text = value.rpartition(":")
    try:
        port = int(port_text)
    except ValueError as error:
        raise WanFailure(f"product readiness has malformed {field}") from error
    if separator != ":" or not address or not 1 <= port <= 65_535:
        raise WanFailure(f"product readiness has invalid {field}")
    return address, port


def validate_udp_mapping(
    ready: dict[str, Any],
) -> tuple[str, int, str, int, int]:
    mapping = ready.get("udp_mapping")
    if not isinstance(mapping, dict) or mapping.get("type") != "mapped":
        raise WanFailure("product did not publish a verified UDP mapping")
    local_address = mapping.get("local_address")
    local_port = mapping.get("local_port")
    external_address = mapping.get("external_address")
    external_port = mapping.get("external_port")
    lease_seconds = mapping.get("lease_seconds")
    if not (
        isinstance(local_address, str)
        and isinstance(local_port, int)
        and isinstance(external_address, str)
        and isinstance(external_port, int)
        and isinstance(lease_seconds, int)
        and 1 <= local_port <= 65_535
        and 1 <= external_port <= 65_535
        and 0 < lease_seconds <= 3_600
        and eligible_public_ipv4(external_address)
    ):
        raise WanFailure("product UDP mapping fields are invalid")
    utp_address, utp_port = parse_endpoint(ready.get("utp_listen"), "utp_listen")
    if (utp_address, utp_port) != (local_address, local_port):
        raise WanFailure("product UDP mapping does not target its uTP listener")
    return local_address, local_port, external_address, external_port, lease_seconds


def verify_installed_mapping(
    control: str,
    service: str,
    local_address: str,
    local_port: int,
    external_port: int,
) -> None:
    installed = query_mapping(control, service, external_port, "UDP")
    if installed is None or not (
        installed.get("NewInternalClient") == local_address
        and installed.get("NewInternalPort") == str(local_port)
        and installed.get("NewEnabled") == "1"
        and installed.get("NewPortMappingDescription") == MAPPING_DESCRIPTION
        and 0 < int(installed.get("NewLeaseDuration", "0")) <= 3_600
    ):
        raise WanFailure("independent query did not verify the exact product UDP mapping")


def validate_product_stop(stopped: dict[str, Any]) -> None:
    utp = stopped.get("utp_before_shutdown")
    if not isinstance(utp, dict) or not (
        utp.get("connection_high_water") == 1
        and utp.get("worker_panics") == 0
        and isinstance(utp.get("datagrams_sent"), int)
        and utp["datagrams_sent"] > 0
    ):
        raise WanFailure("product did not retain exact incoming uTP evidence")
    if not (
        stopped.get("connection_high_water") == 1
        and stopped.get("mapping_tasks_after_shutdown") == 0
        and stopped.get("mappings_after_shutdown") == 0
    ):
        raise WanFailure("product reachability or peer owners were not terminal")


def run(host: str) -> dict[str, Any]:
    if not SSH_ALIAS_PATTERN.fullmatch(host) or host.startswith("-"):
        raise WanFailure("SSH host alias is malformed")
    binary = build_seed(ROOT)
    preflight_address, preflight_control, preflight_service = gateway_preflight()
    remote_run: str | None = None
    remote: RemoteProcess | None = None
    remote_pid: int | None = None
    seed: subprocess.Popen[str] | None = None
    control: str | None = None
    service: str | None = None
    mapping: tuple[str, int, str, int, int] | None = None
    normal_mapping_cleanup = False
    cleanup_errors: list[str] = []
    result: dict[str, Any] | None = None
    with tempfile.TemporaryDirectory(prefix="rstorrent-product-utp-wan-") as temporary:
        root = Path(temporary)
        _torrent_info, storage_root, expected_sha1 = create_fixture(root)
        metainfo = root / "forced-utp.torrent"
        deadline = time.monotonic() + SCENARIO_TIMEOUT_SECONDS
        try:
            remote_run = create_remote_run(host)
            stage_remote_leecher_fixture(host, remote_run, metainfo)
            seed, ready = start_product_seed(
                binary,
                root / "profile",
                storage_root,
                metainfo,
            )
            mapping = validate_udp_mapping(ready)
            local_address, local_port, external_address, external_port, lease_seconds = (
                mapping
            )
            verify_direct_route(host, external_address)
            if local_address != preflight_address:
                raise WanFailure("product mapping used a different local route than preflight")
            control, service = preflight_control, preflight_service
            verify_installed_mapping(
                control,
                service,
                local_address,
                local_port,
                external_port,
            )

            remote = RemoteProcess.start_leecher(
                host,
                remote_run,
                expected_sha1,
                external_address,
                external_port,
            )
            remote_pid = remote_leecher_started_pid(remote.read_event(deadline))
            validate_remote_leecher_ready(remote.read_event(deadline), remote_pid)
            remote_complete = remote.read_event(deadline)
            validate_remote_leecher_complete(remote_complete, expected_sha1)
            remote.wait_success(deadline)

            stopped = stop_seed(seed)
            seed = None
            validate_product_stop(stopped)
            if query_mapping(control, service, external_port, "UDP") is not None:
                raise WanFailure("product UDP mapping survived joined shutdown")
            normal_mapping_cleanup = True
            result = {
                "schema_version": 1,
                "oracle": "product-owned-incoming-utp-wan",
                "libtorrent_version": "2.0.13.0",
                "transport": {
                    "tcp_outgoing": False,
                    "utp_incoming": True,
                    "dht_peer_hint": False,
                    "ssh_data_path": False,
                    "route_class": "ordinary-internet",
                    "endpoint": "<public-ip>:<transient-port>",
                },
                "mapping": {
                    "owner": "product-session",
                    "protocol": "UDP",
                    "lease_seconds": lease_seconds,
                    "target_matches_utp_listener": True,
                    "independently_verified": True,
                    "deleted": True,
                },
                "payload": {
                    "bytes": remote_complete["payload"]["bytes"],
                    "pieces": remote_complete["payload"]["pieces"],
                    "sha1": expected_sha1,
                },
                "remote": {
                    "peer_high_water": remote_complete["peer_high_water"],
                    "libtorrent_stats": remote_complete["libtorrent_stats"],
                },
                "product": {
                    "incoming_connection_high_water": stopped["connection_high_water"],
                    "utp_connection_high_water": stopped["utp_before_shutdown"][
                        "connection_high_water"
                    ],
                    "terminal_mapping_tasks": stopped[
                        "mapping_tasks_after_shutdown"
                    ],
                    "terminal_mappings": stopped["mappings_after_shutdown"],
                },
            }
        finally:
            if seed is not None:
                try:
                    stopped = stop_seed(seed)
                    validate_product_stop(stopped)
                    seed = None
                except BaseException:
                    terminate(seed)
                    seed = None
            if remote is not None:
                remote.cleanup()
            if remote_run is not None:
                try:
                    verify_remote_leecher_cleanup(host, remote_run, remote_pid)
                except Exception as error:
                    cleanup_errors.append(str(error))
            if mapping is not None and control is not None and service is not None:
                local_address, local_port, _external_address, external_port, _lease = mapping
                try:
                    installed = query_mapping(control, service, external_port, "UDP")
                    if installed is not None:
                        owned = (
                            installed.get("NewInternalClient") == local_address
                            and installed.get("NewInternalPort") == str(local_port)
                            and installed.get("NewPortMappingDescription")
                            == MAPPING_DESCRIPTION
                        )
                        if not owned:
                            raise WanFailure(
                                "refusing to delete a non-owned UDP mapping during cleanup"
                            )
                        delete_mapping(control, service, external_port, "UDP")
                    if query_mapping(control, service, external_port, "UDP") is not None:
                        raise WanFailure("product UDP mapping survived cleanup")
                except (GateFailure, OSError, ValueError, WanFailure) as error:
                    cleanup_errors.append(str(error))
    if cleanup_errors:
        raise WanFailure("WAN cleanup failed: " + "; ".join(cleanup_errors))
    if result is None:
        raise WanFailure("product incoming uTP WAN case produced no result")
    result["cleanup"] = {
        "succeeded": True,
        "mapping_deleted_by_product": normal_mapping_cleanup,
        "mapping_absent": True,
        "remote_process_absent": True,
        "remote_run_directory_removed": True,
        "local_temporary_directory_removed": True,
    }
    return result


def main() -> int:
    arguments = parse_arguments()
    started = time.monotonic()
    result = run(arguments.host)
    result["seconds"] = round(time.monotonic() - started, 6)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (GateFailure, WanFailure, OSError, subprocess.TimeoutExpired) as error:
        print(f"product incoming uTP WAN failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
