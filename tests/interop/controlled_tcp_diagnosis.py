#!/usr/bin/env python3
"""Compare focused, resumable, and libtorrent TCP download paths locally."""

from __future__ import annotations

import argparse
import gc
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

import libtorrent as lt

from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    create_session,
    hash_file,
    wait_for_listener,
)
from local_throughput_compare import (
    BLOCK_SIZE,
    MIB,
    PAYLOAD_ALLOWANCE,
    PAYLOAD_NAME,
    Fixture,
    binary_sha256,
    configure_libtorrent_encryption,
    create_fixture,
    oracle_mse_method,
    parse_rstorrent_diagnostic,
)
from performance_profiles import collect_hardware_environment
from public_compare import (
    command_text,
    run_libtorrent as run_resumable_libtorrent,
    run_rstorrent as run_resumable_rstorrent,
    sample_process,
)
from public_compare_contract import parse_metainfo, verify_payload


OWNERS = ("focused", "resumable", "libtorrent")
OWNER_CHOICES = (*OWNERS, "resumable-no-sync", "resumable-summary-observation")
MAX_PAYLOAD_MIB = 2048
MAX_PIECE_KIB = 256 * 1024
MAX_OWNER_SECONDS = 45
MAX_BUDGET_SECONDS = 30 * 60
MAX_CAPTURE_BYTES = 256 * 1024
MAX_CAPTURE_LINES = 200
PROCESS_SAMPLE_SECONDS = 0.1
POLL_SECONDS = 0.005
CLEANUP_SECONDS = 5


class DiagnosisFailure(RuntimeError):
    pass


@dataclass(frozen=True)
class DiagnosisResult:
    size_bytes: int
    piece_size: int
    piece_count: int
    run: int
    order: int
    owner: str
    profile: str
    checkpoint_sync: str
    activity_observation: str
    version: str
    published_seconds: float
    active_seconds: float | None
    throughput_mib_s: float
    cpu_seconds: float | None
    cpu_core_equivalents: float | None
    peak_rss_bytes: int | None
    payload_sha1: str
    payload_bytes: int
    payload_download_bytes: int | None
    redundant_bytes: int | None
    failed_bytes: int | None
    wire_method: str
    connected_peer_high_water: int
    tcp_peer_high_water: int
    utp_peer_high_water: int
    milestones: dict[str, Any]
    diagnostics: dict[str, Any]
    independent_verification: dict[str, Any]
    cleanup_succeeded: bool


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def owner_order(run: int, owners: tuple[str, ...]) -> list[str]:
    if run < 1 or not owners:
        raise ValueError("run and owners must be nonempty")
    rotation = (run - 1) % len(owners)
    return list(owners[rotation:] + owners[:rotation])


def classify_ratio(ratio: float) -> str:
    if ratio < 0.9:
        return "behind"
    if ratio <= 1.1:
        return "near_parity"
    return "ahead"


def build_release_binaries(repository: Path) -> tuple[Path, Path]:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "-p",
            "rstorrent-engine",
            "--bin",
            "rstorrent-download-piece",
            "--bin",
            "rstorrent-public-probe",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=300,
        check=False,
    )
    if completed.returncode != 0:
        raise DiagnosisFailure(
            "failed to build release diagnostics\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    focused = repository / "target" / "release" / "rstorrent-download-piece"
    resumable = repository / "target" / "release" / "rstorrent-public-probe"
    if not focused.is_file() or not resumable.is_file():
        raise DiagnosisFailure("release diagnostic build omitted a requested binary")
    return focused, resumable


def _read_capture(stream: Any, label: str) -> str:
    stream.seek(0, os.SEEK_END)
    size = stream.tell()
    if size > MAX_CAPTURE_BYTES:
        raise DiagnosisFailure(f"{label} exceeded {MAX_CAPTURE_BYTES} bytes")
    stream.seek(0)
    encoded = stream.read()
    try:
        output = encoded.decode("utf-8")
    except UnicodeDecodeError as error:
        raise DiagnosisFailure(f"{label} was not UTF-8") from error
    if len(output.splitlines()) > MAX_CAPTURE_LINES:
        raise DiagnosisFailure(f"{label} exceeded {MAX_CAPTURE_LINES} lines")
    return output


def run_focused(
    binary: Path,
    fixture: Fixture,
    torrent_path: Path,
    peer_port: int,
    output_root: Path,
    timeout_seconds: int,
    profile: str,
    seed_handle: lt.torrent_handle,
) -> dict[str, Any]:
    output_root.mkdir(parents=True, exist_ok=False)
    output_path = output_root / PAYLOAD_NAME
    encryption = "disabled" if profile == "matched-plain-30" else "required"
    command = [
        str(binary),
        "--metainfo",
        str(torrent_path),
        "--peer",
        f"127.0.0.1:{peer_port}",
        "--output",
        str(output_path),
        "--timeout-seconds",
        str(timeout_seconds),
        "--max-buffered-payload-bytes",
        str(PAYLOAD_ALLOWANCE),
        "--encryption",
        encryption,
    ]
    started = time.monotonic()
    observed_methods: set[str] = set()
    connected_high_water = 0
    peak_rss = 0
    last_cpu: float | None = None
    next_process_sample = started
    forced_termination = False
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        process = subprocess.Popen(
            command,
            env={
                **os.environ,
                "RSTORRENT_TEST_STORAGE_WRITE_CONCURRENCY": "4",
                "RSTORRENT_TEST_STORAGE_HASH_CONCURRENCY": "4",
            },
            stdout=stdout,
            stderr=stderr,
        )
        deadline = started + timeout_seconds
        try:
            while process.poll() is None:
                peers = list(seed_handle.get_peer_info())
                connected_high_water = max(connected_high_water, len(peers))
                method = oracle_mse_method(seed_handle)
                if method is not None:
                    observed_methods.add(method)
                now = time.monotonic()
                if now >= next_process_sample:
                    rss, cpu = sample_process(process.pid)
                    if rss is not None:
                        peak_rss = max(peak_rss, rss)
                    if cpu is not None:
                        last_cpu = cpu
                    next_process_sample = now + PROCESS_SAMPLE_SECONDS
                if now >= deadline:
                    forced_termination = True
                    process.terminate()
                    try:
                        process.wait(timeout=CLEANUP_SECONDS)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=CLEANUP_SECONDS)
                    break
                time.sleep(POLL_SECONDS)
        except KeyboardInterrupt:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=CLEANUP_SECONDS)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=CLEANUP_SECONDS)
            raise
        stdout_text = _read_capture(stdout, "focused stdout")
        stderr_text = _read_capture(stderr, "focused stderr")
    published_seconds = time.monotonic() - started
    if forced_termination:
        raise DiagnosisFailure(
            f"focused owner exceeded {timeout_seconds} seconds; "
            f"stdout={stdout_text!r} stderr={stderr_text!r}"
        )
    if process.returncode != 0:
        raise DiagnosisFailure(
            f"focused owner exited with {process.returncode}; "
            f"stdout={stdout_text!r} stderr={stderr_text!r}"
        )
    if connected_high_water != 1:
        raise DiagnosisFailure(
            f"focused owner observed {connected_high_water} seed peers instead of one"
        )
    if profile == "matched-plain-30" and observed_methods:
        raise DiagnosisFailure(
            f"focused plaintext owner exposed MSE methods {sorted(observed_methods)}"
        )
    if profile == "matched-rc4-30" and observed_methods != {"rc4"}:
        raise DiagnosisFailure(
            f"focused RC4 owner exposed methods {sorted(observed_methods)}"
        )
    diagnostic = parse_rstorrent_diagnostic(stdout_text, fixture)
    return {
        "published_seconds": published_seconds,
        "active_seconds": None,
        "payload_download_bytes": fixture.size_bytes,
        "redundant_bytes": 0,
        "failed_bytes": 0,
        "wire_method": "tcp-rc4" if observed_methods else "tcp-plaintext",
        "connected_peer_high_water": connected_high_water,
        "tcp_peer_high_water": connected_high_water,
        "utp_peer_high_water": 0,
        "cpu_seconds": last_cpu,
        "peak_rss_bytes": peak_rss or None,
        "milestones": {"published": published_seconds},
        "diagnostics": {
            "focused": {
                key: int(value)
                for key, value in diagnostic.items()
                if value.isdigit()
            }
        },
    }


def validate_adapter_result(owner: str, result: dict[str, Any], profile: str) -> None:
    if result.get("outcome") != "milestone_reached":
        raise DiagnosisFailure(
            f"{owner} ended as {result.get('outcome')}: "
            f"{result.get('terminal_detail')}"
        )
    if not result.get("integrity_verified") or not result.get("cleanup_succeeded"):
        raise DiagnosisFailure(f"{owner} did not pass integrity and owner cleanup")
    evidence = result.get("diagnostics", {}).get("peer_methods", {})
    if evidence.get("connected_high_water") != 1:
        raise DiagnosisFailure(
            f"{owner} observed {evidence.get('connected_high_water')} peers instead of one"
        )
    if evidence.get("tcp_high_water") != 1 or evidence.get("utp_high_water") != 0:
        raise DiagnosisFailure(
            f"{owner} transport evidence was tcp={evidence.get('tcp_high_water')} "
            f"utp={evidence.get('utp_high_water')}"
        )
    if profile == "matched-plain-30":
        invalid = evidence.get("payload_contributor_plaintext_payload") or evidence.get(
            "payload_contributor_rc4"
        )
        if invalid or not evidence.get("payload_contributor_plaintext_stream"):
            raise DiagnosisFailure(f"{owner} plaintext contributor evidence failed")
    else:
        invalid = evidence.get("payload_contributor_plaintext_stream") or evidence.get(
            "payload_contributor_plaintext_payload"
        )
        if invalid or not evidence.get("payload_contributor_rc4"):
            raise DiagnosisFailure(f"{owner} RC4 contributor evidence failed")


def normalize_adapter_result(
    owner: str, result: dict[str, Any], fixture: Fixture, profile: str
) -> dict[str, Any]:
    validate_adapter_result(owner, result, profile)
    milestones = result["milestones"]
    published_seconds = float(milestones["published"])
    first_payload = milestones.get("first_payload_byte")
    active_seconds = (
        published_seconds - float(first_payload)
        if isinstance(first_payload, (int, float)) and published_seconds >= first_payload
        else None
    )
    diagnostics = result["diagnostics"]
    evidence = diagnostics["peer_methods"]
    status = diagnostics.get("status", {})
    process = result.get("process", {})
    return {
        "published_seconds": published_seconds,
        "active_seconds": active_seconds,
        "payload_download_bytes": status.get(
            "total_payload_download", diagnostics.get("received_bytes")
        ),
        "redundant_bytes": status.get(
            "redundant_bytes", diagnostics.get("redundant_payload_bytes")
        ),
        "failed_bytes": status.get("failed_bytes", diagnostics.get("failed_piece_bytes")),
        "wire_method": "tcp-rc4" if profile == "matched-rc4-30" else "tcp-plaintext",
        "connected_peer_high_water": int(evidence["connected_high_water"]),
        "tcp_peer_high_water": int(evidence["tcp_high_water"]),
        "utp_peer_high_water": int(evidence["utp_high_water"]),
        "cpu_seconds": process.get("cpu_seconds"),
        "peak_rss_bytes": process.get("peak_rss_bytes"),
        "milestones": milestones,
        "diagnostics": diagnostics,
    }


def run_owner(
    owner: str,
    focused_binary: Path,
    resumable_binary: Path,
    fixture: Fixture,
    piece_size: int,
    run: int,
    order: int,
    case_root: Path,
    timeout_seconds: int,
    profile: str,
) -> DiagnosisResult:
    torrent_path = fixture.torrents[piece_size]
    info = lt.torrent_info(str(torrent_path))
    descriptor = parse_metainfo(torrent_path.read_bytes())
    output_root = case_root / owner
    seed_session = create_session()
    configure_libtorrent_encryption(
        seed_session, "disabled" if profile == "matched-plain-30" else "required"
    )
    seed_handle: lt.torrent_handle | None = None
    cleanup_succeeded = False
    try:
        alerts: list[str] = []
        peer_port = wait_for_listener(seed_session, alerts)
        seed_handle = add_seed(seed_session, info, fixture.seed_root, alerts)
        print(
            f"case_start size_bytes={fixture.size_bytes} piece_size={piece_size} "
            f"run={run} order={order} owner={owner} profile={profile}",
            file=sys.stderr,
            flush=True,
        )
        if owner == "focused":
            metrics = run_focused(
                focused_binary,
                fixture,
                torrent_path,
                peer_port,
                output_root,
                timeout_seconds,
                profile,
                seed_handle,
            )
            version = binary_sha256(focused_binary)
        elif owner in (
            "resumable",
            "resumable-no-sync",
            "resumable-summary-observation",
        ):
            checkpoint_sync_bypass = owner == "resumable-no-sync"
            summary_activity_observation = owner == "resumable-summary-observation"
            raw = run_resumable_rstorrent(
                resumable_binary,
                torrent_path,
                profile,
                "complete",
                timeout_seconds,
                CLEANUP_SECONDS,
                output_root,
                descriptor.info_hash,
                fixture.size_bytes * 2,
                peer_hints=[f"127.0.0.1:{peer_port}"],
                diagnostic_checkpoint_sync_bypass=checkpoint_sync_bypass,
                diagnostic_summary_activity_observation=summary_activity_observation,
            )
            metrics = normalize_adapter_result(owner, raw, fixture, profile)
            version = binary_sha256(resumable_binary)
        elif owner == "libtorrent":
            raw = run_resumable_libtorrent(
                torrent_path,
                descriptor.info_hash,
                profile,
                "complete",
                timeout_seconds,
                output_root,
                CLEANUP_SECONDS,
                fixture.size_bytes * 2,
                peer_hints=[f"127.0.0.1:{peer_port}"],
            )
            metrics = normalize_adapter_result(owner, raw, fixture, profile)
            version = lt.version
        else:
            raise DiagnosisFailure(f"unknown owner {owner!r}")

        independent = verify_payload(descriptor, output_root)
        output_path = output_root / PAYLOAD_NAME
        actual_sha1 = hash_file(output_path)
        if actual_sha1 != fixture.expected_sha1:
            raise DiagnosisFailure(
                f"{owner} output SHA-1 {actual_sha1} differs from "
                f"{fixture.expected_sha1}"
            )
        shutil.rmtree(output_root)
        cleanup_succeeded = not output_root.exists()
        if not cleanup_succeeded:
            raise DiagnosisFailure(f"{owner} output cleanup failed")
        published_seconds = float(metrics.pop("published_seconds"))
        cpu_seconds = metrics.pop("cpu_seconds")
        result = DiagnosisResult(
            size_bytes=fixture.size_bytes,
            piece_size=piece_size,
            piece_count=info.num_pieces(),
            run=run,
            order=order,
            owner=owner,
            profile=profile,
            checkpoint_sync=(
                "bypassed"
                if owner == "resumable-no-sync"
                else "enabled"
                if owner == "resumable"
                else "not-applicable"
            ),
            activity_observation=(
                "summary"
                if owner == "resumable-summary-observation"
                else "detailed"
                if owner in ("resumable", "resumable-no-sync")
                else "not-applicable"
            ),
            version=version,
            published_seconds=published_seconds,
            active_seconds=metrics.pop("active_seconds"),
            throughput_mib_s=fixture.size_bytes / MIB / published_seconds,
            cpu_seconds=cpu_seconds,
            cpu_core_equivalents=(
                None if cpu_seconds is None else cpu_seconds / published_seconds
            ),
            peak_rss_bytes=metrics.pop("peak_rss_bytes"),
            payload_sha1=actual_sha1,
            payload_bytes=fixture.size_bytes,
            independent_verification=independent,
            cleanup_succeeded=cleanup_succeeded,
            **metrics,
        )
        print(
            f"case_result size_bytes={fixture.size_bytes} piece_size={piece_size} "
            f"run={run} owner={owner} seconds={published_seconds:.3f} "
            f"mib_s={result.throughput_mib_s:.3f} cleanup=ok",
            file=sys.stderr,
            flush=True,
        )
        return result
    finally:
        if seed_handle is not None and seed_handle.is_valid():
            seed_session.remove_torrent(seed_handle)
        seed_session.pause()
        seed_handle = None
        seed_session = None
        gc.collect()
        if output_root.exists():
            shutil.rmtree(output_root)


def summarize_results(results: list[DiagnosisResult]) -> list[dict[str, Any]]:
    workloads: dict[tuple[int, int, str], list[DiagnosisResult]] = {}
    for result in results:
        workloads.setdefault(
            (result.size_bytes, result.piece_size, result.profile), []
        ).append(result)
    summaries: list[dict[str, Any]] = []
    for (size_bytes, piece_size, profile), cohort in sorted(workloads.items()):
        owners = sorted({result.owner for result in cohort})
        owner_summaries: dict[str, dict[str, Any]] = {}
        for owner in owners:
            samples = [result for result in cohort if result.owner == owner]
            owner_summaries[owner] = {
                "runs": len(samples),
                "median_published_seconds": statistics.median(
                    result.published_seconds for result in samples
                ),
                "median_mib_s": statistics.median(
                    result.throughput_mib_s for result in samples
                ),
                "median_cpu_core_equivalents": (
                    statistics.median(cpu_values)
                    if (
                        cpu_values := [
                            result.cpu_core_equivalents
                            for result in samples
                            if result.cpu_core_equivalents is not None
                        ]
                    )
                    else None
                ),
                "median_peak_rss_bytes": (
                    statistics.median(rss_values)
                    if (
                        rss_values := [
                            result.peak_rss_bytes
                            for result in samples
                            if result.peak_rss_bytes is not None
                        ]
                    )
                    else None
                ),
            }
        reference = owner_summaries.get("libtorrent")
        if reference is not None:
            for values in owner_summaries.values():
                ratio = values["median_mib_s"] / reference["median_mib_s"]
                values["libtorrent_ratio"] = ratio
                values["classification"] = classify_ratio(ratio)
        focused = owner_summaries.get("focused")
        resumable = owner_summaries.get("resumable")
        resumable_no_sync = owner_summaries.get("resumable-no-sync")
        resumable_summary_observation = owner_summaries.get(
            "resumable-summary-observation"
        )
        path_ratio = (
            None
            if focused is None or resumable is None
            else resumable["median_mib_s"] / focused["median_mib_s"]
        )
        checkpoint_ratio = (
            None
            if resumable is None or resumable_no_sync is None
            else resumable_no_sync["median_mib_s"] / resumable["median_mib_s"]
        )
        observation_ratio = (
            None
            if resumable is None or resumable_summary_observation is None
            else resumable_summary_observation["median_mib_s"]
            / resumable["median_mib_s"]
        )
        summaries.append(
            {
                "size_bytes": size_bytes,
                "piece_size": piece_size,
                "profile": profile,
                "owners": owner_summaries,
                "resumable_focused_ratio": path_ratio,
                "resumable_focused_classification": (
                    None if path_ratio is None else classify_ratio(path_ratio)
                ),
                "checkpoint_bypass_enabled_ratio": checkpoint_ratio,
                "checkpoint_bypass_enabled_classification": (
                    None if checkpoint_ratio is None else classify_ratio(checkpoint_ratio)
                ),
                "summary_detailed_observation_ratio": observation_ratio,
                "summary_detailed_observation_classification": (
                    None if observation_ratio is None else classify_ratio(observation_ratio)
                ),
            }
        )
    return summaries


def _piece_sizes(values: list[int]) -> list[int]:
    result: list[int] = []
    for kib in values:
        size = kib * 1024
        if not 16 * 1024 <= size <= MAX_PIECE_KIB * 1024 or size & (size - 1):
            raise argparse.ArgumentTypeError(
                f"piece size {kib} KiB must be a power of two between 16 and "
                f"{MAX_PIECE_KIB} KiB"
            )
        if size not in result:
            result.append(size)
    return result


def parse_args(arguments: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sizes-mib", nargs="+", type=int, default=[1024])
    parser.add_argument(
        "--piece-sizes-kib", nargs="+", type=int, default=[256, 1024, 4096, 16384]
    )
    parser.add_argument("--runs", type=int, choices=range(1, 11), default=1)
    parser.add_argument(
        "--owners", nargs="+", choices=OWNER_CHOICES, default=list(OWNERS)
    )
    parser.add_argument(
        "--profile",
        choices=("matched-plain-30", "matched-rc4-30"),
        default="matched-plain-30",
    )
    parser.add_argument(
        "--timeout-seconds",
        type=int,
        choices=range(1, MAX_OWNER_SECONDS + 1),
        default=MAX_OWNER_SECONDS,
    )
    parser.add_argument(
        "--budget-seconds",
        type=int,
        choices=range(1, MAX_BUDGET_SECONDS + 1),
        default=MAX_BUDGET_SECONDS,
    )
    parser.add_argument("--focused-binary", type=Path)
    parser.add_argument("--resumable-binary", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(arguments)
    if any(size < 1 or size > MAX_PAYLOAD_MIB for size in args.sizes_mib):
        parser.error(f"--sizes-mib must be between 1 and {MAX_PAYLOAD_MIB}")
    try:
        args.piece_sizes = _piece_sizes(args.piece_sizes_kib)
    except argparse.ArgumentTypeError as error:
        parser.error(str(error))
    args.owners = tuple(dict.fromkeys(args.owners))
    return args


def main(arguments: list[str]) -> int:
    args = parse_args(arguments)
    repository = repository_root()
    if (args.focused_binary is None) != (args.resumable_binary is None):
        raise DiagnosisFailure(
            "--focused-binary and --resumable-binary must be supplied together"
        )
    if args.focused_binary is None:
        focused_binary, resumable_binary = build_release_binaries(repository)
    else:
        focused_binary = args.focused_binary.resolve()
        resumable_binary = args.resumable_binary.resolve()
    if not focused_binary.is_file() or not resumable_binary.is_file():
        raise DiagnosisFailure("a requested RSTorrent diagnostic binary is absent")

    temporary_root = Path(tempfile.gettempdir())
    largest = max(args.sizes_mib) * MIB
    required_free = largest * 2 + 2 * 1024 * MIB
    available = shutil.disk_usage(temporary_root).free
    if available < required_free:
        raise DiagnosisFailure(
            f"insufficient temporary disk: need {required_free}, have {available}"
        )

    hardware = collect_hardware_environment(temporary_root)
    environment = {
        **hardware,
        "repository_commit": command_text(["git", "rev-parse", "HEAD"], repository),
        "repository_dirty": bool(
            command_text(["git", "status", "--porcelain"], repository)
        ),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "rustc": command_text(["rustc", "--version"], repository),
        "libtorrent": lt.version,
        "focused_binary_sha256": binary_sha256(focused_binary),
        "resumable_binary_sha256": binary_sha256(resumable_binary),
        "cargo_profile": "release",
        "cache_policy": "warm-uncontrolled-os-page-cache",
    }
    results: list[DiagnosisResult] = []
    experiment_started = time.monotonic()
    deadline = experiment_started + args.budget_seconds
    with tempfile.TemporaryDirectory(prefix="rstorrent-tcp-diagnosis-") as temporary:
        owned_root = Path(temporary)
        for size_mib in args.sizes_mib:
            fixture = create_fixture(owned_root, size_mib * MIB, args.piece_sizes)
            try:
                for piece_size in args.piece_sizes:
                    for run in range(1, args.runs + 1):
                        case_root = owned_root / f"case-{size_mib}-{piece_size}-{run}"
                        case_root.mkdir()
                        try:
                            for order, owner in enumerate(
                                owner_order(run, args.owners), start=1
                            ):
                                if time.monotonic() >= deadline:
                                    raise DiagnosisFailure(
                                        f"experiment exceeded {args.budget_seconds} seconds"
                                    )
                                results.append(
                                    run_owner(
                                        owner,
                                        focused_binary,
                                        resumable_binary,
                                        fixture,
                                        piece_size,
                                        run,
                                        order,
                                        case_root,
                                        args.timeout_seconds,
                                        args.profile,
                                    )
                                )
                        finally:
                            if case_root.exists():
                                shutil.rmtree(case_root)
            finally:
                if fixture.root.exists():
                    shutil.rmtree(fixture.root)

    report = {
        "schema_version": 1,
        "scenario": "controlled-tcp-performance-diagnosis",
        "status": "passed",
        "environment": environment,
        "config": {
            "sizes_mib": args.sizes_mib,
            "piece_sizes_kib": args.piece_sizes_kib,
            "runs": args.runs,
            "owners": list(args.owners),
            "profile": args.profile,
            "timeout_seconds": args.timeout_seconds,
            "cleanup_seconds": CLEANUP_SECONDS,
            "budget_seconds": args.budget_seconds,
            "block_size_bytes": BLOCK_SIZE,
            "payload_allowance_bytes": PAYLOAD_ALLOWANCE,
            "storage_concurrency": {"writes": 4, "hashes": 4},
            "checkpoint_sync": "selected-by-owner",
            "activity_observation": "selected-by-owner",
            "order": "rotating-by-run",
        },
        "elapsed_seconds": time.monotonic() - experiment_started,
        "results": [asdict(result) for result in results],
        "summaries": summarize_results(results),
    }
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output is None:
        print(rendered, end="")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
        print(f"wrote {args.output}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except (DiagnosisFailure, OSError, ScenarioFailure, subprocess.SubprocessError) as error:
        print(f"controlled TCP diagnosis failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
