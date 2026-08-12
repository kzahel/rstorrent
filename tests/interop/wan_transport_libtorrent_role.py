#!/usr/bin/env python3
"""One bounded pinned-libtorrent seed or leecher role for Tactical 142."""

from __future__ import annotations

import argparse
import gc
import hashlib
import ipaddress
import json
import os
import re
import select
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

import libtorrent as lt


MIB = 1024 * 1024
GIB = 1024 * MIB
POLL_SECONDS = 0.02
READY_TIMEOUT_SECONDS = 60.0
STATS_TIMEOUT_SECONDS = 5.0
MAX_TIMEOUT_SECONDS = 12 * 60 * 60
MAX_DIAGNOSTICS = 100
MAX_OUTPUT_ROOT_BYTES = 2 * GIB
MILESTONE_PERCENTS = (25, 50, 75, 100)
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
    "utp.utp_samples_above_target",
    "utp.utp_samples_below_target",
)


class LibtorrentRoleError(RuntimeError):
    pass


def parse_arguments(arguments: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("role", choices=("seed", "leech"))
    parser.add_argument("--metainfo", type=Path, required=True)
    parser.add_argument("--storage-root", type=Path, required=True)
    parser.add_argument("--transport", choices=("tcp", "utp"), required=True)
    parser.add_argument("--expected-sha1", required=True)
    parser.add_argument("--expected-bytes", type=int, required=True)
    parser.add_argument("--expected-piece-bytes", type=int, required=True)
    parser.add_argument("--expected-pieces", type=int, required=True)
    parser.add_argument("--timeout-seconds", type=float, required=True)
    parser.add_argument("--network-scope", choices=("wan", "loopback"), default="wan")
    parser.add_argument("--peer-address")
    parser.add_argument("--peer-port", type=int)
    parsed = parser.parse_args(arguments)
    if (parsed.peer_address is None) != (parsed.peer_port is None):
        parser.error("--peer-address and --peer-port must appear together")
    if parsed.role == "leech" and parsed.peer_address is None:
        parser.error("leech requires --peer-address and --peer-port")
    if parsed.role == "seed" and parsed.peer_address is not None:
        parser.error("seed does not accept a peer endpoint")
    return parsed


def write_event(value: dict[str, Any]) -> None:
    print(json.dumps(value, sort_keys=True, separators=(",", ":")), flush=True)


def hash_file(path: Path) -> str:
    digest = hashlib.sha1()
    with path.open("rb") as source:
        while chunk := source.read(MIB):
            digest.update(chunk)
    return digest.hexdigest()


def local_route_address(scope: str = "wan") -> str:
    if scope == "loopback":
        return "127.0.0.1"
    if scope != "wan":
        raise LibtorrentRoleError("network scope is invalid")
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.connect(("192.0.2.1", 9))
        value = str(probe.getsockname()[0])
    address = ipaddress.ip_address(value)
    if not isinstance(address, ipaddress.IPv4Address) or not address.is_private:
        raise LibtorrentRoleError("ordinary route has no private IPv4 source")
    return value


def available_port(local_address: str, transport: str) -> int:
    kind = socket.SOCK_STREAM if transport == "tcp" else socket.SOCK_DGRAM
    with socket.socket(socket.AF_INET, kind) as probe:
        probe.bind((local_address, 0))
        port = int(probe.getsockname()[1])
    if not 1 <= port <= 65_535:
        raise LibtorrentRoleError("could not select a bounded listener port")
    return port


def verify_direct_route(address_text: str, scope: str = "wan") -> str:
    try:
        address = ipaddress.ip_address(address_text)
    except ValueError as error:
        raise LibtorrentRoleError("peer address is malformed") from error
    if scope == "loopback":
        if not isinstance(address, ipaddress.IPv4Address) or not address.is_loopback:
            raise LibtorrentRoleError("controlled peer endpoint is not IPv4 loopback")
        return "loopback"
    if scope != "wan":
        raise LibtorrentRoleError("network scope is invalid")
    if not isinstance(address, ipaddress.IPv4Address) or not address.is_global:
        raise LibtorrentRoleError("peer endpoint is not an eligible public IPv4 address")
    connection = os.environ.get("SSH_CONNECTION", "").split()
    if connection and connection[0] == address_text:
        raise LibtorrentRoleError("payload endpoint equals the SSH control endpoint")
    ip_command = shutil.which("ip")
    if ip_command:
        completed = subprocess.run(
            [ip_command, "-4", "route", "get", address_text],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
        match = re.search(r"\bdev\s+(\S+)", completed.stdout)
        if completed.returncode != 0 or match is None:
            raise LibtorrentRoleError("could not resolve the ordinary peer route")
        interface = match.group(1)
    else:
        route = shutil.which("route")
        if route is None:
            raise LibtorrentRoleError("host has no route inspection command")
        completed = subprocess.run(
            [route, "-n", "get", address_text],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
        match = re.search(r"^\s*interface:\s*(\S+)\s*$", completed.stdout, re.MULTILINE)
        if completed.returncode != 0 or match is None:
            raise LibtorrentRoleError("could not resolve the ordinary peer route")
        interface = match.group(1)
    if interface.lower().startswith(("utun", "tailscale", "tun", "tap", "wg", "lo")):
        raise LibtorrentRoleError("payload endpoint routes through an overlay interface")
    return interface


def transport_settings(local_address: str, port: int, transport: str) -> dict[str, Any]:
    if transport not in {"tcp", "utp"}:
        raise LibtorrentRoleError("transport selection is invalid")
    tcp = transport == "tcp"
    utp = transport == "utp"
    return {
        "listen_interfaces": f"{local_address}:{port}",
        "enable_dht": False,
        "enable_lsd": False,
        "enable_upnp": False,
        "enable_natpmp": False,
        "enable_incoming_tcp": tcp,
        "enable_outgoing_tcp": tcp,
        "enable_incoming_utp": utp,
        "enable_outgoing_utp": utp,
        "allow_multiple_connections_per_ip": False,
        "in_enc_policy": int(lt.enc_policy.pe_disabled),
        "out_enc_policy": int(lt.enc_policy.pe_disabled),
        "proxy_type": 0,
        "connections_limit": 1,
        "connection_speed": 1,
        "max_peerlist_size": 8,
        "download_rate_limit": 0,
        "upload_rate_limit": 0,
        "alert_queue_size": 512,
        "alert_mask": int(
            lt.alert.category_t.error_notification
            | lt.alert.category_t.peer_notification
            | lt.alert.category_t.stats_notification
            | lt.alert.category_t.status_notification
            | lt.alert.category_t.storage_notification
        ),
    }


def create_session(
    local_address: str, port: int, transport: str
) -> tuple[lt.session, dict[str, bool | int]]:
    requested = transport_settings(local_address, port, transport)
    session = lt.session(requested)
    applied = session.get_settings()
    boolean_names = (
        "enable_dht",
        "enable_lsd",
        "enable_upnp",
        "enable_natpmp",
        "enable_incoming_tcp",
        "enable_outgoing_tcp",
        "enable_incoming_utp",
        "enable_outgoing_utp",
        "allow_multiple_connections_per_ip",
    )
    evidence: dict[str, bool | int] = {
        name: bool(applied[name]) for name in boolean_names
    }
    evidence.update(
        {
            "proxy_type": int(applied["proxy_type"]),
            "connections_limit": int(applied["connections_limit"]),
            "connection_speed": int(applied["connection_speed"]),
        }
    )
    expected: dict[str, bool | int] = {
        name: bool(requested[name]) for name in boolean_names
    }
    expected.update({"proxy_type": 0, "connections_limit": 1, "connection_speed": 1})
    if evidence != expected:
        raise LibtorrentRoleError("libtorrent did not apply exact transport settings")
    return session, evidence


def collect_alerts(session: lt.session, diagnostics: list[str]) -> None:
    for alert in session.pop_alerts():
        if isinstance(alert, (lt.session_stats_alert, lt.session_stats_header_alert)):
            continue
        diagnostics.append(alert.what())
    del diagnostics[:-MAX_DIAGNOSTICS]


def stats_snapshot(session: lt.session, diagnostics: list[str]) -> dict[str, int]:
    available = {
        metric.name for metric in lt.session_stats_metrics() if metric.name in STATS_NAMES
    }
    missing = set(STATS_NAMES) - available
    if missing:
        raise LibtorrentRoleError(f"pinned oracle lacks metrics {sorted(missing)}")
    session.post_session_stats()
    deadline = time.monotonic() + STATS_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        alerts = session.pop_alerts()
        for alert in alerts:
            if isinstance(alert, lt.session_stats_alert):
                return {name: int(alert.values[name]) for name in available}
            if not isinstance(alert, lt.session_stats_header_alert):
                diagnostics.append(alert.what())
        del diagnostics[:-MAX_DIAGNOSTICS]
        time.sleep(POLL_SECONDS)
    raise LibtorrentRoleError("libtorrent statistics timed out")


def peer_count(handle: lt.torrent_handle) -> int:
    return len(handle.get_peer_info())


def validate_arguments(arguments: argparse.Namespace) -> lt.torrent_info:
    if lt.version != "2.0.13.0":
        raise LibtorrentRoleError("oracle version is not pinned libtorrent 2.0.13.0")
    if not re.fullmatch(r"[0-9a-f]{40}", arguments.expected_sha1):
        raise LibtorrentRoleError("expected SHA-1 is malformed")
    if not (
        0 < arguments.expected_bytes <= GIB
        and arguments.expected_piece_bytes == 256 * 1024
        and arguments.expected_pieces > 0
        and arguments.expected_pieces
        == (arguments.expected_bytes + arguments.expected_piece_bytes - 1)
        // arguments.expected_piece_bytes
        and 0 < arguments.timeout_seconds <= MAX_TIMEOUT_SECONDS
    ):
        raise LibtorrentRoleError("fixture geometry or timeout is outside the matrix bound")
    if arguments.peer_port is not None and not 1 <= arguments.peer_port <= 65_535:
        raise LibtorrentRoleError("peer port is outside its bound")
    torrent_info = lt.torrent_info(str(arguments.metainfo))
    if (
        torrent_info.total_size() != arguments.expected_bytes
        or torrent_info.piece_length() != arguments.expected_piece_bytes
        or torrent_info.num_pieces() != arguments.expected_pieces
        or torrent_info.num_files() != 1
        or any(True for _ in torrent_info.trackers())
        or any(True for _ in torrent_info.web_seeds())
    ):
        raise LibtorrentRoleError("metainfo violates the exact matrix fixture contract")
    return torrent_info


def add_torrent(
    session: lt.session,
    torrent_info: lt.torrent_info,
    storage_root: Path,
    *,
    seed: bool,
) -> lt.torrent_handle:
    parameters = lt.add_torrent_params()
    parameters.ti = torrent_info
    parameters.save_path = str(storage_root)
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    if seed:
        parameters.flags |= lt.torrent_flags.seed_mode
    return session.add_torrent(parameters)


def transport_valid(
    transport: str,
    peer_high_water: int,
    stats: dict[str, int],
) -> bool:
    if peer_high_water != 1:
        return False
    if transport == "tcp":
        return (
            stats["utp.utp_packets_in"] == 0
            and stats["utp.utp_packets_out"] == 0
            and stats["peer.num_utp_peers"] == 0
            and stats["peer.num_tcp_peers"] <= 1
        )
    return (
        stats["utp.utp_packets_in"] > 0
        and stats["utp.utp_packets_out"] > 0
        and stats["peer.num_tcp_peers"] == 0
        and stats["peer.num_utp_peers"] <= 1
    )


def run_seed(arguments: argparse.Namespace, torrent_info: lt.torrent_info) -> None:
    payload = arguments.storage_root / torrent_info.name()
    if (
        not payload.is_file()
        or payload.stat().st_size != arguments.expected_bytes
        or hash_file(payload) != arguments.expected_sha1
    ):
        raise LibtorrentRoleError("seed payload violates the exact fixture contract")
    local_address = local_route_address(arguments.network_scope)
    local_port = available_port(local_address, arguments.transport)
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    peer_high_water = 0
    try:
        session, applied = create_session(local_address, local_port, arguments.transport)
        handle = add_torrent(session, torrent_info, arguments.storage_root, seed=True)
        deadline = time.monotonic() + READY_TIMEOUT_SECONDS
        while time.monotonic() < deadline:
            collect_alerts(session, diagnostics)
            status = handle.status()
            if status.errc.value() != 0:
                raise LibtorrentRoleError("seed entered an error state")
            if status.is_seeding and session.is_listening() and session.listen_port() == local_port:
                break
            time.sleep(POLL_SECONDS)
        else:
            raise LibtorrentRoleError("seed readiness timed out")
        write_event(
            {
                "event": "ready",
                "role": "seed",
                "pid": os.getpid(),
                "transport": arguments.transport,
                "local_address": local_address,
                "listen_port": local_port,
                "libtorrent_version": lt.version,
                "applied_transport_settings": applied,
            }
        )
        deadline = time.monotonic() + arguments.timeout_seconds
        command = ""
        while time.monotonic() < deadline:
            collect_alerts(session, diagnostics)
            status = handle.status()
            if status.errc.value() != 0:
                raise LibtorrentRoleError("seed failed during transfer")
            peers = peer_count(handle)
            peer_high_water = max(peer_high_water, peers)
            if peers > 1:
                raise LibtorrentRoleError("seed exceeded one connected peer")
            readable, _, _ = select.select([sys.stdin], [], [], POLL_SECONDS)
            if not readable:
                continue
            command = sys.stdin.readline().strip()
            if command == "snapshot":
                write_event(
                    {
                        "event": "snapshot",
                        "role": "seed",
                        "peer_high_water": peer_high_water,
                        "sent_payload_bytes": int(status.total_payload_upload),
                    }
                )
                continue
            break
        if command not in {"stop", "abort"}:
            raise LibtorrentRoleError("seed did not receive a bounded terminal command")
        stats = stats_snapshot(session, diagnostics)
        if command == "stop" and (
            stats["net.sent_payload_bytes"] < arguments.expected_bytes
            or not transport_valid(arguments.transport, peer_high_water, stats)
        ):
            raise LibtorrentRoleError("seed terminal transport evidence failed")
        write_event(
            {
                "event": "stopped" if command == "stop" else "aborted",
                "role": "seed",
                "transport": arguments.transport,
                "peer_high_water": peer_high_water,
                "libtorrent_stats": stats,
                "diagnostics": diagnostics,
                "session_cleanup": True,
            }
        )
    finally:
        if session is not None and handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        if session is not None:
            session.pause()
        handle = None
        session = None
        gc.collect()


def milestone_thresholds(payload_bytes: int) -> dict[str, int]:
    return {
        str(percent): max(1, (payload_bytes * percent + 99) // 100)
        for percent in MILESTONE_PERCENTS[:-1]
    }


def run_leech(arguments: argparse.Namespace, torrent_info: lt.torrent_info) -> None:
    assert arguments.peer_address is not None and arguments.peer_port is not None
    route_interface = verify_direct_route(arguments.peer_address, arguments.network_scope)
    if arguments.storage_root.exists():
        raise LibtorrentRoleError("leecher storage root already exists")
    arguments.storage_root.mkdir(parents=True)
    local_address = local_route_address(arguments.network_scope)
    local_port = available_port(local_address, arguments.transport)
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    peer_high_water = 0
    try:
        session, applied = create_session(local_address, local_port, arguments.transport)
        handle = add_torrent(session, torrent_info, arguments.storage_root, seed=False)
        deadline = time.monotonic() + READY_TIMEOUT_SECONDS
        while time.monotonic() < deadline:
            collect_alerts(session, diagnostics)
            if session.is_listening() and session.listen_port() == local_port:
                break
            time.sleep(POLL_SECONDS)
        else:
            raise LibtorrentRoleError("leecher readiness timed out")
        write_event(
            {
                "event": "ready",
                "role": "leech",
                "pid": os.getpid(),
                "transport": arguments.transport,
                "route_class": (
                    "ordinary-internet" if arguments.network_scope == "wan" else "controlled-loopback"
                ),
                "route_interface": route_interface,
                "libtorrent_version": lt.version,
                "applied_transport_settings": applied,
            }
        )
        transfer_started = time.monotonic()
        handle.connect_peer((arguments.peer_address, arguments.peer_port))
        deadline = transfer_started + arguments.timeout_seconds
        first_payload_at: float | None = None
        completed_at: float | None = None
        milestones: dict[str, float] = {}
        thresholds = milestone_thresholds(arguments.expected_bytes)
        while time.monotonic() < deadline:
            collect_alerts(session, diagnostics)
            status = handle.status()
            if status.errc.value() != 0:
                raise LibtorrentRoleError("leecher entered an error state")
            peers = peer_count(handle)
            peer_high_water = max(peer_high_water, peers)
            if peers > 1:
                raise LibtorrentRoleError("leecher exceeded one connected peer")
            observed_at = time.monotonic()
            wanted_done = int(status.total_wanted_done)
            if wanted_done > 0 and first_payload_at is None:
                first_payload_at = observed_at
            for name, threshold in thresholds.items():
                if name not in milestones and wanted_done >= threshold:
                    milestones[name] = observed_at - transfer_started
            if status.is_seeding:
                completed_at = observed_at
                milestones["100"] = completed_at - transfer_started
                break
            time.sleep(POLL_SECONDS)
        if completed_at is None:
            raise LibtorrentRoleError("leecher transfer timed out")
        if first_payload_at is None or set(milestones) != {"25", "50", "75", "100"}:
            raise LibtorrentRoleError("leecher missed bounded progress milestones")
        connect_seconds = completed_at - transfer_started
        active_seconds = completed_at - first_payload_at
        if active_seconds <= 0 or connect_seconds <= active_seconds:
            raise LibtorrentRoleError("leecher active timing is below sampling resolution")
        output = arguments.storage_root / torrent_info.name()
        if (
            not output.is_file()
            or output.stat().st_size != arguments.expected_bytes
            or hash_file(output) != arguments.expected_sha1
        ):
            raise LibtorrentRoleError("leecher output violates the exact fixture contract")
        stats = stats_snapshot(session, diagnostics)
        if (
            stats["net.recv_payload_bytes"] < arguments.expected_bytes
            or not transport_valid(arguments.transport, peer_high_water, stats)
        ):
            raise LibtorrentRoleError(
                "leecher terminal transport evidence failed: "
                f"peer_high_water={peer_high_water}, "
                f"tcp={stats['peer.num_tcp_peers']}, "
                f"utp={stats['peer.num_utp_peers']}, "
                f"packets={stats['utp.utp_packets_in']}/{stats['utp.utp_packets_out']}, "
                f"payload={stats['net.recv_payload_bytes']}"
            )
        write_event(
            {
                "event": "complete",
                "role": "leech",
                "transport": arguments.transport,
                "route_class": (
                    "ordinary-internet" if arguments.network_scope == "wan" else "controlled-loopback"
                ),
                "applied_transport_settings": applied,
                "peer_high_water": peer_high_water,
                "payload": {
                    "bytes": arguments.expected_bytes,
                    "pieces": arguments.expected_pieces,
                    "sha1": arguments.expected_sha1,
                },
                "timing": {
                    "connect_to_complete_seconds": round(connect_seconds, 6),
                    "first_payload_seconds": round(first_payload_at - transfer_started, 6),
                    "active_payload_seconds": round(active_seconds, 6),
                    "milestone_seconds": {
                        name: round(value, 6) for name, value in milestones.items()
                    },
                },
                "libtorrent_stats": stats,
                "diagnostics": diagnostics,
                "session_cleanup": True,
            }
        )
    finally:
        if session is not None and handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        if session is not None:
            session.pause()
        handle = None
        session = None
        gc.collect()


def run(arguments: argparse.Namespace) -> None:
    torrent_info = validate_arguments(arguments)
    write_event({"event": "started", "role": arguments.role, "pid": os.getpid()})
    if arguments.role == "seed":
        run_seed(arguments, torrent_info)
    else:
        run_leech(arguments, torrent_info)


def main() -> int:
    try:
        run(parse_arguments())
        return 0
    except (LibtorrentRoleError, OSError, ValueError) as error:
        write_event(
            {
                "event": "failed",
                "role": "libtorrent",
                "reason": str(error)[:512],
            }
        )
        print(f"WAN libtorrent role failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
