#!/usr/bin/env python3
"""Attached pinned-libtorrent seed with one verified temporary UDP mapping."""

from __future__ import annotations

import argparse
import gc
import hashlib
import ipaddress
import json
import os
import re
import select
import socket
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import libtorrent as lt


PAYLOAD_BYTES = 2 * 1024 * 1024 + 731
PIECE_BYTES = 64 * 1024
POLL_SECONDS = 0.05
READY_TIMEOUT_SECONDS = 30.0
STOP_TIMEOUT_SECONDS = 45.0
MAPPING_QUERY_TIMEOUT_SECONDS = 8.0
MAPPING_CLEANUP_SECONDS = 5.0
MAX_UPNPC_BYTES = 64 * 1024
MAX_DIAGNOSTICS = 50
MAX_LEASE_SECONDS = 3_600
STATS_NAMES = (
    "peer.num_tcp_peers",
    "peer.num_utp_peers",
    "net.sent_payload_bytes",
    "net.recv_payload_bytes",
    "utp.utp_packets_in",
    "utp.utp_packets_out",
    "utp.utp_payload_pkts_in",
    "utp.utp_payload_pkts_out",
    "utp.utp_packet_loss",
    "utp.utp_timeout",
    "utp.utp_fast_retransmit",
    "utp.utp_packet_resend",
)
MAPPING_LINE = re.compile(
    r"^\s*\d+\s+(TCP|UDP)\s+(\d+)->([0-9.]+):(\d+)\s+"
    r"'([^']*)'\s+'[^']*'\s+(\d+)\s*$"
)


class RemoteSeedFailure(RuntimeError):
    pass


@dataclass(frozen=True)
class MappingEntry:
    protocol: str
    external_port: int
    internal_address: str
    internal_port: int
    description: str
    lease_seconds: int


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--metainfo", type=Path, required=True)
    parser.add_argument("--seed-root", type=Path, required=True)
    parser.add_argument("--expected-sha1", required=True)
    return parser.parse_args()


def hash_file(path: Path) -> str:
    digest = hashlib.sha1()
    with path.open("rb") as source:
        while chunk := source.read(64 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def eligible_public_ipv4(value: str) -> bool:
    try:
        address = ipaddress.ip_address(value)
    except ValueError:
        return False
    return isinstance(address, ipaddress.IPv4Address) and address.is_global


def local_route_address() -> str:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.connect(("192.0.2.1", 9))
        address = str(probe.getsockname()[0])
    parsed = ipaddress.ip_address(address)
    if not isinstance(parsed, ipaddress.IPv4Address) or not parsed.is_private:
        raise RemoteSeedFailure("ordinary remote route has no private IPv4 source")
    return address


def available_udp_port(local_address: str) -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.bind((local_address, 0))
        port = int(probe.getsockname()[1])
    if not 1 <= port <= 65535:
        raise RemoteSeedFailure("failed to choose a bounded UDP listener port")
    return port


def parse_mapping_entries(output: str) -> list[MappingEntry]:
    entries: list[MappingEntry] = []
    for line in output.splitlines():
        match = MAPPING_LINE.match(line)
        if match is None:
            continue
        entries.append(
            MappingEntry(
                protocol=match.group(1),
                external_port=int(match.group(2)),
                internal_address=match.group(3),
                internal_port=int(match.group(4)),
                description=match.group(5),
                lease_seconds=int(match.group(6)),
            )
        )
    return entries


def query_mappings() -> list[MappingEntry]:
    completed = subprocess.run(
        ["upnpc", "-l"],
        capture_output=True,
        text=True,
        timeout=MAPPING_QUERY_TIMEOUT_SECONDS,
        check=False,
    )
    output = completed.stdout + completed.stderr
    if len(output.encode()) > MAX_UPNPC_BYTES:
        raise RemoteSeedFailure("UPnP mapping inventory exceeded its output bound")
    if completed.returncode != 0:
        raise RemoteSeedFailure("UPnP mapping inventory failed")
    return parse_mapping_entries(output)


def matching_mappings(
    entries: list[MappingEntry],
    local_address: str,
    local_port: int,
) -> list[MappingEntry]:
    return [
        entry
        for entry in entries
        if entry.internal_address == local_address and entry.internal_port == local_port
    ]


def collect_alerts(
    session: lt.session,
    diagnostics: list[str],
) -> list[Any]:
    alerts = session.pop_alerts()
    for alert in alerts:
        if isinstance(
            alert, (lt.session_stats_alert, lt.session_stats_header_alert)
        ):
            continue
        diagnostics.append(alert.what())
    del diagnostics[:-MAX_DIAGNOSTICS]
    return alerts


def stats_snapshot(
    session: lt.session,
    diagnostics: list[str],
    deadline: float,
) -> dict[str, int]:
    available = {
        metric.name
        for metric in lt.session_stats_metrics()
        if metric.name in STATS_NAMES
    }
    missing = set(STATS_NAMES) - available
    if missing:
        raise RemoteSeedFailure("pinned oracle lacks required uTP metrics")
    session.post_session_stats()
    while time.monotonic() < deadline:
        for alert in collect_alerts(session, diagnostics):
            if isinstance(alert, lt.session_stats_alert):
                return {name: int(alert.values[name]) for name in available}
        time.sleep(POLL_SECONDS)
    raise RemoteSeedFailure("pinned oracle statistics timed out")


def create_session(port: int) -> lt.session:
    return lt.session(
        {
            "listen_interfaces": f"0.0.0.0:{port}",
            "enable_dht": False,
            "enable_lsd": False,
            "enable_upnp": True,
            "enable_natpmp": False,
            "enable_incoming_tcp": False,
            "enable_outgoing_tcp": False,
            "enable_incoming_utp": True,
            "enable_outgoing_utp": True,
            "allow_multiple_connections_per_ip": True,
            "in_enc_policy": int(lt.enc_policy.pe_disabled),
            "out_enc_policy": int(lt.enc_policy.pe_disabled),
            "connections_limit": 4,
            "max_peerlist_size": 8,
            "upnp_lease_duration": MAX_LEASE_SECONDS,
            "alert_queue_size": 256,
            "alert_mask": int(
                lt.alert.category_t.error_notification
                | lt.alert.category_t.peer_notification
                | lt.alert.category_t.port_mapping_notification
                | lt.alert.category_t.stats_notification
                | lt.alert.category_t.status_notification
            ),
        }
    )


def add_seed(
    session: lt.session,
    torrent_info: lt.torrent_info,
    seed_root: Path,
) -> lt.torrent_handle:
    parameters = lt.add_torrent_params()
    parameters.ti = torrent_info
    parameters.save_path = str(seed_root)
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    parameters.flags |= lt.torrent_flags.seed_mode
    return session.add_torrent(parameters)


def mapping_alert_values(alert: Any) -> tuple[str, str, int]:
    protocol = "UDP" if alert.map_protocol == lt.portmap_protocol.udp else "TCP"
    transport = (
        "UPnP" if alert.map_transport == lt.portmap_transport.upnp else "NAT-PMP"
    )
    return protocol, transport, int(alert.external_port)


def disable_and_delete_mapping(
    session: lt.session | None,
    mapping_handles: list[Any],
    local_address: str | None,
    local_port: int | None,
    external_port: int | None,
) -> bool:
    if session is not None:
        for handle in mapping_handles:
            try:
                session.delete_port_mapping(handle)
            except RuntimeError:
                pass
        session.apply_settings({"enable_upnp": False})
    if local_address is None or local_port is None or external_port is None:
        return True
    deadline = time.monotonic() + MAPPING_CLEANUP_SECONDS
    while time.monotonic() < deadline:
        if not matching_mappings(query_mappings(), local_address, local_port):
            return True
        time.sleep(0.1)
    subprocess.run(
        ["upnpc", "-d", str(external_port), "UDP"],
        capture_output=True,
        text=True,
        timeout=MAPPING_QUERY_TIMEOUT_SECONDS,
        check=False,
    )
    return not matching_mappings(query_mappings(), local_address, local_port)


def write_event(value: dict[str, Any]) -> None:
    print(json.dumps(value, sort_keys=True), flush=True)


def run(arguments: argparse.Namespace) -> None:
    if lt.version != "2.0.13.0":
        raise RemoteSeedFailure("remote oracle version is not 2.0.13.0")
    if not re.fullmatch(r"[0-9a-f]{40}", arguments.expected_sha1):
        raise RemoteSeedFailure("expected SHA-1 is malformed")
    torrent_info = lt.torrent_info(str(arguments.metainfo))
    if (
        torrent_info.total_size() != PAYLOAD_BYTES
        or torrent_info.piece_length() != PIECE_BYTES
        or torrent_info.num_pieces() != 33
        or torrent_info.num_files() != 1
        or any(True for _ in torrent_info.trackers())
        or any(True for _ in torrent_info.web_seeds())
    ):
        raise RemoteSeedFailure("remote metainfo violates the exact fixture contract")
    payload = arguments.seed_root / torrent_info.name()
    if (
        not payload.is_file()
        or payload.stat().st_size != PAYLOAD_BYTES
        or hash_file(payload) != arguments.expected_sha1
    ):
        raise RemoteSeedFailure("remote seed payload violates the exact fixture contract")

    local_address = local_route_address()
    local_port = available_udp_port(local_address)
    if matching_mappings(query_mappings(), local_address, local_port):
        raise RemoteSeedFailure("selected remote listener port already has a mapping")

    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    mapping_handles: list[Any] = []
    external_port: int | None = None
    external_address: str | None = None
    diagnostics: list[str] = []
    cleanup_succeeded = False
    try:
        session = create_session(local_port)
        handle = add_seed(session, torrent_info, arguments.seed_root)
        ready_deadline = time.monotonic() + READY_TIMEOUT_SECONDS
        while time.monotonic() < ready_deadline:
            for alert in collect_alerts(session, diagnostics):
                if isinstance(alert, lt.portmap_alert):
                    mapping_handles.append(alert.mapping)
                    protocol, transport, port = mapping_alert_values(alert)
                    if transport != "UPnP" or protocol != "UDP":
                        raise RemoteSeedFailure(
                            "remote oracle created a non-UPnP or non-UDP mapping"
                        )
                    external_port = port
                elif isinstance(alert, lt.portmap_error_alert):
                    if alert.map_transport == lt.portmap_transport.upnp:
                        raise RemoteSeedFailure("remote UPnP mapping failed")
                elif isinstance(alert, lt.external_ip_alert):
                    candidate = str(alert.external_address)
                    if eligible_public_ipv4(candidate):
                        external_address = candidate
            status = handle.status()
            if status.errc.value() != 0:
                raise RemoteSeedFailure("remote seed entered an error state")
            if (
                status.is_seeding
                and session.is_listening()
                and session.listen_port() == local_port
                and external_port is not None
                and external_address is not None
            ):
                installed = matching_mappings(
                    query_mappings(), local_address, local_port
                )
                if len(installed) != 1:
                    raise RemoteSeedFailure(
                        "independent query did not find exactly one owned mapping"
                    )
                mapping = installed[0]
                if (
                    mapping.protocol != "UDP"
                    or mapping.external_port != external_port
                    or not 0 < mapping.lease_seconds <= MAX_LEASE_SECONDS
                ):
                    raise RemoteSeedFailure(
                        "independent query rejected the exact finite UDP mapping"
                    )
                write_event(
                    {
                        "event": "ready",
                        "role": "remote-seed",
                        "pid": os.getpid(),
                        "listen_port": local_port,
                        "external_address": external_address,
                        "external_port": external_port,
                        "mapping": {
                            "protocol": "UDP",
                            "transport": "UPnP",
                            "lease_seconds": mapping.lease_seconds,
                        },
                        "libtorrent_version": lt.version,
                    }
                )
                break
            time.sleep(POLL_SECONDS)
        else:
            raise RemoteSeedFailure("remote mapped seed readiness timed out")

        stop_deadline = time.monotonic() + STOP_TIMEOUT_SECONDS
        peer_high_water = 0
        command = None
        while time.monotonic() < stop_deadline:
            collect_alerts(session, diagnostics)
            status = handle.status()
            if status.errc.value() != 0:
                raise RemoteSeedFailure("remote seed failed during transfer")
            peer_high_water = max(peer_high_water, len(handle.get_peer_info()))
            if peer_high_water > 1:
                raise RemoteSeedFailure("remote seed exceeded one peer")
            readable, _, _ = select.select([sys.stdin], [], [], POLL_SECONDS)
            if readable:
                command = sys.stdin.readline().strip()
                break
        if command not in {"stop", "abort"}:
            raise RemoteSeedFailure("remote seed did not receive a bounded stop command")
        if command == "abort":
            raise RemoteSeedFailure("remote seed was aborted by the local owner")

        stats = stats_snapshot(session, diagnostics, time.monotonic() + 2.0)
        if (
            peer_high_water != 1
            or stats["peer.num_tcp_peers"] != 0
            or stats["peer.num_utp_peers"] > 1
            or stats["net.sent_payload_bytes"] < PAYLOAD_BYTES
            or stats["utp.utp_packets_in"] <= 0
            or stats["utp.utp_packets_out"] <= 0
        ):
            raise RemoteSeedFailure("remote seed transport evidence failed")

        session.remove_torrent(handle)
        handle = None
        session.pause()
        cleanup_succeeded = disable_and_delete_mapping(
            session,
            mapping_handles,
            local_address,
            local_port,
            external_port,
        )
        if not cleanup_succeeded:
            raise RemoteSeedFailure("remote UDP mapping cleanup was not verified")
        write_event(
            {
                "event": "complete",
                "role": "remote-seed",
                "peer_high_water": peer_high_water,
                "libtorrent_stats": stats,
                "mapping_deleted": True,
                "diagnostics": diagnostics,
            }
        )
    finally:
        if session is not None and handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        if session is not None:
            session.pause()
        if not cleanup_succeeded:
            cleanup_succeeded = disable_and_delete_mapping(
                session,
                mapping_handles,
                local_address,
                local_port,
                external_port,
            )
        handle = None
        session = None
        gc.collect()
        if not cleanup_succeeded:
            raise RemoteSeedFailure("remote mapping cleanup remained uncertain")


def main() -> int:
    run(parse_arguments())
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RemoteSeedFailure as error:
        print(f"remote uTP seed failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
