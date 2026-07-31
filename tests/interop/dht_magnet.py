#!/usr/bin/env python3
"""Download a trackerless magnet through controlled Mainline DHT discovery."""

from __future__ import annotations

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

import libtorrent as lt

from first_verified_piece import (
    DEFAULT_PAYLOAD_ALLOWANCE,
    ScenarioFailure,
    add_seed,
    compare_payloads,
    create_session,
    wait_for_listener,
)
from magnet_metadata import (
    METADATA_BLOCK_SIZE,
    METADATA_TIMEOUT_SECONDS,
    PROCESS_TIMEOUT_SECONDS,
    Fixture,
    build_binaries,
    create_fixture,
    parse_fields,
    read_listener_line,
)


NODE_ID = b"rstorrent-dht-node01"


@dataclass
class RunResult:
    elapsed_seconds: float
    info_hash: str
    metadata_size: int
    payload_hash: str
    find_node_queries: int
    get_peers_queries: int
    incoming_queries: int
    command_output: str
    cleanup_succeeded: bool = False


class ControlledDhtRouter:
    def __init__(self, info_hash: str, peer_port: int) -> None:
        self.info_hash = bytes.fromhex(info_hash)
        self.peer_port = peer_port
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(("127.0.0.1", 0))
        self.socket.settimeout(10)
        self.port = self.socket.getsockname()[1]
        self.find_node_queries = 0
        self.get_peers_queries = 0
        self.failure: BaseException | None = None
        self.thread = threading.Thread(
            target=self._serve,
            name=f"rstorrent-dht-router-{self.port}",
        )

    def start(self) -> None:
        self.thread.start()

    def join(self) -> None:
        self.thread.join(timeout=12)
        if self.thread.is_alive():
            raise ScenarioFailure("controlled DHT router did not terminate")
        if self.failure is not None:
            raise ScenarioFailure(f"controlled DHT router failed: {self.failure}")
        if self.find_node_queries < 1 or self.get_peers_queries < 1:
            raise ScenarioFailure(
                "controlled DHT router did not receive both find_node and get_peers"
            )

    def close(self) -> None:
        try:
            self.socket.close()
        except OSError:
            pass
        self.thread.join(timeout=2)

    def _response(self, transaction: bytes, client: tuple[str, int], *, peer: bool) -> bytes:
        response: dict[bytes, object] = {b"id": NODE_ID}
        if peer:
            response[b"token"] = b"fixture"
            response[b"values"] = [
                socket.inet_aton("127.0.0.1") + struct.pack("!H", self.peer_port)
            ]
        return bytes(
            lt.bencode(
                {
                    b"ip": socket.inet_aton(client[0]) + struct.pack("!H", client[1]),
                    b"r": response,
                    b"t": transaction,
                    b"y": b"r",
                }
            )
        )

    def _serve(self) -> None:
        try:
            while self.get_peers_queries == 0:
                packet, client = self.socket.recvfrom(2048)
                message = lt.bdecode(packet)
                if not isinstance(message, dict) or message.get(b"y") != b"q":
                    raise ScenarioFailure("controlled DHT received a non-query message")
                transaction = message.get(b"t")
                arguments = message.get(b"a")
                method = message.get(b"q")
                if not isinstance(transaction, bytes) or not 1 <= len(transaction) <= 8:
                    raise ScenarioFailure("DHT query transaction is missing or unbounded")
                if not isinstance(arguments, dict) or len(arguments.get(b"id", b"")) != 20:
                    raise ScenarioFailure("DHT query node ID is missing or malformed")

                if method == b"find_node":
                    self.find_node_queries += 1
                    if len(arguments.get(b"target", b"")) != 20:
                        raise ScenarioFailure("find_node target is malformed")
                    response = self._response(transaction, client, peer=False)
                elif method == b"get_peers":
                    self.get_peers_queries += 1
                    if arguments.get(b"info_hash") != self.info_hash:
                        raise ScenarioFailure("get_peers used the wrong info hash")
                    want = arguments.get(b"want", [])
                    if want and b"n4" not in want:
                        raise ScenarioFailure("get_peers did not request IPv4 nodes")
                    response = self._response(transaction, client, peer=True)
                else:
                    raise ScenarioFailure(f"unexpected DHT method {method!r}")

                stale = lt.bdecode(response)
                stale[b"t"] = bytes((transaction[0] ^ 0xFF,)) + transaction[1:]
                self.socket.sendto(bytes(lt.bencode(stale)), client)
                self.socket.sendto(response, client)
        except BaseException as error:
            self.failure = error
        finally:
            self.socket.close()


def trackerless_magnet(info_hash: str) -> str:
    return f"magnet:?xt=urn:btih:{info_hash}"


def krpc_query(transaction: bytes, method: bytes, arguments: dict[bytes, object]) -> bytes:
    return bytes(
        lt.bencode(
            {
                b"a": {b"id": b"independent-client01", **arguments},
                b"q": method,
                b"t": transaction,
                b"y": b"q",
            }
        )
    )


def exchange_query(
    client: socket.socket,
    address: tuple[str, int],
    transaction: bytes,
    method: bytes,
    arguments: dict[bytes, object],
) -> dict[bytes, object]:
    client.sendto(krpc_query(transaction, method, arguments), address)
    packet, source = client.recvfrom(2048)
    if source != address:
        raise ScenarioFailure("incoming-query response used the wrong endpoint")
    response = lt.bdecode(packet)
    if (
        not isinstance(response, dict)
        or response.get(b"y") != b"r"
        or response.get(b"t") != transaction
        or not isinstance(response.get(b"r"), dict)
        or len(response[b"r"].get(b"id", b"")) != 20
    ):
        raise ScenarioFailure(f"malformed incoming-query response: {response!r}")
    return response[b"r"]


def verify_incoming_queries(binary: Path, info_hash: str) -> int:
    process = subprocess.Popen(
        [str(binary), "--queries", "3"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    client = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    client.bind(("127.0.0.1", 0))
    client.settimeout(5)
    try:
        listener = parse_fields(read_listener_line(process, 5)).get("address")
        if not listener:
            raise ScenarioFailure("DHT diagnostic did not report its address")
        host, port = listener.rsplit(":", 1)
        address = (host, int(port))
        exchange_query(client, address, b"p1", b"ping", {})
        get_peers = exchange_query(
            client,
            address,
            b"g1",
            b"get_peers",
            {b"info_hash": bytes.fromhex(info_hash), b"want": [b"n4"]},
        )
        token = get_peers.get(b"token")
        if not isinstance(token, bytes) or not token:
            raise ScenarioFailure("RSTorrent get_peers response omitted its token")
        exchange_query(
            client,
            address,
            b"a1",
            b"announce_peer",
            {
                b"implied_port": 1,
                b"info_hash": bytes.fromhex(info_hash),
                b"port": client.getsockname()[1],
                b"token": token,
            },
        )
        stdout, stderr = process.communicate(timeout=5)
        if process.returncode != 0:
            raise ScenarioFailure(
                f"DHT diagnostic exited with {process.returncode}\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}"
            )
        fields = parse_fields(stdout)
        if fields.get("queries_received") != "3":
            raise ScenarioFailure("DHT diagnostic did not observe all incoming queries")
        return 3
    finally:
        client.close()
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)


def run_rstorrent_leech(binary: Path, dht_binary: Path, fixture: Fixture) -> RunResult:
    session = create_session()
    handle: lt.torrent_handle | None = None
    router: ControlledDhtRouter | None = None
    diagnostics: list[str] = []
    started = time.monotonic()
    try:
        peer_port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            fixture.torrent_info,
            fixture.seed_directory,
            diagnostics,
        )
        router = ControlledDhtRouter(fixture.info_hash, peer_port)
        router.start()
        output_root = fixture.torrent_path.parent / "dht-output"
        completed = subprocess.run(
            [
                str(binary),
                "--magnet",
                trackerless_magnet(fixture.info_hash),
                "--dht-bootstrap",
                f"127.0.0.1:{router.port}",
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
        router.join()
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        if completed.returncode != 0:
            raise ScenarioFailure(
                f"DHT magnet leech exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
        fields = parse_fields(completed.stdout)
        if fields.get("info_hash") != fixture.info_hash or fields.get("pieces") != "3/3":
            raise ScenarioFailure("DHT magnet leech reported the wrong identity or piece count")
        actual_hash = compare_payloads(fixture.payload_path, output_root / "payload.bin")
        if actual_hash != fixture.payload_hash:
            raise ScenarioFailure("DHT magnet payload differs from libtorrent seed")
        incoming_queries = verify_incoming_queries(dht_binary, fixture.info_hash)
        return RunResult(
            elapsed_seconds=time.monotonic() - started,
            info_hash=fixture.info_hash,
            metadata_size=len(fixture.info_bytes),
            payload_hash=fixture.payload_hash,
            find_node_queries=router.find_node_queries,
            get_peers_queries=router.get_peers_queries,
            incoming_queries=incoming_queries,
            command_output=completed.stdout.strip(),
        )
    finally:
        if router is not None:
            router.close()
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-dht-interop-"))
    failure: BaseException | None = None
    result: RunResult | None = None
    try:
        download_binary, _ = build_binaries(repository)
        dht_binary = repository / "target" / "debug" / "rstorrent-dht-node"
        if not dht_binary.is_file():
            raise ScenarioFailure("DHT diagnostic binary was not created")
        fixture = create_fixture(run_path)
        result = run_rstorrent_leech(download_binary, dht_binary, fixture)
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        failure = error
    finally:
        try:
            shutil.rmtree(run_path)
            cleanup_succeeded = not run_path.exists()
        except OSError as error:
            cleanup_succeeded = False
            if failure is None:
                failure = error
        if result is not None:
            result.cleanup_succeeded = cleanup_succeeded

    if failure is not None:
        print(failure, file=sys.stderr)
        return 1
    if result is None or not result.cleanup_succeeded:
        print("DHT interoperability run did not clean up", file=sys.stderr)
        return 1
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    print("discovery=dht:true,tracker:false transport=tcp loopback_only=true")
    print(
        f"metadata_size={result.metadata_size} "
        f"metadata_blocks={(result.metadata_size + METADATA_BLOCK_SIZE - 1) // METADATA_BLOCK_SIZE} "
        f"info_hash={result.info_hash} payload_sha1={result.payload_hash} "
        f"find_node_queries={result.find_node_queries} "
        f"get_peers_queries={result.get_peers_queries} "
        f"incoming_queries={result.incoming_queries} "
        f"elapsed_seconds={result.elapsed_seconds:.3f} cleanup=ok"
    )
    print(f"leech_report={result.command_output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
