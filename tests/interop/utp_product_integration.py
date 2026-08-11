#!/usr/bin/env python3
"""Prove default-off product uTP selection and sequential TCP fallback."""

from __future__ import annotations

import gc
import json
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import libtorrent as lt

from incoming_seeding import read_json_line, seed_snapshot, terminate_process
from utp_reference_oracle import (
    MAX_DIAGNOSTICS,
    OracleFailure,
    PAYLOAD_NAME,
    PAYLOAD_SIZE,
    PIECE_SIZE,
    POLL_SECONDS,
    TRANSPORT_SETTINGS,
    add_torrent,
    collect_alerts,
    create_fixture,
    hash_file,
    stats_snapshot,
    wait_until_ready,
)


ROOT = Path(__file__).resolve().parents[2]
BINARY_NAME = "rstorrent-incoming-seed"
CASE_TIMEOUT_SECONDS = 60.0
BUILD_TIMEOUT_SECONDS = 180.0
TRANSFER_RATE_BYTES = 256 * 1024


class ProductInteropFailure(RuntimeError):
    pass


@dataclass
class PeerEvidence:
    rows: dict[str, dict[str, Any]] = field(default_factory=dict)
    peer_high_water: int = 0

    def observe(self, snapshot: dict[str, Any]) -> None:
        projection = snapshot.get("peers")
        if not isinstance(projection, dict) or projection.get("type") != "peers":
            raise ProductInteropFailure(f"invalid application peer snapshot: {projection}")
        peers = projection.get("peers")
        if not isinstance(peers, list) or not all(isinstance(row, dict) for row in peers):
            raise ProductInteropFailure("application peer snapshot has invalid rows")
        self.peer_high_water = max(self.peer_high_water, len(peers))
        for row in peers:
            connection_id = row.get("connection_id")
            if not isinstance(connection_id, str):
                raise ProductInteropFailure("application peer row lacks a connection ID")
            self.rows[connection_id] = row.copy()

    def require(self, direction: str, transport: str) -> None:
        matching = [
            row
            for row in self.rows.values()
            if row.get("direction") == direction and row.get("transport") == transport
        ]
        if not matching:
            raise ProductInteropFailure(
                f"application never observed {direction} {transport}: {self.rows}"
            )
        wrong = [
            row
            for row in self.rows.values()
            if row.get("direction") == direction and row.get("transport") != transport
        ]
        if wrong:
            raise ProductInteropFailure(
                f"application observed an unexpected transport for {direction}: {wrong}"
            )

    def summary(self) -> dict[str, Any]:
        return {
            "peer_high_water": self.peer_high_water,
            "directions": sorted(
                {
                    f"{row.get('direction')}:{row.get('transport')}"
                    for row in self.rows.values()
                }
            ),
        }


@dataclass
class ApplicationProcess:
    process: subprocess.Popen[str]
    ready: dict[str, Any]
    evidence: PeerEvidence = field(default_factory=PeerEvidence)

    def snapshot(self) -> dict[str, Any]:
        snapshot = seed_snapshot(self.process)
        self.evidence.observe(snapshot)
        return snapshot

    def stop(self, deadline: float) -> tuple[dict[str, Any], dict[str, Any]]:
        final_snapshot = self.snapshot()
        if self.process.stdin is None:
            raise ProductInteropFailure("application diagnostic stdin is unavailable")
        self.process.stdin.write("stop\n")
        self.process.stdin.flush()
        stopped = read_json_line(self.process, max(1.0, deadline - time.monotonic()))
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise ProductInteropFailure("application shutdown exceeded the case deadline")
        try:
            return_code = self.process.wait(timeout=remaining)
        except subprocess.TimeoutExpired as error:
            terminate_process(self.process)
            raise ProductInteropFailure("application shutdown did not join") from error
        stderr = self.process.stderr.read() if self.process.stderr is not None else ""
        if return_code != 0:
            raise ProductInteropFailure(
                f"application diagnostic exited {return_code}: {stderr[-4096:]}"
            )
        if stopped.get("event") != "stopped":
            raise ProductInteropFailure(f"unexpected application stop event: {stopped}")
        return final_snapshot, stopped

    def cleanup(self) -> None:
        terminate_process(self.process)


def build_binary() -> Path:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "rstorrent-session",
            "--bin",
            BINARY_NAME,
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=BUILD_TIMEOUT_SECONDS,
        check=False,
    )
    if completed.returncode != 0:
        raise ProductInteropFailure(
            "failed to build product uTP diagnostic\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    suffix = ".exe" if sys.platform == "win32" else ""
    binary = ROOT / "target" / "debug" / f"{BINARY_NAME}{suffix}"
    if not binary.is_file():
        raise ProductInteropFailure(f"built diagnostic is missing: {binary}")
    return binary


def parse_loopback_endpoint(value: object, field: str) -> tuple[str, int]:
    if not isinstance(value, str):
        raise ProductInteropFailure(f"application readiness lacks {field}")
    host, separator, port_text = value.rpartition(":")
    if separator != ":" or host != "127.0.0.1":
        raise ProductInteropFailure(f"{field} is not IPv4 loopback: {value}")
    try:
        port = int(port_text)
    except ValueError as error:
        raise ProductInteropFailure(f"{field} has an invalid port: {value}") from error
    if not 1 <= port <= 65535:
        raise ProductInteropFailure(f"{field} has an invalid port: {value}")
    return host, port


def start_application(
    binary: Path,
    *,
    profile_root: Path,
    storage_root: Path,
    torrent_path: Path,
    fixture_payload: Path | None = None,
    peer: tuple[str, int] | None = None,
) -> ApplicationProcess:
    command = [
        str(binary),
        "--profile-root",
        str(profile_root),
        "--storage-root",
        str(storage_root),
        "--metainfo",
        str(torrent_path),
        "--encryption",
        "disabled",
        "--utp",
    ]
    if fixture_payload is not None:
        command.extend(
            [
                "--fixture-payload",
                str(fixture_payload),
                "--initial-piece",
                "0",
            ]
        )
    if peer is not None:
        command.extend(["--peer", f"{peer[0]}:{peer[1]}"])
    process = subprocess.Popen(
        command,
        cwd=ROOT,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    try:
        ready = read_json_line(process, 30)
        if ready.get("event") != "ready" or ready.get("registrations") != 1:
            raise ProductInteropFailure(f"unexpected application readiness: {ready}")
        parse_loopback_endpoint(ready.get("listen"), "listen")
        parse_loopback_endpoint(ready.get("utp_listen"), "utp_listen")
        return ApplicationProcess(process=process, ready=ready)
    except BaseException:
        terminate_process(process)
        raise


def create_libtorrent_session(transport: str) -> lt.session:
    settings = dict(TRANSPORT_SETTINGS)
    if transport == "utp":
        settings.update(
            {
                "enable_incoming_tcp": False,
                "enable_outgoing_tcp": False,
                "enable_incoming_utp": True,
                "enable_outgoing_utp": True,
            }
        )
    elif transport == "tcp":
        settings.update(
            {
                "enable_incoming_tcp": True,
                "enable_outgoing_tcp": True,
                "enable_incoming_utp": False,
                "enable_outgoing_utp": False,
            }
        )
    else:
        raise ProductInteropFailure(f"unknown oracle transport {transport}")
    settings["upload_rate_limit"] = TRANSFER_RATE_BYTES
    settings["download_rate_limit"] = TRANSFER_RATE_BYTES
    return lt.session(settings)


def check_handle(handle: lt.torrent_handle, role: str) -> None:
    status = handle.status()
    if status.errc.value() != 0:
        raise ProductInteropFailure(f"libtorrent {role} failed: {status.errc.message()}")


def validate_payload(path: Path, expected_sha1: str) -> None:
    if not path.is_file() or path.stat().st_size != PAYLOAD_SIZE:
        raise ProductInteropFailure(f"completed payload is missing or malformed: {path}")
    if hash_file(path) != expected_sha1:
        raise ProductInteropFailure(f"completed payload hash differs: {path}")


def validate_application_summary(snapshot: dict[str, Any]) -> None:
    projection = snapshot.get("summary")
    if not isinstance(projection, dict) or projection.get("type") != "torrent":
        raise ProductInteropFailure(f"invalid application summary snapshot: {projection}")
    torrent = projection.get("torrent")
    if not isinstance(torrent, dict):
        raise ProductInteropFailure("application summary has no torrent")
    if torrent.get("state") != "complete" or torrent.get("storage_state") != "published":
        raise ProductInteropFailure(f"application did not publish exact completion: {torrent}")


def validate_utp_snapshot(
    snapshot: dict[str, Any], *, established: bool, inactive: bool = False
) -> dict[str, Any]:
    utp = snapshot.get("utp")
    if not isinstance(utp, dict):
        raise ProductInteropFailure(f"application snapshot lacks uTP counters: {snapshot}")
    if utp.get("worker_panics") != 0:
        raise ProductInteropFailure(f"application observed a uTP worker panic: {utp}")
    if not isinstance(utp.get("datagrams_sent"), int) or utp["datagrams_sent"] <= 0:
        raise ProductInteropFailure(f"application sent no uTP datagrams: {utp}")
    if utp.get("connection_high_water") != 1:
        raise ProductInteropFailure(f"application uTP connection high water changed: {utp}")
    if inactive and utp.get("active_connections") != 0:
        raise ProductInteropFailure(f"fallback retained a live uTP transport: {utp}")
    if established and (
        utp.get("selected_mtu_min_bytes") != 548
        or utp.get("selected_mtu_max_bytes") != 548
    ):
        raise ProductInteropFailure(f"application did not use fixed-548 uTP: {utp}")
    return utp


def validate_forced_utp_stats(stats: dict[str, int], role: str) -> None:
    if stats["peer.num_tcp_peers"] != 0:
        raise ProductInteropFailure(f"libtorrent {role} observed a TCP peer")
    if stats["utp.utp_packets_in"] <= 0 or stats["utp.utp_packets_out"] <= 0:
        raise ProductInteropFailure(f"libtorrent {role} lacked bidirectional uTP packets")


def wait_libtorrent_completion(
    application: ApplicationProcess,
    session: lt.session,
    handle: lt.torrent_handle,
    deadline: float,
    diagnostics: list[str],
) -> dict[str, Any]:
    last_snapshot: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        if application.process.poll() is not None:
            raise ProductInteropFailure("application stopped during incoming uTP transfer")
        collect_alerts(session, diagnostics)
        check_handle(handle, "leecher")
        last_snapshot = application.snapshot()
        if handle.status().is_seeding:
            return last_snapshot
        time.sleep(POLL_SECONDS)
    raise ProductInteropFailure("incoming product uTP transfer exceeded its deadline")


def wait_application_completion(
    application: ApplicationProcess,
    session: lt.session,
    handle: lt.torrent_handle,
    payload_path: Path,
    deadline: float,
    diagnostics: list[str],
) -> dict[str, Any]:
    while time.monotonic() < deadline:
        if application.process.poll() is not None:
            raise ProductInteropFailure("application stopped during outgoing transfer")
        collect_alerts(session, diagnostics)
        check_handle(handle, "seed")
        snapshot = application.snapshot()
        summary = snapshot.get("summary")
        torrent = summary.get("torrent") if isinstance(summary, dict) else None
        if (
            isinstance(torrent, dict)
            and torrent.get("state") == "complete"
            and torrent.get("storage_state") == "published"
            and payload_path.is_file()
        ):
            return snapshot
        time.sleep(POLL_SECONDS)
    raise ProductInteropFailure("outgoing product transfer exceeded its deadline")


def cleanup_libtorrent(
    session: lt.session | None, handle: lt.torrent_handle | None
) -> None:
    if session is not None and handle is not None and handle.is_valid():
        session.remove_torrent(handle)
    if session is not None:
        session.pause()
    gc.collect()


def run_incoming_utp(binary: Path, root: Path) -> dict[str, Any]:
    started = time.monotonic()
    deadline = started + CASE_TIMEOUT_SECONDS
    diagnostics: list[str] = []
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    application: ApplicationProcess | None = None
    root.mkdir()
    torrent_info, seed_root, expected_sha1 = create_fixture(root)
    leech_root = root / "libtorrent-leech"
    leech_root.mkdir()
    try:
        application = start_application(
            binary,
            profile_root=root / "profile",
            storage_root=seed_root,
            torrent_path=root / "forced-utp.torrent",
        )
        session = create_libtorrent_session("utp")
        handle = add_torrent(session, torrent_info, leech_root, seed=False)
        wait_until_ready(
            session,
            handle,
            seed=False,
            deadline=deadline,
            diagnostics=diagnostics,
        )
        handle.connect_peer(parse_loopback_endpoint(application.ready["utp_listen"], "utp_listen"))
        snapshot = wait_libtorrent_completion(
            application, session, handle, deadline, diagnostics
        )
        validate_payload(leech_root / PAYLOAD_NAME, expected_sha1)
        stats = stats_snapshot(session, diagnostics, deadline)
        validate_forced_utp_stats(stats, "leecher")
        application.evidence.require("incoming", "utp")
        validate_utp_snapshot(snapshot, established=True)
        final_snapshot, stopped = application.stop(deadline)
        validate_utp_snapshot(final_snapshot, established=True)
        result = {
            "role": "application_seed",
            "transport": "utp",
            "payload_sha1": expected_sha1,
            "application_peers": application.evidence.summary(),
            "libtorrent_stats": stats,
            "stopped": stopped,
            "seconds": round(time.monotonic() - started, 6),
            "diagnostics": diagnostics[-MAX_DIAGNOSTICS:],
        }
        application = None
        return result
    finally:
        if application is not None:
            application.cleanup()
        cleanup_libtorrent(session, handle)


def run_outgoing_case(binary: Path, root: Path, transport: str) -> dict[str, Any]:
    started = time.monotonic()
    deadline = started + CASE_TIMEOUT_SECONDS
    diagnostics: list[str] = []
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    application: ApplicationProcess | None = None
    root.mkdir()
    torrent_info, seed_root, expected_sha1 = create_fixture(root)
    leech_root = root / "application-leech"
    leech_root.mkdir()
    try:
        session = create_libtorrent_session(transport)
        handle = add_torrent(session, torrent_info, seed_root, seed=True)
        port = wait_until_ready(
            session,
            handle,
            seed=True,
            deadline=deadline,
            diagnostics=diagnostics,
        )
        application = start_application(
            binary,
            profile_root=root / "profile",
            storage_root=leech_root,
            torrent_path=root / "forced-utp.torrent",
            fixture_payload=seed_root / PAYLOAD_NAME,
            peer=("127.0.0.1", port),
        )
        payload_path = leech_root / PAYLOAD_NAME
        snapshot = wait_application_completion(
            application,
            session,
            handle,
            payload_path,
            deadline,
            diagnostics,
        )
        validate_payload(payload_path, expected_sha1)
        validate_application_summary(snapshot)
        stats = stats_snapshot(session, diagnostics, deadline)
        expected_transport = "utp" if transport == "utp" else "tcp"
        application.evidence.require("outgoing", expected_transport)
        if transport == "utp":
            validate_forced_utp_stats(stats, "seed")
            validate_utp_snapshot(snapshot, established=True)
        else:
            if stats["net.sent_payload_bytes"] <= 0:
                raise ProductInteropFailure("TCP-only seed sent no payload bytes")
            validate_utp_snapshot(snapshot, established=False, inactive=True)
        final_snapshot, stopped = application.stop(deadline)
        validate_utp_snapshot(
            final_snapshot,
            established=transport == "utp",
            inactive=transport == "tcp",
        )
        result = {
            "role": "application_leecher",
            "transport": expected_transport,
            "utp_attempted": True,
            "payload_sha1": expected_sha1,
            "application_peers": application.evidence.summary(),
            "libtorrent_stats": stats,
            "stopped": stopped,
            "seconds": round(time.monotonic() - started, 6),
            "diagnostics": diagnostics[-MAX_DIAGNOSTICS:],
        }
        application = None
        return result
    finally:
        if application is not None:
            application.cleanup()
        cleanup_libtorrent(session, handle)


def run() -> dict[str, Any]:
    binary = build_binary()
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="rstorrent-utp-product-") as temporary:
        root = Path(temporary)
        incoming = run_incoming_utp(binary, root / "incoming-utp")
        outgoing = run_outgoing_case(binary, root / "outgoing-utp", "utp")
        fallback = run_outgoing_case(binary, root / "fallback-tcp", "tcp")
    return {
        "schema_version": 1,
        "oracle": "application-libtorrent-utp-composition-loopback",
        "libtorrent_version": lt.__version__,
        "policy": {
            "application_default": "tcp_only",
            "diagnostic_override": "prefer_utp",
            "ipv4_only": True,
            "plaintext_only": True,
            "fallback": "sequential_tcp_after_utp_transport_failure",
        },
        "payload": {
            "bytes": PAYLOAD_SIZE,
            "piece_bytes": PIECE_SIZE,
            "sha1": incoming["payload_sha1"],
        },
        "cases": [incoming, outgoing, fallback],
        "cleanup": {
            "succeeded": True,
            "temporary_directory_removed": True,
        },
        "seconds": round(time.monotonic() - started, 6),
    }


def main() -> int:
    print(json.dumps(run(), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (
        ProductInteropFailure,
        OracleFailure,
        OSError,
        subprocess.SubprocessError,
    ) as error:
        print(f"product uTP interoperability failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
