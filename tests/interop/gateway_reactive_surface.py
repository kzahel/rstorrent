#!/usr/bin/env python3
"""Exercise the TypeScript client against the authenticated Rust gateway."""

from __future__ import annotations

import gc
import hashlib
import os
import selectors
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import libtorrent as lt

from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    create_session,
    wait_for_listener,
)
from magnet_metadata import create_fixture, magnet_uri


TOKEN = "controlled-reactive-surface-token"
ORIGIN = "http://127.0.0.1:5173"
UPLOAD_RATE_LIMIT = 8 * 1024


def build_gateway(repository: Path) -> Path:
    completed = subprocess.run(
        ["cargo", "build", "-p", "rstorrent-gateway"],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to build gateway\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    binary = repository / "target/debug/rstorrent-gateway"
    if not binary.is_file():
        raise ScenarioFailure("gateway binary was not created")
    return binary


def start_gateway(
    binary: Path,
    profile: Path,
    storage: Path,
    origin: str = ORIGIN,
) -> tuple[subprocess.Popen[str], str]:
    profile.mkdir()
    storage.mkdir()
    environment = os.environ.copy()
    environment.update(
        {
            "RSTORRENT_PROFILE_ROOT": str(profile),
            "RSTORRENT_STORAGE_ROOT": str(storage),
            "RSTORRENT_GATEWAY_TOKEN": TOKEN,
            "RSTORRENT_GATEWAY_ORIGIN": origin,
            "RSTORRENT_GATEWAY_BIND": "127.0.0.1:0",
        }
    )
    process = subprocess.Popen(
        [str(binary)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=environment,
    )
    if process.stderr is None:
        raise ScenarioFailure("gateway stderr is unavailable")
    selector = selectors.DefaultSelector()
    selector.register(process.stderr, selectors.EVENT_READ)
    deadline = time.monotonic() + 10
    diagnostics: list[str] = []
    try:
        while time.monotonic() < deadline:
            if not selector.select(deadline - time.monotonic()):
                break
            line = process.stderr.readline()
            if not line:
                break
            diagnostics.append(line.rstrip())
            prefix = "gateway listening on "
            if line.startswith(prefix):
                return process, line[len(prefix) :].strip()
    finally:
        selector.close()
    if process.poll() is None:
        process.terminate()
        try:
            process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate(timeout=5)
    raise ScenarioFailure(
        "gateway did not announce its listener\n" + "\n".join(diagnostics)
    )


def run_typescript_client(
    repository: Path,
    address: str,
    magnet: str,
    torrent_id: str,
) -> str:
    environment = os.environ.copy()
    environment.update(
        {
            "RSTORRENT_INTEROP_GATEWAY_URL": f"ws://{address}/control",
            "RSTORRENT_INTEROP_GATEWAY_ORIGIN": ORIGIN,
            "RSTORRENT_INTEROP_GATEWAY_TOKEN": TOKEN,
            "RSTORRENT_INTEROP_MAGNET": magnet,
            "RSTORRENT_INTEROP_TORRENT_ID": torrent_id,
        }
    )
    completed = subprocess.run(
        [
            "npm",
            "test",
            "--prefix",
            "clients/web",
            "--",
            "src/gateway-interop.test.ts",
            "--disableConsoleIntercept",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        env=environment,
        timeout=60,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "TypeScript gateway client failed\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    if "gateway_interop" not in completed.stdout:
        raise ScenarioFailure(
            "TypeScript gateway client did not record its live trace\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    return next(
        line.strip()
        for line in completed.stdout.splitlines()
        if "gateway_interop" in line
    )


def verify_payload(
    storage: Path,
    info_hash: str,
    expected_hash: str,
) -> None:
    payload = storage / info_hash / "payload.bin"
    if not payload.is_file():
        raise ScenarioFailure(f"gateway download payload is absent: {payload}")
    actual = hashlib.sha1(payload.read_bytes()).hexdigest()
    if actual != expected_hash:
        raise ScenarioFailure(
            f"gateway payload hash differs: expected {expected_hash}, got {actual}"
        )


def stop_gateway(process: subprocess.Popen[str]) -> str:
    if process.poll() is None:
        process.send_signal(signal.SIGINT)
    try:
        stdout, stderr = process.communicate(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        stdout, stderr = process.communicate(timeout=5)
        raise ScenarioFailure(
            "gateway did not join after SIGINT\n"
            f"stdout:\n{stdout}\nstderr:\n{stderr}"
        )
    if process.returncode != 0:
        raise ScenarioFailure(
            f"gateway exited with status {process.returncode}\n"
            f"stdout:\n{stdout}\nstderr:\n{stderr}"
        )
    return stderr


def run() -> None:
    repository = Path(__file__).resolve().parents[2]
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-gateway-reactive-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    gateway: subprocess.Popen[str] | None = None
    diagnostics: list[str] = []
    failure: BaseException | None = None
    try:
        fixture = create_fixture(run_path)
        session = create_session()
        session.apply_settings({"upload_rate_limit": UPLOAD_RATE_LIMIT})
        port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            fixture.torrent_info,
            fixture.seed_directory,
            diagnostics,
        )
        binary = build_gateway(repository)
        storage = run_path / "downloads"
        gateway, address = start_gateway(
            binary,
            run_path / "profile",
            storage,
        )
        trace = run_typescript_client(
            repository,
            address,
            magnet_uri(fixture.info_hash, f"127.0.0.1:{port}"),
            fixture.info_hash,
        )
        verify_payload(storage, fixture.info_hash, fixture.payload_hash)
        stop_gateway(gateway)
        gateway = None
        print(
            f"{trace} metadata_size={len(fixture.info_bytes)} pieces=3 "
            f"payload_sha1={fixture.payload_hash} gateway_shutdown=joined cleanup=ok"
        )
    except BaseException as error:
        failure = error
        raise
    finally:
        if gateway is not None:
            try:
                stop_gateway(gateway)
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"gateway cleanup failed: {cleanup_error}", file=sys.stderr)
        if session is not None:
            if handle is not None:
                try:
                    session.remove_torrent(handle)
                except RuntimeError:
                    pass
            session.pause()
            session.pop_alerts()
        handle = None
        session = None
        gc.collect()
        shutil.rmtree(run_path, ignore_errors=True)


def main() -> int:
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    try:
        run()
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
