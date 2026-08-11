#!/usr/bin/env python3
"""Exercise raw .torrent intake and the 30-MiB BEP 9 boundary locally."""

from __future__ import annotations

import argparse
import gc
import json
import os
import signal
import socket
import subprocess
import sys
import tempfile
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

import libtorrent as lt

from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    compare_payloads,
    create_session,
    wait_for_listener,
)
from magnet_metadata import ROOT_NAME, create_fixture as create_metadata_fixture
from udp_tracker_magnet import OneShotUdpTracker


ORIGIN = "http://127.0.0.1:5173"
TOKEN = "torrent-byte-intake-token"
OWNER = "0123456789abcdef0123456789abcdef"
LARGE_METADATA_BYTES = 30 * 1024 * 1024
EXPLICIT_SOURCE_BYTES = 64 * 1024 * 1024
PROCESS_TIMEOUT_SECONDS = 180


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def build_binaries(repository: Path) -> tuple[Path, Path]:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "rstorrent-gateway",
            "--bin",
            "rstorrent-gateway",
            "-p",
            "rstorrent-engine",
            "--bin",
            "rstorrent-public-probe",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=PROCESS_TIMEOUT_SECONDS,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to build intake binaries\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    gateway = repository / "target" / "debug" / "rstorrent-gateway"
    probe = repository / "target" / "debug" / "rstorrent-public-probe"
    if not gateway.is_file() or not probe.is_file():
        raise ScenarioFailure("intake binaries were not created")
    return gateway, probe


def verify_reference(repository: Path) -> dict[str, str]:
    pins = tomllib.loads((repository / "reference" / "pins.toml").read_text())
    pin = next(entry for entry in pins["checkout"] if entry["name"] == "libtorrent")
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repository / pin["path"],
        capture_output=True,
        text=True,
        check=True,
        timeout=10,
    ).stdout.strip()
    if revision != pin["revision"]:
        raise ScenarioFailure(f"libtorrent checkout is {revision}, expected {pin['revision']}")
    if lt.version != "2.0.13.0":
        raise ScenarioFailure(f"libtorrent binding is {lt.version}, expected 2.0.13.0")
    return {"revision": revision, "binding_version": lt.version}


def reserve_port() -> int:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


class Gateway:
    def __init__(
        self,
        binary: Path,
        profile: Path,
        storage: Path,
        network_policy: str,
        *,
        authentication: str = "bearer",
        environment_overrides: dict[str, str] | None = None,
    ) -> None:
        self.port = reserve_port()
        self.origin = ORIGIN
        environment = os.environ.copy()
        environment.update(
            {
                "RSTORRENT_PROFILE_ROOT": str(profile),
                "RSTORRENT_STORAGE_ROOT": str(storage),
                "RSTORRENT_GATEWAY_AUTH": authentication,
                "RSTORRENT_GATEWAY_TOKEN": TOKEN,
                "RSTORRENT_GATEWAY_BIND": f"127.0.0.1:{self.port}",
                "RSTORRENT_GATEWAY_ORIGIN": self.origin,
                "RSTORRENT_NETWORK_POLICY": network_policy,
            }
        )
        if environment_overrides is not None:
            environment.update(environment_overrides)
        self.process = subprocess.Popen(
            [str(binary)],
            cwd=repository_root(),
            env=environment,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                detail = self.process.stderr.read() if self.process.stderr else ""
                raise ScenarioFailure(f"gateway exited during startup: {detail}")
            try:
                self.request("GET", "/api/v1/hello")
                return
            except (OSError, urllib.error.URLError):
                time.sleep(0.05)
        self.stop()
        raise ScenarioFailure("gateway did not become ready")

    def request(
        self,
        method: str,
        path: str,
        body: bytes | None = None,
        content_type: str | None = None,
    ) -> dict[str, Any]:
        headers = {
            "Authorization": f"Bearer {TOKEN}",
            "Origin": self.origin,
            "X-RSTorrent-Owner": OWNER,
        }
        if content_type is not None:
            headers["Content-Type"] = content_type
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.port}{path}",
            data=body,
            headers=headers,
            method=method,
        )
        with urllib.request.urlopen(request, timeout=120) as response:
            return json.loads(response.read())

    def command(self, request_id: str, command: dict[str, Any]) -> dict[str, Any]:
        envelope = {
            "version": 1,
            "request_id": request_id,
            "command": command,
        }
        response = self.request(
            "POST",
            "/api/v1/commands",
            json.dumps(envelope, separators=(",", ":")).encode(),
            "application/json",
        )
        if response.get("status") != "success":
            raise ScenarioFailure(f"command {request_id} failed: {response}")
        return response

    def snapshot(self, request_id: str) -> dict[str, Any]:
        return self.command(request_id, {"type": "snapshot"})["snapshot"]

    def upload(self, request_id: str, source: bytes, *, start_content: bool) -> dict[str, Any]:
        query = urllib.parse.urlencode(
            {
                "request_id": request_id,
                "storage_root": "downloads",
                "start_content": str(start_content).lower(),
                "selection": "all",
            }
        )
        response = self.request(
            "POST",
            f"/api/v1/torrents?{query}",
            source,
            "application/x-bittorrent",
        )
        if response.get("status") != "success":
            raise ScenarioFailure(f"torrent upload failed: {response}")
        return response

    def stop(self) -> None:
        if self.process.poll() is None:
            self.process.send_signal(signal.SIGTERM)
            try:
                self.process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        if self.process.returncode != 0:
            detail = self.process.stderr.read() if self.process.stderr else ""
            raise ScenarioFailure(
                f"gateway exited with {self.process.returncode}: {detail}"
            )
        if self.process.stderr is not None:
            self.process.stderr.close()


def torrent(snapshot: dict[str, Any], info_hash: str) -> dict[str, Any] | None:
    return next(
        (item for item in snapshot["torrents"] if item["torrent_id"] == info_hash),
        None,
    )


def wait_torrent(
    gateway: Gateway,
    info_hash: str,
    predicate: Any,
    label: str,
    timeout_seconds: float = 30,
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_seconds
    ordinal = 0
    row = None
    while time.monotonic() < deadline:
        row = torrent(gateway.snapshot(f"poll-{label}-{ordinal}"), info_hash)
        if row is not None and predicate(row):
            return row
        ordinal += 1
        time.sleep(0.05)
    raise ScenarioFailure(f"torrent {info_hash} did not reach {label}: {row}")


def wait_removed(gateway: Gateway, info_hash: str) -> None:
    deadline = time.monotonic() + 20
    ordinal = 0
    while time.monotonic() < deadline:
        if torrent(gateway.snapshot(f"poll-removed-{ordinal}"), info_hash) is None:
            return
        ordinal += 1
        time.sleep(0.05)
    raise ScenarioFailure(f"torrent {info_hash} was not removed")


def remove(gateway: Gateway, info_hash: str, request_id: str) -> None:
    gateway.command(
        request_id,
        {
            "type": "remove_torrent",
            "torrent_id": info_hash,
            "data": "delete_managed",
        },
    )
    wait_removed(gateway, info_hash)


def tracker_intake_case(
    owned: Path,
    gateway_binary: Path,
    profile: Path,
    storage: Path,
) -> dict[str, Any]:
    case = owned / "tracker-intake"
    case.mkdir()
    fixture = create_metadata_fixture(case)
    torrent_path = fixture.torrent_path
    seed_directory = fixture.seed_directory
    payload_path = fixture.payload_path
    torrent_info = fixture.torrent_info
    info_hash = str(torrent_info.info_hashes().v1)
    diagnostics: list[str] = []
    session = create_session()
    handle = None
    tracker = None
    gateway = None
    try:
        peer_port = wait_for_listener(session, diagnostics)
        handle = add_seed(session, torrent_info, seed_directory, diagnostics)
        tracker = OneShotUdpTracker(
            info_hash,
            peer_port,
            expected_peer_id=None,
        )
        tracker.start()
        metainfo = lt.bdecode(torrent_path.read_bytes())
        metainfo[b"announce"] = f"udp://127.0.0.1:{tracker.port}/announce".encode()
        source = bytes(lt.bencode(metainfo))
        gateway = Gateway(gateway_binary, profile, storage, "loopback_only")
        gateway.upload("upload-tracker-torrent", source, start_content=True)
        tracker.join()
        row = wait_torrent(
            gateway,
            info_hash,
            lambda item: item["state"] == "complete",
            "complete",
        )
        published = storage / ROOT_NAME / payload_path.name
        actual_hash = compare_payloads(payload_path, published)
        gateway.stop()
        gateway = Gateway(gateway_binary, profile, storage, "offline")
        restarted = torrent(gateway.snapshot("restart-tracker-torrent"), info_hash)
        if restarted is None or restarted["state"] != "complete":
            raise ScenarioFailure(f"tracker torrent did not restart complete: {restarted}")
        remove(gateway, info_hash, "remove-tracker-torrent")
        if published.exists():
            raise ScenarioFailure("managed tracker-intake payload survived removal")
        return {
            "info_hash": info_hash,
            "source_bytes": len(source),
            "payload_bytes": torrent_info.total_size(),
            "payload_sha1": actual_hash,
            "tracker_requests": tracker.requests,
            "restart_state": restarted["state"],
            "piece_count": row["piece_count"],
        }
    finally:
        if gateway is not None:
            gateway.stop()
        if tracker is not None:
            tracker.close()
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def exact_large_info(target_bytes: int) -> tuple[bytes, int, str]:
    approximate_pieces = (target_bytes - 100) // 20
    for piece_count in range(approximate_pieces, approximate_pieces - 5_000, -1):
        hash_bytes = piece_count * 20
        for name_length in range(1, 4_097):
            prefix = (
                f"d6:lengthi{piece_count}e4:name{name_length}:".encode()
            )
            suffix = f"12:piece lengthi1e6:pieces{hash_bytes}:".encode()
            if len(prefix) + name_length + len(suffix) + hash_bytes + 1 == target_bytes:
                name = "x" * name_length
                return prefix + name.encode() + suffix + bytes(hash_bytes) + b"e", piece_count, name
    raise ScenarioFailure(f"could not construct exact {target_bytes}-byte info dictionary")


def exact_outer(info: bytes, target_bytes: int) -> bytes:
    fixed = len(b"d7:comment") + len(b":4:info") + len(info) + 1
    comment_bytes = target_bytes - fixed - len(str(target_bytes))
    while True:
        actual = fixed + len(str(comment_bytes)) + comment_bytes
        if actual == target_bytes:
            break
        comment_bytes += target_bytes - actual
    return (
        b"d7:comment"
        + str(comment_bytes).encode()
        + b":"
        + bytes(comment_bytes)
        + b"4:info"
        + info
        + b"e"
    )


def large_metadata_case(
    owned: Path,
    gateway_binary: Path,
    probe_binary: Path,
    profile: Path,
    storage: Path,
) -> dict[str, Any]:
    case = owned / "large-metadata"
    case.mkdir()
    info, piece_count, name = exact_large_info(LARGE_METADATA_BYTES)
    outer = b"d4:info" + info + b"e"
    torrent_info = lt.torrent_info(outer)
    info_hash = str(torrent_info.info_hashes().v1)
    (case / name).write_bytes(bytes(piece_count))
    diagnostics: list[str] = []
    session = create_session()
    peer_port = wait_for_listener(session, diagnostics)
    handle = add_seed(session, torrent_info, case, diagnostics)
    magnet = f"magnet:?xt=urn:btih:{info_hash}&x.pe=127.0.0.1:{peer_port}"
    probe_output = case / "probe-output"
    probe = subprocess.run(
        [
            str(probe_binary),
            "--magnet",
            magnet,
            "--output",
            str(probe_output),
            "--target",
            "metadata",
            "--discovery",
            "tracker",
            "--timeout-seconds",
            "120",
            "--cleanup-seconds",
            "15",
        ],
        capture_output=True,
        text=True,
        timeout=150,
        check=False,
    )
    if probe.returncode != 0:
        raise ScenarioFailure(
            f"30-MiB BEP 9 probe failed\nstdout:\n{probe.stdout}\nstderr:\n{probe.stderr}"
        )
    probe_result = json.loads(probe.stdout)
    gateway = None
    try:
        gateway = Gateway(gateway_binary, profile, storage, "loopback_only")
        gateway.command(
            "add-large-magnet",
            {
                "type": "add_magnet",
                "magnet": magnet,
                "storage_root": "downloads",
                "start_content": True,
                "skip_files": [],
            },
        )
        acquired = wait_torrent(
            gateway,
            info_hash,
            lambda item: item["metadata_available"],
            "metadata-ready",
            120,
        )
        gateway.command(
            "pause-large-magnet", {"type": "pause", "torrent_id": info_hash}
        )
        wait_torrent(
            gateway,
            info_hash,
            lambda item: item["state"] == "paused",
            "paused",
        )
        gateway.stop()
        gateway = Gateway(gateway_binary, profile, storage, "offline")
        restarted = torrent(gateway.snapshot("restart-large-magnet"), info_hash)
        if (
            restarted is None
            or not restarted["metadata_available"]
            or restarted["piece_count"] != piece_count
        ):
            raise ScenarioFailure(f"large metadata did not restart exactly: {restarted}")
        remove(gateway, info_hash, "remove-large-magnet")
        explicit_source = exact_outer(info, EXPLICIT_SOURCE_BYTES)
        gateway.upload(
            "upload-maximum-explicit-torrent", explicit_source, start_content=False
        )
        explicit = wait_torrent(
            gateway,
            info_hash,
            lambda item: item["metadata_available"] and item["state"] == "paused",
            "explicit-metadata-ready",
            120,
        )
        gateway.stop()
        gateway = Gateway(gateway_binary, profile, storage, "offline")
        explicit_restarted = torrent(
            gateway.snapshot("restart-maximum-explicit-torrent"), info_hash
        )
        if (
            explicit_restarted is None
            or not explicit_restarted["metadata_available"]
            or explicit_restarted["piece_count"] != piece_count
        ):
            raise ScenarioFailure(
                f"maximum explicit source did not restart exactly: {explicit_restarted}"
            )
        remove(gateway, info_hash, "remove-maximum-explicit-torrent")
        return {
            "info_hash": info_hash,
            "metadata_bytes": len(info),
            "metadata_blocks": probe_result["diagnostics"]["metadata_blocks"],
            "metadata_requests": probe_result["diagnostics"]["metadata_requests"],
            "metadata_seconds": probe_result["milestones"]["metadata_verified"],
            "piece_count": piece_count,
            "restart_piece_count": restarted["piece_count"],
            "explicit_source_bytes": len(explicit_source),
            "explicit_restart_piece_count": explicit_restarted["piece_count"],
            "explicit_state": explicit["state"],
            "cleanup_succeeded": probe_result["cleanup_succeeded"],
        }
    finally:
        if gateway is not None:
            gateway.stop()
        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--skip-large", action="store_true")
    arguments = parser.parse_args()
    repository = repository_root()
    gateway_binary, probe_binary = build_binaries(repository)
    reference = verify_reference(repository)
    with tempfile.TemporaryDirectory(prefix="rstorrent-torrent-intake-") as temporary:
        owned = Path(temporary)
        profile = owned / "profile"
        storage = owned / "storage"
        storage.mkdir()
        report: dict[str, Any] = {
            "schema_version": 1,
            "reference": reference,
            "torrent_intake": tracker_intake_case(
                owned, gateway_binary, profile, storage
            ),
        }
        if not arguments.skip_large:
            report["large_metadata"] = large_metadata_case(
                owned,
                gateway_binary,
                probe_binary,
                profile,
                storage,
            )
        print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ScenarioFailure as error:
        print(f"torrent byte intake failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
