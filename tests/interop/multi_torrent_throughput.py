#!/usr/bin/env python3
"""Measure bounded concurrent application downloads from a pinned libtorrent seed."""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import os
import platform
import selectors
import shutil
import statistics
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import libtorrent as lt

from application_identity import torrent_id_from_add
from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    create_session,
    wait_for_listener,
)


MIB = 1024 * 1024
SOURCE_CHUNK = MIB
DEFAULT_COUNTS = (1, 2, 3, 4, 8)


@dataclass(frozen=True)
class Fixture:
    index: int
    name: str
    payload: Path
    seed_root: Path
    torrent: Path
    info_hash: str
    size_bytes: int
    sha1: str


@dataclass(frozen=True)
class RatePolicy:
    name: str
    session_download_bytes_per_second: int
    torrent_download_bytes_per_second: tuple[int | None, ...]
    peer_counts: tuple[int, ...]


def deterministic_payload(path: Path, size_bytes: int, salt: int) -> str:
    pattern = bytearray(
        ((offset * 73) ^ (offset >> 3) ^ (salt * 41) ^ 0xA5) & 0xFF
        for offset in range(SOURCE_CHUNK)
    )
    digest = hashlib.sha1()
    remaining = size_bytes
    chunk_index = 0
    with path.open("xb", buffering=0) as output:
        while remaining:
            length = min(len(pattern), remaining)
            pattern[:8] = (chunk_index + salt * 1_000_000).to_bytes(8, "little")
            chunk = memoryview(pattern)[:length]
            if output.write(chunk) != length:
                raise ScenarioFailure("fixture write made partial progress")
            digest.update(chunk)
            remaining -= length
            chunk_index += 1
        os.fsync(output.fileno())
    return digest.hexdigest()


def create_fixtures(root: Path, count: int, size_bytes: int, piece_size: int) -> list[Fixture]:
    fixtures = []
    for index in range(count):
        fixture_root = root / f"fixture-{index}"
        seed_root = fixture_root / "seed"
        seed_root.mkdir(parents=True)
        name = f"concurrent-{index}.bin"
        payload = seed_root / name
        sha1 = deterministic_payload(payload, size_bytes, index + 1)
        files = lt.file_storage()
        files.add_file(name, size_bytes)
        creator = lt.create_torrent(
            files,
            piece_size=piece_size,
            flags=lt.create_torrent.v1_only,
        )
        lt.set_piece_hashes(creator, str(seed_root))
        torrent = fixture_root / "fixture.torrent"
        torrent.write_bytes(bytes(lt.bencode(creator.generate())))
        info = lt.torrent_info(str(torrent))
        if info.total_size() != size_bytes or info.piece_length() != piece_size:
            raise ScenarioFailure("concurrent fixture geometry diverged")
        fixtures.append(
            Fixture(
                index=index,
                name=name,
                payload=payload,
                seed_root=seed_root,
                torrent=torrent,
                info_hash=str(info.info_hashes().v1),
                size_bytes=size_bytes,
                sha1=sha1,
            )
        )
    return fixtures


def exchange(
    process: subprocess.Popen[str], request: dict[str, Any], timeout_seconds: float = 10
) -> dict[str, Any]:
    if process.stdin is None or process.stdout is None:
        raise ScenarioFailure("session diagnostic pipes are unavailable")
    process.stdin.write(json.dumps(request, separators=(",", ":")) + "\n")
    process.stdin.flush()
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    try:
        if not selector.select(timeout_seconds):
            raise ScenarioFailure(f"session did not answer {request['request_id']}")
        line = process.stdout.readline()
    finally:
        selector.close()
    if not line:
        stderr = process.stderr.read() if process.stderr is not None else ""
        raise ScenarioFailure(f"session exited before responding\n{stderr}")
    response = json.loads(line)
    if response.get("request_id") != request["request_id"]:
        raise ScenarioFailure("session response identity diverged")
    if response.get("status") != "success":
        raise ScenarioFailure(f"session command failed: {response}")
    return response


def envelope(request_id: str, command: dict[str, Any]) -> dict[str, Any]:
    return {"version": 1, "request_id": request_id, "command": command}


def transfer_rate_limit(bytes_per_second: int | None) -> dict[str, Any]:
    if bytes_per_second is None:
        return {"type": "unlimited"}
    return {"type": "limited", "bytes_per_second": bytes_per_second}


def settings(
    active_downloads: int,
    *,
    download_bytes_per_second: int | None = None,
) -> dict[str, Any]:
    return {
        "listener": {"type": "disabled"},
        "preferred_listen_port": 6881,
        "port_mapping": "disabled",
        "peer_connection_limit": 200,
        "upload_slots": 8,
        "active_downloads": active_downloads,
        "upload_rate_limit": transfer_rate_limit(None),
        "download_rate_limit": transfer_rate_limit(download_bytes_per_second),
        "encryption": "allow",
        "ipv6_enabled": True,
        "tracker_https_server_authentication": "system_trust",
    }


def process_sample(pid: int) -> tuple[int, float]:
    completed = subprocess.run(
        ["ps", "-o", "rss=,time=", "-p", str(pid)],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0 or not completed.stdout.strip():
        return 0, 0.0
    fields = completed.stdout.split()
    rss_bytes = int(fields[0]) * 1024
    value = fields[1]
    days = 0
    if "-" in value:
        day, value = value.split("-", 1)
        days = int(day)
    components = [float(component) for component in value.split(":")]
    if len(components) == 3:
        hours, minutes, seconds = components
    elif len(components) == 2:
        hours = 0
        minutes, seconds = components
    else:
        hours = 0
        minutes = 0
        seconds = components[0]
    cpu_seconds = days * 86400 + hours * 3600 + minutes * 60 + seconds
    return rss_bytes, cpu_seconds


def hash_file(path: Path) -> str:
    digest = hashlib.sha1()
    with path.open("rb") as source:
        while chunk := source.read(MIB):
            digest.update(chunk)
    return digest.hexdigest()


def run_case(
    binary: Path,
    fixtures: list[Fixture],
    count: int,
    configured_limit: int,
    run: int,
    root: Path,
    timeout_seconds: int,
    rate_policy: RatePolicy | None = None,
) -> dict[str, Any]:
    policy_suffix = f"-{rate_policy.name}" if rate_policy is not None else ""
    case_root = root / f"n{count}-limit{configured_limit}{policy_suffix}-run{run}"
    profile_root = case_root / "profile"
    payload_root = case_root / "payload"
    report_path = case_root / "resources.json"
    payload_root.mkdir(parents=True)
    alerts: list[str] = []
    seed_entries: list[tuple[Any, Any, Fixture]] = []
    seed_ports: dict[int, list[int]] = {}
    process: subprocess.Popen[str] | None = None
    started = time.monotonic()
    try:
        peer_counts = rate_policy.peer_counts if rate_policy is not None else (1,) * count
        if len(peer_counts) != count:
            raise ScenarioFailure("rate policy peer counts do not match its torrent count")
        for fixture, peer_count in zip(fixtures[:count], peer_counts, strict=True):
            seed_ports[fixture.index] = []
            for _ in range(peer_count):
                seed_session = create_session()
                port = wait_for_listener(seed_session, alerts)
                seed_handle = add_seed(
                    seed_session,
                    lt.torrent_info(str(fixture.torrent)),
                    fixture.seed_root,
                    alerts,
                )
                seed_entries.append((seed_session, seed_handle, fixture))
                seed_ports[fixture.index].append(port)

        def start_session(report: Path) -> subprocess.Popen[str]:
            return subprocess.Popen(
                [
                    str(binary),
                    "--profile-root",
                    str(profile_root),
                    "--storage-root",
                    f"downloads={payload_root}",
                    "--resource-report",
                    str(report),
                    "--timeout-seconds",
                    str(timeout_seconds),
                ],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
            )

        process = start_session(
            case_root / "configuration-resources.json"
            if rate_policy is not None
            else report_path
        )
        exchange(
            process,
            envelope(
                "settings",
                {
                    "type": "update_client_settings",
                    "patch": settings(
                        configured_limit,
                        download_bytes_per_second=(
                            rate_policy.session_download_bytes_per_second
                            if rate_policy is not None
                            else None
                        ),
                    ),
                },
            ),
        )
        torrent_ids: dict[int, str] = {}
        for fixture in fixtures[:count]:
            explicit_peers = "".join(
                f"&x.pe=127.0.0.1:{port}" for port in seed_ports[fixture.index]
            )
            torrent_ids[fixture.index] = torrent_id_from_add(
                exchange(
                    process,
                    envelope(
                    f"add-{fixture.index}",
                    {
                        "type": "add_magnet",
                        "magnet": (
                            f"magnet:?xt=urn:btih:{fixture.info_hash}"
                            f"&dn={fixture.name}{explicit_peers}"
                        ),
                        "storage_root": "downloads",
                        "start_content": rate_policy is None,
                        "skip_files": [],
                    },
                    ),
                )
            )
            if rate_policy is not None:
                torrent_rate = rate_policy.torrent_download_bytes_per_second[fixture.index]
                exchange(
                    process,
                    envelope(
                        f"rate-{fixture.index}",
                        {
                            "type": "update_torrent_settings",
                            "torrent_id": torrent_ids[fixture.index],
                            "patch": {
                                "download_rate_limit": transfer_rate_limit(torrent_rate),
                            },
                        },
                    ),
                )
        if rate_policy is not None:
            exchange(process, envelope("configured-shutdown", {"type": "shutdown"}))
            process.wait(timeout=30)
            if process.returncode != 0:
                stderr = process.stderr.read() if process.stderr is not None else ""
                raise ScenarioFailure(f"configuration session exited {process.returncode}\n{stderr}")
            process = start_session(report_path)
        transfer_started = time.monotonic()
        if rate_policy is not None:
            for fixture in fixtures[:count]:
                exchange(
                    process,
                    envelope(
                        f"resume-{fixture.index}",
                        {
                            "type": "resume",
                            "torrent_id": torrent_ids[fixture.index],
                        },
                    ),
                )
        completion_seconds: dict[str, float] = {}
        progress_samples: dict[str, list[tuple[float, int]]] = {
            torrent_ids[fixture.index]: [(0.0, 0)] for fixture in fixtures[:count]
        }
        rss_high_water = 0
        cpu_seconds = 0.0
        peer_high_water = 0
        deadline = transfer_started + timeout_seconds
        sequence = 0
        while len(completion_seconds) != count:
            if time.monotonic() >= deadline:
                raise ScenarioFailure(
                    f"concurrent n={count} exceeded {timeout_seconds} seconds"
                )
            response = exchange(
                process,
                envelope(f"snapshot-{sequence}", {"type": "snapshot"}),
            )
            sequence += 1
            now = time.monotonic() - transfer_started
            torrents = response["snapshot"]["torrents"]
            for torrent in torrents:
                torrent_id = torrent["torrent_id"]
                if torrent_id not in progress_samples:
                    continue
                stored = int(torrent["verified_piece_count"])
                samples = progress_samples[torrent_id]
                if not samples or samples[-1][1] != stored:
                    samples.append((now, stored))
                if torrent["state"] == "complete":
                    completion_seconds.setdefault(torrent_id, now)
                elif torrent["state"] in ("error", "needs_repair"):
                    raise ScenarioFailure(f"torrent failed during sweep: {torrent}")
            rss, cpu_seconds = process_sample(process.pid)
            rss_high_water = max(rss_high_water, rss)
            if len(completion_seconds) != count:
                time.sleep(0.025)
        transfer_seconds = time.monotonic() - transfer_started
        shutdown_started = time.monotonic()
        exchange(process, envelope("shutdown", {"type": "shutdown"}))
        process.wait(timeout=30)
        shutdown_seconds = time.monotonic() - shutdown_started
        if process.returncode != 0:
            stderr = process.stderr.read() if process.stderr is not None else ""
            raise ScenarioFailure(f"session exited {process.returncode}\n{stderr}")
        resources = json.loads(report_path.read_text())
        peer_high_water = int(resources["peer_budget"]["total_high_water"])
        download = resources["download"]
        ceilings = {
            "outstanding_request_high_water": 256 * MIB,
            "buffered_payload_high_water": 32 * MIB,
            "active_piece_bytes_high_water": 256 * MIB,
            "active_pieces_high_water": 2_048,
            "active_storage_writes_high_water": 4,
            "active_storage_hashes_high_water": 4,
            "registered_generations_high_water": count,
        }
        for field, maximum in ceilings.items():
            if int(download[field]) > maximum:
                raise ScenarioFailure(
                    f"n={count} resource {field}={download[field]} exceeded {maximum}"
                )
        for field in (
            "outstanding_request_bytes",
            "buffered_payload_bytes",
            "active_piece_bytes",
            "active_pieces",
            "active_storage_writes",
            "active_storage_hashes",
            "registered_generations",
        ):
            if int(download[field]) != 0:
                raise ScenarioFailure(f"n={count} terminal {field} did not drain")
        if int(resources["storage_files"]["owned_high_water"]) > 40:
            raise ScenarioFailure("session file-pool ceiling was exceeded")
        if peer_high_water > 200:
            raise ScenarioFailure("session peer ceiling was exceeded")
        for fixture in fixtures[:count]:
            output = payload_root / fixture.name
            if output.stat().st_size != fixture.size_bytes or hash_file(output) != fixture.sha1:
                raise ScenarioFailure(f"publication differs for {fixture.info_hash}")
            fixture_handles = [
                handle
                for _, handle, seeded_fixture in seed_entries
                if seeded_fixture.index == fixture.index
            ]
            if not any(handle.status().total_payload_upload > 0 for handle in fixture_handles):
                raise ScenarioFailure(f"oracle uploaded no payload for {fixture.info_hash}")
        if any(len(samples) < 2 for samples in progress_samples.values()):
            raise ScenarioFailure("a concurrent torrent exposed no measured progress")
        total_bytes = count * fixtures[0].size_bytes
        return {
            "count": count,
            "configured_limit": configured_limit,
            "rate_policy": (
                {
                    "name": rate_policy.name,
                    "session_download_bytes_per_second": (
                        rate_policy.session_download_bytes_per_second
                    ),
                    "torrent_download_bytes_per_second": list(
                        rate_policy.torrent_download_bytes_per_second
                    ),
                    "peer_counts": list(rate_policy.peer_counts),
                }
                if rate_policy is not None
                else None
            ),
            "run": run,
            "payload_bytes": total_bytes,
            "transfer_seconds": transfer_seconds,
            "throughput_mib_s": total_bytes / MIB / transfer_seconds,
            "completion_seconds": completion_seconds,
            "progress_samples": {
                key: len(value) for key, value in progress_samples.items()
            },
            "cpu_seconds": cpu_seconds,
            "cpu_core_equivalents": cpu_seconds / transfer_seconds,
            "rss_high_water_bytes": rss_high_water,
            "peer_connections_high_water": peer_high_water,
            "shutdown_seconds": shutdown_seconds,
            "resources": resources,
            "wall_seconds": time.monotonic() - started,
        }
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=10)
        for seed_session, handle, _ in seed_entries:
            try:
                if handle.is_valid():
                    seed_session.remove_torrent(handle)
            except Exception:
                pass
            seed_session.pause()
        seed_entries.clear()
        gc.collect()
        shutil.rmtree(case_root, ignore_errors=True)


def median(values: list[float]) -> float:
    return float(statistics.median(values))


def command_value(commands: list[list[str]]) -> str:
    for command in commands:
        completed = subprocess.run(command, capture_output=True, text=True, check=False)
        if completed.returncode == 0 and completed.stdout.strip():
            return completed.stdout.strip()
    return "unknown"


def filesystem_name(path: Path) -> str:
    completed = subprocess.run(["mount"], capture_output=True, text=True, check=False)
    if completed.returncode != 0:
        return "unknown"
    resolved = str(path.resolve())
    candidates = []
    for line in completed.stdout.splitlines():
        mac = line.rsplit(" on ", 1)
        if len(mac) != 2:
            continue
        target_and_type = mac[1]
        if " type " in target_and_type:
            target, details = target_and_type.split(" type ", 1)
            filesystem = details.split(maxsplit=1)[0]
        elif " (" in target_and_type:
            target, details = target_and_type.split(" (", 1)
            filesystem = details.split(",", 1)[0].rstrip(")")
        else:
            continue
        if resolved == target or resolved.startswith(target.rstrip("/") + "/") or target == "/":
            candidates.append((len(target), filesystem))
    return max(candidates, default=(0, "unknown"))[1]


def summarize(results: list[dict[str, Any]]) -> list[dict[str, Any]]:
    groups: dict[tuple[int, int], list[dict[str, Any]]] = {}
    for result in results:
        groups.setdefault((result["count"], result["configured_limit"]), []).append(result)
    summaries = []
    for (count, limit), cohort in sorted(groups.items()):
        summaries.append(
            {
                "count": count,
                "configured_limit": limit,
                "runs": len(cohort),
                "median_throughput_mib_s": median(
                    [row["throughput_mib_s"] for row in cohort]
                ),
                "median_cpu_core_equivalents": median(
                    [row["cpu_core_equivalents"] for row in cohort]
                ),
                "max_rss_bytes": max(row["rss_high_water_bytes"] for row in cohort),
                "max_shutdown_seconds": max(row["shutdown_seconds"] for row in cohort),
            }
        )
    return summaries


def validate_rate_result(result: dict[str, Any]) -> dict[str, Any]:
    policy = result["rate_policy"]
    if policy is None:
        raise ScenarioFailure("rate validation received an unlimited throughput result")
    direction = result["resources"]["bandwidth"]["download"]
    granted = int(direction["granted_bytes"])
    returned = int(direction["returned_bytes"])
    admitted = granted - returned
    session_rate = int(policy["session_download_bytes_per_second"])
    torrent_rates = [
        rate for rate in policy["torrent_download_bytes_per_second"] if rate is not None
    ]
    effective_rate = min([session_rate, *torrent_rates])
    elapsed = float(result["transfer_seconds"])
    burst = min(effective_rate, MIB)
    upper_bound = int(effective_rate * elapsed) + burst + 16 * 1024
    if admitted < int(result["payload_bytes"]):
        raise ScenarioFailure(
            f"{policy['name']} admitted fewer peer bytes than its exact payload"
        )
    if admitted > upper_bound:
        raise ScenarioFailure(
            f"{policy['name']} admitted {admitted} bytes above {upper_bound}"
        )
    if admitted / elapsed < effective_rate * 0.60:
        raise ScenarioFailure(f"{policy['name']} made less than 60% useful rate progress")
    if int(direction["throttle_wait_high_water_micros"]) <= 0:
        raise ScenarioFailure(f"{policy['name']} recorded no throttle wait")
    if int(direction["active_waiters"]) != 0 or int(direction["queued_requested_bytes"]) != 0:
        raise ScenarioFailure(f"{policy['name']} left terminal bandwidth work queued")
    if int(direction["registered_torrents"]) != int(result["count"]):
        raise ScenarioFailure(f"{policy['name']} lost a live torrent registration")
    return {
        "admitted_bytes": admitted,
        "effective_rate_bytes_per_second": effective_rate,
        "upper_bound_bytes": upper_bound,
        "utilization": admitted / elapsed / effective_rate,
        "cap_pass": True,
        "terminal_queue_pass": True,
    }


def run_rate_profile(
    binary: Path,
    fixtures: list[Fixture],
    root: Path,
    arguments: argparse.Namespace,
    repository: Path,
) -> int:
    policies = (
        RatePolicy("session-cap", 256 * 1024, (None,), (1,)),
        RatePolicy("torrent-cap", MIB, (256 * 1024,), (1,)),
        RatePolicy("torrent-fairness", 512 * 1024, (None, None), (3, 1)),
    )
    results = []
    for run in range(1, arguments.runs + 1):
        ordered = policies if run % 2 else tuple(reversed(policies))
        for policy in ordered:
            result = run_case(
                binary,
                fixtures,
                len(policy.torrent_download_bytes_per_second),
                len(policy.torrent_download_bytes_per_second),
                run,
                root,
                arguments.timeout_seconds,
                policy,
            )
            result["rate_gate"] = validate_rate_result(result)
            results.append(result)
            print(json.dumps(result, sort_keys=True), flush=True)
    fairness = [
        result for result in results if result["rate_policy"]["name"] == "torrent-fairness"
    ]
    fairness_ratios = []
    for result in fairness:
        completions = list(result["completion_seconds"].values())
        ratio = max(completions) / min(completions)
        fairness_ratios.append(ratio)
        if ratio > 1.35:
            raise ScenarioFailure(
                f"unequal-peer torrent fairness completion ratio {ratio:.3f} exceeded 1.35"
            )
        if int(result["peer_connections_high_water"]) < 4:
            raise ScenarioFailure("unequal-peer fairness case established fewer than four peers")
    report = {
        "schema_version": 1,
        "scenario": "hierarchical-transfer-rate-enforcement",
        "commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=repository, text=True
        ).strip(),
        "libtorrent": lt.version,
        "environment": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "logical_cpus": os.cpu_count(),
            "host_model": command_value([["sysctl", "-n", "hw.model"], ["uname", "-m"]]),
            "filesystem": filesystem_name(root),
        },
        "size_mib_per_torrent": arguments.size_mib,
        "piece_size_kib": arguments.piece_size_kib,
        "recorded_runs_per_case": arguments.runs,
        "gates": {
            "all_caps_pass": True,
            "all_terminal_queues_pass": True,
            "unequal_peer_fairness_ratios": fairness_ratios,
            "unequal_peer_fairness_pass": True,
        },
        "results": results,
    }
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if arguments.output is not None:
        arguments.output.write_text(encoded)
    print(encoded, end="")
    return 0


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--size-mib", type=int, default=128)
    parser.add_argument("--piece-size-kib", type=int, default=1024)
    parser.add_argument("--runs", type=int, choices=range(1, 8), default=5)
    parser.add_argument("--timeout-seconds", type=int, default=120)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument(
        "--rate-policy",
        action="store_true",
        help="run the bounded session/torrent cap and unequal-peer fairness profile",
    )
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    if not 1 <= arguments.size_mib <= 1024:
        raise ScenarioFailure("--size-mib must be between 1 and 1024")
    if not 16 <= arguments.piece_size_kib <= 16 * 1024:
        raise ScenarioFailure("--piece-size-kib must be between 16 and 16384")
    repository = Path(__file__).resolve().parents[2]
    binary = repository / "target" / "release" / "rstorrent-session"
    if not arguments.no_build:
        completed = subprocess.run(
            ["cargo", "build", "--release", "-p", "rstorrent-session", "--bin", "rstorrent-session"],
            cwd=repository,
            check=False,
        )
        if completed.returncode != 0:
            raise ScenarioFailure("release session diagnostic build failed")
    if not binary.is_file():
        raise ScenarioFailure("release session diagnostic is unavailable")
    with tempfile.TemporaryDirectory(prefix="rstorrent-multi-throughput-") as temporary:
        root = Path(temporary)
        fixtures = create_fixtures(
            root,
            2 if arguments.rate_policy else max(DEFAULT_COUNTS),
            arguments.size_mib * MIB,
            arguments.piece_size_kib * 1024,
        )
        if arguments.rate_policy:
            return run_rate_profile(binary, fixtures, root, arguments, repository)
        results = []
        cases = [(1, 1), (1, 3), *((count, count) for count in DEFAULT_COUNTS[1:])]
        for count, limit in cases:
            run_case(
                binary,
                fixtures,
                count,
                limit,
                0,
                root,
                arguments.timeout_seconds,
            )
        for run in range(1, arguments.runs + 1):
            single_cases = cases[:2]
            if run % 2 == 0:
                single_cases = list(reversed(single_cases))
            concurrent_cases = cases[2:]
            offset = (run - 1) % len(concurrent_cases)
            recorded_cases = [
                *single_cases,
                *concurrent_cases[offset:],
                *concurrent_cases[:offset],
            ]
            for count, limit in recorded_cases:
                result = run_case(
                    binary,
                    fixtures,
                    count,
                    limit,
                    run,
                    root,
                    arguments.timeout_seconds,
                )
                results.append(result)
                print(json.dumps(result, sort_keys=True), flush=True)
        summaries = summarize(results)
        by_case = {(row["count"], row["configured_limit"]): row for row in summaries}
        one_limit = by_case[(1, 1)]["median_throughput_mib_s"]
        one_three = by_case[(1, 3)]["median_throughput_mib_s"]
        two = by_case[(2, 2)]["median_throughput_mib_s"]
        gates = {
            "single_limit_three_ratio": one_three / one_limit,
            "two_over_single_ratio": two / one_limit,
            "single_regression_pass": one_three >= 0.95 * one_limit,
            "two_aggregate_pass": two >= 0.90 * one_limit,
        }
        report = {
            "schema_version": 1,
            "scenario": "session-wide-concurrent-torrent-throughput",
            "commit": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=repository, text=True
            ).strip(),
            "libtorrent": lt.version,
            "environment": {
                "platform": platform.platform(),
                "machine": platform.machine(),
                "logical_cpus": os.cpu_count(),
                "host_model": command_value(
                    [["sysctl", "-n", "hw.model"], ["uname", "-m"]]
                ),
                "filesystem": filesystem_name(root),
            },
            "size_mib_per_torrent": arguments.size_mib,
            "piece_size_kib": arguments.piece_size_kib,
            "warmup_runs_per_case": 1,
            "recorded_runs_per_case": arguments.runs,
            "summaries": summaries,
            "gates": gates,
            "results": results,
        }
        encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
        if arguments.output is not None:
            arguments.output.write_text(encoded)
        print(encoded, end="")
        if not gates["single_regression_pass"] or not gates["two_aggregate_pass"]:
            raise ScenarioFailure(f"multi-torrent throughput gate failed: {gates}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ScenarioFailure as error:
        print(f"multi-torrent throughput failed: {error}", file=os.sys.stderr)
        raise SystemExit(1)
