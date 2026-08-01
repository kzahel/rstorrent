#!/usr/bin/env python3
"""Run bounded, alternating RSTorrent/libtorrent public-swarm comparisons."""

from __future__ import annotations

import argparse
import gc
import json
import math
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

import libtorrent as lt


SCHEMA_VERSION = 1
TARGETS = (
    "metadata",
    "first-piece",
    "50-percent",
    "95-percent",
    "99-percent",
    "complete",
)
PROFILES = ("common", "dht", "full-reference")
OWNERS = ("both", "rstorrent", "libtorrent")
MILESTONE_KEYS = {
    "metadata": "metadata_verified",
    "first-piece": "first_piece_verified",
    "50-percent": "50_percent_verified",
    "95-percent": "95_percent_verified",
    "99-percent": "99_percent_verified",
    "complete": "published",
}
MAX_RUNS = 100
MAX_TIMEOUT_SECONDS = 24 * 60 * 60
MAX_DIAGNOSTIC_CHARS = 16_384
POLL_SECONDS = 0.05
UTILITY_SAMPLE_SECONDS = 1.0
MAX_UTILITY_SAMPLES = 1024
DHT_BOOTSTRAP_NODES = ",".join(
    (
        "dht.libtorrent.org:25401",
        "router.bittorrent.com:6881",
        "dht.transmissionbt.com:6881",
    )
)


class HarnessError(RuntimeError):
    pass


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def default_catalog_path() -> Path:
    return repository_root() / "tests" / "live" / "torrents.json"


def load_catalog(path: Path) -> dict[str, Any]:
    try:
        catalog = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise HarnessError(f"could not read catalog {path}: {error}") from error
    if not isinstance(catalog, dict) or catalog.get("schema_version") != 1:
        raise HarnessError("catalog schema_version must be 1")
    torrents = catalog.get("torrents")
    if not isinstance(torrents, list) or not torrents:
        raise HarnessError("catalog torrents must be a nonempty list")
    seen: set[str] = set()
    for entry in torrents:
        validate_catalog_entry(entry, seen)
    return catalog


def validate_catalog_entry(entry: Any, seen: set[str]) -> None:
    if not isinstance(entry, dict):
        raise HarnessError("catalog torrent entries must be objects")
    slug = entry.get("slug")
    name = entry.get("name")
    info_hash = entry.get("info_hash")
    magnet = entry.get("magnet")
    if not isinstance(slug, str) or not slug or slug in seen:
        raise HarnessError(f"invalid or duplicate catalog slug {slug!r}")
    seen.add(slug)
    if not isinstance(name, str) or not name:
        raise HarnessError(f"catalog entry {slug} has no name")
    if (
        not isinstance(info_hash, str)
        or len(info_hash) != 40
        or any(character not in "0123456789abcdef" for character in info_hash)
    ):
        raise HarnessError(f"catalog entry {slug} has an invalid lowercase v1 info hash")
    if not isinstance(magnet, str) or not magnet.startswith("magnet:?"):
        raise HarnessError(f"catalog entry {slug} has an invalid magnet")
    parameters = parse_qsl(urlsplit(magnet).query, keep_blank_values=True)
    expected_xt = f"urn:btih:{info_hash}"
    if ("xt", expected_xt) not in parameters:
        raise HarnessError(f"catalog entry {slug} magnet does not match its info hash")
    for key in ("payload_bytes", "piece_count", "file_count"):
        value = entry.get(key)
        if value is not None and (not isinstance(value, int) or value <= 0):
            raise HarnessError(f"catalog entry {slug} has invalid {key}")


def select_torrent(catalog: dict[str, Any], slug: str) -> dict[str, Any]:
    for entry in catalog["torrents"]:
        if entry["slug"] == slug:
            return entry
    choices = ", ".join(entry["slug"] for entry in catalog["torrents"])
    raise HarnessError(f"unknown torrent {slug!r}; choose one of: {choices}")


def scenario_magnets(entry: dict[str, Any], profile: str) -> tuple[str, str]:
    source = entry["magnet"]
    if profile == "full-reference":
        return source, source
    split = urlsplit(source)
    retained: list[tuple[str, str]] = []
    for key, value in parse_qsl(split.query, keep_blank_values=True):
        if key in ("xt", "dn"):
            retained.append((key, value))
        elif profile == "common" and key == "tr" and value.lower().startswith("udp://"):
            retained.append((key, value))
    magnet = urlunsplit((split.scheme, split.netloc, split.path, urlencode(retained), ""))
    return magnet, magnet


def implementation_order(ordinal: int) -> list[str]:
    return ["rstorrent", "libtorrent"] if ordinal % 2 == 0 else ["libtorrent", "rstorrent"]


def selected_implementations(ordinal: int, owner: str) -> list[str]:
    return implementation_order(ordinal) if owner == "both" else [owner]


def classify_pair(rstorrent: dict[str, Any], libtorrent: dict[str, Any]) -> str:
    outcomes = (rstorrent.get("outcome"), libtorrent.get("outcome"))
    if "harness_error" in outcomes:
        return "harness_error"
    rst_reached = outcomes[0] == "milestone_reached"
    lib_reached = outcomes[1] == "milestone_reached"
    if rst_reached and lib_reached:
        return "both_reached"
    if lib_reached:
        return "reference_only"
    if rst_reached:
        return "rstorrent_only"
    return "both_incomplete"


def classify_owner(result: dict[str, Any]) -> str:
    outcome = result.get("outcome")
    if outcome == "harness_error":
        return "harness_error"
    return "owner_reached" if outcome == "milestone_reached" else "owner_incomplete"


def milestone_seconds(result: dict[str, Any], target: str) -> float | None:
    value = result.get("milestones", {}).get(MILESTONE_KEYS[target])
    return float(value) if isinstance(value, (int, float)) and value >= 0 else None


def summarize(
    runs: list[dict[str, Any]], target: str, owner: str = "both"
) -> dict[str, Any]:
    if owner != "both":
        classifications = {
            name: sum(run["classification"] == name for run in runs)
            for name in ("owner_reached", "owner_incomplete", "harness_error")
        }
        times = [
            seconds
            for run in runs
            if run["classification"] == "owner_reached"
            for seconds in [milestone_seconds(run["implementations"][owner], target)]
            if seconds is not None
        ]
        return {
            "attempts": len(runs),
            "owner": owner,
            "classifications": classifications,
            "milestone_samples": len(times),
            "owner_seconds": distribution(times),
        }
    classifications = {
        name: sum(run["classification"] == name for run in runs)
        for name in (
            "both_reached",
            "reference_only",
            "rstorrent_only",
            "both_incomplete",
            "harness_error",
        )
    }
    ratios: list[float] = []
    rst_times: list[float] = []
    lib_times: list[float] = []
    for run in runs:
        if run["classification"] != "both_reached":
            continue
        rst = milestone_seconds(run["implementations"]["rstorrent"], target)
        lib = milestone_seconds(run["implementations"]["libtorrent"], target)
        if rst is None or lib is None or lib <= 0:
            continue
        rst_times.append(rst)
        lib_times.append(lib)
        ratios.append(rst / lib)
    return {
        "attempts": len(runs),
        "classifications": classifications,
        "comparable_samples": len(ratios),
        "rstorrent_seconds": distribution(rst_times),
        "libtorrent_seconds": distribution(lib_times),
        "rstorrent_over_libtorrent": distribution(ratios),
    }


def distribution(values: list[float]) -> dict[str, float | int | None]:
    if not values:
        return {"count": 0, "min": None, "median": None, "mean": None, "p90": None, "max": None}
    ordered = sorted(values)
    p90_index = max(0, math.ceil(len(ordered) * 0.9) - 1)
    return {
        "count": len(ordered),
        "min": ordered[0],
        "median": statistics.median(ordered),
        "mean": statistics.fmean(ordered),
        "p90": ordered[p90_index],
        "max": ordered[-1],
    }


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
        "wanted_peers": sum(
            peer_has_flag(peer, lt.peer_info.interesting) for peer in connected
        ),
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
        "storage_active_kind": None,
        "storage_active_age_micros": None,
        "pending_disk_bytes": sum(
            max(0, int(peer.pending_disk_bytes)) for peer in connected
        ),
        "payload_rate": max(0, int(status.download_payload_rate)),
        "peer_payload_rates": integer_distribution(payload_rates),
        "peer_request_queues": integer_distribution(request_queues),
    }


def peer_has_flag(peer: Any, flag: Any) -> bool:
    return bool(peer.flags & flag)


def build_probe(repository: Path) -> Path:
    completed = subprocess.run(
        ["cargo", "build", "-p", "rstorrent-engine", "--bin", "rstorrent-public-probe"],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )
    if completed.returncode != 0:
        raise HarnessError(
            "failed to build RSTorrent probe\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    binary = repository / "target" / "debug" / "rstorrent-public-probe"
    if not binary.is_file():
        raise HarnessError(f"RSTorrent probe was not created at {binary}")
    return binary


def environment_snapshot(repository: Path) -> dict[str, Any]:
    return {
        "repository_commit": command_text(["git", "rev-parse", "HEAD"], repository),
        "repository_dirty": bool(command_text(["git", "status", "--porcelain"], repository)),
        "platform": platform.platform(),
        "architecture": platform.machine(),
        "python": platform.python_version(),
        "rustc": command_text(["rustc", "--version"], repository),
        "libtorrent": lt.version,
    }


def command_text(command: list[str], cwd: Path) -> str | None:
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return completed.stdout.strip() if completed.returncode == 0 else None


def run_rstorrent(
    binary: Path,
    magnet: str,
    profile: str,
    target: str,
    timeout_seconds: int,
    cleanup_seconds: int,
    output_root: Path,
) -> dict[str, Any]:
    discovery = {"common": "tracker", "dht": "dht", "full-reference": "full"}[profile]
    command = [
        str(binary),
        "--magnet",
        magnet,
        "--output",
        str(output_root),
        "--discovery",
        discovery,
        "--target",
        target,
        "--timeout-seconds",
        str(timeout_seconds),
        "--cleanup-seconds",
        str(cleanup_seconds),
    ]
    started = time.monotonic()
    try:
        completed = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=timeout_seconds + cleanup_seconds * 2 + 10,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        return harness_failure("rstorrent", time.monotonic() - started, f"outer process timeout: {error}")
    lines = [line for line in completed.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        return harness_failure(
            "rstorrent",
            time.monotonic() - started,
            f"probe emitted {len(lines)} nonempty stdout lines; stderr={bounded(completed.stderr)!r}",
        )
    try:
        result = json.loads(lines[0])
    except json.JSONDecodeError as error:
        return harness_failure("rstorrent", time.monotonic() - started, f"invalid probe JSON: {error}")
    if not isinstance(result, dict) or result.get("schema_version") != 1:
        return harness_failure("rstorrent", time.monotonic() - started, "probe returned an unknown schema")
    result["process"] = {
        "exit_code": completed.returncode,
        "stderr": bounded(completed.stderr) or None,
        "peak_rss_bytes": None,
        "cpu_seconds": None,
    }
    return result


def libtorrent_settings(profile: str) -> tuple[dict[str, Any], dict[str, Any]]:
    common = profile != "full-reference"
    dht = profile in ("dht", "full-reference")
    settings = {
        "listen_interfaces": "0.0.0.0:0",
        "enable_dht": dht,
        "dht_bootstrap_nodes": DHT_BOOTSTRAP_NODES if dht else "",
        "enable_lsd": False,
        "enable_upnp": False,
        "enable_natpmp": False,
        "enable_incoming_utp": False,
        "enable_incoming_tcp": False,
        "enable_outgoing_utp": not common,
        "enable_outgoing_tcp": True,
        "connections_limit": 200,
        "alert_queue_size": 2000,
    }
    capabilities = {
        "network_policy": "online",
        "udp_trackers": profile != "dht",
        "dht": dht,
        "incoming_connections": False,
        "tcp_outgoing": True,
        "utp_outgoing": not common,
        "web_seed": profile == "full-reference",
        "websocket_trackers": False,
        "pex": profile == "full-reference",
    }
    return settings, capabilities


def run_libtorrent(
    magnet: str,
    expected_info_hash: str,
    profile: str,
    target: str,
    timeout_seconds: int,
    output_root: Path,
) -> dict[str, Any]:
    started = time.monotonic()
    settings, capabilities = libtorrent_settings(profile)
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    milestones = empty_milestones()
    geometry = empty_geometry()
    alerts: list[str] = []
    outcome = "error"
    terminal: str | None = None
    integrity = False
    verified_pieces = 0
    verified_bytes = 0
    status_metrics: dict[str, Any] = {}
    utility_timeline: list[dict[str, Any]] = []
    utility_timeline_coalesced = 0
    previous_utility_verified: tuple[float, int] | None = None
    next_utility_sample = 0.0
    last_status: Any | None = None
    cleanup_succeeded = True
    try:
        output_root.mkdir(parents=True, exist_ok=False)
        session = lt.session(settings)
        parameters = lt.parse_magnet_uri(magnet)
        parameters.save_path = str(output_root)
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        if profile != "full-reference":
            parameters.flags |= lt.torrent_flags.disable_lsd
            parameters.flags |= lt.torrent_flags.disable_pex
            if profile == "common":
                parameters.flags |= lt.torrent_flags.disable_dht
        handle = session.add_torrent(parameters)
        deadline = started + timeout_seconds
        while True:
            now = time.monotonic()
            status = handle.status()
            last_status = status
            alerts.extend(bounded(alert.message(), 512) for alert in session.pop_alerts())
            del alerts[:-50]
            if status.has_metadata and milestones["metadata_verified"] is None:
                milestones["metadata_verified"] = now - started
                info = handle.torrent_file()
                if info is None:
                    raise HarnessError("libtorrent reported metadata without torrent_info")
                actual_info_hash = str(info.info_hashes().v1)
                if actual_info_hash != expected_info_hash:
                    outcome = "integrity_failure"
                    terminal = (
                        f"metadata info hash {actual_info_hash} did not match "
                        f"{expected_info_hash}"
                    )
                    break
                geometry = {
                    "total_length": int(info.total_size()),
                    "piece_length": int(info.piece_length()),
                    "piece_count": int(info.num_pieces()),
                    "file_count": int(info.files().num_files()),
                }
            verified_pieces = int(status.num_pieces)
            verified_bytes = int(status.total_wanted_done)
            if status.num_pieces > 0 and milestones["first_piece_verified"] is None:
                milestones["first_piece_verified"] = now - started
            total_wanted = int(status.total_wanted)
            if total_wanted > 0:
                mark_percent_milestones(milestones, verified_bytes, total_wanted, now - started)
            if status.is_seeding:
                info = handle.torrent_file()
                if info is None:
                    raise HarnessError("libtorrent seeded without torrent_info")
                verify_libtorrent_publication(info, output_root)
                milestones["all_pieces_verified"] = milestones["all_pieces_verified"] or now - started
                milestones["published"] = milestones["published"] or now - started
            status_metrics = {
                "peers": int(status.num_peers),
                "seeds": int(status.num_seeds),
                "connect_candidates": int(status.connect_candidates),
                "download_rate": int(status.download_payload_rate),
                "total_payload_download": int(status.total_payload_download),
                "failed_bytes": int(status.total_failed_bytes),
                "redundant_bytes": int(status.total_redundant_bytes),
            }
            elapsed = now - started
            if status.has_metadata and (
                not utility_timeline or elapsed >= next_utility_sample
            ):
                sample = libtorrent_utility_sample(
                    status,
                    list(handle.get_peer_info()),
                    elapsed,
                    previous_utility_verified,
                )
                utility_timeline_coalesced += append_utility_sample(
                    utility_timeline, sample
                )
                previous_utility_verified = (elapsed, int(status.total_wanted_done))
                next_utility_sample = elapsed + UTILITY_SAMPLE_SECONDS
            if milestones[MILESTONE_KEYS[target]] is not None:
                outcome = "milestone_reached"
                integrity = True
                break
            if now >= deadline:
                outcome = "timeout"
                terminal = "target deadline expired"
                break
            time.sleep(POLL_SECONDS)
        if last_status is not None and last_status.has_metadata:
            elapsed = time.monotonic() - started
            sample = libtorrent_utility_sample(
                last_status,
                list(handle.get_peer_info()),
                elapsed,
                previous_utility_verified,
            )
            utility_timeline_coalesced += append_utility_sample(utility_timeline, sample)
    except Exception as error:  # Public harness records owner errors instead of aborting its pair.
        outcome = "harness_error" if isinstance(error, HarnessError) else "error"
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
            terminal = f"{terminal}; cleanup: {error}" if terminal else f"cleanup: {error}"
            outcome = "harness_error"
    return {
        "schema_version": 1,
        "implementation": "libtorrent",
        "version": lt.version,
        "info_hash": expected_info_hash,
        "outcome": outcome,
        "target": target,
        "wall_seconds": time.monotonic() - started,
        "milestones": milestones,
        "geometry": geometry,
        "verified_piece_count": verified_pieces,
        "verified_bytes": verified_bytes,
        "integrity_verified": integrity,
        "cleanup_succeeded": cleanup_succeeded,
        "terminal_detail": terminal,
        "capabilities": capabilities,
        "diagnostics": {
            "status": status_metrics,
            "alerts": alerts,
            "utility_timeline": utility_timeline,
            "utility_timeline_coalesced_samples": utility_timeline_coalesced,
            "storage_write_operations_started": None,
            "storage_write_operations_completed": None,
            "storage_write_queue_wait_micros": None,
            "storage_write_queue_wait_max_micros": None,
            "storage_write_service_micros": None,
            "storage_write_service_max_micros": None,
            "storage_hash_operations_started": None,
            "storage_hash_operations_completed": None,
            "storage_hash_queue_wait_micros": None,
            "storage_hash_queue_wait_max_micros": None,
            "storage_hash_service_micros": None,
            "storage_hash_service_max_micros": None,
            "storage_active_kind": None,
            "storage_active_age_micros": None,
        },
        "process": {"peak_rss_bytes": None, "cpu_seconds": None},
    }


def verify_libtorrent_publication(info: lt.torrent_info, output_root: Path) -> None:
    files = info.files()
    for index in range(files.num_files()):
        if files.file_flags(index) & files.flag_pad_file:
            continue
        relative = Path(files.file_path(index))
        if relative.is_absolute() or ".." in relative.parts:
            raise HarnessError(f"libtorrent metadata exposed unsafe file path {relative}")
        published = output_root / relative
        expected_size = int(files.file_size(index))
        if not published.is_file() or published.stat().st_size != expected_size:
            raise HarnessError(
                f"libtorrent publication mismatch for {relative}: expected {expected_size} bytes"
            )


def empty_milestones() -> dict[str, float | None]:
    return {
        "metadata_verified": None,
        "first_piece_verified": None,
        "50_percent_verified": None,
        "95_percent_verified": None,
        "99_percent_verified": None,
        "all_pieces_verified": None,
        "published": None,
    }


def empty_geometry() -> dict[str, None]:
    return {"total_length": None, "piece_length": None, "piece_count": None, "file_count": None}


def mark_percent_milestones(
    milestones: dict[str, float | None], done: int, total: int, elapsed: float
) -> None:
    for percent, key in ((50, "50_percent_verified"), (95, "95_percent_verified"), (99, "99_percent_verified")):
        if milestones[key] is None and done * 100 >= total * percent:
            milestones[key] = elapsed


def harness_failure(implementation: str, wall_seconds: float, detail: str) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "implementation": implementation,
        "outcome": "harness_error",
        "wall_seconds": wall_seconds,
        "milestones": empty_milestones(),
        "geometry": empty_geometry(),
        "verified_piece_count": 0,
        "verified_bytes": 0,
        "integrity_verified": False,
        "cleanup_succeeded": False,
        "terminal_detail": bounded(detail),
        "capabilities": {},
        "diagnostics": {},
    }


def bounded(value: str, maximum: int = MAX_DIAGNOSTIC_CHARS) -> str:
    return value[:maximum]


def validate_catalog_observation(result: dict[str, Any], torrent: dict[str, Any]) -> None:
    if result.get("outcome") != "milestone_reached":
        return
    geometry = result.get("geometry", {})
    for catalog_key, result_key in (
        ("payload_bytes", "total_length"),
        ("piece_count", "piece_count"),
        ("file_count", "file_count"),
    ):
        expected = torrent.get(catalog_key)
        if expected is not None and geometry.get(result_key) != expected:
            result["outcome"] = "integrity_failure"
            result["integrity_verified"] = False
            result["terminal_detail"] = (
                f"catalog {catalog_key}={expected} did not match "
                f"observed {geometry.get(result_key)!r}"
            )
            return


def parse_args(arguments: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--torrent", default="big-buck-bunny")
    parser.add_argument("--profile", choices=PROFILES, default="common")
    parser.add_argument(
        "--owner",
        choices=OWNERS,
        default="both",
        help="run the alternating pair or one owner under the same harness",
    )
    parser.add_argument("--target", choices=TARGETS, default="metadata")
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--timeout-seconds", type=int, default=120)
    parser.add_argument("--cleanup-seconds", type=int, default=10)
    parser.add_argument("--catalog", type=Path, default=default_catalog_path())
    parser.add_argument("--output", type=Path)
    parser.add_argument("--quiet", action="store_true", help="write only --output")
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args(arguments)
    if not 1 <= args.runs <= MAX_RUNS:
        parser.error(f"--runs must be between 1 and {MAX_RUNS}")
    if not 1 <= args.timeout_seconds <= MAX_TIMEOUT_SECONDS:
        parser.error(f"--timeout-seconds must be between 1 and {MAX_TIMEOUT_SECONDS}")
    if not 1 <= args.cleanup_seconds <= 60:
        parser.error("--cleanup-seconds must be between 1 and 60")
    return args


def run_campaign(args: argparse.Namespace) -> dict[str, Any]:
    repository = repository_root()
    catalog = load_catalog(args.catalog.resolve())
    torrent = select_torrent(catalog, args.torrent)
    rst_magnet, lib_magnet = scenario_magnets(torrent, args.profile)
    binary = repository / "target" / "debug" / "rstorrent-public-probe"
    if args.owner != "libtorrent" and not args.no_build:
        binary = build_probe(repository)
    elif args.owner != "libtorrent" and not binary.is_file():
        raise HarnessError(f"--no-build probe does not exist at {binary}")

    runs: list[dict[str, Any]] = []
    environment = environment_snapshot(repository)
    with tempfile.TemporaryDirectory(prefix="rstorrent-public-compare-") as temporary:
        owned_root = Path(temporary).resolve()
        for ordinal in range(args.runs):
            order = selected_implementations(ordinal, args.owner)
            implementations: dict[str, Any] = {}
            for implementation in order:
                output_root = owned_root / f"run-{ordinal}" / implementation
                output_root.parent.mkdir(parents=True, exist_ok=True)
                if implementation == "rstorrent":
                    implementations[implementation] = run_rstorrent(
                        binary,
                        rst_magnet,
                        args.profile,
                        args.target,
                        args.timeout_seconds,
                        args.cleanup_seconds,
                        output_root,
                    )
                else:
                    implementations[implementation] = run_libtorrent(
                        lib_magnet,
                        torrent["info_hash"],
                        args.profile,
                        args.target,
                        args.timeout_seconds,
                        output_root,
                    )
                validate_catalog_observation(implementations[implementation], torrent)
                resolved = output_root.resolve()
                if resolved != owned_root and owned_root in resolved.parents:
                    shutil.rmtree(resolved, ignore_errors=True)
            classification = (
                classify_pair(implementations["rstorrent"], implementations["libtorrent"])
                if args.owner == "both"
                else classify_owner(implementations[args.owner])
            )
            runs.append(
                {
                    "ordinal": ordinal,
                    "order": order,
                    "classification": classification,
                    "implementations": implementations,
                }
            )
    return {
        "schema_version": SCHEMA_VERSION,
        "generated_at_unix_seconds": time.time(),
        "environment": environment,
        "config": {
            "torrent": args.torrent,
            "profile": args.profile,
            "owner": args.owner,
            "target": args.target,
            "runs": args.runs,
            "timeout_seconds": args.timeout_seconds,
            "cleanup_seconds": args.cleanup_seconds,
            "order": (
                "alternating-rstorrent-first" if args.owner == "both" else "single-owner"
            ),
            "libtorrent_settings": libtorrent_settings(args.profile)[0],
            "rstorrent_magnet": rst_magnet,
            "libtorrent_magnet": lib_magnet,
        },
        "catalog": {
            "schema_version": catalog["schema_version"],
            "source": catalog.get("source"),
            "retrieved": catalog.get("retrieved"),
            "torrent": torrent,
        },
        "runs": runs,
        "summary": summarize(runs, args.target, args.owner),
    }


def main(arguments: list[str]) -> int:
    args = parse_args(arguments)
    try:
        report = run_campaign(args)
    except HarnessError as error:
        print(f"harness error: {error}", file=sys.stderr)
        return 2
    rendered = json.dumps(report, indent=2, sort_keys=True)
    if args.output is not None:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered + "\n", encoding="utf-8")
    if not args.quiet:
        print(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
