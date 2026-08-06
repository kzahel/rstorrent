#!/usr/bin/env python3
"""Prove the BEP 6 lifecycle against scripted and pinned peers."""

from __future__ import annotations

import gc
import hashlib
import json
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

import libtorrent as lt

from first_verified_piece import (
    PAYLOAD_NAME,
    ScenarioFailure,
    add_seed,
    compare_payloads,
    create_fixture,
    create_session,
    run_diagnostic,
    scenario_config,
    wait_for_listener,
)
from incoming_seeding import (
    Fixture as IncomingFixture,
    leech_with_libtorrent,
    parse_address,
    start_seed,
    stop_seed,
    terminate_process,
)


HANDSHAKE_LENGTH = 68
FAST_RESERVED_OFFSET = 27
FAST_RESERVED_BIT = 0x04
TRANSFER_TIMEOUT_SECONDS = 20


def recv_exact(stream: socket.socket, length: int) -> bytes:
    chunks = bytearray()
    while len(chunks) < length:
        chunk = stream.recv(length - len(chunks))
        if not chunk:
            raise EOFError("peer closed before the expected frame completed")
        chunks.extend(chunk)
    return bytes(chunks)


def frame(message_id: int, payload: bytes = b"") -> bytes:
    return struct.pack(">I", len(payload) + 1) + bytes([message_id]) + payload


@dataclass
class DirectionCapture:
    buffer: bytearray = field(default_factory=bytearray)
    handshake: bytes | None = None
    message_ids: list[int] = field(default_factory=list)

    def feed(self, data: bytes) -> None:
        self.buffer.extend(data)
        if self.handshake is None:
            if len(self.buffer) < HANDSHAKE_LENGTH:
                return
            self.handshake = bytes(self.buffer[:HANDSHAKE_LENGTH])
            del self.buffer[:HANDSHAKE_LENGTH]
        while len(self.buffer) >= 4:
            length = struct.unpack(">I", self.buffer[:4])[0]
            if len(self.buffer) < length + 4:
                return
            if length != 0:
                self.message_ids.append(self.buffer[4])
            del self.buffer[: length + 4]


class CapturingProxy:
    def __init__(self, target: tuple[str, int]) -> None:
        self.target = target
        self.client = DirectionCapture()
        self.server = DirectionCapture()
        self.failure: BaseException | None = None
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(1)
        self.port = self.listener.getsockname()[1]
        self.thread = threading.Thread(target=self._run, daemon=True)

    def start(self) -> None:
        self.thread.start()

    def join(self) -> None:
        self.thread.join(timeout=5)
        self.listener.close()
        if self.thread.is_alive():
            raise ScenarioFailure("capturing proxy did not terminate")
        if self.failure is not None:
            raise ScenarioFailure(f"capturing proxy failed: {self.failure}")

    def _run(self) -> None:
        try:
            self.listener.settimeout(10)
            for _ in range(3):
                client_capture = DirectionCapture()
                server_capture = DirectionCapture()
                downstream, _ = self.listener.accept()
                upstream = socket.create_connection(self.target, timeout=5)
                downstream.settimeout(None)
                upstream.settimeout(None)
                threads = (
                    threading.Thread(
                        target=self._relay,
                        args=(downstream, upstream, client_capture),
                        daemon=True,
                    ),
                    threading.Thread(
                        target=self._relay,
                        args=(upstream, downstream, server_capture),
                        daemon=True,
                    ),
                )
                for thread in threads:
                    thread.start()
                for thread in threads:
                    thread.join()
                downstream.close()
                upstream.close()
                if (
                    client_capture.handshake is not None
                    and client_capture.handshake.startswith(b"\x13BitTorrent protocol")
                    and server_capture.handshake is not None
                ):
                    self.client = client_capture
                    self.server = server_capture
                    return
            raise ScenarioFailure("capturing proxy did not observe a plaintext handshake")
        except BaseException as error:
            self.failure = error

    @staticmethod
    def _relay(
        source: socket.socket,
        destination: socket.socket,
        capture: DirectionCapture,
    ) -> None:
        try:
            while data := source.recv(64 * 1024):
                capture.feed(data)
                destination.sendall(data)
        except (ConnectionError, OSError):
            pass
        finally:
            try:
                destination.shutdown(socket.SHUT_WR)
            except OSError:
                pass


def assert_fast_capture(
    capture: DirectionCapture, expected_initial: int, label: str
) -> None:
    if capture.handshake is None or len(capture.handshake) != HANDSHAKE_LENGTH:
        raise ScenarioFailure(f"{label} handshake was not captured")
    if capture.handshake[FAST_RESERVED_OFFSET] & FAST_RESERVED_BIT == 0:
        raise ScenarioFailure(f"{label} did not advertise the Fast reserved bit")
    initial = next(
        (message_id for message_id in capture.message_ids if message_id in (5, 14, 15)),
        None,
    )
    if initial != expected_initial:
        raise ScenarioFailure(
            f"{label} initial availability was {initial}, expected {expected_initial}"
        )


def build_binaries(repository: Path) -> tuple[Path, Path]:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "rstorrent-engine",
            "--bin",
            "rstorrent-download-piece",
            "-p",
            "rstorrent-session",
            "--bin",
            "rstorrent-incoming-seed",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to build Fast diagnostics\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    return (
        repository / "target/debug/rstorrent-download-piece",
        repository / "target/debug/rstorrent-incoming-seed",
    )


def prove_rstorrent_leech_from_libtorrent(
    binary: Path, run_root: Path
) -> dict[str, object]:
    config = scenario_config(False)
    run_directory = run_root / "libtorrent-seed"
    run_directory.mkdir()
    torrent_path, seed_directory, _, expected_hash, torrent_info = create_fixture(
        run_directory, config
    )
    diagnostics: list[str] = []
    session = create_session()
    handle = None
    proxy = None
    try:
        port = wait_for_listener(session, diagnostics)
        handle = add_seed(session, torrent_info, seed_directory, diagnostics)
        proxy = CapturingProxy(("127.0.0.1", port))
        proxy.start()
        output = run_directory / "downloaded.bin"
        completed = run_diagnostic(binary, torrent_path, proxy.port, output, config)
        if completed.returncode != 0:
            raise ScenarioFailure(
                "RSTorrent Fast leecher failed\n"
                f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
            )
        actual_hash = compare_payloads(seed_directory / PAYLOAD_NAME, output)
        if actual_hash != expected_hash:
            raise ScenarioFailure("RSTorrent Fast leecher payload hash differs")
        proxy.join()
        assert_fast_capture(proxy.client, 15, "RSTorrent leecher")
        assert_fast_capture(proxy.server, 14, "libtorrent seed")
        return {
            "bytes": config.payload_size,
            "sha1": actual_hash,
            "rstorrent_initial": 15,
            "libtorrent_initial": 14,
        }
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def prove_libtorrent_leech_from_rstorrent(
    seed_binary: Path, run_root: Path
) -> dict[str, object]:
    config = scenario_config(False)
    run_directory = run_root / "rstorrent-seed"
    run_directory.mkdir()
    torrent_path, seed_directory, _, expected_hash, torrent_info = create_fixture(
        run_directory, config
    )
    fixture = IncomingFixture(
        name="fast",
        torrent_path=torrent_path,
        storage_root=seed_directory,
        profile_root=run_directory / "profile",
        torrent_info=torrent_info,
        info_hash=str(torrent_info.info_hashes().v1),
        files=((Path(PAYLOAD_NAME), expected_hash),),
        output_is_file=True,
    )
    process, ready = start_seed(seed_binary, fixture)
    proxy = CapturingProxy(parse_address(ready))
    proxy.start()
    output_root = run_directory / "libtorrent-output"
    output_root.mkdir()
    stopped = None
    try:
        leech_with_libtorrent(
            fixture,
            {"listen": f"127.0.0.1:{proxy.port}"},
            output_root,
        )
        stopped = stop_seed(process, config.payload_size, minimum_established=1)
        proxy.join()
        assert_fast_capture(proxy.client, 15, "libtorrent leecher")
        assert_fast_capture(proxy.server, 14, "RSTorrent seed")
        actual_hash = hashlib.sha1((output_root / PAYLOAD_NAME).read_bytes()).hexdigest()
        if actual_hash != expected_hash:
            raise ScenarioFailure("libtorrent Fast leecher payload hash differs")
        return {
            "bytes": config.payload_size,
            "sha1": actual_hash,
            "libtorrent_initial": 15,
            "rstorrent_initial": 14,
            "pre_shutdown_established": stopped["established_before_shutdown"],
            "pre_shutdown_reads": stopped["reads_before_shutdown"],
            "mapping_tasks_after_shutdown": stopped["mapping_tasks_after_shutdown"],
        }
    except BaseException as error:
        terminate_process(process)
        try:
            proxy.join()
        except BaseException:
            pass
        raise ScenarioFailure(
            f"{error}; client_handshake={proxy.client.handshake is not None} "
            f"server_handshake={proxy.server.handshake is not None} "
            f"client_prefix={proxy.client.handshake[:20] if proxy.client.handshake else None!r} "
            f"client_info_hash={proxy.client.handshake[28:48].hex() if proxy.client.handshake else None} "
            f"expected_info_hash={fixture.info_hash} "
            f"client messages={proxy.client.message_ids[:30]} "
            f"server messages={proxy.server.message_ids[:30]}"
        ) from error


@dataclass
class ScriptedResult:
    retry_seconds: float = 0.0
    request_counts: dict[tuple[int, int, int], int] = field(default_factory=dict)
    piece_bytes: int = 0
    client_initial: int | None = None
    failure: BaseException | None = None


def run_rejecting_seed(
    listener: socket.socket,
    info_hash: bytes,
    payload: bytes,
    result: ScriptedResult,
) -> None:
    try:
        stream, _ = listener.accept()
        handshake = recv_exact(stream, HANDSHAKE_LENGTH)
        if handshake[FAST_RESERVED_OFFSET] & FAST_RESERVED_BIT == 0:
            raise ScenarioFailure("scripted peer did not receive Fast negotiation")
        reserved = bytearray(8)
        reserved[7] = FAST_RESERVED_BIT
        stream.sendall(
            bytes([19])
            + b"BitTorrent protocol"
            + reserved
            + info_hash
            + b"-RSFAST-SCRIPTED-001"
        )
        stream.sendall(frame(14))
        stream.sendall(frame(1))
        rejected: tuple[int, int, int] | None = None
        rejected_at = 0.0
        while True:
            try:
                length = struct.unpack(">I", recv_exact(stream, 4))[0]
                if length == 0:
                    continue
                body = recv_exact(stream, length)
            except EOFError:
                break
            message_id = body[0]
            if message_id in (5, 14, 15) and result.client_initial is None:
                result.client_initial = message_id
            if message_id != 6:
                continue
            request = struct.unpack(">III", body[1:13])
            result.request_counts[request] = result.request_counts.get(request, 0) + 1
            if rejected is None:
                rejected = request
                rejected_at = time.monotonic()
                stream.sendall(frame(0))
                stream.sendall(frame(16, body[1:13]))
                stream.sendall(frame(1))
                continue
            if request == rejected and result.request_counts[request] == 2:
                result.retry_seconds = time.monotonic() - rejected_at
            index, begin, block_length = request
            if index != 0 or begin + block_length > len(payload):
                raise ScenarioFailure(f"scripted peer received invalid request {request}")
            block = payload[begin : begin + block_length]
            result.piece_bytes += len(block)
            stream.sendall(frame(7, struct.pack(">II", index, begin) + block))
        stream.close()
    except BaseException as error:
        result.failure = error
    finally:
        listener.close()


def prove_exact_reject_refill(binary: Path, run_root: Path) -> dict[str, object]:
    config = scenario_config(False)
    run_directory = run_root / "scripted-reject"
    run_directory.mkdir()
    torrent_path, seed_directory, _, expected_hash, torrent_info = create_fixture(
        run_directory, config
    )
    payload = (seed_directory / PAYLOAD_NAME).read_bytes()
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    result = ScriptedResult()
    server = threading.Thread(
        target=run_rejecting_seed,
        args=(
            listener,
            bytes.fromhex(str(torrent_info.info_hashes().v1)),
            payload,
            result,
        ),
        daemon=True,
    )
    server.start()
    output = run_directory / "downloaded.bin"
    completed = run_diagnostic(
        binary,
        torrent_path,
        listener.getsockname()[1],
        output,
        config,
    )
    server.join(timeout=5)
    if server.is_alive():
        raise ScenarioFailure("scripted rejecting seed did not terminate")
    if result.failure is not None:
        raise ScenarioFailure(f"scripted rejecting seed failed: {result.failure}")
    if completed.returncode != 0:
        raise ScenarioFailure(
            "RSTorrent reject lifecycle failed\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    actual_hash = compare_payloads(seed_directory / PAYLOAD_NAME, output)
    if actual_hash != expected_hash:
        raise ScenarioFailure("reject lifecycle payload hash differs")
    repeated = [count for count in result.request_counts.values() if count == 2]
    if repeated != [2] or any(count not in (1, 2) for count in result.request_counts.values()):
        raise ScenarioFailure(f"unexpected request terminal counts: {result.request_counts}")
    if result.retry_seconds <= 0 or result.retry_seconds >= 2:
        raise ScenarioFailure(
            f"rejected request refill took {result.retry_seconds:.3f}s"
        )
    if result.piece_bytes != len(payload):
        raise ScenarioFailure(
            f"scripted peer sent {result.piece_bytes} payload bytes, expected {len(payload)}"
        )
    if result.client_initial != 15:
        raise ScenarioFailure(
            f"RSTorrent scripted initial availability was {result.client_initial}"
        )
    return {
        "bytes": len(payload),
        "sha1": actual_hash,
        "unique_requests": len(result.request_counts),
        "rejected_request_count": 2,
        "retry_millis": round(result.retry_seconds * 1000, 3),
        "piece_bytes": result.piece_bytes,
    }


def run(repository: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="rstorrent-fast-extension-") as temporary:
        run_root = Path(temporary)
        binary, seed_binary = build_binaries(repository)
        evidence = {
            "python_version": sys.version.split()[0],
            "libtorrent_binding_version": lt.__version__,
            "libtorrent_native_version": lt.version,
            "rstorrent_leech": prove_rstorrent_leech_from_libtorrent(binary, run_root),
            "rstorrent_seed": prove_libtorrent_leech_from_rstorrent(seed_binary, run_root),
            "exact_reject": prove_exact_reject_refill(binary, run_root),
            "result": "pass",
        }
        print(json.dumps(evidence, sort_keys=True))


if __name__ == "__main__":
    try:
        run(Path(__file__).resolve().parents[2])
    except ScenarioFailure as error:
        print(f"Fast extension failed: {error}", file=sys.stderr)
        raise SystemExit(1)
