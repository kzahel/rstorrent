#!/usr/bin/env python3
"""Verify SQLite-backed magnet resume across forced process death."""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import selectors
import signal
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import libtorrent as lt

from application_identity import (
    HAVE_STATE_HEADER_LENGTH,
    torrent_id_blob,
    torrent_id_from_add,
)
from first_verified_piece import (
    DEFAULT_PAYLOAD_ALLOWANCE,
    ScenarioFailure,
    add_seed,
    compare_payloads,
    create_session,
    wait_for_listener,
)
from magnet_metadata import (
    Fixture,
    create_fixture,
    magnet_uri,
)


PROCESS_TIMEOUT_SECONDS = 180
PEER_TIMEOUT_SECONDS = 10
POLL_SECONDS = 0.02
# Cross the checkpoint owner's 64 MiB byte bound, then hold each SQLite commit
# long enough for the harness to kill the process after one partial durable
# epoch. This keeps the restart boundary deterministic even when loopback can
# finish the former 32 MiB fixture before its two-second age trigger.
RESUME_PAYLOAD_SIZE = 80 * 1024 * 1024
RESUME_PIECE_SIZE = 256 * 1024
FORCED_DEATH_COMMIT_DELAY_MILLIS = 1000


@dataclass
class RunResult:
    ordinal: int
    info_hash: str
    metadata_size: int
    pieces_before_kill: int
    physically_valid_after_crash: int
    restart_payload_upload: int
    accepted_corrupt_piece: int
    force_recheck_upload: int
    verification_generation: int
    payload_hash: str
    elapsed_seconds: float
    cleanup_succeeded: bool = False


def build_binary(repository: Path) -> Path:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "rstorrent-session",
            "--bin",
            "rstorrent-session",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to build session diagnostic\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    binary = repository / "target" / "debug" / "rstorrent-session"
    if not binary.is_file():
        raise ScenarioFailure("session diagnostic binary was not created")
    return binary


def start_process(
    binary: Path,
    profile_root: Path,
    payload_root: Path,
    *,
    timeout_seconds: int = PEER_TIMEOUT_SECONDS,
    payload_allowance: int = DEFAULT_PAYLOAD_ALLOWANCE,
    checkpoint_sync_delay_millis: int = 0,
    checkpoint_commit_delay_millis: int = 0,
    trace_checkpoint_stages: bool = False,
    publication_delay_stage: str | None = None,
    publication_delay_millis: int = 0,
    trace_publication_stages: bool = False,
    storage_write_concurrency: int = 4,
    storage_hash_concurrency: int = 4,
) -> subprocess.Popen[str]:
    command = [
        str(binary),
        "--profile-root",
        str(profile_root),
        "--profile-id",
        "interop",
        "--storage-root",
        f"downloads={payload_root}",
        "--timeout-seconds",
        str(timeout_seconds),
        "--max-buffered-payload-bytes",
        str(payload_allowance),
        "--storage-write-concurrency",
        str(storage_write_concurrency),
        "--storage-hash-concurrency",
        str(storage_hash_concurrency),
    ]
    if checkpoint_sync_delay_millis > 0:
        command.extend(
            ["--checkpoint-sync-delay-millis", str(checkpoint_sync_delay_millis)]
        )
    if checkpoint_commit_delay_millis > 0:
        command.extend(
            ["--checkpoint-commit-delay-millis", str(checkpoint_commit_delay_millis)]
        )
    if trace_checkpoint_stages:
        command.extend(["--trace-checkpoint-stages", "true"])
    if publication_delay_stage is not None:
        command.extend(["--publication-delay-stage", publication_delay_stage])
        command.extend(
            ["--publication-delay-millis", str(publication_delay_millis)]
        )
    if trace_publication_stages:
        command.extend(["--trace-publication-stages", "true"])
    return subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )


def exchange(
    process: subprocess.Popen[str],
    request: dict[str, Any],
    timeout_seconds: float = 5,
) -> dict[str, Any]:
    if process.stdin is None or process.stdout is None:
        raise ScenarioFailure("session diagnostic pipes are unavailable")
    process.stdin.write(json.dumps(request, separators=(",", ":")) + "\n")
    process.stdin.flush()
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    try:
        if not selector.select(timeout_seconds):
            raise ScenarioFailure(
                f"session diagnostic did not answer {request.get('request_id')}"
            )
        line = process.stdout.readline()
    finally:
        selector.close()
    if not line:
        stderr = process.stderr.read() if process.stderr is not None else ""
        raise ScenarioFailure(
            "session diagnostic exited before responding\n"
            f"stderr:\n{stderr}"
        )
    response = json.loads(line)
    if response.get("request_id") != request.get("request_id"):
        raise ScenarioFailure("session response has the wrong request identity")
    if response.get("status") != "success":
        raise ScenarioFailure(f"session command failed: {response}")
    return response


def envelope(request_id: str, command: dict[str, Any]) -> dict[str, Any]:
    return {
        "version": 1,
        "request_id": request_id,
        "command": command,
    }


def read_durable_state(
    database_path: Path,
    torrent_id: str,
) -> tuple[bytes | None, int, int, str]:
    with sqlite3.connect(database_path, timeout=1) as connection:
        row = connection.execute(
            """
            SELECT raw_info, piece_count, have_state, desired_state,
                   payload_state, verification_requested,
                   verification_completed, quarantine_reason
            FROM torrents
            WHERE torrent_id = ?
            """,
            (torrent_id_blob(torrent_id),),
        ).fetchone()
    if row is None:
        raise ScenarioFailure("durable torrent row is missing")
    (
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
        return raw_info, 0, 0, state
    if len(have_state) != HAVE_STATE_HEADER_LENGTH + (piece_count + 7) // 8:
        raise ScenarioFailure("durable have state has unexpected geometry")
    verified = sum(
        1
        for index in range(piece_count)
        if have_state[HAVE_STATE_HEADER_LENGTH + index // 8]
        & (1 << (7 - index % 8))
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
    return raw_info, piece_count, verified, state


def derive_durable_state(
    raw_info: bytes | None,
    piece_count: int,
    verified: int,
    desired_state: str,
    payload_state: str,
    verification_requested: int,
    verification_completed: int,
    quarantine_reason: str | None,
) -> str:
    if quarantine_reason is not None:
        return "needs_repair"
    if raw_info is None:
        return "awaiting_metadata" if desired_state == "running" else "paused"
    if verification_requested != verification_completed:
        return "checking"
    if desired_state != "running":
        return "paused"
    if payload_state == "publication_pending":
        return "awaiting_publication"
    if (
        piece_count > 0
        and verified == piece_count
        and payload_state in {"legacy_owned", "final_owned"}
    ):
        return "complete"
    return "downloading"


def read_durable_piece_indices(database_path: Path, torrent_id: str) -> set[int]:
    with sqlite3.connect(database_path, timeout=1) as connection:
        row = connection.execute(
            """
            SELECT piece_count, have_state FROM torrents
            WHERE torrent_id = ?
            """,
            (torrent_id_blob(torrent_id),),
        ).fetchone()
    if row is None or row[1] is None:
        return set()
    piece_count, have_state = row
    return {
        index
        for index in range(piece_count)
        if have_state[HAVE_STATE_HEADER_LENGTH + index // 8]
        & (1 << (7 - index % 8))
    }


def read_verification_generation(database_path: Path, torrent_id: str) -> tuple[int, int]:
    with sqlite3.connect(database_path, timeout=1) as connection:
        row = connection.execute(
            """
            SELECT verification_requested, verification_completed FROM torrents
            WHERE torrent_id = ?
            """,
            (torrent_id_blob(torrent_id),),
        ).fetchone()
    if row is None:
        raise ScenarioFailure("durable torrent row is missing")
    return int(row[0]), int(row[1])


def sha1_file(path: Path) -> str:
    digest = hashlib.sha1()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def valid_payload_pieces(path: Path, torrent_info: lt.torrent_info) -> list[int]:
    if not path.is_file():
        return []
    valid: list[int] = []
    with path.open("rb") as payload:
        for piece_index in range(torrent_info.num_pieces()):
            length = torrent_info.piece_size(piece_index)
            piece = payload.read(length)
            if len(piece) != length:
                break
            expected = torrent_info.hash_for_piece(piece_index)
            if hasattr(expected, "to_bytes"):
                expected = expected.to_bytes()
            if hashlib.sha1(piece).digest() == expected:
                valid.append(piece_index)
    return valid


def wait_for_piece_checkpoint(
    process: subprocess.Popen[str],
    database_path: Path,
    torrent_id: str,
    fixture: Fixture,
) -> tuple[int, int]:
    deadline = time.monotonic() + PROCESS_TIMEOUT_SECONDS
    last_error: BaseException | None = None
    last_observation: tuple[int, int, str] | None = None
    while time.monotonic() < deadline:
        try:
            raw_info, piece_count, verified, state = read_durable_state(
                database_path,
                torrent_id,
            )
        except (sqlite3.Error, ScenarioFailure) as error:
            last_error = error
            time.sleep(POLL_SECONDS)
            continue
        if raw_info is not None and bytes(raw_info) != fixture.info_bytes:
            raise ScenarioFailure("SQLite retained different raw info bytes")
        last_observation = (piece_count, verified, state)
        if state == "complete":
            raise ScenarioFailure("download completed before forced-death checkpoint")
        if verified >= 2 and verified < piece_count:
            return piece_count, verified
        time.sleep(POLL_SECONDS)
    snapshot = exchange(process, envelope("checkpoint-timeout", {"type": "snapshot"}))
    raise ScenarioFailure(
        "session did not durably checkpoint two partial pieces"
        + (
            f": last SQLite observation: {last_error or last_observation}"
            if last_error or last_observation
            else ""
        )
        + f"; snapshot={snapshot['snapshot']['torrents']}"
    )


def wait_for_complete(
    process: subprocess.Popen[str],
    fixture: Fixture,
    torrent_id: str,
    minimum_revision: int = 0,
    timeout_seconds: int = PROCESS_TIMEOUT_SECONDS,
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_seconds
    request_number = 0
    while time.monotonic() < deadline:
        response = exchange(
            process,
            envelope(
                f"snapshot-{request_number}",
                {"type": "snapshot"},
            ),
        )
        request_number += 1
        torrents = response["snapshot"]["torrents"]
        if len(torrents) != 1:
            raise ScenarioFailure("session snapshot has the wrong torrent count")
        torrent = torrents[0]
        if torrent["torrent_id"] != torrent_id:
            raise ScenarioFailure("session snapshot has the wrong torrent identity")
        if (
            torrent["state"] == "complete"
            and int(response["revision"]) >= minimum_revision
        ):
            if torrent["verified_piece_count"] != torrent["piece_count"]:
                raise ScenarioFailure("complete snapshot has incomplete have state")
            return response
        if torrent["state"] == "needs_repair":
            raise ScenarioFailure(f"restart entered repair state: {torrent}")
        time.sleep(0.05)
    raise ScenarioFailure("restarted session did not complete before timeout")


def stop_process(process: subprocess.Popen[str], graceful: bool) -> None:
    if process.poll() is not None:
        return
    if graceful:
        try:
            exchange(process, envelope("shutdown", {"type": "shutdown"}))
            process.wait(timeout=5)
            return
        except (ScenarioFailure, subprocess.TimeoutExpired, BrokenPipeError):
            pass
    process.kill()
    process.wait(timeout=5)


def run_once(binary: Path, ordinal: int) -> RunResult:
    run_path = Path(tempfile.mkdtemp(prefix=f"rstorrent-session-{ordinal}-"))
    diagnostics: list[str] = []
    failure: BaseException | None = None
    result: RunResult | None = None
    process: subprocess.Popen[str] | None = None
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    started = time.monotonic()
    try:
        fixture = create_fixture(
            run_path,
            payload_size=RESUME_PAYLOAD_SIZE,
            piece_size=RESUME_PIECE_SIZE,
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
            checkpoint_commit_delay_millis=FORCED_DEATH_COMMIT_DELAY_MILLIS,
        )
        add = envelope(
            "add",
            {
                "type": "add_magnet",
                "magnet": magnet_uri(
                    fixture.info_hash,
                    f"127.0.0.1:{port}",
                ),
                "storage_root": "downloads",
                "start_content": True,
                "skip_files": [],
            },
        )
        first_response = exchange(process, add)
        torrent_id = torrent_id_from_add(first_response)

        piece_count, pieces_before_kill = wait_for_piece_checkpoint(
            process,
            database_path,
            torrent_id,
            fixture,
        )
        if piece_count < 4:
            raise ScenarioFailure(
                f"session fixture has only {piece_count} pieces; expected at least 4"
            )
        process.kill()
        process.wait(timeout=5)
        process = None

        staging_payload = (
            payload_root
            / f".{torrent_id}.rstorrent-staging"
            / "payload.bin"
        )
        if not staging_payload.is_file():
            raise ScenarioFailure("forced death did not retain staging payload")
        durable_indices = read_durable_piece_indices(database_path, torrent_id)
        if len(durable_indices) != pieces_before_kill:
            raise ScenarioFailure("durable checkpoint count and bitmap disagree")
        corrupt_piece = min(durable_indices)
        corrupt_offset = corrupt_piece * RESUME_PIECE_SIZE
        with staging_payload.open("r+b") as payload:
            payload.seek(corrupt_offset)
            first = payload.read(1)
            if len(first) != 1:
                raise ScenarioFailure("staging payload is unexpectedly empty")
            payload.seek(corrupt_offset)
            payload.write(bytes([first[0] ^ 0xFF]))
            payload.flush()

        upload_before_restart = handle.status().total_payload_upload
        process = start_process(binary, profile_root, payload_root)
        valid_after_crash = valid_payload_pieces(
            staging_payload,
            fixture.torrent_info,
        )
        physically_valid_after_crash = len(valid_after_crash)
        completion = wait_for_complete(process, fixture, torrent_id)
        upload_after_restart = handle.status().total_payload_upload
        restart_payload_upload = upload_after_restart - upload_before_restart
        expected_restart_upload = sum(
            fixture.torrent_info.piece_size(piece_index)
            for piece_index in range(fixture.torrent_info.num_pieces())
            if piece_index not in durable_indices
        )
        if restart_payload_upload != expected_restart_upload:
            raise ScenarioFailure(
                "fast restart did not retain exactly the durable claims and "
                "redownload every false bitmap entry: "
                f"expected {expected_restart_upload}, got {restart_payload_upload}"
            )

        output_payload = payload_root / fixture.torrent_info.name() / "payload.bin"
        if sha1_file(output_payload) == fixture.payload_hash:
            raise ScenarioFailure("ordinary fast resume did not retain same-length corruption")
        force_upload_before = handle.status().total_payload_upload
        force = exchange(
            process,
            envelope(
                "force-recheck",
                {"type": "force_recheck", "torrent_id": torrent_id},
            ),
        )
        process.send_signal(signal.SIGSTOP)
        requested, completed = read_verification_generation(
            database_path,
            torrent_id,
        )
        if requested != 1 or completed != 0:
            process.send_signal(signal.SIGCONT)
            raise ScenarioFailure("Force recheck was not durably pending at process death")
        process.kill()
        process.wait(timeout=5)
        process = start_process(binary, profile_root, payload_root)
        completion = wait_for_complete(
            process,
            fixture,
            torrent_id,
            minimum_revision=int(force["revision"]) + 2,
        )
        force_recheck_upload = handle.status().total_payload_upload - force_upload_before
        expected_force_upload = fixture.torrent_info.piece_size(corrupt_piece)
        if force_recheck_upload != expected_force_upload:
            raise ScenarioFailure(
                f"Force recheck uploaded {force_recheck_upload} bytes; "
                f"expected {expected_force_upload}"
            )
        payload_hash = compare_payloads(fixture.payload_path, output_payload)
        if payload_hash != fixture.payload_hash:
            raise ScenarioFailure("Force recheck did not restore exact payload")
        raw_info, completed_piece_count, completed_pieces, state = read_durable_state(
            database_path,
            torrent_id,
        )
        if bytes(raw_info or b"") != fixture.info_bytes:
            raise ScenarioFailure("completion changed exact raw info bytes")
        if state != "complete" or completed_pieces != completed_piece_count:
            raise ScenarioFailure("completion was not durably checkpointed")
        requested, completed = read_verification_generation(
            database_path,
            torrent_id,
        )
        if requested != 1 or completed != 1:
            raise ScenarioFailure("Force recheck generation did not settle exactly once")

        stop_process(process, graceful=True)
        process = None
        completed_revision = int(completion["revision"])
        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None

        process = start_process(binary, profile_root, payload_root)
        wait_for_complete(
            process,
            fixture,
            torrent_id,
            minimum_revision=completed_revision,
        )
        if read_verification_generation(database_path, torrent_id) != (1, 1):
            raise ScenarioFailure("stable restart unexpectedly started another checker")
        stop_process(process, graceful=True)
        process = None
        result = RunResult(
            ordinal=ordinal,
            info_hash=fixture.info_hash,
            metadata_size=len(fixture.info_bytes),
            pieces_before_kill=pieces_before_kill,
            physically_valid_after_crash=physically_valid_after_crash,
            restart_payload_upload=restart_payload_upload,
            accepted_corrupt_piece=corrupt_piece,
            force_recheck_upload=force_recheck_upload,
            verification_generation=requested,
            payload_hash=payload_hash,
            elapsed_seconds=time.monotonic() - started,
        )
    except BaseException as error:
        failure = error
    finally:
        if process is not None:
            stop_process(process, graceful=False)
            if process.stderr is not None:
                diagnostics.extend(process.stderr.read().splitlines())
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
            f"session resume run {ordinal} failed: {failure}\n"
            f"cleanup={'ok' if not run_path.exists() else 'failed'}\n"
            f"libtorrent alerts:\n{diagnostic_text}"
        ) from failure
    if result is None or not result.cleanup_succeeded:
        raise ScenarioFailure(f"session resume run {ordinal} did not clean up")
    return result


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--runs",
        type=int,
        default=1,
        choices=range(1, 11),
        metavar="1..10",
        help="number of consecutive forced-restart runs (default: 1)",
    )
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    repository = Path(__file__).resolve().parents[2]
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    print(
        "discovery=dht:false,lsd:false,upnp:false,natpmp:false,"
        "utp_in:false,utp_out:false transport=tcp loopback_only=true"
    )
    try:
        binary = build_binary(repository)
        for ordinal in range(1, arguments.runs + 1):
            result = run_once(binary, ordinal)
            print(
                f"run={result.ordinal} info_hash={result.info_hash} "
                f"metadata_size={result.metadata_size} "
                f"pieces_before_kill={result.pieces_before_kill} "
                f"physically_valid_after_crash={result.physically_valid_after_crash} "
                f"restart_payload_upload={result.restart_payload_upload} "
                f"accepted_corrupt_piece={result.accepted_corrupt_piece} "
                f"force_recheck_upload={result.force_recheck_upload} "
                f"verification_generation={result.verification_generation} "
                f"payload_sha1={result.payload_hash} "
                f"elapsed_seconds={result.elapsed_seconds:.3f} cleanup=ok"
            )
    except (ScenarioFailure, OSError, sqlite3.Error, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
