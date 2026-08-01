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
TOTAL_SIZE = 32 * 1024 * 1024
PAYLOAD_ALLOWANCE = 8 * 1024 * 1024
DIAGNOSTIC_TIMEOUT_SECONDS = 120
PROCESS_TIMEOUT_SECONDS = 135
ROOT_NAME = "fixture"
FILES = (
    ("first.bin", 10 * 1024 * 1024 + 12_345),
    ("nested/second.bin", 12 * 1024 * 1024 + 54_321),
    ("third.bin", TOTAL_SIZE - (22 * 1024 * 1024 + 66_666)),
)


@dataclass
class ProfileResult:
    ordinal: int
    transfer_seconds: float
    info_hash: str
    payload_high_water: int
    file_hashes: dict[str, str]
    cleanup_succeeded: bool = False


def create_fixture(
    run_directory: Path,
) -> tuple[Path, Path, lt.torrent_info, dict[str, str], list[str]]:
    seed_directory = run_directory / "seed"
    torrent_root = seed_directory / ROOT_NAME
    torrent_root.mkdir(parents=True)
    files = lt.file_storage()
    expected_hashes: dict[str, str] = {}
    torrent_offset = 0
    for relative_path, length in FILES:
        files.add_file(f"{ROOT_NAME}/{relative_path}", length)
        expected_hashes[relative_path] = write_deterministic_range(
            torrent_root / relative_path,
            torrent_offset,
            length,
        )
        torrent_offset += length
    if torrent_offset != TOTAL_SIZE:
        raise ScenarioFailure(
            f"profile totals {torrent_offset} bytes instead of {TOTAL_SIZE}"
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
    expected_piece_count = TOTAL_SIZE // PIECE_SIZE
    if torrent_info.num_pieces() != expected_piece_count:
        raise ScenarioFailure(
            f"profile has {torrent_info.num_pieces()} pieces instead of "
            f"{expected_piece_count}"
        )
    if torrent_info.piece_length() != PIECE_SIZE:
        raise ScenarioFailure(
            f"profile piece length is {torrent_info.piece_length()} instead of {PIECE_SIZE}"
        )
    if torrent_info.total_size() != TOTAL_SIZE:
        raise ScenarioFailure(
            f"profile total is {torrent_info.total_size()} instead of {TOTAL_SIZE}"
        )
    if torrent_info.num_files() != len(FILES):
        raise ScenarioFailure(
            f"profile has {torrent_info.num_files()} files instead of {len(FILES)}"
        )
    if any(True for _ in torrent_info.trackers()):
        raise ScenarioFailure("controlled profile unexpectedly contains a tracker")
    piece_hashes = [
        bytes(torrent_info.hash_for_piece(index)).hex()
        for index in range(torrent_info.num_pieces())
    ]
    return torrent_path, seed_directory, torrent_info, expected_hashes, piece_hashes


def parse_diagnostic(output: str) -> dict[str, str]:
    values: dict[str, str] = {}
    for token in output.split():
        if "=" in token:
            key, value = token.split("=", 1)
            values[key] = value
    piece_count = TOTAL_SIZE // PIECE_SIZE
    block_count = TOTAL_SIZE // BLOCK_SIZE
    expected = {
        "pieces": f"{piece_count}/{piece_count}",
        "skipped_pieces": "0",
        "bytes": str(TOTAL_SIZE),
        "blocks": str(block_count),
        "payload_limit": str(PAYLOAD_ALLOWANCE),
        "verification_buffer": str(BLOCK_SIZE),
        "selected_file_bytes": str(TOTAL_SIZE),
        "skipped_file_bytes": "0",
        "padding_bytes": "0",
        "selected_written_bytes": str(TOTAL_SIZE),
        "part_written_bytes": "0",
        "materialized_bytes": "0",
        "part_slots_before": "0",
        "part_slots_after": "0",
        "part_reopened": "true",
    }
    required = {*expected, "sha1", "info_hash", "payload_high_water", "part_path"}
    missing = required - values.keys()
    if missing:
        raise ScenarioFailure(f"profile diagnostic is missing fields: {sorted(missing)}")
    for key, expected_value in expected.items():
        if values[key] != expected_value:
            raise ScenarioFailure(
                f"profile diagnostic {key}={values[key]}, expected {expected_value}"
            )
    payload_high_water = int(values["payload_high_water"])
    if not 0 < payload_high_water <= PAYLOAD_ALLOWANCE:
        raise ScenarioFailure(
            f"payload high water {payload_high_water} is outside 1..{PAYLOAD_ALLOWANCE}"
        )
    return values


def run_diagnostic(
    binary: Path,
    torrent_path: Path,
    peer_port: int,
    output_root: Path,
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
        str(DIAGNOSTIC_TIMEOUT_SECONDS),
        "--max-buffered-payload-bytes",
        str(PAYLOAD_ALLOWANCE),
    ]
    try:
        return subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=PROCESS_TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise ScenarioFailure(
            "RSTorrent selective hash profile exceeded its process timeout\n"
            f"stdout:\n{error.stdout or ''}\n"
            f"stderr:\n{error.stderr or ''}"
        ) from error


def run_once(binary: Path, ordinal: int) -> ProfileResult:
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
        ) = create_fixture(run_path)
        session = create_session()
        peer_port = wait_for_listener(session, alerts)
        handle = add_seed(session, torrent_info, seed_directory, alerts)
        output_root = run_path / "downloaded"
        started = time.monotonic()
        completed = run_diagnostic(binary, torrent_path, peer_port, output_root)
        transfer_seconds = time.monotonic() - started
        alerts.extend(alert.message() for alert in session.pop_alerts())
        if completed.returncode != 0:
            raise ScenarioFailure(
                f"RSTorrent exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
        diagnostic = parse_diagnostic(completed.stdout)
        info_hash = str(torrent_info.info_hashes().v1)
        if diagnostic["info_hash"] != info_hash:
            raise ScenarioFailure("profile diagnostic reported the wrong info hash")
        if diagnostic["sha1"] != piece_hashes[-1]:
            raise ScenarioFailure("profile diagnostic reported the wrong final piece hash")
        actual_hashes: dict[str, str] = {}
        for relative_path, _ in FILES:
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
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    repository = Path(__file__).resolve().parents[2]
    print(f"python_version={sys.version.split()[0]}")
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    print(
        f"scenario=selective-hash total_size={TOTAL_SIZE} piece_size={PIECE_SIZE} "
        f"pieces={TOTAL_SIZE // PIECE_SIZE} files={len(FILES)} "
        f"blocks={TOTAL_SIZE // BLOCK_SIZE} payload_allowance={PAYLOAD_ALLOWANCE}"
    )
    try:
        binary = arguments.binary or build_diagnostic(repository)
        results = [run_once(binary, ordinal) for ordinal in range(1, arguments.runs + 1)]
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
