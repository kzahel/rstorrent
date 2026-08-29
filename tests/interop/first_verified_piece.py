#!/usr/bin/env python3
"""Run deterministic one-piece scenarios against a libtorrent seed."""

from __future__ import annotations

import argparse
import gc
import hashlib
import re
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

import libtorrent as lt


PAYLOAD_NAME = "fixture.bin"
BLOCK_SIZE = 16 * 1024
HARNESS_CHUNK_SIZE = 1024 * 1024
STARTUP_TIMEOUT_SECONDS = 8
DEFAULT_PAYLOAD_SIZE = 40_000
DEFAULT_PIECE_SIZE = 65_536
DEFAULT_PAYLOAD_ALLOWANCE = 256 * 1024
DEFAULT_DIAGNOSTIC_TIMEOUT_SECONDS = 15
DEFAULT_PROCESS_TIMEOUT_SECONDS = 20
LARGE_PAYLOAD_SIZE = 32 * 1024 * 1024
LARGE_PIECE_SIZE = LARGE_PAYLOAD_SIZE
LARGE_PAYLOAD_ALLOWANCE = 256 * 1024
LARGE_DIAGNOSTIC_TIMEOUT_SECONDS = 60
LARGE_PROCESS_TIMEOUT_SECONDS = 75
SELECTIVE_ROOT_NAME = "fixture"
SELECTIVE_PIECE_SIZE = 32_768
SELECTIVE_PAYLOAD_ALLOWANCE = 32_768
SELECTIVE_TOTAL_SIZE = 133_304
SELECTIVE_REQUESTED_BYTES = 97_232
SELECTIVE_BLOCK_COUNT = 7
SELECTIVE_FILES = (
    ("wanted/start.bin", 20_000, False),
    ("skip/large.bin", 50_000, False),
    ("later.bin", 7_000, False),
    ("wanted/end.bin", 18_000, False),
    ("wanted/empty.bin", 0, False),
    (".pad/3304", 3_304, True),
    ("tail.bin", 35_000, False),
)


class ScenarioFailure(RuntimeError):
    pass


@dataclass(frozen=True)
class ScenarioConfig:
    name: str
    payload_size: int
    piece_size: int
    payload_allowance: int
    diagnostic_timeout_seconds: int
    process_timeout_seconds: int


@dataclass
class RunResult:
    ordinal: int
    elapsed_seconds: float
    transfer_seconds: float
    expected_hash: str
    actual_hash: str
    info_hash: str
    block_count: int
    payload_limit: int
    payload_high_water: int
    verification_buffer: int
    command_output: str
    cleanup_succeeded: bool = False


@dataclass
class SelectiveRunResult:
    ordinal: int
    elapsed_seconds: float
    transfer_seconds: float
    info_hash: str
    piece_hashes: list[str]
    file_hashes: dict[str, str]
    payload_high_water: int
    command_output: str
    cleanup_succeeded: bool = False


def scenario_config(large_piece: bool) -> ScenarioConfig:
    if large_piece:
        return ScenarioConfig(
            name="large",
            payload_size=LARGE_PAYLOAD_SIZE,
            piece_size=LARGE_PIECE_SIZE,
            payload_allowance=LARGE_PAYLOAD_ALLOWANCE,
            diagnostic_timeout_seconds=LARGE_DIAGNOSTIC_TIMEOUT_SECONDS,
            process_timeout_seconds=LARGE_PROCESS_TIMEOUT_SECONDS,
        )
    return ScenarioConfig(
        name="small",
        payload_size=DEFAULT_PAYLOAD_SIZE,
        piece_size=DEFAULT_PIECE_SIZE,
        payload_allowance=DEFAULT_PAYLOAD_ALLOWANCE,
        diagnostic_timeout_seconds=DEFAULT_DIAGNOSTIC_TIMEOUT_SECONDS,
        process_timeout_seconds=DEFAULT_PROCESS_TIMEOUT_SECONDS,
    )


def deterministic_chunk(start: int, length: int) -> bytes:
    return bytes(
        ((offset * 73) ^ (offset >> 3) ^ (offset * offset >> 11) ^ 0xA5) & 0xFF
        for offset in range(start, start + length)
    )


def write_deterministic_payload(path: Path, payload_size: int) -> str:
    digest = hashlib.sha1()
    with path.open("wb") as output:
        offset = 0
        while offset < payload_size:
            length = min(HARNESS_CHUNK_SIZE, payload_size - offset)
            chunk = deterministic_chunk(offset, length)
            output.write(chunk)
            digest.update(chunk)
            offset += length
    return digest.hexdigest()


def compare_payloads(source_path: Path, output_path: Path) -> str:
    digest = hashlib.sha1()
    with source_path.open("rb") as source, output_path.open("rb") as output:
        while True:
            expected = source.read(HARNESS_CHUNK_SIZE)
            actual = output.read(HARNESS_CHUNK_SIZE)
            if actual != expected:
                raise ScenarioFailure("downloaded payload differs from deterministic source")
            if not actual:
                break
            digest.update(actual)
    return digest.hexdigest()


def write_deterministic_range(path: Path, start: int, length: int) -> str:
    digest = hashlib.sha1()
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as output:
        offset = 0
        while offset < length:
            chunk_length = min(HARNESS_CHUNK_SIZE, length - offset)
            chunk = deterministic_chunk(start + offset, chunk_length)
            output.write(chunk)
            digest.update(chunk)
            offset += chunk_length
    return digest.hexdigest()


def hash_file(path: Path) -> str:
    digest = hashlib.sha1()
    with path.open("rb") as source:
        while chunk := source.read(HARNESS_CHUNK_SIZE):
            digest.update(chunk)
    return digest.hexdigest()


def build_diagnostic(repository: Path) -> Path:
    command = [
        "cargo",
        "build",
        "-p",
        "rstorrent-engine",
        "--bin",
        "rstorrent-download-piece",
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
            "failed to build diagnostic\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    binary = repository / "target" / "debug" / "rstorrent-download-piece"
    if not binary.is_file():
        raise ScenarioFailure(f"diagnostic binary was not created at {binary}")
    return binary


def create_fixture(
    run_directory: Path,
    config: ScenarioConfig,
    *,
    require_single_piece: bool = True,
) -> tuple[Path, Path, Path, str, lt.torrent_info]:
    seed_directory = run_directory / "seed"
    seed_directory.mkdir()
    payload_path = seed_directory / PAYLOAD_NAME
    expected_hash = write_deterministic_payload(payload_path, config.payload_size)

    files = lt.file_storage()
    files.add_file(PAYLOAD_NAME, config.payload_size)
    creator = lt.create_torrent(
        files,
        piece_size=config.piece_size,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(seed_directory))
    torrent_path = run_directory / "fixture.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    torrent_info = lt.torrent_info(str(torrent_path))

    if require_single_piece and torrent_info.num_pieces() != 1:
        raise ScenarioFailure(
            f"fixture has {torrent_info.num_pieces()} pieces instead of one"
        )
    if torrent_info.piece_length() != config.piece_size:
        raise ScenarioFailure(
            f"fixture piece length is {torrent_info.piece_length()}, "
            f"expected {config.piece_size}"
        )
    if torrent_info.total_size() != config.payload_size:
        raise ScenarioFailure(
            f"fixture payload length is {torrent_info.total_size()}, "
            f"expected {config.payload_size}"
        )
    if any(True for _ in torrent_info.trackers()):
        raise ScenarioFailure("controlled fixture unexpectedly contains a tracker")
    return torrent_path, seed_directory, payload_path, expected_hash, torrent_info


def create_selective_fixture(
    run_directory: Path,
    root_name: str = SELECTIVE_ROOT_NAME,
    content_offset: int = 0,
) -> tuple[Path, Path, lt.torrent_info, dict[str, str], list[str]]:
    seed_directory = run_directory / "seed"
    torrent_root = seed_directory / root_name
    torrent_root.mkdir(parents=True)
    files = lt.file_storage()
    expected_file_hashes: dict[str, str] = {}
    torrent_offset = 0
    for relative_path, length, padding in SELECTIVE_FILES:
        torrent_path = f"{root_name}/{relative_path}"
        flags = lt.file_storage.flag_pad_file if padding else 0
        files.add_file(torrent_path, length, flags)
        if not padding:
            expected_file_hashes[relative_path] = write_deterministic_range(
                torrent_root / relative_path,
                torrent_offset + content_offset,
                length,
            )
        torrent_offset += length

    if torrent_offset != SELECTIVE_TOTAL_SIZE:
        raise ScenarioFailure(
            f"selective fixture totals {torrent_offset}, expected {SELECTIVE_TOTAL_SIZE}"
        )
    creator = lt.create_torrent(
        files,
        piece_size=SELECTIVE_PIECE_SIZE,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(seed_directory))
    torrent_path = run_directory / "selective.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    torrent_info = lt.torrent_info(str(torrent_path))

    if torrent_info.num_pieces() != 5:
        raise ScenarioFailure(
            f"selective fixture has {torrent_info.num_pieces()} pieces instead of five"
        )
    if torrent_info.piece_length() != SELECTIVE_PIECE_SIZE:
        raise ScenarioFailure(
            f"selective piece length is {torrent_info.piece_length()}, "
            f"expected {SELECTIVE_PIECE_SIZE}"
        )
    if torrent_info.total_size() != SELECTIVE_TOTAL_SIZE:
        raise ScenarioFailure(
            f"selective total is {torrent_info.total_size()}, "
            f"expected {SELECTIVE_TOTAL_SIZE}"
        )
    if torrent_info.num_files() != len(SELECTIVE_FILES):
        raise ScenarioFailure(
            f"selective fixture has {torrent_info.num_files()} files, "
            f"expected {len(SELECTIVE_FILES)}"
        )
    if any(True for _ in torrent_info.trackers()):
        raise ScenarioFailure("controlled selective fixture unexpectedly contains a tracker")
    piece_hashes = [
        bytes(torrent_info.hash_for_piece(index)).hex()
        for index in range(torrent_info.num_pieces())
    ]
    return (
        torrent_path,
        seed_directory,
        torrent_info,
        expected_file_hashes,
        piece_hashes,
    )


def create_session() -> lt.session:
    return lt.session(
        {
            "listen_interfaces": "127.0.0.1:0",
            "enable_dht": False,
            "enable_lsd": False,
            "enable_upnp": False,
            "enable_natpmp": False,
            "enable_incoming_utp": False,
            "enable_outgoing_utp": False,
            "enable_incoming_tcp": True,
            "enable_outgoing_tcp": True,
            "alert_queue_size": 1000,
        }
    )


def wait_for_listener(session: lt.session, diagnostics: list[str]) -> int:
    deadline = time.monotonic() + STARTUP_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        port = session.listen_port()
        if session.is_listening() and port > 0:
            return port
        time.sleep(0.05)
    raise ScenarioFailure("libtorrent did not bind its loopback listener before timeout")


def add_seed(
    session: lt.session,
    torrent_info: lt.torrent_info,
    seed_directory: Path,
    diagnostics: list[str],
) -> lt.torrent_handle:
    parameters = lt.add_torrent_params()
    parameters.ti = torrent_info
    parameters.save_path = str(seed_directory)
    parameters.flags |= lt.torrent_flags.seed_mode
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    handle = session.add_torrent(parameters)

    deadline = time.monotonic() + STARTUP_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        status = handle.status()
        if status.errc.value() != 0:
            raise ScenarioFailure(f"libtorrent seed failed: {status.errc.message()}")
        if status.is_seeding:
            return handle
        time.sleep(0.05)
    raise ScenarioFailure("libtorrent did not enter seed state before timeout")


def run_diagnostic(
    binary: Path,
    torrent_path: Path,
    peer_port: int,
    output_path: Path,
    config: ScenarioConfig,
) -> subprocess.CompletedProcess[str]:
    command = [
        str(binary),
        "--metainfo",
        str(torrent_path),
        "--peer",
        f"127.0.0.1:{peer_port}",
        "--output",
        str(output_path),
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
            "RSTorrent process exceeded harness timeout\n"
            f"stdout:\n{error.stdout or ''}\n"
            f"stderr:\n{error.stderr or ''}"
        ) from error


def run_selective_diagnostic(
    binary: Path,
    torrent_path: Path,
    peer_port: int,
    output_path: Path,
) -> subprocess.CompletedProcess[str]:
    command = [
        str(binary),
        "--metainfo",
        str(torrent_path),
        "--peer",
        f"127.0.0.1:{peer_port}",
        "--output",
        str(output_path),
        "--timeout-seconds",
        str(DEFAULT_DIAGNOSTIC_TIMEOUT_SECONDS),
        "--max-buffered-payload-bytes",
        str(SELECTIVE_PAYLOAD_ALLOWANCE),
        "--skip-file",
        "1",
        "--skip-file",
        "2",
    ]
    try:
        return subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=DEFAULT_PROCESS_TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise ScenarioFailure(
            "selective RSTorrent process exceeded harness timeout\n"
            f"stdout:\n{error.stdout or ''}\n"
            f"stderr:\n{error.stderr or ''}"
        ) from error


def parse_diagnostic(output: str, config: ScenarioConfig) -> dict[str, str]:
    values: dict[str, str] = {}
    for token in output.split():
        if "=" in token:
            key, value = token.split("=", 1)
            values[key] = value

    required = {
        "bytes",
        "sha1",
        "info_hash",
        "blocks",
        "payload_limit",
        "payload_high_water",
        "verification_buffer",
    }
    missing = required - values.keys()
    if missing:
        raise ScenarioFailure(f"diagnostic output is missing fields: {sorted(missing)}")

    try:
        payload_length = int(values["bytes"])
        payload_limit = int(values["payload_limit"])
        payload_high_water = int(values["payload_high_water"])
        block_count = int(values["blocks"])
        verification_buffer = int(values["verification_buffer"])
    except ValueError as error:
        raise ScenarioFailure("diagnostic output contains a non-integer counter") from error

    expected_blocks = (config.payload_size + BLOCK_SIZE - 1) // BLOCK_SIZE
    if payload_length != config.payload_size:
        raise ScenarioFailure("diagnostic reported an unexpected payload length")
    if payload_limit != config.payload_allowance:
        raise ScenarioFailure(
            f"diagnostic payload limit is {payload_limit}, expected "
            f"{config.payload_allowance}"
        )
    if payload_high_water > payload_limit:
        raise ScenarioFailure(
            f"diagnostic payload high-water {payload_high_water} exceeds limit {payload_limit}"
        )
    if block_count != expected_blocks:
        raise ScenarioFailure(
            f"diagnostic block count is {block_count}, expected {expected_blocks}"
        )
    if verification_buffer != BLOCK_SIZE:
        raise ScenarioFailure(
            f"diagnostic verification buffer is {verification_buffer}, expected {BLOCK_SIZE}"
        )
    return values


def parse_selective_diagnostic(output: str) -> dict[str, str]:
    values: dict[str, str] = {}
    for token in output.split():
        if "=" in token:
            key, value = token.split("=", 1)
            values[key] = value

    expected_values = {
        "pieces": "4/5",
        "skipped_pieces": "1",
        "bytes": str(SELECTIVE_REQUESTED_BYTES),
        "blocks": str(SELECTIVE_BLOCK_COUNT),
        "payload_limit": str(SELECTIVE_PAYLOAD_ALLOWANCE),
        "verification_buffer": str(BLOCK_SIZE),
        "selected_file_bytes": "73000",
        "skipped_file_bytes": "57000",
        "padding_bytes": "3304",
        "selected_written_bytes": "73000",
        "part_written_bytes": "24232",
        "part_slots": "2",
        "part_reopened": "true",
    }
    required = {*expected_values, "sha1", "info_hash", "payload_high_water", "part_path"}
    missing = required - values.keys()
    if missing:
        raise ScenarioFailure(
            f"selective diagnostic output is missing fields: {sorted(missing)}"
        )
    for key, expected in expected_values.items():
        if values[key] != expected:
            raise ScenarioFailure(
                f"selective diagnostic {key}={values[key]}, expected {expected}"
            )
    try:
        payload_high_water = int(values["payload_high_water"])
    except ValueError as error:
        raise ScenarioFailure(
            "selective diagnostic payload high-water is not an integer"
        ) from error
    if not (0 < payload_high_water <= SELECTIVE_PAYLOAD_ALLOWANCE):
        raise ScenarioFailure(
            f"selective payload high-water {payload_high_water} is outside "
            f"1..{SELECTIVE_PAYLOAD_ALLOWANCE}"
        )
    return values


def run_once(binary: Path, ordinal: int, config: ScenarioConfig) -> RunResult:
    run_path = Path(tempfile.mkdtemp(prefix=f"rstorrent-interop-{ordinal}-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    alerts: list[str] = []
    result: RunResult | None = None
    failure: BaseException | None = None
    cleanup_errors: list[str] = []
    started = time.monotonic()

    try:
        (
            torrent_path,
            seed_directory,
            payload_path,
            expected_hash,
            torrent_info,
        ) = create_fixture(run_path, config)
        info_hash = str(torrent_info.info_hashes().v1)
        session = create_session()
        peer_port = wait_for_listener(session, alerts)
        handle = add_seed(session, torrent_info, seed_directory, alerts)

        output_path = run_path / "downloaded.bin"
        transfer_started = time.monotonic()
        completed = run_diagnostic(binary, torrent_path, peer_port, output_path, config)
        transfer_seconds = time.monotonic() - transfer_started
        alerts.extend(alert.message() for alert in session.pop_alerts())
        if completed.returncode != 0:
            raise ScenarioFailure(
                f"RSTorrent exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
        if not output_path.is_file():
            raise ScenarioFailure("RSTorrent succeeded without creating output")

        diagnostic = parse_diagnostic(completed.stdout, config)
        actual_hash = compare_payloads(payload_path, output_path)
        if actual_hash != expected_hash:
            raise ScenarioFailure(
                "downloaded payload hash differs from deterministic source\n"
                f"expected_sha1={expected_hash}\nactual_sha1={actual_hash}"
            )
        if diagnostic["sha1"] != expected_hash:
            raise ScenarioFailure(
                "diagnostic output did not report the expected verified hash\n"
                f"stdout:\n{completed.stdout}"
            )
        if diagnostic["info_hash"] != info_hash:
            raise ScenarioFailure(
                "diagnostic output did not report the fixture info hash\n"
                f"stdout:\n{completed.stdout}"
            )

        result = RunResult(
            ordinal=ordinal,
            elapsed_seconds=time.monotonic() - started,
            transfer_seconds=transfer_seconds,
            expected_hash=expected_hash,
            actual_hash=actual_hash,
            info_hash=info_hash,
            block_count=int(diagnostic["blocks"]),
            payload_limit=int(diagnostic["payload_limit"]),
            payload_high_water=int(diagnostic["payload_high_water"]),
            verification_buffer=int(diagnostic["verification_buffer"]),
            command_output=completed.stdout.strip(),
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
        if cleanup_errors:
            cleanup_detail = "; ".join(cleanup_errors)
            if failure is None:
                failure = ScenarioFailure(cleanup_detail)
            else:
                failure = ScenarioFailure(f"{failure}; {cleanup_detail}")
        if result is not None:
            result.cleanup_succeeded = cleanup_succeeded and not cleanup_errors

    if failure is not None:
        diagnostic_text = "\n".join(alerts[-100:]) or "(no libtorrent alerts)"
        raise ScenarioFailure(
            f"run {ordinal} failed: {failure}\n"
            f"cleanup={'ok' if not run_path.exists() else 'failed'}\n"
            f"libtorrent alerts:\n{diagnostic_text}"
        ) from failure
    if result is None:
        raise ScenarioFailure(f"run {ordinal} ended without a result")
    if not result.cleanup_succeeded:
        raise ScenarioFailure(f"run {ordinal} did not clean its temporary directory")
    return result


def run_selective_once(binary: Path, ordinal: int) -> SelectiveRunResult:
    run_path = Path(tempfile.mkdtemp(prefix=f"rstorrent-selective-{ordinal}-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    alerts: list[str] = []
    result: SelectiveRunResult | None = None
    failure: BaseException | None = None
    cleanup_errors: list[str] = []
    started = time.monotonic()

    try:
        (
            torrent_path,
            seed_directory,
            torrent_info,
            expected_file_hashes,
            piece_hashes,
        ) = create_selective_fixture(run_path)
        info_hash = str(torrent_info.info_hashes().v1)
        session = create_session()
        peer_port = wait_for_listener(session, alerts)
        handle = add_seed(session, torrent_info, seed_directory, alerts)

        output_root = run_path / "downloaded"
        transfer_started = time.monotonic()
        completed = run_selective_diagnostic(
            binary,
            torrent_path,
            peer_port,
            output_root,
        )
        transfer_seconds = time.monotonic() - transfer_started
        alerts.extend(alert.message() for alert in session.pop_alerts())
        if completed.returncode != 0:
            raise ScenarioFailure(
                f"selective RSTorrent exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
        if not output_root.is_dir():
            raise ScenarioFailure("selective run succeeded without an output root")

        diagnostic = parse_selective_diagnostic(completed.stdout)
        if diagnostic["info_hash"] != info_hash:
            raise ScenarioFailure(
                "selective diagnostic reported the wrong info hash\n"
                f"stdout:\n{completed.stdout}"
            )
        if diagnostic["sha1"] != piece_hashes[-1]:
            raise ScenarioFailure(
                "selective diagnostic reported the wrong final piece hash\n"
                f"stdout:\n{completed.stdout}"
            )

        actual_hashes: dict[str, str] = {}
        for file_index, (relative_path, _, padding) in enumerate(SELECTIVE_FILES):
            output_path = output_root / relative_path
            if padding or file_index in (1, 2):
                if output_path.exists():
                    raise ScenarioFailure(
                        f"skipped or padding path was created: {relative_path}"
                    )
                continue
            if not output_path.is_file():
                raise ScenarioFailure(f"wanted path is absent: {relative_path}")
            actual_hashes[relative_path] = hash_file(output_path)
            if actual_hashes[relative_path] != expected_file_hashes[relative_path]:
                raise ScenarioFailure(
                    f"direct file differs from seed: {relative_path}\n"
                    f"expected_sha1={expected_file_hashes[relative_path]}\n"
                    f"actual_sha1={actual_hashes[relative_path]}"
                )

        expected_part_path = Path(diagnostic["part_path"])
        if (
            expected_part_path.parent != run_path
            or re.fullmatch(
                r"\.t1-[0-9a-f]{32}\.rstorrent-parts",
                expected_part_path.name,
            )
            is None
        ):
            raise ScenarioFailure(
                f"diagnostic part path is not an opaque owner artifact: "
                f"{diagnostic['part_path']}"
            )
        if not expected_part_path.is_file():
            raise ScenarioFailure("validated part file did not survive the successful run")
        if list(run_path.glob(".*.rstorrent-staging")):
            raise ScenarioFailure("legacy staging root was created")

        result = SelectiveRunResult(
            ordinal=ordinal,
            elapsed_seconds=time.monotonic() - started,
            transfer_seconds=transfer_seconds,
            info_hash=info_hash,
            piece_hashes=piece_hashes,
            file_hashes=actual_hashes,
            payload_high_water=int(diagnostic["payload_high_water"]),
            command_output=completed.stdout.strip(),
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
        if cleanup_errors:
            cleanup_detail = "; ".join(cleanup_errors)
            if failure is None:
                failure = ScenarioFailure(cleanup_detail)
            else:
                failure = ScenarioFailure(f"{failure}; {cleanup_detail}")
        if result is not None:
            result.cleanup_succeeded = cleanup_succeeded and not cleanup_errors

    if failure is not None:
        diagnostic_text = "\n".join(alerts[-100:]) or "(no libtorrent alerts)"
        raise ScenarioFailure(
            f"selective run {ordinal} failed: {failure}\n"
            f"cleanup={'ok' if not run_path.exists() else 'failed'}\n"
            f"libtorrent alerts:\n{diagnostic_text}"
        ) from failure
    if result is None:
        raise ScenarioFailure(f"selective run {ordinal} ended without a result")
    if not result.cleanup_succeeded:
        raise ScenarioFailure(
            f"selective run {ordinal} did not clean its temporary directory"
        )
    return result


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--runs",
        type=int,
        default=1,
        choices=range(1, 11),
        metavar="1..10",
        help="number of consecutive clean runs (default: 1)",
    )
    parser.add_argument(
        "--large-piece",
        action="store_true",
        help="use the 32 MiB piece and 256 KiB payload allowance profile",
    )
    parser.add_argument(
        "--selective-files",
        action="store_true",
        help="use the five-piece selective multi-file profile",
    )
    parser.add_argument(
        "--binary",
        type=Path,
        help="use an existing rstorrent-download-piece binary instead of building",
    )
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    if arguments.large_piece and arguments.selective_files:
        print("--large-piece and --selective-files are mutually exclusive", file=sys.stderr)
        return 2
    config = scenario_config(arguments.large_piece)
    repository = Path(__file__).resolve().parents[2]
    print(f"python_version={sys.version.split()[0]}")
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    if arguments.selective_files:
        print(
            f"scenario=selective total_size={SELECTIVE_TOTAL_SIZE} "
            f"piece_size={SELECTIVE_PIECE_SIZE} pieces=5 files={len(SELECTIVE_FILES)} "
            f"requested_bytes={SELECTIVE_REQUESTED_BYTES} blocks={SELECTIVE_BLOCK_COUNT} "
            f"payload_allowance={SELECTIVE_PAYLOAD_ALLOWANCE}"
        )
    else:
        print(
            f"scenario={config.name} payload_size={config.payload_size} "
            f"piece_size={config.piece_size} payload_allowance={config.payload_allowance}"
        )

    try:
        binary = arguments.binary or build_diagnostic(repository)
        if not binary.is_file():
            raise ScenarioFailure(f"diagnostic binary does not exist: {binary}")
        if arguments.selective_files:
            selective_results = [
                run_selective_once(binary, ordinal)
                for ordinal in range(1, arguments.runs + 1)
            ]
            for result in selective_results:
                print(
                    f"run={result.ordinal} elapsed_seconds={result.elapsed_seconds:.3f} "
                    f"transfer_seconds={result.transfer_seconds:.3f} "
                    f"info_hash={result.info_hash} "
                    f"piece_hashes={','.join(result.piece_hashes)} "
                    f"file_hashes={','.join(f'{path}:{digest}' for path, digest in result.file_hashes.items())} "
                    f"payload_high_water={result.payload_high_water} cleanup=ok"
                )
                print(f"run={result.ordinal} diagnostic={result.command_output}")
            print(
                f"all_runs={len(selective_results)} cleanup=ok result=pass"
            )
            return 0

        results = [
            run_once(binary, ordinal, config)
            for ordinal in range(1, arguments.runs + 1)
        ]
    except (ScenarioFailure, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        return 1

    for result in results:
        print(
            f"run={result.ordinal} elapsed_seconds={result.elapsed_seconds:.3f} "
            f"transfer_seconds={result.transfer_seconds:.3f} "
            f"expected_sha1={result.expected_hash} actual_sha1={result.actual_hash} "
            f"info_hash={result.info_hash} blocks={result.block_count} "
            f"payload_limit={result.payload_limit} "
            f"payload_high_water={result.payload_high_water} "
            f"verification_buffer={result.verification_buffer} cleanup=ok"
        )
        print(f"run={result.ordinal} diagnostic={result.command_output}")
    print(f"all_runs={len(results)} cleanup=ok result=pass")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
