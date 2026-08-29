#!/usr/bin/env python3
"""Exercise the finite downloader against a pinned libtorrent seed."""

from __future__ import annotations

import argparse
import gc
import hashlib
import os
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

import libtorrent as lt

from bep52_metainfo_oracle import (
    BLOCK,
    SourceFile,
    assert_libtorrent,
    hybrid_fixture,
    pure_v2_fixture,
)
from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    compare_payloads,
    create_session,
    hash_file,
    wait_for_listener,
    write_deterministic_range,
)


ROOT_NAME = "foreground-fixture"
PIECE_SIZE = 16 * 1024
BOUNDARY_BYTES = 8_000
WANTED_BYTES = 1024 * 1024
FILES = (
    ("left-skipped.bin", BOUNDARY_BYTES),
    ("wanted.bin", WANTED_BYTES),
    ("right-skipped.bin", BOUNDARY_BYTES),
)
PADDING_FILE = ".pad/zero-selection.bin"
PADDING_BYTES = PIECE_SIZE
PROCESS_TIMEOUT_SECONDS = 90
WAIT_SECONDS = 15
SLOW_UPLOAD_BYTES = 64 * 1024
FAST_UPLOAD_BYTES = 128 * 1024 * 1024
CONTROL_DIRECTORY = "rstorrent-download-v1"
LOCK_DOMAIN = b"rstorrent-download-output-root-v1\0"
MARKER_FILE = ".rstorrent-download-workspace-v1"


@dataclass(frozen=True)
class V1Fixture:
    torrent_path: Path
    torrent_info: lt.torrent_info
    seed_directory: Path
    seed_root: Path
    info_hash: str
    expected_hashes: dict[str, str]


@dataclass(frozen=True)
class ProcessResult:
    returncode: int
    stdout: str
    stderr: str
    peak_rss_kib: int
    peak_part_bytes: int
    saw_part: bool


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--binary", type=Path)
    values = parser.parse_args()
    if values.binary is not None and not values.binary.is_absolute():
        parser.error("--binary must be absolute")
    return values


def build_binary(repository: Path, configured: Path | None, no_build: bool) -> Path:
    binary = configured or repository / "target" / "debug" / "rstorrent-download"
    if not no_build:
        completed = subprocess.run(
            [
                "cargo",
                "build",
                "-p",
                "rstorrent-session",
                "--bin",
                "rstorrent-download",
            ],
            cwd=repository,
            capture_output=True,
            text=True,
            timeout=180,
            check=False,
        )
        if completed.returncode != 0:
            raise ScenarioFailure(
                "failed to build rstorrent-download\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
    if not binary.is_file():
        raise ScenarioFailure(f"rstorrent-download is unavailable at {binary}")
    return binary


def create_v1_fixture(run_root: Path) -> V1Fixture:
    seed_directory = run_root / "v1-seed"
    seed_root = seed_directory / ROOT_NAME
    seed_root.mkdir(parents=True)
    storage = lt.file_storage()
    expected_hashes: dict[str, str] = {}
    offset = 0
    for relative, length in FILES:
        storage.add_file(f"{ROOT_NAME}/{relative}", length)
        expected_hashes[relative] = write_deterministic_range(
            seed_root / relative, offset, length
        )
        offset += length
    storage.add_file(
        f"{ROOT_NAME}/{PADDING_FILE}",
        PADDING_BYTES,
        lt.file_storage.flag_pad_file,
    )
    creator = lt.create_torrent(
        storage,
        piece_size=PIECE_SIZE,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(seed_directory))
    torrent_path = run_root / "foreground-v1.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    torrent_info = lt.torrent_info(str(torrent_path))
    if (
        torrent_info.num_files() != len(FILES) + 1
        or torrent_info.piece_length() != PIECE_SIZE
        or any(True for _ in torrent_info.trackers())
        or any(True for _ in torrent_info.web_seeds())
    ):
        raise ScenarioFailure("generated v1 fixture changed shape")
    info_hash = str(torrent_info.info_hashes().v1)
    if info_hash != hashlib.sha1(bytes(torrent_info.info_section())).hexdigest():
        raise ScenarioFailure("generated v1 fixture identity is inconsistent")
    return V1Fixture(
        torrent_path=torrent_path,
        torrent_info=torrent_info,
        seed_directory=seed_directory,
        seed_root=seed_root,
        info_hash=info_hash,
        expected_hashes=expected_hashes,
    )


def magnet(fixture: V1Fixture, port: int, selection: str | None = None) -> str:
    value = f"magnet:?xt=urn:btih:{fixture.info_hash}&x.pe=127.0.0.1:{port}"
    return value if selection is None else f"{value}&{selection}"


def assert_libtorrent_selection_oracle(uri: str) -> None:
    parameters = lt.parse_magnet_uri(uri)
    priorities = [int(priority) for priority in parameters.file_priorities]
    if not parameters.flags & lt.torrent_flags.default_dont_download:
        raise ScenarioFailure("libtorrent did not retain default_dont_download for so")
    if priorities != [0, 4]:
        raise ScenarioFailure(f"libtorrent so priorities changed: {priorities}")


def control_root(output_root: Path) -> Path:
    canonical = os.fsencode(output_root.resolve())
    digest = hashlib.sha256(
        LOCK_DOMAIN + struct.pack("<Q", len(canonical)) + canonical
    ).hexdigest()
    return Path(tempfile.gettempdir()) / CONTROL_DIRECTORY / digest


def workspace_paths(output_root: Path) -> list[Path]:
    root = control_root(output_root)
    return sorted(path for path in root.glob("run-*") if path.is_dir())


def assert_workspace_clean(output_root: Path) -> None:
    paths = workspace_paths(output_root)
    if paths:
        raise ScenarioFailure(f"auxiliary workspaces remain after exit: {paths}")


def wait_for_workspace(output_root: Path, process: subprocess.Popen[str]) -> Path:
    deadline = time.monotonic() + WAIT_SECONDS
    while time.monotonic() < deadline:
        paths = workspace_paths(output_root)
        if len(paths) == 1 and (paths[0] / MARKER_FILE).is_file():
            return paths[0]
        if process.poll() is not None:
            stdout, stderr = process.communicate()
            raise ScenarioFailure(
                f"downloader exited before workspace observation: {process.returncode}\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}"
            )
        time.sleep(0.005)
    raise ScenarioFailure("downloader workspace did not appear")


def sample_rss_kib(process: subprocess.Popen[str]) -> int:
    if os.name != "posix":
        return 0
    sampled = subprocess.run(
        ["ps", "-o", "rss=", "-p", str(process.pid)],
        capture_output=True,
        text=True,
        timeout=2,
        check=False,
    )
    try:
        return int(sampled.stdout.strip())
    except ValueError:
        return 0


def observe_process(
    process: subprocess.Popen[str],
    output_root: Path,
    *,
    interrupt_on_part: bool = False,
    kill_on_part: bool = False,
) -> ProcessResult:
    deadline = time.monotonic() + PROCESS_TIMEOUT_SECONDS
    peak_rss_kib = 0
    peak_part_bytes = 0
    saw_part = False
    action_taken = False
    next_rss_sample = 0.0
    while process.poll() is None:
        now = time.monotonic()
        if now >= deadline:
            process.kill()
            stdout, stderr = process.communicate(timeout=5)
            raise ScenarioFailure(
                "rstorrent-download exceeded the controlled timeout\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}"
            )
        if now >= next_rss_sample:
            peak_rss_kib = max(peak_rss_kib, sample_rss_kib(process))
            next_rss_sample = now + 0.05
        for workspace in workspace_paths(output_root):
            for part in workspace.glob(".t1-*.rstorrent-parts"):
                saw_part = True
                try:
                    peak_part_bytes = max(peak_part_bytes, part.stat().st_size)
                except FileNotFoundError:
                    pass
        if saw_part and not action_taken and (interrupt_on_part or kill_on_part):
            action_taken = True
            if kill_on_part:
                process.kill()
            else:
                process.send_signal(signal.SIGINT)
        time.sleep(0.002)
    stdout, stderr = process.communicate(timeout=5)
    peak_rss_kib = max(peak_rss_kib, sample_rss_kib(process))
    if "\x1b" in stdout or "\x1b" in stderr:
        raise ScenarioFailure("non-interactive output contains terminal escapes")
    return ProcessResult(
        returncode=int(process.returncode),
        stdout=stdout,
        stderr=stderr,
        peak_rss_kib=peak_rss_kib,
        peak_part_bytes=peak_part_bytes,
        saw_part=saw_part,
    )


def start_cli(binary: Path, output_root: Path, source: str | Path) -> subprocess.Popen[str]:
    output_root.mkdir(parents=True, exist_ok=True)
    return subprocess.Popen(
        [str(binary), "--output", str(output_root), str(source)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def run_cli(
    binary: Path,
    output_root: Path,
    source: str | Path,
    *,
    interrupt_on_part: bool = False,
    kill_on_part: bool = False,
) -> ProcessResult:
    return observe_process(
        start_cli(binary, output_root, source),
        output_root,
        interrupt_on_part=interrupt_on_part,
        kill_on_part=kill_on_part,
    )


def expect_exit(result: ProcessResult, expected: int, scenario: str) -> None:
    if result.returncode != expected:
        raise ScenarioFailure(
            f"{scenario} exited {result.returncode}, expected {expected}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )


def assert_files(
    fixture: V1Fixture,
    output_root: Path,
    wanted: set[str],
) -> None:
    content_root = output_root / ROOT_NAME
    for relative, _ in FILES:
        output = content_root / relative
        if relative in wanted:
            if not output.is_file():
                raise ScenarioFailure(f"wanted output is missing: {output}")
            actual = hash_file(output)
            if actual != fixture.expected_hashes[relative]:
                raise ScenarioFailure(
                    f"wanted output hash differs for {relative}: {actual}"
                )
        elif output.exists():
            raise ScenarioFailure(f"skipped output was materialized: {output}")
    adjacent = list(output_root.rglob("*.rstorrent-parts"))
    if adjacent:
        raise ScenarioFailure(f"part files escaped into the payload root: {adjacent}")
    padding = content_root / PADDING_FILE
    if padding.exists():
        raise ScenarioFailure(f"padding output was materialized: {padding}")


def assert_no_profile_artifacts(output_root: Path) -> None:
    forbidden = {
        "session.db",
        "session.db-shm",
        "session.db-wal",
        "metrics.db",
        "metrics.db-shm",
        "metrics.db-wal",
    }
    found = sorted(
        path for path in output_root.rglob("*") if path.name in forbidden
    )
    if found:
        raise ScenarioFailure(f"ephemeral run wrote profile databases: {found}")


def run_local_existing(binary: Path, fixture: V1Fixture, run_root: Path) -> None:
    output = run_root / "local-existing"
    shutil.copytree(fixture.seed_root, output / ROOT_NAME)
    result = run_cli(binary, output, fixture.torrent_path)
    expect_exit(result, 0, "local existing v1")
    assert_files(fixture, output, set(fixture.expected_hashes))
    if "checking" not in result.stderr and "verified complete" not in result.stderr:
        raise ScenarioFailure("local existing run did not report verification")
    assert_workspace_clean(output)
    assert_no_profile_artifacts(output)


def materialize_bep52(
    output_root: Path,
    sources: list[SourceFile],
) -> None:
    for source in sources:
        relative = Path(*(component.decode("ascii") for component in source.path))
        destination = output_root / "root" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(source.data)


def assert_bep52_files(output_root: Path, sources: list[SourceFile]) -> None:
    for source in sources:
        relative = Path(*(component.decode("ascii") for component in source.path))
        if (output_root / "root" / relative).read_bytes() != source.data:
            raise ScenarioFailure(f"BEP 52 existing payload differs for {relative}")


def run_bep52_existing(binary: Path, run_root: Path) -> None:
    sources = [
        SourceFile((b"a.bin",), bytes((index * 17) % 251 for index in range(20_001))),
        SourceFile((b"nested", b"b.bin"), bytes((index * 29) % 253 for index in range(70_123))),
    ]
    fixtures = [
        pure_v2_fixture("foreground-pure-v2", sources, 64 * 1024),
        hybrid_fixture("foreground-hybrid", sources, BLOCK, True),
    ]
    for fixture in fixtures:
        torrent_path = run_root / f"{fixture.name}.torrent"
        torrent_path.write_bytes(fixture.torrent)
        assert_libtorrent(fixture, torrent_path)
        output = run_root / f"local-{fixture.name}"
        materialize_bep52(output, sources)
        result = run_cli(binary, output, torrent_path)
        expect_exit(result, 0, fixture.name)
        assert_bep52_files(output, sources)
        assert_workspace_clean(output)
        assert_no_profile_artifacts(output)


def run_network_matrix(
    binary: Path,
    fixture: V1Fixture,
    port: int,
    handle: lt.torrent_handle,
    run_root: Path,
) -> tuple[int, int]:
    ordinary_output = run_root / "magnet-all"
    ordinary = run_cli(binary, ordinary_output, magnet(fixture, port))
    expect_exit(ordinary, 0, "ordinary magnet")
    assert_files(fixture, ordinary_output, set(fixture.expected_hashes))
    assert_workspace_clean(ordinary_output)
    assert_no_profile_artifacts(ordinary_output)

    selected_uri = magnet(fixture, port, "so=1&so=1-1")
    assert_libtorrent_selection_oracle(selected_uri)
    selected_output = run_root / "magnet-selected"
    handle.set_upload_limit(SLOW_UPLOAD_BYTES)
    interrupted = run_cli(
        binary,
        selected_output,
        selected_uri,
        interrupt_on_part=True,
    )
    expect_exit(interrupted, 130, "selective Ctrl-C")
    if not interrupted.saw_part:
        raise ScenarioFailure("selective Ctrl-C did not observe the auxiliary part")
    assert_workspace_clean(selected_output)
    assert_no_profile_artifacts(selected_output)

    handle.set_upload_limit(FAST_UPLOAD_BYTES)
    resumed = run_cli(binary, selected_output, selected_uri)
    expect_exit(resumed, 0, "selective restart")
    assert_files(fixture, selected_output, {"wanted.bin"})
    assert_workspace_clean(selected_output)
    assert_no_profile_artifacts(selected_output)

    empty_output = run_root / "magnet-empty"
    empty = run_cli(
        binary,
        empty_output,
        magnet(fixture, port, f"so={len(FILES)}"),
    )
    expect_exit(empty, 0, "empty selection")
    if "0 files selected" not in empty.stdout:
        raise ScenarioFailure("empty selection did not report its no-op")
    if any(empty_output.iterdir()):
        raise ScenarioFailure("empty selection materialized payload")
    assert_workspace_clean(empty_output)

    rejected_output = run_root / "magnet-rejected"
    rejected_uri = magnet(
        fixture,
        port,
        "so=1-&tr=https%3A%2F%2Fuser%3Asecret%40tracker.invalid%2Fannounce",
    )
    rejected = run_cli(binary, rejected_output, rejected_uri)
    expect_exit(rejected, 4, "malformed selection")
    if "secret" in rejected.stdout or "secret" in rejected.stderr:
        raise ScenarioFailure("rejected magnet leaked tracker credentials")
    assert_workspace_clean(rejected_output)

    return (
        max(ordinary.peak_rss_kib, interrupted.peak_rss_kib, resumed.peak_rss_kib),
        max(interrupted.peak_part_bytes, resumed.peak_part_bytes),
    )


def run_contention_and_crash(
    binary: Path,
    fixture: V1Fixture,
    port: int,
    handle: lt.torrent_handle,
    run_root: Path,
) -> tuple[int, int]:
    selected_uri = magnet(fixture, port, "so=1&so=1-1")
    output = run_root / "contention"
    handle.set_upload_limit(SLOW_UPLOAD_BYTES)
    owner = start_cli(binary, output, selected_uri)
    wait_for_workspace(output, owner)
    contender = subprocess.run(
        [
            str(binary),
            "--output",
            str(output / "."),
            str(output / "missing-source.torrent"),
        ],
        capture_output=True,
        text=True,
        timeout=10,
        check=False,
    )
    if contender.returncode != 3:
        owner.send_signal(signal.SIGINT)
        owner.communicate(timeout=10)
        raise ScenarioFailure(
            f"same-root contender exited {contender.returncode}, expected 3\n"
            f"stdout:\n{contender.stdout}\nstderr:\n{contender.stderr}"
        )
    owner.send_signal(signal.SIGINT)
    owner_result = observe_process(owner, output)
    expect_exit(owner_result, 130, "contention owner interruption")
    assert_workspace_clean(output)

    crash_output = run_root / "forced-crash"
    crashed = run_cli(binary, crash_output, selected_uri, kill_on_part=True)
    if crashed.returncode >= 0 or not crashed.saw_part:
        raise ScenarioFailure(
            f"forced crash did not die with an auxiliary part: {crashed.returncode}"
        )
    stale = workspace_paths(crash_output)
    if len(stale) != 1 or not list(stale[0].glob(".t1-*.rstorrent-parts")):
        raise ScenarioFailure("forced crash did not retain its exact stale workspace")
    handle.set_upload_limit(FAST_UPLOAD_BYTES)
    recovered = run_cli(binary, crash_output, selected_uri)
    expect_exit(recovered, 0, "forced-crash recovery")
    assert_files(fixture, crash_output, {"wanted.bin"})
    assert_workspace_clean(crash_output)
    assert_no_profile_artifacts(crash_output)
    return (
        max(owner_result.peak_rss_kib, crashed.peak_rss_kib, recovered.peak_rss_kib),
        max(crashed.peak_part_bytes, recovered.peak_part_bytes),
    )


def corrupt_and_repair(
    binary: Path,
    fixture: V1Fixture,
    port: int,
    run_root: Path,
) -> None:
    output = run_root / "repair"
    shutil.copytree(fixture.seed_root, output / ROOT_NAME)
    wanted = output / ROOT_NAME / "wanted.bin"
    with wanted.open("r+b") as payload:
        payload.seek(WANTED_BYTES // 2)
        original = payload.read(1)
        payload.seek(WANTED_BYTES // 2)
        payload.write(bytes([original[0] ^ 0xFF]))
    repaired = run_cli(binary, output, magnet(fixture, port))
    expect_exit(repaired, 0, "same-length corruption repair")
    assert_files(fixture, output, set(fixture.expected_hashes))
    with wanted.open("r+b") as payload:
        payload.truncate(WANTED_BYTES // 2)
    partial = run_cli(binary, output, magnet(fixture, port))
    expect_exit(partial, 0, "partial payload repair")
    assert_files(fixture, output, set(fixture.expected_hashes))
    assert_workspace_clean(output)


def run(arguments: argparse.Namespace) -> None:
    if os.name != "posix":
        raise ScenarioFailure("the signal/crash interop harness requires a POSIX host")
    if lt.version != "2.0.13.0":
        raise ScenarioFailure(f"libtorrent {lt.version} is not pinned 2.0.13.0")
    repository = repository_root()
    binary = build_binary(repository, arguments.binary, arguments.no_build)
    with tempfile.TemporaryDirectory(prefix="rstorrent-foreground-download-") as temporary:
        run_root = Path(temporary)
        fixture = create_v1_fixture(run_root)
        run_local_existing(binary, fixture, run_root)
        run_bep52_existing(binary, run_root)

        diagnostics: list[str] = []
        session = create_session()
        handle: lt.torrent_handle | None = None
        try:
            port = wait_for_listener(session, diagnostics)
            handle = add_seed(
                session,
                fixture.torrent_info,
                fixture.seed_directory,
                diagnostics,
            )
            network_rss, network_part = run_network_matrix(
                binary, fixture, port, handle, run_root
            )
            crash_rss, crash_part = run_contention_and_crash(
                binary, fixture, port, handle, run_root
            )
            corrupt_and_repair(binary, fixture, port, run_root)
            diagnostics.extend(alert.message() for alert in session.pop_alerts())
        finally:
            if handle is not None and handle.is_valid():
                session.remove_torrent(handle)
            session.pause()
            handle = None
            session = None
            gc.collect()

        print(
            "stateless_foreground_download=ok "
            f"libtorrent={lt.version} payload_bytes={fixture.torrent_info.total_size()} "
            f"piece_count={fixture.torrent_info.num_pieces()} "
            f"peak_rss_kib={max(network_rss, crash_rss)} "
            f"part_high_water_bytes={max(network_part, crash_part)} "
            "profile_artifacts=0 stale_workspaces=0"
        )


def main() -> int:
    try:
        run(parse_arguments())
        return 0
    except (OSError, RuntimeError, ValueError) as error:
        print(f"stateless foreground download failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
