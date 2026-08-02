#!/usr/bin/env python3
"""Measure raw positional-write and SHA-1 ceilings across bounded worker points."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any


MIB = 1024 * 1024
GIB = 1024 * MIB
MAX_SIZE_MIB = 10 * 1024
MAX_CONCURRENCY = 8
STAGES = ("write", "file_hash_warm", "memory_hash", "combined")


class ProfileFailure(RuntimeError):
    pass


def parse_storage_point(value: str) -> tuple[int, int]:
    try:
        write_text, hash_text = value.split("/", maxsplit=1)
        write_concurrency = int(write_text)
        hash_concurrency = int(hash_text)
    except ValueError as error:
        raise argparse.ArgumentTypeError(
            f"storage point {value!r} must have the form WRITE/HASH"
        ) from error
    if not 1 <= write_concurrency <= MAX_CONCURRENCY:
        raise argparse.ArgumentTypeError(
            f"write concurrency must be from 1 through {MAX_CONCURRENCY}"
        )
    if not 1 <= hash_concurrency <= MAX_CONCURRENCY:
        raise argparse.ArgumentTypeError(
            f"hash concurrency must be from 1 through {MAX_CONCURRENCY}"
        )
    return write_concurrency, hash_concurrency


def power_of_two_kib(value: str) -> int:
    try:
        kib = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("size must be an integer") from error
    if kib < 16 or kib > 256 * 1024 or kib & (kib - 1):
        raise argparse.ArgumentTypeError(
            "size must be a power of two from 16 through 262144 KiB"
        )
    return kib


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--size-mib", type=int, default=10 * 1024)
    parser.add_argument("--piece-size-kib", type=power_of_two_kib, default=4096)
    parser.add_argument("--write-chunk-kib", type=power_of_two_kib, default=256)
    parser.add_argument(
        "--write-order",
        choices=("sequential", "permuted"),
        default="permuted",
    )
    parser.add_argument(
        "--storage-points",
        nargs="+",
        type=parse_storage_point,
        default=[(1, 1), (2, 2), (4, 4), (8, 4), (8, 8)],
        metavar="WRITE/HASH",
    )
    parser.add_argument("--runs", type=int, choices=range(1, 4), default=1)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    if not 1 <= arguments.size_mib <= MAX_SIZE_MIB:
        parser.error(f"--size-mib must be from 1 through {MAX_SIZE_MIB}")
    if arguments.write_chunk_kib > 256:
        parser.error("--write-chunk-kib must not exceed 256 KiB")
    if arguments.piece_size_kib % arguments.write_chunk_kib:
        parser.error("piece size must be an exact multiple of write-chunk size")
    size_kib = arguments.size_mib * 1024
    if size_kib % arguments.piece_size_kib:
        parser.error("profile size must be an exact multiple of piece size")
    arguments.storage_points = list(dict.fromkeys(arguments.storage_points))
    return arguments


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


def binary_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(MIB):
            digest.update(chunk)
    return digest.hexdigest()


def build_binary(repository: Path) -> Path:
    subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "rstorrent-engine",
            "--bin",
            "rstorrent-storage-stage-profile",
        ],
        cwd=repository,
        check=True,
    )
    suffix = ".exe" if sys.platform == "win32" else ""
    return repository / "target" / "debug" / f"rstorrent-storage-stage-profile{suffix}"


def validate_report(
    report: dict[str, Any],
    size_bytes: int,
    piece_size: int,
    write_chunk_size: int,
    storage_point: tuple[int, int],
    write_order: str,
) -> None:
    write_concurrency, hash_concurrency = storage_point
    expected = {
        "schema_version": 1,
        "scenario": "raw-positional-write-and-sha1",
        "size_bytes": size_bytes,
        "piece_size": piece_size,
        "write_chunk_size": write_chunk_size,
        "write_concurrency": write_concurrency,
        "hash_concurrency": hash_concurrency,
        "write_order": write_order,
        "piece_count": size_bytes // piece_size,
        "combined_queue_capacity": write_concurrency + hash_concurrency,
        "cleanup_succeeded": True,
    }
    for key, value in expected.items():
        if report.get(key) != value:
            raise ProfileFailure(
                f"profile field {key} is {report.get(key)!r}, expected {value!r}"
            )
    if report.get("materialized_allocated_bytes", 0) < size_bytes:
        raise ProfileFailure("materialized raw-write file was not fully allocated")
    if report.get("combined_allocated_bytes", 0) < size_bytes:
        raise ProfileFailure("combined raw-write file was not fully allocated")
    stages = report.get("stages")
    if not isinstance(stages, list) or [stage.get("stage") for stage in stages] != list(
        STAGES
    ):
        raise ProfileFailure("profile returned the wrong stage sequence")
    expected_writes = size_bytes // write_chunk_size
    expected_hashes = size_bytes // piece_size
    for stage in stages:
        if stage["wall_seconds"] <= 0 or stage["throughput_mib_s"] <= 0:
            raise ProfileFailure(f"stage {stage['stage']} returned nonpositive timing")
        write = stage.get("write")
        if write is not None and write["operations"] != expected_writes:
            raise ProfileFailure(f"stage {stage['stage']} returned wrong write count")
        hashed = stage.get("hash")
        if hashed is not None and hashed["operations"] != expected_hashes:
            raise ProfileFailure(f"stage {stage['stage']} returned wrong hash count")
    combined = stages[-1]
    maximum_backlog = write_concurrency * 2 + hash_concurrency
    if not 0 < combined["queue_high_water"] <= maximum_backlog:
        raise ProfileFailure("combined ready backlog exceeded its explicit bound")


def run_profile(
    binary: Path,
    root: Path,
    size_mib: int,
    piece_size_kib: int,
    write_chunk_kib: int,
    run: int,
    order: int,
    storage_point: tuple[int, int],
    write_order: str,
) -> dict[str, Any]:
    write_concurrency, hash_concurrency = storage_point
    output_path = root / (
        f"run-{run}-order-{order}-w{write_concurrency}-h{hash_concurrency}.bin"
    )
    command = [
        str(binary),
        "--path",
        str(output_path),
        "--size-mib",
        str(size_mib),
        "--piece-size-kib",
        str(piece_size_kib),
        "--write-chunk-kib",
        str(write_chunk_kib),
        "--write-concurrency",
        str(write_concurrency),
        "--hash-concurrency",
        str(hash_concurrency),
        "--write-order",
        write_order,
    ]
    print(
        f"profile_start run={run} order={order} "
        f"storage={write_concurrency}/{hash_concurrency}",
        flush=True,
    )
    started = time.monotonic()
    completed = subprocess.run(
        command,
        capture_output=True,
        text=True,
        timeout=30 * 60,
        check=False,
    )
    process_seconds = time.monotonic() - started
    if completed.returncode != 0:
        raise ProfileFailure(
            f"profile exited with {completed.returncode}: "
            f"stdout={completed.stdout!r} stderr={completed.stderr!r}"
        )
    try:
        report = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise ProfileFailure(f"profile returned invalid JSON: {error}") from error
    validate_report(
        report,
        size_mib * MIB,
        piece_size_kib * 1024,
        write_chunk_kib * 1024,
        storage_point,
        write_order,
    )
    if output_path.exists() or output_path.with_name(output_path.name + ".combined").exists():
        raise ProfileFailure("profile left an owned data file behind")
    report["run"] = run
    report["order"] = order
    report["process_seconds"] = process_seconds
    print(
        f"profile_result run={run} storage={write_concurrency}/{hash_concurrency} "
        + " ".join(
            f"{stage['stage']}={stage['throughput_mib_s']:.1f}MiB/s"
            for stage in report["stages"]
        ),
        flush=True,
    )
    return report


def summarize(results: list[dict[str, Any]]) -> list[dict[str, Any]]:
    summaries: list[dict[str, Any]] = []
    points = sorted(
        {(result["write_concurrency"], result["hash_concurrency"]) for result in results}
    )
    for write_concurrency, hash_concurrency in points:
        cohort = [
            result
            for result in results
            if result["write_concurrency"] == write_concurrency
            and result["hash_concurrency"] == hash_concurrency
        ]
        stages: dict[str, Any] = {}
        for stage_name in STAGES:
            stage_cohort = [
                next(stage for stage in result["stages"] if stage["stage"] == stage_name)
                for result in cohort
            ]
            stages[stage_name] = {
                "median_wall_seconds": statistics.median(
                    stage["wall_seconds"] for stage in stage_cohort
                ),
                "median_mib_s": statistics.median(
                    stage["throughput_mib_s"] for stage in stage_cohort
                ),
                "median_sync_seconds": (
                    statistics.median(
                        stage["sync_seconds"] for stage in stage_cohort
                    )
                    if stage_cohort[0]["sync_seconds"] is not None
                    else None
                ),
            }
        summaries.append(
            {
                "write_concurrency": write_concurrency,
                "hash_concurrency": hash_concurrency,
                "runs": len(cohort),
                "stages": stages,
            }
        )
    return summaries


def main() -> int:
    arguments = parse_arguments()
    repository = Path(__file__).resolve().parents[2]
    try:
        binary = (arguments.binary or build_binary(repository)).resolve()
    except subprocess.SubprocessError as error:
        print(f"storage stage profile failed to build: {error}", file=sys.stderr)
        return 1
    if not binary.is_file():
        print(f"storage stage profile binary is absent: {binary}", file=sys.stderr)
        return 2
    required_free = arguments.size_mib * MIB * 2 + 4 * GIB
    available = shutil.disk_usage(tempfile.gettempdir()).free
    if available < required_free:
        print(
            f"insufficient temporary disk: need {required_free} bytes, have {available}",
            file=sys.stderr,
        )
        return 2

    results: list[dict[str, Any]] = []
    started = time.monotonic()
    try:
        with tempfile.TemporaryDirectory(prefix="rstorrent-storage-stage-") as temporary:
            root = Path(temporary)
            for run in range(1, arguments.runs + 1):
                rotation = (run - 1) % len(arguments.storage_points)
                points = (
                    arguments.storage_points[rotation:]
                    + arguments.storage_points[:rotation]
                )
                for order, storage_point in enumerate(points, start=1):
                    results.append(
                        run_profile(
                            binary,
                            root,
                            arguments.size_mib,
                            arguments.piece_size_kib,
                            arguments.write_chunk_kib,
                            run,
                            order,
                            storage_point,
                            arguments.write_order,
                        )
                    )
            if any(root.iterdir()):
                raise ProfileFailure("profile temporary root is not empty")
    except (OSError, ProfileFailure, subprocess.SubprocessError) as error:
        print(f"storage stage profile failed: {error}", file=sys.stderr)
        return 1

    report = {
        "schema_version": 1,
        "scenario": "raw-storage-stage-concurrency-sweep",
        "environment": {
            "repository_commit": command_text(
                ["git", "rev-parse", "HEAD"], repository
            ),
            "repository_dirty": bool(
                command_text(["git", "status", "--porcelain"], repository)
            ),
            "platform": platform.platform(),
            "architecture": platform.machine(),
            "python": platform.python_version(),
            "rustc": command_text(["rustc", "--version"], repository),
            "binary_sha256": binary_sha256(binary),
            "available_temporary_bytes_before": available,
            "cache_policy": "warm-os-page-cache; sync measured separately",
        },
        "config": {
            "size_mib": arguments.size_mib,
            "piece_size_kib": arguments.piece_size_kib,
            "write_chunk_kib": arguments.write_chunk_kib,
            "write_order": arguments.write_order,
            "storage_points": [
                {
                    "write_concurrency": write_concurrency,
                    "hash_concurrency": hash_concurrency,
                }
                for write_concurrency, hash_concurrency in arguments.storage_points
            ],
            "runs": arguments.runs,
            "point_order": "rotating-by-run",
        },
        "elapsed_seconds": time.monotonic() - started,
        "summaries": summarize(results),
        "results": results,
    }
    rendered = json.dumps(report, indent=2, sort_keys=True)
    if arguments.output is not None:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(rendered + "\n", encoding="utf-8")
    print(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
