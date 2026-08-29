#!/usr/bin/env python3
"""Prove hybrid dual-swarm runtime roles against pinned libtorrent."""

from __future__ import annotations

import argparse
import gc
import json
import resource
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

from application_surface_harness import start_gateway, stop_gateway
from bep52_metainfo_oracle import BLOCK, SourceFile, hybrid_fixture
from first_verified_piece import DEFAULT_PAYLOAD_ALLOWANCE, ScenarioFailure
from incoming_seeding import parse_address, read_json_line, terminate_process
from mse_peer_encryption import TcpProxy, assert_successful_wire_shape
from pure_v2_runtime import (
    PROCESS_TIMEOUT_SECONDS,
    TRANSFER_TIMEOUT_SECONDS,
    PlaintextBep52Proxy,
    RuntimeFixture,
    added_torrent_id,
    application_command,
    available_tcp_port,
    compare_files,
    compare_libtorrent_files,
    deterministic_bytes,
    libtorrent_session,
    observed_encryption,
    start_libtorrent_seed,
    stop_rstorrent_download,
    stop_rstorrent_seed,
    validate_libtorrent_utp,
    wait_application_complete,
    wait_rstorrent_download,
)
from udp_tracker_magnet import (
    ANNOUNCE_ACTION,
    ANNOUNCE_FORMAT,
    CONNECTION_ID,
    CONNECT_ACTION,
    PROTOCOL_ID,
)


SKIPPED_FILE = 3
SELECTED_FILES = frozenset({0, 1, 2, 4, 5})
NODE_ID = b"rstorrent-hybrid-dht"


class DualIdentityUdpTracker:
    """Return one peer only after observing both exact hybrid announces."""

    def __init__(self, fixture: RuntimeFixture, peer_port: int) -> None:
        self.expected = {
            bytes.fromhex(fixture.wire_info_hash),
            bytes.fromhex(fixture.full_info_hash[:40]),
        }
        self.expected_left = fixture.total_size
        self.peer_port = peer_port
        self.announces: dict[str, int] = {}
        self.lifecycle: dict[str, list[int]] = {}
        self.requests = 0
        self.failure: BaseException | None = None
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(("127.0.0.1", 0))
        self.socket.settimeout(10)
        self.port = self.socket.getsockname()[1]
        self.thread = threading.Thread(
            target=self._serve,
            name=f"rstorrent-hybrid-tracker-{self.port}",
        )

    def start(self) -> None:
        self.thread.start()

    def join(self) -> None:
        self.thread.join(timeout=12)
        if self.thread.is_alive():
            raise ScenarioFailure("hybrid UDP tracker did not terminate")
        if self.failure is not None:
            raise ScenarioFailure(f"hybrid UDP tracker failed: {self.failure}")
        if set(self.announces) != {value.hex() for value in self.expected}:
            raise ScenarioFailure(
                f"hybrid UDP tracker missed an identity: {self.announces}"
            )

    def close(self) -> None:
        try:
            self.socket.close()
        except OSError:
            pass
        self.thread.join(timeout=2)

    def _serve(self) -> None:
        try:
            while set(self.announces) != {value.hex() for value in self.expected}:
                packet, client = self.socket.recvfrom(2048)
                self.requests += 1
                if len(packet) == 16:
                    protocol_id, action, transaction = struct.unpack("!QII", packet)
                    if protocol_id != PROTOCOL_ID or action != CONNECT_ACTION:
                        raise ScenarioFailure("hybrid tracker received an invalid connect")
                    self.socket.sendto(
                        struct.pack("!IIQ", CONNECT_ACTION, transaction, CONNECTION_ID),
                        client,
                    )
                    continue
                if len(packet) != struct.calcsize(ANNOUNCE_FORMAT):
                    raise ScenarioFailure(
                        f"hybrid tracker received {len(packet)} unexpected bytes"
                    )
                (
                    connection_id,
                    action,
                    transaction,
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
                ) = struct.unpack(ANNOUNCE_FORMAT, packet)
                if (
                    connection_id != CONNECTION_ID
                    or action != ANNOUNCE_ACTION
                    or info_hash not in self.expected
                    or not peer_id.startswith(b"-RS0001-")
                    or downloaded + left != self.expected_left
                    or uploaded != 0
                    or event not in (0, 1, 2)
                    or announced_ip != 0
                    or key == 0
                    or num_want != 200
                    or listen_port == 0
                ):
                    raise ScenarioFailure(
                        "hybrid tracker announce fields changed: "
                        f"connection={connection_id} action={action} "
                        f"identity={info_hash.hex()} peer_id={peer_id!r} "
                        f"counters={(downloaded, left, uploaded)} "
                        f"expected_left={self.expected_left} event={event} "
                        f"ip={announced_ip} key={key} want={num_want} "
                        f"port={listen_port}"
                    )
                identity = info_hash.hex()
                self.announces[identity] = self.announces.get(identity, 0) + 1
                self.lifecycle.setdefault(identity, []).append(event)
                response = struct.pack(
                    "!IIIII",
                    ANNOUNCE_ACTION,
                    transaction,
                    1800,
                    1,
                    1,
                )
                if set(self.announces) == {
                    value.hex() for value in self.expected
                }:
                    response += socket.inet_aton("127.0.0.1") + struct.pack(
                        "!H", self.peer_port
                    )
                self.socket.sendto(response, client)
        except BaseException as error:
            self.failure = ScenarioFailure(
                f"{error}; announces={self.announces} lifecycle={self.lifecycle}"
            )
        finally:
            self.socket.close()


class DualIdentityDhtRouter:
    """Observe lookup and announce for both versioned hybrid keys."""

    def __init__(self, fixture: RuntimeFixture, peer_port: int) -> None:
        self.expected = {
            bytes.fromhex(fixture.wire_info_hash),
            bytes.fromhex(fixture.full_info_hash[:40]),
        }
        self.peer_port = peer_port
        self.find_node_queries = 0
        self.get_peers: dict[str, int] = {}
        self.announces: dict[str, int] = {}
        self.failure: BaseException | None = None
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(("127.0.0.1", 0))
        self.socket.settimeout(10)
        self.port = self.socket.getsockname()[1]
        self.thread = threading.Thread(
            target=self._serve,
            name=f"rstorrent-hybrid-dht-{self.port}",
        )

    def start(self) -> None:
        self.thread.start()

    def join(self) -> None:
        self.thread.join(timeout=12)
        if self.thread.is_alive():
            raise ScenarioFailure("hybrid DHT router did not terminate")
        if self.failure is not None:
            raise ScenarioFailure(f"hybrid DHT router failed: {self.failure}")
        expected = {value.hex() for value in self.expected}
        if (
            set(self.get_peers) != expected
            or set(self.announces) != expected
            or self.find_node_queries < 1
        ):
            raise ScenarioFailure(
                "hybrid DHT missed a versioned operation: "
                f"find_node={self.find_node_queries} get={self.get_peers} "
                f"announce={self.announces}"
            )

    def close(self) -> None:
        try:
            self.socket.close()
        except OSError:
            pass
        self.thread.join(timeout=2)

    def _response(
        self,
        transaction: bytes,
        client: tuple[str, int],
        values: bool,
    ) -> bytes:
        response: dict[bytes, object] = {b"id": NODE_ID}
        if values:
            response[b"token"] = b"hybrid-fixture"
            response[b"values"] = [
                socket.inet_aton("127.0.0.1")
                + struct.pack("!H", self.peer_port)
            ]
        return bytes(
            lt.bencode(
                {
                    b"ip": socket.inet_aton(client[0])
                    + struct.pack("!H", client[1]),
                    b"r": response,
                    b"t": transaction,
                    b"y": b"r",
                }
            )
        )

    def _serve(self) -> None:
        expected = {value.hex() for value in self.expected}
        try:
            while set(self.announces) != expected:
                packet, client = self.socket.recvfrom(2048)
                message = lt.bdecode(packet)
                if not isinstance(message, dict) or message.get(b"y") != b"q":
                    raise ScenarioFailure("hybrid DHT received a non-query")
                transaction = message.get(b"t")
                arguments = message.get(b"a")
                method = message.get(b"q")
                if (
                    not isinstance(transaction, bytes)
                    or not 1 <= len(transaction) <= 8
                    or not isinstance(arguments, dict)
                    or len(arguments.get(b"id", b"")) != 20
                ):
                    raise ScenarioFailure("hybrid DHT query envelope changed")
                values = False
                if method == b"find_node":
                    self.find_node_queries += 1
                    if len(arguments.get(b"target", b"")) != 20:
                        raise ScenarioFailure("hybrid DHT find_node target changed")
                elif method in (b"get_peers", b"announce_peer"):
                    info_hash = arguments.get(b"info_hash")
                    if info_hash not in self.expected:
                        raise ScenarioFailure("hybrid DHT used an unrelated identity")
                    identity = info_hash.hex()
                    if method == b"get_peers":
                        self.get_peers[identity] = self.get_peers.get(identity, 0) + 1
                        values = True
                    else:
                        if arguments.get(b"token") != b"hybrid-fixture":
                            raise ScenarioFailure("hybrid DHT announce token changed")
                        self.announces[identity] = self.announces.get(identity, 0) + 1
                else:
                    raise ScenarioFailure(f"unexpected hybrid DHT method {method!r}")
                self.socket.sendto(
                    self._response(transaction, client, values),
                    client,
                )
        except BaseException as error:
            self.failure = error
        finally:
            self.socket.close()


def make_fixture(root: Path) -> RuntimeFixture:
    name = "hybrid-aligned-multi"
    files = (
        SourceFile((b"a-empty.bin",), b""),
        SourceFile((b"b-one-piece.bin",), deterministic_bytes(7, 137)),
        SourceFile(
            (b"c-nested", b"selected-multi.bin"),
            deterministic_bytes(11, BLOCK * 4 + 731),
        ),
        SourceFile(
            (b"d-skipped-multi.bin",),
            deterministic_bytes(17, BLOCK * 4 + 911),
        ),
        SourceFile((b"e-empty.bin",), b""),
        SourceFile((b"f-short-tail.bin",), deterministic_bytes(23, 701)),
    )
    fixture_root = root / name
    fixture_root.mkdir(parents=True)
    independent = hybrid_fixture(name, list(files), BLOCK * 4, True)
    torrent_path = fixture_root / f"{name}.torrent"
    torrent_path.write_bytes(independent.torrent)
    torrent_info = lt.torrent_info(str(torrent_path))
    hashes = torrent_info.info_hashes()
    v1 = str(hashes.v1)
    v2 = str(hashes.v2)
    if v1 != independent.expected["v1_info_hash"]:
        raise ScenarioFailure(f"libtorrent hybrid v1 identity changed: {v1}")
    if v2 != independent.expected["v2_info_hash"]:
        raise ScenarioFailure(f"libtorrent hybrid v2 identity changed: {v2}")
    if not hashes.has_v1() or not hashes.has_v2():
        raise ScenarioFailure("pinned libtorrent did not retain both hybrid identities")
    if torrent_info.num_pieces() != independent.expected["logical_pieces"]:
        raise ScenarioFailure("pinned libtorrent changed hybrid logical geometry")

    storage_root = fixture_root / "rstorrent-content"
    for source in files:
        path = storage_root / "root" / Path(
            *(component.decode("utf-8") for component in source.path)
        )
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(source.data)
    libtorrent_storage_root = fixture_root / "libtorrent-content"
    payload_index = 0
    storage = torrent_info.files()
    for index in range(storage.num_files()):
        if int(storage.file_flags(index)) & int(lt.file_storage.flag_pad_file):
            continue
        source = files[payload_index]
        path = libtorrent_storage_root / storage.file_path(index)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(source.data)
        payload_index += 1
    if payload_index != len(files):
        raise ScenarioFailure("hybrid payload file mapping changed")
    return RuntimeFixture(
        name=name,
        torrent_path=torrent_path,
        torrent_info=torrent_info,
        files=files,
        storage_root=storage_root,
        libtorrent_storage_root=libtorrent_storage_root,
        profile_root=fixture_root / "seed-profile",
        full_info_hash=v2,
        wire_info_hash=v1,
    )


def build_binaries(repository: Path) -> tuple[Path, Path, Path]:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "rstorrent-session",
            "--bin",
            "rstorrent-incoming-seed",
            "-p",
            "rstorrent-engine",
            "--bin",
            "rstorrent-download-piece",
            "-p",
            "rstorrent-gateway",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=240,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to build hybrid runtime harness\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    binaries = (
        repository / "target/debug/rstorrent-incoming-seed",
        repository / "target/debug/rstorrent-download-piece",
        repository / "target/debug/rstorrent-gateway",
    )
    if not all(binary.is_file() for binary in binaries):
        raise ScenarioFailure("hybrid runtime binaries were not created")
    return binaries


def parse_download_report(stdout: str) -> dict[str, str]:
    return {
        key: value
        for token in stdout.split()
        if "=" in token
        for key, value in [token.split("=", 1)]
    }


def assert_hybrid_upgrade(
    observation: dict[str, object], fixture: RuntimeFixture, label: str
) -> None:
    handshakes = observation.get("handshakes")
    if not isinstance(handshakes, list):
        raise ScenarioFailure(f"{label} lost handshake observations")
    offered = [
        row
        for row in handshakes
        if row.get("direction") == "client"
        and row.get("info_hash") == fixture.wire_info_hash
        and row.get("hybrid_v2") is True
    ]
    accepted = [
        row
        for row in handshakes
        if row.get("direction") == "upstream"
        and row.get("info_hash") == fixture.full_info_hash[:40]
    ]
    if not offered or not accepted:
        raise ScenarioFailure(
            f"{label} lacked an exact v1-to-v2 hybrid upgrade: {observation}"
        )


def rstorrent_btih_leech(
    binary: Path, fixture: RuntimeFixture, output: Path
) -> dict[str, object]:
    session, handle, diagnostics = start_libtorrent_seed(
        fixture, listen_port=available_tcp_port()
    )
    proxy = PlaintextBep52Proxy(("127.0.0.1", session.listen_port()))
    process: subprocess.Popen[str] | None = None
    try:
        selection = ",".join(str(index) for index in sorted(SELECTED_FILES))
        magnet = (
            f"magnet:?xt=urn:btih:{fixture.wire_info_hash}"
            f"&x.pe={proxy.endpoint[0]}:{proxy.endpoint[1]}&so={selection}"
        )
        process = subprocess.Popen(
            [
                str(binary),
                "--magnet",
                magnet,
                "--output",
                str(output),
                "--timeout-seconds",
                str(TRANSFER_TIMEOUT_SECONDS),
                "--max-buffered-payload-bytes",
                str(DEFAULT_PAYLOAD_ALLOWANCE),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            stdout, stderr = process.communicate(timeout=PROCESS_TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired as error:
            process.kill()
            process.communicate()
            raise ScenarioFailure("btih hybrid leech exceeded its deadline") from error
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        if process.returncode != 0:
            raise ScenarioFailure(
                f"btih hybrid leech exited with {process.returncode}\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}\n"
                + "\n".join(diagnostics[-40:])
            )
        fields = parse_download_report(stdout)
        if fields.get("info_hash") != fixture.wire_info_hash:
            raise ScenarioFailure(f"btih leech reported wrong identity: {fields}")
        if int(fields.get("payload_high_water", "-1")) > DEFAULT_PAYLOAD_ALLOWANCE:
            raise ScenarioFailure(f"btih leech exceeded payload bound: {fields}")
        compare_files(
            fixture, output, skipped=frozenset({SKIPPED_FILE}), named_root=False
        )
        observation = proxy.snapshot()
        assert_hybrid_upgrade(observation, fixture, "RSTorrent btih leecher")
        if not observation["hash_requests"] or int(observation["hash_responses"]) < 1:
            raise ScenarioFailure(
                f"btih hybrid leech did not authenticate v2 hashes: {observation}"
            )
        skipped_pieces = {3, 4}
        requested_pieces = {
            int(message[0]) for message in observation["piece_messages"]
        }
        if requested_pieces & skipped_pieces:
            raise ScenarioFailure(
                f"btih hybrid leech requested skipped payload: {observation}"
            )
        return {
            "report": fields,
            "wire": observation,
            "selected_files": sorted(SELECTED_FILES),
            "skipped_file": SKIPPED_FILE,
        }
    finally:
        if process is not None and process.poll() is None:
            terminate_process(process)
        proxy.close()
        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        gc.collect()


def hybrid_application_restart(
    gateway_binary: Path, fixture: RuntimeFixture, root: Path
) -> dict[str, object]:
    profile = root / "profile"
    storage = root / "storage"
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    proxy: PlaintextBep52Proxy | None = None
    gateway: subprocess.Popen[str] | None = None
    failure: BaseException | None = None
    try:
        session, handle, seed_alerts = start_libtorrent_seed(
            fixture, listen_port=available_tcp_port()
        )
        proxy = PlaintextBep52Proxy(("127.0.0.1", session.listen_port()))
        gateway, address = start_gateway(gateway_binary, profile, storage)
        selected = ",".join(str(index) for index in sorted(SELECTED_FILES))
        magnet = (
            "magnet:?xt=urn:btmh:1220"
            f"{fixture.full_info_hash}"
            f"&x.pe={proxy.endpoint[0]}:{proxy.endpoint[1]}&so={selected}"
        )
        torrent_id = added_torrent_id(
            application_command(
                address,
                "add-hybrid-btmh",
                {
                    "type": "add_magnet",
                    "magnet": magnet,
                    "storage_root": "downloads",
                    "start_content": True,
                    "skip_files": [],
                },
            )
        )
        try:
            initial = wait_application_complete(
                address, torrent_id, "hybrid-initial", minimum_verified=4
            )
        except ScenarioFailure as error:
            seed_alerts.extend(alert.message() for alert in session.pop_alerts())
            diagnostics = stop_gateway(gateway)
            gateway = None
            raise ScenarioFailure(
                f"{error}; wire={proxy.snapshot()}; "
                f"libtorrent={seed_alerts[-40:]}; gateway={diagnostics}"
            ) from error
        identities = initial.get("protocol_identities")
        if identities != {
            "v1": fixture.wire_info_hash,
            "v2": fixture.full_info_hash,
        }:
            raise ScenarioFailure(f"hybrid application lost aliases: {initial}")
        compare_files(
            fixture, storage, skipped=frozenset({SKIPPED_FILE}), named_root=True
        )
        before_promotion = proxy.snapshot()
        client_handshakes = [
            row
            for row in before_promotion["handshakes"]
            if row.get("direction") == "client"
        ]
        if not client_handshakes or any(
            row.get("info_hash") != fixture.full_info_hash[:40]
            for row in client_handshakes
        ):
            raise ScenarioFailure(
                f"btmh application leech did not route directly as v2: {before_promotion}"
            )
        application_command(
            address,
            "promote-hybrid-file",
            {
                "type": "download_files",
                "torrent_id": torrent_id,
                "file_indices": [SKIPPED_FILE],
            },
        )
        completed = wait_application_complete(
            address, torrent_id, "hybrid-promoted", minimum_verified=6
        )
        compare_files(fixture, storage)
        exported = application_command(
            address,
            "export-hybrid-magnet",
            {"type": "export_magnet", "torrent_id": torrent_id},
        )
        result = exported.get("result")
        result = result.get("result") if isinstance(result, dict) else None
        canonical = result.get("magnet") if isinstance(result, dict) else None
        expected_prefix = (
            f"magnet:?xt=urn:btih:{fixture.wire_info_hash}"
            f"&xt=urn:btmh:1220{fixture.full_info_hash}"
        )
        if not isinstance(canonical, str) or not canonical.startswith(expected_prefix):
            raise ScenarioFailure(f"hybrid canonical export changed: {exported}")
        stop_gateway(gateway)
        gateway = None
        before_restart = proxy.snapshot()
        gateway, restarted_address = start_gateway(gateway_binary, profile, storage)
        restarted = wait_application_complete(
            restarted_address, torrent_id, "hybrid-restart", minimum_verified=6
        )
        if restarted.get("protocol_identities") != identities:
            raise ScenarioFailure(f"hybrid restart changed identities: {restarted}")
        after_restart = proxy.snapshot()
        if after_restart["piece_frames"] != before_restart["piece_frames"]:
            raise ScenarioFailure(
                "complete hybrid restart redownloaded payload: "
                f"before={before_restart} after={after_restart}"
            )
        return {
            "torrent_id": torrent_id,
            "initial_verified": initial.get("verified_piece_count"),
            "promoted_verified": completed.get("verified_piece_count"),
            "restarted_verified": restarted.get("verified_piece_count"),
            "canonical_magnet": canonical,
            "before_promotion": before_promotion,
            "after_restart": after_restart,
        }
    except BaseException as error:
        failure = error
        raise
    finally:
        if gateway is not None:
            try:
                stop_gateway(gateway)
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"hybrid gateway cleanup failed: {cleanup_error}", file=sys.stderr)
        if proxy is not None:
            proxy.close()
        if session is not None and handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        if session is not None:
            session.pause()
        gc.collect()


def start_rstorrent_hybrid_seed(
    binary: Path,
    fixture: RuntimeFixture,
    *,
    require_mse: bool = False,
    utp: bool = False,
) -> tuple[subprocess.Popen[str], dict[str, object]]:
    command = [
        str(binary),
        "--profile-root",
        str(fixture.profile_root),
        "--storage-root",
        str(fixture.storage_root),
        "--metainfo",
        str(fixture.torrent_path),
    ]
    if require_mse:
        command.extend(["--encryption", "required"])
    if utp:
        command.extend(["--utp", "--encryption", "disabled"])
    process = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        ready = read_json_line(process, 30)
        expected = {
            "event": "ready",
            "protocol": "hybrid",
            "info_hash": fixture.wire_info_hash,
            "full_info_hash": f"{fixture.wire_info_hash}+{fixture.full_info_hash}",
            "registrations": 2,
        }
        for field, value in expected.items():
            if ready.get(field) != value:
                raise ScenarioFailure(
                    f"hybrid seed {field}={ready.get(field)!r}, expected {value!r}"
                )
        return process, ready
    except BaseException:
        terminate_process(process)
        raise


def libtorrent_hybrid_leech(
    fixture: RuntimeFixture,
    ready: dict[str, object],
    output: Path,
    *,
    identity: str,
    require_mse: bool = False,
    utp_only: bool = False,
) -> dict[str, object]:
    output.mkdir(parents=True)
    session = libtorrent_session(
        incoming=False, require_mse=require_mse, utp_only=utp_only
    )
    handle: lt.torrent_handle | None = None
    proxy: PlaintextBep52Proxy | TcpProxy | None = None
    diagnostics: list[str] = []
    negotiated = None
    try:
        endpoint = (
            parse_address({"listen": ready.get("utp_listen")})
            if utp_only
            else parse_address(ready)
        )
        if require_mse:
            proxy = TcpProxy(endpoint)
        elif not utp_only:
            proxy = PlaintextBep52Proxy(endpoint)
        target = proxy.endpoint if proxy is not None else endpoint
        if identity == "v1":
            # Supplying the complete hybrid dictionary lets libtorrent know
            # that a v1 entry may offer the BEP 52 upgrade on its first dial.
            parameters = lt.add_torrent_params()
            parameters.ti = fixture.torrent_info
        else:
            parameters = lt.parse_magnet_uri(
                "magnet:?xt=urn:btmh:1220"
                f"{fixture.full_info_hash}&x.pe={target[0]}:{target[1]}"
            )
        parameters.save_path = str(output)
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        handle = session.add_torrent(parameters)
        handle.connect_peer(target)
        deadline = time.monotonic() + TRANSFER_TIMEOUT_SECONDS
        while time.monotonic() < deadline:
            diagnostics.extend(alert.message() for alert in session.pop_alerts())
            negotiated = negotiated or observed_encryption(handle)
            status = handle.status()
            if status.errc.value() != 0:
                raise ScenarioFailure(
                    f"libtorrent hybrid {identity} leech failed: {status.errc.message()}"
                )
            if status.is_seeding:
                break
            time.sleep(0.02)
        else:
            wire = proxy.snapshot() if isinstance(proxy, PlaintextBep52Proxy) else {}
            raise ScenarioFailure(
                f"libtorrent hybrid {identity} leech timed out\n"
                f"wire={wire}\n" + "\n".join(diagnostics[-40:])
            )
        compare_libtorrent_files(fixture, output)
        wire: dict[str, object] = {}
        if isinstance(proxy, PlaintextBep52Proxy):
            wire = proxy.snapshot()
            if identity == "v1":
                assert_hybrid_upgrade(wire, fixture, "libtorrent btih leecher")
            else:
                client = [
                    row
                    for row in wire["handshakes"]
                    if row.get("direction") == "client"
                ]
                if not client or any(
                    row.get("info_hash") != fixture.full_info_hash[:40]
                    for row in client
                ):
                    raise ScenarioFailure(
                        f"libtorrent btmh leech did not use direct v2: {wire}"
                    )
                if (
                    not wire["hash_requests"]
                    or int(wire["hash_responses"]) < 1
                    or int(wire["hash_rejects"]) != 0
                ):
                    raise ScenarioFailure(
                        "libtorrent btmh leech did not receive v2 verification hashes: "
                        f"{wire}"
                    )
        if isinstance(proxy, TcpProxy):
            assert_successful_wire_shape(proxy.traces(), "rc4")
            negotiated = "rc4"
        utp = validate_libtorrent_utp(session, diagnostics) if utp_only else None
        return {
            "identity": identity,
            "wire": wire,
            "encryption": negotiated,
            "utp": utp,
            "alerts": diagnostics[-10:],
        }
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        if proxy is not None:
            proxy.close()
        gc.collect()


def seed_roles(
    seed_binary: Path, fixture: RuntimeFixture, root: Path
) -> dict[str, object]:
    process, ready = start_rstorrent_hybrid_seed(seed_binary, fixture)
    try:
        v1 = libtorrent_hybrid_leech(fixture, ready, root / "v1", identity="v1")
        v2 = libtorrent_hybrid_leech(fixture, ready, root / "v2", identity="v2")
        stopped = stop_rstorrent_seed(process, fixture.total_size * 2)
        process = None
        return {"v1": v1, "v2": v2, "stopped": stopped}
    finally:
        if process is not None:
            terminate_process(process)


def transport_roles(
    seed_binary: Path, fixture: RuntimeFixture, root: Path
) -> dict[str, object]:
    mse_process, mse_ready = start_rstorrent_hybrid_seed(
        seed_binary, fixture, require_mse=True
    )
    try:
        mse = libtorrent_hybrid_leech(
            fixture, mse_ready, root / "mse", identity="v1", require_mse=True
        )
        mse_stopped = stop_rstorrent_seed(mse_process, fixture.total_size)
        mse_process = None
    finally:
        if mse_process is not None:
            terminate_process(mse_process)

    utp_fixture = RuntimeFixture(
        name=fixture.name,
        torrent_path=fixture.torrent_path,
        torrent_info=fixture.torrent_info,
        files=fixture.files,
        storage_root=fixture.storage_root,
        libtorrent_storage_root=fixture.libtorrent_storage_root,
        profile_root=fixture.torrent_path.parent / "utp-seed-profile",
        full_info_hash=fixture.full_info_hash,
        wire_info_hash=fixture.wire_info_hash,
    )
    utp_process, utp_ready = start_rstorrent_hybrid_seed(
        seed_binary, utp_fixture, utp=True
    )
    try:
        utp = libtorrent_hybrid_leech(
            fixture, utp_ready, root / "utp", identity="v2", utp_only=True
        )
        utp_stopped = stop_rstorrent_seed(utp_process, fixture.total_size)
        utp_process = None
    finally:
        if utp_process is not None:
            terminate_process(utp_process)
    return {
        "mse": mse,
        "mse_stopped": mse_stopped,
        "utp": utp,
        "utp_stopped": utp_stopped,
    }


def hybrid_discovery_fixture(
    fixture: RuntimeFixture,
    root: Path,
    *,
    tracker_url: str | None = None,
) -> RuntimeFixture:
    root.mkdir()
    torrent_path = fixture.torrent_path
    torrent_info = fixture.torrent_info
    if tracker_url is not None:
        outer = lt.bdecode(fixture.torrent_path.read_bytes())
        if not isinstance(outer, dict):
            raise ScenarioFailure("hybrid discovery metainfo is not a dictionary")
        outer[b"announce"] = tracker_url.encode("utf-8")
        torrent_path = root / "discovery.torrent"
        torrent_path.write_bytes(bytes(lt.bencode(outer)))
        torrent_info = lt.torrent_info(str(torrent_path))
        hashes = torrent_info.info_hashes()
        if (
            str(hashes.v1) != fixture.wire_info_hash
            or str(hashes.v2) != fixture.full_info_hash
        ):
            raise ScenarioFailure("hybrid outer tracker changed an identity")
    storage_root = root / "download"
    storage_root.mkdir()
    return RuntimeFixture(
        name=fixture.name,
        torrent_path=torrent_path,
        torrent_info=torrent_info,
        files=fixture.files,
        storage_root=storage_root,
        libtorrent_storage_root=fixture.libtorrent_storage_root,
        profile_root=root / "profile",
        full_info_hash=fixture.full_info_hash,
        wire_info_hash=fixture.wire_info_hash,
    )


def start_hybrid_discovery_download(
    binary: Path,
    fixture: RuntimeFixture,
    *,
    dht_port: int | None = None,
) -> tuple[subprocess.Popen[str], dict[str, object]]:
    command = [
        str(binary),
        "--profile-root",
        str(fixture.profile_root),
        "--storage-root",
        str(fixture.storage_root),
        "--metainfo",
        str(fixture.torrent_path),
        "--download-fixture",
        "--utp",
        "--encryption",
        "disabled",
        "--download-rate-limit",
        str(BLOCK),
    ]
    if dht_port is not None:
        command.extend(["--dht-bootstrap", f"127.0.0.1:{dht_port}"])
    process = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        ready = read_json_line(process, 30)
        if (
            ready.get("event") != "ready"
            or ready.get("protocol") != "hybrid"
            or ready.get("full_info_hash")
            != f"{fixture.wire_info_hash}+{fixture.full_info_hash}"
            or ready.get("registrations") != 2
        ):
            raise ScenarioFailure(f"unexpected hybrid discovery readiness: {ready}")
        return process, ready
    except BaseException:
        terminate_process(process)
        raise


def tracker_discovery_role(
    binary: Path,
    fixture: RuntimeFixture,
    root: Path,
) -> dict[str, object]:
    session, handle, diagnostics = start_libtorrent_seed(fixture, utp_only=True)
    tracker = DualIdentityUdpTracker(fixture, session.listen_port())
    discovered = hybrid_discovery_fixture(
        fixture,
        root,
        tracker_url=f"udp://127.0.0.1:{tracker.port}/announce",
    )
    process: subprocess.Popen[str] | None = None
    tracker.start()
    try:
        process, ready = start_hybrid_discovery_download(
            binary,
            discovered,
        )
        try:
            _, application = wait_rstorrent_download(process, discovered)
        except BaseException as error:
            raise ScenarioFailure(
                f"{error}; tracker_announces={tracker.announces} "
                f"tracker_lifecycle={tracker.lifecycle} "
                f"tracker_failure={tracker.failure}"
            ) from error
        tracker.join()
        oracle = validate_libtorrent_utp(session, diagnostics)
        stopped = stop_rstorrent_download(process)
        process = None
        return {
            "announces": tracker.announces,
            "lifecycle": tracker.lifecycle,
            "requests": tracker.requests,
            "application": application,
            "oracle": oracle,
            "ready_udp": ready.get("utp_listen"),
            "cleanup": stopped.get("event") == "stopped",
        }
    finally:
        if process is not None:
            terminate_process(process)
        tracker.close()
        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        gc.collect()


def dht_discovery_role(
    binary: Path,
    fixture: RuntimeFixture,
    root: Path,
) -> dict[str, object]:
    session, handle, diagnostics = start_libtorrent_seed(fixture, utp_only=True)
    router = DualIdentityDhtRouter(fixture, session.listen_port())
    discovered = hybrid_discovery_fixture(fixture, root)
    process: subprocess.Popen[str] | None = None
    router.start()
    try:
        process, ready = start_hybrid_discovery_download(
            binary,
            discovered,
            dht_port=router.port,
        )
        _, application = wait_rstorrent_download(process, discovered)
        router.join()
        oracle = validate_libtorrent_utp(session, diagnostics)
        stopped = stop_rstorrent_download(process)
        process = None
        return {
            "find_node_queries": router.find_node_queries,
            "get_peers": router.get_peers,
            "announces": router.announces,
            "application": application,
            "oracle": oracle,
            "ready_udp": ready.get("utp_listen"),
            "cleanup": stopped.get("event") == "stopped",
        }
    finally:
        if process is not None:
            terminate_process(process)
        router.close()
        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        gc.collect()


def discovery_roles(
    binary: Path,
    fixture: RuntimeFixture,
    root: Path,
) -> dict[str, object]:
    root.mkdir()
    return {
        "tracker": tracker_discovery_role(
            binary,
            fixture,
            root / "tracker",
        ),
        "dht": dht_discovery_role(
            binary,
            fixture,
            root / "dht",
        ),
    }


def run(repository: Path, *, no_build: bool) -> None:
    run_root = Path(tempfile.mkdtemp(prefix="rstorrent-hybrid-runtime-"))
    failure: BaseException | None = None
    try:
        fixture = make_fixture(run_root)
        binaries = (
            (
                repository / "target/debug/rstorrent-incoming-seed",
                repository / "target/debug/rstorrent-download-piece",
                repository / "target/debug/rstorrent-gateway",
            )
            if no_build
            else build_binaries(repository)
        )
        if not all(binary.is_file() for binary in binaries):
            raise ScenarioFailure("hybrid runtime binaries are absent")
        seed_binary, download_binary, gateway_binary = binaries
        evidence = {
            "rstorrent_btih": rstorrent_btih_leech(
                download_binary, fixture, run_root / "rstorrent-btih"
            ),
            "application_btmh": hybrid_application_restart(
                gateway_binary, fixture, run_root / "application"
            ),
            "seed_roles": seed_roles(seed_binary, fixture, run_root / "seed-roles"),
            "transports": transport_roles(
                seed_binary, fixture, run_root / "transport-roles"
            ),
            "discovery": discovery_roles(
                seed_binary,
                fixture,
                run_root / "discovery-roles",
            ),
        }
        rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        child_rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        if sys.platform != "darwin":
            rss *= 1024
            child_rss *= 1024
        print(json.dumps(evidence, sort_keys=True))
        print(
            "interop=hybrid-runtime "
            f"libtorrent_binding={lt.__version__} libtorrent_native={lt.version} "
            "roles=both entry_lanes=v1-upgrade,direct-v2 "
            "selection=promotion restart=true tracker=both dht=both "
            "mse=rc4 utp=true cleanup=true "
            f"oracle_rss_bytes={rss} child_peak_rss_bytes={child_rss}"
        )
    except BaseException as error:
        failure = error
        raise
    finally:
        try:
            shutil.rmtree(run_root)
        except OSError as cleanup_error:
            if failure is None:
                raise
            print(f"hybrid runtime cleanup failed: {cleanup_error}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-build", action="store_true")
    arguments = parser.parse_args()
    try:
        run(Path(__file__).resolve().parents[2], no_build=arguments.no_build)
    except ScenarioFailure as error:
        print(f"hybrid runtime failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
