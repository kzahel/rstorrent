#!/usr/bin/env python3
"""Serve one bounded exact torrent to a physical iOS product client over LAN."""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import select
import shutil
import socket
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

import libtorrent as lt


MIB = 1024 * 1024
POLL_SECONDS = 0.02
MAX_TIMEOUT_SECONDS = 15 * 60


class SeedFailure(RuntimeError):
    pass


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--payload-mib", type=int, default=2)
    parser.add_argument("--timeout-seconds", type=int, default=600)
    parsed = parser.parse_args()
    if not 1 <= parsed.payload_mib <= 64:
        parser.error("--payload-mib must be between 1 and 64")
    if not 30 <= parsed.timeout_seconds <= MAX_TIMEOUT_SECONDS:
        parser.error(f"--timeout-seconds must be between 30 and {MAX_TIMEOUT_SECONDS}")
    return parsed


def write_event(value: dict[str, Any]) -> None:
    print(json.dumps(value, sort_keys=True, separators=(",", ":")), flush=True)


def route_address() -> str:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.connect(("192.0.2.1", 9))
        return str(probe.getsockname()[0])


def write_payload(path: Path, length: int) -> str:
    path.parent.mkdir(parents=True, exist_ok=True)
    block = bytes((((index * 73) ^ (index >> 3) ^ 0x5A) & 0xFF) for index in range(MIB))
    digest = hashlib.sha1()
    with path.open("wb") as output:
        remaining = length
        while remaining:
            chunk = block[: min(remaining, len(block))]
            output.write(chunk)
            digest.update(chunk)
            remaining -= len(chunk)
    return digest.hexdigest()


def create_fixture(root: Path, length: int) -> tuple[lt.torrent_info, Path, str]:
    storage = root / "seed"
    payload = storage / "rstorrent-ios-controlled.bin"
    digest = write_payload(payload, length)
    files = lt.file_storage()
    files.add_file(payload.name, length)
    creator = lt.create_torrent(files, piece_size=256 * 1024, flags=lt.create_torrent.v1_only)
    lt.set_piece_hashes(creator, str(storage))
    metainfo = root / "rstorrent-ios-controlled.torrent"
    metainfo.write_bytes(bytes(lt.bencode(creator.generate())))
    return lt.torrent_info(str(metainfo)), metainfo, digest


def open_seed(torrent: lt.torrent_info, storage: Path, address: str) -> tuple[lt.session, lt.torrent_handle]:
    settings = {
        "listen_interfaces": f"{address}:0",
        "enable_dht": False,
        "enable_lsd": False,
        "enable_upnp": False,
        "enable_natpmp": False,
        "enable_incoming_tcp": True,
        "enable_outgoing_tcp": True,
        "enable_incoming_utp": False,
        "enable_outgoing_utp": False,
        "allow_multiple_connections_per_ip": False,
        "connections_limit": 2,
        "connection_speed": 2,
        "max_peerlist_size": 8,
        "alert_queue_size": 256,
        "in_enc_policy": int(lt.enc_policy.pe_disabled),
        "out_enc_policy": int(lt.enc_policy.pe_disabled),
    }
    session = lt.session(settings)
    params = lt.add_torrent_params()
    params.ti = torrent
    params.save_path = str(storage)
    params.flags &= ~lt.torrent_flags.auto_managed
    params.flags &= ~lt.torrent_flags.paused
    handle = session.add_torrent(params)
    return session, handle


def run(parsed: argparse.Namespace) -> None:
    owned = Path(tempfile.mkdtemp(prefix="rstorrent-ios-seed-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    expected_bytes = parsed.payload_mib * MIB
    try:
        torrent, metainfo, digest = create_fixture(owned, expected_bytes)
        address = route_address()
        session, handle = open_seed(torrent, owned / "seed", address)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            status = handle.status()
            if status.errc.value() != 0:
                raise SeedFailure(f"seed failed: {status.errc.message()}")
            if status.is_seeding and session.is_listening() and session.listen_port() > 0:
                break
            time.sleep(POLL_SECONDS)
        else:
            raise SeedFailure("seed readiness timed out")

        info_hash = str(torrent.info_hashes().v1)
        port = session.listen_port()
        write_event(
            {
                "event": "ready",
                "info_hash": info_hash,
                "magnet": (
                    f"magnet:?xt=urn:btih:{info_hash}"
                    f"&dn=rstorrent-ios-controlled.bin&x.pe={address}:{port}"
                ),
                "metainfo": str(metainfo),
                "expected_bytes": expected_bytes,
                "expected_sha1": digest,
            }
        )

        command = ""
        peer_high_water = 0
        deadline = time.monotonic() + parsed.timeout_seconds
        while time.monotonic() < deadline:
            status = handle.status()
            if status.errc.value() != 0:
                raise SeedFailure(f"seed failed: {status.errc.message()}")
            peer_high_water = max(peer_high_water, int(status.num_peers))
            if peer_high_water > 1:
                raise SeedFailure("seed exceeded one connected product peer")
            readable, _, _ = select.select([sys.stdin], [], [], POLL_SECONDS)
            if not readable:
                continue
            command = sys.stdin.readline().strip()
            if command == "snapshot":
                write_event(
                    {
                        "event": "snapshot",
                        "peer_high_water": peer_high_water,
                        "payload_bytes_sent": int(status.total_payload_upload),
                    }
                )
                continue
            break
        if command not in {"stop", "abort"}:
            raise SeedFailure("seed did not receive a bounded terminal command")
        status = handle.status()
        sent = int(status.total_payload_upload)
        if command == "stop" and sent < expected_bytes:
            raise SeedFailure(f"seed sent {sent} bytes, expected at least {expected_bytes}")
        write_event(
            {
                "event": "stopped" if command == "stop" else "aborted",
                "peer_high_water": peer_high_water,
                "payload_bytes_sent": sent,
                "cleanup": True,
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
        shutil.rmtree(owned, ignore_errors=True)


def main() -> int:
    try:
        run(arguments())
        return 0
    except (OSError, SeedFailure, ValueError) as error:
        write_event({"event": "failed", "reason": str(error)[:512]})
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
