#!/usr/bin/env python3
"""Run the deterministic first-piece scenario against a libtorrent seed."""

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


PAYLOAD_NAME = "fixture.bin"
PAYLOAD_SIZE = 40_000
PIECE_SIZE = 65_536
STARTUP_TIMEOUT_SECONDS = 8
PROCESS_TIMEOUT_SECONDS = 20
DIAGNOSTIC_TIMEOUT_SECONDS = 15


class ScenarioFailure(RuntimeError):
    pass


@dataclass
class RunResult:
    ordinal: int
    elapsed_seconds: float
    expected_hash: str
    actual_hash: str
    info_hash: str
    command_output: str
    cleanup_succeeded: bool = False


def deterministic_payload() -> bytes:
    return bytes(
        ((offset * 73) ^ (offset >> 3) ^ (offset * offset >> 11) ^ 0xA5) & 0xFF
        for offset in range(PAYLOAD_SIZE)
    )


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


def create_fixture(run_directory: Path) -> tuple[Path, Path, bytes, lt.torrent_info]:
    seed_directory = run_directory / "seed"
    seed_directory.mkdir()
    payload = deterministic_payload()
    payload_path = seed_directory / PAYLOAD_NAME
    payload_path.write_bytes(payload)

    files = lt.file_storage()
    files.add_file(PAYLOAD_NAME, len(payload))
    creator = lt.create_torrent(
        files,
        piece_size=PIECE_SIZE,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(seed_directory))
    torrent_path = run_directory / "fixture.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    torrent_info = lt.torrent_info(str(torrent_path))

    if torrent_info.num_pieces() != 1:
        raise ScenarioFailure(
            f"fixture has {torrent_info.num_pieces()} pieces instead of one"
        )
    if torrent_info.piece_length() != PIECE_SIZE:
        raise ScenarioFailure(
            f"fixture piece length is {torrent_info.piece_length()}, expected {PIECE_SIZE}"
        )
    return torrent_path, seed_directory, payload, torrent_info


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
        str(DIAGNOSTIC_TIMEOUT_SECONDS),
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
            "RSTorrent process exceeded harness timeout\n"
            f"stdout:\n{error.stdout or ''}\n"
            f"stderr:\n{error.stderr or ''}"
        ) from error


def run_once(binary: Path, ordinal: int) -> RunResult:
    run_path = Path(tempfile.mkdtemp(prefix=f"rstorrent-interop-{ordinal}-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    alerts: list[str] = []
    result: RunResult | None = None
    failure: BaseException | None = None
    started = time.monotonic()

    try:
        torrent_path, seed_directory, payload, torrent_info = create_fixture(run_path)
        expected_hash = hashlib.sha1(payload).hexdigest()
        info_hash = str(torrent_info.info_hashes().v1)
        session = create_session()
        peer_port = wait_for_listener(session, alerts)
        handle = add_seed(session, torrent_info, seed_directory, alerts)

        output_path = run_path / "downloaded.bin"
        completed = run_diagnostic(binary, torrent_path, peer_port, output_path)
        alerts.extend(alert.message() for alert in session.pop_alerts())
        if completed.returncode != 0:
            raise ScenarioFailure(
                f"RSTorrent exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
        if not output_path.is_file():
            raise ScenarioFailure("RSTorrent succeeded without creating output")

        actual_payload = output_path.read_bytes()
        actual_hash = hashlib.sha1(actual_payload).hexdigest()
        if actual_payload != payload:
            raise ScenarioFailure(
                "downloaded payload differs from deterministic source\n"
                f"expected_sha1={expected_hash}\nactual_sha1={actual_hash}"
            )
        if expected_hash not in completed.stdout:
            raise ScenarioFailure(
                "diagnostic output did not report the expected verified hash\n"
                f"stdout:\n{completed.stdout}"
            )

        result = RunResult(
            ordinal=ordinal,
            elapsed_seconds=time.monotonic() - started,
            expected_hash=expected_hash,
            actual_hash=actual_hash,
            info_hash=info_hash,
            command_output=completed.stdout.strip(),
        )
    except BaseException as error:
        failure = error
    finally:
        if session is not None:
            alerts.extend(alert.message() for alert in session.pop_alerts())
            if handle is not None and handle.is_valid():
                session.remove_torrent(handle)
            session.pause()
        handle = None
        session = None
        gc.collect()
        try:
            shutil.rmtree(run_path)
            cleanup_succeeded = not run_path.exists()
        except OSError as error:
            cleanup_succeeded = False
            if failure is None:
                failure = ScenarioFailure(f"temporary directory cleanup failed: {error}")
        if result is not None:
            result.cleanup_succeeded = cleanup_succeeded

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
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    repository = Path(__file__).resolve().parents[2]
    print(f"python_version={sys.version.split()[0]}")
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    print(f"payload_size={PAYLOAD_SIZE} piece_size={PIECE_SIZE}")

    try:
        binary = build_diagnostic(repository)
        results = [run_once(binary, ordinal) for ordinal in range(1, arguments.runs + 1)]
    except (ScenarioFailure, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        return 1

    for result in results:
        print(
            f"run={result.ordinal} elapsed_seconds={result.elapsed_seconds:.3f} "
            f"expected_sha1={result.expected_hash} actual_sha1={result.actual_hash} "
            f"info_hash={result.info_hash} cleanup=ok"
        )
        print(f"run={result.ordinal} diagnostic={result.command_output}")
    print(f"all_runs={len(results)} cleanup=ok result=pass")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
