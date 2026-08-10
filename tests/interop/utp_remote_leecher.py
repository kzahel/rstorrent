#!/usr/bin/env python3
"""Attached pinned-libtorrent leecher for a mapped RSTorrent uTP seed."""

from __future__ import annotations

import argparse
import gc
import ipaddress
import json
import os
import re
import shutil
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
    create_session,
    eligible_public_ipv4,
    hash_file,
    local_route_address,
    stats_snapshot,
)


TRANSFER_TIMEOUT_SECONDS = 180.0


class RemoteLeecherFailure(RuntimeError):
    pass


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--metainfo", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--peer-address", required=True)
    parser.add_argument("--peer-port", type=int, required=True)
    parser.add_argument("--expected-sha1", required=True)
    return parser.parse_args()


def write_event(value: dict[str, Any]) -> None:
    print(json.dumps(value, sort_keys=True), flush=True)


def verify_direct_route(address: str) -> str:
    if not eligible_public_ipv4(address):
        raise RemoteLeecherFailure("RSTorrent endpoint is not public IPv4")
    connection = os.environ.get("SSH_CONNECTION", "").split()
    if connection and connection[0] == address:
        raise RemoteLeecherFailure("uTP endpoint equals the SSH control endpoint")
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
        raise RemoteLeecherFailure("uTP endpoint routes through an overlay interface")
    return interface


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
    verify_direct_route(arguments.peer_address)
    write_event({"event": "started", "role": "remote-leecher", "pid": os.getpid()})

    torrent_info = lt.torrent_info(str(arguments.metainfo))
    if (
        torrent_info.total_size() != PAYLOAD_BYTES
        or torrent_info.piece_length() != PIECE_BYTES
        or torrent_info.num_pieces() != 33
        or torrent_info.num_files() != 1
        or any(True for _ in torrent_info.trackers())
        or any(True for _ in torrent_info.web_seeds())
    ):
        raise RemoteLeecherFailure("remote metainfo violates the exact fixture contract")
    arguments.output_root.mkdir()
    local_address = local_route_address()
    local_port = available_udp_port(local_address)
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    try:
        session = create_session(local_port)
        handle = add_leecher(session, torrent_info, arguments.output_root)
        deadline = time.monotonic() + TRANSFER_TIMEOUT_SECONDS
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
            }
        )

        transfer_started = time.monotonic()
        handle.connect_peer((arguments.peer_address, arguments.peer_port))
        peer_high_water = 0
        while time.monotonic() < deadline:
            collect_alerts(session, diagnostics)
            status = handle.status()
            if status.errc.value() != 0:
                raise RemoteLeecherFailure("remote leecher failed during transfer")
            peer_high_water = max(peer_high_water, len(handle.get_peer_info()))
            if peer_high_water > 1:
                raise RemoteLeecherFailure("remote leecher exceeded one peer")
            if status.is_seeding:
                break
            time.sleep(POLL_SECONDS)
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
            raise RemoteLeecherFailure("remote uTP download timed out")

        output = arguments.output_root / torrent_info.name()
        if (
            not output.is_file()
            or output.stat().st_size != PAYLOAD_BYTES
            or hash_file(output) != arguments.expected_sha1
        ):
            raise RemoteLeecherFailure("remote output violates the exact fixture contract")
        stats = stats_snapshot(session, diagnostics, min(deadline, time.monotonic() + 2.0))
        if (
            peer_high_water != 1
            or stats["peer.num_tcp_peers"] != 0
            or stats["peer.num_utp_peers"] > 1
            or stats["net.recv_payload_bytes"] < PAYLOAD_BYTES
            or stats["utp.utp_packets_in"] <= 0
            or stats["utp.utp_packets_out"] <= 0
        ):
            raise RemoteLeecherFailure("remote leecher transport evidence failed")
        session.remove_torrent(handle)
        handle = None
        session.pause()
        write_event(
            {
                "event": "complete",
                "role": "remote-leecher",
                "peer_high_water": peer_high_water,
                "payload": {
                    "bytes": PAYLOAD_BYTES,
                    "pieces": torrent_info.num_pieces(),
                    "sha1": arguments.expected_sha1,
                },
                "libtorrent_stats": stats,
                "diagnostics": diagnostics[-MAX_DIAGNOSTICS:],
                "transfer_seconds": round(time.monotonic() - transfer_started, 6),
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
        print(f"remote uTP leecher failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
