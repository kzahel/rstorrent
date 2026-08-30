#!/usr/bin/env python3
"""Serve one bounded single-file fixture from pinned libtorrent."""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import sys
import time
from pathlib import Path

import libtorrent as lt


MAX_FIXTURE_BYTES = 64 * 1024 * 1024
STARTUP_SECONDS = 10
POLL_SECONDS = 0.05


class SeedFailure(RuntimeError):
    pass


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metainfo", type=Path, required=True)
    parser.add_argument("--storage-root", type=Path, required=True)
    parser.add_argument("--timeout-seconds", type=int, default=90)
    parser.add_argument("--upload-rate-limit", type=int, default=0)
    values = parser.parse_args()
    if (
        not values.metainfo.is_absolute()
        or not values.metainfo.is_file()
        or not values.storage_root.is_absolute()
        or not values.storage_root.is_dir()
        or not 1 <= values.timeout_seconds <= 300
        or not (
            values.upload_rate_limit == 0
            or 16 * 1024 <= values.upload_rate_limit <= 1024 * 1024
        )
    ):
        parser.error(
            "fixture paths must be absolute, timeout must be 1..300 seconds, "
            "and upload rate must be 0 or 16384..1048576 bytes per second"
        )
    return values


def event(value: dict[str, object]) -> None:
    print(json.dumps(value, sort_keys=True, separators=(",", ":")), flush=True)


def file_sha1(path: Path) -> str:
    digest = hashlib.sha1()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def run(arguments: argparse.Namespace) -> None:
    if lt.version != "2.0.13.0":
        raise SeedFailure(f"libtorrent {lt.version} is not the pinned 2.0.13.0 oracle")
    torrent_info = lt.torrent_info(str(arguments.metainfo))
    payload = arguments.storage_root / torrent_info.name()
    if (
        torrent_info.num_files() != 1
        or not 0 < torrent_info.total_size() <= MAX_FIXTURE_BYTES
        or not payload.is_file()
        or payload.stat().st_size != torrent_info.total_size()
        or any(True for _ in torrent_info.trackers())
        or any(True for _ in torrent_info.web_seeds())
    ):
        raise SeedFailure("fixture violates the bounded single-file contract")
    session = lt.session(
        {
            "listen_interfaces": "127.0.0.1:0",
            "enable_dht": False,
            "enable_lsd": False,
            "enable_upnp": False,
            "enable_natpmp": False,
            "enable_incoming_utp": False,
            "enable_outgoing_utp": False,
            "enable_incoming_tcp": True,
            "enable_outgoing_tcp": True,
            "allow_multiple_connections_per_ip": False,
            "connections_limit": 1,
            "alert_queue_size": 512,
            "upload_rate_limit": arguments.upload_rate_limit,
            "ignore_limits_on_local_network": False,
        }
    )
    parameters = lt.add_torrent_params()
    parameters.ti = torrent_info
    parameters.save_path = str(arguments.storage_root)
    parameters.flags |= lt.torrent_flags.seed_mode
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    handle = session.add_torrent(parameters)
    peer_high_water = 0
    try:
        deadline = time.monotonic() + STARTUP_SECONDS
        while time.monotonic() < deadline:
            status = handle.status()
            if status.errc.value() != 0:
                raise SeedFailure(f"seed startup failed: {status.errc.message()}")
            if status.is_seeding and session.is_listening() and session.listen_port() > 0:
                break
            session.pop_alerts()
            time.sleep(POLL_SECONDS)
        else:
            raise SeedFailure("seed did not become ready")
        event(
            {
                "event": "ready",
                "listen": f"127.0.0.1:{session.listen_port()}",
                "libtorrent_version": lt.version,
                "payload_bytes": torrent_info.total_size(),
                "piece_count": torrent_info.num_pieces(),
                "sha1": file_sha1(payload),
                "upload_rate_limit": arguments.upload_rate_limit,
            }
        )

        deadline = time.monotonic() + arguments.timeout_seconds
        uploaded = 0
        while time.monotonic() < deadline:
            status = handle.status()
            if status.errc.value() != 0:
                raise SeedFailure(f"seed transfer failed: {status.errc.message()}")
            peer_high_water = max(peer_high_water, len(handle.get_peer_info()))
            uploaded = int(status.total_payload_upload)
            if uploaded >= torrent_info.total_size():
                time.sleep(2)
                break
            session.pop_alerts()
            time.sleep(POLL_SECONDS)
        else:
            raise SeedFailure("seed transfer timed out")
        if peer_high_water != 1:
            raise SeedFailure("seed did not retain one bounded peer")
        event(
            {
                "event": "complete",
                "uploaded_bytes": uploaded,
                "peer_high_water": peer_high_water,
                "cleanup": "joined",
            }
        )
    finally:
        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def main() -> int:
    try:
        run(parse_arguments())
        return 0
    except (OSError, RuntimeError, ValueError) as error:
        event({"event": "failed", "reason": str(error)[:512]})
        print(f"libtorrent fixture seed failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
