#!/usr/bin/env python3
"""Verify bidirectional multi-block magnet metadata with libtorrent."""

from __future__ import annotations

import argparse
import gc
import hashlib
import selectors
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

import libtorrent as lt

from first_verified_piece import (
    DEFAULT_PAYLOAD_ALLOWANCE,
    ScenarioFailure,
    add_seed,
    compare_payloads,
    create_session,
    wait_for_listener,
    write_deterministic_payload,
)


METADATA_BLOCK_SIZE = 16 * 1024
PIECE_SIZE = 16 * 1024
PAYLOAD_SIZE = 40_000
EMPTY_FILE_COUNT = 120
ROOT_NAME = "magnet-fixture"
PROCESS_TIMEOUT_SECONDS = 25
METADATA_TIMEOUT_SECONDS = 15


@dataclass(frozen=True)
class Fixture:
    torrent_path: Path
    seed_directory: Path
    payload_path: Path
    payload_hash: str
    torrent_info: lt.torrent_info
    info_bytes: bytes
    info_hash: str
    files: tuple[tuple[str, int], ...]


@dataclass
class RunResult:
    ordinal: int
    leech_seconds: float
    seed_seconds: float
    info_hash: str
    metadata_size: int
    metadata_blocks: int
    payload_hash: str
    leech_output: str
    seed_output: str
    cleanup_succeeded: bool = False


def build_binaries(repository: Path) -> tuple[Path, Path]:
    command = [
        "cargo",
        "build",
        "-p",
        "rstorrent-engine",
        "--bin",
        "rstorrent-download-piece",
        "--bin",
        "rstorrent-metadata-seed",
        "--bin",
        "rstorrent-dht-node",
    ]
    completed = subprocess.run(
        command,
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to build metadata diagnostics\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    download = repository / "target" / "debug" / "rstorrent-download-piece"
    seed = repository / "target" / "debug" / "rstorrent-metadata-seed"
    if not download.is_file() or not seed.is_file():
        raise ScenarioFailure("metadata diagnostic binaries were not created")
    return download, seed


def create_fixture(
    run_path: Path,
    *,
    payload_size: int = PAYLOAD_SIZE,
    piece_size: int = PIECE_SIZE,
) -> Fixture:
    seed_directory = run_path / "seed"
    torrent_root = seed_directory / ROOT_NAME
    torrent_root.mkdir(parents=True)
    files = lt.file_storage()
    for index in range(EMPTY_FILE_COUNT):
        name = f"{index:03}-{'a' * 176}.empty"
        relative = f"metadata/{name}"
        torrent_name = f"{ROOT_NAME}/{relative}"
        files.add_file(torrent_name, 0)
        empty_path = torrent_root / relative
        empty_path.parent.mkdir(parents=True, exist_ok=True)
        empty_path.touch()

    payload_path = torrent_root / "payload.bin"
    payload_hash = write_deterministic_payload(payload_path, payload_size)
    files.add_file(f"{ROOT_NAME}/payload.bin", payload_size)
    creator = lt.create_torrent(
        files,
        piece_size=piece_size,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(seed_directory))
    torrent_path = run_path / "magnet-metadata.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    torrent_info = lt.torrent_info(str(torrent_path))
    info_bytes = bytes(torrent_info.info_section())
    info_hash = str(torrent_info.info_hashes().v1)
    expected_hash = hashlib.sha1(info_bytes).hexdigest()
    if info_hash != expected_hash:
        raise ScenarioFailure("fixture info section does not hash to its v1 identity")
    if len(info_bytes) <= METADATA_BLOCK_SIZE:
        raise ScenarioFailure(
            f"fixture metadata is only {len(info_bytes)} bytes; expected multiple blocks"
        )
    if len(info_bytes) > 1024 * 1024:
        raise ScenarioFailure("fixture metadata exceeds RSTorrent's declared ceiling")
    if any(True for _ in torrent_info.trackers()):
        raise ScenarioFailure("magnet fixture unexpectedly contains a tracker")
    torrent_files = torrent_info.files()
    expected_files = tuple(
        (torrent_files.file_path(index), torrent_files.file_size(index))
        for index in range(torrent_files.num_files())
    )
    if len(expected_files) != EMPTY_FILE_COUNT + 1:
        raise ScenarioFailure("fixture file count changed during torrent creation")
    return Fixture(
        torrent_path=torrent_path,
        seed_directory=seed_directory,
        payload_path=payload_path,
        payload_hash=payload_hash,
        torrent_info=torrent_info,
        info_bytes=info_bytes,
        info_hash=info_hash,
        files=expected_files,
    )


def magnet_uri(info_hash: str, address: str) -> str:
    return f"magnet:?xt=urn:btih:{info_hash}&x.pe={address}"


def parse_fields(output: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for token in output.split():
        if "=" in token:
            key, value = token.split("=", 1)
            fields[key] = value
    return fields


def run_rstorrent_leech(
    binary: Path,
    fixture: Fixture,
    ordinal: int,
    diagnostics: list[str],
) -> tuple[float, str]:
    session = create_session()
    handle: lt.torrent_handle | None = None
    started = time.monotonic()
    try:
        port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            fixture.torrent_info,
            fixture.seed_directory,
            diagnostics,
        )
        output_root = fixture.torrent_path.parent / f"leech-output-{ordinal}"
        command = [
            str(binary),
            "--magnet",
            magnet_uri(fixture.info_hash, f"127.0.0.1:{port}"),
            "--output",
            str(output_root),
            "--timeout-seconds",
            str(METADATA_TIMEOUT_SECONDS),
            "--max-buffered-payload-bytes",
            str(DEFAULT_PAYLOAD_ALLOWANCE),
        ]
        completed = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=PROCESS_TIMEOUT_SECONDS,
            check=False,
        )
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        if completed.returncode != 0:
            raise ScenarioFailure(
                f"magnet leech exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
        fields = parse_fields(completed.stdout)
        if fields.get("info_hash") != fixture.info_hash:
            raise ScenarioFailure("magnet leech reported the wrong info hash")
        if fields.get("pieces") != "3/3":
            raise ScenarioFailure(
                f"magnet leech reported pieces={fields.get('pieces')}, expected 3/3"
            )
        output_payload = output_root / "payload.bin"
        actual_hash = compare_payloads(fixture.payload_path, output_payload)
        if actual_hash != fixture.payload_hash:
            raise ScenarioFailure("magnet leech payload differs from libtorrent seed")
        return time.monotonic() - started, completed.stdout.strip()
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def read_listener_line(
    process: subprocess.Popen[str],
    timeout_seconds: float,
) -> str:
    if process.stdout is None:
        raise ScenarioFailure("metadata seed stdout pipe is unavailable")
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    try:
        events = selector.select(timeout_seconds)
        if not events:
            raise ScenarioFailure("metadata seed did not announce its listener")
        line = process.stdout.readline()
    finally:
        selector.close()
    if not line:
        stderr = process.stderr.read() if process.stderr is not None else ""
        raise ScenarioFailure(
            f"metadata seed exited before listener announcement\nstderr:\n{stderr}"
        )
    return line.strip()


def wait_for_metadata(
    session: lt.session,
    handle: lt.torrent_handle,
    diagnostics: list[str],
) -> lt.torrent_info:
    deadline = time.monotonic() + METADATA_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        status = handle.status()
        if status.errc.value() != 0:
            raise ScenarioFailure(
                f"libtorrent metadata client failed: {status.errc.message()}"
            )
        if status.has_metadata:
            torrent_info = handle.torrent_file()
            if torrent_info is None:
                raise ScenarioFailure("libtorrent reported metadata without torrent_info")
            return torrent_info
        time.sleep(0.02)
    raise ScenarioFailure("libtorrent did not obtain metadata before timeout")


def run_rstorrent_seed(
    binary: Path,
    fixture: Fixture,
    ordinal: int,
    diagnostics: list[str],
) -> tuple[float, str]:
    command = [
        str(binary),
        "--metainfo",
        str(fixture.torrent_path),
        "--listen",
        "127.0.0.1:0",
        "--timeout-seconds",
        str(METADATA_TIMEOUT_SECONDS),
    ]
    process = subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    started = time.monotonic()
    try:
        listener_line = read_listener_line(process, 5)
        fields = parse_fields(listener_line)
        address = fields.get("address")
        if not address or fields.get("info_hash") != fixture.info_hash:
            raise ScenarioFailure(
                f"metadata seed listener report is invalid: {listener_line}"
            )
        if int(fields.get("metadata_size", "0")) != len(fixture.info_bytes):
            raise ScenarioFailure("metadata seed announced the wrong metadata size")

        session = create_session()
        parameters = lt.parse_magnet_uri(magnet_uri(fixture.info_hash, address))
        parameters.save_path = str(
            fixture.torrent_path.parent / f"libtorrent-metadata-{ordinal}"
        )
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        parameters.flags |= lt.torrent_flags.upload_mode
        handle = session.add_torrent(parameters)
        received = wait_for_metadata(session, handle, diagnostics)
        received_info = bytes(received.info_section())
        if received_info != fixture.info_bytes:
            raise ScenarioFailure("libtorrent accepted different raw info bytes")
        if str(received.info_hashes().v1) != fixture.info_hash:
            raise ScenarioFailure("libtorrent accepted the wrong v1 info hash")
        received_files = received.files()
        actual_files = tuple(
            (received_files.file_path(index), received_files.file_size(index))
            for index in range(received_files.num_files())
        )
        if actual_files != fixture.files:
            raise ScenarioFailure("libtorrent received a different metadata file list")

        remaining_stdout, stderr = process.communicate(timeout=5)
        output = "\n".join(
            part for part in (listener_line, remaining_stdout.strip()) if part
        )
        if process.returncode != 0:
            raise ScenarioFailure(
                f"metadata seed exited with status {process.returncode}\n"
                f"stdout:\n{output}\nstderr:\n{stderr}"
            )
        served = parse_fields(output)
        expected_blocks = (
            len(fixture.info_bytes) + METADATA_BLOCK_SIZE - 1
        ) // METADATA_BLOCK_SIZE
        if int(served.get("blocks", "0")) != expected_blocks:
            raise ScenarioFailure("metadata seed reported the wrong block count")
        if int(served.get("requests", "0")) < expected_blocks:
            raise ScenarioFailure("metadata seed reported too few requests")
        return time.monotonic() - started, output
    finally:
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
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)


def run_once(
    download_binary: Path,
    seed_binary: Path,
    ordinal: int,
) -> RunResult:
    run_path = Path(tempfile.mkdtemp(prefix=f"rstorrent-metadata-{ordinal}-"))
    diagnostics: list[str] = []
    failure: BaseException | None = None
    result: RunResult | None = None
    try:
        fixture = create_fixture(run_path)
        leech_seconds, leech_output = run_rstorrent_leech(
            download_binary,
            fixture,
            ordinal,
            diagnostics,
        )
        seed_seconds, seed_output = run_rstorrent_seed(
            seed_binary,
            fixture,
            ordinal,
            diagnostics,
        )
        result = RunResult(
            ordinal=ordinal,
            leech_seconds=leech_seconds,
            seed_seconds=seed_seconds,
            info_hash=fixture.info_hash,
            metadata_size=len(fixture.info_bytes),
            metadata_blocks=(
                len(fixture.info_bytes) + METADATA_BLOCK_SIZE - 1
            )
            // METADATA_BLOCK_SIZE,
            payload_hash=fixture.payload_hash,
            leech_output=leech_output,
            seed_output=seed_output,
        )
    except BaseException as error:
        failure = error
    finally:
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
            f"magnet metadata run {ordinal} failed: {failure}\n"
            f"cleanup={'ok' if not run_path.exists() else 'failed'}\n"
            f"libtorrent alerts:\n{diagnostic_text}"
        ) from failure
    if result is None or not result.cleanup_succeeded:
        raise ScenarioFailure(f"magnet metadata run {ordinal} did not clean up")
    return result


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--runs",
        type=int,
        default=1,
        choices=range(1, 11),
        metavar="1..10",
        help="number of consecutive bidirectional runs (default: 1)",
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
        download_binary, seed_binary = build_binaries(repository)
        for ordinal in range(1, arguments.runs + 1):
            result = run_once(download_binary, seed_binary, ordinal)
            print(
                f"run={result.ordinal} metadata_size={result.metadata_size} "
                f"metadata_blocks={result.metadata_blocks} "
                f"info_hash={result.info_hash} payload_sha1={result.payload_hash} "
                f"leech_seconds={result.leech_seconds:.3f} "
                f"seed_seconds={result.seed_seconds:.3f} cleanup=ok"
            )
            print(f"leech_report={result.leech_output}")
            print(f"seed_report={result.seed_output.replace(chr(10), ' | ')}")
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
