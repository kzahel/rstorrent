#!/usr/bin/env python3
"""Exercise crossed RSTorrent/libtorrent connections in both ID orderings."""

from __future__ import annotations

import gc
import hashlib
import json
import select
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import warnings
from dataclasses import dataclass
from pathlib import Path

import libtorrent as lt

from first_verified_piece import ScenarioFailure


PAYLOAD_SIZE = 512 * 1024 + 731
PIECE_SIZE = 64 * 1024
TRANSFER_TIMEOUT_SECONDS = 30


@dataclass(frozen=True)
class Fixture:
    torrent_info: lt.torrent_info
    torrent_path: Path
    seed_root: Path
    info_hash: str
    payload_sha1: str


class GatedProxy:
    """Hold RSTorrent's outgoing handshake until libtorrent dials it."""

    def __init__(self, target_port: int, hold_upstream_after_handshake: bool = False) -> None:
        self.target_port = target_port
        self.hold_upstream_after_handshake = hold_upstream_after_handshake
        self.gate = threading.Event()
        self.upstream_payload_gate = threading.Event()
        if not hold_upstream_after_handshake:
            self.upstream_payload_gate.set()
        self.accepted = threading.Event()
        self.stop_requested = threading.Event()
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(1)
        self.listener.settimeout(0.1)
        self.address = f"127.0.0.1:{self.listener.getsockname()[1]}"
        self.sockets: list[socket.socket] = []
        self.error: BaseException | None = None
        self.downstream_bytes = 0
        self.upstream_bytes = 0
        self.downstream_trace = bytearray()
        self.thread = threading.Thread(target=self._run, name="peer-id-race-proxy")

    def start(self) -> None:
        self.thread.start()

    def release(self) -> None:
        self.gate.set()

    def release_upstream_payload(self) -> None:
        self.upstream_payload_gate.set()

    def shutdown(self) -> None:
        self.stop_requested.set()
        self.gate.set()
        self.upstream_payload_gate.set()
        try:
            self.listener.close()
        except OSError:
            pass
        for stream in self.sockets:
            try:
                stream.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            stream.close()
        self.thread.join(2)
        if self.thread.is_alive():
            raise ScenarioFailure("peer-ID race proxy did not terminate")
        if self.error is not None:
            raise ScenarioFailure(f"peer-ID race proxy failed: {self.error}")

    def _run(self) -> None:
        try:
            while not self.stop_requested.is_set():
                try:
                    downstream, _ = self.listener.accept()
                except TimeoutError:
                    continue
                except OSError:
                    if self.stop_requested.is_set():
                        return
                    raise
                self.sockets.append(downstream)
                self.accepted.set()
                while not self.gate.wait(0.05):
                    if self.stop_requested.is_set():
                        return
                upstream = socket.create_connection(
                    ("127.0.0.1", self.target_port), timeout=2
                )
                self.sockets.append(upstream)
                self._forward(downstream, upstream)
                return
        except BaseException as error:
            if not self.stop_requested.is_set():
                self.error = error

    def _forward(self, downstream: socket.socket, upstream: socket.socket) -> None:
        while not self.stop_requested.is_set():
            readable, _, _ = select.select([downstream, upstream], [], [], 0.1)
            for source in readable:
                try:
                    payload = source.recv(64 * 1024)
                except ConnectionResetError:
                    return
                if not payload:
                    return
                target = upstream if source is downstream else downstream
                if source is downstream:
                    self.downstream_bytes += len(payload)
                    if len(self.downstream_trace) < 4096:
                        self.downstream_trace.extend(
                            payload[: 4096 - len(self.downstream_trace)]
                        )
                else:
                    forwarded = self.upstream_bytes
                    self.upstream_bytes += len(payload)
                    if self.hold_upstream_after_handshake:
                        handshake_bytes = max(0, min(len(payload), 68 - forwarded))
                        if handshake_bytes > 0:
                            target.sendall(payload[:handshake_bytes])
                        payload = payload[handshake_bytes:]
                        while payload and not self.upstream_payload_gate.wait(0.05):
                            if self.stop_requested.is_set():
                                return
                        if not payload:
                            continue
                try:
                    target.sendall(payload)
                except (BrokenPipeError, ConnectionResetError):
                    return


def create_fixture(root: Path) -> Fixture:
    seed_root = root / "seed"
    content_root = seed_root / "peer-id-race"
    content_root.mkdir(parents=True)
    payload = bytes((index * 37 + 11) % 251 for index in range(PAYLOAD_SIZE))
    (content_root / "payload.bin").write_bytes(payload)
    files = lt.file_storage()
    files.add_file("peer-id-race/payload.bin", len(payload))
    creator = lt.create_torrent(
        files, piece_size=PIECE_SIZE, flags=lt.create_torrent.v1_only
    )
    lt.set_piece_hashes(creator, str(seed_root))
    torrent_path = root / "peer-id-race.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    torrent_info = lt.torrent_info(str(torrent_path))
    return Fixture(
        torrent_info=torrent_info,
        torrent_path=torrent_path,
        seed_root=seed_root,
        info_hash=str(torrent_info.info_hashes().v1),
        payload_sha1=hashlib.sha1(payload).hexdigest(),
    )


def free_port() -> int:
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.bind(("127.0.0.1", 0))
    port = listener.getsockname()[1]
    listener.close()
    return int(port)


def trace_contains_request(trace: bytearray) -> bool:
    snapshot = bytes(trace)
    offset = 68
    while offset + 4 <= len(snapshot):
        length = int.from_bytes(snapshot[offset : offset + 4], "big")
        if offset + 4 + length > len(snapshot):
            return False
        if length > 0 and snapshot[offset + 4] == 6:
            return True
        offset += 4 + length
    return False


def libtorrent_session(peer_fingerprint: str) -> lt.session:
    exact_peer_id = (peer_fingerprint + "CONTROL00001").encode()
    if len(exact_peer_id) != 20:
        raise ScenarioFailure(f"invalid controlled peer ID: {exact_peer_id!r}")
    session = lt.session(
        {
            "listen_interfaces": "127.0.0.1:0",
            "peer_fingerprint": peer_fingerprint,
            "enable_dht": False,
            "enable_lsd": False,
            "enable_upnp": False,
            "enable_natpmp": False,
            "enable_incoming_utp": False,
            "enable_outgoing_utp": False,
            "enable_incoming_tcp": True,
            "enable_outgoing_tcp": True,
            "allow_multiple_connections_per_ip": True,
            "out_enc_policy": 2,
            "in_enc_policy": 2,
            "alert_queue_size": 1000,
        }
    )
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", DeprecationWarning)
        session.set_peer_id(lt.sha1_hash(exact_peer_id))
    return session


def wait_for_seed(session: lt.session, handle: lt.torrent_handle) -> int:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        status = handle.status()
        if status.errc.value() != 0:
            raise ScenarioFailure(f"libtorrent seed failed: {status.errc.message()}")
        if status.is_seeding and session.is_listening() and session.listen_port() > 0:
            return session.listen_port()
        time.sleep(0.02)
    raise ScenarioFailure("libtorrent seed did not become ready")


def wait_for_leecher(session: lt.session, handle: lt.torrent_handle) -> int:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        status = handle.status()
        if status.errc.value() != 0:
            raise ScenarioFailure(f"libtorrent leecher failed: {status.errc.message()}")
        if (
            status.state == lt.torrent_status.downloading
            and session.is_listening()
            and session.listen_port() > 0
        ):
            return session.listen_port()
        time.sleep(0.02)
    raise ScenarioFailure("libtorrent leecher did not become ready")


def run_case(
    probe_binary: Path,
    fixture: Fixture,
    root: Path,
    peer_fingerprint: str,
    expected_direction: str,
) -> dict[str, object]:
    session = libtorrent_session(peer_fingerprint)
    parameters = lt.add_torrent_params()
    parameters.ti = fixture.torrent_info
    leech_root = root / "libtorrent-leech"
    leech_root.mkdir(parents=True)
    parameters.save_path = str(leech_root)
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    handle = session.add_torrent(parameters)
    handle.set_upload_limit(64 * 1024)
    libtorrent_port = wait_for_leecher(session, handle)
    proxy = GatedProxy(libtorrent_port)
    proxy.start()
    listen_port = free_port()
    output = root / "rstorrent-output"
    process = subprocess.Popen(
        [
            str(probe_binary),
            "--metainfo",
            str(fixture.torrent_path),
            "--seed-root",
            str(fixture.seed_root),
            "--output",
            str(output),
            "--peer",
            proxy.address,
            "--listen-port",
            str(listen_port),
            "--peer-id",
            "-RS0001-CONTROL00001",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    source_session = None
    source_handle = None
    incoming_proxy = None
    try:
        if process.stdout is None or process.stdin is None:
            raise ScenarioFailure("peer-ID race probe pipes are unavailable")
        ready = json.loads(process.stdout.readline())
        if ready.get("event") != "ready":
            raise ScenarioFailure(f"unexpected probe readiness: {ready}")
        if not proxy.accepted.wait(5):
            raise ScenarioFailure("RSTorrent did not begin the gated outgoing dial")
        ready_port = int(str(ready.get("listen", "")).rpartition(":")[2])
        incoming_proxy = GatedProxy(
            ready_port,
            hold_upstream_after_handshake=expected_direction == "incoming",
        )
        incoming_proxy.start()
        incoming_proxy.release()

        def start_crossed_incoming() -> None:
            incoming_port = int(incoming_proxy.address.rpartition(":")[2])
            handle.connect_peer(("127.0.0.1", incoming_port))
            if not incoming_proxy.accepted.wait(5):
                raise ScenarioFailure("libtorrent did not begin the crossed outgoing dial")
            incoming_deadline = time.monotonic() + 5
            while (
                time.monotonic() < incoming_deadline
                and incoming_proxy.upstream_bytes < 68
            ):
                time.sleep(0.05)
            if incoming_proxy.upstream_bytes < 68:
                raise ScenarioFailure(
                    "libtorrent crossed outgoing dial did not handshake; "
                    f"forwarded={incoming_proxy.downstream_bytes}/"
                    f"{incoming_proxy.upstream_bytes} "
                    f"trace={incoming_proxy.downstream_trace.hex()}"
                )

        def start_source(upload_limit: int = 0) -> tuple[lt.session, lt.torrent_handle]:
            controlled_source = libtorrent_session("-LS0001-")
            source_parameters = lt.add_torrent_params()
            source_parameters.ti = fixture.torrent_info
            source_parameters.save_path = str(fixture.seed_root)
            source_parameters.flags |= lt.torrent_flags.seed_mode
            source_parameters.flags &= ~lt.torrent_flags.paused
            source_parameters.flags &= ~lt.torrent_flags.auto_managed
            controlled_handle = controlled_source.add_torrent(source_parameters)
            if upload_limit > 0:
                controlled_handle.set_upload_limit(upload_limit)
            source_port = wait_for_seed(controlled_source, controlled_handle)
            handle.connect_peer(("127.0.0.1", source_port))
            return controlled_source, controlled_handle

        if expected_direction == "outgoing":
            start_crossed_incoming()
            proxy.release()
        else:
            proxy.release()
            outgoing_deadline = time.monotonic() + 5
            while time.monotonic() < outgoing_deadline and proxy.upstream_bytes < 68:
                time.sleep(0.05)
            if proxy.upstream_bytes < 68:
                raise ScenarioFailure("RSTorrent outgoing leg did not handshake first")
            source_session, source_handle = start_source(64 * 1024)
            request_deadline = time.monotonic() + 15
            while (
                time.monotonic() < request_deadline
                and not trace_contains_request(proxy.downstream_trace)
            ):
                time.sleep(0.05)
            if not trace_contains_request(proxy.downstream_trace):
                raise ScenarioFailure("outgoing loser did not own a content request")
            source_session.remove_torrent(source_handle)
            source_handle = None
            source_session.pause()
            start_crossed_incoming()

        convergence_deadline = time.monotonic() + 5
        while time.monotonic() < convergence_deadline:
            if len(handle.get_peer_info()) == 1:
                break
            time.sleep(0.05)
        else:
            raise ScenarioFailure(
                f"{expected_direction} crossed connections did not converge: "
                f"libtorrent_peers={len(handle.get_peer_info())}"
            )
        incoming_proxy.release_upstream_payload()

        if expected_direction == "outgoing":
            source_session, source_handle = start_source()

        deadline = time.monotonic() + TRANSFER_TIMEOUT_SECONDS
        while time.monotonic() < deadline:
            rstorrent_verified = any(
                path.is_file()
                and hashlib.sha1(path.read_bytes()).hexdigest() == fixture.payload_sha1
                for path in output.parent.glob(f"{output.name}/**/*")
            )
            if handle.status().is_seeding and (
                expected_direction == "incoming" or rstorrent_verified
            ):
                break
            time.sleep(0.05)
        else:
            raise ScenarioFailure("crossed winner did not complete verified payload")
        process.stdin.write("stop\n")
        process.stdin.flush()
        completed = json.loads(process.stdout.readline())
        returncode = process.wait(timeout=10)
        if returncode != 0:
            stderr = process.stderr.read() if process.stderr is not None else ""
            raise ScenarioFailure(f"peer-ID race probe failed:\n{stderr}")
        winner_field = f"{expected_direction}_winner_observed"
        if not completed.get(winner_field):
            raise ScenarioFailure(f"probe did not observe {expected_direction} winner: {completed}")
        if completed.get("duplicate_connections") != 1:
            raise ScenarioFailure(f"probe lacks typed duplicate evidence: {completed}")
        for field in ("terminal_pending", "terminal_established", "terminal_connections"):
            if completed.get(field) != 0:
                raise ScenarioFailure(f"probe retained {field}: {completed}")
        return {
            "fingerprint": peer_fingerprint,
            "winner": expected_direction,
            "libtorrent_crossed_peers": 1,
            "duplicate_connections": completed["duplicate_connections"],
            "connection_high_water": completed["connection_high_water"],
            "payload_sha1": fixture.payload_sha1,
        }
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=5)
        proxy.shutdown()
        if incoming_proxy is not None:
            incoming_proxy.shutdown()
        if source_handle is not None and source_handle.is_valid():
            source_session.remove_torrent(source_handle)
        if source_session is not None:
            source_session.pause()
        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def build_probe(repository: Path) -> Path:
    completed = subprocess.run(
        ["cargo", "build", "-p", "rstorrent-engine", "--bin", "rstorrent-peer-id-race"],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            f"failed to build peer-ID race probe\n{completed.stdout}\n{completed.stderr}"
        )
    return repository / "target/debug/rstorrent-peer-id-race"


def run() -> None:
    repository = Path(__file__).resolve().parents[2]
    root = Path(tempfile.mkdtemp(prefix="rstorrent-peer-id-duplicate-"))
    try:
        probe = build_probe(repository)
        fixture = create_fixture(root)
        cases = [
            run_case(probe, fixture, root / "rstorrent-greater", "-LT20D0-", "outgoing"),
            run_case(probe, fixture, root / "libtorrent-greater", "-ZZ9999-", "incoming"),
        ]
        print(f"libtorrent_binding_version={lt.__version__}")
        print(f"libtorrent_native_version={lt.version}")
        for case in cases:
            print("peer_id_duplicate_case " + " ".join(f"{key}={value}" for key, value in case.items()))
    finally:
        shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    try:
        run()
    except BaseException as error:
        raise SystemExit(f"peer-ID duplicate scenario failed: {error}") from error
