#!/usr/bin/env python3
"""Run RSTorrent's uTP bulk sender through bounded deterministic UDP faults."""

from __future__ import annotations

import gc
import heapq
import json
import selectors
import socket
import sys
import tempfile
import threading
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

import libtorrent as lt

from utp_reference_oracle import (
    MAX_DIAGNOSTICS,
    PAYLOAD_NAME,
    PAYLOAD_SIZE,
    POLL_SECONDS,
    TRANSPORT_SETTINGS,
    add_torrent,
    collect_alerts,
    create_fixture,
    hash_file,
    peer_addresses,
    stats_snapshot,
    wait_until_ready,
)
from utp_rstorrent_interop import (
    InteropFailure,
    RoleProcess,
    build_role_binary,
    remove_session_torrent,
    validate_complete,
    validate_libtorrent_stats,
    validate_ready,
)


PROFILES = (
    "clean",
    "delay-jitter",
    "sparse-loss",
    "duplicate-reorder",
    "burst-loss",
    "mtu-black-hole",
)
SCENARIO_TIMEOUT_SECONDS = 180.0
MAX_PACKET_DECISIONS = 10_000
MAX_PROFILE_BYTES = 16 * 1024 * 1024
MAX_QUEUED_DATAGRAMS = 256
MAX_QUEUED_BYTES = 1024 * 1024
MAX_DATAGRAM_BYTES = 65_535


class ImpairmentFailure(InteropFailure):
    pass


@dataclass(frozen=True)
class RelayDecision:
    drop: bool
    delays_seconds: tuple[float, ...]
    reordered: bool = False


class RelayPolicy:
    def __init__(self, profile: str) -> None:
        if profile not in PROFILES:
            raise ImpairmentFailure(f"unknown relay profile {profile}")
        self.profile = profile
        self.packet_ordinal = 0
        self.data_ordinal = 0

    def decide(self, direction: str, payload: bytes) -> RelayDecision:
        self.packet_ordinal += 1
        eligible_data = direction == "target-to-client" and utp_packet_type(payload) == 0
        if eligible_data:
            self.data_ordinal += 1
        if self.profile == "delay-jitter":
            delay = 0.005 if self.packet_ordinal % 2 else 0.025
            return RelayDecision(False, (delay,))
        if self.profile == "sparse-loss" and eligible_data:
            return RelayDecision(self.data_ordinal % 100 == 0, (0.002,))
        if self.profile == "duplicate-reorder" and eligible_data:
            reordered = self.data_ordinal % 53 == 0
            delay = 0.050 if reordered else 0.002
            copies = (delay, delay + 0.001) if self.data_ordinal % 79 == 0 else (delay,)
            return RelayDecision(False, copies, reordered)
        if self.profile == "burst-loss" and eligible_data:
            return RelayDecision(64 <= self.data_ordinal <= 66, (0.002,))
        if self.profile == "mtu-black-hole" and eligible_data:
            return RelayDecision(len(payload) > 1_280, (0.002,))
        return RelayDecision(False, (0.002,))


def utp_packet_type(payload: bytes) -> int | None:
    if len(payload) < 20 or payload[0] & 0x0F != 1:
        return None
    packet_type = payload[0] >> 4
    return packet_type if 0 <= packet_type <= 4 else None


@dataclass
class RelaySnapshot:
    packet_decisions: int = 0
    considered_bytes: int = 0
    forwarded_datagrams: int = 0
    forwarded_bytes: int = 0
    dropped_datagrams: int = 0
    duplicated_datagrams: int = 0
    reordered_datagrams: int = 0
    data_datagrams: int = 0
    max_data_datagram_bytes: int = 0
    client_high_water: int = 0
    queue_high_water: int = 0
    queued_bytes_high_water: int = 0
    queued_datagrams: int = 0
    queued_bytes: int = 0
    discarded_on_stop: int = 0


class DeterministicUdpRelay:
    def __init__(self, target: tuple[str, int], profile: str) -> None:
        self.target = target
        self.policy = RelayPolicy(profile)
        self.client_socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.target_socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.client_socket.bind(("127.0.0.1", 0))
        self.target_socket.bind(("127.0.0.1", 0))
        self.client_socket.setblocking(False)
        self.target_socket.setblocking(False)
        self.client: tuple[str, int] | None = None
        self.events: list[
            tuple[float, int, socket.socket, bytes, tuple[str, int]]
        ] = []
        self.serial = 0
        self.snapshot_value = RelaySnapshot()
        self.failure: str | None = None
        self.lock = threading.Lock()
        self.stop_event = threading.Event()
        self.thread = threading.Thread(target=self._run, daemon=True)

    @property
    def endpoint(self) -> tuple[str, int]:
        address, port = self.client_socket.getsockname()
        return str(address), int(port)

    def start(self) -> None:
        self.thread.start()

    def _fail(self, detail: str) -> None:
        with self.lock:
            if self.failure is None:
                self.failure = detail
        self.stop_event.set()

    def _schedule(
        self,
        delay: float,
        outgoing: socket.socket,
        payload: bytes,
        destination: tuple[str, int],
    ) -> None:
        snapshot = self.snapshot_value
        if len(self.events) >= MAX_QUEUED_DATAGRAMS:
            raise ImpairmentFailure("relay datagram queue exceeded its bound")
        if snapshot.queued_bytes + len(payload) > MAX_QUEUED_BYTES:
            raise ImpairmentFailure("relay byte queue exceeded its bound")
        self.serial += 1
        heapq.heappush(
            self.events,
            (time.monotonic() + delay, self.serial, outgoing, payload, destination),
        )
        snapshot.queued_datagrams = len(self.events)
        snapshot.queued_bytes += len(payload)
        snapshot.queue_high_water = max(snapshot.queue_high_water, len(self.events))
        snapshot.queued_bytes_high_water = max(
            snapshot.queued_bytes_high_water, snapshot.queued_bytes
        )

    def _receive(
        self,
        incoming: socket.socket,
        direction: str,
    ) -> None:
        payload, source = incoming.recvfrom(MAX_DATAGRAM_BYTES)
        source = (str(source[0]), int(source[1]))
        if direction == "client-to-target":
            if self.client is None:
                self.client = source
                self.snapshot_value.client_high_water = 1
            elif source != self.client:
                raise ImpairmentFailure("relay observed more than one client endpoint")
            outgoing = self.target_socket
            destination = self.target
        else:
            if source != self.target:
                raise ImpairmentFailure("relay target socket received a spoofed endpoint")
            if self.client is None:
                raise ImpairmentFailure("relay received target traffic before client ownership")
            outgoing = self.client_socket
            destination = self.client

        snapshot = self.snapshot_value
        snapshot.packet_decisions += 1
        snapshot.considered_bytes += len(payload)
        if snapshot.packet_decisions > MAX_PACKET_DECISIONS:
            raise ImpairmentFailure("relay packet decisions exceeded their bound")
        decision = self.policy.decide(direction, payload)
        if direction == "target-to-client" and utp_packet_type(payload) == 0:
            snapshot.data_datagrams += 1
            snapshot.max_data_datagram_bytes = max(
                snapshot.max_data_datagram_bytes, len(payload)
            )
        copies = 0 if decision.drop else len(decision.delays_seconds)
        snapshot.considered_bytes += max(0, copies - 1) * len(payload)
        if snapshot.considered_bytes > MAX_PROFILE_BYTES:
            raise ImpairmentFailure("relay profile byte budget was exceeded")
        if decision.drop:
            snapshot.dropped_datagrams += 1
            return
        if copies > 1:
            snapshot.duplicated_datagrams += copies - 1
        if decision.reordered:
            snapshot.reordered_datagrams += 1
        for delay in decision.delays_seconds:
            self._schedule(delay, outgoing, payload, destination)

    def _deliver_due(self) -> None:
        snapshot = self.snapshot_value
        now = time.monotonic()
        while self.events and self.events[0][0] <= now:
            _, _, outgoing, payload, destination = heapq.heappop(self.events)
            snapshot.queued_bytes -= len(payload)
            snapshot.queued_datagrams = len(self.events)
            outgoing.sendto(payload, destination)
            snapshot.forwarded_datagrams += 1
            snapshot.forwarded_bytes += len(payload)

    def _run(self) -> None:
        selector = selectors.DefaultSelector()
        selector.register(self.client_socket, selectors.EVENT_READ, "client-to-target")
        selector.register(self.target_socket, selectors.EVENT_READ, "target-to-client")
        try:
            while not self.stop_event.is_set():
                self._deliver_due()
                wait = 0.05
                if self.events:
                    wait = min(wait, max(0.0, self.events[0][0] - time.monotonic()))
                for key, _ in selector.select(wait):
                    self._receive(key.fileobj, str(key.data))
        except (ImpairmentFailure, OSError) as error:
            self._fail(str(error))
        finally:
            selector.close()

    def check(self) -> None:
        with self.lock:
            failure = self.failure
        if failure is not None:
            raise ImpairmentFailure(failure)

    def wait_idle(self, timeout_seconds: float) -> None:
        deadline = time.monotonic() + timeout_seconds
        while time.monotonic() < deadline:
            self.check()
            if not self.events:
                return
            time.sleep(0.005)
        raise ImpairmentFailure("relay did not drain its bounded event queue")

    def snapshot(self) -> dict[str, int]:
        with self.lock:
            return asdict(self.snapshot_value)

    def stop(self) -> dict[str, int]:
        self.stop_event.set()
        self.thread.join(timeout=5)
        if self.thread.is_alive():
            raise ImpairmentFailure("relay task did not terminate")
        with self.lock:
            self.snapshot_value.discarded_on_stop += len(self.events)
            self.events.clear()
            self.snapshot_value.queued_datagrams = 0
            self.snapshot_value.queued_bytes = 0
        self.client_socket.close()
        self.target_socket.close()
        self.check()
        return self.snapshot()


def validate_profile(profile: str, relay: dict[str, int], rstorrent: dict[str, Any]) -> None:
    if relay["packet_decisions"] <= 0 or relay["data_datagrams"] <= 0:
        raise ImpairmentFailure(f"{profile} relayed no uTP DATA traffic")
    if relay["packet_decisions"] > MAX_PACKET_DECISIONS:
        raise ImpairmentFailure(f"{profile} exceeded its decision bound")
    if relay["considered_bytes"] > MAX_PROFILE_BYTES:
        raise ImpairmentFailure(f"{profile} exceeded its byte bound")
    if relay["queue_high_water"] > MAX_QUEUED_DATAGRAMS:
        raise ImpairmentFailure(f"{profile} exceeded its datagram queue")
    if relay["queued_bytes_high_water"] > MAX_QUEUED_BYTES:
        raise ImpairmentFailure(f"{profile} exceeded its byte queue")
    if relay["client_high_water"] != 1:
        raise ImpairmentFailure(f"{profile} did not retain one client")
    if profile in ("clean", "delay-jitter", "mtu-black-hole"):
        if relay["dropped_datagrams"] != 0:
            raise ImpairmentFailure(f"{profile} unexpectedly dropped traffic")
    if profile in ("sparse-loss", "burst-loss"):
        if relay["dropped_datagrams"] <= 0:
            raise ImpairmentFailure(f"{profile} did not apply its fixed loss")
        live = rstorrent["resources"]["live_utp"]
        if live["retransmission_datagrams_sent"] <= 0:
            raise ImpairmentFailure(f"{profile} observed no RSTorrent recovery")
    if profile == "duplicate-reorder" and not (
        relay["duplicated_datagrams"] > 0 and relay["reordered_datagrams"] > 0
    ):
        raise ImpairmentFailure("duplicate-reorder did not apply both policies")
    if profile == "mtu-black-hole" and relay["max_data_datagram_bytes"] > 548:
        raise ImpairmentFailure("fixed-MTU baseline emitted an oversized DATA datagram")


def run_profile(binary: Path, root: Path, profile: str) -> dict[str, Any]:
    started = time.monotonic()
    deadline = started + SCENARIO_TIMEOUT_SECONDS
    root.mkdir()
    torrent_info, seed_root, expected_sha1 = create_fixture(root)
    leech_root = root / "libtorrent-leech"
    leech_root.mkdir()
    role: RoleProcess | None = None
    relay: DeterministicUdpRelay | None = None
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    relay_live: dict[str, int] | None = None
    relay_terminal: dict[str, int] | None = None
    try:
        role = RoleProcess.start(
            binary,
            [
                "impairment-seed",
                "--metainfo",
                str(root / "forced-utp.torrent"),
                "--storage-root",
                str(seed_root),
            ],
        )
        _, seed_port = validate_ready(role.read_event(deadline), "impairment-seed")
        relay = DeterministicUdpRelay(("127.0.0.1", seed_port), profile)
        relay.start()
        leecher_settings = dict(TRANSPORT_SETTINGS)
        leecher_settings["enable_incoming_utp"] = False
        leecher_settings["allow_multiple_connections_per_ip"] = False
        session = lt.session(leecher_settings)
        handle = add_torrent(session, torrent_info, leech_root, seed=False)
        leecher_port = wait_until_ready(
            session,
            handle,
            seed=False,
            deadline=deadline,
            diagnostics=diagnostics,
        )
        transfer_started = time.monotonic()
        handle.connect_peer(relay.endpoint)
        peer_high_water = 0
        observed_addresses: set[str] = set()
        peer_detail_at_high_water: list[str] = []
        while time.monotonic() < deadline:
            relay.check()
            if role.process.poll() is not None:
                raise ImpairmentFailure(
                    f"RSTorrent seed stopped: {role.stderr.captured[-MAX_DIAGNOSTICS:]}"
                )
            collect_alerts(session, diagnostics)
            peers = peer_addresses(handle)
            if len(peers) > peer_high_water:
                peer_high_water = len(peers)
                peer_detail_at_high_water = [
                    (
                        f"ip={peer.ip},local={peer.local_endpoint},"
                        f"type={peer.connection_type},flags={int(peer.flags)}"
                    )
                    for peer in handle.get_peer_info()
                ][:4]
            observed_addresses.update(peers)
            status = handle.status()
            if status.errc.value() != 0:
                raise ImpairmentFailure("libtorrent leecher entered an error state")
            if status.is_seeding:
                break
            time.sleep(POLL_SECONDS)
        else:
            raise ImpairmentFailure(f"{profile} transfer exceeded its deadline")
        active_seconds = time.monotonic() - transfer_started
        output = leech_root / PAYLOAD_NAME
        if (
            not output.is_file()
            or output.stat().st_size != PAYLOAD_SIZE
            or hash_file(output) != expected_sha1
        ):
            raise ImpairmentFailure(f"{profile} output failed exact verification")
        stats = stats_snapshot(session, diagnostics, deadline)
        validate_libtorrent_stats("leecher", stats)
        if peer_high_water != 1 or observed_addresses != {"127.0.0.1"}:
            raise ImpairmentFailure(
                f"{profile} peer evidence failed: high_water={peer_high_water}, "
                f"addresses={sorted(observed_addresses)}, "
                f"details={peer_detail_at_high_water}, "
                f"relay_port={relay.endpoint[1]}, seed_port={seed_port}, "
                f"leecher_port={leecher_port}"
            )
        role.send_stop()
        complete = role.read_event(deadline)
        role.wait_success(deadline)
        validate_complete(complete, "impairment-seed", expected_sha1)
        relay.wait_idle(1.0)
        relay_live = relay.snapshot()
        validate_profile(profile, relay_live, complete)
        return {
            "profile": profile,
            "payload_sha1": expected_sha1,
            "active_transfer_seconds": round(active_seconds, 6),
            "peer_high_water": peer_high_water,
            "libtorrent_stats": stats,
            "rstorrent": complete,
            "relay": relay_live,
            "diagnostics": diagnostics[-MAX_DIAGNOSTICS:],
            "seconds": round(time.monotonic() - started, 6),
        }
    finally:
        if role is not None:
            role.cleanup()
        remove_session_torrent(session, handle)
        handle = None
        session = None
        gc.collect()
        if relay is not None:
            relay_terminal = relay.stop()
            if relay_terminal["queued_datagrams"] != 0 or relay_terminal["queued_bytes"] != 0:
                raise ImpairmentFailure("relay retained terminal queue ownership")


def run() -> dict[str, Any]:
    binary = build_role_binary()
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="rstorrent-utp-impairment-") as temporary:
        root = Path(temporary)
        profiles = [run_profile(binary, root / profile, profile) for profile in PROFILES]
    return {
        "schema_version": 1,
        "oracle": "rstorrent-pinned-libtorrent-utp-real-socket-impairment",
        "libtorrent_version": lt.__version__,
        "payload": {
            "bytes": PAYLOAD_SIZE,
            "sha1": profiles[0]["payload_sha1"],
        },
        "profiles": profiles,
        "cleanup": {
            "succeeded": True,
            "terminal_relay_queues": 0,
            "temporary_directory_removed": True,
        },
        "seconds": round(time.monotonic() - started, 6),
    }


def main() -> int:
    print(json.dumps(run(), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ImpairmentFailure, OSError) as error:
        print(f"RSTorrent uTP impairment failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
