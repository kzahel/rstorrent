#!/usr/bin/env python3
"""Download a tracker-only magnet from a controlled libtorrent seed."""

from __future__ import annotations

import argparse
import gc
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import quote

import libtorrent as lt

from first_verified_piece import (
    DEFAULT_PAYLOAD_ALLOWANCE,
    ScenarioFailure,
    add_seed,
    build_diagnostic,
    compare_payloads,
    create_session,
    wait_for_listener,
)
from magnet_metadata import (
    METADATA_BLOCK_SIZE,
    METADATA_TIMEOUT_SECONDS,
    PROCESS_TIMEOUT_SECONDS,
    Fixture,
    create_fixture,
    parse_fields,
)


PROTOCOL_ID = 0x41727101980
CONNECT_ACTION = 0
ANNOUNCE_ACTION = 1
STARTED_EVENT = 2
CONNECTION_ID = 0x0102030405060708
UNKNOWN_MAGNET_LEFT = 16 * 1024
NUM_WANT = 200
ANNOUNCED_PORT = 6881
DIAGNOSTIC_PEER_ID = b"-RS0001-000000000000"
ANNOUNCE_FORMAT = "!QII20s20sQQQIIIiH"


@dataclass
class RunResult:
    ordinal: int
    elapsed_seconds: float
    info_hash: str
    metadata_size: int
    metadata_blocks: int
    payload_hash: str
    tracker_requests: int
    command_output: str
    cleanup_succeeded: bool = False


class OneShotUdpTracker:
    def __init__(
        self,
        info_hash: str,
        peer_port: int,
        *,
        response_delay_seconds: float = 0,
        seeders: int = 1,
        leechers: int = 1,
        expected_left: int = UNKNOWN_MAGNET_LEFT,
        expected_peer_id: bytes | None = DIAGNOSTIC_PEER_ID,
    ) -> None:
        self.info_hash = bytes.fromhex(info_hash)
        self.peer_port = peer_port
        self.response_delay_seconds = response_delay_seconds
        self.seeders = seeders
        self.leechers = leechers
        self.expected_left = expected_left
        self.expected_peer_id = expected_peer_id
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(("127.0.0.1", 0))
        self.socket.settimeout(10)
        self.port = self.socket.getsockname()[1]
        self.requests = 0
        self.failure: BaseException | None = None
        self.thread = threading.Thread(
            target=self._serve,
            name=f"rstorrent-udp-tracker-{self.port}",
        )

    def start(self) -> None:
        self.thread.start()

    def join(self) -> None:
        self.thread.join(timeout=12)
        if self.thread.is_alive():
            raise ScenarioFailure("controlled UDP tracker did not terminate")
        if self.failure is not None:
            raise ScenarioFailure(f"controlled UDP tracker failed: {self.failure}")
        if self.requests != 2:
            raise ScenarioFailure(
                f"controlled UDP tracker saw {self.requests} requests, expected 2"
            )

    def close(self) -> None:
        try:
            self.socket.close()
        except OSError:
            pass
        self.thread.join(timeout=2)

    def _serve(self) -> None:
        try:
            connect, client = self.socket.recvfrom(2048)
            self.requests += 1
            if len(connect) != 16:
                raise ScenarioFailure(
                    f"UDP connect request length is {len(connect)}, expected 16"
                )
            protocol_id, action, connect_transaction = struct.unpack("!QII", connect)
            if protocol_id != PROTOCOL_ID or action != CONNECT_ACTION:
                raise ScenarioFailure("UDP connect request has the wrong protocol or action")
            if connect_transaction == 0:
                raise ScenarioFailure("UDP connect transaction is zero")

            stale_connect = struct.pack(
                "!IIQ",
                CONNECT_ACTION,
                (connect_transaction + 1) & 0xFFFFFFFF,
                CONNECTION_ID,
            )
            self.socket.sendto(stale_connect, client)
            self.socket.sendto(
                struct.pack(
                    "!IIQ",
                    CONNECT_ACTION,
                    connect_transaction,
                    CONNECTION_ID,
                ),
                client,
            )

            announce, announce_client = self.socket.recvfrom(2048)
            self.requests += 1
            if announce_client != client:
                raise ScenarioFailure("UDP announce came from a different client endpoint")
            if len(announce) != struct.calcsize(ANNOUNCE_FORMAT):
                raise ScenarioFailure(
                    f"UDP announce request length is {len(announce)}, expected 98"
                )
            (
                connection_id,
                action,
                announce_transaction,
                info_hash,
                peer_id,
                downloaded,
                left,
                uploaded,
                event,
                announced_ip,
                key,
                num_want,
                listen_port,
            ) = struct.unpack(ANNOUNCE_FORMAT, announce)
            if connection_id != CONNECTION_ID or action != ANNOUNCE_ACTION:
                raise ScenarioFailure("UDP announce has the wrong connection or action")
            if announce_transaction == 0 or announce_transaction == connect_transaction:
                raise ScenarioFailure("UDP announce transaction was not renewed")
            if info_hash != self.info_hash:
                raise ScenarioFailure("UDP announce has the wrong info hash")
            if self.expected_peer_id is None:
                valid_peer_id = peer_id.startswith(b"-RS0001-")
            else:
                valid_peer_id = peer_id == self.expected_peer_id
            if not valid_peer_id:
                raise ScenarioFailure("UDP announce has the wrong peer ID")
            if (downloaded, left, uploaded) != (0, self.expected_left, 0):
                raise ScenarioFailure(
                    "UDP announce has unexpected transfer counters "
                    f"{(downloaded, left, uploaded)}, expected {(0, self.expected_left, 0)}"
                )
            if event != STARTED_EVENT or announced_ip != 0:
                raise ScenarioFailure("UDP announce has the wrong event or IP field")
            if key == 0 or num_want != NUM_WANT or listen_port != ANNOUNCED_PORT:
                raise ScenarioFailure("UDP announce has the wrong key, peer limit, or port")

            if self.response_delay_seconds > 0:
                time.sleep(self.response_delay_seconds)
            response = struct.pack(
                "!IIIII4sH",
                ANNOUNCE_ACTION,
                announce_transaction,
                1800,
                self.leechers,
                self.seeders,
                socket.inet_aton("127.0.0.1"),
                self.peer_port,
            )
            stale_response = bytearray(response)
            struct.pack_into(
                "!I",
                stale_response,
                4,
                (announce_transaction + 1) & 0xFFFFFFFF,
            )
            self.socket.sendto(stale_response, client)
            self.socket.sendto(response, client)
        except BaseException as error:
            self.failure = error
        finally:
            self.socket.close()


def tracker_magnet(info_hash: str, tracker_port: int) -> str:
    tracker = quote(f"udp://127.0.0.1:{tracker_port}/announce", safe="")
    return f"magnet:?xt=urn:btih:{info_hash}&tr={tracker}"


def run_rstorrent_leech(
    binary: Path,
    fixture: Fixture,
    ordinal: int,
    diagnostics: list[str],
) -> tuple[float, str, int]:
    session = create_session()
    handle: lt.torrent_handle | None = None
    tracker: OneShotUdpTracker | None = None
    started = time.monotonic()
    try:
        peer_port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            fixture.torrent_info,
            fixture.seed_directory,
            diagnostics,
        )
        tracker = OneShotUdpTracker(fixture.info_hash, peer_port)
        tracker.start()
        output_root = fixture.torrent_path.parent / f"tracker-output-{ordinal}"
        completed = subprocess.run(
            [
                str(binary),
                "--magnet",
                tracker_magnet(fixture.info_hash, tracker.port),
                "--output",
                str(output_root),
                "--timeout-seconds",
                str(METADATA_TIMEOUT_SECONDS),
                "--max-buffered-payload-bytes",
                str(DEFAULT_PAYLOAD_ALLOWANCE),
            ],
            capture_output=True,
            text=True,
            timeout=PROCESS_TIMEOUT_SECONDS,
            check=False,
        )
        tracker.join()
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        if completed.returncode != 0:
            raise ScenarioFailure(
                f"tracker magnet leech exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
        fields = parse_fields(completed.stdout)
        if fields.get("info_hash") != fixture.info_hash:
            raise ScenarioFailure("tracker magnet leech reported the wrong info hash")
        if fields.get("pieces") != "3/3":
            raise ScenarioFailure(
                f"tracker magnet leech reported pieces={fields.get('pieces')}, expected 3/3"
            )
        actual_hash = compare_payloads(
            fixture.payload_path,
            output_root / "payload.bin",
        )
        if actual_hash != fixture.payload_hash:
            raise ScenarioFailure("tracker magnet payload differs from libtorrent seed")
        return (
            time.monotonic() - started,
            completed.stdout.strip(),
            tracker.requests,
        )
    finally:
        if tracker is not None:
            tracker.close()
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def run_once(binary: Path, ordinal: int) -> RunResult:
    run_path = Path(tempfile.mkdtemp(prefix=f"rstorrent-udp-tracker-{ordinal}-"))
    diagnostics: list[str] = []
    failure: BaseException | None = None
    result: RunResult | None = None
    try:
        fixture = create_fixture(run_path)
        elapsed_seconds, output, tracker_requests = run_rstorrent_leech(
            binary,
            fixture,
            ordinal,
            diagnostics,
        )
        result = RunResult(
            ordinal=ordinal,
            elapsed_seconds=elapsed_seconds,
            info_hash=fixture.info_hash,
            metadata_size=len(fixture.info_bytes),
            metadata_blocks=(
                len(fixture.info_bytes) + METADATA_BLOCK_SIZE - 1
            )
            // METADATA_BLOCK_SIZE,
            payload_hash=fixture.payload_hash,
            tracker_requests=tracker_requests,
            command_output=output,
        )
    except BaseException as error:
        failure = error
    finally:
        try:
            shutil.rmtree(run_path)
            if run_path.exists():
                raise ScenarioFailure("run directory still exists after cleanup")
            if result is not None:
                result.cleanup_succeeded = True
        except BaseException as cleanup_error:
            failure = cleanup_error if failure is None else failure

    if failure is not None:
        diagnostic_text = "\n".join(diagnostics[-100:]) or "(no libtorrent alerts)"
        raise ScenarioFailure(
            f"UDP tracker magnet run {ordinal} failed: {failure}\n"
            f"libtorrent alerts:\n{diagnostic_text}"
        ) from failure
    if result is None or not result.cleanup_succeeded:
        raise ScenarioFailure(f"UDP tracker magnet run {ordinal} did not clean up")
    return result


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=1)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.runs < 1:
        raise ScenarioFailure("--runs must be positive")
    repository = Path(__file__).resolve().parents[2]
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    try:
        binary = build_diagnostic(repository)
        for ordinal in range(1, args.runs + 1):
            result = run_once(binary, ordinal)
            print(
                f"run={ordinal} metadata_size={result.metadata_size} "
                f"metadata_blocks={result.metadata_blocks} "
                f"info_hash={result.info_hash} payload_sha1={result.payload_hash} "
                f"tracker_requests={result.tracker_requests} "
                f"elapsed_seconds={result.elapsed_seconds:.3f} cleanup=ok"
            )
            print(f"leech_report={result.command_output}")
    except (ScenarioFailure, subprocess.TimeoutExpired) as error:
        print(f"interop failure: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
