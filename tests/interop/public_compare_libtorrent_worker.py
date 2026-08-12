#!/usr/bin/env python3
"""One-process pinned-libtorrent owner for the public comparison harness."""

from __future__ import annotations

import argparse
import gc
import json
import math
import sys
import time
from pathlib import Path
from typing import Any

import libtorrent as lt

from public_compare_contract import (
    REPORT_SCHEMA_VERSION,
    ContractError,
    comparison_profile,
    normalize_profile,
    parse_metainfo,
)


POLL_SECONDS = 0.005
UTILITY_SAMPLE_SECONDS = 1.0
MAX_UTILITY_SAMPLES = 1024
MAX_ALERTS = 50
DHT_BOOTSTRAP_NODES = ",".join(
    (
        "dht.libtorrent.org:25401",
        "router.bittorrent.com:6881",
        "dht.transmissionbt.com:6881",
    )
)
TARGET_KEYS = {
    "metadata": "metadata_verified",
    "first-piece": "first_piece_verified",
    "10-percent": "10_percent_verified",
    "50-percent": "50_percent_verified",
    "90-percent": "90_percent_verified",
    "95-percent": "95_percent_verified",
    "99-percent": "99_percent_verified",
    "complete": "published",
}


class WorkerError(RuntimeError):
    pass


def libtorrent_settings(profile: str) -> tuple[dict[str, Any], dict[str, Any]]:
    contract = comparison_profile(profile)
    profile = contract["name"]
    symbolic = contract["libtorrent"]
    dht = bool(symbolic["enable_dht"])
    encryption = {
        "disabled": {
            "in_enc_policy": int(lt.enc_policy.pe_disabled),
            "out_enc_policy": int(lt.enc_policy.pe_disabled),
            "allowed_enc_level": int(lt.enc_level.pe_both),
            "prefer_rc4": False,
        },
        "required-rc4": {
            "in_enc_policy": int(lt.enc_policy.pe_forced),
            "out_enc_policy": int(lt.enc_policy.pe_forced),
            "allowed_enc_level": int(lt.enc_level.pe_rc4),
            "prefer_rc4": True,
        },
        "allow": {
            "in_enc_policy": int(lt.enc_policy.pe_enabled),
            "out_enc_policy": int(lt.enc_policy.pe_enabled),
            "allowed_enc_level": int(lt.enc_level.pe_both),
            "prefer_rc4": False,
        },
    }[symbolic["out_enc_policy"] if symbolic["out_enc_policy"] == "disabled" else (
        "required-rc4" if symbolic["out_enc_policy"] == "forced" else "allow"
    )]
    settings = {
        "listen_interfaces": symbolic["listen_interfaces"],
        "enable_dht": dht,
        "dht_bootstrap_nodes": DHT_BOOTSTRAP_NODES if dht else "",
        "enable_lsd": symbolic["enable_lsd"],
        "enable_upnp": symbolic["enable_upnp"],
        "enable_natpmp": symbolic["enable_natpmp"],
        "enable_incoming_utp": symbolic["enable_incoming_utp"],
        "enable_incoming_tcp": symbolic["enable_incoming_tcp"],
        "enable_outgoing_utp": symbolic["enable_outgoing_utp"],
        "enable_outgoing_tcp": symbolic["enable_outgoing_tcp"],
        "connections_limit": symbolic["connections_limit"],
        "connection_speed": symbolic["connection_speed"],
        "peer_connect_timeout": symbolic["peer_connect_timeout"],
        "request_timeout": symbolic["request_timeout"],
        "request_queue_time": symbolic["request_queue_time"],
        "max_out_request_queue": symbolic["max_out_request_queue"],
        "download_rate_limit": symbolic["download_rate_limit"],
        "upload_rate_limit": symbolic["upload_rate_limit"],
        "unchoke_slots_limit": symbolic["unchoke_slots_limit"],
        "max_web_seed_connections": 3 if symbolic["web_seed"] else 0,
        "alert_queue_size": 2000,
        "alert_mask": int(
            lt.alert.category_t.error_notification
            | lt.alert.category_t.peer_notification
            | lt.alert.category_t.status_notification
            | lt.alert.category_t.storage_notification
            | lt.alert.category_t.tracker_notification
            | lt.alert.category_t.dht_notification
            | lt.alert.category_t.performance_warning
        ),
        **encryption,
    }
    capabilities = {
        "network_policy": "online",
        "tracker": symbolic["tracker"],
        "dht": dht,
        "pex": symbolic["pex"],
        "incoming_connections": False,
        "tcp_outgoing": symbolic["enable_outgoing_tcp"],
        "utp_outgoing": symbolic["enable_outgoing_utp"],
        "web_seed": symbolic["web_seed"],
        "websocket_trackers": False,
        "address_families": ["ipv4", "ipv6"],
        "encryption": contract["rstorrent"]["encryption"],
        "incomplete_upload": True,
        "upload_slots": symbolic["unchoke_slots_limit"],
    }
    return settings, capabilities


def append_utility_sample(samples: list[dict[str, Any]], sample: dict[str, Any]) -> int:
    coalesced = 0
    if len(samples) >= MAX_UTILITY_SAMPLES:
        retained = [
            value for index, value in enumerate(samples) if index == 0 or index % 2 == 1
        ]
        coalesced = len(samples) - len(retained)
        samples[:] = retained
    samples.append(sample)
    return coalesced


def integer_distribution(values: list[int]) -> dict[str, int | None]:
    if not values:
        return {"count": 0, "min": None, "median": None, "p90": None, "max": None}
    ordered = sorted(values)
    p90_index = max(0, math.ceil(len(ordered) * 0.9) - 1)
    return {
        "count": len(ordered),
        "min": ordered[0],
        "median": ordered[(len(ordered) - 1) // 2],
        "p90": ordered[p90_index],
        "max": ordered[-1],
    }


def peer_has_flag(peer: Any, flag: Any) -> bool:
    return bool(peer.flags & flag)


def encryption_method(peer: Any) -> str:
    if peer_has_flag(peer, lt.peer_info.rc4_encrypted):
        return "rc4"
    if peer_has_flag(peer, lt.peer_info.plaintext_encrypted):
        return "plaintext-payload"
    return "plaintext-stream"


def transport_method(peer: Any) -> str:
    # libtorrent 2.0's Python binding exposes socket_type_t through the
    # connection_type property but does not export the uTP flag constant.
    connection_type = getattr(peer, "connection_type", 0)
    try:
        encoded = int(connection_type)
    except (TypeError, ValueError):
        encoded = 0
    return "utp" if encoded in (3, 8) else "tcp"


def libtorrent_utility_sample(
    status: Any,
    peers: list[Any],
    elapsed_seconds: float,
    previous_verified: tuple[float, int] | None,
) -> dict[str, Any]:
    verified_bytes = int(status.total_wanted_done)
    verified_rate = None
    if previous_verified is not None:
        previous_at, previous_bytes = previous_verified
        interval = elapsed_seconds - previous_at
        if interval > 0:
            verified_rate = round(max(0, verified_bytes - previous_bytes) / interval)
    connected = [
        peer
        for peer in peers
        if not peer_has_flag(peer, lt.peer_info.connecting)
        and not peer_has_flag(peer, lt.peer_info.handshake)
    ]
    payload_rates = [max(0, int(peer.payload_down_speed)) for peer in connected]
    request_queues = [max(0, int(peer.download_queue_length)) for peer in connected]
    encryption_counts = {
        method: sum(encryption_method(peer) == method for peer in connected)
        for method in ("plaintext-stream", "plaintext-payload", "rc4")
    }
    transport_counts = {
        method: sum(transport_method(peer) == method for peer in connected)
        for method in ("tcp", "utp")
    }
    return {
        "elapsed_seconds": elapsed_seconds,
        "verified_piece_count": int(status.num_pieces),
        "verified_bytes": verified_bytes,
        "verified_rate": verified_rate,
        "tracker_response_batches": None,
        "tracker_reported_peers": None,
        "dht_response_batches": None,
        "dht_reported_peers": None,
        "dial_attempts": None,
        "known_peers": int(status.list_peers),
        "eligible_peers": int(status.connect_candidates),
        "connecting_peers": max(0, int(status.num_connections) - int(status.num_peers)),
        "connected_peers": int(status.num_peers),
        "unchoked_peers": sum(
            not peer_has_flag(peer, lt.peer_info.remote_choked) for peer in connected
        ),
        "wanted_peers": sum(peer_has_flag(peer, lt.peer_info.interesting) for peer in connected),
        "ever_useful_peers": sum(int(peer.total_download) > 0 for peer in connected),
        "active_payload_peers": sum(rate > 0 for rate in payload_rates),
        "stalled_peers": sum(peer_has_flag(peer, lt.peer_info.snubbed) for peer in connected),
        "zero_payload_peers": sum(int(peer.total_download) == 0 for peer in connected),
        "active_requests": sum(request_queues),
        "request_queue_bytes": sum(max(0, int(peer.queue_bytes)) for peer in connected),
        "request_target": None,
        "writing_blocks": None,
        "storage_jobs": None,
        "storage_queue_wait_micros": None,
        "storage_write_service_micros": None,
        "storage_hash_service_micros": None,
        "storage_write_blocks_completed": None,
        "storage_write_batch_blocks_high_water": None,
        "storage_write_batch_bytes_high_water": None,
        "storage_active_kind": None,
        "storage_active_age_micros": None,
        "pending_disk_bytes": sum(max(0, int(peer.pending_disk_bytes)) for peer in connected),
        "payload_rate": max(0, int(status.download_payload_rate)),
        "payload_upload_rate": max(0, int(status.upload_payload_rate)),
        "peer_payload_rates": integer_distribution(payload_rates),
        "peer_request_queues": integer_distribution(request_queues),
        "transport_counts": transport_counts,
        "encryption_counts": encryption_counts,
    }


def empty_milestones() -> dict[str, float | None]:
    return {
        "process_ready": 0.0,
        "torrent_admitted": None,
        "metadata_verified": None,
        "first_candidate": None,
        "first_connection": None,
        "first_payload_byte": None,
        "first_piece_verified": None,
        "10_percent_verified": None,
        "50_percent_verified": None,
        "90_percent_verified": None,
        "95_percent_verified": None,
        "99_percent_verified": None,
        "all_pieces_verified": None,
        "published": None,
        "owner_stopped": None,
        "shutdown_joined": None,
    }


def update_peer_methods(evidence: dict[str, Any], peers: list[Any]) -> None:
    connected = [
        peer
        for peer in peers
        if not peer_has_flag(peer, lt.peer_info.connecting)
        and not peer_has_flag(peer, lt.peer_info.handshake)
    ]
    encryption_counts = {
        method: sum(encryption_method(peer) == method for peer in connected)
        for method in ("plaintext-stream", "plaintext-payload", "rc4")
    }
    transport_counts = {
        method: sum(transport_method(peer) == method for peer in connected)
        for method in ("tcp", "utp")
    }
    evidence["snapshots"] += 1
    evidence["connected_high_water"] = max(evidence["connected_high_water"], len(connected))
    for method in ("tcp", "utp"):
        key = f"{method}_high_water"
        evidence[key] = max(evidence[key], transport_counts[method])
    for method in ("plaintext_stream", "plaintext_payload", "rc4"):
        key = f"{method}_high_water"
        evidence[key] = max(evidence[key], encryption_counts[method.replace("_", "-")])
    useful_payload = sum(max(0, int(peer.total_download)) for peer in connected)
    uploaded_payload = sum(max(0, int(peer.total_upload)) for peer in connected)
    evidence["useful_payload_bytes_high_water"] = max(
        evidence["useful_payload_bytes_high_water"], useful_payload
    )
    evidence["uploaded_payload_bytes_high_water"] = max(
        evidence["uploaded_payload_bytes_high_water"], uploaded_payload
    )
    for peer in connected:
        if int(peer.total_download) > 0:
            key = f"payload_contributor_{encryption_method(peer).replace('-', '_')}"
            evidence[key] = True


def mark_percent_milestones(
    milestones: dict[str, float | None], done: int, total: int, elapsed: float
) -> None:
    for percent in (10, 50, 90, 95, 99):
        key = f"{percent}_percent_verified"
        if key in milestones and milestones[key] is None and done * 100 >= total * percent:
            milestones[key] = elapsed


def _add_parameters(request: dict[str, Any], profile: str, output_root: Path) -> Any:
    input_config = request["input"]
    mode = input_config["mode"]
    descriptor = None
    if mode == "magnet":
        parameters = lt.parse_magnet_uri(input_config["magnet"])
    elif mode == "metainfo":
        metainfo_path = Path(input_config["path"])
        payload = metainfo_path.read_bytes()
        descriptor = parse_metainfo(payload)
        if descriptor.info_hash != request["expected_info_hash"]:
            raise WorkerError("metainfo input does not match expected v1 info hash")
        if descriptor.outer_sha256 != input_config["sha256"]:
            raise WorkerError("metainfo input does not match expected outer SHA-256")
        parameters = lt.add_torrent_params()
        parameters.ti = lt.torrent_info(str(metainfo_path))
    else:
        raise WorkerError(f"unknown input mode {mode!r}")
    parameters.save_path = str(output_root)
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    contract = comparison_profile(profile)["libtorrent"]
    parameters.max_connections = contract["connections_limit"]
    parameters.max_uploads = contract["unchoke_slots_limit"]
    parameters.upload_limit = contract["upload_rate_limit"]
    parameters.download_limit = contract["download_rate_limit"]
    if not contract["pex"]:
        parameters.flags |= lt.torrent_flags.disable_pex
    if not contract["enable_dht"]:
        parameters.flags |= lt.torrent_flags.disable_dht
    parameters.flags |= lt.torrent_flags.disable_lsd
    if not contract["web_seed"]:
        parameters.flags |= lt.torrent_flags.override_web_seeds
        parameters.url_seeds = []
        parameters.http_seeds = []
    if descriptor is not None and profile in ("matched-plain-30", "matched-rc4-30"):
        retained = [
            (tier_index, url)
            for tier_index, tier in enumerate(descriptor.tracker_tiers)
            for url in tier
            if url.lower().startswith(("udp://", "http://", "https://"))
        ]
        parameters.flags |= lt.torrent_flags.override_trackers
        parameters.trackers = [url for _, url in retained]
        parameters.tracker_tiers = [tier for tier, _ in retained]
    if not contract["tracker"]:
        parameters.flags |= lt.torrent_flags.override_trackers
        parameters.trackers = []
        parameters.tracker_tiers = []
    return parameters


def run_libtorrent(request: dict[str, Any]) -> dict[str, Any]:
    started = time.monotonic()
    profile = normalize_profile(request["profile"])
    expected_profile = comparison_profile(profile)
    if request.get("profile_sha256") != expected_profile["sha256"]:
        raise WorkerError("profile SHA-256 mismatch")
    target = request["target"]
    if target not in TARGET_KEYS:
        raise WorkerError(f"unknown target {target!r}")
    timeout_seconds = int(request["timeout_seconds"])
    wire_payload_ceiling = int(request["wire_payload_ceiling_bytes"])
    output_root = Path(request["output_root"])
    settings, capabilities = libtorrent_settings(profile)
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    milestones = empty_milestones()
    geometry = {"total_length": None, "piece_length": None, "piece_count": None, "file_count": None}
    alert_counts: dict[str, int] = {}
    outcome = "error"
    terminal: str | None = None
    verified_pieces = 0
    verified_bytes = 0
    status_metrics: dict[str, Any] = {}
    utility_timeline: list[dict[str, Any]] = []
    utility_timeline_coalesced = 0
    peer_methods = {
        "snapshots": 0,
        "connected_high_water": 0,
        "tcp_high_water": 0,
        "utp_high_water": 0,
        "plaintext_stream_high_water": 0,
        "plaintext_payload_high_water": 0,
        "rc4_high_water": 0,
        "payload_contributor_plaintext_stream": False,
        "payload_contributor_plaintext_payload": False,
        "payload_contributor_rc4": False,
        "useful_payload_bytes_high_water": 0,
        "uploaded_payload_bytes_high_water": 0,
    }
    previous_utility_verified: tuple[float, int] | None = None
    next_utility_sample = 0.0
    last_status: Any | None = None
    cleanup_succeeded = True
    owner_integrity = False
    try:
        output_root.mkdir(parents=True, exist_ok=False)
        session = lt.session(settings)
        parameters = _add_parameters(request, profile, output_root)
        handle = session.add_torrent(parameters)
        for encoded_peer in request.get("peer_hints", []):
            host, encoded_port = encoded_peer.rsplit(":", 1)
            handle.connect_peer((host, int(encoded_port)))
        milestones["torrent_admitted"] = time.monotonic() - started
        deadline = started + timeout_seconds
        while True:
            now = time.monotonic()
            elapsed = now - started
            status = handle.status()
            last_status = status
            for alert in session.pop_alerts():
                name = type(alert).__name__
                if len(alert_counts) < MAX_ALERTS or name in alert_counts:
                    alert_counts[name] = alert_counts.get(name, 0) + 1
            peers = list(handle.get_peer_info())
            update_peer_methods(peer_methods, peers)
            if int(status.list_peers) > 0 and milestones["first_candidate"] is None:
                milestones["first_candidate"] = elapsed
            if int(status.num_peers) > 0 and milestones["first_connection"] is None:
                milestones["first_connection"] = elapsed
            if int(status.total_payload_download) > 0 and milestones["first_payload_byte"] is None:
                milestones["first_payload_byte"] = elapsed
            if status.has_metadata and milestones["metadata_verified"] is None:
                milestones["metadata_verified"] = elapsed
                info = handle.torrent_file()
                if info is None:
                    raise WorkerError("libtorrent reported metadata without torrent_info")
                actual_info_hash = str(info.info_hashes().v1)
                if actual_info_hash != request["expected_info_hash"]:
                    outcome = "integrity_failure"
                    terminal = "metadata info hash did not match the expected identity"
                    break
                geometry = {
                    "total_length": int(info.total_size()),
                    "piece_length": int(info.piece_length()),
                    "piece_count": int(info.num_pieces()),
                    "file_count": int(info.files().num_files()),
                }
            verified_pieces = int(status.num_pieces)
            verified_bytes = int(status.total_wanted_done)
            if verified_pieces > 0 and milestones["first_piece_verified"] is None:
                milestones["first_piece_verified"] = elapsed
            total_wanted = int(status.total_wanted)
            if total_wanted > 0:
                mark_percent_milestones(milestones, verified_bytes, total_wanted, elapsed)
            if status.is_seeding:
                milestones["all_pieces_verified"] = milestones["all_pieces_verified"] or elapsed
                milestones["published"] = milestones["published"] or elapsed
                owner_integrity = True
            physical_download = int(status.total_payload_download)
            if physical_download > wire_payload_ceiling:
                outcome = "resource_bound"
                terminal = "wire payload ceiling exceeded"
                break
            status_metrics = {
                "peers": int(status.num_peers),
                "connections": int(status.num_connections),
                "seeds": int(status.num_seeds),
                "connect_candidates": int(status.connect_candidates),
                "download_payload_rate": int(status.download_payload_rate),
                "upload_payload_rate": int(status.upload_payload_rate),
                "total_payload_download": physical_download,
                "total_payload_upload": int(status.total_payload_upload),
                "failed_bytes": int(status.total_failed_bytes),
                "redundant_bytes": int(status.total_redundant_bytes),
            }
            if status.has_metadata and (
                not utility_timeline or elapsed >= next_utility_sample
            ):
                sample = libtorrent_utility_sample(
                    status, peers, elapsed, previous_utility_verified
                )
                utility_timeline_coalesced += append_utility_sample(
                    utility_timeline, sample
                )
                previous_utility_verified = (elapsed, verified_bytes)
                next_utility_sample = elapsed + UTILITY_SAMPLE_SECONDS
            if milestones[TARGET_KEYS[target]] is not None:
                outcome = "milestone_reached"
                break
            if now >= deadline:
                outcome = "timeout"
                terminal = "target deadline expired"
                break
            time.sleep(POLL_SECONDS)
        if last_status is not None and last_status.has_metadata:
            elapsed = time.monotonic() - started
            utility_timeline_coalesced += append_utility_sample(
                utility_timeline,
                libtorrent_utility_sample(
                    last_status,
                    list(handle.get_peer_info()),
                    elapsed,
                    previous_utility_verified,
                ),
            )
    except (ContractError, WorkerError) as error:
        outcome = "harness_error"
        terminal = str(error)
    except Exception as error:  # The worker must return one bounded owner result.
        outcome = "error"
        terminal = f"{type(error).__name__}: {error}"
    finally:
        try:
            if session is not None and handle is not None and handle.is_valid():
                session.remove_torrent(handle)
            if session is not None:
                session.pause()
            handle = None
            session = None
            gc.collect()
        except Exception as error:
            cleanup_succeeded = False
            terminal = f"cleanup failed: {type(error).__name__}"
            outcome = "harness_error"
    milestones["owner_stopped"] = time.monotonic() - started
    milestones["shutdown_joined"] = milestones["owner_stopped"] if cleanup_succeeded else None
    return {
        "schema_version": REPORT_SCHEMA_VERSION,
        "implementation": "libtorrent",
        "version": lt.version,
        "info_hash": request["expected_info_hash"],
        "profile": profile,
        "profile_sha256": expected_profile["sha256"],
        "effective_settings": expected_profile["libtorrent"],
        "outcome": outcome,
        "target": target,
        "input_mode": request["input"]["mode"],
        "wall_seconds": time.monotonic() - started,
        "milestones": milestones,
        "geometry": geometry,
        "verified_piece_count": verified_pieces,
        "verified_bytes": verified_bytes,
        "owner_integrity_verified": owner_integrity,
        "integrity_verified": False,
        "cleanup_succeeded": cleanup_succeeded,
        "terminal_detail": terminal,
        "capabilities": capabilities,
        "diagnostics": {
            "status": status_metrics,
            "alert_counts": alert_counts,
            "utility_timeline": utility_timeline,
            "utility_timeline_coalesced_samples": utility_timeline_coalesced,
            "peer_methods": peer_methods,
        },
    }


def parse_args(arguments: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--request", type=Path, required=True)
    return parser.parse_args(arguments)


def main(arguments: list[str]) -> int:
    args = parse_args(arguments)
    try:
        request = json.loads(args.request.read_text(encoding="utf-8"))
        if not isinstance(request, dict) or request.get("schema_version") != REPORT_SCHEMA_VERSION:
            raise WorkerError("unknown request schema")
        result = run_libtorrent(request)
    except Exception as error:
        result = {
            "schema_version": REPORT_SCHEMA_VERSION,
            "implementation": "libtorrent",
            "outcome": "harness_error",
            "terminal_detail": f"{type(error).__name__}: {error}",
            "cleanup_succeeded": False,
        }
    print(json.dumps(result, sort_keys=True, separators=(",", ":")))
    return 0 if result.get("outcome") == "milestone_reached" else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
