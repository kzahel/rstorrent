#!/usr/bin/env python3
"""Measure SQLite application throughput under real and adversarial view sets."""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import os
import platform
import re
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
    compare_payloads,
    create_session,
    wait_for_listener,
)
from magnet_metadata import create_fixture, magnet_uri
from performance_profiles import (
    PerformanceProfileError,
    collect_hardware_environment,
    load_performance_profile,
    profile_runs,
    selected_application_cases,
    validate_environment,
)


MIB = 1024 * 1024
GIB = 1024 * MIB
POLL_SECONDS = 0.05
MAX_PROCESS_GRACE_SECONDS = 30


@dataclass
class ApplicationResult:
    baseline_case_id: str
    mode: str
    run: int
    order: int
    torrent_id: str
    transfer_seconds: float
    throughput_mib_s: float
    process_wall_seconds: float
    process_cpu_seconds: float | None
    peak_rss_bytes: int | None
    payload_sha1: str
    payload_bytes: int
    piece_count: int
    verified_piece_count: int
    completion_polls: int
    consumer_delay_millis: int
    batches: int
    empty_batches: int
    updates: int
    snapshot_updates: int
    patch_updates: int
    reset_updates: int
    serialized_bytes: int
    per_view_updates: dict[str, int]
    queue_bytes_at_end: int
    queue_high_water_bytes: int
    view_set_reset_count: int
    cleanup_succeeded: bool = False


def build_binary(repository: Path) -> Path:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "rstorrent-session",
            "--bin",
            "rstorrent-application-throughput-profile",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to build application throughput profile\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    suffix = ".exe" if sys.platform == "win32" else ""
    binary = (
        repository
        / "target"
        / "debug"
        / f"rstorrent-application-throughput-profile{suffix}"
    )
    if not binary.is_file():
        raise ScenarioFailure("application throughput profile binary was not created")
    return binary


def binary_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(MIB):
            digest.update(chunk)
    return digest.hexdigest()


def command_text(command: list[str], root: Path) -> str | None:
    completed = subprocess.run(
        command,
        cwd=root,
        capture_output=True,
        text=True,
        timeout=10,
        check=False,
    )
    return completed.stdout.strip() if completed.returncode == 0 else None


def report_path(path: Path, repository: Path) -> str:
    try:
        return str(path.relative_to(repository))
    except ValueError:
        return path.name


def sampled_rss_bytes(pid: int) -> int | None:
    if sys.platform.startswith("linux"):
        try:
            values: dict[str, int] = {}
            for line in Path(f"/proc/{pid}/status").read_text(encoding="utf-8").splitlines():
                if line.startswith(("VmHWM:", "VmRSS:")):
                    name, value, unit = line.split()
                    if unit != "kB":
                        continue
                    values[name.rstrip(":")] = int(value) * 1024
            return values.get("VmHWM") or values.get("VmRSS")
        except (OSError, UnicodeError, ValueError):
            return None
    if sys.platform == "darwin":
        try:
            completed = subprocess.run(
                ["ps", "-o", "rss=", "-p", str(pid)],
                capture_output=True,
                text=True,
                timeout=2,
                check=False,
            )
            if completed.returncode == 0 and completed.stdout.strip():
                return int(completed.stdout.strip()) * 1024
        except (OSError, subprocess.SubprocessError, ValueError):
            return None
    return None


def rusage_peak_bytes(usage: Any) -> int | None:
    maximum = getattr(usage, "ru_maxrss", None)
    if not isinstance(maximum, (int, float)) or maximum <= 0:
        return None
    # Darwin reports bytes; Linux and the BSDs used by hosted CI report KiB.
    return int(maximum) if sys.platform == "darwin" else int(maximum) * 1024


def wait_with_resources(
    process: subprocess.Popen[str], timeout_seconds: int
) -> tuple[int, str, str, float | None, int | None]:
    started = time.monotonic()
    deadline = started + timeout_seconds + MAX_PROCESS_GRACE_SECONDS
    peak_rss = None
    usage = None
    status = None
    if hasattr(os, "wait4"):
        while status is None:
            if time.monotonic() >= deadline:
                process.kill()
                _, raw_status, usage = os.wait4(process.pid, 0)
                process.returncode = os.waitstatus_to_exitcode(raw_status)
                raise ScenarioFailure(
                    f"application profile exceeded {timeout_seconds + MAX_PROCESS_GRACE_SECONDS} seconds"
                )
            waited_pid, raw_status, observed_usage = os.wait4(process.pid, os.WNOHANG)
            if waited_pid == process.pid:
                status = os.waitstatus_to_exitcode(raw_status)
                usage = observed_usage
                process.returncode = status
                break
            sample = sampled_rss_bytes(process.pid)
            if sample is not None:
                peak_rss = sample if peak_rss is None else max(peak_rss, sample)
            time.sleep(POLL_SECONDS)
        stdout = process.stdout.read() if process.stdout is not None else ""
        stderr = process.stderr.read() if process.stderr is not None else ""
    else:
        try:
            stdout, stderr = process.communicate(
                timeout=timeout_seconds + MAX_PROCESS_GRACE_SECONDS
            )
        except subprocess.TimeoutExpired as error:
            process.kill()
            stdout, stderr = process.communicate()
            raise ScenarioFailure(
                f"application profile exceeded {timeout_seconds + MAX_PROCESS_GRACE_SECONDS} seconds"
            ) from error
        status = process.returncode
    if usage is not None:
        resource_peak = rusage_peak_bytes(usage)
        if resource_peak is not None:
            peak_rss = resource_peak if peak_rss is None else max(peak_rss, resource_peak)
        cpu_seconds = float(usage.ru_utime + usage.ru_stime)
    else:
        cpu_seconds = None
    return int(status), stdout, stderr, cpu_seconds, peak_rss


def run_case(
    binary: Path,
    fixture: Any,
    peer_port: int,
    case: dict[str, Any],
    run: int,
    order: int,
    root: Path,
    timeout_seconds: int,
    slow_consumer_delay_millis: int,
) -> ApplicationResult:
    case_root = root / f"case-{run}-{order}-{case['mode']}"
    profile_root = case_root / "profile"
    payload_root = case_root / "payload"
    case_root.mkdir()
    command = [
        str(binary),
        "--profile-root",
        str(profile_root),
        "--payload-root",
        str(payload_root),
        "--magnet",
        magnet_uri(fixture.info_hash, f"127.0.0.1:{peer_port}"),
        "--info-hash",
        fixture.info_hash,
        "--publication-name",
        fixture.torrent_info.name(),
        "--payload-bytes",
        str(fixture.torrent_info.total_size()),
        "--mode",
        case["mode"],
        "--timeout-seconds",
        str(timeout_seconds),
        "--consumer-delay-millis",
        str(slow_consumer_delay_millis),
        "--write-concurrency",
        "4",
        "--hash-concurrency",
        "4",
    ]
    print(
        f"case_start run={run} order={order} mode={case['mode']} "
        f"case_id={case['id']}",
        flush=True,
    )
    started = time.monotonic()
    process = subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    failure: BaseException | None = None
    result: ApplicationResult | None = None
    try:
        returncode, stdout, stderr, cpu_seconds, peak_rss = wait_with_resources(
            process, timeout_seconds
        )
        process_wall_seconds = time.monotonic() - started
        if returncode != 0:
            raise ScenarioFailure(
                f"application mode {case['mode']} exited with {returncode}\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}"
            )
        lines = [line for line in stdout.splitlines() if line.strip()]
        if len(lines) != 1:
            raise ScenarioFailure(
                f"application mode {case['mode']} emitted {len(lines)} output lines: {stdout!r}"
            )
        report = json.loads(lines[0])
        if report.get("schema_version") != 1 or report.get("mode") != case["mode"]:
            raise ScenarioFailure("application profile returned the wrong schema or mode")
        torrent_id = report.get("torrent_id")
        if not isinstance(torrent_id, str) or re.fullmatch(
            r"t1-[0-9a-f]{32}", torrent_id
        ) is None:
            raise ScenarioFailure("application profile returned an invalid torrent owner")
        publication_name = report.get("publication_name")
        if publication_name != fixture.torrent_info.name():
            raise ScenarioFailure("application profile returned the wrong publication name")
        expected_bytes = fixture.torrent_info.total_size()
        if report.get("payload_bytes") != expected_bytes:
            raise ScenarioFailure("application profile returned the wrong payload size")
        if report.get("piece_count") != fixture.torrent_info.num_pieces():
            raise ScenarioFailure("application profile returned the wrong piece count")
        if report.get("verified_piece_count") != report.get("piece_count"):
            raise ScenarioFailure("application profile did not verify every piece")
        published_root = payload_root / publication_name
        candidates = list(published_root.rglob("payload.bin"))
        if len(candidates) != 1:
            raise ScenarioFailure(
                f"application profile published {len(candidates)} payload.bin files"
            )
        payload_hash = compare_payloads(fixture.payload_path, candidates[0])
        if payload_hash != fixture.payload_hash:
            raise ScenarioFailure("application profile payload differs from the seed")
        delivery = report["delivery"]
        result = ApplicationResult(
            baseline_case_id=case["id"],
            mode=case["mode"],
            run=run,
            order=order,
            torrent_id=torrent_id,
            transfer_seconds=float(report["transfer_seconds"]),
            throughput_mib_s=float(report["throughput_mib_s"]),
            process_wall_seconds=process_wall_seconds,
            process_cpu_seconds=cpu_seconds,
            peak_rss_bytes=peak_rss,
            payload_sha1=payload_hash,
            payload_bytes=expected_bytes,
            piece_count=int(report["piece_count"]),
            verified_piece_count=int(report["verified_piece_count"]),
            completion_polls=int(report["completion_polls"]),
            consumer_delay_millis=int(report["consumer_delay_millis"]),
            batches=int(delivery["batches"]),
            empty_batches=int(delivery["empty_batches"]),
            updates=int(delivery["updates"]),
            snapshot_updates=int(delivery["snapshot_updates"]),
            patch_updates=int(delivery["patch_updates"]),
            reset_updates=int(delivery["reset_updates"]),
            serialized_bytes=int(delivery["serialized_bytes"]),
            per_view_updates={
                str(key): int(value)
                for key, value in delivery["per_view_updates"].items()
            },
            queue_bytes_at_end=int(delivery["queue_bytes_at_end"]),
            queue_high_water_bytes=int(delivery["queue_high_water_bytes"]),
            view_set_reset_count=int(delivery["view_set_reset_count"]),
        )
        print(
            f"case_result run={run} mode={case['mode']} "
            f"transfer_seconds={result.transfer_seconds:.3f} "
            f"throughput_mib_s={result.throughput_mib_s:.3f} "
            f"cpu_seconds={result.process_cpu_seconds} peak_rss_bytes={result.peak_rss_bytes} "
            f"batches={result.batches} updates={result.updates} "
            f"delivery_bytes={result.serialized_bytes} "
            f"queue_high_water={result.queue_high_water_bytes} "
            f"resets={result.view_set_reset_count} sha1={payload_hash}",
            flush=True,
        )
    except BaseException as error:
        failure = error
    finally:
        if process.returncode is None:
            process.kill()
            process.wait(timeout=5)
        try:
            shutil.rmtree(case_root)
            cleanup_succeeded = not case_root.exists()
        except OSError as error:
            cleanup_succeeded = False
            if failure is None:
                failure = error
        if result is not None:
            result.cleanup_succeeded = cleanup_succeeded
    if failure is not None:
        raise ScenarioFailure(
            f"application mode {case['mode']} failed: {failure}; "
            f"cleanup={'ok' if not case_root.exists() else 'failed'}"
        ) from failure
    if result is None or not result.cleanup_succeeded:
        raise ScenarioFailure(f"application mode {case['mode']} did not clean up")
    return result


def summarize_results(
    results: list[ApplicationResult], cases: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    grouped: dict[str, list[ApplicationResult]] = {}
    for result in results:
        grouped.setdefault(result.mode, []).append(result)
    idle_throughput = statistics.median(
        result.throughput_mib_s for result in grouped["idle"]
    )
    case_by_mode = {case["mode"]: case for case in cases}
    summaries: list[dict[str, Any]] = []
    for mode, cohort in grouped.items():
        throughput = statistics.median(result.throughput_mib_s for result in cohort)
        cpu_values = [
            result.process_cpu_seconds
            for result in cohort
            if result.process_cpu_seconds is not None
        ]
        rss_values = [
            result.peak_rss_bytes
            for result in cohort
            if result.peak_rss_bytes is not None
        ]
        case = case_by_mode[mode]
        summaries.append(
            {
                "baseline_case_id": case["id"],
                "mode": mode,
                "runs": len(cohort),
                "median_transfer_seconds": statistics.median(
                    result.transfer_seconds for result in cohort
                ),
                "median_mib_s": throughput,
                "idle_throughput_ratio": throughput / idle_throughput,
                "median_process_cpu_seconds": (
                    statistics.median(cpu_values) if cpu_values else None
                ),
                "median_peak_rss_bytes": (
                    int(statistics.median(rss_values)) if rss_values else None
                ),
                "median_serialized_bytes": statistics.median(
                    result.serialized_bytes for result in cohort
                ),
                "median_batches": statistics.median(
                    result.batches for result in cohort
                ),
                "median_updates": statistics.median(
                    result.updates for result in cohort
                ),
                "maximum_queue_high_water_bytes": max(
                    result.queue_high_water_bytes for result in cohort
                ),
                "maximum_view_set_reset_count": max(
                    result.view_set_reset_count for result in cohort
                ),
                "observed": case.get("observed"),
                "required": case["required"],
            }
        )
    return sorted(summaries, key=lambda summary: summary["mode"])


def gate_summaries(summaries: list[dict[str, Any]]) -> list[str]:
    failures: list[str] = []
    for summary in summaries:
        required = summary["required"]
        label = f"mode={summary['mode']}"
        minimum_mib_s = required.get("minimum_mib_s")
        if minimum_mib_s is not None and summary["median_mib_s"] < minimum_mib_s:
            failures.append(
                f"{label} median {summary['median_mib_s']:.3f} MiB/s is below "
                f"{minimum_mib_s:.3f} MiB/s"
            )
        minimum_idle_ratio = required.get("minimum_idle_ratio")
        if (
            minimum_idle_ratio is not None
            and summary["idle_throughput_ratio"] < minimum_idle_ratio
        ):
            failures.append(
                f"{label} idle ratio {summary['idle_throughput_ratio']:.3f} is "
                f"below {minimum_idle_ratio:.3f}"
            )
        maximum_resets = required.get("maximum_resets")
        if (
            maximum_resets is not None
            and summary["maximum_view_set_reset_count"] > maximum_resets
        ):
            failures.append(
                f"{label} reset count {summary['maximum_view_set_reset_count']} exceeds "
                f"{maximum_resets}"
            )
        maximum_queue = required.get("maximum_queue_high_water_bytes")
        if (
            maximum_queue is not None
            and summary["maximum_queue_high_water_bytes"] > maximum_queue
        ):
            failures.append(
                f"{label} queue high water {summary['maximum_queue_high_water_bytes']} "
                f"exceeds {maximum_queue} bytes"
            )
    return failures


def parse_arguments(repository: Path) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--baseline-profile",
        required=True,
        help="named tests/perf/baselines profile or TOML path",
    )
    parser.add_argument(
        "--profile-tier",
        choices=("smoke", "full"),
        default="full",
    )
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    try:
        arguments.profile, arguments.profile_path = load_performance_profile(
            repository, arguments.baseline_profile
        )
        arguments.application_cases = selected_application_cases(
            arguments.profile, arguments.profile_tier
        )
    except PerformanceProfileError as error:
        parser.error(str(error))
    application = arguments.profile["application_observation"]
    arguments.runs = profile_runs(application, arguments.profile_tier)
    arguments.size_mib = int(application["size_mib"])
    arguments.piece_size_kib = int(application["piece_size_kib"])
    arguments.timeout_seconds = int(application["timeout_seconds"])
    arguments.slow_consumer_delay_millis = int(
        application["slow_consumer_delay_millis"]
    )
    return arguments


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    arguments = parse_arguments(repository)
    temporary_root = Path(tempfile.gettempdir())
    hardware_environment = collect_hardware_environment(temporary_root)
    applicability_failures = validate_environment(
        arguments.profile, hardware_environment
    )
    if applicability_failures:
        report = {
            "schema_version": 1,
            "scenario": "sqlite-application-view-throughput",
            "status": "not_applicable",
            "environment": hardware_environment,
            "baseline_profile": {
                "profile_id": arguments.profile["profile_id"],
                "tier": arguments.profile_tier,
                "calibration_status": arguments.profile["calibration_status"],
            },
            "applicability_failures": applicability_failures,
            "results": [],
            "summaries": [],
            "gate": {"passed": False, "failures": []},
        }
        rendered = json.dumps(report, indent=2, sort_keys=True)
        if arguments.output is not None:
            arguments.output.parent.mkdir(parents=True, exist_ok=True)
            arguments.output.write_text(rendered + "\n", encoding="utf-8")
        print(rendered)
        for failure in applicability_failures:
            print(f"application profile is not applicable: {failure}", file=sys.stderr)
        return 2

    payload_bytes = arguments.size_mib * MIB
    required_free = payload_bytes * 2 + 2 * GIB
    available = shutil.disk_usage(temporary_root).free
    if available < required_free:
        print(
            f"insufficient temporary disk: need {required_free} bytes, have {available}",
            file=sys.stderr,
        )
        return 2
    try:
        binary = (arguments.binary or build_binary(repository)).resolve()
        if not binary.is_file():
            raise ScenarioFailure(f"application profile binary is absent: {binary}")
    except (OSError, ScenarioFailure, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1

    environment = {
        **hardware_environment,
        "repository_commit": command_text(["git", "rev-parse", "HEAD"], repository),
        "repository_dirty": bool(
            command_text(["git", "status", "--porcelain"], repository)
        ),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "rustc": command_text(["rustc", "--version"], repository),
        "libtorrent": lt.version,
        "rstorrent_binary_sha256": binary_sha256(binary),
        "source_cache_policy": "warm-uncontrolled-os-page-cache",
    }
    results: list[ApplicationResult] = []
    diagnostics: list[str] = []
    seed_session = None
    seed_handle = None
    started = time.monotonic()
    try:
        with tempfile.TemporaryDirectory(
            prefix="rstorrent-application-throughput-"
        ) as temporary:
            owned_root = Path(temporary)
            fixture = create_fixture(
                owned_root / "fixture",
                payload_size=payload_bytes,
                piece_size=arguments.piece_size_kib * 1024,
            )
            seed_session = create_session()
            peer_port = wait_for_listener(seed_session, diagnostics)
            seed_handle = add_seed(
                seed_session,
                fixture.torrent_info,
                fixture.seed_directory,
                diagnostics,
            )
            case_ordinal = 0
            for run in range(1, arguments.runs + 1):
                rotation = case_ordinal % len(arguments.application_cases)
                order = (
                    arguments.application_cases[rotation:]
                    + arguments.application_cases[:rotation]
                )
                for case_order, case in enumerate(order, start=1):
                    results.append(
                        run_case(
                            binary,
                            fixture,
                            peer_port,
                            case,
                            run,
                            case_order,
                            owned_root,
                            arguments.timeout_seconds,
                            arguments.slow_consumer_delay_millis,
                        )
                    )
                case_ordinal += 1
    except (OSError, ScenarioFailure, subprocess.SubprocessError, json.JSONDecodeError) as error:
        diagnostic_text = "\n".join(diagnostics[-100:]) or "(no libtorrent alerts)"
        print(f"application throughput failed: {error}\n{diagnostic_text}", file=sys.stderr)
        return 1
    finally:
        if seed_session is not None:
            try:
                diagnostics.extend(alert.message() for alert in seed_session.pop_alerts())
                if seed_handle is not None and seed_handle.is_valid():
                    seed_session.remove_torrent(seed_handle)
                seed_session.pause()
            except Exception:
                pass
        seed_handle = None
        seed_session = None
        gc.collect()

    summaries = summarize_results(results, arguments.application_cases)
    gate_failures = gate_summaries(summaries)
    ranking = sorted(
        (
            {
                "mode": summary["mode"],
                "idle_throughput_ratio": summary["idle_throughput_ratio"],
                "median_mib_s": summary["median_mib_s"],
                "median_process_cpu_seconds": summary["median_process_cpu_seconds"],
                "median_serialized_bytes": summary["median_serialized_bytes"],
            }
            for summary in summaries
            if summary["mode"] != "idle"
        ),
        key=lambda item: (item["idle_throughput_ratio"], item["mode"]),
    )
    report = {
        "schema_version": 1,
        "scenario": "sqlite-application-view-throughput",
        "status": "passed" if not gate_failures else "regression",
        "environment": environment,
        "baseline_profile": {
            "profile_id": arguments.profile["profile_id"],
            "tier": arguments.profile_tier,
            "calibration_status": arguments.profile["calibration_status"],
            "path": report_path(arguments.profile_path, repository),
            "requirements": arguments.profile["requirements"],
        },
        "config": {
            "size_mib": arguments.size_mib,
            "piece_size_kib": arguments.piece_size_kib,
            "runs": arguments.runs,
            "timeout_seconds": arguments.timeout_seconds,
            "storage_point": "4/4",
            "slow_consumer_delay_millis": arguments.slow_consumer_delay_millis,
            "client_order": "rotating-by-run",
            "cases": arguments.application_cases,
        },
        "elapsed_seconds": time.monotonic() - started,
        "results": [asdict(result) for result in results],
        "summaries": summaries,
        "worst_view_ranking": ranking,
        "gate": {"passed": not gate_failures, "failures": gate_failures},
    }
    rendered = json.dumps(report, indent=2, sort_keys=True)
    if arguments.output is not None:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(rendered + "\n", encoding="utf-8")
    print(rendered)
    if gate_failures:
        for failure in gate_failures:
            print(f"application throughput gate failed: {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
