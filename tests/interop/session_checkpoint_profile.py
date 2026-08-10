#!/usr/bin/env python3
"""Measure SQLite-backed durability checkpoints during a controlled download."""

from __future__ import annotations

import argparse
import gc
import hashlib
import shutil
import sqlite3
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
    compare_payloads,
    create_session,
    wait_for_listener,
)
from magnet_metadata import create_fixture, magnet_uri
from session_resume import (
    build_binary,
    derive_durable_state,
    envelope,
    exchange,
    read_durable_state,
    start_process,
    stop_process,
    wait_for_complete,
)


PIECE_SIZE = 256 * 1024
TOTAL_SIZE = 128 * 1024 * 1024
PAYLOAD_ALLOWANCE = 32 * 1024 * 1024
PROCESS_TIMEOUT_SECONDS = 180
POLL_SECONDS = 0.005


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


@dataclass
class ProfileResult:
    ordinal: int
    transfer_seconds: float
    total_seconds: float
    info_hash: str
    piece_count: int
    baseline_verified_pieces: int
    checkpoint_revision_delta: int
    payload_hash: str
    cleanup_succeeded: bool = False


def read_checkpoint_state(
    database_path: Path,
    info_hash: str,
) -> tuple[int, int, int, str]:
    with sqlite3.connect(database_path, timeout=1) as connection:
        row = connection.execute(
            """
            SELECT updated_revision, raw_info, piece_count, have_state,
                   desired_state, payload_state, verification_requested,
                   verification_completed, quarantine_reason
            FROM torrents
            WHERE lower(hex(info_hash)) = ?
            """,
            (info_hash,),
        ).fetchone()
    if row is None:
        raise ScenarioFailure("durable torrent row is missing")
    (
        updated_revision,
        raw_info,
        piece_count,
        have_state,
        desired_state,
        payload_state,
        verification_requested,
        verification_completed,
        quarantine_reason,
    ) = row
    if piece_count is None or have_state is None:
        state = derive_durable_state(
            raw_info,
            0,
            0,
            desired_state,
            payload_state,
            verification_requested,
            verification_completed,
            quarantine_reason,
        )
        return int(updated_revision), 0, 0, state
    if len(have_state) != 34 + (piece_count + 7) // 8:
        raise ScenarioFailure("durable have state has unexpected geometry")
    verified = sum(
        1
        for index in range(piece_count)
        if have_state[34 + index // 8] & (1 << (7 - index % 8))
    )
    state = derive_durable_state(
        raw_info,
        piece_count,
        verified,
        desired_state,
        payload_state,
        verification_requested,
        verification_completed,
        quarantine_reason,
    )
    return int(updated_revision), int(piece_count), verified, state


def wait_for_metadata_checkpoint(
    database_path: Path,
    info_hash: str,
) -> tuple[float, int, int, int]:
    deadline = time.monotonic() + PROCESS_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        try:
            revision, piece_count, verified, state = read_checkpoint_state(
                database_path, info_hash
            )
        except (sqlite3.Error, ScenarioFailure):
            time.sleep(POLL_SECONDS)
            continue
        if piece_count > 0 and state in {"downloading", "complete"}:
            return time.monotonic(), revision, piece_count, verified
        time.sleep(POLL_SECONDS)
    raise ScenarioFailure("session did not durably checkpoint metadata")


def run_once(
    binary: Path,
    ordinal: int,
    write_concurrency: int,
    hash_concurrency: int,
) -> ProfileResult:
    run_path = Path(tempfile.mkdtemp(prefix=f"rstorrent-checkpoint-{ordinal}-"))
    diagnostics: list[str] = []
    failure: BaseException | None = None
    result: ProfileResult | None = None
    process: subprocess.Popen[str] | None = None
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    try:
        fixture = create_fixture(
            run_path,
            payload_size=TOTAL_SIZE,
            piece_size=PIECE_SIZE,
        )
        profile_root = run_path / "profile"
        payload_root = run_path / "payload"
        database_path = profile_root / "session.db"
        session = create_session()
        port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            fixture.torrent_info,
            fixture.seed_directory,
            diagnostics,
        )
        process = start_process(
            binary,
            profile_root,
            payload_root,
            timeout_seconds=PROCESS_TIMEOUT_SECONDS,
            payload_allowance=PAYLOAD_ALLOWANCE,
            storage_write_concurrency=write_concurrency,
            storage_hash_concurrency=hash_concurrency,
        )
        add = envelope(
            "add",
            {
                "type": "add_magnet",
                "magnet": magnet_uri(fixture.info_hash, f"127.0.0.1:{port}"),
                "storage_root": "downloads",
                "skip_files": [],
            },
        )
        total_started = time.monotonic()
        exchange(process, add)
        (
            transfer_started,
            baseline_revision,
            piece_count,
            baseline_verified,
        ) = wait_for_metadata_checkpoint(database_path, fixture.info_hash)
        completion = wait_for_complete(
            process,
            fixture,
            timeout_seconds=PROCESS_TIMEOUT_SECONDS,
        )
        transfer_seconds = time.monotonic() - transfer_started
        total_seconds = time.monotonic() - total_started
        final_revision, final_piece_count, final_verified, state = read_checkpoint_state(
            database_path, fixture.info_hash
        )
        raw_info, durable_piece_count, durable_verified, durable_state = (
            read_durable_state(database_path, fixture.info_hash)
        )
        if bytes(raw_info or b"") != fixture.info_bytes:
            raise ScenarioFailure("completion changed exact raw info bytes")
        if (
            state != "complete"
            or durable_state != "complete"
            or final_piece_count != piece_count
            or durable_piece_count != piece_count
            or final_verified != piece_count
            or durable_verified != piece_count
        ):
            raise ScenarioFailure("completion was not fully checkpointed")
        if int(completion["revision"]) != final_revision:
            raise ScenarioFailure("completion snapshot and SQLite revision differ")
        output_payload = payload_root / fixture.info_hash / "payload.bin"
        payload_hash = compare_payloads(fixture.payload_path, output_payload)
        if payload_hash != fixture.payload_hash:
            raise ScenarioFailure("checkpoint profile payload differs from seed")
        stop_process(process, graceful=True)
        process = None
        result = ProfileResult(
            ordinal=ordinal,
            transfer_seconds=transfer_seconds,
            total_seconds=total_seconds,
            info_hash=fixture.info_hash,
            piece_count=piece_count,
            baseline_verified_pieces=baseline_verified,
            checkpoint_revision_delta=final_revision - baseline_revision,
            payload_hash=payload_hash,
        )
    except BaseException as error:
        failure = error
    finally:
        if process is not None:
            stop_process(process, graceful=False)
        if session is not None:
            try:
                diagnostics.extend(alert.message() for alert in session.pop_alerts())
                if handle is not None and handle.is_valid():
                    session.remove_torrent(handle)
                session.pause()
            except Exception as error:
                diagnostics.append(f"libtorrent cleanup failed: {error}")
        handle = None
        session = None
        gc.collect()
        try:
            shutil.rmtree(run_path)
            cleanup_succeeded = not run_path.exists()
        except OSError as error:
            cleanup_succeeded = False
            if failure is None:
                failure = error
        if result is not None:
            result.cleanup_succeeded = cleanup_succeeded
    if failure is not None:
        diagnostic_text = "\n".join(diagnostics[-100:]) or "(no libtorrent alerts)"
        raise ScenarioFailure(
            f"checkpoint profile run {ordinal} failed: {failure}\n"
            f"cleanup={'ok' if not run_path.exists() else 'failed'}\n"
            f"libtorrent alerts:\n{diagnostic_text}"
        ) from failure
    if result is None or not result.cleanup_succeeded:
        raise ScenarioFailure(f"checkpoint profile run {ordinal} did not clean up")
    return result


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--runs",
        type=int,
        default=1,
        choices=range(1, 6),
        metavar="1..5",
    )
    parser.add_argument("--binary", type=Path)
    parser.add_argument(
        "--write-concurrency", type=int, choices=range(1, 9), default=4
    )
    parser.add_argument(
        "--hash-concurrency", type=int, choices=range(1, 9), default=4
    )
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    repository = Path(__file__).resolve().parents[2]
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    print(
        f"scenario=session-checkpoint total_size={TOTAL_SIZE} "
        f"piece_size={PIECE_SIZE} pieces={TOTAL_SIZE // PIECE_SIZE} "
        f"payload_allowance={PAYLOAD_ALLOWANCE} "
        f"write_concurrency={arguments.write_concurrency} "
        f"hash_concurrency={arguments.hash_concurrency}"
    )
    try:
        binary = arguments.binary or build_binary(repository)
        binary = binary.resolve()
        print(f"binary={binary} binary_sha256={sha256_file(binary)}")
        results = [
            run_once(
                binary,
                ordinal,
                arguments.write_concurrency,
                arguments.hash_concurrency,
            )
            for ordinal in range(1, arguments.runs + 1)
        ]
    except (ScenarioFailure, OSError, sqlite3.Error, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    for result in results:
        print(
            f"run={result.ordinal} transfer_seconds={result.transfer_seconds:.3f} "
            f"total_seconds={result.total_seconds:.3f} "
            f"info_hash={result.info_hash} pieces={result.piece_count} "
            f"baseline_verified={result.baseline_verified_pieces} "
            f"checkpoint_revision_delta={result.checkpoint_revision_delta} "
            f"payload_sha1={result.payload_hash} cleanup=ok"
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
