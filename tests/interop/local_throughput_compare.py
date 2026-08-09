#!/usr/bin/env python3
"""Compare RSTorrent and libtorrent on large controlled loopback torrents."""

from __future__ import annotations

import argparse
import gc
import hashlib
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
    build_diagnostic,
    create_session,
    hash_file,
    wait_for_listener,
)
from performance_profiles import (
    PerformanceProfileError,
    collect_hardware_environment,
    load_performance_profile,
    profile_runs,
    selected_throughput_cases,
    validate_environment,
)


MIB = 1024 * 1024
GIB = 1024 * MIB
BLOCK_SIZE = 16 * 1024
PAYLOAD_NAME = "payload.bin"
SOURCE_CHUNK_SIZE = MIB
MAX_TOTAL_SIZE = 10 * GIB
MAX_PIECE_SIZE = 256 * MIB
MIN_PIECE_SIZE = 16 * 1024
MAX_TIMEOUT_SECONDS = 4 * 60 * 60
PAYLOAD_ALLOWANCE = 64 * MIB
POLL_SECONDS = 0.05


@dataclass(frozen=True)
class Fixture:
    size_bytes: int
    root: Path
    seed_root: Path
    payload_path: Path
    expected_sha1: str
    allocated_bytes: int
    torrents: dict[int, Path]


@dataclass(frozen=True)
class TransferResult:
    baseline_case_id: str | None
    size_bytes: int
    piece_size: int
    piece_count: int
    run: int
    order: int
    implementation: str
    encryption: str | None
    oracle_mse_method: str | None
    version: str
    write_concurrency: int | None
    hash_concurrency: int | None
    transfer_seconds: float
    throughput_mib_s: float
    cpu_seconds: float
    cpu_core_equivalents: float
    cpu_logical_capacity_percent: float
    validation_seconds: float
    payload_sha1: str
    payload_bytes: int
    payload_download_bytes: int
    redundant_bytes: int
    failed_bytes: int
    write_operations: int | None
    write_blocks: int | None
    write_service_seconds: float | None
    write_active_high_water: int | None
    hash_operations: int | None
    hash_service_seconds: float | None
    hash_active_high_water: int | None
    payload_high_water: int | None
    cleanup_succeeded: bool


def deterministic_source(path: Path, size_bytes: int) -> tuple[str, int]:
    """Write nonzero, position-varying material without retaining the payload."""
    pattern = bytearray(
        ((offset * 73) ^ (offset >> 3) ^ (offset * offset >> 11) ^ 0xA5) & 0xFF
        for offset in range(SOURCE_CHUNK_SIZE)
    )
    digest = hashlib.sha1()
    remaining = size_bytes
    chunk_index = 0
    with path.open("xb", buffering=0) as output:
        while remaining:
            length = min(len(pattern), remaining)
            pattern[:8] = chunk_index.to_bytes(8, "little")
            chunk = memoryview(pattern)[:length]
            written = output.write(chunk)
            if written != length:
                raise ScenarioFailure(
                    f"source write made {written} bytes of {length} bytes of progress"
                )
            digest.update(chunk)
            remaining -= length
            chunk_index += 1
        os.fsync(output.fileno())
    stat = path.stat()
    allocated_bytes = getattr(stat, "st_blocks", 0) * 512
    if stat.st_size != size_bytes:
        raise ScenarioFailure(
            f"source size is {stat.st_size} bytes instead of {size_bytes}"
        )
    return digest.hexdigest(), allocated_bytes


def process_tree_cpu_seconds() -> float:
    """Return cumulative CPU time for this harness and reaped child processes."""
    usage = os.times()
    return usage.user + usage.system + usage.children_user + usage.children_system


def create_fixture(root: Path, size_bytes: int, piece_sizes: list[int]) -> Fixture:
    fixture_root = root / f"fixture-{size_bytes}"
    seed_root = fixture_root / "seed"
    seed_root.mkdir(parents=True)
    payload_path = seed_root / PAYLOAD_NAME
    started = time.monotonic()
    expected_sha1, allocated_bytes = deterministic_source(payload_path, size_bytes)
    print(
        f"fixture_source size_bytes={size_bytes} allocated_bytes={allocated_bytes} "
        f"seconds={time.monotonic() - started:.3f} sha1={expected_sha1}",
        flush=True,
    )

    torrents: dict[int, Path] = {}
    for piece_size in piece_sizes:
        files = lt.file_storage()
        files.add_file(PAYLOAD_NAME, size_bytes)
        creator = lt.create_torrent(
            files,
            piece_size=piece_size,
            flags=lt.create_torrent.v1_only,
        )
        started = time.monotonic()
        lt.set_piece_hashes(creator, str(seed_root))
        torrent_path = fixture_root / f"piece-{piece_size}.torrent"
        torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
        info = lt.torrent_info(str(torrent_path))
        expected_pieces = (size_bytes + piece_size - 1) // piece_size
        if info.total_size() != size_bytes:
            raise ScenarioFailure(
                f"torrent size is {info.total_size()} bytes instead of {size_bytes}"
            )
        if info.piece_length() != piece_size:
            raise ScenarioFailure(
                f"torrent piece size is {info.piece_length()} instead of {piece_size}"
            )
        if info.num_pieces() != expected_pieces:
            raise ScenarioFailure(
                f"torrent has {info.num_pieces()} pieces instead of {expected_pieces}"
            )
        if any(True for _ in info.trackers()):
            raise ScenarioFailure("controlled throughput torrent contains a tracker")
        torrents[piece_size] = torrent_path
        print(
            f"fixture_torrent size_bytes={size_bytes} piece_size={piece_size} "
            f"pieces={expected_pieces} hash_seconds={time.monotonic() - started:.3f}",
            flush=True,
        )
    return Fixture(
        size_bytes=size_bytes,
        root=fixture_root,
        seed_root=seed_root,
        payload_path=payload_path,
        expected_sha1=expected_sha1,
        allocated_bytes=allocated_bytes,
        torrents=torrents,
    )


def parse_rstorrent_diagnostic(output: str, fixture: Fixture) -> dict[str, str]:
    values: dict[str, str] = {}
    for token in output.split():
        if "=" in token:
            key, value = token.split("=", 1)
            values[key] = value
    required = {
        "pieces",
        "bytes",
        "blocks",
        "payload_high_water",
        "storage_write_operations",
        "storage_write_blocks",
        "storage_write_service_micros",
        "storage_write_active_high_water",
        "storage_hash_operations",
        "storage_hash_service_micros",
        "storage_hash_active_high_water",
    }
    missing = required - values.keys()
    if missing:
        raise ScenarioFailure(
            f"RSTorrent diagnostic is missing fields {sorted(missing)}: {output}"
        )
    if int(values["bytes"]) != fixture.size_bytes:
        raise ScenarioFailure(
            f"RSTorrent reported {values['bytes']} bytes instead of {fixture.size_bytes}"
        )
    if int(values["blocks"]) != (fixture.size_bytes + BLOCK_SIZE - 1) // BLOCK_SIZE:
        raise ScenarioFailure("RSTorrent reported the wrong logical block count")
    return values


def configure_seed_encryption(session: lt.session, encryption: str | None) -> None:
    if encryption is None:
        return
    if encryption == "disabled":
        session.apply_settings(
            {
                "in_enc_policy": int(lt.enc_policy.pe_disabled),
                "out_enc_policy": int(lt.enc_policy.pe_disabled),
                "allowed_enc_level": int(lt.enc_level.pe_both),
                "prefer_rc4": False,
            }
        )
        return
    if encryption == "required":
        session.apply_settings(
            {
                "in_enc_policy": int(lt.enc_policy.pe_forced),
                "out_enc_policy": int(lt.enc_policy.pe_forced),
                "allowed_enc_level": int(lt.enc_level.pe_rc4),
                "prefer_rc4": True,
            }
        )
        return
    raise ScenarioFailure(f"unsupported paired encryption mode {encryption}")


def oracle_mse_method(handle: lt.torrent_handle) -> str | None:
    for peer in handle.get_peer_info():
        if peer.flags & lt.peer_info.rc4_encrypted:
            return "rc4"
        if peer.flags & lt.peer_info.plaintext_encrypted:
            return "plaintext_payload"
    return None


def run_rstorrent(
    binary: Path,
    fixture: Fixture,
    info: lt.torrent_info,
    torrent_path: Path,
    peer_port: int,
    output_path: Path,
    timeout_seconds: int,
    write_concurrency: int,
    hash_concurrency: int,
    encryption: str | None,
    seed_handle: lt.torrent_handle,
) -> tuple[float, dict[str, Any]]:
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
    ]
    if encryption is not None:
        command.extend(["--encryption", encryption])
    started = time.monotonic()
    process = subprocess.Popen(
        command,
        env={
            **os.environ,
            "RSTORRENT_TEST_STORAGE_WRITE_CONCURRENCY": str(write_concurrency),
            "RSTORRENT_TEST_STORAGE_HASH_CONCURRENCY": str(hash_concurrency),
        },
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    observed_methods: set[str] = set()
    deadline = started + timeout_seconds + 30
    while process.poll() is None:
        method = oracle_mse_method(seed_handle)
        if method is not None:
            observed_methods.add(method)
        if time.monotonic() >= deadline:
            process.kill()
            stdout, stderr = process.communicate()
            raise ScenarioFailure(
                f"RSTorrent exceeded {timeout_seconds + 30} seconds: "
                f"stdout={stdout!r} stderr={stderr!r}"
            )
        time.sleep(0.005)
    stdout, stderr = process.communicate()
    completed = subprocess.CompletedProcess(
        command,
        process.returncode,
        stdout,
        stderr,
    )
    transfer_seconds = time.monotonic() - started
    if completed.returncode != 0:
        raise ScenarioFailure(
            f"RSTorrent exited with {completed.returncode}: "
            f"stdout={completed.stdout!r} stderr={completed.stderr!r}"
        )
    if encryption == "required" and observed_methods != {"rc4"}:
        raise ScenarioFailure(
            "libtorrent did not observe exactly the forced RC4 method: "
            f"{sorted(observed_methods)}"
        )
    if encryption == "disabled" and observed_methods:
        raise ScenarioFailure(
            "libtorrent observed MSE during the ordinary-plain case: "
            f"{sorted(observed_methods)}"
        )
    diagnostic = parse_rstorrent_diagnostic(completed.stdout, fixture)
    expected_pieces = info.num_pieces()
    if diagnostic["pieces"] != f"{expected_pieces}/{expected_pieces}":
        raise ScenarioFailure(
            f"RSTorrent reported pieces={diagnostic['pieces']}, "
            f"expected {expected_pieces}/{expected_pieces}"
        )
    return transfer_seconds, {
        "payload_download_bytes": fixture.size_bytes,
        "redundant_bytes": 0,
        "failed_bytes": 0,
        "write_operations": int(diagnostic["storage_write_operations"]),
        "write_blocks": int(diagnostic["storage_write_blocks"]),
        "write_service_seconds": int(diagnostic["storage_write_service_micros"])
        / 1_000_000,
        "write_active_high_water": int(diagnostic["storage_write_active_high_water"]),
        "hash_operations": int(diagnostic["storage_hash_operations"]),
        "hash_service_seconds": int(diagnostic["storage_hash_service_micros"])
        / 1_000_000,
        "hash_active_high_water": int(diagnostic["storage_hash_active_high_water"]),
        "payload_high_water": int(diagnostic["payload_high_water"]),
        "oracle_mse_method": next(iter(observed_methods), None),
    }


def run_libtorrent(
    fixture: Fixture,
    info: lt.torrent_info,
    peer_port: int,
    output_root: Path,
    timeout_seconds: int,
) -> tuple[float, dict[str, Any]]:
    output_root.mkdir(parents=True)
    session = create_session()
    handle: lt.torrent_handle | None = None
    last_status: Any | None = None
    started = time.monotonic()
    try:
        parameters = lt.add_torrent_params()
        parameters.ti = info
        parameters.save_path = str(output_root)
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        parameters.flags |= lt.torrent_flags.disable_dht
        parameters.flags |= lt.torrent_flags.disable_lsd
        parameters.flags |= lt.torrent_flags.disable_pex
        handle = session.add_torrent(parameters)
        handle.connect_peer(("127.0.0.1", peer_port))
        deadline = started + timeout_seconds
        while True:
            status = handle.status()
            last_status = status
            error = status.errc
            if error.value() != 0:
                raise ScenarioFailure(f"libtorrent client failed: {error.message()}")
            if status.is_seeding:
                break
            if time.monotonic() >= deadline:
                raise ScenarioFailure(
                    f"libtorrent exceeded {timeout_seconds} seconds at "
                    f"{status.total_wanted_done}/{fixture.size_bytes} bytes"
                )
            time.sleep(POLL_SECONDS)
        transfer_seconds = time.monotonic() - started
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()
    if last_status is None:
        raise ScenarioFailure("libtorrent returned no terminal status")
    return transfer_seconds, {
        "payload_download_bytes": int(last_status.total_payload_download),
        "redundant_bytes": int(last_status.total_redundant_bytes),
        "failed_bytes": int(last_status.total_failed_bytes),
        "write_operations": None,
        "write_blocks": None,
        "write_service_seconds": None,
        "write_active_high_water": None,
        "hash_operations": None,
        "hash_service_seconds": None,
        "hash_active_high_water": None,
        "payload_high_water": None,
    }


def run_transfer(
    implementation: str,
    binary: Path,
    fixture: Fixture,
    piece_size: int,
    run: int,
    order: int,
    case_root: Path,
    timeout_seconds: int,
    write_concurrency: int,
    hash_concurrency: int,
    baseline_case_id: str | None = None,
    encryption: str | None = None,
) -> TransferResult:
    torrent_path = fixture.torrents[piece_size]
    info = lt.torrent_info(str(torrent_path))
    alerts: list[str] = []
    seed_session = create_session()
    configure_seed_encryption(seed_session, encryption)
    seed_handle: lt.torrent_handle | None = None
    cleanup_succeeded = False
    case_label = (
        f"rstorrent-{encryption or 'default'}-w{write_concurrency}-h{hash_concurrency}"
        if implementation == "rstorrent"
        else implementation
    )
    output_root = case_root / case_label
    output_path = output_root / PAYLOAD_NAME
    try:
        peer_port = wait_for_listener(seed_session, alerts)
        seed_handle = add_seed(seed_session, info, fixture.seed_root, alerts)
        print(
            f"case_start size_bytes={fixture.size_bytes} piece_size={piece_size} "
            f"pieces={info.num_pieces()} run={run} order={order} "
            f"implementation={implementation} "
            f"encryption={encryption or 'default'} "
            f"write_concurrency="
            f"{write_concurrency if implementation == 'rstorrent' else 'n/a'} "
            f"hash_concurrency="
            f"{hash_concurrency if implementation == 'rstorrent' else 'n/a'}",
            flush=True,
        )
        cpu_started = process_tree_cpu_seconds()
        if implementation == "rstorrent":
            output_root.mkdir(parents=True)
            transfer_seconds, metrics = run_rstorrent(
                binary,
                fixture,
                info,
                torrent_path,
                peer_port,
                output_path,
                timeout_seconds,
                write_concurrency,
                hash_concurrency,
                encryption,
                seed_handle,
            )
            version = binary_sha256(binary)
        elif implementation == "libtorrent":
            transfer_seconds, metrics = run_libtorrent(
                fixture,
                info,
                peer_port,
                output_root,
                timeout_seconds,
            )
            version = lt.version
        else:
            raise ScenarioFailure(f"unknown implementation {implementation}")
        cpu_seconds = max(0.0, process_tree_cpu_seconds() - cpu_started)
        cpu_core_equivalents = cpu_seconds / transfer_seconds
        cpu_logical_capacity_percent = (
            cpu_core_equivalents / max(1, os.cpu_count() or 1) * 100.0
        )

        if not output_path.is_file():
            raise ScenarioFailure(f"{implementation} did not publish {output_path}")
        if output_path.stat().st_size != fixture.size_bytes:
            raise ScenarioFailure(
                f"{implementation} output has {output_path.stat().st_size} bytes "
                f"instead of {fixture.size_bytes}"
            )
        validation_started = time.monotonic()
        actual_sha1 = hash_file(output_path)
        validation_seconds = time.monotonic() - validation_started
        if actual_sha1 != fixture.expected_sha1:
            raise ScenarioFailure(
                f"{implementation} output SHA-1 {actual_sha1} differs from "
                f"{fixture.expected_sha1}"
            )
        shutil.rmtree(output_root)
        cleanup_succeeded = not output_root.exists()
        result = TransferResult(
            baseline_case_id=baseline_case_id,
            size_bytes=fixture.size_bytes,
            piece_size=piece_size,
            piece_count=info.num_pieces(),
            run=run,
            order=order,
            implementation=implementation,
            encryption=encryption,
            oracle_mse_method=metrics.pop("oracle_mse_method", None),
            version=version,
            write_concurrency=(
                write_concurrency if implementation == "rstorrent" else None
            ),
            hash_concurrency=(
                hash_concurrency if implementation == "rstorrent" else None
            ),
            transfer_seconds=transfer_seconds,
            throughput_mib_s=fixture.size_bytes / MIB / transfer_seconds,
            cpu_seconds=cpu_seconds,
            cpu_core_equivalents=cpu_core_equivalents,
            cpu_logical_capacity_percent=cpu_logical_capacity_percent,
            validation_seconds=validation_seconds,
            payload_sha1=actual_sha1,
            payload_bytes=fixture.size_bytes,
            cleanup_succeeded=cleanup_succeeded,
            **metrics,
        )
        print(
            f"case_result size_bytes={fixture.size_bytes} piece_size={piece_size} "
            f"run={run} implementation={implementation} "
            f"transfer_seconds={transfer_seconds:.3f} "
            f"throughput_mib_s={result.throughput_mib_s:.3f} "
            f"cpu_core_equivalents={cpu_core_equivalents:.3f} "
            f"cpu_logical_capacity_percent={cpu_logical_capacity_percent:.3f} "
            f"validation_seconds={validation_seconds:.3f} sha1={actual_sha1} "
            f"cleanup={'ok' if cleanup_succeeded else 'failed'}",
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


def validate_piece_sizes(values: list[int]) -> list[int]:
    sizes: list[int] = []
    for kib in values:
        size = kib * 1024
        if not MIN_PIECE_SIZE <= size <= MAX_PIECE_SIZE or size & (size - 1):
            raise argparse.ArgumentTypeError(
                f"piece size {kib} KiB must be a power of two between 16 KiB "
                f"and {MAX_PIECE_SIZE // 1024} KiB"
            )
        if size not in sizes:
            sizes.append(size)
    return sizes


def bounded_timeout(value: str) -> int:
    try:
        seconds = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("timeout must be an integer") from error
    if not 1 <= seconds <= MAX_TIMEOUT_SECONDS:
        raise argparse.ArgumentTypeError(
            f"timeout must be between 1 and {MAX_TIMEOUT_SECONDS} seconds"
        )
    return seconds


def positive_float(value: str) -> float:
    try:
        parsed = float(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("value must be a number") from error
    if parsed <= 0:
        raise argparse.ArgumentTypeError("value must be greater than zero")
    return parsed


def parse_storage_point(value: str) -> tuple[int, int]:
    try:
        write_text, hash_text = value.split("/", maxsplit=1)
        write_concurrency = int(write_text)
        hash_concurrency = int(hash_text)
    except (ValueError, TypeError) as error:
        raise argparse.ArgumentTypeError(
            f"storage point {value!r} must have the form WRITE/HASH"
        ) from error
    if not 1 <= write_concurrency <= 8 or not 1 <= hash_concurrency <= 8:
        raise argparse.ArgumentTypeError(
            f"storage point {value!r} must use values from 1 through 8"
        )
    return write_concurrency, hash_concurrency


def parse_arguments(repository: Path) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--sizes-mib",
        nargs="+",
        type=int,
        metavar="MIB",
        help="payload sizes in MiB (default: 1024 10240)",
    )
    parser.add_argument(
        "--piece-sizes-kib",
        nargs="+",
        type=int,
        metavar="KIB",
    )
    parser.add_argument("--runs", type=int, choices=range(1, 21))
    parser.add_argument(
        "--timeout-seconds",
        type=bounded_timeout,
        metavar="SECONDS",
    )
    parser.add_argument("--write-concurrency", type=int, choices=range(1, 9))
    parser.add_argument("--hash-concurrency", type=int, choices=range(1, 9))
    parser.add_argument(
        "--storage-points",
        nargs="+",
        type=parse_storage_point,
        metavar="WRITE/HASH",
        help=(
            "RSTorrent write/hash concurrency points to compare against one "
            "libtorrent client run per workload"
        ),
    )
    parser.add_argument(
        "--minimum-rstorrent-mib-s",
        type=positive_float,
        help="fail when any RSTorrent cohort median is below this throughput",
    )
    parser.add_argument(
        "--minimum-rstorrent-libtorrent-ratio",
        type=positive_float,
        help=(
            "fail when any RSTorrent cohort median throughput divided by the "
            "matching libtorrent median is below this ratio"
        ),
    )
    parser.add_argument(
        "--encryption-pair",
        action="store_true",
        help=(
            "compare matched RSTorrent ordinary-plain and forced-RC4 runs "
            "instead of comparing RSTorrent with libtorrent"
        ),
    )
    parser.add_argument(
        "--minimum-rc4-plain-ratio",
        type=positive_float,
        help=(
            "minimum median within-pair RC4/plain throughput ratio in "
            "--encryption-pair mode (default: 0.75)"
        ),
    )
    parser.add_argument(
        "--baseline-profile",
        help=(
            "named tests/perf/baselines profile or TOML path; cannot be combined "
            "with free-form workload or gate options"
        ),
    )
    parser.add_argument(
        "--profile-tier",
        choices=("smoke", "full"),
        default="full",
        help="workload tier selected from --baseline-profile (default: full)",
    )
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path, help="optional JSON result path")
    arguments = parser.parse_args()

    direct_options = {
        "--sizes-mib": arguments.sizes_mib,
        "--piece-sizes-kib": arguments.piece_sizes_kib,
        "--runs": arguments.runs,
        "--timeout-seconds": arguments.timeout_seconds,
        "--write-concurrency": arguments.write_concurrency,
        "--hash-concurrency": arguments.hash_concurrency,
        "--storage-points": arguments.storage_points,
        "--minimum-rstorrent-mib-s": arguments.minimum_rstorrent_mib_s,
        "--minimum-rstorrent-libtorrent-ratio": (
            arguments.minimum_rstorrent_libtorrent_ratio
        ),
        "--encryption-pair": arguments.encryption_pair or None,
        "--minimum-rc4-plain-ratio": arguments.minimum_rc4_plain_ratio,
    }
    arguments.profile = None
    arguments.profile_path = None
    if arguments.baseline_profile is not None:
        conflicts = [name for name, value in direct_options.items() if value is not None]
        if conflicts:
            parser.error(
                "--baseline-profile cannot be combined with " + ", ".join(conflicts)
            )
        try:
            arguments.profile, arguments.profile_path = load_performance_profile(
                repository, arguments.baseline_profile
            )
            selected = selected_throughput_cases(
                arguments.profile, arguments.profile_tier
            )
        except PerformanceProfileError as error:
            parser.error(str(error))
        arguments.throughput_cases = selected
        arguments.runs = profile_runs(
            arguments.profile["throughput"], arguments.profile_tier
        )
        arguments.timeout_seconds = int(
            arguments.profile["throughput"]["timeout_seconds"]
        )
        arguments.sizes_mib = list(dict.fromkeys(case["size_mib"] for case in selected))
        arguments.piece_sizes_kib = list(
            dict.fromkeys(case["piece_size_kib"] for case in selected)
        )
        arguments.piece_sizes = [piece * 1024 for piece in arguments.piece_sizes_kib]
        arguments.storage_points = list(
            dict.fromkeys(
                (case["write_concurrency"], case["hash_concurrency"])
                for case in selected
            )
        )
        return arguments

    arguments.sizes_mib = arguments.sizes_mib or [1024, 10 * 1024]
    arguments.piece_sizes_kib = arguments.piece_sizes_kib or [256, 1024, 4096, 16384]
    arguments.runs = arguments.runs or (6 if arguments.encryption_pair else 1)
    arguments.timeout_seconds = arguments.timeout_seconds or 2 * 60 * 60
    if arguments.encryption_pair:
        arguments.minimum_rc4_plain_ratio = (
            arguments.minimum_rc4_plain_ratio or 0.75
        )
    elif arguments.minimum_rc4_plain_ratio is not None:
        parser.error("--minimum-rc4-plain-ratio requires --encryption-pair")
    if any(size <= 0 or size * MIB > MAX_TOTAL_SIZE for size in arguments.sizes_mib):
        parser.error(
            f"--sizes-mib values must be between 1 and {MAX_TOTAL_SIZE // MIB}"
        )
    try:
        arguments.piece_sizes = validate_piece_sizes(arguments.piece_sizes_kib)
    except argparse.ArgumentTypeError as error:
        parser.error(str(error))
    if arguments.storage_points is not None and (
        arguments.write_concurrency is not None
        or arguments.hash_concurrency is not None
    ):
        parser.error(
            "--storage-points cannot be combined with --write-concurrency or "
            "--hash-concurrency"
        )
    if arguments.storage_points is None:
        arguments.storage_points = [
            (
                arguments.write_concurrency or 4,
                arguments.hash_concurrency or 4,
            )
        ]
    else:
        arguments.storage_points = list(dict.fromkeys(arguments.storage_points))
    if arguments.encryption_pair and arguments.runs < 6:
        parser.error("--encryption-pair requires at least six --runs")
    arguments.throughput_cases = [
        {
            "id": None,
            "size_mib": size_mib,
            "piece_size_kib": piece_size // 1024,
            "write_concurrency": write_concurrency,
            "hash_concurrency": hash_concurrency,
            "observed": None,
            "required": None,
        }
        for size_mib in arguments.sizes_mib
        for piece_size in arguments.piece_sizes
        for write_concurrency, hash_concurrency in arguments.storage_points
    ]
    return arguments


def build_release_diagnostic(repository: Path) -> Path:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "-p",
            "rstorrent-engine",
            "--bin",
            "rstorrent-download-piece",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to build release throughput diagnostic\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    binary = repository / "target/release/rstorrent-download-piece"
    if not binary.is_file():
        raise ScenarioFailure(f"release throughput diagnostic was not created at {binary}")
    return binary


def summarize_results(results: list[TransferResult]) -> list[dict[str, Any]]:
    libtorrent_groups: dict[tuple[int, int], list[TransferResult]] = {}
    rstorrent_groups: dict[tuple[int, int, int, int], list[TransferResult]] = {}
    for result in results:
        if result.implementation == "libtorrent":
            libtorrent_groups.setdefault(
                (result.size_bytes, result.piece_size), []
            ).append(result)
        else:
            assert result.write_concurrency is not None
            assert result.hash_concurrency is not None
            rstorrent_groups.setdefault(
                (
                    result.size_bytes,
                    result.piece_size,
                    result.write_concurrency,
                    result.hash_concurrency,
                ),
                [],
            ).append(result)

    summaries: list[dict[str, Any]] = []
    for key, cohort in sorted(rstorrent_groups.items()):
        size_bytes, piece_size, write_concurrency, hash_concurrency = key
        reference = libtorrent_groups[(size_bytes, piece_size)]
        rstorrent_seconds = statistics.median(
            result.transfer_seconds for result in cohort
        )
        rstorrent_throughput = statistics.median(
            result.throughput_mib_s for result in cohort
        )
        libtorrent_seconds = statistics.median(
            result.transfer_seconds for result in reference
        )
        libtorrent_throughput = statistics.median(
            result.throughput_mib_s for result in reference
        )
        summaries.append(
            {
                "baseline_case_id": next(
                    iter({result.baseline_case_id for result in cohort})
                ),
                "size_bytes": size_bytes,
                "piece_size": piece_size,
                "runs": len(cohort),
                "write_concurrency": write_concurrency,
                "hash_concurrency": hash_concurrency,
                "rstorrent_median_seconds": rstorrent_seconds,
                "rstorrent_median_mib_s": rstorrent_throughput,
                "rstorrent_median_cpu_core_equivalents": statistics.median(
                    result.cpu_core_equivalents for result in cohort
                ),
                "rstorrent_median_cpu_logical_capacity_percent": statistics.median(
                    result.cpu_logical_capacity_percent for result in cohort
                ),
                "libtorrent_median_seconds": libtorrent_seconds,
                "libtorrent_median_mib_s": libtorrent_throughput,
                "libtorrent_median_cpu_core_equivalents": statistics.median(
                    result.cpu_core_equivalents for result in reference
                ),
                "libtorrent_median_cpu_logical_capacity_percent": statistics.median(
                    result.cpu_logical_capacity_percent for result in reference
                ),
                "rstorrent_libtorrent_ratio": (
                    rstorrent_throughput / libtorrent_throughput
                ),
            }
        )
    return summaries


def summarize_encryption_pairs(results: list[TransferResult]) -> list[dict[str, Any]]:
    groups: dict[tuple[int, int, int, int], dict[int, dict[str, TransferResult]]] = {}
    for result in results:
        if result.implementation != "rstorrent":
            raise ScenarioFailure("encryption pairs may contain only RSTorrent runs")
        if result.write_concurrency is None or result.hash_concurrency is None:
            raise ScenarioFailure("encryption pair is missing storage concurrency")
        if result.encryption not in {"disabled", "required"}:
            raise ScenarioFailure(
                f"unexpected encryption-pair policy {result.encryption!r}"
            )
        key = (
            result.size_bytes,
            result.piece_size,
            result.write_concurrency,
            result.hash_concurrency,
        )
        pair = groups.setdefault(key, {}).setdefault(result.run, {})
        if result.encryption in pair:
            raise ScenarioFailure(
                f"duplicate {result.encryption} result in encryption pair {key} run {result.run}"
            )
        pair[result.encryption] = result

    summaries: list[dict[str, Any]] = []
    for key, runs in sorted(groups.items()):
        size_bytes, piece_size, write_concurrency, hash_concurrency = key
        pairs: list[dict[str, Any]] = []
        for run, pair in sorted(runs.items()):
            if set(pair) != {"disabled", "required"}:
                raise ScenarioFailure(
                    f"incomplete encryption pair {key} run {run}: {sorted(pair)}"
                )
            plain = pair["disabled"]
            rc4 = pair["required"]
            ratio = rc4.throughput_mib_s / plain.throughput_mib_s
            pairs.append(
                {
                    "run": run,
                    "plain_order": plain.order,
                    "rc4_order": rc4.order,
                    "plain_seconds": plain.transfer_seconds,
                    "rc4_seconds": rc4.transfer_seconds,
                    "plain_mib_s": plain.throughput_mib_s,
                    "rc4_mib_s": rc4.throughput_mib_s,
                    "plain_cpu_core_equivalents": plain.cpu_core_equivalents,
                    "rc4_cpu_core_equivalents": rc4.cpu_core_equivalents,
                    "plain_cpu_logical_capacity_percent": (
                        plain.cpu_logical_capacity_percent
                    ),
                    "rc4_cpu_logical_capacity_percent": (
                        rc4.cpu_logical_capacity_percent
                    ),
                    "rc4_plain_ratio": ratio,
                }
            )
        ratios = [pair["rc4_plain_ratio"] for pair in pairs]
        median_ratio = statistics.median(ratios)
        summaries.append(
            {
                "size_bytes": size_bytes,
                "piece_size": piece_size,
                "pairs": len(pairs),
                "write_concurrency": write_concurrency,
                "hash_concurrency": hash_concurrency,
                "plain_median_mib_s": statistics.median(
                    pair["plain_mib_s"] for pair in pairs
                ),
                "rc4_median_mib_s": statistics.median(
                    pair["rc4_mib_s"] for pair in pairs
                ),
                "plain_median_cpu_core_equivalents": statistics.median(
                    pair["plain_cpu_core_equivalents"] for pair in pairs
                ),
                "rc4_median_cpu_core_equivalents": statistics.median(
                    pair["rc4_cpu_core_equivalents"] for pair in pairs
                ),
                "plain_median_cpu_logical_capacity_percent": statistics.median(
                    pair["plain_cpu_logical_capacity_percent"] for pair in pairs
                ),
                "rc4_median_cpu_logical_capacity_percent": statistics.median(
                    pair["rc4_cpu_logical_capacity_percent"] for pair in pairs
                ),
                "median_paired_rc4_plain_ratio": median_ratio,
                "median_paired_regression_percent": (1.0 - median_ratio) * 100.0,
                "diagnostic_ten_percent_target_met": median_ratio >= 0.9,
                "pair_results": pairs,
            }
        )
    return summaries


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    arguments = parse_arguments(repository)
    temporary_root = Path(tempfile.gettempdir())
    hardware_environment = collect_hardware_environment(temporary_root)
    applicability_failures = (
        []
        if arguments.profile is None
        else validate_environment(arguments.profile, hardware_environment)
    )
    if applicability_failures:
        report = {
            "schema_version": 5,
            "scenario": "controlled-single-file-loopback-throughput",
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
            print(f"throughput profile is not applicable: {failure}", file=sys.stderr)
        return 2

    if arguments.binary is not None:
        binary = arguments.binary.resolve()
        binary_profile = "explicit"
    elif arguments.encryption_pair:
        binary = build_release_diagnostic(repository).resolve()
        binary_profile = "release"
    else:
        binary = build_diagnostic(repository).resolve()
        binary_profile = "dev"
    if not binary.is_file():
        print(f"diagnostic binary is absent: {binary}", file=sys.stderr)
        return 2
    sizes = [size * MIB for size in arguments.sizes_mib]
    required_free = max(sizes) * 2 + 4 * GIB
    available = shutil.disk_usage(temporary_root).free
    if available < required_free:
        print(
            f"insufficient temporary disk: need {required_free} bytes, "
            f"have {available}",
            file=sys.stderr,
        )
        return 2

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
        "rstorrent_binary_profile": binary_profile,
        "source_cache_policy": "warm-uncontrolled-os-page-cache",
    }
    throughput_case_by_id = {
        case["id"]: case
        for case in arguments.throughput_cases
        if case["id"] is not None
    }
    workload_cases: dict[tuple[int, int], list[dict[str, Any]]] = {}
    for case in arguments.throughput_cases:
        key = (case["size_mib"] * MIB, case["piece_size_kib"] * 1024)
        workload_cases.setdefault(key, []).append(case)
    results: list[TransferResult] = []
    started = time.monotonic()
    try:
        with tempfile.TemporaryDirectory(prefix="rstorrent-local-throughput-") as temporary:
            owned_root = Path(temporary)
            case_ordinal = 0
            for size_bytes in sizes:
                size_workloads = {
                    piece_size: cases
                    for (case_size, piece_size), cases in workload_cases.items()
                    if case_size == size_bytes
                }
                fixture = create_fixture(
                    owned_root, size_bytes, list(size_workloads.keys())
                )
                try:
                    for piece_size, selected_cases in size_workloads.items():
                        for run in range(1, arguments.runs + 1):
                            if arguments.encryption_pair:
                                client_cases = [
                                    (
                                        "rstorrent",
                                        case["write_concurrency"],
                                        case["hash_concurrency"],
                                        case["id"],
                                        encryption,
                                    )
                                    for case in selected_cases
                                    for encryption in ("disabled", "required")
                                ]
                            else:
                                client_cases = [
                                    (
                                        "rstorrent",
                                        case["write_concurrency"],
                                        case["hash_concurrency"],
                                        case["id"],
                                        None,
                                    )
                                    for case in selected_cases
                                ]
                                client_cases.append(("libtorrent", 0, 0, None, None))
                            rotation = case_ordinal % len(client_cases)
                            owner_order = (
                                client_cases[rotation:] + client_cases[:rotation]
                            )
                            case_root = owned_root / (
                                f"case-{size_bytes}-{piece_size}-{run}"
                            )
                            case_root.mkdir()
                            for order, client_case in enumerate(owner_order, start=1):
                                (
                                    implementation,
                                    write_limit,
                                    hash_limit,
                                    baseline_case_id,
                                    encryption,
                                ) = client_case
                                results.append(
                                    run_transfer(
                                        implementation,
                                        binary,
                                        fixture,
                                        piece_size,
                                        run,
                                        order,
                                        case_root,
                                        arguments.timeout_seconds,
                                        write_limit,
                                        hash_limit,
                                        baseline_case_id,
                                        encryption,
                                    )
                                )
                            case_root.rmdir()
                            case_ordinal += 1
                finally:
                    shutil.rmtree(fixture.root)
    except (OSError, ScenarioFailure, subprocess.SubprocessError) as error:
        print(f"throughput comparison failed: {error}", file=sys.stderr)
        return 1

    summaries = (
        summarize_encryption_pairs(results)
        if arguments.encryption_pair
        else summarize_results(results)
    )
    gate_failures: list[str] = []
    if arguments.encryption_pair:
        for summary in summaries:
            if (
                summary["median_paired_rc4_plain_ratio"]
                < arguments.minimum_rc4_plain_ratio
            ):
                gate_failures.append(
                    f"size={summary['size_bytes']} piece={summary['piece_size']} "
                    f"storage={summary['write_concurrency']}/"
                    f"{summary['hash_concurrency']} median paired RC4/plain ratio "
                    f"{summary['median_paired_rc4_plain_ratio']:.3f} is below "
                    f"{arguments.minimum_rc4_plain_ratio:.3f}"
                )
    else:
        for summary in summaries:
            label = (
                f"size={summary['size_bytes']} piece={summary['piece_size']} "
                f"storage={summary['write_concurrency']}/"
                f"{summary['hash_concurrency']}"
            )
            baseline_case = (
                None
                if summary["baseline_case_id"] is None
                else throughput_case_by_id[summary["baseline_case_id"]]
            )
            if baseline_case is not None:
                summary["observed"] = baseline_case.get("observed")
                summary["required"] = baseline_case["required"]
            minimum_throughput = (
                arguments.minimum_rstorrent_mib_s
                if baseline_case is None
                else baseline_case["required"].get("minimum_mib_s")
            )
            if (
                minimum_throughput is not None
                and summary["rstorrent_median_mib_s"] < minimum_throughput
            ):
                gate_failures.append(
                    f"{label} RSTorrent median {summary['rstorrent_median_mib_s']:.3f} "
                    f"MiB/s is below {minimum_throughput:.3f} MiB/s"
                )
            minimum_ratio = (
                arguments.minimum_rstorrent_libtorrent_ratio
                if baseline_case is None
                else baseline_case["required"].get("minimum_libtorrent_ratio")
            )
            if (
                minimum_ratio is not None
                and summary["rstorrent_libtorrent_ratio"] < minimum_ratio
            ):
                gate_failures.append(
                    f"{label} RSTorrent/libtorrent median ratio "
                    f"{summary['rstorrent_libtorrent_ratio']:.3f} is below "
                    f"{minimum_ratio:.3f}"
                )

    report = {
        "schema_version": 5,
        "scenario": (
            "controlled-mse-paired-loopback-throughput"
            if arguments.encryption_pair
            else "controlled-single-file-loopback-throughput"
        ),
        "status": "passed" if not gate_failures else "regression",
        "environment": environment,
        "baseline_profile": (
            None
            if arguments.profile is None
            else {
                "profile_id": arguments.profile["profile_id"],
                "tier": arguments.profile_tier,
                "calibration_status": arguments.profile["calibration_status"],
                "path": report_path(arguments.profile_path, repository),
                "requirements": arguments.profile["requirements"],
            }
        ),
        "config": {
            "sizes_mib": arguments.sizes_mib,
            "piece_sizes_kib": arguments.piece_sizes_kib,
            "runs": arguments.runs,
            "timeout_seconds": arguments.timeout_seconds,
            "storage_points": [
                {
                    "write_concurrency": write_limit,
                    "hash_concurrency": hash_limit,
                }
                for write_limit, hash_limit in arguments.storage_points
            ],
            "payload_allowance_bytes": PAYLOAD_ALLOWANCE,
            "client_order": "rotating-by-case",
            "encryption_pair": arguments.encryption_pair,
            "minimum_rc4_plain_ratio": arguments.minimum_rc4_plain_ratio,
            "throughput_cases": arguments.throughput_cases,
            "minimum_rstorrent_mib_s": arguments.minimum_rstorrent_mib_s,
            "minimum_rstorrent_libtorrent_ratio": (
                arguments.minimum_rstorrent_libtorrent_ratio
            ),
        },
        "elapsed_seconds": time.monotonic() - started,
        "results": [asdict(result) for result in results],
        "summaries": summaries,
        "gate": {
            "passed": not gate_failures,
            "failures": gate_failures,
        },
    }
    rendered = json.dumps(report, indent=2, sort_keys=True)
    if arguments.output is not None:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(rendered + "\n", encoding="utf-8")
    print(rendered)
    if gate_failures:
        for failure in gate_failures:
            print(f"throughput gate failed: {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
