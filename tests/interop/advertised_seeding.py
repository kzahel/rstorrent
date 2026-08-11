#!/usr/bin/env python3
"""Prove tracker-only and DHT-only discovery of a completed RSTorrent seed."""

from __future__ import annotations

import gc
import hashlib
import os
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Any

import libtorrent as lt

from first_verified_piece import ScenarioFailure
from upnp_external_seeding import (
    GateFailure,
    PROCESS_TIMEOUT,
    build_seed,
    create_fixture,
    discover_control,
    finish_remote,
    query_mapping,
    read_json_line,
    start_remote,
    stop_seed,
    terminate,
)
from utp_reference_oracle import OracleFailure, stats_snapshot


PROTOCOL_ID = 0x41727101980
CONNECTION_ID = 0x0102030405060708
ANNOUNCE_FORMAT = "!QII20s20sQQQIIIiH"
NODE_ID = b"rstorrent-dht-node01"
TRANSFER_TIMEOUT = 30


class ControlledUdpTracker:
    def __init__(self, info_hash: str) -> None:
        self.info_hash = bytes.fromhex(info_hash)
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(("127.0.0.1", 0))
        self.socket.settimeout(0.2)
        self.port = self.socket.getsockname()[1]
        self.seed_port: int | None = None
        self.seed_started = threading.Event()
        self.seed_stopped = threading.Event()
        self.leecher_announces = 0
        self.events: list[tuple[bool, int, int, bool]] = []
        self.failure: BaseException | None = None
        self.finished = threading.Event()
        self.thread = threading.Thread(target=self._serve, name="advertised-seed-tracker")

    @property
    def url(self) -> str:
        return f"udp://127.0.0.1:{self.port}/announce"

    def start(self) -> None:
        self.thread.start()

    def wait_seed_started(self) -> None:
        if not self.seed_started.wait(timeout=10):
            raise ScenarioFailure("RSTorrent did not send a tracker started announce")
        self.raise_failure()

    def wait_seed_port(self, expected: int) -> None:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            self.raise_failure()
            if self.seed_port == expected:
                return
            time.sleep(0.02)
        raise ScenarioFailure("tracker did not observe the selected mapped TCP port")

    def wait_seed_stopped(self) -> None:
        if not self.seed_stopped.wait(timeout=7):
            raise ScenarioFailure("RSTorrent did not send a bounded tracker stopped announce")
        self.raise_failure()

    def close(self) -> None:
        self.finished.set()
        self.thread.join(timeout=3)
        try:
            self.socket.close()
        except OSError:
            pass
        if self.thread.is_alive():
            raise ScenarioFailure("controlled tracker did not terminate")
        self.raise_failure()

    def raise_failure(self) -> None:
        if self.failure is not None:
            raise ScenarioFailure(f"controlled tracker failed: {self.failure}")

    def _serve(self) -> None:
        try:
            while not self.finished.is_set():
                try:
                    packet, client = self.socket.recvfrom(2048)
                except TimeoutError:
                    continue
                if len(packet) == 16:
                    protocol, action, transaction = struct.unpack("!QII", packet)
                    if protocol != PROTOCOL_ID or action != 0:
                        raise ScenarioFailure("tracker received an invalid connect request")
                    self.socket.sendto(
                        struct.pack("!IIQ", 0, transaction, CONNECTION_ID), client
                    )
                    continue
                announce_length = struct.calcsize(ANNOUNCE_FORMAT)
                if len(packet) < announce_length:
                    raise ScenarioFailure("tracker received an invalid announce length")
                (
                    connection,
                    action,
                    transaction,
                    info_hash,
                    peer_id,
                    _downloaded,
                    left,
                    _uploaded,
                    event,
                    _announced_ip,
                    key,
                    _num_want,
                    port,
                ) = struct.unpack(ANNOUNCE_FORMAT, packet[:announce_length])
                if connection != CONNECTION_ID or action != 1 or transaction == 0 or key == 0:
                    raise ScenarioFailure("tracker announce identity fields are invalid")
                if info_hash != self.info_hash:
                    raise ScenarioFailure("tracker announce used the wrong info hash")
                is_seed = peer_id.startswith(b"-RS0001-")
                if is_seed:
                    if len(packet) != announce_length:
                        raise ScenarioFailure("RSTorrent emitted unexpected tracker extensions")
                    if left != 0:
                        raise ScenarioFailure("completed RSTorrent seed did not report left=0")
                    if event in (0, 2) and port > 1:
                        self.seed_port = port
                        self.seed_started.set()
                    elif event == 3:
                        if port != self.seed_port:
                            raise ScenarioFailure("stopped announce changed the seed port")
                        self.seed_stopped.set()
                else:
                    self.leecher_announces += 1
                peers = b""
                if not is_seed and self.seed_port is not None:
                    peers = socket.inet_aton("127.0.0.1") + struct.pack("!H", self.seed_port)
                self.events.append((is_seed, event, port, bool(peers)))
                response = struct.pack("!IIIII", 1, transaction, 900, 0, 1) + peers
                self.socket.sendto(response, client)
        except BaseException as error:
            self.failure = error
            self.seed_started.set()
            self.seed_stopped.set()


class ControlledDhtRouter:
    def __init__(self, info_hash: str) -> None:
        self.info_hash = bytes.fromhex(info_hash)
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(("127.0.0.1", 0))
        self.socket.settimeout(0.2)
        self.port = self.socket.getsockname()[1]
        self.seed_port: int | None = None
        self.seed_client: tuple[str, int] | None = None
        self.seed_announced = threading.Event()
        self.get_peers_queries = 0
        self.announce_queries = 0
        self.failure: BaseException | None = None
        self.finished = threading.Event()
        self.thread = threading.Thread(target=self._serve, name="advertised-seed-dht")

    def start(self) -> None:
        self.thread.start()

    def wait_seed_announced(self) -> None:
        if not self.seed_announced.wait(timeout=10):
            raise ScenarioFailure("RSTorrent did not self-announce through DHT")
        self.raise_failure()

    def wait_seed_port(self, expected: int) -> None:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            self.raise_failure()
            if self.seed_port == expected:
                return
            time.sleep(0.02)
        raise ScenarioFailure("DHT did not observe the selected UDP/uTP port")

    def close(self) -> None:
        self.finished.set()
        self.thread.join(timeout=3)
        try:
            self.socket.close()
        except OSError:
            pass
        if self.thread.is_alive():
            raise ScenarioFailure("controlled DHT router did not terminate")
        self.raise_failure()

    def raise_failure(self) -> None:
        if self.failure is not None:
            raise ScenarioFailure(f"controlled DHT router failed: {self.failure}")

    def _response(
        self,
        transaction: bytes,
        client: tuple[str, int],
        values: list[bytes] | None = None,
        token: bytes | None = None,
    ) -> bytes:
        body: dict[bytes, object] = {b"id": NODE_ID}
        if values:
            body[b"values"] = values
        if token is not None:
            body[b"token"] = token
        return bytes(
            lt.bencode(
                {
                    b"ip": socket.inet_aton(client[0]) + struct.pack("!H", client[1]),
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
                    raise ScenarioFailure("DHT router received a malformed query")
                if method in (b"ping", b"find_node"):
                    response = self._response(transaction, client)
                elif method == b"get_peers":
                    if arguments.get(b"info_hash") != self.info_hash:
                        response = self._response(transaction, client, token=b"fixture")
                    else:
                        self.get_peers_queries += 1
                        values = None
                        if self.seed_port is not None:
                            values = [
                                socket.inet_aton("127.0.0.1")
                                + struct.pack("!H", self.seed_port)
                            ]
                        response = self._response(
                            transaction, client, values=values, token=b"fixture"
                        )
                elif method == b"announce_peer":
                    if arguments.get(b"info_hash") != self.info_hash:
                        raise ScenarioFailure("DHT announce used the wrong info hash")
                    if arguments.get(b"token") != b"fixture":
                        raise ScenarioFailure("DHT announce did not reuse the node token")
                    port = arguments.get(b"port")
                    if not isinstance(port, int) or port <= 1:
                        raise ScenarioFailure("DHT announce omitted the explicit UDP/uTP port")
                    if self.seed_client is None:
                        if arguments.get(b"implied_port", 0) != 0:
                            raise ScenarioFailure(
                                "RSTorrent DHT announce unexpectedly implied the UDP port"
                            )
                        self.seed_client = client
                        self.seed_port = port
                        self.announce_queries += 1
                        self.seed_announced.set()
                    response = self._response(transaction, client)
                else:
                    continue
                self.socket.sendto(response, client)
        except BaseException as error:
            self.failure = error
            self.seed_announced.set()


def start_seed(
    binary: Path,
    fixture: dict[str, object],
    extra_arguments: list[str],
) -> tuple[subprocess.Popen[str], dict[str, Any]]:
    process = subprocess.Popen(
        [
            str(binary),
            "--profile-root",
            str(fixture["profile"]),
            "--storage-root",
            str(fixture["storage"]),
            "--metainfo",
            str(fixture["torrent"]),
            *extra_arguments,
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        ready = read_json_line(process, PROCESS_TIMEOUT)
        if ready.get("event") != "ready" or ready.get("registrations") != 1:
            raise ScenarioFailure("RSTorrent seed did not become incoming-routable")
        return process, ready
    except BaseException:
        terminate(process)
        raise


def tracker_torrent(source: Path, destination: Path, tracker_url: str) -> lt.torrent_info:
    metainfo = lt.bdecode(source.read_bytes())
    if not isinstance(metainfo, dict):
        raise ScenarioFailure("fixture metainfo is not a dictionary")
    metainfo[b"announce"] = tracker_url.encode()
    destination.write_bytes(bytes(lt.bencode(metainfo)))
    return lt.torrent_info(str(destination))


def leech(
    torrent_info: lt.torrent_info,
    output_root: Path,
    expected_sha256: str,
    *,
    dht_router: ControlledDhtRouter | None,
    transport: str,
) -> tuple[dict[str, int], int]:
    if transport not in {"tcp", "utp"}:
        raise ScenarioFailure(f"unknown leecher transport {transport}")
    force_utp = transport == "utp"
    settings: dict[str, object] = {
        "listen_interfaces": "127.0.0.1:0",
        "enable_dht": dht_router is not None,
        "enable_lsd": False,
        "enable_upnp": False,
        "enable_natpmp": False,
        "enable_incoming_utp": force_utp,
        "enable_outgoing_utp": force_utp,
        # libtorrent gates torrent DHT announces (and their peer-result
        # callback) on having a listening socket, even for this leecher-only
        # fixture.
        "enable_incoming_tcp": dht_router is not None and not force_utp,
        "enable_outgoing_tcp": not force_utp,
        "alert_queue_size": 1000,
    }
    if dht_router is not None:
        # A one-node loopback DHT is deliberately unlike the public network.
        # Disable libtorrent's anti-sybil subnet filters so the controlled
        # router can independently return the loopback RSTorrent seed.
        settings.update(
            {
                "dht_restrict_routing_ips": False,
                "dht_restrict_search_ips": False,
                "dht_ignore_dark_internet": False,
                "allow_multiple_connections_per_ip": True,
                "alert_mask": int(lt.alert.category_t.all_categories),
            }
        )
    session = lt.session(settings)
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    peer_high_water = 0
    try:
        if dht_router is not None:
            session.add_dht_node(("127.0.0.1", dht_router.port))
        parameters = lt.add_torrent_params()
        parameters.ti = torrent_info
        parameters.save_path = str(output_root)
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        handle = session.add_torrent(parameters)
        deadline = time.monotonic() + TRANSFER_TIMEOUT
        next_dht_announce = time.monotonic()
        while time.monotonic() < deadline:
            if dht_router is not None and time.monotonic() >= next_dht_announce:
                # The first call can race libtorrent's asynchronous file check.
                # Repeat while bounded by the transfer deadline.
                handle.force_dht_announce()
                next_dht_announce = time.monotonic() + 1
            diagnostics.extend(alert.message() for alert in session.pop_alerts())
            status = handle.status()
            peer_high_water = max(peer_high_water, len(handle.get_peer_info()))
            if status.errc.value() != 0:
                raise ScenarioFailure(f"libtorrent leecher failed: {status.errc.message()}")
            if status.is_seeding:
                break
            time.sleep(0.02)
        else:
            raise ScenarioFailure(
                "libtorrent did not discover and complete from RSTorrent\n"
                + "\n".join(diagnostics[-50:])
            )
        payload = output_root / "external-seed.bin"
        if hashlib.sha256(payload.read_bytes()).hexdigest() != expected_sha256:
            raise ScenarioFailure("libtorrent payload failed the fixture SHA-256 check")
        try:
            stats = stats_snapshot(session, diagnostics, deadline)
        except OracleFailure as error:
            raise ScenarioFailure(str(error)) from error
        if force_utp:
            if stats["peer.num_tcp_peers"] != 0:
                raise ScenarioFailure("DHT-only leecher observed a TCP peer")
            if stats["utp.utp_packets_in"] <= 0 or stats["utp.utp_packets_out"] <= 0:
                raise ScenarioFailure("DHT-only leecher lacked bidirectional uTP packets")
            if peer_high_water != 1:
                raise ScenarioFailure(
                    f"DHT-only leecher peer high water was {peer_high_water}, expected 1"
                )
        return stats, peer_high_water
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def run_tracker(binary: Path, root: Path) -> tuple[int, int]:
    root.mkdir(parents=True)
    fixture = create_fixture(root)
    tracker = ControlledUdpTracker(str(fixture["info_hash"]))
    tracker.start()
    seed: subprocess.Popen[str] | None = None
    try:
        seed, _ready = start_seed(binary, fixture, ["--tracker", tracker.url])
        tracker.wait_seed_started()
        torrent_info = tracker_torrent(
            Path(str(fixture["torrent"])), root / "tracker-leecher.torrent", tracker.url
        )
        try:
            leech(
                torrent_info,
                root / "tracker-output",
                str(fixture["payload_sha256"]),
                dht_router=None,
                transport="tcp",
            )
        except ScenarioFailure as error:
            raise ScenarioFailure(f"{error}\ntracker_events={tracker.events[-20:]}") from error
        stop_seed(seed)
        seed = None
        tracker.wait_seed_stopped()
        if tracker.leecher_announces < 1:
            raise ScenarioFailure("tracker-only leecher never announced to the tracker")
        return tracker.leecher_announces, tracker.seed_port or 0
    finally:
        if seed is not None:
            terminate(seed)
        tracker.close()


def endpoint_port(value: object, field: str) -> int:
    if not isinstance(value, str):
        raise ScenarioFailure(f"seed readiness omitted {field}")
    _host, separator, port_text = value.rpartition(":")
    if separator != ":":
        raise ScenarioFailure(f"seed readiness has malformed {field}")
    try:
        port = int(port_text)
    except ValueError as error:
        raise ScenarioFailure(f"seed readiness has malformed {field}") from error
    if not 1 < port <= 65535:
        raise ScenarioFailure(f"seed readiness has invalid {field}")
    return port


def run_dht(binary: Path, root: Path) -> tuple[int, int, int, int]:
    root.mkdir(parents=True)
    fixture = create_fixture(root)
    router = ControlledDhtRouter(str(fixture["info_hash"]))
    router.start()
    seed: subprocess.Popen[str] | None = None
    try:
        seed, ready = start_seed(
            binary,
            fixture,
            ["--utp", "--dht-bootstrap", f"127.0.0.1:{router.port}"],
        )
        router.wait_seed_announced()
        utp_port = endpoint_port(ready.get("utp_listen"), "utp_listen")
        router.wait_seed_port(utp_port)
        try:
            stats, peer_high_water = leech(
                lt.torrent_info(str(fixture["torrent"])),
                root / "dht-output",
                str(fixture["payload_sha256"]),
                dht_router=router,
                transport="utp",
            )
        except ScenarioFailure as error:
            raise ScenarioFailure(
                f"{error}\ndht_get_peers={router.get_peers_queries} "
                f"dht_announces={router.announce_queries} seed_port={router.seed_port}"
            ) from error
        stopped = stop_seed(seed)
        seed = None
        utp = stopped.get("utp_before_shutdown")
        if not isinstance(utp, dict) or utp.get("connection_high_water") != 1:
            raise ScenarioFailure("RSTorrent did not observe exactly one incoming uTP connection")
        return (
            router.get_peers_queries,
            router.seed_port or 0,
            peer_high_water,
            stats["peer.num_tcp_peers"],
        )
    finally:
        if seed is not None:
            terminate(seed)
        router.close()


def run_mapped_external(
    repository: Path, binary: Path, root: Path
) -> tuple[int, int, int]:
    target = os.environ.get("RSTORRENT_OFF_LAN_SSH_TARGET")
    if not target:
        raise GateFailure("off-LAN SSH target is not configured")
    root.mkdir(parents=True)
    fixture = create_fixture(root)
    tracker = ControlledUdpTracker(str(fixture["info_hash"]))
    router = ControlledDhtRouter(str(fixture["info_hash"]))
    tracker.start()
    router.start()
    seed: subprocess.Popen[str] | None = None
    remote_processes: list[subprocess.Popen[str]] = []
    try:
        seed, ready = start_seed(
            binary,
            fixture,
            [
                "--upnp",
                "--tracker",
                tracker.url,
                "--dht-bootstrap",
                f"127.0.0.1:{router.port}",
            ],
        )
        mapping = ready.get("mapping")
        if not isinstance(mapping, dict) or mapping.get("type") != "mapped":
            raise ScenarioFailure("seed did not publish mapped readiness")
        local_address = mapping.get("local_address")
        local_port = mapping.get("local_port")
        external_address = mapping.get("external_address")
        external_port = mapping.get("external_port")
        if not (
            isinstance(local_address, str)
            and isinstance(local_port, int)
            and isinstance(external_address, str)
            and isinstance(external_port, int)
        ):
            raise ScenarioFailure("mapped readiness fields are invalid")

        tracker.wait_seed_port(external_port)
        router.wait_seed_port(external_port)
        control, service = discover_control(local_address)
        installed = query_mapping(control, service, external_port)
        if installed is None or not (
            installed["NewInternalClient"] == local_address
            and int(installed["NewInternalPort"]) == local_port
            and installed["NewEnabled"] == "1"
        ):
            raise ScenarioFailure("independent query did not verify the mapping")

        remote_source = (repository / "tests/interop/off_lan_peer_wire.py").read_text()
        remote = start_remote(
            target,
            remote_source,
            {
                "host": external_address,
                # This is deliberately the tracker-wire observation, with
                # equality against the DHT-wire observation checked above.
                "port": tracker.seed_port,
                "info_hash": fixture["info_hash"],
                "total_length": fixture["total_length"],
                "piece_length": 16 * 1024,
                "piece_hashes": fixture["piece_hashes"],
                "payload_sha256": fixture["payload_sha256"],
                "hold_seconds": 0,
            },
        )
        remote_processes.append(remote)
        result = finish_remote(remote, "verified")
        if (
            result.get("bytes") != fixture["total_length"]
            or result.get("sha256") != fixture["payload_sha256"]
        ):
            raise ScenarioFailure("off-LAN payload did not verify")

        stopped = stop_seed(seed)
        seed = None
        tracker.wait_seed_stopped()
        if query_mapping(control, service, external_port) is not None:
            raise ScenarioFailure("mapping survived joined shutdown")
        unreachable = start_remote(
            target,
            remote_source,
            {
                "host": external_address,
                "port": tracker.seed_port,
                "expect_connect_failure": True,
            },
        )
        remote_processes.append(unreachable)
        finish_remote(unreachable, "unreachable")
        if not (
            stopped.get("mapping_tasks_after_shutdown") == 0
            and stopped.get("mappings_after_shutdown") == 0
        ):
            raise ScenarioFailure("mapping owners were not terminal at shutdown")
        return external_port, tracker.leecher_announces, router.announce_queries
    finally:
        for remote in remote_processes:
            terminate(remote)
        if seed is not None:
            try:
                stop_seed(seed)
            except BaseException:
                terminate(seed)
        tracker.close()
        router.close()


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    root = Path(tempfile.mkdtemp(prefix="rstorrent-advertised-seeding-"))
    try:
        binary = build_seed(repository)
        mapped_external = "--mapped-external" in sys.argv[1:]
        if any(argument != "--mapped-external" for argument in sys.argv[1:]):
            raise ScenarioFailure("unknown argument")
        tracker_announces, tracker_port = run_tracker(binary, root / "tracker")
        dht_queries, dht_port, dht_peer_high_water, dht_tcp_peers = run_dht(
            binary, root / "dht"
        )
        mapped_result = (
            run_mapped_external(repository, binary, root / "mapped")
            if mapped_external
            else None
        )
        print(f"libtorrent_binding_version={lt.__version__}")
        print(f"libtorrent_native_version={lt.version}")
        print(
            "tracker_only_tcp=verified dht_only_utp=verified "
            "explicit_peer_hint=false payload_sha256=verified"
        )
        print(
            f"tracker_leecher_announces={tracker_announces} "
            f"tracker_seed_port_nonzero={tracker_port > 1}"
        )
        print(
            f"dht_get_peers_queries={dht_queries} "
            f"dht_seed_port_nonzero={dht_port > 1} "
            f"dht_peer_high_water={dht_peer_high_water} "
            f"dht_tcp_peers={dht_tcp_peers}"
        )
        if mapped_result is not None:
            mapped_port, mapped_tracker_announces, mapped_dht_announces = mapped_result
            print(
                "mapped_external=verified tracker_wire_port_matches=true "
                "dht_wire_port_matches=true off_lan_payload_sha256=verified "
                "post_shutdown_unreachable=true"
            )
            print(
                f"mapped_port_nonzero={mapped_port > 1} "
                f"mapped_tracker_leecher_announces={mapped_tracker_announces} "
                f"mapped_dht_announces={mapped_dht_announces}"
            )
        return 0
    except (GateFailure, OSError, ScenarioFailure, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    finally:
        shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
