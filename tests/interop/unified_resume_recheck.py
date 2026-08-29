#!/usr/bin/env python3
"""Exercise direct v1 storage, recheck, and repair against libtorrent."""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

import libtorrent as lt

from application_identity import torrent_id_from_add
from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    create_session,
    deterministic_chunk,
    wait_for_listener,
)
from session_checkpoint_crash import SCENARIOS as CHECKPOINT_SCENARIOS
from session_checkpoint_crash import run_once as run_checkpoint_crash
from session_resume import (
    build_binary,
    envelope,
    exchange,
    start_process,
    stop_process,
    wait_for_complete,
)


PIECE_SIZE = 32 * 1024
PROCESS_TIMEOUT_SECONDS = 45
REFERENCE_REVISION = "7d7fc38fac61177fa5e02148f791b2f65250b09d"
OVERSIZED_SUFFIX = b"ignored oversized suffix"


@dataclass(frozen=True)
class FixtureFile:
    path: tuple[str, ...]
    length: int
    sha1: str


@dataclass(frozen=True)
class Fixture:
    shape: str
    name: str
    info_hash: str
    info_bytes: bytes
    torrent_info: lt.torrent_info
    torrent_path: Path
    seed_directory: Path
    files: tuple[FixtureFile, ...]
    piece_hashes: tuple[str, ...]


@dataclass
class TopologyResult:
    shape: str
    info_hash: str
    pieces: int
    artifact: str
    initial_upload: int
    invalidated_pieces: list[int]
    repair_upload: int
    final_file_hashes: dict[str, str]
    libtorrent_recheck_pieces: int
    oversized_suffix_bytes: int
    cleanup: bool = False


def bencode(value: Any) -> bytes:
    if isinstance(value, int):
        return b"i" + str(value).encode("ascii") + b"e"
    if isinstance(value, bytes):
        return str(len(value)).encode("ascii") + b":" + value
    if isinstance(value, list):
        return b"l" + b"".join(bencode(item) for item in value) + b"e"
    if isinstance(value, dict):
        encoded = bytearray(b"d")
        for key in sorted(value):
            encoded.extend(bencode(key))
            encoded.extend(bencode(value[key]))
        encoded.extend(b"e")
        return bytes(encoded)
    raise TypeError(f"cannot bencode {type(value).__name__}")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def fixture_definition(shape: str) -> tuple[str, tuple[tuple[tuple[str, ...], int], ...]]:
    if shape == "length":
        return "single.bin", ((('single.bin',), 96_123),)
    if shape == "one_entry_files":
        return "one-tree", ((('nested', 'payload.bin'), 70_123),)
    if shape == "cross_file":
        return "cross-tree", (
            (('first.bin',), 20_000),
            (('middle.bin',), 20_000),
            (('tail.bin',), 50_123),
        )
    raise ValueError(f"unknown fixture shape {shape}")


def create_fixture(root: Path, shape: str) -> Fixture:
    name, definitions = fixture_definition(shape)
    seed_directory = root / "seed"
    seed_directory.mkdir()
    offset = 0
    fixture_files: list[FixtureFile] = []
    logical = bytearray()
    for parts, length in definitions:
        path = seed_directory.joinpath(name, *parts) if shape != "length" else seed_directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        payload = deterministic_chunk(offset, length)
        path.write_bytes(payload)
        if path.stat().st_size != length:
            raise ScenarioFailure("fixture write did not produce its exact length")
        logical.extend(payload)
        fixture_files.append(
            FixtureFile(parts, length, hashlib.sha1(payload).hexdigest())
        )
        offset += length
    piece_hashes = tuple(
        hashlib.sha1(logical[start : start + PIECE_SIZE]).hexdigest()
        for start in range(0, len(logical), PIECE_SIZE)
    )
    pieces = b"".join(bytes.fromhex(piece_hash) for piece_hash in piece_hashes)
    if shape == "length":
        info: dict[bytes, Any] = {
            b"length": len(logical),
            b"name": name.encode(),
            b"piece length": PIECE_SIZE,
            b"pieces": pieces,
        }
    else:
        info = {
            b"files": [
                {
                    b"length": file.length,
                    b"path": [part.encode() for part in file.path],
                }
                for file in fixture_files
            ],
            b"name": name.encode(),
            b"piece length": PIECE_SIZE,
            b"pieces": pieces,
        }
    info_bytes = bencode(info)
    info_hash = hashlib.sha1(info_bytes).hexdigest()
    torrent_path = root / f"{shape}.torrent"
    torrent_path.write_bytes(bencode({b"info": info}))
    torrent_info = lt.torrent_info(str(torrent_path))
    if str(torrent_info.info_hashes().v1) != info_hash:
        raise ScenarioFailure("independent v1 encoding has the wrong info hash")
    if bytes(torrent_info.info_section()) != info_bytes:
        raise ScenarioFailure("libtorrent changed the independently encoded info dictionary")
    if torrent_info.num_pieces() != len(piece_hashes):
        raise ScenarioFailure("fixture piece geometry changed")
    return Fixture(
        shape,
        name,
        info_hash,
        info_bytes,
        torrent_info,
        torrent_path,
        seed_directory,
        tuple(fixture_files),
        piece_hashes,
    )


def final_file(root: Path, fixture: Fixture, file: FixtureFile) -> Path:
    if fixture.shape == "length":
        return root / fixture.name
    return root.joinpath(fixture.name, *file.path)


def verify_final(
    root: Path, fixture: Fixture, *, preserve_oversized_suffix: bool = False
) -> dict[str, str]:
    hashes: dict[str, str] = {}
    for file in fixture.files:
        path = final_file(root, fixture, file)
        suffix = (
            OVERSIZED_SUFFIX
            if preserve_oversized_suffix
            and fixture.shape in {"length", "one_entry_files"}
            and file == fixture.files[0]
            else b""
        )
        if path.stat().st_size != file.length + len(suffix):
            raise ScenarioFailure(f"{fixture.shape} final length differs for {path}")
        with path.open("rb") as source:
            declared = source.read(file.length)
            actual_suffix = source.read()
        if actual_suffix != suffix:
            raise ScenarioFailure(f"{fixture.shape} oversized suffix differs for {path}")
        actual = hashlib.sha1(declared).hexdigest()
        if actual != file.sha1:
            raise ScenarioFailure(f"{fixture.shape} final hash differs for {path}")
        hashes["/".join(file.path)] = actual
    singular = [
        path
        for path in root.rglob("*")
        if path.name.endswith(".rstorrent-part")
    ]
    if singular:
        raise ScenarioFailure(f"singular bring-up artifacts survived: {singular}")
    return hashes


def add_fixture(
    process: subprocess.Popen[str], fixture: Fixture, port: int, request_id: str
) -> str:
    return torrent_id_from_add(
        exchange(
            process,
            envelope(
                request_id,
                {
                    "type": "add_magnet",
                    "magnet": (
                        f"magnet:?xt=urn:btih:{fixture.info_hash}"
                        f"&x.pe=127.0.0.1:{port}"
                    ),
                    "storage_root": "downloads",
                    "start_content": True,
                    "skip_files": [],
                },
            ),
        )
    )


def mutate_final(root: Path, fixture: Fixture) -> list[int]:
    if fixture.shape == "length":
        path = final_file(root, fixture, fixture.files[0])
        with path.open("r+b") as output:
            output.seek(PIECE_SIZE + 17)
            original = output.read(1)
            output.seek(PIECE_SIZE + 17)
            output.write(bytes([original[0] ^ 0xFF]))
            output.seek(0, 2)
            output.write(OVERSIZED_SUFFIX)
        return [1]
    if fixture.shape == "one_entry_files":
        path = final_file(root, fixture, fixture.files[0])
        with path.open("r+b") as output:
            output.seek(PIECE_SIZE + 31)
            original = output.read(1)
            output.seek(PIECE_SIZE + 31)
            output.write(bytes([original[0] ^ 0xFF]))
            output.seek(0, 2)
            output.write(OVERSIZED_SUFFIX)
        return [1]
    middle = final_file(root, fixture, fixture.files[1])
    middle.unlink()
    return [0, 1]


def wait_libtorrent_check(
    session: lt.session,
    handle: lt.torrent_handle,
    diagnostics: list[str],
) -> int:
    checking = {
        lt.torrent_status.checking_files,
        lt.torrent_status.checking_resume_data,
    }
    deadline = time.monotonic() + PROCESS_TIMEOUT_SECONDS
    observed_check = False
    while time.monotonic() < deadline:
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        status = handle.status()
        if status.errc.value() != 0:
            raise ScenarioFailure(f"libtorrent check failed: {status.errc.message()}")
        if status.state in checking:
            observed_check = True
        elif observed_check or status.num_pieces > 0:
            return status.num_pieces
        time.sleep(0.02)
    raise ScenarioFailure("libtorrent client check timed out")


def libtorrent_recheck_oracle(
    fixture: Fixture,
    invalidated: list[int],
    root: Path,
    diagnostics: list[str],
) -> int:
    oracle = root / "libtorrent-client"
    shutil.copytree(fixture.seed_directory, oracle)
    mutate_final(oracle, fixture)
    session = create_session()
    handle: lt.torrent_handle | None = None
    try:
        parameters = lt.add_torrent_params()
        parameters.ti = fixture.torrent_info
        parameters.save_path = str(oracle)
        parameters.flags &= ~lt.torrent_flags.seed_mode
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        handle = session.add_torrent(parameters)
        checked = wait_libtorrent_check(session, handle, diagnostics)
        expected = fixture.torrent_info.num_pieces() - len(invalidated)
        if checked != expected:
            raise ScenarioFailure(
                f"libtorrent retained {checked} pieces, expected {expected}"
            )
        handle.force_recheck()
        rechecked = wait_libtorrent_check(session, handle, diagnostics)
        if rechecked != expected:
            raise ScenarioFailure("libtorrent force recheck changed the oracle result")
        if fixture.shape in {"length", "one_entry_files"}:
            mutated = final_file(oracle, fixture, fixture.files[0])
            if mutated.stat().st_size != fixture.files[0].length + len(OVERSIZED_SUFFIX):
                raise ScenarioFailure("libtorrent truncated the oversized oracle file")
            with mutated.open("rb") as source:
                source.seek(fixture.files[0].length)
                if source.read() != OVERSIZED_SUFFIX:
                    raise ScenarioFailure("libtorrent changed the oversized oracle suffix")
        return rechecked
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def run_topology_case(binary: Path, shape: str) -> TopologyResult:
    run_path = Path(tempfile.mkdtemp(prefix=f"rstorrent-t073-{shape}-"))
    diagnostics: list[str] = []
    process: subprocess.Popen[str] | None = None
    seed_session: lt.session | None = None
    seed_handle: lt.torrent_handle | None = None
    result: TopologyResult | None = None
    failure: BaseException | None = None
    try:
        fixture = create_fixture(run_path, shape)
        profile = run_path / "profile"
        payload = run_path / "payload"
        seed_session = create_session()
        port = wait_for_listener(seed_session, diagnostics)
        seed_handle = add_seed(
            seed_session, fixture.torrent_info, fixture.seed_directory, diagnostics
        )
        upload_before = seed_handle.status().total_payload_upload
        process = start_process(binary, profile, payload)
        torrent_id = add_fixture(process, fixture, port, "add")
        wait_for_complete(process, fixture, torrent_id)
        initial_upload = seed_handle.status().total_payload_upload - upload_before
        if initial_upload != fixture.torrent_info.total_size():
            raise ScenarioFailure("fresh download did not transfer exact payload bytes")
        verify_final(payload, fixture)
        exchange(
            process,
            envelope(
                "remove-keep-data",
                {
                    "type": "remove_torrent",
                    "torrent_id": torrent_id,
                    "data": "keep",
                },
            ),
        )
        if exchange(process, envelope("removed-snapshot", {"type": "snapshot"}))[
            "snapshot"
        ]["torrents"]:
            raise ScenarioFailure("keep-data removal retained the durable torrent row")
        invalidated = mutate_final(payload, fixture)
        repair_before = seed_handle.status().total_payload_upload
        torrent_id = add_fixture(process, fixture, port, "readd")
        wait_for_complete(process, fixture, torrent_id)
        repair_upload = seed_handle.status().total_payload_upload - repair_before
        expected_upload = sum(
            fixture.torrent_info.piece_size(index) for index in invalidated
        )
        if repair_upload != expected_upload:
            raise ScenarioFailure(
                f"repair uploaded {repair_upload} bytes, expected {expected_upload}"
            )
        final_hashes = verify_final(
            payload,
            fixture,
            preserve_oversized_suffix=shape in {"length", "one_entry_files"},
        )
        oracle_pieces = libtorrent_recheck_oracle(
            fixture, invalidated, run_path, diagnostics
        )
        stop_process(process, graceful=True)
        process = None
        result = TopologyResult(
            shape,
            fixture.info_hash,
            fixture.torrent_info.num_pieces(),
            "file" if shape == "length" else "tree",
            initial_upload,
            invalidated,
            repair_upload,
            final_hashes,
            oracle_pieces,
            len(OVERSIZED_SUFFIX) if shape in {"length", "one_entry_files"} else 0,
        )
    except BaseException as error:
        failure = error
    finally:
        if process is not None:
            stop_process(process, graceful=False)
        if seed_session is not None:
            if seed_handle is not None and seed_handle.is_valid():
                seed_session.remove_torrent(seed_handle)
            seed_session.pause()
        seed_handle = None
        seed_session = None
        gc.collect()
        try:
            shutil.rmtree(run_path)
            cleaned = not run_path.exists()
        except OSError as error:
            cleaned = False
            failure = failure or error
        if result is not None:
            result.cleanup = cleaned
    if failure is not None:
        raise ScenarioFailure(
            f"topology case {shape} failed: {failure}\n"
            f"diagnostics:\n" + "\n".join(diagnostics[-100:])
        ) from failure
    if result is None or not result.cleanup:
        raise ScenarioFailure(f"topology case {shape} did not clean up")
    return result


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--phase",
        choices=("all", "topology", "checkpoint"),
        default="all",
    )
    parser.add_argument("--binary", type=Path)
    parser.add_argument(
        "--shape",
        choices=("length", "one_entry_files", "cross_file"),
        help="run only one topology shape",
    )
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    repository = Path(__file__).resolve().parents[2]
    binary = (arguments.binary or build_binary(repository)).resolve()
    result: dict[str, Any] = {
        "result": "pass",
        "rstorrent_binary_sha256": sha256_file(binary),
        "libtorrent_binding_version": lt.__version__,
        "libtorrent_native_version": lt.version,
        "libtorrent_reference_revision": REFERENCE_REVISION,
        "piece_size": PIECE_SIZE,
        "topology": [],
        "checkpoint_crashes": [],
        "cleanup": True,
    }
    try:
        if arguments.phase in {"all", "topology"}:
            shapes = (
                (arguments.shape,)
                if arguments.shape is not None
                else ("length", "one_entry_files", "cross_file")
            )
            result["topology"] = [
                asdict(run_topology_case(binary, shape))
                for shape in shapes
            ]
        if arguments.phase in {"all", "checkpoint"}:
            result["checkpoint_crashes"] = [
                asdict(run_checkpoint_crash(binary, scenario))
                for scenario in CHECKPOINT_SCENARIOS
            ]
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        result["result"] = "fail"
        result["error"] = str(error)
        print(json.dumps(result, sort_keys=True), file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
