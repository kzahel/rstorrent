#!/usr/bin/env python3
"""Prove bounded two-hop BEP 11 discovery against pinned libtorrent."""

from __future__ import annotations

import gc
import hashlib
import json
import re
import shutil
import socket
import struct
import subprocess
import tempfile
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

import libtorrent as lt

from first_verified_piece import (
    PAYLOAD_NAME,
    ScenarioConfig,
    ScenarioFailure,
    compare_payloads,
    create_fixture,
    create_session,
    wait_for_listener,
)


HANDSHAKE_LENGTH = 68
TRANSFER_TIMEOUT_SECONDS = 300
PROCESS_TIMEOUT_SECONDS = 330
PAYLOAD_SIZE = 16 * 1024 * 1024
PIECE_SIZE = 256 * 1024
SEED_UPLOAD_LIMIT = 32 * 1024
BOOTSTRAP_DOWNLOAD_LIMIT = 8 * 1024
BOOTSTRAP_UPLOAD_LIMIT = 32 * 1024
FINISH_TRANSFER_LIMIT = 512 * 1024


@dataclass
class ExtendedCapture:
    buffer: bytearray = field(default_factory=bytearray)
    handshake: bytes | None = None
    messages: list[tuple[int, bytes]] = field(default_factory=list)

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
            frame = bytes(self.buffer[4 : length + 4])
            del self.buffer[: length + 4]
            if len(frame) >= 2 and frame[0] == 20:
                self.messages.append((frame[1], frame[2:]))


class CapturingProxy:
    def __init__(self, target: tuple[str, int]) -> None:
        self.target = target
        self.from_downstream = ExtendedCapture()
        self.from_target = ExtendedCapture()
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
            raise ScenarioFailure("PEX capture proxy did not terminate")
        if self.failure is not None:
            raise ScenarioFailure(f"PEX capture proxy failed: {self.failure}")

    def _run(self) -> None:
        try:
            self.listener.settimeout(10)
            downstream, _ = self.listener.accept()
            upstream = socket.create_connection(self.target, timeout=5)
            downstream.settimeout(None)
            upstream.settimeout(None)
            threads = (
                threading.Thread(
                    target=self._relay,
                    args=(downstream, upstream, self.from_downstream),
                    daemon=True,
                ),
                threading.Thread(
                    target=self._relay,
                    args=(upstream, downstream, self.from_target),
                    daemon=True,
                ),
            )
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join()
            downstream.close()
            upstream.close()
        except BaseException as error:
            self.failure = error

    @staticmethod
    def _relay(
        source: socket.socket,
        destination: socket.socket,
        capture: ExtendedCapture | None,
    ) -> None:
        try:
            while data := source.recv(64 * 1024):
                if capture is not None:
                    capture.feed(data)
                destination.sendall(data)
        except (ConnectionError, OSError):
            pass
        finally:
            try:
                destination.shutdown(socket.SHUT_WR)
            except OSError:
                pass


class TwoConnectionRelay:
    """Give bootstrap and RSTorrent independent sockets to one seed."""

    def __init__(self, target: tuple[str, int]) -> None:
        self.target = target
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(2)
        self.port = self.listener.getsockname()[1]
        self.first_accepted = threading.Event()
        self.rstorrent_accepted = threading.Event()
        self.stop = threading.Event()
        self.rstorrent_index: int | None = None
        self.connections: list[tuple[socket.socket, socket.socket]] = []
        self.rstorrent_connections: list[bool] = []
        self.peer_ids: list[bytes] = []
        self.threads: list[threading.Thread] = []
        self.failure: BaseException | None = None
        self.thread = threading.Thread(target=self._run, daemon=True)

    def start(self) -> None:
        self.thread.start()

    def wait_for(self, index: int, timeout_seconds: float = 10) -> None:
        event = self.first_accepted if index == 0 else self.rstorrent_accepted
        if not event.wait(timeout_seconds):
            label = "bootstrap" if index == 0 else "RSTorrent"
            raise ScenarioFailure(f"second-hop relay {label} connection did not arrive")

    def close_bootstrap_connection(self) -> None:
        self.wait_for(0)
        for is_rstorrent, pair in zip(
            self.rstorrent_connections,
            self.connections,
            strict=True,
        ):
            if is_rstorrent:
                continue
            for stream in pair:
                try:
                    stream.setsockopt(
                        socket.SOL_SOCKET,
                        socket.SO_LINGER,
                        struct.pack("ii", 1, 0),
                    )
                except OSError:
                    pass
                stream.close()

    def join(self) -> None:
        self.stop.set()
        self.listener.close()
        self.thread.join(timeout=5)
        for stream_pair in self.connections:
            for stream in stream_pair:
                try:
                    stream.close()
                except OSError:
                    pass
        for thread in self.threads:
            thread.join(timeout=2)
        if self.failure is not None:
            raise ScenarioFailure(f"second-hop relay failed: {self.failure}")

    def _run(self) -> None:
        try:
            self.listener.settimeout(1)
            attempt = 0
            while not self.stop.is_set():
                try:
                    client, _ = self.listener.accept()
                except socket.timeout:
                    continue
                client.settimeout(10)
                handshake = bytearray()
                while len(handshake) < HANDSHAKE_LENGTH:
                    chunk = client.recv(HANDSHAKE_LENGTH - len(handshake))
                    if not chunk:
                        raise ScenarioFailure("relay client closed during handshake")
                    handshake.extend(chunk)
                client.settimeout(None)
                is_rstorrent = bytes(handshake[48:68]).startswith(b"-RS")
                if attempt != 0 and not is_rstorrent:
                    client.close()
                    continue
                upstream = socket.create_connection(self.target, timeout=5)
                upstream.sendall(handshake)
                index = len(self.connections)
                self.connections.append((client, upstream))
                self.rstorrent_connections.append(is_rstorrent)
                self.peer_ids.append(bytes(handshake[48:68]))
                for source, destination in ((client, upstream), (upstream, client)):
                    thread = threading.Thread(
                        target=CapturingProxy._relay,
                        args=(source, destination, None),
                        daemon=True,
                    )
                    self.threads.append(thread)
                    thread.start()
                if index == 0:
                    self.first_accepted.set()
                if is_rstorrent:
                    self.rstorrent_index = index
                    self.rstorrent_accepted.set()
                    self.listener.close()
                    return
                attempt += 1
        except OSError as error:
            if not self.stop.is_set():
                self.failure = error
        except BaseException as error:
            self.failure = error


class InboundAnchor:
    """Hold a non-advertisable inbound peer so libtorrent can send diffs."""

    def __init__(
        self,
        target: tuple[str, int],
        info_hash: bytes,
        num_pieces: int,
        payload: bytes,
    ) -> None:
        self.stop = threading.Event()
        self.stream = socket.create_connection(target, timeout=5)
        handshake = (
            b"\x13BitTorrent protocol"
            + bytes(8)
            + info_hash
            + b"-AN0001-000000000000"
        )
        self.stream.sendall(handshake)
        response = bytearray()
        while len(response) < HANDSHAKE_LENGTH:
            chunk = self.stream.recv(HANDSHAKE_LENGTH - len(response))
            if not chunk:
                raise ScenarioFailure("inbound anchor closed during handshake")
            response.extend(chunk)
        bitfield = bytearray([0xFF] * ((num_pieces + 7) // 8))
        if num_pieces % 8:
            bitfield[-1] &= 0xFF << (8 - num_pieces % 8)
        self.stream.sendall(
            struct.pack(">I", 1 + len(bitfield)) + bytes([5]) + bitfield
        )
        self.stream.sendall(struct.pack(">IB", 1, 1))
        self.payload = payload
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.thread.start()

    def close(self) -> None:
        self.stop.set()
        try:
            self.stream.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        self.stream.close()
        self.thread.join(timeout=2)

    def _serve(self) -> None:
        self.stream.settimeout(1)
        buffer = bytearray()
        last_keep_alive = time.monotonic()
        while not self.stop.is_set():
            try:
                chunk = self.stream.recv(64 * 1024)
                if not chunk:
                    return
                buffer.extend(chunk)
            except socket.timeout:
                pass
            except OSError:
                return
            while len(buffer) >= 4:
                length = struct.unpack(">I", buffer[:4])[0]
                if len(buffer) < 4 + length:
                    break
                frame = bytes(buffer[4 : 4 + length])
                del buffer[: 4 + length]
                if len(frame) != 13 or frame[0] != 6:
                    continue
                piece, begin, block_length = struct.unpack(">III", frame[1:])
                offset = piece * PIECE_SIZE + begin
                block = self.payload[offset : offset + block_length]
                if len(block) != block_length:
                    return
                response = struct.pack(">IBII", 9 + len(block), 7, piece, begin)
                try:
                    self.stream.sendall(response + block)
                except OSError:
                    return
            if time.monotonic() - last_keep_alive >= 20:
                try:
                    self.stream.sendall(bytes(4))
                except OSError:
                    return
                last_keep_alive = time.monotonic()


def add_bootstrap(
    session: lt.session,
    torrent_info: lt.torrent_info,
    download_directory: Path,
    seed_payload: Path,
) -> lt.torrent_handle:
    download_directory.mkdir()
    partial_payload = download_directory / PAYLOAD_NAME
    shutil.copy2(seed_payload, partial_payload)
    with partial_payload.open("r+b") as output:
        output.seek(PAYLOAD_SIZE // 2)
        remaining = PAYLOAD_SIZE - PAYLOAD_SIZE // 2
        zeroes = bytes(1024 * 1024)
        while remaining:
            chunk = zeroes[: min(len(zeroes), remaining)]
            output.write(chunk)
            remaining -= len(chunk)
    parameters = lt.add_torrent_params()
    parameters.ti = torrent_info
    parameters.save_path = str(download_directory)
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    handle = session.add_torrent(parameters)
    handle.set_download_limit(BOOTSTRAP_DOWNLOAD_LIMIT)
    handle.set_upload_limit(BOOTSTRAP_UPLOAD_LIMIT)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        status = handle.status()
        if 0 < status.num_pieces < torrent_info.num_pieces():
            return handle
        if status.errc.value() != 0:
            raise ScenarioFailure(f"partial bootstrap failed: {status.errc.message()}")
        time.sleep(0.05)
    raise ScenarioFailure("partial bootstrap did not verify its retained prefix")


def add_second_hop(
    session: lt.session,
    torrent_info: lt.torrent_info,
    download_directory: Path,
    seed_payload: Path,
) -> lt.torrent_handle:
    download_directory.mkdir()
    partial_payload = download_directory / PAYLOAD_NAME
    shutil.copy2(seed_payload, partial_payload)
    with partial_payload.open("r+b") as output:
        output.seek(0)
        remaining = PAYLOAD_SIZE // 2
        zeroes = bytes(1024 * 1024)
        while remaining:
            chunk = zeroes[: min(len(zeroes), remaining)]
            output.write(chunk)
            remaining -= len(chunk)
    parameters = lt.add_torrent_params()
    parameters.ti = torrent_info
    parameters.save_path = str(download_directory)
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    handle = session.add_torrent(parameters)
    handle.set_upload_limit(SEED_UPLOAD_LIMIT)
    handle.set_flags(lt.torrent_flags.disable_pex)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        status = handle.status()
        if 0 < status.num_pieces < torrent_info.num_pieces():
            return handle
        if status.errc.value() != 0:
            raise ScenarioFailure(f"partial second hop failed: {status.errc.message()}")
        time.sleep(0.05)
    raise ScenarioFailure("partial second hop did not verify its retained suffix")


def wait_for_peer(handle: lt.torrent_handle, endpoint_port: int) -> None:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if any(peer.ip[1] == endpoint_port for peer in handle.get_peer_info()):
            return
        time.sleep(0.05)
    raise ScenarioFailure("libtorrent bootstrap did not establish its relay peer")


def wait_for_peer_withdrawal(handle: lt.torrent_handle, endpoint_port: int) -> None:
    deadline = time.monotonic() + 10
    matching: list[lt.peer_info] = []
    while time.monotonic() < deadline:
        matching = [
            peer
            for peer in handle.get_peer_info()
            if peer.ip[1] == endpoint_port
        ]
        if not matching or all(
            peer.flags & (lt.peer_info.connecting | lt.peer_info.handshake)
            for peer in matching
        ):
            return
        time.sleep(0.05)
    raise ScenarioFailure(
        "libtorrent retained an established relay peer: "
        + ", ".join(f"{peer.ip} flags={peer.flags}" for peer in matching)
    )


def require_connected_peer_count(
    handle: lt.torrent_handle,
    minimum: int,
    label: str,
) -> None:
    peers = [
        peer
        for peer in handle.get_peer_info()
        if not peer.flags & (lt.peer_info.connecting | lt.peer_info.handshake)
    ]
    if len(peers) < minimum:
        raise ScenarioFailure(
            f"{label} has {len(peers)} established peers, expected {minimum}: "
            + ", ".join(f"{peer.ip} flags={peer.flags}" for peer in peers)
        )


def wait_for_pex_baseline(session: lt.session) -> list[str]:
    deadline = time.monotonic() + 130
    pex_alerts: list[str] = []
    while time.monotonic() < deadline:
        for alert in session.pop_alerts():
            message = alert.message()
            if "PEX" not in message:
                continue
            pex_alerts.append(message)
            if "client: RS" in message and "==> PEX_DIFF" in message:
                return pex_alerts
        time.sleep(0.1)
    raise ScenarioFailure(
        f"libtorrent did not build a live-relay PEX baseline: {pex_alerts[-50:]!r}"
    )


def wait_for_pex_drop(session: lt.session) -> list[str]:
    deadline = time.monotonic() + 130
    pex_alerts: list[str] = []
    while time.monotonic() < deadline:
        for alert in session.pop_alerts():
            message = alert.message()
            if "PEX" not in message:
                continue
            pex_alerts.append(message)
            sent = re.search(r"==> PEX_DIFF \[ dropped: (\d+)", message)
            received = re.search(r"<== PEX \[ dropped: (\d+)", message)
            if (
                "client: RS" in message
                and any(
                    match is not None and int(match.group(1)) > 0
                    for match in (sent, received)
                )
            ):
                return pex_alerts
        time.sleep(0.1)
    raise ScenarioFailure(
        f"libtorrent did not emit the relay PEX drop: {pex_alerts[-50:]!r}"
    )


def compact_v4(port: int) -> bytes:
    return socket.inet_aton("127.0.0.1") + struct.pack(">H", port)


def advertised_pex_id(capture: ExtendedCapture, label: str) -> int:
    for extension_id, payload in capture.messages:
        if extension_id != 0:
            continue
        decoded = lt.bdecode(payload)
        mapping = decoded.get(b"m", {}) if isinstance(decoded, dict) else {}
        value = mapping.get(b"ut_pex") if isinstance(mapping, dict) else None
        if isinstance(value, int) and value > 0:
            return value
    raise ScenarioFailure(f"{label} did not advertise ut_pex")


def assert_pex_lifecycle(
    from_rstorrent: ExtendedCapture,
    from_libtorrent: ExtendedCapture,
    endpoint_port: int,
) -> dict[str, int]:
    libtorrent_pex_id = advertised_pex_id(from_libtorrent, "libtorrent")
    rstorrent_pex_id = advertised_pex_id(from_rstorrent, "RSTorrent")
    additions = 0
    drops = 0
    observed: list[dict[str, str]] = []
    expected = compact_v4(endpoint_port)
    for extension_id, payload in from_libtorrent.messages:
        if extension_id != rstorrent_pex_id:
            continue
        decoded = lt.bdecode(payload)
        if not isinstance(decoded, dict):
            continue
        added = decoded.get(b"added", b"")
        dropped = decoded.get(b"dropped", b"")
        observed.append(
            {
                "added": added.hex(),
                "dropped": dropped.hex(),
                "added6": decoded.get(b"added6", b"").hex(),
                "dropped6": decoded.get(b"dropped6", b"").hex(),
            }
        )
        additions += sum(
            added[index : index + 6] == expected
            for index in range(0, len(added), 6)
        )
        drops += sum(
            dropped[index : index + 6] == expected
            for index in range(0, len(dropped), 6)
        )
    if additions == 0:
        raise ScenarioFailure(
            f"libtorrent did not advertise the second-hop relay: {observed!r}"
        )
    return {
        "libtorrent_pex_id": libtorrent_pex_id,
        "rstorrent_pex_id": rstorrent_pex_id,
        "additions": additions,
        "drops": drops,
    }


def build_binary(repository: Path) -> Path:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "rstorrent-engine",
            "--bin",
            "rstorrent-download-piece",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to build PEX diagnostic\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    return repository / "target/debug/rstorrent-download-piece"


def run(repository: Path) -> dict[str, object]:
    binary = build_binary(repository)
    config = ScenarioConfig(
        name="pex",
        payload_size=PAYLOAD_SIZE,
        piece_size=PIECE_SIZE,
        payload_allowance=512 * 1024,
        diagnostic_timeout_seconds=TRANSFER_TIMEOUT_SECONDS,
        process_timeout_seconds=PROCESS_TIMEOUT_SECONDS,
    )
    with tempfile.TemporaryDirectory(prefix="rstorrent-pex-") as temporary:
        run_root = Path(temporary)
        torrent_path, seed_directory, _, expected_hash, torrent_info = create_fixture(
            run_root, config, require_single_piece=False
        )
        diagnostics: list[str] = []
        seed_session = create_session()
        bootstrap_session = create_session()
        alert_mask = int(lt.alert.category_t.all_categories)
        seed_session.apply_settings(
            {
                "allow_multiple_connections_per_ip": True,
                "alert_mask": alert_mask,
                "alert_queue_size": 100_000,
            }
        )
        bootstrap_session.apply_settings(
            {
                "allow_multiple_connections_per_ip": True,
                "alert_mask": alert_mask,
                "alert_queue_size": 100_000,
            }
        )
        seed_port = wait_for_listener(seed_session, diagnostics)
        bootstrap_port = wait_for_listener(bootstrap_session, diagnostics)
        seed = add_second_hop(
            seed_session,
            torrent_info,
            run_root / "second-hop",
            seed_directory / PAYLOAD_NAME,
        )
        relay = TwoConnectionRelay(("127.0.0.1", seed_port))
        relay.start()
        bootstrap = add_bootstrap(
            bootstrap_session,
            torrent_info,
            run_root / "bootstrap",
            seed_directory / PAYLOAD_NAME,
        )
        bootstrap.connect_peer(("127.0.0.1", relay.port))
        relay.wait_for(0)
        wait_for_peer(bootstrap, relay.port)
        anchor = InboundAnchor(
            ("127.0.0.1", bootstrap_port),
            torrent_info.info_hashes().v1.to_bytes(),
            torrent_info.num_pieces(),
            (seed_directory / PAYLOAD_NAME).read_bytes(),
        )
        capture = CapturingProxy(("127.0.0.1", bootstrap_port))
        capture.start()
        output = run_root / "downloaded.bin"
        started = time.monotonic()
        process = subprocess.Popen(
            [
                str(binary),
                "--metainfo",
                str(torrent_path),
                "--peer",
                f"127.0.0.1:{capture.port}",
                "--output",
                str(output),
                "--timeout-seconds",
                str(config.diagnostic_timeout_seconds),
                "--max-buffered-payload-bytes",
                str(config.payload_allowance),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            try:
                relay.wait_for(1, timeout_seconds=75)
            except ScenarioFailure as error:
                process_status = process.poll()
                early_output = ""
                if process_status is not None:
                    early_stdout, early_stderr = process.communicate(timeout=2)
                    early_output = f"; stdout={early_stdout!r}; stderr={early_stderr!r}"
                early_alerts = " | ".join(
                    alert.message() for alert in bootstrap_session.pop_alerts()
                )
                raise ScenarioFailure(
                    f"{error}; captured extensions="
                    f"{[(extension_id, len(payload)) for extension_id, payload in capture.from_target.messages]}; "
                    f"target_handshake={capture.from_target.handshake is not None}; "
                    f"process_status={process_status}{early_output}; alerts={early_alerts[-4000:]}"
                ) from error
            baseline_pex_alerts = wait_for_pex_baseline(bootstrap_session)
            seed.set_upload_limit(1)
            seed_peers = seed.get_peer_info()
            if relay.rstorrent_index is None:
                raise ScenarioFailure("relay lost the RSTorrent connection index")
            second_upstream_port = relay.connections[relay.rstorrent_index][1].getsockname()[1]
            if not any(peer.ip[1] == second_upstream_port for peer in seed_peers):
                raise ScenarioFailure(
                    "seed did not retain RSTorrent's second-hop relay: "
                    + ", ".join(
                        f"{peer.ip} state={peer.flags}" for peer in seed_peers
                    )
                    + "; relay upstreams="
                    + ", ".join(
                        str(pair[1].getsockname()) for pair in relay.connections
                    )
                )
            relay.close_bootstrap_connection()
            try:
                wait_for_peer_withdrawal(bootstrap, relay.port)
            except ScenarioFailure as error:
                raise ScenarioFailure(
                    f"{error}; relay_port={relay.port}; relay_peer_ids="
                    f"{relay.peer_ids!r}; roles={relay.rstorrent_connections!r}"
                ) from error
            try:
                require_connected_peer_count(
                    bootstrap,
                    2,
                    "bootstrap after relay withdrawal",
                )
            except ScenarioFailure as error:
                alerts = " | ".join(
                    alert.message() for alert in bootstrap_session.pop_alerts()
                )
                raise ScenarioFailure(f"{error}; alerts={alerts[-6000:]}") from error
            drop_pex_alerts = wait_for_pex_drop(bootstrap_session)
            seed.set_upload_limit(FINISH_TRANSFER_LIMIT)
            bootstrap.set_download_limit(FINISH_TRANSFER_LIMIT)
            bootstrap.set_upload_limit(FINISH_TRANSFER_LIMIT)
            stdout, stderr = process.communicate(timeout=PROCESS_TIMEOUT_SECONDS)
        except BaseException:
            process.kill()
            process.wait(timeout=5)
            raise
        if process.returncode != 0:
            seed_alerts = [alert.message() for alert in seed_session.pop_alerts()]
            bootstrap_alerts = [
                alert.message() for alert in bootstrap_session.pop_alerts()
            ]
            alerts = [
                *(f"seed: {message}" for message in seed_alerts[-25:]),
                *(
                    f"bootstrap: {message}"
                    for message in bootstrap_alerts[-25:]
                ),
            ]
            raise ScenarioFailure(
                "RSTorrent PEX leecher failed\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}\n"
                f"libtorrent alerts:\n" + "\n".join(alerts)
            )
        actual_hash = compare_payloads(seed_directory / PAYLOAD_NAME, output)
        if actual_hash != expected_hash:
            raise ScenarioFailure("PEX download hash differs from the seed")
        capture.join()
        lifecycle = assert_pex_lifecycle(
            capture.from_downstream,
            capture.from_target,
            relay.port,
        )
        remaining_pex_alerts = []
        for alert in bootstrap_session.pop_alerts():
            message = alert.message()
            if "PEX" in message:
                remaining_pex_alerts.append(message)
        pex_alerts = baseline_pex_alerts + drop_pex_alerts + remaining_pex_alerts
        logged_drops = sum(
            sum(int(match.group(1)) for match in matches if match is not None)
            for message in pex_alerts
            if "client: RS" in message
            for matches in [
                (
                    re.search(r"==> PEX_DIFF \[ dropped: (\d+)", message),
                    re.search(r"<== PEX \[ dropped: (\d+)", message),
                )
            ]
        )
        if lifecycle["drops"] == 0 and logged_drops == 0:
            raise ScenarioFailure(
                "libtorrent did not withdraw the second-hop relay: "
                f"{pex_alerts[-100:]!r}"
            )
        lifecycle["logged_drops"] = logged_drops
        elapsed = time.monotonic() - started
        if elapsed < 60:
            raise ScenarioFailure("controlled transfer ended before the minute PEX diff")
        anchor.close()
        seed_session.remove_torrent(seed)
        bootstrap_session.remove_torrent(bootstrap)
        relay.join()
        del seed
        del bootstrap
        del seed_session
        del bootstrap_session
        gc.collect()
        return {
            "status": "ok",
            "libtorrent_version": lt.__version__,
            "info_hash": str(torrent_info.info_hashes().v1),
            "payload_bytes": PAYLOAD_SIZE,
            "payload_sha1": hashlib.sha1(output.read_bytes()).hexdigest(),
            "elapsed_seconds": round(elapsed, 3),
            **lifecycle,
        }


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    try:
        print(json.dumps(run(repository), sort_keys=True))
        return 0
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(f"PEX interoperability failed: {error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
