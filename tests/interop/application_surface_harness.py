"""Shared process and payload helpers for application-surface scenarios."""

from __future__ import annotations

import hashlib
import json
import os
import selectors
import signal
import subprocess
import time
from pathlib import Path

from first_verified_piece import ScenarioFailure


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
    network_policy: str = "loopback_only",
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
            "RSTORRENT_NETWORK_POLICY": network_policy,
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


def verify_payload(storage: Path, info_hash: str, expected_hash: str) -> None:
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


def connection_metrics(stderr: str) -> dict[str, object]:
    prefix = "gateway_connection_metrics "
    matches = [
        json.loads(line[len(prefix) :])
        for line in stderr.splitlines()
        if line.startswith(prefix)
    ]
    if len(matches) != 1 or not isinstance(matches[0], dict):
        raise ScenarioFailure("gateway did not emit one connection metrics snapshot")
    return matches[0]
