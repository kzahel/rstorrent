#!/usr/bin/env python3
"""Measure a representative controlled multi-file download and hash profile."""

from __future__ import annotations

import argparse
import gc
import hashlib
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

import libtorrent as lt

from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    build_diagnostic,
    create_session,
    hash_file,
    wait_for_listener,
    write_deterministic_range,
)


BLOCK_SIZE = 16 * 1024
PIECE_SIZE = 256 * 1024
ROOT_NAME = "fixture"


@dataclass(frozen=True)
class ProfileConfig:
    name: str
    total_size: int
    payload_allowance: int
    diagnostic_timeout_seconds: int
    process_timeout_seconds: int

    @property
    def files(self) -> tuple[tuple[str, int], ...]:
        first = self.total_size * 5 // 16 + 12_345
        second = self.total_size * 3 // 8 + 54_321
        return (
            ("first.bin", first),
            ("nested/second.bin", second),
            ("third.bin", self.total_size - first - second),
        )


PROFILES = {
    "quick": ProfileConfig(
        name="quick",
        total_size=32 * 1024 * 1024,
        payload_allowance=8 * 1024 * 1024,
        diagnostic_timeout_seconds=120,
        process_timeout_seconds=135,
    ),
    "steady": ProfileConfig(
        name="steady",
        total_size=128 * 1024 * 1024,
        payload_allowance=32 * 1024 * 1024,
        diagnostic_timeout_seconds=180,
        process_timeout_seconds=195,
    ),
}


@dataclass
class ProfileResult:
    ordinal: int
    transfer_seconds: float
    info_hash: str
    payload_high_water: int
    storage_write_operations: int
    storage_write_blocks: int
    storage_write_batch_blocks_high_water: int
    storage_write_batch_bytes_high_water: int
    storage_write_service_micros: int
    file_hashes: dict[str, str]
    cleanup_succeeded: bool = False


def create_fixture(
    run_directory: Path, config: ProfileConfig,
) -> tuple[Path, Path, lt.torrent_info, dict[str, str], list[str]]:
    seed_directory = run_directory / "seed"
    torrent_root = seed_directory / ROOT_NAME
    torrent_root.mkdir(parents=True)
    files = lt.file_storage()
    expected_hashes: dict[str, str] = {}
    torrent_offset = 0
    for relative_path, length in config.files:
        files.add_file(f"{ROOT_NAME}/{relative_path}", length)
        expected_hashes[relative_path] = write_deterministic_range(
            torrent_root / relative_path,
            torrent_offset,
            length,
        )
        torrent_offset += length
    if torrent_offset != config.total_size:
        raise ScenarioFailure(
            f"profile totals {torrent_offset} bytes instead of {config.total_size}"
        )

    creator = lt.create_torrent(
        files,
        piece_size=PIECE_SIZE,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(seed_directory))
    torrent_path = run_directory / "profile.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    torrent_info = lt.torrent_info(str(torrent_path))
    expected_piece_count = config.total_size // PIECE_SIZE
    if torrent_info.num_pieces() != expected_piece_count:
        raise ScenarioFailure(
            f"profile has {torrent_info.num_pieces()} pieces instead of "
            f"{expected_piece_count}"
        )
    if torrent_info.piece_length() != PIECE_SIZE:
        raise ScenarioFailure(
            f"profile piece length is {torrent_info.piece_length()} instead of {PIECE_SIZE}"
        )
    if torrent_info.total_size() != config.total_size:
        raise ScenarioFailure(
            f"profile total is {torrent_info.total_size()} instead of "
            f"{config.total_size}"
        )
    if torrent_info.num_files() != len(config.files):
        raise ScenarioFailure(
            f"profile has {torrent_info.num_files()} files instead of "
            f"{len(config.files)}"
        )
    if any(True for _ in torrent_info.trackers()):
        raise ScenarioFailure("controlled profile unexpectedly contains a tracker")
    piece_hashes = [
        bytes(torrent_info.hash_for_piece(index)).hex()
        for index in range(torrent_info.num_pieces())
    ]
    return torrent_path, seed_directory, torrent_info, expected_hashes, piece_hashes


def parse_diagnostic(output: str, config: ProfileConfig) -> dict[str, str]:
    values: dict[str, str] = {}
    for token in output.split():
        if "=" in token:
            key, value = token.split("=", 1)
            values[key] = value
    piece_count = config.total_size // PIECE_SIZE
    block_count = config.total_size // BLOCK_SIZE
    expected = {
        "pieces": f"{piece_count}/{piece_count}",
        "skipped_pieces": "0",
        "bytes": str(config.total_size),
        "blocks": str(block_count),
        "payload_limit": str(config.payload_allowance),
        "verification_buffer": str(BLOCK_SIZE),
        "selected_file_bytes": str(config.total_size),
        "skipped_file_bytes": "0",
        "padding_bytes": "0",
        "selected_written_bytes": str(config.total_size),
        "part_written_bytes": "0",
        "materialized_bytes": "0",
        "part_slots_before": "0",
        "part_slots_after": "0",
        "part_reopened": "true",
    }
    required = {
        *expected,
        "sha1",
        "info_hash",
        "payload_high_water",
        "part_path",
        "storage_write_operations",
        "storage_write_blocks",
        "storage_write_batch_blocks_high_water",
        "storage_write_batch_bytes_high_water",
        "storage_write_service_micros",
    }
    missing = required - values.keys()
    if missing:
        raise ScenarioFailure(f"profile diagnostic is missing fields: {sorted(missing)}")
    for key, expected_value in expected.items():
        if values[key] != expected_value:
            raise ScenarioFailure(
                f"profile diagnostic {key}={values[key]}, expected {expected_value}"
            )
    payload_high_water = int(values["payload_high_water"])
    if not 0 < payload_high_water <= config.payload_allowance:
        raise ScenarioFailure(
            f"payload high water {payload_high_water} is outside "
            f"1..{config.payload_allowance}"
        )
    return values


def run_diagnostic(
    binary: Path,
    torrent_path: Path,
    peer_port: int,
    output_root: Path,
    config: ProfileConfig,
) -> subprocess.CompletedProcess[str]:
    command = [
        str(binary),
        "--metainfo",
        str(torrent_path),
        "--peer",
        f"127.0.0.1:{peer_port}",
        "--output",
        str(output_root),
        "--timeout-seconds",
        str(config.diagnostic_timeout_seconds),
        "--max-buffered-payload-bytes",
        str(config.payload_allowance),
    ]
    try:
        return subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=config.process_timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise ScenarioFailure(
            "RSTorrent selective hash profile exceeded its process timeout\n"
            f"stdout:\n{error.stdout or ''}\n"
            f"stderr:\n{error.stderr or ''}"
        ) from error


def run_once(binary: Path, config: ProfileConfig, ordinal: int) -> ProfileResult:
    run_path = Path(tempfile.mkdtemp(prefix=f"rstorrent-selective-hash-{ordinal}-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    alerts: list[str] = []
    result: ProfileResult | None = None
    failure: BaseException | None = None
    cleanup_errors: list[str] = []
    try:
        (
            torrent_path,
            seed_directory,
            torrent_info,
            expected_hashes,
            piece_hashes,
        ) = create_fixture(run_path, config)
        session = create_session()
        peer_port = wait_for_listener(session, alerts)
        handle = add_seed(session, torrent_info, seed_directory, alerts)
        output_root = run_path / "downloaded"
        started = time.monotonic()
        completed = run_diagnostic(
            binary, torrent_path, peer_port, output_root, config
        )
        transfer_seconds = time.monotonic() - started
        alerts.extend(alert.message() for alert in session.pop_alerts())
        if completed.returncode != 0:
            raise ScenarioFailure(
                f"RSTorrent exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
        diagnostic = parse_diagnostic(completed.stdout, config)
        info_hash = str(torrent_info.info_hashes().v1)
        if diagnostic["info_hash"] != info_hash:
            raise ScenarioFailure("profile diagnostic reported the wrong info hash")
        if diagnostic["sha1"] != piece_hashes[-1]:
            raise ScenarioFailure("profile diagnostic reported the wrong final piece hash")
        actual_hashes: dict[str, str] = {}
        for relative_path, _ in config.files:
            output_path = output_root / relative_path
            if not output_path.is_file():
                raise ScenarioFailure(f"published file is absent: {relative_path}")
            actual_hashes[relative_path] = hash_file(output_path)
            if actual_hashes[relative_path] != expected_hashes[relative_path]:
                raise ScenarioFailure(f"published file differs: {relative_path}")
        part_path = Path(diagnostic["part_path"])
        if part_path != run_path / ".downloaded.rstorrent-parts":
            raise ScenarioFailure(f"unexpected part path: {part_path}")
        if not part_path.is_file():
            raise ScenarioFailure("validated empty part file did not survive publication")
        if (run_path / ".downloaded.rstorrent-staging").exists():
            raise ScenarioFailure("selective staging root survived publication")
        result = ProfileResult(
            ordinal=ordinal,
            transfer_seconds=transfer_seconds,
            info_hash=info_hash,
            payload_high_water=int(diagnostic["payload_high_water"]),
            storage_write_operations=int(diagnostic["storage_write_operations"]),
            storage_write_blocks=int(diagnostic["storage_write_blocks"]),
            storage_write_batch_blocks_high_water=int(
                diagnostic["storage_write_batch_blocks_high_water"]
            ),
            storage_write_batch_bytes_high_water=int(
                diagnostic["storage_write_batch_bytes_high_water"]
            ),
            storage_write_service_micros=int(
                diagnostic["storage_write_service_micros"]
            ),
            file_hashes=actual_hashes,
        )
    except BaseException as error:
        failure = error
    finally:
        if session is not None:
            try:
                alerts.extend(alert.message() for alert in session.pop_alerts())
            except Exception as error:
                cleanup_errors.append(f"libtorrent alert drain failed: {error}")
            try:
                if handle is not None and handle.is_valid():
                    session.remove_torrent(handle)
            except Exception as error:
                cleanup_errors.append(f"libtorrent torrent removal failed: {error}")
            try:
                session.pause()
            except Exception as error:
                cleanup_errors.append(f"libtorrent session pause failed: {error}")
        handle = None
        session = None
        gc.collect()
        try:
            shutil.rmtree(run_path)
            cleanup_succeeded = not run_path.exists()
        except OSError as error:
            cleanup_succeeded = False
            cleanup_errors.append(f"temporary directory cleanup failed: {error}")
        if result is not None:
            result.cleanup_succeeded = cleanup_succeeded and not cleanup_errors
        if cleanup_errors:
            detail = "; ".join(cleanup_errors)
            failure = ScenarioFailure(detail if failure is None else f"{failure}; {detail}")
    if failure is not None:
        diagnostic_text = "\n".join(alerts[-100:]) or "(no libtorrent alerts)"
        raise ScenarioFailure(
            f"profile run {ordinal} failed: {failure}\n"
            f"cleanup={'ok' if not run_path.exists() else 'failed'}\n"
            f"libtorrent alerts:\n{diagnostic_text}"
        ) from failure
    if result is None or not result.cleanup_succeeded:
        raise ScenarioFailure(f"profile run {ordinal} ended without exact cleanup")
    return result


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--runs",
        type=int,
        default=1,
        choices=range(1, 11),
        metavar="1..10",
    )
    parser.add_argument("--binary", type=Path)
    parser.add_argument(
        "--profile",
        choices=tuple(PROFILES),
        default="quick",
        help="quick preserves the historical 32 MiB case; steady targets 30-60 seconds",
    )
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    config = PROFILES[arguments.profile]
    repository = Path(__file__).resolve().parents[2]
    print(f"python_version={sys.version.split()[0]}")
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    print(
        f"scenario=selective-hash profile={config.name} "
        f"total_size={config.total_size} piece_size={PIECE_SIZE} "
        f"pieces={config.total_size // PIECE_SIZE} files={len(config.files)} "
        f"blocks={config.total_size // BLOCK_SIZE} "
        f"payload_allowance={config.payload_allowance}"
    )
    try:
        binary = arguments.binary or build_diagnostic(repository)
        results = [
            run_once(binary, config, ordinal)
            for ordinal in range(1, arguments.runs + 1)
        ]
    except (ScenarioFailure, subprocess.SubprocessError, ValueError) as error:
        print(str(error), file=sys.stderr)
        return 1
    for result in results:
        hashes = ",".join(
            f"{path}:{digest}" for path, digest in result.file_hashes.items()
        )
        print(
            f"run={result.ordinal} transfer_seconds={result.transfer_seconds:.3f} "
            f"info_hash={result.info_hash} payload_high_water={result.payload_high_water} "
            f"write_operations={result.storage_write_operations} "
            f"write_blocks={result.storage_write_blocks} "
            f"batch_blocks_high_water={result.storage_write_batch_blocks_high_water} "
            f"batch_bytes_high_water={result.storage_write_batch_bytes_high_water} "
            f"write_service_seconds={result.storage_write_service_micros / 1_000_000:.3f} "
            f"file_hashes={hashes} cleanup=ok"
        )
    ordered = sorted(result.transfer_seconds for result in results)
    median = ordered[(len(ordered) - 1) // 2]
    print(
        f"all_runs={len(results)} median_transfer_seconds={median:.3f} "
        "cleanup=ok result=pass"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
