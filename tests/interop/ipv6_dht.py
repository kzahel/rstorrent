#!/usr/bin/env python3
"""Exercise IPv6 peer transfer and BEP 32 DHT against pinned libtorrent."""

from __future__ import annotations

import gc
import hashlib
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

import libtorrent as lt

from advertised_seeding import start_seed, stop_seed
from first_verified_piece import (
    DEFAULT_PAYLOAD_ALLOWANCE,
    ScenarioFailure,
    add_seed,
    compare_payloads,
    wait_for_listener,
)
from magnet_metadata import (
    METADATA_TIMEOUT_SECONDS,
    PROCESS_TIMEOUT_SECONDS,
    build_binaries,
    create_fixture as create_download_fixture,
    parse_fields,
    read_listener_line,
)
from upnp_external_seeding import build_seed, create_fixture as create_seed_fixture


NODE_ID = b"rstorrent-dht-node01"
TRANSFER_TIMEOUT_SECONDS = 30


def ipv6_session(*, dht: bool) -> lt.session:
    settings: dict[str, object] = {
        "listen_interfaces": "[::1]:0",
        "enable_dht": dht,
        "enable_lsd": False,
        "enable_upnp": False,
        "enable_natpmp": False,
        "enable_incoming_utp": False,
        "enable_outgoing_utp": False,
        "enable_incoming_tcp": True,
        "enable_outgoing_tcp": True,
        "alert_queue_size": 1000,
    }
    if dht:
        settings.update(
            {
                "dht_restrict_routing_ips": False,
                "dht_restrict_search_ips": False,
                "dht_ignore_dark_internet": False,
                "allow_multiple_connections_per_ip": True,
                "alert_mask": int(lt.alert.category_t.all_categories),
            }
        )
    return lt.session(settings)


class Ipv6DhtRouter:
    def __init__(self, info_hash: str) -> None:
        self.info_hash = bytes.fromhex(info_hash)
        self.socket = socket.socket(socket.AF_INET6, socket.SOCK_DGRAM)
        self.socket.bind(("::1", 0))
        self.socket.settimeout(0.2)
        self.port = self.socket.getsockname()[1]
        self.seed_port: int | None = None
        self.seed_announced = threading.Event()
        self.find_node_queries = 0
        self.get_peers_queries = 0
        self.announce_queries = 0
        self.same_family_want_omissions = 0
        self.failure: BaseException | None = None
        self.finished = threading.Event()
        self.thread = threading.Thread(target=self._serve, name="ipv6-dht-router")

    @property
    def address(self) -> str:
        return f"[::1]:{self.port}"

    def start(self) -> None:
        self.thread.start()

    def wait_seed_announced(self) -> None:
        if not self.seed_announced.wait(timeout=10):
            raise ScenarioFailure("RSTorrent did not announce on the IPv6 DHT")
        self.raise_failure()

    def raise_failure(self) -> None:
        if self.failure is not None:
            raise ScenarioFailure(f"IPv6 DHT router failed: {self.failure}")

    def close(self) -> None:
        self.finished.set()
        self.thread.join(timeout=3)
        try:
            self.socket.close()
        except OSError:
            pass
        if self.thread.is_alive():
            raise ScenarioFailure("IPv6 DHT router did not terminate")
        self.raise_failure()

    def _response(
        self,
        transaction: bytes,
        client: tuple[str, int, int, int],
        *,
        token: bytes | None = None,
        include_seed: bool = False,
    ) -> bytes:
        body: dict[bytes, object] = {b"id": NODE_ID}
        if token is not None:
            body[b"token"] = token
        if include_seed and self.seed_port is not None:
            body[b"values"] = [
                socket.inet_pton(socket.AF_INET6, "::1")
                + struct.pack("!H", self.seed_port)
            ]
        return bytes(
            lt.bencode(
                {
                    b"ip": socket.inet_pton(socket.AF_INET6, client[0])
                    + struct.pack("!H", client[1]),
                    b"r": body,
                    b"t": transaction,
                    b"y": b"r",
                }
            )
        )

    def _serve(self) -> None:
        try:
            while not self.finished.is_set():
                try:
                    packet, client = self.socket.recvfrom(2048)
                except TimeoutError:
                    continue
                message = lt.bdecode(packet)
                if not isinstance(message, dict) or message.get(b"y") != b"q":
                    continue
                transaction = message.get(b"t")
                arguments = message.get(b"a")
                method = message.get(b"q")
                if not isinstance(transaction, bytes) or not isinstance(arguments, dict):
                    raise ScenarioFailure("IPv6 DHT router received a malformed query")
                if method in (b"find_node", b"get_peers"):
                    if b"want" in arguments:
                        raise ScenarioFailure(
                            "native IPv6 DHT query unexpectedly carried want"
                        )
                    self.same_family_want_omissions += 1
                if method == b"ping":
                    response = self._response(transaction, client)
                elif method == b"find_node":
                    self.find_node_queries += 1
                    response = self._response(transaction, client)
                elif method == b"get_peers":
                    if arguments.get(b"info_hash") == self.info_hash:
                        self.get_peers_queries += 1
                    response = self._response(
                        transaction,
                        client,
                        token=b"ipv6-fixture",
                        include_seed=arguments.get(b"info_hash") == self.info_hash,
                    )
                elif method == b"announce_peer":
                    if arguments.get(b"info_hash") != self.info_hash:
                        response = self._response(transaction, client)
                    else:
                        if arguments.get(b"token") != b"ipv6-fixture":
                            raise ScenarioFailure("IPv6 announce did not reuse its token")
                        if arguments.get(b"implied_port", 0) != 0:
                            raise ScenarioFailure("IPv6 announce unexpectedly implied UDP port")
                        port = arguments.get(b"port")
                        if not isinstance(port, int) or port <= 1:
                            raise ScenarioFailure("IPv6 announce omitted the TCP port")
                        if self.seed_port is None:
                            self.seed_port = port
                            self.seed_announced.set()
                        self.announce_queries += 1
                        response = self._response(transaction, client)
                else:
                    continue
                self.socket.sendto(response, client)
        except BaseException as error:
            self.failure = error
            self.seed_announced.set()


def run_direct_ipv6_download(binary: Path, root: Path) -> tuple[str, float]:
    fixture = create_download_fixture(root)
    session = ipv6_session(dht=False)
    handle: lt.torrent_handle | None = None
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
        output_root = root / "direct-output"
        completed = subprocess.run(
            [
                str(binary),
                "--metainfo",
                str(fixture.torrent_path),
                "--peer",
                f"[::1]:{peer_port}",
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
        if completed.returncode != 0:
            raise ScenarioFailure(
                "IPv6 direct download failed\n"
                f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
            )
        fields = parse_fields(completed.stdout)
        if fields.get("info_hash") != fixture.info_hash or fields.get("pieces") != "3/3":
            raise ScenarioFailure("IPv6 direct download reported the wrong torrent")
        payload_hash = compare_payloads(fixture.payload_path, output_root / "payload.bin")
        if payload_hash != fixture.payload_hash:
            raise ScenarioFailure("IPv6 direct download payload did not verify")
        return payload_hash, time.monotonic() - started
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def run_libtorrent_ipv6_dht_leech(
    torrent: Path,
    output_root: Path,
    expected_sha256: str,
    router: Ipv6DhtRouter,
) -> None:
    session = ipv6_session(dht=True)
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    try:
        session.add_dht_node(("::1", router.port))
        parameters = lt.add_torrent_params()
        parameters.ti = lt.torrent_info(str(torrent))
        parameters.save_path = str(output_root)
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        handle = session.add_torrent(parameters)
        deadline = time.monotonic() + TRANSFER_TIMEOUT_SECONDS
        next_announce = time.monotonic()
        while time.monotonic() < deadline:
            if time.monotonic() >= next_announce:
                handle.force_dht_announce()
                next_announce = time.monotonic() + 1
            diagnostics.extend(alert.message() for alert in session.pop_alerts())
            status = handle.status()
            if status.errc.value() != 0:
                raise ScenarioFailure(f"libtorrent IPv6 DHT leecher failed: {status.errc.message()}")
            if status.is_seeding:
                break
            time.sleep(0.02)
        else:
            raise ScenarioFailure(
                "libtorrent did not discover the RSTorrent IPv6 seed\n"
                + "\n".join(diagnostics[-50:])
            )
        payload = output_root / "external-seed.bin"
        if hashlib.sha256(payload.read_bytes()).hexdigest() != expected_sha256:
            raise ScenarioFailure("libtorrent IPv6 DHT payload did not verify")
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def run_dht_advertised_seed(binary: Path, root: Path) -> tuple[int, int, int]:
    root.mkdir(parents=True)
    fixture = create_seed_fixture(root)
    router = Ipv6DhtRouter(str(fixture["info_hash"]))
    router.start()
    seed: subprocess.Popen[str] | None = None
    try:
        seed, ready = start_seed(
            binary,
            fixture,
            ["--dht-bootstrap", router.address],
        )
        router.wait_seed_announced()
        listen = ready.get("listen")
        if not isinstance(listen, str):
            raise ScenarioFailure("RSTorrent seed omitted its listen endpoint")
        expected_port = int(listen.rsplit(":", 1)[1])
        if router.seed_port != expected_port:
            raise ScenarioFailure("IPv6 DHT announce carried the wrong family port")
        run_libtorrent_ipv6_dht_leech(
            Path(str(fixture["torrent"])),
            root / "dht-output",
            str(fixture["payload_sha256"]),
            router,
        )
        stop_seed(seed)
        seed = None
        if router.find_node_queries < 1 or router.get_peers_queries < 1:
            raise ScenarioFailure("IPv6 DHT did not complete bootstrap and traversal")
        return (
            router.find_node_queries,
            router.get_peers_queries,
            router.announce_queries,
        )
    finally:
        if seed is not None:
            stop_seed(seed)
        router.close()


def krpc_query(
    transaction: bytes,
    method: bytes,
    arguments: dict[bytes, object],
) -> bytes:
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
    if source[0] != address[0] or source[1] != address[1]:
        raise ScenarioFailure("DHT response used the wrong address-family endpoint")
    message = lt.bdecode(packet)
    if (
        not isinstance(message, dict)
        or message.get(b"y") != b"r"
        or message.get(b"t") != transaction
        or not isinstance(message.get(b"r"), dict)
    ):
        raise ScenarioFailure(f"malformed IPv6 DHT response: {message!r}")
    return message[b"r"]


def parse_endpoint(value: str) -> tuple[str, int]:
    if value.startswith("["):
        host, port = value[1:].rsplit("]:", 1)
        return host, int(port)
    host, port = value.rsplit(":", 1)
    return host, int(port)


def verify_incoming_bep32(binary: Path, info_hash: str) -> int:
    process = subprocess.Popen(
        [str(binary), "--family", "dual", "--queries", "8"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    ipv4 = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    ipv6 = socket.socket(socket.AF_INET6, socket.SOCK_DGRAM)
    ipv4.bind(("127.0.0.1", 0))
    ipv6.bind(("::1", 0))
    ipv4.settimeout(5)
    ipv6.settimeout(5)
    try:
        fields = parse_fields(read_listener_line(process, 5))
        v4_address = parse_endpoint(fields["address_ipv4"])
        v6_address = parse_endpoint(fields["address_ipv6"])
        exchange_query(ipv4, v4_address, b"p4", b"ping", {})
        exchange_query(ipv6, v6_address, b"p6", b"ping", {})
        target = {b"target": bytes.fromhex(info_hash)}
        receiving = exchange_query(ipv6, v6_address, b"f0", b"find_node", target)
        if b"nodes" in receiving:
            raise ScenarioFailure("want-less IPv6 find_node did not select its own table")
        requested_v4 = exchange_query(
            ipv6,
            v6_address,
            b"f4",
            b"find_node",
            {**target, b"want": [b"n4"]},
        )
        if b"nodes6" in requested_v4:
            raise ScenarioFailure("IPv6 find_node did not honor want n4")
        requested_v6 = exchange_query(
            ipv6,
            v6_address,
            b"f6",
            b"find_node",
            {**target, b"want": [b"n6"]},
        )
        if b"nodes" in requested_v6:
            raise ScenarioFailure("IPv6 find_node did not honor want n6")
        requested_both = exchange_query(
            ipv6,
            v6_address,
            b"fb",
            b"find_node",
            {**target, b"want": [b"n4", b"n6"]},
        )
        if not isinstance(requested_both.get(b"id"), bytes):
            raise ScenarioFailure("IPv6 find_node rejected both want tokens")
        get_peers = exchange_query(
            ipv6,
            v6_address,
            b"gp",
            b"get_peers",
            {b"info_hash": bytes.fromhex(info_hash)},
        )
        token = get_peers.get(b"token")
        if not isinstance(token, bytes) or not token:
            raise ScenarioFailure("IPv6 get_peers response omitted its token")
        exchange_query(
            ipv6,
            v6_address,
            b"ap",
            b"announce_peer",
            {
                b"implied_port": 1,
                b"info_hash": bytes.fromhex(info_hash),
                b"port": ipv6.getsockname()[1],
                b"token": token,
            },
        )
        stdout, stderr = process.communicate(timeout=5)
        if process.returncode != 0:
            raise ScenarioFailure(
                f"dual-stack DHT diagnostic exited with {process.returncode}\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}"
            )
        if parse_fields(stdout).get("queries_received") != "8":
            raise ScenarioFailure("dual-stack DHT diagnostic missed incoming queries")
        return 8
    finally:
        ipv4.close()
        ipv6.close()
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    root = Path(tempfile.mkdtemp(prefix="rstorrent-ipv6-dht-interop-"))
    try:
        download_binary, _ = build_binaries(repository)
        dht_binary = repository / "target/debug/rstorrent-dht-node"
        seed_binary = build_seed(repository)
        payload_hash, direct_seconds = run_direct_ipv6_download(
            download_binary, root / "direct"
        )
        find_nodes, get_peers, announces = run_dht_advertised_seed(
            seed_binary, root / "advertised"
        )
        incoming_queries = verify_incoming_bep32(dht_binary, "11" * 20)
        print(f"libtorrent_binding_version={lt.__version__}")
        print(f"libtorrent_native_version={lt.version}")
        print(
            "ipv6_direct_download=verified ipv6_dht_discovery=verified "
            "payload_hashes=verified"
        )
        print(
            f"direct_payload_sha1={payload_hash} direct_seconds={direct_seconds:.3f} "
            f"find_node_queries={find_nodes} get_peers_queries={get_peers} "
            f"announce_queries={announces} incoming_bep32_queries={incoming_queries}"
        )
        return 0
    except (OSError, ScenarioFailure, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    finally:
        shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
