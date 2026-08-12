#!/usr/bin/env python3
"""Run RSTorrent's uTP bulk sender through bounded deterministic UDP faults."""

from __future__ import annotations

import gc
import heapq
import json
import selectors
import socket
import statistics
import subprocess
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
DIAGNOSTIC_MTU_PROFILE = "diagnostic-mtu-black-hole"
PRODUCT_MTU_CLEAN_PROFILE = "product-mtu-clean-1500"
PRODUCT_MTU_1280_PROFILE = "product-mtu-1280"
PRODUCT_MTU_PROFILES = (PRODUCT_MTU_CLEAN_PROFILE, PRODUCT_MTU_1280_PROFILE)
LONG_RTT_PROFILE = "long-rtt-160ms"
LONG_RTT_PAYLOAD_SIZE = 16 * 1024 * 1024 + 731
DYNAMIC_MTU_PROFILES = PRODUCT_MTU_PROFILES + (LONG_RTT_PROFILE,)
RELAY_PROFILES = PROFILES + (DIAGNOSTIC_MTU_PROFILE,) + DYNAMIC_MTU_PROFILES
EFFICIENCY_PAIRS = 5
SCENARIO_TIMEOUT_SECONDS = 180.0
LONG_RTT_SCENARIO_TIMEOUT_SECONDS = 60.0
MAX_PACKET_DECISIONS = 20_000
MAX_PROFILE_BYTES = 16 * 1024 * 1024
LONG_RTT_MAX_PACKET_DECISIONS = 200_000
LONG_RTT_MAX_PROFILE_BYTES = 160 * 1024 * 1024
MAX_QUEUED_DATAGRAMS = 256
MAX_QUEUED_BYTES = 1024 * 1024
LONG_RTT_MAX_QUEUED_DATAGRAMS = 1024
LONG_RTT_MAX_QUEUED_BYTES = 4 * 1024 * 1024
MAX_DATAGRAM_BYTES = 65_535


class ImpairmentFailure(InteropFailure):
    pass


@dataclass(frozen=True)
class RelayDecision:
    drop: bool
    delays_seconds: tuple[float, ...]
    reordered: bool = False
    fragmentable_mtu_retry: bool = False


class RelayPolicy:
    def __init__(self, profile: str) -> None:
        if profile not in RELAY_PROFILES:
            raise ImpairmentFailure(f"unknown relay profile {profile}")
        self.profile = profile
        self.packet_ordinal = 0
        self.direction_ordinals = {
            "client-to-target": 0,
            "target-to-client": 0,
        }
        self.data_ordinal = 0
        self.protected_oversized_sequences: set[int] = set()

    def decide(self, direction: str, payload: bytes) -> RelayDecision:
        self.packet_ordinal += 1
        try:
            self.direction_ordinals[direction] += 1
        except KeyError as error:
            raise ImpairmentFailure(f"unknown relay direction {direction}") from error
        direction_ordinal = self.direction_ordinals[direction]
        eligible_data = direction == "target-to-client" and utp_packet_type(payload) == 0
        if eligible_data:
            self.data_ordinal += 1
        if self.profile == "delay-jitter":
            delay = 0.005 if direction_ordinal % 2 else 0.025
            return RelayDecision(False, (delay,))
        if self.profile == LONG_RTT_PROFILE:
            return RelayDecision(False, (0.080,))
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
        if (
            self.profile in (DIAGNOSTIC_MTU_PROFILE, PRODUCT_MTU_1280_PROFILE)
            and eligible_data
            and len(payload) > 1_280
        ):
            sequence = int.from_bytes(payload[16:18], "big")
            if sequence not in self.protected_oversized_sequences:
                self.protected_oversized_sequences.add(sequence)
                return RelayDecision(True, (0.002,))
            return RelayDecision(False, (0.002,), fragmentable_mtu_retry=True)
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
    mtu_probe_drops: int = 0
    mtu_fragmentable_retries: int = 0
    data_datagrams: int = 0
    max_data_datagram_bytes: int = 0
    client_high_water: int = 0
    queue_high_water: int = 0
    queued_bytes_high_water: int = 0
    queued_datagrams: int = 0
    queued_bytes: int = 0
    discarded_on_stop: int = 0
    last_client_sequence: int | None = None
    last_client_acknowledgement: int | None = None
    last_target_sequence: int | None = None
    last_target_acknowledgement: int | None = None


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
        datagram_limit = (
            LONG_RTT_MAX_QUEUED_DATAGRAMS
            if self.policy.profile == LONG_RTT_PROFILE
            else MAX_QUEUED_DATAGRAMS
        )
        byte_limit = (
            LONG_RTT_MAX_QUEUED_BYTES
            if self.policy.profile == LONG_RTT_PROFILE
            else MAX_QUEUED_BYTES
        )
        if len(self.events) >= datagram_limit:
            raise ImpairmentFailure("relay datagram queue exceeded its bound")
        if snapshot.queued_bytes + len(payload) > byte_limit:
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
        packet_decision_limit = (
            LONG_RTT_MAX_PACKET_DECISIONS
            if self.policy.profile == LONG_RTT_PROFILE
            else MAX_PACKET_DECISIONS
        )
        if snapshot.packet_decisions > packet_decision_limit:
            raise ImpairmentFailure("relay packet decisions exceeded their bound")
        decision = self.policy.decide(direction, payload)
        if utp_packet_type(payload) is not None:
            sequence = int.from_bytes(payload[16:18], "big")
            acknowledgement = int.from_bytes(payload[18:20], "big")
            if direction == "client-to-target":
                snapshot.last_client_sequence = sequence
                snapshot.last_client_acknowledgement = acknowledgement
            else:
                snapshot.last_target_sequence = sequence
                snapshot.last_target_acknowledgement = acknowledgement
        if direction == "target-to-client" and utp_packet_type(payload) == 0:
            snapshot.data_datagrams += 1
            snapshot.max_data_datagram_bytes = max(
                snapshot.max_data_datagram_bytes, len(payload)
            )
        copies = 0 if decision.drop else len(decision.delays_seconds)
        snapshot.considered_bytes += max(0, copies - 1) * len(payload)
        profile_byte_limit = (
            LONG_RTT_MAX_PROFILE_BYTES
            if self.policy.profile == LONG_RTT_PROFILE
            else MAX_PROFILE_BYTES
        )
        if snapshot.considered_bytes > profile_byte_limit:
            raise ImpairmentFailure("relay profile byte budget was exceeded")
        if decision.drop:
            snapshot.dropped_datagrams += 1
            if self.policy.profile in (
                DIAGNOSTIC_MTU_PROFILE,
                PRODUCT_MTU_1280_PROFILE,
            ):
                snapshot.mtu_probe_drops += 1
            return
        if decision.fragmentable_mtu_retry:
            snapshot.mtu_fragmentable_retries += 1
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


def parse_process_time(value: str) -> float:
    days = 0
    if "-" in value:
        day_text, value = value.split("-", 1)
        days = int(day_text)
    components = value.split(":")
    if len(components) == 2:
        hours = 0
        minutes, seconds = components
    elif len(components) == 3:
        hours, minutes, seconds = components
    else:
        raise ImpairmentFailure(f"unexpected process CPU time {value}")
    return (
        days * 86_400
        + int(hours) * 3_600
        + int(minutes) * 60
        + float(seconds)
    )


@dataclass
class ProcessResources:
    cpu_seconds: float = 0.0
    rss_high_water_bytes: int = 0

    def observe(self, process: subprocess.Popen[str]) -> None:
        completed = subprocess.run(
            ["ps", "-o", "rss=", "-o", "time=", "-p", str(process.pid)],
            capture_output=True,
            text=True,
            timeout=2,
            check=False,
        )
        if completed.returncode != 0:
            if process.poll() is not None:
                return
            raise ImpairmentFailure(
                f"failed to sample RSTorrent resources: {completed.stderr.strip()}"
            )
        fields = completed.stdout.split()
        if len(fields) != 2:
            raise ImpairmentFailure(
                f"unexpected RSTorrent resource sample: {completed.stdout!r}"
            )
        self.rss_high_water_bytes = max(
            self.rss_high_water_bytes, int(fields[0]) * 1_024
        )
        self.cpu_seconds = max(self.cpu_seconds, parse_process_time(fields[1]))

    def snapshot(self) -> dict[str, float | int]:
        return {
            "cpu_seconds": round(self.cpu_seconds, 6),
            "rss_high_water_bytes": self.rss_high_water_bytes,
        }


def validate_profile(profile: str, relay: dict[str, int], rstorrent: dict[str, Any]) -> None:
    live = rstorrent["resources"]["live_utp"]
    udp = rstorrent["resources"]["live_udp"]
    if relay["packet_decisions"] <= 0 or relay["data_datagrams"] <= 0:
        raise ImpairmentFailure(f"{profile} relayed no uTP DATA traffic")
    packet_decision_limit = (
        LONG_RTT_MAX_PACKET_DECISIONS
        if profile == LONG_RTT_PROFILE
        else MAX_PACKET_DECISIONS
    )
    profile_byte_limit = (
        LONG_RTT_MAX_PROFILE_BYTES
        if profile == LONG_RTT_PROFILE
        else MAX_PROFILE_BYTES
    )
    if relay["packet_decisions"] > packet_decision_limit:
        raise ImpairmentFailure(f"{profile} exceeded its decision bound")
    if relay["considered_bytes"] > profile_byte_limit:
        raise ImpairmentFailure(f"{profile} exceeded its byte bound")
    if relay["queue_high_water"] > MAX_QUEUED_DATAGRAMS:
        raise ImpairmentFailure(f"{profile} exceeded its datagram queue")
    if relay["queued_bytes_high_water"] > MAX_QUEUED_BYTES:
        raise ImpairmentFailure(f"{profile} exceeded its byte queue")
    if relay["client_high_water"] != 1:
        raise ImpairmentFailure(f"{profile} did not retain one client")
    if relay["data_datagrams"] != live["data_datagrams_sent"]:
        raise ImpairmentFailure(f"{profile} relay/runtime DATA counts disagree")
    if profile in (
        "clean",
        "delay-jitter",
        "mtu-black-hole",
        PRODUCT_MTU_CLEAN_PROFILE,
        LONG_RTT_PROFILE,
    ):
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
    if profile in PROFILES:
        if relay["max_data_datagram_bytes"] > 548:
            raise ImpairmentFailure(
                f"{profile} fixed-MTU control emitted an oversized DATA datagram"
            )
        if live.get("path_mtu_profile") != "fixed_548":
            raise ImpairmentFailure(f"{profile} did not use the fixed MTU profile")
        if (
            live["mtu_candidate_min_bytes"] != 548
            or live["mtu_candidate_max_bytes"] != 548
        ):
            raise ImpairmentFailure(f"{profile} changed the fixed MTU candidate")
        for key in (
            "mtu_probes_started_high_water",
            "mtu_probes_acknowledged_high_water",
            "mtu_probes_failed_high_water",
            "mtu_probe_datagrams_sent",
            "mtu_fragmentable_retry_datagrams_sent",
        ):
            if live[key] != 0:
                raise ImpairmentFailure(f"{profile} unexpectedly reported {key}")
    if profile in (DIAGNOSTIC_MTU_PROFILE, PRODUCT_MTU_1280_PROFILE):
        selected = live["selected_mtu_max_bytes"]
        if relay["mtu_probe_drops"] <= 0 or relay["mtu_fragmentable_retries"] <= 0:
            raise ImpairmentFailure("diagnostic MTU relay observed no probe/retry feedback")
        if relay["dropped_datagrams"] != relay["mtu_probe_drops"]:
            raise ImpairmentFailure("diagnostic MTU relay dropped non-probe traffic")
        if relay["max_data_datagram_bytes"] <= 1_280:
            raise ImpairmentFailure("diagnostic MTU runtime emitted no oversized probe")
        if live["mtu_probes_started_high_water"] <= 0:
            raise ImpairmentFailure("diagnostic MTU runtime started no probes")
        if live["mtu_probes_acknowledged_high_water"] <= 0:
            raise ImpairmentFailure("diagnostic MTU runtime acknowledged no probes")
        if live["mtu_probes_failed_high_water"] <= 0:
            raise ImpairmentFailure("diagnostic MTU runtime failed no probes")
        if live["mtu_probe_datagrams_sent"] != live["mtu_probes_started_high_water"]:
            raise ImpairmentFailure("diagnostic MTU probe emissions do not match starts")
        if (
            live["mtu_fragmentable_retry_datagrams_sent"]
            != live["mtu_probes_failed_high_water"]
        ):
            raise ImpairmentFailure("diagnostic MTU retries do not match failed probes")
        if relay["mtu_probe_drops"] != live["mtu_probes_failed_high_water"]:
            raise ImpairmentFailure("diagnostic MTU relay/runtime loss counts disagree")
        if (
            relay["mtu_fragmentable_retries"]
            != live["mtu_fragmentable_retry_datagrams_sent"]
        ):
            raise ImpairmentFailure("diagnostic MTU relay/runtime retry counts disagree")
        if (
            live["selected_mtu_min_bytes"] != 548
            or selected is None
            or selected > 1_280
            or 1_280 - selected > 16
        ):
            raise ImpairmentFailure(
                f"diagnostic MTU search did not converge below 1280: {selected}"
            )
        if live["mtu_candidate_max_bytes"] <= 1_280:
            raise ImpairmentFailure("diagnostic MTU search recorded no oversized candidate")
        if live["loss_reduction_high_water"] != 0:
            raise ImpairmentFailure("diagnostic MTU probe loss reduced congestion")
        if live["timeout_collapse_high_water"] != 0:
            raise ImpairmentFailure("diagnostic MTU probe loss collapsed congestion")
    if profile in DYNAMIC_MTU_PROFILES:
        if live.get("path_mtu_profile") != "dynamic_ipv4":
            raise ImpairmentFailure(f"{profile} did not use the product MTU profile")
        if live["mtu_probes_started_high_water"] <= 0:
            raise ImpairmentFailure(f"{profile} started no product MTU probes")
        if live["mtu_probes_acknowledged_high_water"] <= 0:
            raise ImpairmentFailure(f"{profile} acknowledged no product MTU probes")
        if udp["protected_sends_attempted"] != live["mtu_probe_datagrams_sent"]:
            raise ImpairmentFailure(
                f"{profile} protected-send and probe counts disagree"
            )
        if udp["protected_sends_sent"] != udp["protected_sends_attempted"]:
            raise ImpairmentFailure(f"{profile} did not send every protected probe")
        if udp["fragmentation_restore_failures"] != 0:
            raise ImpairmentFailure(f"{profile} failed to restore fragmentation policy")
    if profile in (PRODUCT_MTU_CLEAN_PROFILE, LONG_RTT_PROFILE):
        selected = live["selected_mtu_max_bytes"]
        if selected is None or not 1_456 <= selected <= 1_472:
            raise ImpairmentFailure(
                f"clean product MTU search did not converge near 1472: {selected}"
            )
        if relay["max_data_datagram_bytes"] < 1_456:
            raise ImpairmentFailure("clean product MTU emitted no large DATA datagram")
        if live["mtu_probes_failed_high_water"] != 0:
            raise ImpairmentFailure("clean product MTU unexpectedly failed a probe")


def run_profile(
    binary: Path,
    root: Path,
    profile: str,
    *,
    payload_size: int = PAYLOAD_SIZE,
) -> dict[str, Any]:
    started = time.monotonic()
    scenario_timeout = (
        LONG_RTT_SCENARIO_TIMEOUT_SECONDS
        if profile == LONG_RTT_PROFILE
        else SCENARIO_TIMEOUT_SECONDS
    )
    deadline = started + scenario_timeout
    root.mkdir()
    torrent_info, seed_root, expected_sha1 = create_fixture(
        root,
        payload_size=payload_size,
    )
    piece_count = torrent_info.num_pieces()
    leech_root = root / "libtorrent-leech"
    leech_root.mkdir()
    role: RoleProcess | None = None
    relay: DeterministicUdpRelay | None = None
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    process_resources = ProcessResources()
    relay_live: dict[str, int] | None = None
    relay_terminal: dict[str, int] | None = None
    if profile == DIAGNOSTIC_MTU_PROFILE:
        role_name = "diagnostic-mtu-seed"
    elif profile in DYNAMIC_MTU_PROFILES:
        role_name = "product-mtu-seed"
    else:
        role_name = "impairment-seed"
    try:
        role = RoleProcess.start(
            binary,
            [
                role_name,
                "--metainfo",
                str(root / "forced-utp.torrent"),
                "--storage-root",
                str(seed_root),
            ],
        )
        process_resources.observe(role.process)
        _, seed_port = validate_ready(role.read_event(deadline), role_name)
        relay = DeterministicUdpRelay(("127.0.0.1", seed_port), profile)
        relay.start()
        leecher_settings = dict(TRANSPORT_SETTINGS)
        leecher_settings["alert_mask"] |= int(lt.alert.category_t.peer_notification)
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
            process_resources.observe(role.process)
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
            role.send_command("snapshot")
            snapshot_event = role.read_event(time.monotonic() + 2.0)
            if (
                snapshot_event.get("event") != "snapshot"
                or snapshot_event.get("role") != role_name
            ):
                raise ImpairmentFailure("impairment seed omitted its live snapshot")
            stats = stats_snapshot(
                session,
                diagnostics,
                time.monotonic() + 2.0,
            )
            status = handle.status()
            relay_snapshot = relay.snapshot()
            live = snapshot_event["resources"]["live_utp"]
            udp = snapshot_event["resources"]["live_udp"]
            incoming = snapshot_event["resources"]["live_incoming"]
            role.send_stop()
            raise ImpairmentFailure(
                f"{profile} timeout evidence: progress_ppm={status.progress_ppm}, "
                f"wanted_done={status.total_wanted_done}, "
                f"uploaded={incoming['payload_bytes_sent']}, "
                f"relay_data={relay_snapshot['data_datagrams']}, "
                f"relay_forwarded={relay_snapshot['forwarded_datagrams']}, "
                f"relay_queue_hwm={relay_snapshot['queue_high_water']}, "
                f"relay_dropped={relay_snapshot['dropped_datagrams']}, "
                f"relay_client_seq_ack={relay_snapshot['last_client_sequence']}/"
                f"{relay_snapshot['last_client_acknowledgement']}, "
                f"relay_target_seq_ack={relay_snapshot['last_target_sequence']}/"
                f"{relay_snapshot['last_target_acknowledgement']}, "
                f"rst_retransmits={live['retransmission_datagrams_sent']}, "
                f"rst_connection_queue_hwm="
                f"{live['connection_datagram_queue_high_water']}, "
                f"rst_connection_drops={live['connection_datagrams_dropped']}, "
                f"rst_malformed={live['malformed_datagrams']}, "
                f"rst_unknown={live['unknown_connection_datagrams']}, "
                f"rst_active={live['active_connections']}, "
                f"rst_connections_started={live['connections_started']}, "
                f"rst_terminals={{graceful:{live['graceful_connections']},"
                f"reset:{live['reset_connections']},"
                f"consumer:{live['consumer_dropped_connections']},"
                f"protocol:{live['protocol_error_connections']},"
                f"io:{live['io_error_connections']}}}, "
                f"udp_utp_queue_hwm={udp['utp_queue_high_water']}, "
                f"udp_utp_drops={udp['utp_datagrams_dropped']}, "
                f"rst_loss_reductions={live['loss_reduction_high_water']}, "
                f"rst_timeouts={live['timeout_collapse_high_water']}, "
                f"rst_rtt={live['smoothed_rtt_min_micros']}.."
                f"{live['smoothed_rtt_max_micros']}, "
                f"rst_rto={live['effective_rto_min_micros']}.."
                f"{live['effective_rto_max_micros']}, "
                f"rst_queue_delay={live['queue_delay_min_micros']}.."
                f"{live['queue_delay_max_micros']}, "
                f"rst_cwnd_min={live['congestion_window_min_bytes']}, "
                f"rst_cwnd_max={live['congestion_window_max_bytes']}, "
                f"incoming_reads={incoming['reads']}, "
                f"incoming_read_bytes={incoming['read_bytes']}, "
                f"incoming_queued_requests_hwm="
                f"{incoming['queued_requests_high_water']}, "
                f"incoming_writer_hwm={incoming['writer_send_buffer_high_water']}, "
                f"incoming_rejections={incoming['rejection_counts']}, "
                f"lt_loss={stats['utp.utp_packet_loss']}, "
                f"lt_timeouts={stats['utp.utp_timeout']}, "
                f"lt_fast_retransmit={stats['utp.utp_fast_retransmit']}, "
                f"lt_resends={stats['utp.utp_packet_resend']}, "
                f"libtorrent_diagnostics={diagnostics[-MAX_DIAGNOSTICS:]}"
            )
        active_seconds = time.monotonic() - transfer_started
        output = leech_root / PAYLOAD_NAME
        if (
            not output.is_file()
            or output.stat().st_size != payload_size
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
        role.send_command("snapshot")
        pre_stop = role.read_event(deadline)
        process_resources.observe(role.process)
        if (
            pre_stop.get("event") != "snapshot"
            or pre_stop.get("role") != role_name
            or pre_stop.get("resources", {})
            .get("live_incoming", {})
            .get("payload_bytes_sent")
            != payload_size
        ):
            raise ImpairmentFailure(f"{profile} pre-stop snapshot is invalid")
        role.send_stop()
        complete = role.read_event(deadline)
        role.wait_success(deadline)
        validate_complete(
            complete,
            role_name,
            expected_sha1,
            require_fixed_mtu=profile in PROFILES,
            expected_payload_bytes=payload_size,
            expected_piece_count=piece_count,
        )
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
            "process_resources": process_resources.snapshot(),
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


def run(profiles_to_run: tuple[str, ...] = PROFILES) -> dict[str, Any]:
    binary = build_role_binary()
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="rstorrent-utp-impairment-") as temporary:
        root = Path(temporary)
        profiles = [
            run_profile(binary, root / profile, profile) for profile in profiles_to_run
        ]
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


def run_long_rtt() -> dict[str, Any]:
    binary = build_role_binary()
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="rstorrent-utp-long-rtt-") as temporary:
        profile = run_profile(
            binary,
            Path(temporary) / LONG_RTT_PROFILE,
            LONG_RTT_PROFILE,
            payload_size=LONG_RTT_PAYLOAD_SIZE,
        )
    return {
        "schema_version": 1,
        "oracle": "rstorrent-pinned-libtorrent-utp-long-rtt",
        "libtorrent_version": lt.__version__,
        "payload": {
            "bytes": LONG_RTT_PAYLOAD_SIZE,
            "sha1": profile["payload_sha1"],
        },
        "profiles": [profile],
        "cleanup": {
            "succeeded": True,
            "terminal_relay_queues": 0,
            "temporary_directory_removed": True,
        },
        "seconds": round(time.monotonic() - started, 6),
    }


def run_efficiency() -> dict[str, Any]:
    binary = build_role_binary()
    started = time.monotonic()
    pairs: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="rstorrent-utp-efficiency-") as temporary:
        root = Path(temporary)
        for ordinal in range(1, EFFICIENCY_PAIRS + 1):
            fixed = run_profile(binary, root / f"pair-{ordinal:02d}-fixed", "clean")
            dynamic = run_profile(
                binary,
                root / f"pair-{ordinal:02d}-dynamic",
                PRODUCT_MTU_CLEAN_PROFILE,
            )
            pairs.append({"ordinal": ordinal, "fixed": fixed, "dynamic": dynamic})

    fixed_datagrams = [pair["fixed"]["relay"]["data_datagrams"] for pair in pairs]
    dynamic_datagrams = [
        pair["dynamic"]["relay"]["data_datagrams"] for pair in pairs
    ]
    fixed_seconds = [pair["fixed"]["active_transfer_seconds"] for pair in pairs]
    dynamic_seconds = [pair["dynamic"]["active_transfer_seconds"] for pair in pairs]
    fixed_cpu = [
        pair["fixed"]["process_resources"]["cpu_seconds"] for pair in pairs
    ]
    dynamic_cpu = [
        pair["dynamic"]["process_resources"]["cpu_seconds"] for pair in pairs
    ]
    fixed_rss = [
        pair["fixed"]["process_resources"]["rss_high_water_bytes"] for pair in pairs
    ]
    dynamic_rss = [
        pair["dynamic"]["process_resources"]["rss_high_water_bytes"] for pair in pairs
    ]
    fixed_datagram_median = statistics.median(fixed_datagrams)
    dynamic_datagram_median = statistics.median(dynamic_datagrams)
    reduction = 1.0 - dynamic_datagram_median / fixed_datagram_median
    fixed_time_median = statistics.median(fixed_seconds)
    dynamic_time_median = statistics.median(dynamic_seconds)
    fixed_cpu_median = statistics.median(fixed_cpu)
    dynamic_cpu_median = statistics.median(dynamic_cpu)
    fixed_rss_median = statistics.median(fixed_rss)
    dynamic_rss_median = statistics.median(dynamic_rss)
    if reduction < 0.50:
        raise ImpairmentFailure(
            f"dynamic MTU reduced median DATA datagrams by only {reduction:.1%}"
        )
    if dynamic_time_median > fixed_time_median * 1.10:
        raise ImpairmentFailure(
            "dynamic MTU exceeded the 10% median active-transfer time gate"
        )
    if dynamic_cpu_median > fixed_cpu_median * 1.25 + 0.02:
        raise ImpairmentFailure("dynamic MTU materially increased median seed CPU")
    if dynamic_rss_median > fixed_rss_median * 1.10 + 2 * 1024 * 1024:
        raise ImpairmentFailure("dynamic MTU materially increased median seed RSS")

    queue_high_waters = {
        "fixed_connection_datagrams": max(
            pair["fixed"]["rstorrent"]["resources"]["live_utp"][
                "connection_datagram_queue_high_water"
            ]
            for pair in pairs
        ),
        "dynamic_connection_datagrams": max(
            pair["dynamic"]["rstorrent"]["resources"]["live_utp"][
                "connection_datagram_queue_high_water"
            ]
            for pair in pairs
        ),
        "fixed_relay_datagrams": max(
            pair["fixed"]["relay"]["queue_high_water"] for pair in pairs
        ),
        "dynamic_relay_datagrams": max(
            pair["dynamic"]["relay"]["queue_high_water"] for pair in pairs
        ),
        "fixed_egress_waiters": max(
            pair["fixed"]["rstorrent"]["resources"]["live_udp"][
                "egress_waiter_high_water"
            ]
            for pair in pairs
        ),
        "dynamic_egress_waiters": max(
            pair["dynamic"]["rstorrent"]["resources"]["live_udp"][
                "egress_waiter_high_water"
            ]
            for pair in pairs
        ),
    }
    return {
        "schema_version": 1,
        "oracle": "rstorrent-product-utp-path-mtu-efficiency",
        "libtorrent_version": lt.__version__,
        "alternating_pairs": EFFICIENCY_PAIRS,
        "payload_bytes_per_case": PAYLOAD_SIZE,
        "summary": {
            "fixed_data_datagram_median": fixed_datagram_median,
            "dynamic_data_datagram_median": dynamic_datagram_median,
            "data_datagram_reduction_fraction": round(reduction, 6),
            "fixed_active_seconds_median": round(fixed_time_median, 6),
            "dynamic_active_seconds_median": round(dynamic_time_median, 6),
            "fixed_cpu_seconds_median": round(fixed_cpu_median, 6),
            "dynamic_cpu_seconds_median": round(dynamic_cpu_median, 6),
            "fixed_rss_bytes_median": fixed_rss_median,
            "dynamic_rss_bytes_median": dynamic_rss_median,
            "queue_high_waters": queue_high_waters,
        },
        "pairs": pairs,
        "cleanup": {
            "succeeded": True,
            "terminal_relay_queues": 0,
            "temporary_directory_removed": True,
        },
        "seconds": round(time.monotonic() - started, 6),
    }


def main() -> int:
    arguments = sys.argv[1:]
    if not arguments:
        result = run(PROFILES)
    elif arguments == ["--diagnostic-mtu"]:
        result = run((DIAGNOSTIC_MTU_PROFILE,))
    elif arguments == ["--product-mtu"]:
        result = run(PRODUCT_MTU_PROFILES)
    elif arguments == ["--efficiency"]:
        result = run_efficiency()
    elif arguments == ["--long-rtt"]:
        result = run_long_rtt()
    else:
        raise ImpairmentFailure(
            "usage: utp_runtime_impairment.py "
            "[--diagnostic-mtu|--product-mtu|--efficiency|--long-rtt]"
        )
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ImpairmentFailure, OSError) as error:
        print(f"RSTorrent uTP impairment failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
