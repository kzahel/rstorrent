#!/usr/bin/env python3
"""Standard-library BitTorrent peer-wire verifier for an off-LAN runner.

The JSON configuration is read from stdin so an orchestrator can stream this
source to an operator-selected host without installing packages or files.
"""

from __future__ import annotations

import hashlib
import json
import socket
import struct
import sys
import time
from typing import Any


BLOCK_SIZE = 16 * 1024
MAX_PAYLOAD_BYTES = 64 * 1024 * 1024
MAX_FRAME_BYTES = BLOCK_SIZE + 64
MAX_IN_FLIGHT = 16
MAX_MESSAGES = 100_000
PROTOCOL = b"BitTorrent protocol"


class VerificationFailure(RuntimeError):
    pass


def exact(socket_: socket.socket, length: int) -> bytes:
    chunks = bytearray()
    while len(chunks) < length:
        chunk = socket_.recv(length - len(chunks))
        if not chunk:
            raise VerificationFailure("peer closed before the expected bytes")
        chunks.extend(chunk)
    return bytes(chunks)


def frame(socket_: socket.socket) -> tuple[int | None, bytes]:
    length = struct.unpack(">I", exact(socket_, 4))[0]
    if length == 0:
        return None, b""
    if length > MAX_FRAME_BYTES:
        raise VerificationFailure("peer frame exceeds the verifier bound")
    payload = exact(socket_, length)
    return payload[0], payload[1:]


def send_message(socket_: socket.socket, message_id: int, payload: bytes = b"") -> None:
    socket_.sendall(struct.pack(">IB", len(payload) + 1, message_id) + payload)


def integer(config: dict[str, Any], field: str, minimum: int, maximum: int) -> int:
    value = config.get(field)
    if not isinstance(value, int) or isinstance(value, bool):
        raise VerificationFailure(f"{field} is not an integer")
    if not minimum <= value <= maximum:
        raise VerificationFailure(f"{field} is outside its bound")
    return value


def text(config: dict[str, Any], field: str, maximum: int) -> str:
    value = config.get(field)
    if not isinstance(value, str) or not value or len(value) > maximum:
        raise VerificationFailure(f"{field} is invalid")
    return value


def decode_hex(value: str, length: int, field: str) -> bytes:
    try:
        decoded = bytes.fromhex(value)
    except ValueError as error:
        raise VerificationFailure(f"{field} is not hexadecimal") from error
    if len(decoded) != length:
        raise VerificationFailure(f"{field} has the wrong length")
    return decoded


def verify(config: dict[str, Any]) -> dict[str, object]:
    host = text(config, "host", 64)
    port = integer(config, "port", 1, 65_535)
    info_hash = decode_hex(text(config, "info_hash", 40), 20, "info_hash")
    total_length = integer(config, "total_length", 1, MAX_PAYLOAD_BYTES)
    piece_length = integer(config, "piece_length", 1, 4 * 1024 * 1024)
    piece_hash_values = config.get("piece_hashes")
    if not isinstance(piece_hash_values, list) or not piece_hash_values:
        raise VerificationFailure("piece_hashes is not a nonempty list")
    piece_hashes = [
        decode_hex(value, 20, "piece hash")
        for value in piece_hash_values
        if isinstance(value, str)
    ]
    if len(piece_hashes) != len(piece_hash_values):
        raise VerificationFailure("piece_hashes contains a non-string value")
    expected_pieces = (total_length + piece_length - 1) // piece_length
    if len(piece_hashes) != expected_pieces:
        raise VerificationFailure("piece hash count does not match payload geometry")
    expected_sha256 = decode_hex(
        text(config, "payload_sha256", 64), 32, "payload_sha256"
    )
    hold_seconds = integer(config, "hold_seconds", 0, 10)

    peer_id = b"-RSUPNP-000000000001"
    if len(peer_id) != 20:
        raise AssertionError("fixed verifier peer ID must be 20 bytes")
    handshake = bytes([len(PROTOCOL)]) + PROTOCOL + bytes(8) + info_hash + peer_id
    payload = bytearray(total_length)
    requests = [
        (piece, begin, min(BLOCK_SIZE, min(piece_length, total_length - piece * piece_length) - begin))
        for piece in range(expected_pieces)
        for begin in range(0, min(piece_length, total_length - piece * piece_length), BLOCK_SIZE)
    ]
    pending: set[tuple[int, int, int]] = set()
    completed: set[tuple[int, int, int]] = set()
    request_index = 0
    unchoked = False
    messages = 0

    with socket.create_connection((host, port), timeout=10) as peer:
        peer.settimeout(15)
        peer.sendall(handshake)
        response = exact(peer, 68)
        if response[:20] != handshake[:20] or response[28:48] != info_hash:
            raise VerificationFailure("peer returned a mismatched handshake")
        send_message(peer, 2)
        while len(completed) != len(requests):
            while unchoked and request_index < len(requests) and len(pending) < MAX_IN_FLIGHT:
                request = requests[request_index]
                request_index += 1
                pending.add(request)
                send_message(peer, 6, struct.pack(">III", *request))
            message_id, body = frame(peer)
            messages += 1
            if messages > MAX_MESSAGES:
                raise VerificationFailure("peer message count exceeds the verifier bound")
            if message_id is None:
                continue
            if message_id == 0:
                unchoked = False
                continue
            if message_id == 1:
                unchoked = True
                continue
            if message_id != 7:
                continue
            if len(body) < 8:
                raise VerificationFailure("piece frame is truncated")
            piece, begin = struct.unpack(">II", body[:8])
            block = body[8:]
            request = (piece, begin, len(block))
            if request not in pending or request in completed:
                raise VerificationFailure("peer returned unsolicited or duplicate payload")
            pending.remove(request)
            completed.add(request)
            offset = piece * piece_length + begin
            payload[offset : offset + len(block)] = block

        for piece, expected_hash in enumerate(piece_hashes):
            start = piece * piece_length
            end = min(start + piece_length, total_length)
            if hashlib.sha1(payload[start:end]).digest() != expected_hash:
                raise VerificationFailure("downloaded piece hash differs")
        actual_sha256 = hashlib.sha256(payload).digest()
        if actual_sha256 != expected_sha256:
            raise VerificationFailure("whole payload digest differs")
        if hold_seconds:
            time.sleep(hold_seconds)

    return {
        "status": "verified",
        "bytes": total_length,
        "pieces": len(piece_hashes),
        "sha256": actual_sha256.hex(),
    }


def verify_unreachable(config: dict[str, Any]) -> dict[str, object]:
    host = text(config, "host", 64)
    port = integer(config, "port", 1, 65_535)
    try:
        with socket.create_connection((host, port), timeout=5):
            pass
    except OSError:
        return {"status": "unreachable"}
    raise VerificationFailure("mapped endpoint still accepts TCP after cleanup")


def main() -> int:
    try:
        config = json.load(sys.stdin)
        if not isinstance(config, dict):
            raise VerificationFailure("configuration is not an object")
        result = (
            verify_unreachable(config)
            if config.get("expect_connect_failure") is True
            else verify(config)
        )
        print(json.dumps(result, separators=(",", ":")), flush=True)
        return 0
    except (OSError, ValueError, VerificationFailure) as error:
        print(f"off-LAN peer verification failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
