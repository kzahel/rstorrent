#!/usr/bin/env python3
"""Run one bounded, metadata-only public observation with product uTP."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from public_compare_contract import ContractError, load_catalog_document


ROOT = Path(__file__).resolve().parents[2]
CATALOG = ROOT / "tests" / "live" / "torrents.json"
BINARY_NAME = "rstorrent-public-probe"
TORRENT_SLUG = "big-buck-bunny"
TIMEOUT_SECONDS = 180
CLEANUP_SECONDS = 10
OUTER_TIMEOUT_SECONDS = TIMEOUT_SECONDS + CLEANUP_SECONDS + 15
MAX_BUFFERED_PAYLOAD_BYTES = 64 * 1024 * 1024
WIRE_PAYLOAD_CEILING_BYTES = 512 * 1024 * 1024
MAX_REPORT_BYTES = 32 * 1024 * 1024
FIXED_UTP_MTU_BYTES = 548

EXPECTED_SETTINGS = {
    "network_policy": "online",
    "address_families": ["ipv4", "ipv6"],
    "tracker": True,
    "dht": True,
    "pex": True,
    "lsd": False,
    "upnp": False,
    "natpmp": False,
    "web_seed": False,
    "incoming_connections": False,
    "outgoing_tcp": True,
    "outgoing_utp": True,
    "session_connection_limit": 30,
    "torrent_connection_limit": 30,
    "pending_dial_limit": 30,
    "connection_attempts_per_second": 30,
    "peer_connect_timeout_seconds": 15,
    "request_timeout_seconds": 60,
    "request_queue_time_seconds": 3,
    "max_outgoing_request_queue": 500,
    "download_rate_limit_bytes_per_second": 0,
    "upload_rate_limit_bytes_per_second": 0,
    "upload_slots": 8,
    "encryption": "allow",
}

EXPECTED_CAPABILITIES = {
    "network_policy": "online",
    "tracker": True,
    "dht": True,
    "pex": True,
    "incoming_connections": False,
    "tcp_outgoing": True,
    "utp_outgoing": True,
    "web_seed": False,
    "websocket_trackers": False,
    "address_families": ["ipv4", "ipv6"],
    "encryption": "allow",
    "incomplete_upload": True,
    "upload_slots": 8,
}

PROFILE_DEFINITION = {
    "schema_version": 1,
    "name": "product-utp",
    "target": "metadata",
    "peer_hints": [],
    "timeout_seconds": TIMEOUT_SECONDS,
    "cleanup_seconds": CLEANUP_SECONDS,
    "max_buffered_payload_bytes": MAX_BUFFERED_PAYLOAD_BYTES,
    "wire_payload_ceiling_bytes": WIRE_PAYLOAD_CEILING_BYTES,
    "effective_settings": EXPECTED_SETTINGS,
}
PROFILE_SHA256 = hashlib.sha256(
    json.dumps(PROFILE_DEFINITION, sort_keys=True, separators=(",", ":")).encode()
).hexdigest()


class ObservationError(RuntimeError):
    pass


def build_binary() -> Path:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "-p",
            "rstorrent-engine",
            "--bin",
            BINARY_NAME,
        ],
        cwd=ROOT,
        capture_output=True,
        timeout=300,
        check=False,
    )
    if completed.returncode != 0:
        raise ObservationError("release probe build failed")
    suffix = ".exe" if sys.platform == "win32" else ""
    binary = ROOT / "target" / "release" / f"{BINARY_NAME}{suffix}"
    if not binary.is_file():
        raise ObservationError("release probe binary is missing")
    return binary


def load_torrent() -> dict[str, Any]:
    try:
        catalog = load_catalog_document(CATALOG)
    except ContractError as error:
        raise ObservationError("public torrent catalog is invalid") from error
    entries = [entry for entry in catalog["torrents"] if entry["slug"] == TORRENT_SLUG]
    if len(entries) != 1:
        raise ObservationError("catalog must contain exactly one Big Buck Bunny entry")
    entry = entries[0]
    if "magnet" not in entry or "magnet" not in entry["input_modes"]:
        raise ObservationError("catalog entry does not support the reviewed magnet input")
    return entry


def binary_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def repository_commit() -> str:
    completed = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=10,
        check=False,
    )
    if completed.returncode != 0:
        raise ObservationError("repository commit is unavailable")
    return completed.stdout.strip()


def parse_probe_output(stdout: bytes) -> dict[str, Any]:
    if len(stdout) > MAX_REPORT_BYTES:
        raise ObservationError("probe report exceeded the bounded size")
    try:
        lines = [line for line in stdout.decode("utf-8").splitlines() if line.strip()]
    except UnicodeDecodeError as error:
        raise ObservationError("probe report was not UTF-8") from error
    if len(lines) != 1:
        raise ObservationError("probe did not emit exactly one report")
    try:
        result = json.loads(lines[0])
    except json.JSONDecodeError as error:
        raise ObservationError("probe report was not JSON") from error
    if not isinstance(result, dict):
        raise ObservationError("probe report root was not an object")
    return result


def require_bounded_integer(
    values: dict[str, Any], field: str, maximum: int, *, terminal_zero: bool = False
) -> int:
    value = values.get(field)
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= maximum:
        raise ObservationError(f"probe {field} violated its bound")
    if terminal_zero and value != 0:
        raise ObservationError(f"probe {field} retained terminal work")
    return value


def validate_result(result: dict[str, Any], entry: dict[str, Any], exit_code: int) -> str:
    if (
        result.get("schema_version") != 2
        or result.get("implementation") != "rstorrent"
        or result.get("profile") != "product-utp"
        or result.get("profile_sha256") != PROFILE_SHA256
        or result.get("input_mode") != "magnet"
        or result.get("info_hash") != entry["info_hash"]
        or result.get("target") != "metadata"
    ):
        raise ObservationError("probe identity or profile echo did not match")
    if result.get("effective_settings") != EXPECTED_SETTINGS:
        raise ObservationError("probe effective settings did not match the bounded profile")
    if result.get("capabilities") != EXPECTED_CAPABILITIES:
        raise ObservationError("probe capability report did not match the bounded profile")

    outcome = result.get("outcome")
    allowed_outcomes = {"milestone_reached", "timeout", "error", "resource_bound"}
    if outcome not in allowed_outcomes or result.get("cleanup_succeeded") is not True:
        raise ObservationError("probe did not return a safe evidence outcome")
    if outcome == "milestone_reached":
        if exit_code != 0 or result.get("integrity_verified") is not True:
            raise ObservationError("metadata milestone lacked verified success")
        status = "metadata_reached"
    else:
        if exit_code == 0 or result.get("integrity_verified") is not False:
            raise ObservationError("evidence-limited probe exit did not match its report")
        status = "evidence_limited"

    utp = result.get("utp_evidence")
    if not isinstance(utp, dict):
        raise ObservationError("probe omitted terminal uTP evidence")
    require_bounded_integer(utp, "active_connections_after_shutdown", 0, terminal_zero=True)
    connection_high_water = require_bounded_integer(utp, "connection_high_water", 64)
    require_bounded_integer(utp, "incoming_half_open_after_shutdown", 0, terminal_zero=True)
    require_bounded_integer(utp, "incoming_half_open_high_water", 16)
    require_bounded_integer(utp, "incoming_stream_queue_high_water", 16)
    require_bounded_integer(utp, "connection_datagram_queue_high_water", 64)
    require_bounded_integer(utp, "retransmission_queue_high_water", 1024)
    require_bounded_integer(utp, "delivered_byte_high_water", 1024 * 1024)
    require_bounded_integer(utp, "receive_reorder_packet_high_water", 953)
    require_bounded_integer(utp, "receive_buffered_byte_high_water", 1024 * 1024)
    require_bounded_integer(utp, "receive_window_drop_high_water", 2**64 - 1)
    require_bounded_integer(utp, "unsent_byte_high_water", 1024 * 1024)
    require_bounded_integer(utp, "sent_byte_high_water", 1024 * 1024)
    if not isinstance(utp.get("slow_start_active_observed"), bool):
        raise ObservationError("probe omitted bounded slow-start activity evidence")
    require_bounded_integer(utp, "slow_start_threshold_byte_high_water", 1024 * 1024)
    require_bounded_integer(utp, "slow_start_acknowledgements_high_water", 2**64 - 1)
    require_bounded_integer(utp, "slow_start_exits_high_water", 2**64 - 1)
    require_bounded_integer(utp, "worker_panics", 0, terminal_zero=True)
    for field in (
        "malformed_datagrams",
        "unknown_connection_datagrams",
        "stale_generation_datagrams",
        "connection_datagrams_dropped",
        "datagrams_sent",
        "datagram_bytes_sent",
        "retransmission_datagrams_sent",
        "retransmission_bytes_sent",
    ):
        require_bounded_integer(utp, field, 2**64 - 1)
    mtu_min = utp.get("selected_mtu_min_bytes")
    mtu_max = utp.get("selected_mtu_max_bytes")
    if connection_high_water == 0:
        if mtu_min is not None or mtu_max is not None:
            raise ObservationError("probe reported an MTU without a uTP connection")
    elif mtu_min != FIXED_UTP_MTU_BYTES or mtu_max != FIXED_UTP_MTU_BYTES:
        raise ObservationError("probe departed from the fixed diagnostic uTP MTU")

    udp = result.get("udp_evidence")
    if not isinstance(udp, dict):
        raise ObservationError("probe omitted terminal session UDP evidence")
    require_bounded_integer(udp, "tasks_after_shutdown", 0, terminal_zero=True)
    require_bounded_integer(udp, "task_high_water", 2)
    require_bounded_integer(udp, "queued_after_shutdown", 0, terminal_zero=True)
    require_bounded_integer(udp, "queue_high_water", 64)
    require_bounded_integer(udp, "utp_queued_after_shutdown", 0, terminal_zero=True)
    require_bounded_integer(udp, "utp_queue_high_water", 256)
    for field in (
        "datagrams_received",
        "datagram_bytes_received",
        "datagrams_dropped",
        "dht_datagrams_dropped",
        "utp_datagrams_classified",
        "utp_datagram_bytes_classified",
        "utp_datagrams_dropped",
    ):
        require_bounded_integer(udp, field, 2**64 - 1)

    peer_methods = result.get("diagnostics", {}).get("peer_methods")
    if not isinstance(peer_methods, dict):
        raise ObservationError("probe omitted endpoint-free peer aggregates")
    for field, maximum in (
        ("connected_high_water", 30),
        ("tcp_high_water", 30),
        ("utp_high_water", 30),
        ("utp_unknown_high_water", 1000),
        ("utp_advertised_high_water", 1000),
        ("utp_confirmed_high_water", 1000),
        ("utp_suppressed_high_water", 1000),
        ("utp_suppression_failures_high_water", 255),
    ):
        require_bounded_integer(peer_methods, field, maximum)
    require_bounded_integer(peer_methods, "utp_endpoint_snapshots", 2**64 - 1)
    return status


def endpoint_free_summary(result: dict[str, Any]) -> dict[str, Any]:
    diagnostics = result["diagnostics"]
    return {
        "outcome": result["outcome"],
        "wall_seconds": result["wall_seconds"],
        "integrity_verified": result["integrity_verified"],
        "cleanup_succeeded": result["cleanup_succeeded"],
        "metadata_verified_seconds": result["milestones"].get("metadata_verified"),
        "verified_piece_count": result["verified_piece_count"],
        "verified_bytes": result["verified_bytes"],
        "tracker_response_batches": diagnostics["tracker_response_batches"],
        "tracker_reported_peers": diagnostics["tracker_reported_peers"],
        "peer_dial_attempts": diagnostics["peer_dial_attempts"],
        "peer_methods": diagnostics["peer_methods"],
        "dht_evidence": result["dht_evidence"],
        "utp_evidence": result["utp_evidence"],
        "udp_evidence": result["udp_evidence"],
    }


def artifact_summary(root: Path) -> dict[str, int]:
    files = [path for path in root.rglob("*") if path.is_file()]
    return {
        "regular_files_before_cleanup": len(files),
        "regular_file_bytes_before_cleanup": sum(path.stat().st_size for path in files),
    }


def run_observation(binary: Path, entry: dict[str, Any]) -> dict[str, Any]:
    temporary_path: Path | None = None
    with tempfile.TemporaryDirectory(prefix="rstorrent-utp-public-") as temporary:
        temporary_path = Path(temporary)
        output = temporary_path / "payload"
        command = [
            str(binary),
            "--magnet",
            entry["magnet"],
            "--expected-info-hash",
            entry["info_hash"],
            "--output",
            str(output),
            "--profile",
            "product-utp",
            "--profile-sha256",
            PROFILE_SHA256,
            "--target",
            "metadata",
            "--timeout-seconds",
            str(TIMEOUT_SECONDS),
            "--cleanup-seconds",
            str(CLEANUP_SECONDS),
            "--max-buffered-payload-bytes",
            str(MAX_BUFFERED_PAYLOAD_BYTES),
            "--wire-payload-ceiling-bytes",
            str(WIRE_PAYLOAD_CEILING_BYTES),
        ]
        try:
            completed = subprocess.run(
                command,
                cwd=ROOT,
                capture_output=True,
                timeout=OUTER_TIMEOUT_SECONDS,
                check=False,
            )
        except subprocess.TimeoutExpired as error:
            raise ObservationError("probe exceeded its outer owner deadline") from error
        result = parse_probe_output(completed.stdout)
        status = validate_result(result, entry, completed.returncode)
        artifacts = artifact_summary(temporary_path)
        process = {
            "exit_code": completed.returncode,
            "stderr_bytes": len(completed.stderr),
            "stderr_sha256": (
                hashlib.sha256(completed.stderr).hexdigest() if completed.stderr else None
            ),
        }
        summary = endpoint_free_summary(result)
    if temporary_path is None or temporary_path.exists():
        raise ObservationError("fresh temporary observation root was not removed")
    return {
        "schema_version": 1,
        "status": status,
        "observed_at_utc": datetime.now(timezone.utc).isoformat(),
        "repository_commit": repository_commit(),
        "torrent": {
            "slug": entry["slug"],
            "info_hash": entry["info_hash"],
            "catalog_retrieved": entry["source"]["retrieved"],
        },
        "profile": PROFILE_DEFINITION,
        "profile_sha256": PROFILE_SHA256,
        "binary_sha256": binary_sha256(binary),
        "process": process,
        "artifacts": {**artifacts, "temporary_root_removed": True},
        "observation": summary,
    }


def parse_args(arguments: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--allow-public-network",
        action="store_true",
        help="authorize the single bounded public BitTorrent observation",
    )
    args = parser.parse_args(arguments)
    if not args.allow_public_network:
        parser.error("public execution requires --allow-public-network")
    return args


def main(arguments: list[str]) -> int:
    parse_args(arguments)
    try:
        report = run_observation(build_binary(), load_torrent())
    except ObservationError as error:
        print(
            json.dumps(
                {
                    "schema_version": 1,
                    "status": "harness_error",
                    "detail": str(error),
                },
                sort_keys=True,
            )
        )
        return 1
    print(json.dumps(report, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
