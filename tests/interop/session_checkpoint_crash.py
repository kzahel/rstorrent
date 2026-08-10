#!/usr/bin/env python3
"""Crash at exact durability boundaries and verify conservative restart."""

from __future__ import annotations

import argparse
import gc
import hashlib
import selectors
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
from session_checkpoint_profile import read_checkpoint_state
from session_resume import (
    build_binary,
    envelope,
    exchange,
    read_durable_state,
    start_process,
    stop_process,
    valid_payload_pieces,
    wait_for_complete,
)


PIECE_SIZE = 256 * 1024
TOTAL_SIZE = 64 * 1024 * 1024
PIECE_COUNT = TOTAL_SIZE // PIECE_SIZE
PAYLOAD_ALLOWANCE = 32 * 1024 * 1024
PROCESS_TIMEOUT_SECONDS = 180
BOUNDARY_DELAY_MILLIS = 60_000


@dataclass(frozen=True)
class CrashScenario:
    name: str
    stage: str
    sync_delay_millis: int
    commit_delay_millis: int
    expect_durable: bool


SCENARIOS = (
    CrashScenario(
        name="pre_sync",
        stage="syncing",
        sync_delay_millis=BOUNDARY_DELAY_MILLIS,
        commit_delay_millis=0,
        expect_durable=False,
    ),
    CrashScenario(
        name="post_sync_pre_commit",
        stage="committing",
        sync_delay_millis=0,
        commit_delay_millis=BOUNDARY_DELAY_MILLIS,
        expect_durable=False,
    ),
    CrashScenario(
        name="post_commit",
        stage="idle",
        sync_delay_millis=0,
        commit_delay_millis=0,
        expect_durable=True,
    ),
)


@dataclass
class CrashResult:
    scenario: str
    revision_after_crash: int
    durable_pieces_after_crash: int
    valid_pieces_after_crash: int
    false_negative_pieces_after_crash: int
    restart_payload_upload: int
    stable_verification_generation: int
    payload_hash: str
    elapsed_seconds: float
    cleanup_succeeded: bool = False


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def parse_checkpoint_marker(line: str) -> dict[str, str] | None:
    if not line.startswith("checkpoint_stage="):
        return None
    fields: dict[str, str] = {}
    for item in line.strip().split():
        key, separator, value = item.partition("=")
        if not separator:
            return None
        fields[key] = value
    return fields


def read_durable_piece_indices(database: Path, info_hash: str) -> set[int]:
    with sqlite3.connect(database, timeout=1) as connection:
        row = connection.execute(
            """
            SELECT piece_count, have_state FROM torrents
            WHERE lower(hex(info_hash)) = ?
            """,
            (info_hash,),
        ).fetchone()
    if row is None or row[1] is None:
        return set()
    piece_count, have_state = row
    return {
        index
        for index in range(piece_count)
        if have_state[34 + index // 8] & (1 << (7 - index % 8))
    }


def read_verification_generation(database: Path, info_hash: str) -> tuple[int, int]:
    with sqlite3.connect(database, timeout=1) as connection:
        row = connection.execute(
            """
            SELECT verification_requested, verification_completed FROM torrents
            WHERE lower(hex(info_hash)) = ?
            """,
            (info_hash,),
        ).fetchone()
    if row is None:
        raise ScenarioFailure("stable neighbor row is missing")
    return int(row[0]), int(row[1])


def wait_for_target_complete(
    process: subprocess.Popen[str],
    fixture,
    expected_torrents: int,
) -> dict:
    deadline = time.monotonic() + PROCESS_TIMEOUT_SECONDS
    request_number = 0
    while time.monotonic() < deadline:
        response = exchange(
            process,
            envelope(f"target-snapshot-{request_number}", {"type": "snapshot"}),
        )
        request_number += 1
        torrents = response["snapshot"]["torrents"]
        if len(torrents) != expected_torrents:
            raise ScenarioFailure("restart snapshot has the wrong torrent count")
        torrent = next(
            (row for row in torrents if row["torrent_id"] == fixture.info_hash),
            None,
        )
        if torrent is None:
            raise ScenarioFailure("restart snapshot lacks the crashing torrent")
        if torrent["state"] == "complete":
            if torrent["verified_piece_count"] != torrent["piece_count"]:
                raise ScenarioFailure("complete target has incomplete have state")
            return response
        if torrent["state"] == "needs_repair":
            raise ScenarioFailure(f"restart entered repair state: {torrent}")
        time.sleep(0.05)
    raise ScenarioFailure("restarted target did not complete before timeout")


def wait_for_crash_boundary(
    process: subprocess.Popen[str],
    scenario: CrashScenario,
    diagnostics: list[str],
) -> dict[str, str]:
    if process.stderr is None:
        raise ScenarioFailure("session diagnostic stderr is unavailable")
    selector = selectors.DefaultSelector()
    selector.register(process.stderr, selectors.EVENT_READ)
    deadline = time.monotonic() + PROCESS_TIMEOUT_SECONDS
    try:
        while time.monotonic() < deadline:
            if process.poll() is not None:
                remainder = process.stderr.read()
                if remainder:
                    diagnostics.extend(remainder.splitlines())
                raise ScenarioFailure(
                    f"session exited before {scenario.name} boundary"
                )
            if not selector.select(0.25):
                continue
            line = process.stderr.readline()
            if not line:
                continue
            diagnostics.append(line.rstrip())
            marker = parse_checkpoint_marker(line)
            if marker is None or marker.get("checkpoint_stage") != scenario.stage:
                continue
            dirty_pieces = int(marker.get("dirty_pieces", "0"))
            batches_completed = int(marker.get("batches_completed", "0"))
            if scenario.expect_durable:
                if batches_completed >= 1:
                    return marker
            elif dirty_pieces > 0 and batches_completed == 0:
                return marker
    finally:
        selector.close()
    raise ScenarioFailure(f"session did not reach {scenario.name} boundary")


def run_once(binary: Path, scenario: CrashScenario) -> CrashResult:
    run_path = Path(tempfile.mkdtemp(prefix=f"rstorrent-crash-{scenario.name}-"))
    diagnostics: list[str] = []
    failure: BaseException | None = None
    result: CrashResult | None = None
    process: subprocess.Popen[str] | None = None
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    stable_handle: lt.torrent_handle | None = None
    started = time.monotonic()
    try:
        fixture = create_fixture(
            run_path / "crashing",
            payload_size=TOTAL_SIZE,
            piece_size=PIECE_SIZE,
            root_name=f"crashing-{scenario.name}",
        )
        stable_fixture = create_fixture(
            run_path / "stable",
            payload_size=4 * PIECE_SIZE,
            piece_size=PIECE_SIZE,
            root_name="stable-neighbor",
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
        stable_handle = add_seed(
            session,
            stable_fixture.torrent_info,
            stable_fixture.seed_directory,
            diagnostics,
        )
        process = start_process(
            binary,
            profile_root,
            payload_root,
            timeout_seconds=PROCESS_TIMEOUT_SECONDS,
            payload_allowance=PAYLOAD_ALLOWANCE,
        )
        exchange(
            process,
            envelope(
                "add-stable-neighbor",
                {
                    "type": "add_magnet",
                    "magnet": magnet_uri(
                        stable_fixture.info_hash,
                        f"127.0.0.1:{port}",
                    ),
                    "storage_root": "downloads",
                    "skip_files": [],
                },
            ),
        )
        wait_for_complete(process, stable_fixture)
        stop_process(process, graceful=True)
        process = None
        if read_verification_generation(database_path, stable_fixture.info_hash) != (0, 0):
            raise ScenarioFailure("stable neighbor unexpectedly entered checking")

        process = start_process(
            binary,
            profile_root,
            payload_root,
            timeout_seconds=PROCESS_TIMEOUT_SECONDS,
            payload_allowance=PAYLOAD_ALLOWANCE,
            checkpoint_sync_delay_millis=scenario.sync_delay_millis,
            checkpoint_commit_delay_millis=scenario.commit_delay_millis,
            trace_checkpoint_stages=True,
        )
        exchange(
            process,
            envelope(
                "add",
                {
                    "type": "add_magnet",
                    "magnet": magnet_uri(
                        fixture.info_hash,
                        f"127.0.0.1:{port}",
                    ),
                    "storage_root": "downloads",
                    "skip_files": [],
                },
            ),
        )
        marker = wait_for_crash_boundary(process, scenario, diagnostics)
        if marker["checkpoint_stage"] != scenario.stage:
            raise ScenarioFailure("checkpoint marker changed before crash")
        process.kill()
        process.wait(timeout=5)
        process = None

        revision, piece_count, verified, state = read_checkpoint_state(
            database_path,
            fixture.info_hash,
        )
        raw_info, durable_piece_count, durable_verified, durable_state = (
            read_durable_state(database_path, fixture.info_hash)
        )
        if bytes(raw_info or b"") != fixture.info_bytes:
            raise ScenarioFailure("crash changed exact raw info bytes")
        if piece_count != PIECE_COUNT or durable_piece_count != PIECE_COUNT:
            raise ScenarioFailure("crash retained unexpected have geometry")
        if verified != durable_verified or state != durable_state:
            raise ScenarioFailure("SQLite crash reads disagree")
        if scenario.expect_durable:
            valid_verified_count = 0 < verified <= PIECE_COUNT
        else:
            valid_verified_count = verified == 0
        if not valid_verified_count:
            raise ScenarioFailure(
                f"{scenario.name} retained unexpected durable count {verified}"
            )
        durable_indices = read_durable_piece_indices(
            database_path,
            fixture.info_hash,
        )
        if len(durable_indices) != verified:
            raise ScenarioFailure("durable have count and bitmap disagree")

        staging_payload = (
            payload_root
            / f".{fixture.info_hash}.rstorrent-staging"
            / "payload.bin"
        )
        valid_after_crash = valid_payload_pieces(
            staging_payload,
            fixture.torrent_info,
        )
        upload_before_restart = handle.status().total_payload_upload
        process = start_process(
            binary,
            profile_root,
            payload_root,
            timeout_seconds=PROCESS_TIMEOUT_SECONDS,
            payload_allowance=PAYLOAD_ALLOWANCE,
        )
        wait_for_target_complete(process, fixture, expected_torrents=2)
        restart_payload_upload = (
            handle.status().total_payload_upload - upload_before_restart
        )
        expected_restart_upload = sum(
            fixture.torrent_info.piece_size(piece_index)
            for piece_index in range(fixture.torrent_info.num_pieces())
            if piece_index not in durable_indices
        )
        if restart_payload_upload != expected_restart_upload:
            raise ScenarioFailure(
                f"{scenario.name} restart uploaded {restart_payload_upload} bytes; "
                f"expected {expected_restart_upload}"
            )
        false_negative_pieces = set(valid_after_crash) - durable_indices
        if not scenario.expect_durable and not false_negative_pieces:
            raise ScenarioFailure(
                f"{scenario.name} did not retain a physically valid false-negative piece"
            )

        output_payload = payload_root / fixture.torrent_info.name() / "payload.bin"
        payload_hash = compare_payloads(fixture.payload_path, output_payload)
        if payload_hash != fixture.payload_hash:
            raise ScenarioFailure(f"{scenario.name} restart payload differs from seed")
        _, final_piece_count, final_verified, final_state = read_durable_state(
            database_path,
            fixture.info_hash,
        )
        if final_state != "complete" or final_verified != final_piece_count:
            raise ScenarioFailure(f"{scenario.name} restart was not fully durable")
        stable_requested, stable_completed = read_verification_generation(
            database_path,
            stable_fixture.info_hash,
        )
        if (stable_requested, stable_completed) != (0, 0):
            raise ScenarioFailure("crashing torrent caused stable neighbor checking")
        stop_process(process, graceful=True)
        process = None
        result = CrashResult(
            scenario=scenario.name,
            revision_after_crash=revision,
            durable_pieces_after_crash=verified,
            valid_pieces_after_crash=len(valid_after_crash),
            false_negative_pieces_after_crash=len(false_negative_pieces),
            restart_payload_upload=restart_payload_upload,
            stable_verification_generation=stable_requested,
            payload_hash=payload_hash,
            elapsed_seconds=time.monotonic() - started,
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
                if stable_handle is not None and stable_handle.is_valid():
                    session.remove_torrent(stable_handle)
                session.pause()
            except Exception as error:
                diagnostics.append(f"libtorrent cleanup failed: {error}")
        handle = None
        stable_handle = None
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
        diagnostic_text = "\n".join(diagnostics[-120:]) or "(no diagnostics)"
        raise ScenarioFailure(
            f"checkpoint crash scenario {scenario.name} failed: {failure}\n"
            f"cleanup={'ok' if not run_path.exists() else 'failed'}\n"
            f"diagnostics:\n{diagnostic_text}"
        ) from failure
    if result is None or not result.cleanup_succeeded:
        raise ScenarioFailure(
            f"checkpoint crash scenario {scenario.name} did not clean up"
        )
    return result


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--scenario",
        choices=["all", *(scenario.name for scenario in SCENARIOS)],
        default="all",
    )
    parser.add_argument("--binary", type=Path)
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    repository = Path(__file__).resolve().parents[2]
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    print(
        f"scenario=session-checkpoint-crash total_size={TOTAL_SIZE} "
        f"piece_size={PIECE_SIZE} pieces={PIECE_COUNT} "
        f"payload_allowance={PAYLOAD_ALLOWANCE}"
    )
    try:
        binary = (arguments.binary or build_binary(repository)).resolve()
        print(f"binary={binary} binary_sha256={sha256_file(binary)}")
        selected = [
            scenario
            for scenario in SCENARIOS
            if arguments.scenario in {"all", scenario.name}
        ]
        for scenario in selected:
            result = run_once(binary, scenario)
            print(
                f"crash={result.scenario} "
                f"revision_after_crash={result.revision_after_crash} "
                f"durable_pieces_after_crash={result.durable_pieces_after_crash} "
                f"valid_pieces_after_crash={result.valid_pieces_after_crash} "
                f"false_negative_pieces_after_crash={result.false_negative_pieces_after_crash} "
                f"restart_payload_upload={result.restart_payload_upload} "
                f"stable_verification_generation={result.stable_verification_generation} "
                f"payload_sha1={result.payload_hash} "
                f"elapsed_seconds={result.elapsed_seconds:.3f} cleanup=ok"
            )
    except (ScenarioFailure, OSError, sqlite3.Error, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    print("result=pass cleanup=ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
