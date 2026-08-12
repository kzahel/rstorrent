#!/usr/bin/env python3
"""Attached pinned-libtorrent leecher for a mapped RSTorrent seed."""

from __future__ import annotations

import argparse
import gc
import ipaddress
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

import libtorrent as lt

from utp_remote_seed import (
    MAX_DIAGNOSTICS,
    PAYLOAD_BYTES,
    PIECE_BYTES,
    POLL_SECONDS,
    available_udp_port,
    collect_alerts,
    eligible_public_ipv4,
    hash_file,
    local_route_address,
    stats_snapshot,
)


TRANSFER_TIMEOUT_SECONDS = 180.0
MAX_TRANSFER_TIMEOUT_SECONDS = 600.0
MILESTONE_FRACTIONS = (1, 25, 50, 75, 100)


class RemoteLeecherFailure(RuntimeError):
    pass


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--metainfo", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--peer-address", required=True)
    parser.add_argument("--peer-port", type=int, required=True)
    parser.add_argument("--expected-sha1", required=True)
    parser.add_argument("--transport", choices=("tcp", "utp"), default="utp")
    parser.add_argument("--expected-bytes", type=int, default=PAYLOAD_BYTES)
    parser.add_argument("--expected-piece-bytes", type=int, default=PIECE_BYTES)
    parser.add_argument("--expected-pieces", type=int, default=33)
    parser.add_argument(
        "--timeout-seconds", type=float, default=TRANSFER_TIMEOUT_SECONDS
    )
    return parser.parse_args()


def write_event(value: dict[str, Any]) -> None:
    print(json.dumps(value, sort_keys=True), flush=True)


def verify_direct_route(address: str) -> str:
    if not eligible_public_ipv4(address):
        raise RemoteLeecherFailure("RSTorrent endpoint is not public IPv4")
    connection = os.environ.get("SSH_CONNECTION", "").split()
    if connection and connection[0] == address:
        raise RemoteLeecherFailure("payload endpoint equals the SSH control endpoint")
    ip_command = shutil.which("ip")
    if ip_command is None:
        raise RemoteLeecherFailure("remote host has no route inspection command")
    completed = subprocess.run(
        [ip_command, "-4", "route", "get", address],
        capture_output=True,
        text=True,
        timeout=5,
        check=False,
    )
    match = re.search(r"\bdev\s+(\S+)", completed.stdout)
    if completed.returncode != 0 or match is None:
        raise RemoteLeecherFailure("could not resolve route to RSTorrent endpoint")
    interface = match.group(1)
    if interface.lower().startswith(("utun", "tailscale", "tun", "tap", "wg", "lo")):
        raise RemoteLeecherFailure("payload endpoint routes through an overlay interface")
    return interface


def available_tcp_port(local_address: str) -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind((local_address, 0))
        port = int(probe.getsockname()[1])
    if not 1 <= port <= 65_535:
        raise RemoteLeecherFailure("failed to choose a bounded TCP listener port")
    return port


def transport_settings(local_address: str, port: int, transport: str) -> dict[str, Any]:
    if transport not in {"tcp", "utp"}:
        raise RemoteLeecherFailure("remote transport selection is invalid")
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
        "allow_multiple_connections_per_ip": True,
        "in_enc_policy": int(lt.enc_policy.pe_disabled),
        "out_enc_policy": int(lt.enc_policy.pe_disabled),
        "proxy_type": 0,
        "connections_limit": 4,
        "max_peerlist_size": 8,
        "alert_queue_size": 256,
        "alert_mask": int(
            lt.alert.category_t.error_notification
            | lt.alert.category_t.peer_notification
            | lt.alert.category_t.stats_notification
            | lt.alert.category_t.status_notification
        ),
    }


def create_transport_session(
    local_address: str, port: int, transport: str
) -> tuple[lt.session, dict[str, bool]]:
    expected = transport_settings(local_address, port, transport)
    session = lt.session(expected)
    applied = session.get_settings()
    names = (
        "enable_incoming_tcp",
        "enable_outgoing_tcp",
        "enable_incoming_utp",
        "enable_outgoing_utp",
        "enable_dht",
        "enable_lsd",
        "enable_upnp",
        "enable_natpmp",
    )
    evidence = {name: bool(applied[name]) for name in names}
    expected_evidence = {name: bool(expected[name]) for name in names}
    if evidence != expected_evidence or int(applied["proxy_type"]) != 0:
        raise RemoteLeecherFailure("libtorrent did not apply exact transport settings")
    return session, evidence


def milestone_thresholds(payload_bytes: int) -> dict[str, int]:
    return {
        str(percent): max(1, (payload_bytes * percent + 99) // 100)
        for percent in MILESTONE_FRACTIONS
    }


def add_leecher(
    session: lt.session,
    torrent_info: lt.torrent_info,
    output_root: Path,
) -> lt.torrent_handle:
    parameters = lt.add_torrent_params()
    parameters.ti = torrent_info
    parameters.save_path = str(output_root)
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    return session.add_torrent(parameters)


def run(arguments: argparse.Namespace) -> None:
    if lt.version != "2.0.13.0":
        raise RemoteLeecherFailure("remote oracle version is not 2.0.13.0")
    if not re.fullmatch(r"[0-9a-f]{40}", arguments.expected_sha1):
        raise RemoteLeecherFailure("expected SHA-1 is malformed")
    if not 1 <= arguments.peer_port <= 65535:
        raise RemoteLeecherFailure("RSTorrent endpoint port is invalid")
    if not (
        arguments.expected_bytes > 0
        and arguments.expected_piece_bytes > 0
        and arguments.expected_pieces > 0
        and 0 < arguments.timeout_seconds <= MAX_TRANSFER_TIMEOUT_SECONDS
    ):
        raise RemoteLeecherFailure("remote fixture or timeout bound is invalid")
    verify_direct_route(arguments.peer_address)
    write_event({"event": "started", "role": "remote-leecher", "pid": os.getpid()})

    torrent_info = lt.torrent_info(str(arguments.metainfo))
    if (
        torrent_info.total_size() != arguments.expected_bytes
        or torrent_info.piece_length() != arguments.expected_piece_bytes
        or torrent_info.num_pieces() != arguments.expected_pieces
        or torrent_info.num_files() != 1
        or any(True for _ in torrent_info.trackers())
        or any(True for _ in torrent_info.web_seeds())
    ):
        raise RemoteLeecherFailure("remote metainfo violates the exact fixture contract")
    arguments.output_root.mkdir()
    local_address = local_route_address()
    local_port = (
        available_tcp_port(local_address)
        if arguments.transport == "tcp"
        else available_udp_port(local_address)
    )
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    try:
        session, applied_settings = create_transport_session(
            local_address, local_port, arguments.transport
        )
        handle = add_leecher(session, torrent_info, arguments.output_root)
        deadline = time.monotonic() + arguments.timeout_seconds
        while time.monotonic() < deadline:
            collect_alerts(session, diagnostics)
            status = handle.status()
            if status.errc.value() != 0:
                raise RemoteLeecherFailure("remote leecher entered an error state")
            if session.is_listening() and session.listen_port() == local_port:
                break
            time.sleep(POLL_SECONDS)
        else:
            raise RemoteLeecherFailure("remote leecher readiness timed out")
        write_event(
            {
                "event": "ready",
                "role": "remote-leecher",
                "pid": os.getpid(),
                "listen_port": local_port,
                "libtorrent_version": lt.version,
                "route_class": "ordinary-internet",
                "transport": arguments.transport,
                "applied_transport_settings": applied_settings,
            }
        )

        transfer_started = time.monotonic()
        handle.connect_peer((arguments.peer_address, arguments.peer_port))
        peer_high_water = 0
        first_payload_at: float | None = None
        milestones: dict[str, float] = {}
        thresholds = milestone_thresholds(arguments.expected_bytes)
        while time.monotonic() < deadline:
            collect_alerts(session, diagnostics)
            status = handle.status()
            if status.errc.value() != 0:
                raise RemoteLeecherFailure("remote leecher failed during transfer")
            peer_high_water = max(peer_high_water, len(handle.get_peer_info()))
            if peer_high_water > 1:
                raise RemoteLeecherFailure("remote leecher exceeded one peer")
            observed_at = time.monotonic()
            wanted_done = int(status.total_wanted_done)
            if wanted_done > 0 and first_payload_at is None:
                first_payload_at = observed_at
            for name, threshold in thresholds.items():
                if name not in milestones and wanted_done >= threshold:
                    milestones[name] = observed_at - transfer_started
            if status.is_seeding:
                completed_at = observed_at
                break
            time.sleep(0.01 if first_payload_at is not None else POLL_SECONDS)
        else:
            stats = stats_snapshot(session, diagnostics, time.monotonic() + 2.0)
            status = handle.status()
            write_event(
                {
                    "event": "failed",
                    "role": "remote-leecher",
                    "reason": "transfer-timeout",
                    "peer_high_water": peer_high_water,
                    "progress_ppm": int(status.progress_ppm),
                    "wanted_done_bytes": int(status.total_wanted_done),
                    "libtorrent_stats": stats,
                    "diagnostics": diagnostics[-MAX_DIAGNOSTICS:],
                }
            )
            raise RemoteLeecherFailure("remote download timed out")

        if first_payload_at is None or set(milestones) != {
            str(percent) for percent in MILESTONE_FRACTIONS
        }:
            raise RemoteLeecherFailure("remote leecher missed exact payload milestones")
        connect_seconds = completed_at - transfer_started
        first_payload_seconds = first_payload_at - transfer_started
        active_seconds = completed_at - first_payload_at
        if not 0 <= first_payload_seconds < connect_seconds or active_seconds <= 0:
            raise RemoteLeecherFailure("remote leecher timing interval is invalid")

        output = arguments.output_root / torrent_info.name()
        if (
            not output.is_file()
            or output.stat().st_size != arguments.expected_bytes
            or hash_file(output) != arguments.expected_sha1
        ):
            raise RemoteLeecherFailure("remote output violates the exact fixture contract")
        stats = stats_snapshot(session, diagnostics, min(deadline, time.monotonic() + 2.0))
        transport_valid = (
            stats["peer.num_tcp_peers"] <= 1
            and stats["peer.num_utp_peers"] == 0
            and stats["utp.utp_packets_in"] == 0
            and stats["utp.utp_packets_out"] == 0
            if arguments.transport == "tcp"
            else stats["peer.num_tcp_peers"] == 0
            and stats["peer.num_utp_peers"] <= 1
            and stats["utp.utp_packets_in"] > 0
            and stats["utp.utp_packets_out"] > 0
        )
        if (
            peer_high_water != 1
            or stats["net.recv_payload_bytes"] < arguments.expected_bytes
            or not transport_valid
        ):
            raise RemoteLeecherFailure("remote leecher transport evidence failed")
        session.remove_torrent(handle)
        handle = None
        session.pause()
        write_event(
            {
                "event": "complete",
                "role": "remote-leecher",
                "transport": arguments.transport,
                "applied_transport_settings": applied_settings,
                "peer_high_water": peer_high_water,
                "payload": {
                    "bytes": arguments.expected_bytes,
                    "pieces": torrent_info.num_pieces(),
                    "sha1": arguments.expected_sha1,
                },
                "libtorrent_stats": stats,
                "diagnostics": diagnostics[-MAX_DIAGNOSTICS:],
                "timing": {
                    "connect_to_complete_seconds": round(connect_seconds, 6),
                    "first_payload_seconds": round(first_payload_seconds, 6),
                    "active_payload_seconds": round(active_seconds, 6),
                    "milestone_seconds": {
                        name: round(value, 6) for name, value in milestones.items()
                    },
                },
                "transfer_seconds": round(connect_seconds, 6),
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


def main() -> int:
    run(parse_arguments())
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RemoteLeecherFailure as error:
        print(f"remote leecher failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
