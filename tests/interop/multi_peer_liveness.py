#!/usr/bin/env python3
"""Complete through libtorrent while a connected scripted peer stays choked."""

from __future__ import annotations

import gc
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

import libtorrent as lt

from first_verified_piece import (
    DEFAULT_PAYLOAD_ALLOWANCE,
    ScenarioConfig,
    ScenarioFailure,
    add_seed,
    build_diagnostic,
    compare_payloads,
    create_fixture,
    create_session,
    parse_diagnostic,
    wait_for_listener,
)


MIXED_CONFIG = ScenarioConfig(
    name="mixed-multi-peer",
    payload_size=1024 * 1024,
    piece_size=64 * 1024,
    payload_allowance=DEFAULT_PAYLOAD_ALLOWANCE,
    diagnostic_timeout_seconds=15,
    process_timeout_seconds=20,
)
MAX_FRAME_SIZE = 1024 * 1024
PEER_ID = b"-RS-ADVERSE-00000000"


def bencode(value: int | bytes | dict[bytes, object]) -> bytes:
    if isinstance(value, int):
        return b"i" + str(value).encode() + b"e"
    if isinstance(value, bytes):
        return str(len(value)).encode() + b":" + value
    if isinstance(value, dict):
        encoded = bytearray(b"d")
        for key in sorted(value):
            encoded.extend(bencode(key))
            encoded.extend(bencode(value[key]))
        encoded.extend(b"e")
        return bytes(encoded)
    raise TypeError(f"unsupported bencode value {type(value)!r}")


class AdversePeer:
    """Serve valid metadata, advertise the piece, and never unchoke."""

    def __init__(
        self,
        info_hash: bytes,
        info: bytes,
        piece_count: int,
        advertised_pieces: set[int] | None = None,
    ) -> None:
        self.info_hash = info_hash
        self.info = info
        self.piece_count = piece_count
        self.advertised_pieces = advertised_pieces
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(4)
        self.listener.settimeout(0.1)
        self.address = f"127.0.0.1:{self.listener.getsockname()[1]}"
        self.stop_requested = threading.Event()
        self.started = threading.Event()
        self.content_interested = threading.Event()
        self.thread = threading.Thread(target=self._run, name="adverse-peer")
        self.active: socket.socket | None = None
        self.error: BaseException | None = None
        self.connections = 0
        self.metadata_requests = 0
        self.interested_messages = 0

    def start(self) -> None:
        self.thread.start()
        if not self.started.wait(1):
            raise ScenarioFailure("scripted adverse peer did not start")

    def shutdown(self) -> None:
        self.stop_requested.set()
        try:
            self.listener.close()
        except OSError:
            pass
        if self.active is not None:
            try:
                self.active.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        self.thread.join(2)
        if self.thread.is_alive():
            raise ScenarioFailure("scripted adverse peer did not terminate")
        if self.error is not None:
            raise ScenarioFailure(f"scripted adverse peer failed: {self.error}")

    def _run(self) -> None:
        self.started.set()
        try:
            while not self.stop_requested.is_set():
                try:
                    connection, _ = self.listener.accept()
                except TimeoutError:
                    continue
                except OSError:
                    if self.stop_requested.is_set():
                        break
                    raise
                self.active = connection
                self.connections += 1
                try:
                    self._serve(connection)
                except (ConnectionError, TimeoutError, OSError):
                    if not self.stop_requested.is_set():
                        continue
                finally:
                    self.active = None
                    connection.close()
        except BaseException as error:
            self.error = error

    def _serve(self, connection: socket.socket) -> None:
        connection.settimeout(0.1)
        handshake = self._receive_exact(connection, 68)
        if handshake[0] != 19 or handshake[1:20] != b"BitTorrent protocol":
            raise ScenarioFailure("scripted peer received an invalid handshake")
        if handshake[28:48] != self.info_hash:
            raise ScenarioFailure("scripted peer received the wrong info hash")
        supports_extensions = bool(handshake[25] & 0x10)
        reserved = bytearray(8)
        reserved[5] = 0x10
        connection.sendall(
            bytes([19])
            + b"BitTorrent protocol"
            + reserved
            + self.info_hash
            + PEER_ID
        )
        bitfield = bytearray((self.piece_count + 7) // 8)
        advertised = (
            range(self.piece_count)
            if self.advertised_pieces is None
            else self.advertised_pieces
        )
        for piece in advertised:
            if piece < 0 or piece >= self.piece_count:
                raise ScenarioFailure(f"scripted peer piece {piece} is out of range")
            bitfield[piece // 8] |= 1 << (7 - piece % 8)
        self._send_frame(connection, 5, bytes(bitfield))

        if supports_extensions:
            while True:
                message_id, payload = self._receive_frame(connection)
                if message_id == 20 and payload[:1] == b"\x00":
                    break
            extension_handshake = bencode(
                {
                    b"m": {b"ut_metadata": 1},
                    b"metadata_size": len(self.info),
                }
            )
            self._send_frame(connection, 20, b"\x00" + extension_handshake)
            while True:
                message_id, payload = self._receive_frame(connection)
                if message_id == 20 and payload[:1] == b"\x01":
                    self.metadata_requests += 1
                    metadata_header = bencode(
                        {
                            b"msg_type": 1,
                            b"piece": 0,
                            b"total_size": len(self.info),
                        }
                    )
                    self._send_frame(connection, 20, b"\x01" + metadata_header + self.info)
                    break

        while not self.stop_requested.is_set():
            message_id, _ = self._receive_frame(connection)
            if message_id == 2:
                self.interested_messages += 1
                self.content_interested.set()
            elif message_id == 6:
                raise ScenarioFailure("choked scripted peer received a block request")

    def _receive_exact(self, connection: socket.socket, length: int) -> bytes:
        received = bytearray()
        while len(received) < length:
            if self.stop_requested.is_set():
                raise ConnectionAbortedError("scripted peer stopping")
            try:
                chunk = connection.recv(length - len(received))
            except TimeoutError:
                continue
            if not chunk:
                raise ConnectionResetError("peer closed")
            received.extend(chunk)
        return bytes(received)

    def _receive_frame(self, connection: socket.socket) -> tuple[int | None, bytes]:
        length = int.from_bytes(self._receive_exact(connection, 4), "big")
        if length == 0:
            return None, b""
        if length > MAX_FRAME_SIZE:
            raise ScenarioFailure(f"scripted peer received oversized frame {length}")
        frame = self._receive_exact(connection, length)
        return frame[0], frame[1:]

    @staticmethod
    def _send_frame(connection: socket.socket, message_id: int, payload: bytes) -> None:
        length = 1 + len(payload)
        connection.sendall(length.to_bytes(4, "big") + bytes([message_id]) + payload)


def run(repository: Path) -> None:
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-multi-peer-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    adverse: AdversePeer | None = None
    diagnostics: list[str] = []
    failure: BaseException | None = None
    cleanup_errors: list[str] = []
    try:
        binary = build_diagnostic(repository)
        torrent_path, seed_directory, payload_path, expected_hash, torrent_info = (
            create_fixture(run_path, MIXED_CONFIG, require_single_piece=False)
        )
        info = bytes(torrent_info.info_section())
        info_hash = bytes.fromhex(str(torrent_info.info_hashes().v1))
        adverse = AdversePeer(info_hash, info, torrent_info.num_pieces())
        adverse.start()

        session = create_session()
        session.apply_settings({"upload_rate_limit": 512 * 1024})
        libtorrent_port = wait_for_listener(session, diagnostics)
        handle = add_seed(session, torrent_info, seed_directory, diagnostics)
        magnet = (
            f"magnet:?xt=urn:btih:{info_hash.hex()}"
            f"&x.pe={adverse.address}&x.pe=127.0.0.1:{libtorrent_port}"
        )
        output_path = run_path / "downloaded.bin"
        command = [
            str(binary),
            "--magnet",
            magnet,
            "--output",
            str(output_path),
            "--timeout-seconds",
            str(MIXED_CONFIG.diagnostic_timeout_seconds),
            "--max-buffered-payload-bytes",
            str(MIXED_CONFIG.payload_allowance),
        ]
        completed = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=MIXED_CONFIG.process_timeout_seconds,
            check=False,
        )
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        if completed.returncode != 0:
            raise ScenarioFailure(
                f"RSTorrent exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
            )
        fields = parse_diagnostic(completed.stdout, MIXED_CONFIG)
        actual_hash = compare_payloads(payload_path, output_path)
        if actual_hash != expected_hash:
            raise ScenarioFailure("mixed swarm published unexpected payload bytes")
        piece_hashes = {
            bytes(torrent_info.hash_for_piece(index)).hex()
            for index in range(torrent_info.num_pieces())
        }
        if fields["sha1"] not in piece_hashes:
            raise ScenarioFailure("mixed swarm reported an unknown verified piece hash")
        if fields["info_hash"] != info_hash.hex():
            raise ScenarioFailure("mixed swarm reported the wrong info hash")
        payload_uploaded = handle.status().total_payload_upload
        if payload_uploaded < MIXED_CONFIG.payload_size:
            raise ScenarioFailure(
                "libtorrent did not account for the complete payload upload: "
                f"{payload_uploaded} < {MIXED_CONFIG.payload_size}"
            )
        if adverse.interested_messages == 0:
            raise ScenarioFailure("scripted choked peer never joined the content swarm")
        print(
            "scenario=mixed-multi-peer result=pass "
            f"bytes={MIXED_CONFIG.payload_size} sha1={actual_hash} "
            f"libtorrent_payload_upload={payload_uploaded} "
            f"adverse_connections={adverse.connections} "
            f"adverse_metadata_requests={adverse.metadata_requests} "
            f"adverse_interested={adverse.interested_messages}"
        )
        print(f"diagnostic={completed.stdout.strip()}")
    except BaseException as error:
        failure = error
    finally:
        if adverse is not None:
            try:
                adverse.shutdown()
            except BaseException as error:
                cleanup_errors.append(str(error))
        if session is not None:
            try:
                diagnostics.extend(alert.message() for alert in session.pop_alerts())
                if handle is not None and handle.is_valid():
                    session.remove_torrent(handle)
                session.pause()
            except BaseException as error:
                cleanup_errors.append(f"libtorrent cleanup failed: {error}")
        handle = None
        session = None
        gc.collect()
        try:
            shutil.rmtree(run_path)
        except OSError as error:
            cleanup_errors.append(f"temporary cleanup failed: {error}")

    if failure is not None or cleanup_errors:
        detail = str(failure) if failure is not None else "scenario cleanup failed"
        if cleanup_errors:
            detail += "; " + "; ".join(cleanup_errors)
        alerts = "\n".join(diagnostics[-100:]) or "(no libtorrent alerts)"
        raise ScenarioFailure(f"{detail}\nlibtorrent alerts:\n{alerts}") from failure


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    print(f"python_version={sys.version.split()[0]}")
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    try:
        run(repository)
    except (ScenarioFailure, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
