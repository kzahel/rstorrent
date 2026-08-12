#!/usr/bin/env python3
"""Compare product-owned incoming TCP and uTP on one ordinary WAN path."""

from __future__ import annotations

import argparse
import json
import re
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from product_utp_reachability import (
    MAPPING_DESCRIPTION,
    gateway_preflight,
    parse_endpoint,
    start_product_seed,
)
from upnp_external_seeding import (
    GateFailure,
    build_seed,
    command_seed,
    delete_mapping,
    list_mappings,
    query_mapping,
    rows,
    stop_seed,
    terminate,
)
from utp_reference_oracle import PAYLOAD_NAME, create_fixture
from utp_remote_seed import MAX_DIAGNOSTICS, eligible_public_ipv4
from utp_rstorrent_wan import (
    SSH_ALIAS_PATTERN,
    RemoteProcess,
    WanFailure,
    bounded_diagnostics,
    create_remote_run,
    remote_leecher_started_pid,
    run_ssh,
    stage_remote_leecher_fixture,
    verify_direct_route,
    verify_remote_leecher_cleanup,
)


ROOT = Path(__file__).resolve().parents[2]
PAYLOAD_BYTES = 8 * 1024 * 1024 + 731
PIECE_BYTES = 256 * 1024
PIECES = 33
REQUIRED_PAIRS = 3
MAX_PAIR_ATTEMPTS = 4
TRANSFER_TIMEOUT_SECONDS = 600.0
PAIR_TIMEOUT_SECONDS = 25 * 60.0
PEER_DRAIN_SECONDS = 20.0
REMOTE_HOLD_SECONDS = 1.0
MILESTONE_NAMES = ("1", "25", "50", "75", "100")


@dataclass(frozen=True)
class ProductMapping:
    protocol: str
    local_address: str
    local_port: int
    external_address: str
    external_port: int
    lease_seconds: int


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", required=True)
    return parser.parse_args()


def pair_order(attempt: int) -> tuple[str, str]:
    if not 1 <= attempt <= MAX_PAIR_ATTEMPTS:
        raise WanFailure("pair attempt is outside the campaign bound")
    return ("tcp", "utp") if attempt % 2 else ("utp", "tcp")


def validate_product_mapping(
    ready: dict[str, Any],
    *,
    mapping_field: str,
    listener_field: str,
    protocol: str,
) -> ProductMapping:
    mapping = ready.get(mapping_field)
    if not isinstance(mapping, dict) or mapping.get("type") != "mapped":
        raise WanFailure(f"product did not publish a verified {protocol} mapping")
    local_address = mapping.get("local_address")
    local_port = mapping.get("local_port")
    external_address = mapping.get("external_address")
    external_port = mapping.get("external_port")
    lease_seconds = mapping.get("lease_seconds")
    if not (
        isinstance(local_address, str)
        and isinstance(local_port, int)
        and isinstance(external_address, str)
        and isinstance(external_port, int)
        and isinstance(lease_seconds, int)
        and 1 <= local_port <= 65_535
        and 1 <= external_port <= 65_535
        and 0 < lease_seconds <= 3_600
        and eligible_public_ipv4(external_address)
    ):
        raise WanFailure(f"product {protocol} mapping fields are invalid")
    listener_address, listener_port = parse_endpoint(
        ready.get(listener_field), listener_field
    )
    if listener_port != local_port or listener_address not in {
        local_address,
        "0.0.0.0",
    }:
        raise WanFailure(f"product {protocol} mapping missed its actual listener")
    return ProductMapping(
        protocol,
        local_address,
        local_port,
        external_address,
        external_port,
        lease_seconds,
    )


def validate_dual_mappings(
    ready: dict[str, Any], expected_local_address: str
) -> dict[str, ProductMapping]:
    tcp = validate_product_mapping(
        ready,
        mapping_field="mapping",
        listener_field="listen",
        protocol="TCP",
    )
    utp = validate_product_mapping(
        ready,
        mapping_field="udp_mapping",
        listener_field="utp_listen",
        protocol="UDP",
    )
    if (
        tcp.local_address != expected_local_address
        or utp.local_address != expected_local_address
        or tcp.external_address != utp.external_address
    ):
        raise WanFailure("product mappings do not share one local and public path")
    return {"tcp": tcp, "utp": utp}


def verify_installed_mapping(
    control: str, service: str, mapping: ProductMapping
) -> None:
    installed = query_mapping(
        control,
        service,
        mapping.external_port,
        mapping.protocol,
    )
    if installed is None or not (
        installed.get("NewInternalClient") == mapping.local_address
        and installed.get("NewInternalPort") == str(mapping.local_port)
        and installed.get("NewEnabled") == "1"
        and installed.get("NewPortMappingDescription") == MAPPING_DESCRIPTION
        and 0 < int(installed.get("NewLeaseDuration", "0")) <= 3_600
    ):
        raise WanFailure(
            f"independent query rejected the exact product {mapping.protocol} mapping"
        )


def expected_transport_settings(transport: str) -> dict[str, bool]:
    if transport not in {"tcp", "utp"}:
        raise WanFailure("transport evidence selection is invalid")
    tcp = transport == "tcp"
    utp = transport == "utp"
    return {
        "enable_incoming_tcp": tcp,
        "enable_outgoing_tcp": tcp,
        "enable_incoming_utp": utp,
        "enable_outgoing_utp": utp,
        "enable_dht": False,
        "enable_lsd": False,
        "enable_upnp": False,
        "enable_natpmp": False,
    }


def validate_remote_ready(
    event: dict[str, Any], expected_pid: int, transport: str
) -> None:
    if event.get("event") != "ready" or event.get("role") != "remote-leecher":
        raise WanFailure("remote comparator leecher did not become ready")
    if not (
        event.get("pid") == expected_pid
        and event.get("libtorrent_version") == "2.0.13.0"
        and event.get("route_class") == "ordinary-internet"
        and event.get("transport") == transport
        and event.get("applied_transport_settings")
        == expected_transport_settings(transport)
    ):
        raise WanFailure("remote comparator readiness evidence is inconsistent")
    port = event.get("listen_port")
    if not isinstance(port, int) or not 1 <= port <= 65_535:
        raise WanFailure("remote comparator reported an invalid listener port")


def _positive_number(value: object, field: str) -> float:
    if not isinstance(value, (int, float)) or isinstance(value, bool) or value <= 0:
        raise WanFailure(f"remote comparator {field} is invalid")
    return float(value)


def validate_remote_complete(
    event: dict[str, Any], transport: str, expected_sha1: str
) -> dict[str, Any]:
    if event.get("event") == "failed" and event.get("role") == "remote-leecher":
        raise WanFailure(
            "remote comparator transfer failed: "
            f"reason={event.get('reason', 'missing')}, "
            f"progress_ppm={event.get('progress_ppm', 'missing')}, "
            f"wanted_done={event.get('wanted_done_bytes', 'missing')}"
        )
    if not (
        event.get("event") == "complete"
        and event.get("role") == "remote-leecher"
        and event.get("transport") == transport
        and event.get("peer_high_water") == 1
        and event.get("applied_transport_settings")
        == expected_transport_settings(transport)
    ):
        raise WanFailure("remote comparator terminal ownership evidence failed")
    payload = event.get("payload")
    if not isinstance(payload, dict) or payload != {
        "bytes": PAYLOAD_BYTES,
        "pieces": PIECES,
        "sha1": expected_sha1,
    }:
        raise WanFailure("remote comparator payload evidence failed")
    diagnostics = event.get("diagnostics")
    if not isinstance(diagnostics, list) or len(diagnostics) > MAX_DIAGNOSTICS:
        raise WanFailure("remote comparator diagnostics exceeded their bound")
    stats = event.get("libtorrent_stats")
    if not isinstance(stats, dict) or stats.get("net.recv_payload_bytes", 0) < PAYLOAD_BYTES:
        raise WanFailure("remote comparator receive accounting is incomplete")
    if transport == "tcp":
        transport_valid = (
            stats.get("peer.num_utp_peers") == 0
            and stats.get("peer.num_tcp_peers", 0) <= 1
            and stats.get("utp.utp_packets_in") == 0
            and stats.get("utp.utp_packets_out") == 0
        )
    else:
        transport_valid = (
            stats.get("peer.num_tcp_peers") == 0
            and stats.get("peer.num_utp_peers", 0) <= 1
            and stats.get("utp.utp_packets_in", 0) > 0
            and stats.get("utp.utp_packets_out", 0) > 0
        )
    if not transport_valid:
        raise WanFailure("remote comparator transport counters show masking")

    timing = event.get("timing")
    if not isinstance(timing, dict):
        raise WanFailure("remote comparator omitted monotonic timing")
    connect_seconds = _positive_number(
        timing.get("connect_to_complete_seconds"), "connection duration"
    )
    active_seconds = _positive_number(
        timing.get("active_payload_seconds"), "active duration"
    )
    first_seconds = timing.get("first_payload_seconds")
    if not isinstance(first_seconds, (int, float)) or isinstance(first_seconds, bool):
        raise WanFailure("remote comparator first-payload timing is invalid")
    first_seconds = float(first_seconds)
    milestone_seconds = timing.get("milestone_seconds")
    if not isinstance(milestone_seconds, dict) or set(milestone_seconds) != set(
        MILESTONE_NAMES
    ):
        raise WanFailure("remote comparator milestone set is invalid")
    milestone_values = [
        _positive_number(milestone_seconds[name], f"{name}% milestone")
        for name in MILESTONE_NAMES
    ]
    if not (
        0 <= first_seconds <= milestone_values[0]
        and milestone_values == sorted(milestone_values)
        and abs(milestone_values[-1] - connect_seconds) <= 0.002
        and abs((connect_seconds - first_seconds) - active_seconds) <= 0.002
        and connect_seconds <= TRANSFER_TIMEOUT_SECONDS
    ):
        raise WanFailure("remote comparator milestone timing is inconsistent")
    payload_mib = PAYLOAD_BYTES / (1024 * 1024)
    return {
        "transport": transport,
        "payload": payload,
        "timing": {
            "connect_to_complete_seconds": connect_seconds,
            "first_payload_seconds": first_seconds,
            "active_payload_seconds": active_seconds,
            "milestone_seconds": {
                name: float(milestone_seconds[name]) for name in MILESTONE_NAMES
            },
        },
        "rates_mib_per_second": {
            "active": round(payload_mib / active_seconds, 6),
            "connect": round(payload_mib / connect_seconds, 6),
        },
        "applied_transport_settings": event["applied_transport_settings"],
        "libtorrent_stats": stats,
        "diagnostics": bounded_diagnostics([str(line) for line in diagnostics]),
    }


def product_snapshot_summary(snapshot: dict[str, Any]) -> dict[str, Any]:
    if snapshot.get("event") != "snapshot":
        raise WanFailure("product comparator snapshot is malformed")
    result: dict[str, Any] = {}
    for field in ("pending", "established", "connection_high_water", "payload_bytes_sent"):
        value = snapshot.get(field)
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise WanFailure(f"product comparator snapshot omitted {field}")
        result[field] = value
    utp = snapshot.get("utp")
    if not isinstance(utp, dict):
        raise WanFailure("product comparator omitted uTP snapshot")
    result["utp"] = {
        field: utp.get(field)
        for field in (
            "active_connections",
            "connection_high_water",
            "datagrams_sent",
            "data_datagrams_sent",
            "worker_panics",
        )
    }
    return result


def wait_for_peer_drain(
    seed: subprocess.Popen[str], expected_payload: int, pair_deadline: float
) -> dict[str, Any]:
    deadline = min(pair_deadline, time.monotonic() + PEER_DRAIN_SECONDS)
    while time.monotonic() < deadline:
        snapshot = command_seed(seed, "snapshot")
        summary = product_snapshot_summary(snapshot)
        if summary["payload_bytes_sent"] > expected_payload:
            raise WanFailure("product payload accounting exceeded the exact pair bound")
        utp = summary["utp"]
        if (
            not rows(snapshot, "peers")
            and summary["pending"] == 0
            and summary["established"] == 0
            and utp.get("active_connections") == 0
            and summary["payload_bytes_sent"] == expected_payload
        ):
            return summary
        time.sleep(0.05)
    raise WanFailure("product peer generation did not drain before the next transport")


def run_transport_case(
    *,
    host: str,
    remote_run: str,
    seed: subprocess.Popen[str],
    transport: str,
    mapping: ProductMapping,
    expected_sha1: str,
    expected_payload: int,
    pair_deadline: float,
    remote_pids: list[int],
) -> dict[str, Any]:
    before = product_snapshot_summary(command_seed(seed, "snapshot"))
    if before["payload_bytes_sent"] != expected_payload - PAYLOAD_BYTES:
        raise WanFailure("product case began with unexpected cumulative payload")
    remote = RemoteProcess.start_leecher(
        host,
        remote_run,
        expected_sha1,
        mapping.external_address,
        mapping.external_port,
        transport=transport,
        expected_bytes=PAYLOAD_BYTES,
        expected_piece_bytes=PIECE_BYTES,
        expected_pieces=PIECES,
        timeout_seconds=TRANSFER_TIMEOUT_SECONDS,
        hold_complete_seconds=REMOTE_HOLD_SECONDS,
        output_name=f"leech-{transport}",
    )
    observed_product_transport = False
    try:
        pid = remote_leecher_started_pid(remote.read_event(pair_deadline))
        remote_pids.append(pid)
        validate_remote_ready(remote.read_event(pair_deadline), pid, transport)
        while remote.process.poll() is None and time.monotonic() < pair_deadline:
            snapshot = command_seed(seed, "snapshot")
            peer_rows = rows(snapshot, "peers")
            if len(peer_rows) > 1:
                raise WanFailure("product comparator exceeded one live peer")
            if peer_rows:
                peer = peer_rows[0]
                if not (
                    peer.get("direction") == "incoming"
                    and peer.get("transport") == transport
                ):
                    raise WanFailure("product comparator observed transport masking")
                try:
                    uploaded = int(peer.get("payload_uploaded_bytes") or "0")
                except (TypeError, ValueError) as error:
                    raise WanFailure("product peer payload counter is malformed") from error
                observed_product_transport |= uploaded > 0
            time.sleep(0.03)
        if time.monotonic() >= pair_deadline:
            raise WanFailure("product comparator exceeded the pair deadline")
        complete_event = remote.read_event(pair_deadline)
        complete = validate_remote_complete(complete_event, transport, expected_sha1)
        remote.wait_success(pair_deadline)
        if not observed_product_transport:
            raise WanFailure("product views missed the selected incoming transport")
    finally:
        remote.cleanup()
    after = wait_for_peer_drain(seed, expected_payload, pair_deadline)
    complete["product"] = {
        "before": before,
        "after_drain": after,
        "selected_transport_observed": True,
    }
    return complete


def validate_product_pair_stop(stopped: dict[str, Any]) -> None:
    utp = stopped.get("utp_before_shutdown")
    if not isinstance(utp, dict) or not (
        utp.get("connection_high_water") == 1
        and utp.get("worker_panics") == 0
        and isinstance(utp.get("datagrams_sent"), int)
        and utp["datagrams_sent"] > 0
    ):
        raise WanFailure("product pair did not retain exact uTP ownership evidence")
    if not (
        stopped.get("payload_bytes_sent") == 2 * PAYLOAD_BYTES
        and stopped.get("connection_high_water") == 1
        and stopped.get("mapping_tasks_after_shutdown") == 0
        and stopped.get("mappings_after_shutdown") == 0
    ):
        raise WanFailure("product pair accounting or terminal owners are not exact")


def cleanup_owned_mappings(
    control: str, service: str, local_address: str
) -> None:
    owned = [
        entry
        for entry in list_mappings(control, service)
        if entry.get("NewInternalClient") == local_address
        and entry.get("NewPortMappingDescription") == MAPPING_DESCRIPTION
    ]
    if len(owned) > 2:
        raise WanFailure("gateway cleanup found too many owned product mappings")
    for entry in owned:
        protocol = entry.get("NewProtocol")
        try:
            port = int(entry.get("NewExternalPort", "0"))
        except ValueError as error:
            raise WanFailure("owned mapping cleanup identity is malformed") from error
        if protocol not in {"TCP", "UDP"} or not 1 <= port <= 65_535:
            raise WanFailure("owned mapping cleanup target is invalid")
        delete_mapping(control, service, port, protocol)
    retained = [
        entry
        for entry in list_mappings(control, service)
        if entry.get("NewInternalClient") == local_address
        and entry.get("NewPortMappingDescription") == MAPPING_DESCRIPTION
    ]
    if retained:
        raise WanFailure("owned product mapping survived exact cleanup")


def _failure_kind(message: str) -> str:
    lowered = message.lower()
    if "timeout" in lowered or "deadline" in lowered:
        return "timeout"
    if "mapping" in lowered or "gateway" in lowered:
        return "mapping"
    if "transport" in lowered:
        return "transport-evidence"
    if "drain" in lowered or "peer" in lowered:
        return "peer-lifecycle"
    return "environmental"


def run_pair(
    *,
    host: str,
    attempt: int,
    binary: Path,
    metainfo: Path,
    storage_root: Path,
    expected_sha1: str,
) -> dict[str, Any]:
    order = pair_order(attempt)
    pair_deadline = time.monotonic() + PAIR_TIMEOUT_SECONDS
    local_address, control, service = gateway_preflight()
    remote_run: str | None = None
    remote_pids: list[int] = []
    seed: subprocess.Popen[str] | None = None
    mappings: dict[str, ProductMapping] = {}
    cases: list[dict[str, Any]] = []
    failure: Exception | None = None
    stopped: dict[str, Any] | None = None
    cleanup_errors: list[str] = []
    with tempfile.TemporaryDirectory(prefix=f"rstorrent-wan-compare-{attempt}-") as pair_root_text:
        pair_root = Path(pair_root_text)
        try:
            remote_run = create_remote_run(host)
            stage_remote_leecher_fixture(host, remote_run, metainfo)
            seed, ready = start_product_seed(
                binary,
                pair_root / "profile",
                storage_root,
                metainfo,
            )
            mappings = validate_dual_mappings(ready, local_address)
            for mapping in mappings.values():
                verify_direct_route(host, mapping.external_address)
                verify_installed_mapping(control, service, mapping)
            initial = wait_for_peer_drain(seed, 0, pair_deadline)
            for index, transport in enumerate(order, start=1):
                cases.append(
                    run_transport_case(
                        host=host,
                        remote_run=remote_run,
                        seed=seed,
                        transport=transport,
                        mapping=mappings[transport],
                        expected_sha1=expected_sha1,
                        expected_payload=index * PAYLOAD_BYTES,
                        pair_deadline=pair_deadline,
                        remote_pids=remote_pids,
                    )
                )
            stopped = stop_seed(seed)
            seed = None
            validate_product_pair_stop(stopped)
            for mapping in mappings.values():
                if query_mapping(
                    control, service, mapping.external_port, mapping.protocol
                ) is not None:
                    raise WanFailure(
                        f"product {mapping.protocol} mapping survived joined shutdown"
                    )
            cleanup_owned_mappings(control, service, local_address)
        except (
            GateFailure,
            WanFailure,
            OSError,
            subprocess.SubprocessError,
            ValueError,
        ) as error:
            failure = error
        finally:
            if seed is not None:
                try:
                    stopped = stop_seed(seed)
                    seed = None
                except BaseException as error:
                    terminate(seed)
                    seed = None
                    cleanup_errors.append(str(error))
            for pid in remote_pids:
                try:
                    process_check = run_ssh(
                        host, f"kill -0 {pid} 2>/dev/null", check=False
                    )
                    if process_check.returncode == 0:
                        raise WanFailure("remote comparator process survived cleanup")
                except BaseException as error:
                    cleanup_errors.append(str(error))
            if remote_run is not None:
                try:
                    verify_remote_leecher_cleanup(host, remote_run, None)
                except BaseException as error:
                    cleanup_errors.append(str(error))
            try:
                cleanup_owned_mappings(control, service, local_address)
            except BaseException as error:
                cleanup_errors.append(str(error))
    if cleanup_errors:
        raise WanFailure(
            "pair cleanup remained uncertain: "
            + "; ".join(bounded_diagnostics(cleanup_errors))
        )
    cleanup = {
        "succeeded": True,
        "tcp_mapping_absent": True,
        "udp_mapping_absent": True,
        "remote_processes_absent": True,
        "remote_run_directory_absent": True,
        "local_pair_root_absent": True,
    }
    if failure is not None:
        reason = bounded_diagnostics([str(failure).replace(host, "<host>")])[0]
        return {
            "attempt": attempt,
            "status": "failed",
            "order": list(order),
            "failure": {"kind": _failure_kind(reason), "reason": reason},
            "complete_cases": cases,
            "cleanup": cleanup,
        }
    if stopped is None or len(cases) != 2:
        raise WanFailure("complete pair omitted terminal evidence")
    by_transport = {case["transport"]: case for case in cases}
    ratios = {
        "active_utp_over_tcp": round(
            by_transport["tcp"]["timing"]["active_payload_seconds"]
            / by_transport["utp"]["timing"]["active_payload_seconds"],
            6,
        ),
        "connect_utp_over_tcp": round(
            by_transport["tcp"]["timing"]["connect_to_complete_seconds"]
            / by_transport["utp"]["timing"]["connect_to_complete_seconds"],
            6,
        ),
    }
    return {
        "attempt": attempt,
        "status": "complete",
        "order": list(order),
        "mapping": {
            "same_public_address": True,
            "tcp": {
                "protocol": "TCP",
                "lease_seconds": mappings["tcp"].lease_seconds,
                "target_matches_listener": True,
                "independently_verified": True,
                "deleted": True,
            },
            "utp": {
                "protocol": "UDP",
                "lease_seconds": mappings["utp"].lease_seconds,
                "target_matches_listener": True,
                "independently_verified": True,
                "deleted": True,
            },
        },
        "initial_product": initial,
        "cases": cases,
        "ratios": ratios,
        "product_terminal": {
            "payload_bytes_sent": stopped["payload_bytes_sent"],
            "connection_high_water": stopped["connection_high_water"],
            "utp_connection_high_water": stopped["utp_before_shutdown"][
                "connection_high_water"
            ],
            "worker_panics": stopped["utp_before_shutdown"]["worker_panics"],
            "mapping_tasks": stopped["mapping_tasks_after_shutdown"],
            "mappings": stopped["mappings_after_shutdown"],
        },
        "cleanup": cleanup,
    }


def _range(values: list[float]) -> dict[str, float]:
    return {
        "min": round(min(values), 6),
        "median": round(statistics.median(values), 6),
        "max": round(max(values), 6),
    }


def summarize_complete_pairs(attempts: list[dict[str, Any]]) -> dict[str, Any] | None:
    complete = [attempt for attempt in attempts if attempt.get("status") == "complete"]
    if len(complete) != REQUIRED_PAIRS:
        return None
    active = [float(pair["ratios"]["active_utp_over_tcp"]) for pair in complete]
    connect = [float(pair["ratios"]["connect_utp_over_tcp"]) for pair in complete]
    strata: dict[str, dict[str, list[float]]] = {}
    for pair in complete:
        key = "-then-".join(pair["order"])
        values = strata.setdefault(key, {"active": [], "connect": []})
        values["active"].append(float(pair["ratios"]["active_utp_over_tcp"]))
        values["connect"].append(float(pair["ratios"]["connect_utp_over_tcp"]))
    return {
        "complete_pairs": REQUIRED_PAIRS,
        "active_utp_over_tcp": _range(active),
        "connect_utp_over_tcp": _range(connect),
        "order_strata": {
            key: {
                "pairs": len(values["active"]),
                "active_utp_over_tcp": _range(values["active"]),
                "connect_utp_over_tcp": _range(values["connect"]),
            }
            for key, values in sorted(strata.items())
        },
    }


def assert_redacted_report(report: dict[str, Any], host: str) -> None:
    serialized = json.dumps(report, sort_keys=True)
    if host in serialized:
        raise WanFailure("comparison report retained the control host identity")
    if re.search(r"(?<![0-9])(?:[0-9]{1,3}\.){3}[0-9]{1,3}(?![0-9])", serialized):
        raise WanFailure("comparison report retained a network address")
    if "/tmp/" in serialized or "http://" in serialized or "https://" in serialized:
        raise WanFailure("comparison report retained a path or gateway identity")


def run(host: str) -> dict[str, Any]:
    if not SSH_ALIAS_PATTERN.fullmatch(host) or host.startswith("-"):
        raise WanFailure("SSH host alias is malformed")
    binary = build_seed(ROOT)
    attempts: list[dict[str, Any]] = []
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="rstorrent-wan-compare-cohort-") as root_text:
        root = Path(root_text)
        torrent_info, storage_root, expected_sha1 = create_fixture(
            root,
            payload_size=PAYLOAD_BYTES,
            piece_size=PIECE_BYTES,
        )
        if torrent_info.num_pieces() != PIECES:
            raise WanFailure("comparison fixture piece count is not exact")
        metainfo = root / "forced-utp.torrent"
        for attempt in range(1, MAX_PAIR_ATTEMPTS + 1):
            attempts.append(
                run_pair(
                    host=host,
                    attempt=attempt,
                    binary=binary,
                    metainfo=metainfo,
                    storage_root=storage_root,
                    expected_sha1=expected_sha1,
                )
            )
            if sum(item.get("status") == "complete" for item in attempts) == REQUIRED_PAIRS:
                break
    summary = summarize_complete_pairs(attempts)
    report = {
        "schema_version": 1,
        "oracle": "product-owned-incoming-wan-tcp-utp-comparison",
        "status": "complete" if summary is not None else "evidence-limited",
        "libtorrent_version": "2.0.13.0",
        "route_class": "ordinary-internet",
        "ssh_data_path": False,
        "fixture": {
            "name": PAYLOAD_NAME,
            "bytes": PAYLOAD_BYTES,
            "piece_bytes": PIECE_BYTES,
            "pieces": PIECES,
            "sha1": expected_sha1,
        },
        "attempts": attempts,
        "summary": summary,
        "seconds": round(time.monotonic() - started, 6),
        "cleanup": {
            "all_attempts_exact": all(
                attempt.get("cleanup", {}).get("succeeded") is True
                for attempt in attempts
            ),
            "local_cohort_root_absent": True,
        },
    }
    assert_redacted_report(report, host)
    return report


def main() -> int:
    arguments = parse_arguments()
    try:
        print(json.dumps(run(arguments.host), indent=2, sort_keys=True))
        return 0
    except (GateFailure, WanFailure, OSError, subprocess.SubprocessError) as error:
        detail = bounded_diagnostics([str(error).replace(arguments.host, "<host>")])
        print(f"WAN transport comparison failed: {detail[0]}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
