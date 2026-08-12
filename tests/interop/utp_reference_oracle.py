#!/usr/bin/env python3
"""Run a bounded, forced-uTP transfer between two pinned libtorrent sessions."""

from __future__ import annotations

import gc
import hashlib
import json
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

import libtorrent as lt


PAYLOAD_NAME = "payload.bin"
PAYLOAD_SIZE = 2 * 1024 * 1024 + 731
PIECE_SIZE = 64 * 1024
SOURCE_CHUNK_SIZE = 64 * 1024
POLL_SECONDS = 0.02
SCENARIO_TIMEOUT_SECONDS = 30.0
STATS_TIMEOUT_SECONDS = 2.0
MAX_DIAGNOSTICS = 50

TRANSPORT_SETTINGS: dict[str, Any] = {
    "listen_interfaces": "127.0.0.1:0",
    "enable_dht": False,
    "enable_lsd": False,
    "enable_upnp": False,
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
    "alert_queue_size": 256,
    "alert_mask": int(
        lt.alert.category_t.error_notification
        | lt.alert.category_t.stats_notification
    ),
}

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


class OracleFailure(RuntimeError):
    pass


def write_payload(path: Path, payload_size: int = PAYLOAD_SIZE) -> str:
    if payload_size <= 0:
        raise OracleFailure("controlled payload size must be positive")
    digest = hashlib.sha1()
    remaining = payload_size
    chunk_index = 0
    with path.open("xb") as output:
        while remaining:
            length = min(SOURCE_CHUNK_SIZE, remaining)
            chunk = bytes(
                (
                    (offset * 73)
                    ^ (offset >> 3)
                    ^ (chunk_index * 29)
                    ^ 0xA5
                )
                & 0xFF
                for offset in range(length)
            )
            output.write(chunk)
            digest.update(chunk)
            remaining -= length
            chunk_index += 1
    return digest.hexdigest()


def hash_file(path: Path) -> str:
    digest = hashlib.sha1()
    with path.open("rb") as source:
        while chunk := source.read(SOURCE_CHUNK_SIZE):
            digest.update(chunk)
    return digest.hexdigest()


def create_fixture(
    root: Path,
    *,
    payload_size: int = PAYLOAD_SIZE,
    piece_size: int = PIECE_SIZE,
) -> tuple[lt.torrent_info, Path, str]:
    if payload_size <= 0 or piece_size <= 0:
        raise OracleFailure("controlled fixture geometry must be positive")
    seed_root = root / "seed"
    seed_root.mkdir()
    expected_sha1 = write_payload(seed_root / PAYLOAD_NAME, payload_size)
    files = lt.file_storage()
    files.add_file(PAYLOAD_NAME, payload_size)
    creator = lt.create_torrent(
        files,
        piece_size=piece_size,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(seed_root))
    torrent_path = root / "forced-utp.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    torrent_info = lt.torrent_info(str(torrent_path))
    if (
        torrent_info.total_size() != payload_size
        or torrent_info.piece_length() != piece_size
    ):
        raise OracleFailure("controlled torrent has the wrong payload size")
    if any(True for _ in torrent_info.trackers()):
        raise OracleFailure("controlled torrent unexpectedly contains a tracker")
    return torrent_info, seed_root, expected_sha1


def create_session() -> lt.session:
    return lt.session(dict(TRANSPORT_SETTINGS))


def collect_alerts(session: lt.session, diagnostics: list[str]) -> list[Any]:
    alerts = session.pop_alerts()
    for alert in alerts:
        if isinstance(
            alert, (lt.session_stats_alert, lt.session_stats_header_alert)
        ):
            continue
        diagnostics.append(alert.message())
    del diagnostics[:-MAX_DIAGNOSTICS]
    return alerts


def wait_until_ready(
    session: lt.session,
    handle: lt.torrent_handle,
    *,
    seed: bool,
    deadline: float,
    diagnostics: list[str],
) -> int:
    while time.monotonic() < deadline:
        collect_alerts(session, diagnostics)
        status = handle.status()
        if status.errc.value() != 0:
            raise OracleFailure(f"libtorrent failed: {status.errc.message()}")
        state_ready = status.is_seeding if seed else not status.is_seeding
        if state_ready and session.is_listening() and session.listen_port() > 0:
            return session.listen_port()
        time.sleep(POLL_SECONDS)
    role = "seed" if seed else "leecher"
    raise OracleFailure(f"{role} did not become ready before the scenario deadline")


def add_torrent(
    session: lt.session,
    torrent_info: lt.torrent_info,
    save_path: Path,
    *,
    seed: bool,
) -> lt.torrent_handle:
    parameters = lt.add_torrent_params()
    parameters.ti = torrent_info
    parameters.save_path = str(save_path)
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    if seed:
        parameters.flags |= lt.torrent_flags.seed_mode
    return session.add_torrent(parameters)


def stats_snapshot(
    session: lt.session,
    diagnostics: list[str],
    scenario_deadline: float,
) -> dict[str, int]:
    available_metrics = {
        metric.name
        for metric in lt.session_stats_metrics()
        if metric.name in STATS_NAMES
    }
    missing = set(STATS_NAMES) - available_metrics
    if missing:
        raise OracleFailure(f"libtorrent lacks required metrics: {sorted(missing)}")
    session.post_session_stats()
    deadline = min(
        scenario_deadline,
        time.monotonic() + STATS_TIMEOUT_SECONDS,
    )
    while time.monotonic() < deadline:
        for alert in collect_alerts(session, diagnostics):
            if isinstance(alert, lt.session_stats_alert):
                return {
                    name: int(alert.values[name])
                    for name in available_metrics
                }
        time.sleep(POLL_SECONDS)
    raise OracleFailure("libtorrent did not post session statistics")


def peer_addresses(handle: lt.torrent_handle) -> list[str]:
    addresses: list[str] = []
    for peer in handle.get_peer_info():
        endpoint = peer.ip
        address = str(endpoint[0] if isinstance(endpoint, tuple) else endpoint)
        addresses.append(address)
        if address != "127.0.0.1":
            raise OracleFailure(f"non-loopback peer observed: {address}")
    return addresses


def run() -> dict[str, Any]:
    started = time.monotonic()
    deadline = started + SCENARIO_TIMEOUT_SECONDS
    seed_session: lt.session | None = None
    leech_session: lt.session | None = None
    seed_handle: lt.torrent_handle | None = None
    leech_handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    cleanup_started = 0.0
    cleanup_seconds = 0.0
    result: dict[str, Any] | None = None

    with tempfile.TemporaryDirectory(prefix="rstorrent-utp-oracle-") as temporary:
        root = Path(temporary)
        torrent_info, seed_root, expected_sha1 = create_fixture(root)
        leech_root = root / "leech"
        leech_root.mkdir()
        try:
            seed_session = create_session()
            leech_session = create_session()
            seed_handle = add_torrent(
                seed_session, torrent_info, seed_root, seed=True
            )
            leech_handle = add_torrent(
                leech_session, torrent_info, leech_root, seed=False
            )
            seed_port = wait_until_ready(
                seed_session,
                seed_handle,
                seed=True,
                deadline=deadline,
                diagnostics=diagnostics,
            )
            wait_until_ready(
                leech_session,
                leech_handle,
                seed=False,
                deadline=deadline,
                diagnostics=diagnostics,
            )
            leech_handle.connect_peer(("127.0.0.1", seed_port))

            peer_high_water = {"seed": 0, "leecher": 0}
            observed_peer_addresses: set[str] = set()
            while time.monotonic() < deadline:
                collect_alerts(seed_session, diagnostics)
                collect_alerts(leech_session, diagnostics)
                seed_peers = peer_addresses(seed_handle)
                leech_peers = peer_addresses(leech_handle)
                observed_peer_addresses.update(seed_peers)
                observed_peer_addresses.update(leech_peers)
                peer_high_water["seed"] = max(
                    peer_high_water["seed"], len(seed_peers)
                )
                peer_high_water["leecher"] = max(
                    peer_high_water["leecher"], len(leech_peers)
                )
                status = leech_handle.status()
                if status.errc.value() != 0:
                    raise OracleFailure(
                        f"leecher failed: {status.errc.message()}"
                    )
                if status.is_seeding:
                    break
                time.sleep(POLL_SECONDS)
            else:
                raise OracleFailure("forced-uTP transfer exceeded 30 seconds")

            downloaded_path = leech_root / PAYLOAD_NAME
            if not downloaded_path.is_file():
                raise OracleFailure("downloaded payload is missing")
            actual_sha1 = hash_file(downloaded_path)
            if actual_sha1 != expected_sha1:
                raise OracleFailure("downloaded payload hash does not match source")
            if downloaded_path.stat().st_size != PAYLOAD_SIZE:
                raise OracleFailure("downloaded payload has the wrong size")
            transfer_seconds = time.monotonic() - started

            seed_stats = stats_snapshot(seed_session, diagnostics, deadline)
            leech_stats = stats_snapshot(leech_session, diagnostics, deadline)
            for role, snapshot in (("seed", seed_stats), ("leecher", leech_stats)):
                if snapshot["peer.num_tcp_peers"] != 0:
                    raise OracleFailure(f"{role} observed a TCP peer")
                if snapshot["utp.utp_packets_in"] <= 0:
                    raise OracleFailure(f"{role} received no uTP packets")
                if snapshot["utp.utp_packets_out"] <= 0:
                    raise OracleFailure(f"{role} sent no uTP packets")
            if peer_high_water != {"seed": 1, "leecher": 1}:
                raise OracleFailure(
                    f"unexpected peer high-water marks: {peer_high_water}"
                )
            if observed_peer_addresses != {"127.0.0.1"}:
                raise OracleFailure(
                    "did not observe exactly one loopback peer address"
                )

            result = {
                "schema_version": 1,
                "oracle": "forced-utp-loopback",
                "libtorrent_version": lt.__version__,
                "transport": {
                    "tcp_incoming": False,
                    "tcp_outgoing": False,
                    "utp_incoming": True,
                    "utp_outgoing": True,
                    "dht": False,
                    "lsd": False,
                    "natpmp": False,
                    "upnp": False,
                    "peer_addresses": sorted(observed_peer_addresses),
                },
                "payload": {
                    "bytes": PAYLOAD_SIZE,
                    "piece_bytes": PIECE_SIZE,
                    "sha1": actual_sha1,
                },
                "peer_high_water": peer_high_water,
                "session_stats": {
                    "seed": seed_stats,
                    "leecher": leech_stats,
                },
                "transfer_seconds": round(transfer_seconds, 6),
            }
        finally:
            cleanup_started = time.monotonic()
            for session, handle in (
                (seed_session, seed_handle),
                (leech_session, leech_handle),
            ):
                if session is not None and handle is not None and handle.is_valid():
                    session.remove_torrent(handle)
                if session is not None:
                    session.pause()
            seed_handle = None
            leech_handle = None
            seed_session = None
            leech_session = None
            session = None
            handle = None
            gc.collect()
            cleanup_seconds = time.monotonic() - cleanup_started

    if result is None:
        raise OracleFailure("forced-uTP oracle produced no result")
    total_seconds = time.monotonic() - started
    if total_seconds > SCENARIO_TIMEOUT_SECONDS:
        raise OracleFailure("forced-uTP oracle exceeded 30 seconds including cleanup")
    result["cleanup"] = {
        "succeeded": True,
        "seconds": round(cleanup_seconds, 6),
        "temporary_directory_removed": True,
    }
    result["total_seconds"] = round(total_seconds, 6)
    result["diagnostics"] = diagnostics
    return result


def main() -> int:
    print(json.dumps(run(), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except OracleFailure as error:
        print(f"uTP reference oracle failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
