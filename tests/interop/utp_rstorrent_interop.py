#!/usr/bin/env python3
"""Run both controlled RSTorrent/libtorrent uTP loopback roles."""

from __future__ import annotations

import gc
import json
import os
import queue
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, TextIO

import libtorrent as lt

from utp_reference_oracle import (
    MAX_DIAGNOSTICS,
    PAYLOAD_NAME,
    PAYLOAD_SIZE,
    PIECE_SIZE,
    POLL_SECONDS,
    SCENARIO_TIMEOUT_SECONDS,
    add_torrent,
    collect_alerts,
    create_fixture,
    create_session,
    hash_file,
    peer_addresses,
    stats_snapshot,
    wait_until_ready,
)


ROOT = Path(__file__).resolve().parents[2]
ROLE_BINARY_NAME = "rstorrent-utp-interop"
PROCESS_CLEANUP_SECONDS = 2.0


class InteropFailure(RuntimeError):
    pass


@dataclass
class OutputPump:
    stream: TextIO
    lines: queue.Queue[str | None] = field(default_factory=queue.Queue)
    captured: list[str] = field(default_factory=list)

    def start(self) -> threading.Thread:
        thread = threading.Thread(target=self._run, daemon=True)
        thread.start()
        return thread

    def _run(self) -> None:
        try:
            for line in self.stream:
                text = line.rstrip("\n")
                self.captured.append(text)
                del self.captured[:-MAX_DIAGNOSTICS]
                self.lines.put(text)
        finally:
            self.lines.put(None)


@dataclass
class RoleProcess:
    process: subprocess.Popen[str]
    stdout: OutputPump
    stderr: OutputPump
    threads: tuple[threading.Thread, threading.Thread]

    @classmethod
    def start(cls, binary: Path, arguments: list[str]) -> RoleProcess:
        process = subprocess.Popen(
            [str(binary), *arguments],
            cwd=ROOT,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        if process.stdout is None or process.stderr is None:
            raise InteropFailure("failed to capture role process output")
        stdout = OutputPump(process.stdout)
        stderr = OutputPump(process.stderr)
        return cls(process, stdout, stderr, (stdout.start(), stderr.start()))

    def read_event(self, deadline: float) -> dict[str, Any]:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise InteropFailure("role process event exceeded the scenario deadline")
        try:
            line = self.stdout.lines.get(timeout=remaining)
        except queue.Empty as error:
            raise InteropFailure(
                "role process produced no event before the scenario deadline"
            ) from error
        if line is None:
            raise InteropFailure(
                f"role process stopped before its next event: {self.stderr.captured}"
            )
        try:
            event = json.loads(line)
        except json.JSONDecodeError as error:
            raise InteropFailure(f"role process emitted invalid JSON: {line}") from error
        if not isinstance(event, dict):
            raise InteropFailure(f"role process emitted a non-object event: {event}")
        return event

    def send_stop(self) -> None:
        if self.process.stdin is None:
            raise InteropFailure("role process stdin is unavailable")
        self.process.stdin.write("stop\n")
        self.process.stdin.flush()

    def wait_success(self, deadline: float) -> None:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise InteropFailure("role process exceeded the scenario deadline")
        try:
            return_code = self.process.wait(timeout=remaining)
        except subprocess.TimeoutExpired as error:
            raise InteropFailure("role process exceeded the scenario deadline") from error
        for thread in self.threads:
            thread.join(timeout=PROCESS_CLEANUP_SECONDS)
        if return_code != 0:
            raise InteropFailure(
                f"role process exited {return_code}: {self.stderr.captured}"
            )

    def cleanup(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=PROCESS_CLEANUP_SECONDS)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=PROCESS_CLEANUP_SECONDS)
        if self.process.stdin is not None:
            self.process.stdin.close()
        if self.process.stdout is not None:
            self.process.stdout.close()
        if self.process.stderr is not None:
            self.process.stderr.close()


def build_role_binary() -> Path:
    subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "rstorrent-engine",
            "--bin",
            ROLE_BINARY_NAME,
        ],
        cwd=ROOT,
        check=True,
    )
    metadata = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    target = Path(json.loads(metadata.stdout)["target_directory"])
    suffix = ".exe" if os.name == "nt" else ""
    binary = target / "debug" / f"{ROLE_BINARY_NAME}{suffix}"
    if not binary.is_file():
        raise InteropFailure(f"built role binary is missing: {binary}")
    return binary


def validate_ready(event: dict[str, Any], role: str) -> tuple[str, int]:
    if event.get("event") != "ready" or event.get("role") != role:
        raise InteropFailure(f"unexpected {role} ready event: {event}")
    listen = event.get("listen")
    if not isinstance(listen, str):
        raise InteropFailure(f"{role} ready event has no listener")
    host, separator, port_text = listen.rpartition(":")
    if separator != ":" or host != "127.0.0.1":
        raise InteropFailure(f"{role} bound a non-loopback endpoint: {listen}")
    port = int(port_text)
    if not 1 <= port <= 65535:
        raise InteropFailure(f"{role} bound an invalid port: {listen}")
    return host, port


def validate_complete(event: dict[str, Any], role: str, expected_sha1: str) -> None:
    if event.get("event") != "complete" or event.get("role") != role:
        raise InteropFailure(f"unexpected {role} completion event: {event}")
    payload = event.get("payload", {})
    if payload.get("bytes") != PAYLOAD_SIZE or payload.get("pieces") != 33:
        raise InteropFailure(f"{role} reported unexpected payload geometry: {payload}")
    if role == "leecher" and payload.get("sha1") != expected_sha1:
        raise InteropFailure(f"{role} reported the wrong payload hash")
    resources = event.get("resources", {})
    terminal_utp = resources.get("terminal_utp", {})
    terminal_udp = resources.get("terminal_udp", {})
    if terminal_utp.get("active_connections") != 0:
        raise InteropFailure(f"{role} retained a terminal uTP connection")
    if terminal_utp.get("incoming_half_open") != 0:
        raise InteropFailure(f"{role} retained a terminal half-open uTP connection")
    if terminal_utp.get("worker_panics") != 0:
        raise InteropFailure(f"{role} observed a uTP worker panic")
    if terminal_udp.get("tasks") != 0:
        raise InteropFailure(f"{role} retained a terminal session UDP task")
    if terminal_udp.get("dht_queued") != 0 or terminal_udp.get("utp_queued") != 0:
        raise InteropFailure(f"{role} retained terminal session UDP datagrams")
    live_utp = resources.get("live_utp", {})
    live_udp = resources.get("live_udp", {})
    if live_utp.get("connection_high_water") != 1:
        raise InteropFailure(f"{role} did not observe exactly one uTP connection")
    if live_utp.get("datagrams_sent", 0) <= 0:
        raise InteropFailure(f"{role} sent no uTP datagrams")
    if live_udp.get("utp_datagrams_classified", 0) <= 0:
        raise InteropFailure(f"{role} received no uTP datagrams")
    for key in (
        "malformed_datagrams",
        "unknown_connection_datagrams",
        "stale_generation_datagrams",
        "connection_datagrams_dropped",
    ):
        if live_utp.get(key) != 0:
            raise InteropFailure(f"{role} reported nonzero {key}: {live_utp}")
    if live_udp.get("datagrams_dropped") != 0:
        raise InteropFailure(f"{role} dropped session UDP datagrams")


def validate_libtorrent_stats(role: str, snapshot: dict[str, int]) -> None:
    if snapshot["peer.num_tcp_peers"] != 0:
        raise InteropFailure(f"libtorrent {role} observed a TCP peer")
    if snapshot["utp.utp_packets_in"] <= 0:
        raise InteropFailure(f"libtorrent {role} received no uTP packets")
    if snapshot["utp.utp_packets_out"] <= 0:
        raise InteropFailure(f"libtorrent {role} sent no uTP packets")


def remove_session_torrent(
    session: lt.session | None, handle: lt.torrent_handle | None
) -> None:
    if session is not None and handle is not None and handle.is_valid():
        session.remove_torrent(handle)
    if session is not None:
        session.pause()


def run_rstorrent_leecher(binary: Path, root: Path) -> dict[str, Any]:
    started = time.monotonic()
    deadline = started + SCENARIO_TIMEOUT_SECONDS
    diagnostics: list[str] = []
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    role: RoleProcess | None = None
    root.mkdir()
    torrent_info, seed_root, expected_sha1 = create_fixture(root)
    output = root / "rstorrent-leech" / PAYLOAD_NAME
    try:
        session = create_session()
        handle = add_torrent(session, torrent_info, seed_root, seed=True)
        port = wait_until_ready(
            session,
            handle,
            seed=True,
            deadline=deadline,
            diagnostics=diagnostics,
        )
        role = RoleProcess.start(
            binary,
            [
                "leecher",
                "--metainfo",
                str(root / "forced-utp.torrent"),
                "--peer",
                f"127.0.0.1:{port}",
                "--output",
                str(output),
            ],
        )
        validate_ready(role.read_event(deadline), "leecher")
        peer_high_water = 0
        observed_addresses: set[str] = set()
        while time.monotonic() < deadline and role.process.poll() is None:
            collect_alerts(session, diagnostics)
            peers = peer_addresses(handle)
            peer_high_water = max(peer_high_water, len(peers))
            observed_addresses.update(peers)
            time.sleep(POLL_SECONDS)
        if role.process.poll() is None:
            raise InteropFailure("RSTorrent leecher exceeded 30 seconds")
        complete = role.read_event(deadline)
        role.wait_success(deadline)
        validate_complete(complete, "leecher", expected_sha1)
        if not output.is_file() or output.stat().st_size != PAYLOAD_SIZE:
            raise InteropFailure("RSTorrent leecher output is missing or has the wrong size")
        if hash_file(output) != expected_sha1:
            raise InteropFailure("RSTorrent leecher output hash differs from the seed")
        stats = stats_snapshot(session, diagnostics, deadline)
        validate_libtorrent_stats("seed", stats)
        if peer_high_water != 1 or observed_addresses != {"127.0.0.1"}:
            raise InteropFailure(
                "libtorrent seed did not observe exactly one loopback peer: "
                f"high_water={peer_high_water} addresses={sorted(observed_addresses)}"
            )
        return {
            "role": "rstorrent-leecher",
            "payload_sha1": expected_sha1,
            "peer_high_water": peer_high_water,
            "peer_addresses": sorted(observed_addresses),
            "libtorrent_stats": stats,
            "rstorrent": complete,
            "seconds": round(time.monotonic() - started, 6),
            "diagnostics": diagnostics,
        }
    finally:
        if role is not None:
            role.cleanup()
        remove_session_torrent(session, handle)
        handle = None
        session = None
        gc.collect()


def run_rstorrent_seed(binary: Path, root: Path) -> dict[str, Any]:
    started = time.monotonic()
    deadline = started + SCENARIO_TIMEOUT_SECONDS
    diagnostics: list[str] = []
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    role: RoleProcess | None = None
    root.mkdir()
    torrent_info, seed_root, expected_sha1 = create_fixture(root)
    leech_root = root / "libtorrent-leech"
    leech_root.mkdir()
    try:
        role = RoleProcess.start(
            binary,
            [
                "seed",
                "--metainfo",
                str(root / "forced-utp.torrent"),
                "--storage-root",
                str(seed_root),
            ],
        )
        _, port = validate_ready(role.read_event(deadline), "seed")
        session = create_session()
        handle = add_torrent(session, torrent_info, leech_root, seed=False)
        wait_until_ready(
            session,
            handle,
            seed=False,
            deadline=deadline,
            diagnostics=diagnostics,
        )
        handle.connect_peer(("127.0.0.1", port))
        peer_high_water = 0
        observed_addresses: set[str] = set()
        while time.monotonic() < deadline:
            if role.process.poll() is not None:
                raise InteropFailure(
                    f"RSTorrent seed stopped during transfer: {role.stderr.captured}"
                )
            collect_alerts(session, diagnostics)
            peers = peer_addresses(handle)
            peer_high_water = max(peer_high_water, len(peers))
            observed_addresses.update(peers)
            status = handle.status()
            if status.errc.value() != 0:
                raise InteropFailure(f"libtorrent leecher failed: {status.errc.message()}")
            if status.is_seeding:
                break
            time.sleep(POLL_SECONDS)
        else:
            raise InteropFailure("RSTorrent seed transfer exceeded 30 seconds")

        downloaded = leech_root / PAYLOAD_NAME
        if not downloaded.is_file() or downloaded.stat().st_size != PAYLOAD_SIZE:
            raise InteropFailure("libtorrent leecher output is missing or has the wrong size")
        if hash_file(downloaded) != expected_sha1:
            raise InteropFailure("libtorrent leecher output hash differs from the seed")
        stats = stats_snapshot(session, diagnostics, deadline)
        validate_libtorrent_stats("leecher", stats)
        if peer_high_water != 1 or observed_addresses != {"127.0.0.1"}:
            raise InteropFailure(
                "libtorrent leecher did not observe exactly one loopback peer: "
                f"high_water={peer_high_water} addresses={sorted(observed_addresses)}"
            )
        role.send_stop()
        complete = role.read_event(deadline)
        role.wait_success(deadline)
        validate_complete(complete, "seed", expected_sha1)
        peer_evidence = complete.get("peer_evidence", {})
        if (
            peer_evidence.get("connection_high_water") != 1
            or peer_evidence.get("utp_high_water") != 1
            or peer_evidence.get("tcp_high_water") != 0
        ):
            raise InteropFailure(
                f"RSTorrent seed reported unexpected peer evidence: {peer_evidence}"
            )
        terminal_incoming = complete.get("resources", {}).get(
            "terminal_incoming", {}
        )
        if (
            terminal_incoming.get("pending") != 0
            or terminal_incoming.get("established") != 0
            or terminal_incoming.get("connections") != 0
            or terminal_incoming.get("registrations") != 0
        ):
            raise InteropFailure(
                "RSTorrent seed retained terminal incoming-peer ownership"
            )
        return {
            "role": "rstorrent-seed",
            "payload_sha1": expected_sha1,
            "peer_high_water": peer_high_water,
            "peer_addresses": sorted(observed_addresses),
            "libtorrent_stats": stats,
            "rstorrent": complete,
            "seconds": round(time.monotonic() - started, 6),
            "diagnostics": diagnostics,
        }
    finally:
        if role is not None:
            role.cleanup()
        remove_session_torrent(session, handle)
        handle = None
        session = None
        gc.collect()


def run() -> dict[str, Any]:
    binary = build_role_binary()
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="rstorrent-utp-interop-") as temporary:
        root = Path(temporary)
        leecher = run_rstorrent_leecher(binary, root / "rstorrent-leecher")
        seed = run_rstorrent_seed(binary, root / "rstorrent-seed")
    return {
        "schema_version": 1,
        "oracle": "rstorrent-libtorrent-forced-utp-loopback",
        "libtorrent_version": lt.__version__,
        "transport": {
            "tcp_incoming": False,
            "tcp_outgoing": False,
            "mse": False,
            "dht": False,
            "lsd": False,
            "natpmp": False,
            "upnp": False,
            "loopback_only": True,
        },
        "payload": {
            "bytes": PAYLOAD_SIZE,
            "piece_bytes": PIECE_SIZE,
            "sha1": leecher["payload_sha1"],
        },
        "scenarios": [leecher, seed],
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
    except (InteropFailure, subprocess.CalledProcessError) as error:
        print(f"RSTorrent uTP interoperability failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
