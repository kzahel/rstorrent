#!/usr/bin/env python3
"""Prove incomplete-torrent duplex exchange against pinned libtorrent."""

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

from first_verified_piece import ScenarioFailure, add_seed, create_session, wait_for_listener
from incoming_seeding import parse_address, read_json_line, seed_snapshot, terminate_process
from mse_peer_encryption import TcpProxy, configure_encryption


PIECE_SIZE = 32 * 1024
BLOCK_SIZE = 16 * 1024
FAST_RESERVED_OFFSET = 27
FAST_RESERVED_BIT = 0x04
HANDSHAKE_LENGTH = 68
TRANSFER_TIMEOUT_SECONDS = 30
PROCESS_TIMEOUT_SECONDS = 45
RST_INITIAL = (0, 2)
REMOTE_INITIAL = (1, 3)


@dataclass(frozen=True)
class FileSpec:
    path: Path
    length: int
    padding: bool = False


FILES = (
    FileSpec(Path("wanted-a.bin"), 20_000),
    FileSpec(Path("skip-a.bin"), 8_000),
    FileSpec(Path(".pad/4768"), 4_768, True),
    FileSpec(Path("wanted-b.bin"), 20_000),
    FileSpec(Path("skip-b.bin"), 8_000),
    FileSpec(Path("wanted-c.bin"), 4_768),
    FileSpec(Path("wanted-d.bin"), 12_000),
    FileSpec(Path("skip-c.bin"), 12_000),
    FileSpec(Path(".pad/8768"), 8_768, True),
    FileSpec(Path("wanted-e.bin"), 20_000),
    FileSpec(Path("skip-d.bin"), 8_000),
    FileSpec(Path("wanted-f.bin"), 4_768),
)
SKIP_FILES = tuple(index for index, spec in enumerate(FILES) if "skip-" in spec.path.name)


@dataclass(frozen=True)
class Fixture:
    root: Path
    torrent_path: Path
    torrent_info: lt.torrent_info
    flat_payload: Path
    source_root: Path
    expected_hashes: dict[Path, str]
    info_hash: str


def deterministic_bytes(offset: int, length: int) -> bytes:
    return bytes(
        ((index * 73) ^ (index >> 3) ^ (index * index >> 11) ^ 0xA5) & 0xFF
        for index in range(offset, offset + length)
    )


def create_fixture(root: Path) -> Fixture:
    source_root = root / "source"
    torrent_root = source_root / "duplex-tree"
    torrent_root.mkdir(parents=True)
    storage = lt.file_storage()
    flat = bytearray()
    hashes: dict[Path, str] = {}
    offset = 0
    for spec in FILES:
        flags = lt.file_storage.flag_pad_file if spec.padding else 0
        storage.add_file(f"duplex-tree/{spec.path.as_posix()}", spec.length, flags)
        if spec.padding:
            payload = bytes(spec.length)
        else:
            payload = deterministic_bytes(offset, spec.length)
            path = torrent_root / spec.path
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(payload)
            hashes[spec.path] = hashlib.sha1(payload).hexdigest()
        flat.extend(payload)
        offset += spec.length
    if len(flat) != 4 * PIECE_SIZE:
        raise ScenarioFailure(f"duplex fixture has unexpected length {len(flat)}")
    creator = lt.create_torrent(
        storage,
        piece_size=PIECE_SIZE,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(source_root))
    torrent_path = root / "duplex.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    info = lt.torrent_info(str(torrent_path))
    if info.num_pieces() != 4 or info.total_size() != len(flat):
        raise ScenarioFailure("duplex fixture geometry changed")
    flat_path = root / "logical-payload.bin"
    flat_path.write_bytes(flat)
    return Fixture(
        root=root,
        torrent_path=torrent_path,
        torrent_info=info,
        flat_payload=flat_path,
        source_root=source_root,
        expected_hashes=hashes,
        info_hash=str(info.info_hashes().v1),
    )


def write_libtorrent_partial(
    fixture: Fixture, output_root: Path, retained: tuple[int, ...]
) -> None:
    output_root.mkdir(parents=True)
    flat = bytearray(fixture.flat_payload.read_bytes())
    retained_set = set(retained)
    for piece in range(fixture.torrent_info.num_pieces()):
        if piece not in retained_set:
            begin = piece * PIECE_SIZE
            flat[begin : begin + PIECE_SIZE] = bytes(PIECE_SIZE)
    offset = 0
    for spec in FILES:
        payload = flat[offset : offset + spec.length]
        offset += spec.length
        if spec.padding:
            continue
        path = output_root / "duplex-tree" / spec.path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)


def add_partial(
    session: lt.session,
    fixture: Fixture,
    output_root: Path,
    retained: tuple[int, ...] = REMOTE_INITIAL,
) -> lt.torrent_handle:
    write_libtorrent_partial(fixture, output_root, retained)
    parameters = lt.add_torrent_params()
    parameters.ti = fixture.torrent_info
    parameters.save_path = str(output_root)
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    parameters.flags |= lt.torrent_flags.disable_dht
    parameters.flags |= lt.torrent_flags.disable_lsd
    parameters.flags |= lt.torrent_flags.disable_pex
    handle = session.add_torrent(parameters)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        status = handle.status()
        if status.errc.value() != 0:
            raise ScenarioFailure(f"libtorrent partial failed: {status.errc.message()}")
        if status.num_pieces == len(retained):
            actual = tuple(
                piece
                for piece in range(fixture.torrent_info.num_pieces())
                if handle.have_piece(piece)
            )
            if actual != retained:
                raise ScenarioFailure(
                    f"libtorrent retained pieces {actual}, expected {retained}"
                )
            logical = bytearray()
            for spec in FILES:
                if spec.padding:
                    logical.extend(bytes(spec.length))
                else:
                    logical.extend(
                        (output_root / "duplex-tree" / spec.path).read_bytes()
                    )
            for piece in retained:
                payload = logical[piece * PIECE_SIZE : (piece + 1) * PIECE_SIZE]
                expected = bytes(fixture.torrent_info.hash_for_piece(piece))
                if hashlib.sha1(payload).digest() != expected:
                    raise ScenarioFailure(
                        f"libtorrent retained piece {piece} differs on independent readback"
                    )
            return handle
        time.sleep(0.05)
    raise ScenarioFailure(
        f"libtorrent verified {handle.status().num_pieces} partial pieces, "
        f"expected {len(retained)}"
    )


@dataclass
class DirectionEvidence:
    handshake: bytes | None = None
    message_ids: list[int] = field(default_factory=list)
    pieces: list[tuple[int, int, int, int]] = field(default_factory=list)


class PlaintextDuplexProxy:
    def __init__(
        self,
        target: tuple[str, int],
        *,
        clear_fast: bool = False,
        hold_until_release: bool = False,
        piece_delay_seconds: float = 0.25,
    ) -> None:
        self.target = target
        self.clear_fast = clear_fast
        self.piece_delay_seconds = piece_delay_seconds
        self.release = threading.Event()
        if not hold_until_release:
            self.release.set()
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(1)
        self.listener.settimeout(15)
        self.endpoint = ("127.0.0.1", int(self.listener.getsockname()[1]))
        self.client = DirectionEvidence()
        self.upstream = DirectionEvidence()
        self._piece_sequence = 0
        self._piece_sequence_lock = threading.Lock()
        self._sockets: list[socket.socket] = []
        self._failure: BaseException | None = None
        self._closing = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def allow_connection(self) -> None:
        self.release.set()

    def _run(self) -> None:
        try:
            downstream, _ = self.listener.accept()
            self._sockets.append(downstream)
            if not self.release.wait(15):
                raise ScenarioFailure("duplex proxy connection release timed out")
            upstream = socket.create_connection(self.target, timeout=5)
            self._sockets.append(upstream)
            workers = (
                threading.Thread(
                    target=self._relay_guard,
                    args=(downstream, upstream, self.client),
                    daemon=True,
                ),
                threading.Thread(
                    target=self._relay_guard,
                    args=(upstream, downstream, self.upstream),
                    daemon=True,
                ),
            )
            for worker in workers:
                worker.start()
            for worker in workers:
                worker.join()
        except BaseException as error:
            if not (self._closing.is_set() and isinstance(error, OSError)):
                self._failure = error
        finally:
            for stream in self._sockets:
                try:
                    stream.close()
                except OSError:
                    pass

    def _relay_guard(
        self,
        source: socket.socket,
        destination: socket.socket,
        evidence: DirectionEvidence,
    ) -> None:
        try:
            self._relay(source, destination, evidence)
        except (ConnectionError, OSError) as error:
            if evidence.handshake is None and not self._closing.is_set():
                self._failure = error
        except EOFError as error:
            if not self._closing.is_set():
                self._failure = error
        except BaseException as error:
            self._failure = error
        if self._failure is not None:
            for stream in self._sockets:
                try:
                    stream.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass

    def _relay(
        self,
        source: socket.socket,
        destination: socket.socket,
        evidence: DirectionEvidence,
    ) -> None:
        handshake = bytearray(recv_exact(source, HANDSHAKE_LENGTH))
        if not handshake.startswith(b"\x13BitTorrent protocol"):
            raise ScenarioFailure("plaintext proxy received a non-BitTorrent handshake")
        if self.clear_fast:
            handshake[FAST_RESERVED_OFFSET] &= ~FAST_RESERVED_BIT
        evidence.handshake = bytes(handshake)
        destination.sendall(handshake)
        while True:
            prefix = recv_maybe_exact(source, 4)
            if prefix is None:
                try:
                    destination.shutdown(socket.SHUT_WR)
                except OSError:
                    pass
                return
            length = struct.unpack(">I", prefix)[0]
            body = recv_exact(source, length) if length else b""
            if body:
                evidence.message_ids.append(body[0])
            if len(body) >= 9 and body[0] == 7:
                piece, begin = struct.unpack(">II", body[1:9])
                with self._piece_sequence_lock:
                    self._piece_sequence += 1
                    evidence.pieces.append(
                        (piece, begin, len(body) - 9, self._piece_sequence)
                    )
                    destination.sendall(prefix + body)
                time.sleep(self.piece_delay_seconds)
                continue
            destination.sendall(prefix + body)

    def close(self) -> None:
        self._closing.set()
        self.release.set()
        try:
            self.listener.close()
        except OSError:
            pass
        for stream in self._sockets:
            try:
                stream.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        self._thread.join(timeout=5)
        if self._thread.is_alive():
            raise ScenarioFailure("duplex proxy did not join")
        if self._failure is not None:
            raise ScenarioFailure(
                "duplex proxy failed: "
                f"{self._failure}; client handshake={self.client.handshake!r} "
                f"messages={self.client.message_ids} "
                f"pieces={self.client.pieces}; upstream messages={self.upstream.message_ids} "
                f"handshake={self.upstream.handshake!r} pieces={self.upstream.pieces}"
            )


class CappedPieceProxy:
    """Relay repeated peer connections until two complete pieces reach the client."""

    def __init__(self, target: tuple[str, int], piece_limit: int = 2) -> None:
        self.target = target
        self.piece_limit = piece_limit
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(8)
        self.listener.settimeout(0.2)
        self.endpoint = ("127.0.0.1", int(self.listener.getsockname()[1]))
        self._closing = threading.Event()
        self._capped = threading.Event()
        self._lock = threading.Lock()
        self._blocks: dict[int, set[tuple[int, int]]] = {}
        self._retained: list[int] = []
        self._expected_bytes = required_piece_payload_bytes()
        self._sockets: list[socket.socket] = []
        self._failure: BaseException | None = None
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    @property
    def retained_pieces(self) -> tuple[int, ...]:
        with self._lock:
            return tuple(sorted(self._retained))

    def _run(self) -> None:
        try:
            while not self._closing.is_set() and not self._capped.is_set():
                try:
                    downstream, _ = self.listener.accept()
                except TimeoutError:
                    continue
                upstream = socket.create_connection(self.target, timeout=5)
                self._sockets.extend((downstream, upstream))
                self._relay_connection(downstream, upstream)
        except BaseException as error:
            if not (self._closing.is_set() and isinstance(error, OSError)):
                self._failure = error
        finally:
            self._shutdown_sockets()

    def _relay_connection(
        self, downstream: socket.socket, upstream: socket.socket
    ) -> None:
        stop = threading.Event()
        workers = (
            threading.Thread(
                target=self._relay_guard,
                args=(downstream, upstream, stop, False),
                daemon=True,
            ),
            threading.Thread(
                target=self._relay_guard,
                args=(upstream, downstream, stop, True),
                daemon=True,
            ),
        )
        for worker in workers:
            worker.start()
        for worker in workers:
            worker.join()
        for stream in (downstream, upstream):
            try:
                stream.close()
            except OSError:
                pass

    def _relay_guard(
        self,
        source: socket.socket,
        destination: socket.socket,
        stop: threading.Event,
        observe_pieces: bool,
    ) -> None:
        try:
            self._relay(source, destination, stop, observe_pieces)
        except (ConnectionError, EOFError, OSError):
            pass
        except BaseException as error:
            self._failure = error
        finally:
            stop.set()
            for stream in (source, destination):
                try:
                    stream.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass

    def _relay(
        self,
        source: socket.socket,
        destination: socket.socket,
        stop: threading.Event,
        observe_pieces: bool,
    ) -> None:
        handshake = recv_exact(source, HANDSHAKE_LENGTH)
        if not handshake.startswith(b"\x13BitTorrent protocol"):
            raise ScenarioFailure("capped proxy received a non-BitTorrent handshake")
        destination.sendall(handshake)
        while not stop.is_set():
            prefix = recv_maybe_exact(source, 4)
            if prefix is None:
                return
            length = struct.unpack(">I", prefix)[0]
            body = recv_exact(source, length) if length else b""
            destination.sendall(prefix + body)
            if observe_pieces and len(body) >= 9 and body[0] == 7:
                piece, begin = struct.unpack(">II", body[1:9])
                if self._observe_piece(piece, begin, len(body) - 9):
                    self._capped.set()
                    time.sleep(1)
                    return

    def _observe_piece(self, piece: int, begin: int, length: int) -> bool:
        with self._lock:
            blocks = self._blocks.setdefault(piece, set())
            blocks.add((begin, length))
            covered = sum(block_length for _, block_length in blocks)
            if (
                covered >= self._expected_bytes[piece]
                and piece not in self._retained
            ):
                self._retained.append(piece)
            return len(self._retained) >= self.piece_limit

    def _shutdown_sockets(self) -> None:
        for stream in self._sockets:
            try:
                stream.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass

    def close(self) -> None:
        self._closing.set()
        try:
            self.listener.close()
        except OSError:
            pass
        self._shutdown_sockets()
        self._thread.join(timeout=5)
        if self._thread.is_alive():
            raise ScenarioFailure("capped proxy did not join")
        if self._failure is not None:
            raise ScenarioFailure(f"capped proxy failed: {self._failure}")


def required_piece_payload_bytes() -> dict[int, int]:
    expected = {piece: 0 for piece in range(4)}
    torrent_offset = 0
    for spec in FILES:
        file_start = torrent_offset
        file_end = file_start + spec.length
        torrent_offset = file_end
        if spec.padding:
            continue
        for piece in expected:
            piece_start = piece * PIECE_SIZE
            piece_end = piece_start + PIECE_SIZE
            expected[piece] += max(
                0, min(file_end, piece_end) - max(file_start, piece_start)
            )
    return expected


def recv_exact(stream: socket.socket, length: int) -> bytes:
    data = bytearray()
    while len(data) < length:
        chunk = stream.recv(length - len(data))
        if not chunk:
            raise EOFError("peer closed within a framed message")
        data.extend(chunk)
    return bytes(data)


def recv_maybe_exact(stream: socket.socket, length: int) -> bytes | None:
    first = stream.recv(length)
    if not first:
        return None
    if len(first) == length:
        return first
    return first + recv_exact(stream, length - len(first))


def build_binary(repository: Path) -> Path:
    completed = subprocess.run(
        [
            "cargo",
            "build",
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
            "failed to build incomplete duplex diagnostic\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    binary = repository / "target/debug/rstorrent-incoming-seed"
    if not binary.is_file():
        raise ScenarioFailure("incomplete duplex diagnostic binary was not created")
    return binary


def start_rstorrent(
    binary: Path,
    fixture: Fixture,
    run_root: Path,
    initial: tuple[int, ...],
    *,
    peer: tuple[str, int] | None = None,
    encryption: str = "allow",
    rate_limited: bool = False,
) -> tuple[subprocess.Popen[str], dict[str, object], Path]:
    storage_root = run_root / "storage"
    profile_root = run_root / "profile"
    storage_root.mkdir(parents=True)
    command = [
        str(binary),
        "--profile-root",
        str(profile_root),
        "--storage-root",
        str(storage_root),
        "--metainfo",
        str(fixture.torrent_path),
        "--fixture-payload",
        str(fixture.flat_payload),
        "--encryption",
        encryption,
        "--tcp-only",
    ]
    if rate_limited:
        command.extend(
            [
                "--upload-rate-limit",
                str(24 * 1024),
                "--download-rate-limit",
                str(24 * 1024),
                "--torrent-upload-rate-limit",
                str(16 * 1024),
                "--torrent-download-rate-limit",
                str(16 * 1024),
            ]
        )
    for piece in initial:
        command.extend(["--initial-piece", str(piece)])
    for file_index in SKIP_FILES:
        command.extend(["--skip-file", str(file_index)])
    if peer is not None:
        command.extend(["--peer", f"{peer[0]}:{peer[1]}"])
    process = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        ready = read_json_line(process, 30)
        if ready.get("event") != "ready" or ready.get("info_hash") != fixture.info_hash:
            raise ScenarioFailure(f"unexpected partial readiness: {ready}")
        if ready.get("registrations") != 1:
            raise ScenarioFailure("partial RSTorrent did not install one active route")
        return process, ready, storage_root
    except BaseException:
        terminate_process(process)
        raise


def wait_complete(
    processes: tuple[subprocess.Popen[str], ...],
    roots: tuple[Path, ...],
    handles: tuple[lt.torrent_handle, ...] = (),
) -> None:
    deadline = time.monotonic() + TRANSFER_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        for process in processes:
            if process.poll() is not None:
                stderr = process.stderr.read() if process.stderr is not None else ""
                raise ScenarioFailure(
                    f"partial RSTorrent exited before completion\nstderr:\n{stderr}"
                )
        if all((root / "duplex-tree").is_dir() for root in roots) and all(
            handle.status().is_seeding for handle in handles
        ):
            return
        for handle in handles:
            status = handle.status()
            if status.errc.value() != 0:
                raise ScenarioFailure(f"libtorrent transfer failed: {status.errc.message()}")
        time.sleep(0.05)
    process_details = []
    for process in processes:
        try:
            process_details.append(seed_snapshot(process))
        except BaseException as error:
            process_details.append({"snapshot_error": str(error)})
    handle_details = [
        {
            "num_pieces": handle.status().num_pieces,
            "is_seeding": handle.status().is_seeding,
            "progress": handle.status().progress,
            "peers": len(handle.get_peer_info()),
        }
        for handle in handles
    ]
    raise ScenarioFailure(
        "incomplete duplex participants did not complete before timeout: "
        f"rstorrent={process_details} libtorrent={handle_details}"
    )


def verify_files(root: Path, fixture: Fixture, *, skipped_absent: bool) -> None:
    for index, spec in enumerate(FILES):
        if spec.padding:
            continue
        path = root / "duplex-tree" / spec.path
        if skipped_absent and index in SKIP_FILES:
            if path.exists():
                raise ScenarioFailure(f"skipped file was unexpectedly published: {path}")
            continue
        if not path.is_file():
            raise ScenarioFailure(f"completed payload file is missing: {path}")
        actual = hashlib.sha1(path.read_bytes()).hexdigest()
        if actual != fixture.expected_hashes[spec.path]:
            raise ScenarioFailure(f"completed payload hash differs: {path}")


def stop_rstorrent(process: subprocess.Popen[str]) -> dict[str, object]:
    snapshot = seed_snapshot(process)
    if process.stdin is None:
        raise ScenarioFailure("partial RSTorrent stdin is unavailable")
    process.stdin.write("stop\n")
    process.stdin.flush()
    stopped = read_json_line(process, 10)
    try:
        returncode = process.wait(timeout=10)
    except subprocess.TimeoutExpired as error:
        terminate_process(process)
        raise ScenarioFailure("partial RSTorrent shutdown did not join") from error
    stderr = process.stderr.read() if process.stderr is not None else ""
    if returncode != 0:
        raise ScenarioFailure(
            f"partial RSTorrent exited with {returncode}\nstderr:\n{stderr}"
        )
    if stopped.get("event") != "stopped":
        raise ScenarioFailure(f"unexpected partial shutdown: {stopped}")
    if not isinstance(stopped.get("payload_bytes_sent"), int) or stopped[
        "payload_bytes_sent"
    ] < PIECE_SIZE:
        raise ScenarioFailure("partial RSTorrent did not account uploaded payload")
    for field, maximum in {
        "pending_high_water": 8,
        "connection_high_water": 210,
        "upload_slots_high_water": 8,
        "queued_requests_high_water": 2_000,
        "read_high_water": 10,
    }.items():
        value = stopped.get(field)
        if not isinstance(value, int) or value > maximum:
            raise ScenarioFailure(f"invalid resource observation {field}={value}")
    return {"snapshot": snapshot, "stopped": stopped}


def assert_plaintext(
    proxy: PlaintextDuplexProxy,
    *,
    fast: bool,
    client_initial: tuple[int, ...],
    upstream_initial: tuple[int, ...],
    require_before_either_complete: bool,
) -> dict[str, object]:
    expected_fast = FAST_RESERVED_BIT if fast else 0
    for label, evidence in (("client", proxy.client), ("upstream", proxy.upstream)):
        if evidence.handshake is None:
            raise ScenarioFailure(f"{label} handshake was not captured")
        if evidence.handshake[FAST_RESERVED_OFFSET] & FAST_RESERVED_BIT != expected_fast:
            raise ScenarioFailure(f"{label} Fast negotiation was not {fast}")
        initial = next((message for message in evidence.message_ids if message in (5, 14, 15)), None)
        if initial != 5:
            raise ScenarioFailure(f"{label} did not send a sparse initial bitfield")
    if not proxy.client.pieces or not proxy.upstream.pieces:
        raise ScenarioFailure("proxy did not capture Piece frames in both directions")
    client_part = any(
        piece in client_initial and begin == BLOCK_SIZE and length > 0
        for piece, begin, length, _ in proxy.client.pieces
    )
    upstream_part = any(
        piece in upstream_initial and begin == BLOCK_SIZE and length > 0
        for piece, begin, length, _ in proxy.upstream.pieces
    )
    if not client_part or not upstream_part:
        raise ScenarioFailure("proxy did not capture cross-file/part-backed Piece frames")
    client_completion = completion_sequence(proxy.client, upstream_initial)
    upstream_completion = completion_sequence(proxy.upstream, client_initial)
    first_client = proxy.client.pieces[0][3]
    first_upstream = proxy.upstream.pieces[0][3]
    completions = [
        sequence
        for sequence in (client_completion, upstream_completion)
        if sequence is not None
    ]
    completion_boundary = (
        min(completions)
        if require_before_either_complete
        else max(completions)
        if completions
        else None
    )
    if completion_boundary is not None and max(first_client, first_upstream) >= completion_boundary:
        raise ScenarioFailure(
            "Piece frames did not satisfy the configured pre-completion gate: "
            f"client_first={first_client} upstream_first={first_upstream} "
            f"client_complete={client_completion} "
            f"upstream_complete={upstream_completion}"
        )
    return {
        "fast": fast,
        "client_piece_frames": len(proxy.client.pieces),
        "upstream_piece_frames": len(proxy.upstream.pieces),
        "client_part_frame": client_part,
        "upstream_part_frame": upstream_part,
        "first_client_piece_sequence": first_client,
        "first_upstream_piece_sequence": first_upstream,
        "first_completion_sequence": min(completions) if completions else None,
        "completion_gate": (
            "before_either_complete"
            if require_before_either_complete
            else "before_both_complete"
        ),
    }


def completion_sequence(
    evidence: DirectionEvidence, receiver_initial: tuple[int, ...]
) -> int | None:
    missing = set(range(4)) - set(receiver_initial)
    expected = 0
    torrent_offset = 0
    for spec in FILES:
        file_start = torrent_offset
        file_end = file_start + spec.length
        torrent_offset = file_end
        if spec.padding:
            continue
        for piece in missing:
            piece_start = piece * PIECE_SIZE
            piece_end = piece_start + PIECE_SIZE
            expected += max(0, min(file_end, piece_end) - max(file_start, piece_start))
    received = 0
    seen: set[tuple[int, int]] = set()
    for piece, begin, length, sequence in evidence.pieces:
        if piece not in missing or (piece, begin) in seen:
            continue
        seen.add((piece, begin))
        received += length
        if received >= expected:
            return sequence
    return None


def run_libtorrent_case(
    binary: Path,
    fixture: Fixture,
    root: Path,
    *,
    rstorrent_initiates: bool,
    fast: bool,
    rate_limited: bool = False,
) -> dict[str, object]:
    session = create_session()
    configure_encryption(session, "disabled")
    handle = None
    process = None
    proxy = None
    try:
        port = wait_for_listener(session, [])
        libtorrent_root = root / "libtorrent"
        handle = add_partial(session, fixture, libtorrent_root)
        if rstorrent_initiates:
            proxy = PlaintextDuplexProxy(
                ("127.0.0.1", port),
                clear_fast=not fast,
                hold_until_release=True,
            )
            process, _, storage_root = start_rstorrent(
                binary,
                fixture,
                root / "rstorrent",
                RST_INITIAL,
                peer=proxy.endpoint,
                encryption="disabled",
                rate_limited=rate_limited,
            )
            transfer_started = time.monotonic()
            proxy.allow_connection()
            client_initial, upstream_initial = RST_INITIAL, REMOTE_INITIAL
        else:
            process, ready, storage_root = start_rstorrent(
                binary,
                fixture,
                root / "rstorrent",
                RST_INITIAL,
                encryption="disabled",
                rate_limited=rate_limited,
            )
            proxy = PlaintextDuplexProxy(
                parse_address(ready),
                clear_fast=not fast,
            )
            transfer_started = time.monotonic()
            handle.connect_peer(proxy.endpoint)
            client_initial, upstream_initial = REMOTE_INITIAL, RST_INITIAL
        wait_complete((process,), (storage_root,), (handle,))
        verify_files(libtorrent_root, fixture, skipped_absent=False)
        verify_files(storage_root, fixture, skipped_absent=True)
        evidence = assert_plaintext(
            proxy,
            fast=fast,
            client_initial=client_initial,
            upstream_initial=upstream_initial,
            require_before_either_complete=False,
        )
        transfer_seconds = time.monotonic() - transfer_started
        evidence["rstorrent"] = stop_rstorrent(process)
        evidence["transfer_seconds"] = transfer_seconds
        if rate_limited:
            evidence["rate_gate"] = assert_rate_limited(
                evidence["rstorrent"], transfer_seconds
            )
        process = None
        return evidence
    finally:
        if process is not None:
            terminate_process(process)
        if proxy is not None:
            proxy.close()
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def assert_rate_limited(
    rstorrent: dict[str, object], transfer_seconds: float
) -> dict[str, object]:
    stopped = rstorrent["stopped"]
    if not isinstance(stopped, dict):
        raise ScenarioFailure("rate-limited shutdown evidence is malformed")
    bandwidth = stopped.get("bandwidth")
    if not isinstance(bandwidth, dict):
        raise ScenarioFailure("rate-limited shutdown has no bandwidth evidence")
    result: dict[str, object] = {
        "session_bytes_per_second": 24 * 1024,
        "torrent_bytes_per_second": 16 * 1024,
    }
    for direction_name in ("upload", "download"):
        direction = bandwidth.get(direction_name)
        if not isinstance(direction, dict):
            raise ScenarioFailure(f"rate-limited {direction_name} evidence is malformed")
        admitted = int(direction["granted_bytes"]) - int(direction["returned_bytes"])
        upper_bound = int(16 * 1024 * transfer_seconds) + 2 * 16 * 1024
        if admitted < PIECE_SIZE:
            raise ScenarioFailure(
                f"rate-limited {direction_name} admitted no complete piece"
            )
        if admitted > upper_bound:
            raise ScenarioFailure(
                f"rate-limited {direction_name} admitted {admitted} above {upper_bound}"
            )
        if int(direction["throttle_wait_high_water_micros"]) <= 0:
            raise ScenarioFailure(f"rate-limited {direction_name} never throttled")
        if int(direction["active_waiters"]) != 0 or int(
            direction["queued_requested_bytes"]
        ) != 0:
            raise ScenarioFailure(f"rate-limited {direction_name} did not drain")
        result[direction_name] = {
            "admitted_bytes": admitted,
            "upper_bound_bytes": upper_bound,
            "throttle_wait_high_water_micros": int(
                direction["throttle_wait_high_water_micros"]
            ),
        }
    return result


def run_rstorrent_case(binary: Path, fixture: Fixture, root: Path) -> dict[str, object]:
    right = left = None
    proxy = None
    rescue_session = None
    rescue_handle = None
    try:
        right, right_ready, right_root = start_rstorrent(
            binary, fixture, root / "right", REMOTE_INITIAL
        )
        proxy = PlaintextDuplexProxy(
            parse_address(right_ready), hold_until_release=True
        )
        left, left_ready, left_root = start_rstorrent(
            binary,
            fixture,
            root / "left",
            RST_INITIAL,
            peer=proxy.endpoint,
        )
        proxy.allow_connection()
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if proxy.client.pieces and proxy.upstream.pieces:
                break
            time.sleep(0.01)
        else:
            raise ScenarioFailure("RSTorrent pair did not exchange Piece frames in both directions")

        rescue_session = create_session()
        diagnostics: list[str] = []
        rescue_handle = add_seed(
            rescue_session,
            fixture.torrent_info,
            fixture.source_root,
            diagnostics,
        )
        rescue_handle.connect_peer(parse_address(left_ready))
        rescue_handle.connect_peer(parse_address(right_ready))
        wait_complete((left, right), (left_root, right_root))
        verify_files(left_root, fixture, skipped_absent=True)
        verify_files(right_root, fixture, skipped_absent=True)
        evidence = assert_plaintext(
            proxy,
            fast=True,
            client_initial=RST_INITIAL,
            upstream_initial=REMOTE_INITIAL,
            require_before_either_complete=True,
        )
        evidence["left"] = stop_rstorrent(left)
        left = None
        evidence["right"] = stop_rstorrent(right)
        right = None
        return evidence
    finally:
        if left is not None:
            terminate_process(left)
        if right is not None:
            terminate_process(right)
        if proxy is not None:
            proxy.close()
        if rescue_handle is not None and rescue_handle.is_valid():
            rescue_session.remove_torrent(rescue_handle)
        if rescue_session is not None:
            rescue_session.pause()
        rescue_handle = None
        rescue_session = None
        gc.collect()


def run_mse_case(binary: Path, fixture: Fixture, root: Path) -> dict[str, object]:
    session = create_session()
    configure_encryption(session, "forced")
    handle = None
    process = None
    proxy = None
    try:
        port = wait_for_listener(session, [])
        libtorrent_root = root / "libtorrent"
        handle = add_partial(session, fixture, libtorrent_root)
        proxy = TcpProxy(("127.0.0.1", port))
        process, _, storage_root = start_rstorrent(
            binary,
            fixture,
            root / "rstorrent",
            RST_INITIAL,
            peer=proxy.endpoint,
            encryption="required",
        )
        wait_complete((process,), (storage_root,), (handle,))
        verify_files(libtorrent_root, fixture, skipped_absent=False)
        verify_files(storage_root, fixture, skipped_absent=True)
        result = stop_rstorrent(process)
        process = None
        traces = proxy.traces()
        if not traces:
            raise ScenarioFailure("forced-MSE proxy captured no connection")
        trace = max(
            traces,
            key=lambda item: len(item.client_to_upstream) + len(item.upstream_to_client),
        )
        if not trace.client_to_upstream or not trace.upstream_to_client:
            raise ScenarioFailure("forced-MSE proxy did not observe both directions")
        if trace.client_to_upstream.startswith(b"\x13BitTorrent protocol") or trace.upstream_to_client.startswith(
            b"\x13BitTorrent protocol"
        ):
            raise ScenarioFailure("forced-MSE case exposed a plaintext handshake")
        return {
            "encrypted_client_bytes": len(trace.client_to_upstream),
            "encrypted_upstream_bytes": len(trace.upstream_to_client),
            "rstorrent": result,
        }
    finally:
        if process is not None:
            terminate_process(process)
        if proxy is not None:
            proxy.close()
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    try:
        binary = build_binary(repository)
        with tempfile.TemporaryDirectory(prefix="rstorrent-incomplete-duplex-") as temporary:
            root = Path(temporary)
            fixture = create_fixture(root / "fixture")
            results = {
                "rstorrent_to_libtorrent_ordinary": run_libtorrent_case(
                    binary,
                    fixture,
                    root / "rstorrent-to-libtorrent",
                    rstorrent_initiates=True,
                    fast=False,
                ),
                "libtorrent_to_rstorrent_fast": run_libtorrent_case(
                    binary,
                    fixture,
                    root / "libtorrent-to-rstorrent",
                    rstorrent_initiates=False,
                    fast=True,
                ),
                "rate_limited_full_duplex": run_libtorrent_case(
                    binary,
                    fixture,
                    root / "rate-limited-full-duplex",
                    rstorrent_initiates=True,
                    fast=True,
                    rate_limited=True,
                ),
                "rstorrent_to_rstorrent_fast": run_rstorrent_case(
                    binary, fixture, root / "rstorrent-pair"
                ),
                "forced_mse": run_mse_case(binary, fixture, root / "mse"),
            }
            print(
                json.dumps(
                    {
                        "status": "ok",
                        "info_hash": fixture.info_hash,
                        "piece_size": PIECE_SIZE,
                        "pieces": fixture.torrent_info.num_pieces(),
                        "skip_files": list(SKIP_FILES),
                        "cases": results,
                    },
                    sort_keys=True,
                )
            )
        return 0
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(f"incomplete duplex interoperability failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
