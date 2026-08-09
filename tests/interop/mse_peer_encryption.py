#!/usr/bin/env python3
"""Exercise the controlled RSTorrent/libtorrent MSE policy and method matrix."""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import select
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path

import libtorrent as lt

from first_verified_piece import ScenarioFailure, add_seed, create_session, wait_for_listener
from incoming_seeding import Fixture, parse_address, start_seed, stop_seed


BT_HEADER = b"\x13BitTorrent protocol"
PAYLOAD_NAME = "mse-fixture.bin"
PAYLOAD_SIZE = 8 * 1024 * 1024 + 731
PIECE_SIZE = 256 * 1024
CASE_TIMEOUT_SECONDS = 6
PROCESS_TIMEOUT_SECONDS = 12
CAPTURE_LIMIT = 4 * 1024


@dataclass
class ConnectionTrace:
    client_to_upstream: bytearray
    upstream_to_client: bytearray
    direction_turns: int
    delayed_turns: int
    last_direction: str | None

    @classmethod
    def empty(cls) -> ConnectionTrace:
        return cls(bytearray(), bytearray(), 0, 0, None)


class TcpProxy:
    def __init__(
        self,
        upstream: tuple[str, int],
        *,
        flight_delay_seconds: float = 0.0,
        delayed_turn_limit: int = 0,
    ) -> None:
        self._upstream = upstream
        self._flight_delay_seconds = flight_delay_seconds
        self._delayed_turn_limit = delayed_turn_limit
        self._listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._listener.bind(("127.0.0.1", 0))
        self._listener.listen()
        self._listener.settimeout(0.1)
        self.endpoint = ("127.0.0.1", int(self._listener.getsockname()[1]))
        self._stop = threading.Event()
        self._lock = threading.Lock()
        self._traces: list[ConnectionTrace] = []
        self._sockets: list[socket.socket] = []
        self._workers: list[threading.Thread] = []
        self._acceptor = threading.Thread(target=self._accept, daemon=True)
        self._acceptor.start()

    def _accept(self) -> None:
        while not self._stop.is_set():
            try:
                client, _ = self._listener.accept()
            except TimeoutError:
                continue
            except OSError:
                break
            trace = ConnectionTrace.empty()
            with self._lock:
                self._traces.append(trace)
                self._sockets.append(client)
            worker = threading.Thread(
                target=self._forward,
                args=(client, trace),
                daemon=True,
            )
            self._workers.append(worker)
            worker.start()

    def _forward(self, client: socket.socket, trace: ConnectionTrace) -> None:
        upstream = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        with self._lock:
            self._sockets.append(upstream)
        try:
            upstream.connect(self._upstream)
            client.setblocking(False)
            upstream.setblocking(False)
            open_reads = {client, upstream}
            while open_reads and not self._stop.is_set():
                readable, _, _ = select.select(list(open_reads), [], [], 0.1)
                for source in readable:
                    destination = upstream if source is client else client
                    try:
                        chunk = source.recv(64 * 1024)
                    except BlockingIOError:
                        continue
                    if not chunk:
                        open_reads.remove(source)
                        try:
                            destination.shutdown(socket.SHUT_WR)
                        except OSError:
                            pass
                        continue
                    capture = (
                        trace.client_to_upstream
                        if source is client
                        else trace.upstream_to_client
                    )
                    direction = "client_to_upstream" if source is client else "upstream_to_client"
                    if trace.last_direction != direction:
                        trace.last_direction = direction
                        trace.direction_turns += 1
                        if trace.delayed_turns < self._delayed_turn_limit:
                            trace.delayed_turns += 1
                            time.sleep(self._flight_delay_seconds)
                    if len(capture) < CAPTURE_LIMIT:
                        capture.extend(chunk[: CAPTURE_LIMIT - len(capture)])
                    view = memoryview(chunk)
                    while view:
                        try:
                            written = destination.send(view)
                        except BlockingIOError:
                            select.select([], [destination], [], 0.1)
                            continue
                        if written == 0:
                            raise ConnectionError("proxy made no forwarding progress")
                        view = view[written:]
        except (ConnectionError, OSError):
            pass
        finally:
            for stream in (client, upstream):
                try:
                    stream.close()
                except OSError:
                    pass

    def traces(self) -> list[ConnectionTrace]:
        with self._lock:
            return [
                ConnectionTrace(
                    bytearray(trace.client_to_upstream),
                    bytearray(trace.upstream_to_client),
                    trace.direction_turns,
                    trace.delayed_turns,
                    trace.last_direction,
                )
                for trace in self._traces
            ]

    def close(self) -> None:
        self._stop.set()
        try:
            self._listener.close()
        except OSError:
            pass
        with self._lock:
            sockets = list(self._sockets)
        for stream in sockets:
            try:
                stream.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            try:
                stream.close()
            except OSError:
                pass
        self._acceptor.join(timeout=2)
        for worker in self._workers:
            worker.join(timeout=2)
        if self._acceptor.is_alive() or any(worker.is_alive() for worker in self._workers):
            raise ScenarioFailure("TCP proxy did not join every forwarding task")


def build_binaries(repository: Path) -> tuple[Path, Path]:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "rstorrent-engine",
            "--bin",
            "rstorrent-download-piece",
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
            "failed to build MSE interoperability binaries\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    downloader = repository / "target/debug/rstorrent-download-piece"
    seed = repository / "target/debug/rstorrent-incoming-seed"
    if not downloader.is_file() or not seed.is_file():
        raise ScenarioFailure("MSE interoperability binaries were not created")
    return downloader, seed


def create_fixture(root: Path) -> Fixture:
    fixture_root = root / "fixture"
    storage_root = fixture_root / "published"
    storage_root.mkdir(parents=True)
    payload_path = storage_root / PAYLOAD_NAME
    payload = bytes(
        ((offset * 73) ^ (offset >> 3) ^ (offset * offset >> 11) ^ 0xA5) & 0xFF
        for offset in range(PAYLOAD_SIZE)
    )
    payload_path.write_bytes(payload)
    expected_sha1 = hashlib.sha1(payload).hexdigest()
    files = lt.file_storage()
    files.add_file(PAYLOAD_NAME, PAYLOAD_SIZE)
    creator = lt.create_torrent(
        files,
        piece_size=PIECE_SIZE,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(storage_root))
    torrent_path = fixture_root / "fixture.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    info = lt.torrent_info(str(torrent_path))
    return Fixture(
        name="mse",
        torrent_path=torrent_path,
        storage_root=storage_root,
        profile_root=fixture_root / "profile",
        torrent_info=info,
        info_hash=str(info.info_hashes().v1),
        files=((Path(PAYLOAD_NAME), expected_sha1),),
        output_is_file=True,
    )


def configure_encryption(
    session: lt.session,
    policy: str,
    level: str = "both",
    prefer_rc4: bool = False,
) -> None:
    policies = {
        "disabled": lt.enc_policy.pe_disabled,
        "enabled": lt.enc_policy.pe_enabled,
        "forced": lt.enc_policy.pe_forced,
    }
    levels = {
        "plaintext": lt.enc_level.pe_plaintext,
        "rc4": lt.enc_level.pe_rc4,
        "both": lt.enc_level.pe_both,
    }
    session.apply_settings(
        {
            "in_enc_policy": int(policies[policy]),
            "out_enc_policy": int(policies[policy]),
            "allowed_enc_level": int(levels[level]),
            "prefer_rc4": prefer_rc4,
        }
    )


def observed_method(handle: lt.torrent_handle) -> str | None:
    for peer in handle.get_peer_info():
        if peer.flags & lt.peer_info.rc4_encrypted:
            return "rc4"
        if peer.flags & lt.peer_info.plaintext_encrypted:
            return "plaintext_payload"
    return None


def handshake_completed(handle: lt.torrent_handle) -> bool:
    return any(
        not peer.flags & lt.peer_info.connecting
        and not peer.flags & lt.peer_info.handshake
        for peer in handle.get_peer_info()
    )


def assert_successful_wire_shape(
    traces: list[ConnectionTrace],
    method: str | None,
) -> None:
    if not traces:
        raise ScenarioFailure("proxy observed no TCP connection")
    successful = traces[-1]
    sent = bytes(successful.client_to_upstream)
    received = bytes(successful.upstream_to_client)
    if method is None:
        if not sent.startswith(BT_HEADER):
            raise ScenarioFailure("ordinary connection did not begin with a BitTorrent handshake")
    elif method == "rc4":
        if BT_HEADER in sent or BT_HEADER in received:
            raise ScenarioFailure("known BitTorrent handshake appeared on a forced-RC4 wire")
    elif method == "plaintext_payload":
        if BT_HEADER in sent:
            raise ScenarioFailure("initiator IA was visible in plaintext-payload MSE")
        if BT_HEADER not in received:
            raise ScenarioFailure("responder post-PE4 handshake was not plaintext")
    else:
        raise ScenarioFailure(f"unknown negotiated method {method}")


def run_outgoing_case(
    downloader: Path,
    fixture: Fixture,
    root: Path,
    rst_policy: str,
    oracle_policy: str,
    expected_success: bool,
    expected_method: str | None,
    *,
    level: str = "both",
    prefer_rc4: bool = False,
    label: str | None = None,
    flight_delay_seconds: float = 0.0,
    delayed_turn_limit: int = 0,
) -> dict[str, object]:
    name = label or f"outgoing-{rst_policy}-{oracle_policy}"
    session = create_session()
    configure_encryption(session, oracle_policy, level, prefer_rc4)
    diagnostics: list[str] = []
    handle: lt.torrent_handle | None = None
    proxy: TcpProxy | None = None
    output = root / f"{name}.out"
    observed: set[str] = set()
    try:
        port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            fixture.torrent_info,
            fixture.storage_root,
            diagnostics,
        )
        proxy = TcpProxy(
            ("127.0.0.1", port),
            flight_delay_seconds=flight_delay_seconds,
            delayed_turn_limit=delayed_turn_limit,
        )
        setup_started = time.monotonic()
        process = subprocess.Popen(
            [
                str(downloader),
                "--metainfo",
                str(fixture.torrent_path),
                "--peer",
                f"{proxy.endpoint[0]}:{proxy.endpoint[1]}",
                "--output",
                str(output),
                "--timeout-seconds",
                str(CASE_TIMEOUT_SECONDS),
                "--encryption",
                rst_policy,
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        deadline = time.monotonic() + PROCESS_TIMEOUT_SECONDS
        setup_seconds: float | None = None
        while process.poll() is None:
            method = observed_method(handle)
            if method is not None:
                observed.add(method)
            if setup_seconds is None and handshake_completed(handle):
                setup_seconds = time.monotonic() - setup_started
            if time.monotonic() >= deadline:
                process.kill()
                raise ScenarioFailure(f"{name} exceeded its process deadline")
            time.sleep(0.002)
        stdout, stderr = process.communicate()
        method = next(iter(observed), None)
        traces = proxy.traces()
        if expected_success:
            if setup_seconds is None:
                raise ScenarioFailure(f"{name} never exposed a completed peer handshake")
            if process.returncode != 0:
                raise ScenarioFailure(
                    f"{name} failed unexpectedly: stdout={stdout!r} stderr={stderr!r}"
                )
            if (
                not output.is_file()
                or hashlib.sha1(output.read_bytes()).hexdigest()
                != fixture.files[0][1]
            ):
                raise ScenarioFailure(f"{name} did not publish the exact payload")
            if method != expected_method:
                raise ScenarioFailure(
                    f"{name} oracle method {method!r}, expected {expected_method!r}"
                )
            assert_successful_wire_shape(traces, method)
        elif process.returncode == 0:
            raise ScenarioFailure(f"{name} succeeded but the policy matrix requires failure")
        if rst_policy == "prefer" and oracle_policy == "disabled":
            if len(traces) != 2:
                raise ScenarioFailure(
                    f"{name} used {len(traces)} sockets instead of one MSE attempt and one fallback"
                )
            if bytes(traces[0].client_to_upstream).startswith(BT_HEADER):
                raise ScenarioFailure(f"{name} did not attempt MSE first")
            if not bytes(traces[1].client_to_upstream).startswith(BT_HEADER):
                raise ScenarioFailure(f"{name} fallback was not ordinary plaintext")
        elif len(traces) != 1:
            raise ScenarioFailure(f"{name} unexpectedly used {len(traces)} sockets")
        return {
            "name": name,
            "direction": "rstorrent_initiates",
            "rstorrent_policy": rst_policy,
            "libtorrent_policy": oracle_policy,
            "libtorrent_level": level,
            "prefer_rc4": prefer_rc4,
            "expected_success": expected_success,
            "negotiated_method": method,
            "connections": len(traces),
            "payload_sha1": fixture.files[0][1] if expected_success else None,
            "setup_seconds": setup_seconds,
            "flight_delay_seconds": flight_delay_seconds,
            "delayed_turns": traces[-1].delayed_turns if traces else 0,
        }
    finally:
        if proxy is not None:
            proxy.close()
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()
        output.unlink(missing_ok=True)


def run_incoming_case(
    seed_binary: Path,
    fixture: Fixture,
    root: Path,
    rst_policy: str,
    oracle_policy: str,
    expected_success: bool,
    expected_method: str | None,
    *,
    level: str = "both",
    prefer_rc4: bool = False,
    label: str | None = None,
) -> dict[str, object]:
    name = label or f"incoming-{oracle_policy}-{rst_policy}"
    process, ready = start_seed(seed_binary, fixture, rst_policy)
    upstream = parse_address(ready)
    proxy = TcpProxy(upstream)
    session = create_session()
    configure_encryption(session, oracle_policy, level, prefer_rc4)
    output_root = root / f"{name}-output"
    output_root.mkdir()
    handle: lt.torrent_handle | None = None
    observed: set[str] = set()
    succeeded = False
    try:
        parameters = lt.add_torrent_params()
        parameters.ti = fixture.torrent_info
        parameters.save_path = str(output_root)
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        parameters.flags |= lt.torrent_flags.disable_dht
        parameters.flags |= lt.torrent_flags.disable_lsd
        parameters.flags |= lt.torrent_flags.disable_pex
        handle = session.add_torrent(parameters)
        setup_started = time.monotonic()
        handle.connect_peer(proxy.endpoint)
        deadline = time.monotonic() + CASE_TIMEOUT_SECONDS
        setup_seconds: float | None = None
        while time.monotonic() < deadline:
            method = observed_method(handle)
            if method is not None:
                observed.add(method)
            if setup_seconds is None and handshake_completed(handle):
                setup_seconds = time.monotonic() - setup_started
            status = handle.status()
            if status.errc.value() != 0:
                raise ScenarioFailure(f"{name} libtorrent error: {status.errc.message()}")
            if status.is_seeding:
                succeeded = True
                break
            time.sleep(0.005)
        method = next(iter(observed), None)
        traces = proxy.traces()
        if succeeded != expected_success:
            raise ScenarioFailure(
                f"{name} success={succeeded}, expected {expected_success}; "
                f"connections={len(traces)}"
            )
        if expected_success:
            if setup_seconds is None:
                raise ScenarioFailure(f"{name} never exposed a completed peer handshake")
            payload = output_root / PAYLOAD_NAME
            if (
                not payload.is_file()
                or hashlib.sha1(payload.read_bytes()).hexdigest()
                != fixture.files[0][1]
            ):
                raise ScenarioFailure(f"{name} did not download the exact payload")
            if method != expected_method:
                raise ScenarioFailure(
                    f"{name} oracle method {method!r}, expected {expected_method!r}"
                )
            assert_successful_wire_shape(traces, method)
        if oracle_policy == "enabled" and rst_policy == "disabled":
            if len(traces) < 2:
                raise ScenarioFailure(f"{name} did not retry its refused MSE socket in plaintext")
            if bytes(traces[0].client_to_upstream).startswith(BT_HEADER):
                raise ScenarioFailure(f"{name} first socket did not use MSE")
            if not bytes(traces[-1].client_to_upstream).startswith(BT_HEADER):
                raise ScenarioFailure(f"{name} retry socket was not ordinary plaintext")
        return {
            "name": name,
            "direction": "libtorrent_initiates",
            "rstorrent_policy": rst_policy,
            "libtorrent_policy": oracle_policy,
            "libtorrent_level": level,
            "prefer_rc4": prefer_rc4,
            "expected_success": expected_success,
            "negotiated_method": method,
            "connections": len(traces),
            "payload_sha1": fixture.files[0][1] if expected_success else None,
            "setup_seconds": setup_seconds,
        }
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()
        proxy.close()
        try:
            stop_seed(
                process,
                fixture.total_size if succeeded else 0,
                1 if succeeded else 0,
            )
        except BaseException:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=2)
            raise
        shutil.rmtree(output_root, ignore_errors=True)


def run_matrix(repository: Path) -> dict[str, object]:
    downloader, seed_binary = build_binaries(repository)
    results: list[dict[str, object]] = []
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="rstorrent-mse-interop-") as temporary:
        root = Path(temporary)
        fixture = create_fixture(root)

        for rst_policy in ("disabled", "allow", "prefer", "required"):
            for oracle_policy in ("disabled", "enabled", "forced"):
                success = not (
                    (rst_policy in {"disabled", "allow"} and oracle_policy == "forced")
                    or (rst_policy == "required" and oracle_policy == "disabled")
                )
                method = (
                    "plaintext_payload"
                    if success
                    and rst_policy in {"prefer", "required"}
                    and oracle_policy in {"enabled", "forced"}
                    else None
                )
                results.append(
                    run_outgoing_case(
                        downloader,
                        fixture,
                        root,
                        rst_policy,
                        oracle_policy,
                        success,
                        method,
                    )
                )

        for oracle_policy in ("disabled", "forced", "enabled"):
            for rst_policy in ("disabled", "allow", "prefer", "required"):
                success = not (
                    (oracle_policy == "disabled" and rst_policy == "required")
                    or (oracle_policy == "forced" and rst_policy == "disabled")
                )
                method = (
                    "rc4"
                    if success
                    and (
                        oracle_policy == "forced"
                        or (oracle_policy == "enabled" and rst_policy != "disabled")
                    )
                    else None
                )
                results.append(
                    run_incoming_case(
                        seed_binary,
                        fixture,
                        root,
                        rst_policy,
                        oracle_policy,
                        success,
                        method,
                    )
                )

        results.append(
            run_outgoing_case(
                downloader,
                fixture,
                root,
                "required",
                "forced",
                True,
                "rc4",
                prefer_rc4=True,
                label="outgoing-method-rc4",
            )
        )
        results.append(
            run_incoming_case(
                seed_binary,
                fixture,
                root,
                "required",
                "forced",
                True,
                "plaintext_payload",
                level="plaintext",
                label="incoming-method-plaintext",
            )
        )

        flight_delay_seconds = 0.025
        results.append(
            run_outgoing_case(
                downloader,
                fixture,
                root,
                "disabled",
                "disabled",
                True,
                None,
                label="outgoing-flight-plain",
                flight_delay_seconds=flight_delay_seconds,
                delayed_turn_limit=2,
            )
        )
        results.append(
            run_outgoing_case(
                downloader,
                fixture,
                root,
                "required",
                "forced",
                True,
                "rc4",
                prefer_rc4=True,
                label="outgoing-flight-rc4",
                flight_delay_seconds=flight_delay_seconds,
                delayed_turn_limit=4,
            )
        )

    setup_summary: list[dict[str, object]] = []
    for direction in ("rstorrent_initiates", "libtorrent_initiates"):
        successful = [
            result
            for result in results
            if result["direction"] == direction
            and result["expected_success"]
            and result["setup_seconds"] is not None
            and not str(result["name"]).startswith("outgoing-flight-")
        ]
        plain = [
            float(result["setup_seconds"])
            for result in successful
            if result["negotiated_method"] is None
        ]
        mse = [
            float(result["setup_seconds"])
            for result in successful
            if result["negotiated_method"] is not None
        ]
        plain_median = statistics.median(plain)
        mse_median = statistics.median(mse)
        setup_summary.append(
            {
                "direction": direction,
                "plain_samples": len(plain),
                "mse_samples": len(mse),
                "plain_median_millis": plain_median * 1000,
                "mse_median_millis": mse_median * 1000,
                "added_median_millis": (mse_median - plain_median) * 1000,
                "diagnostic_25ms_target_met": mse_median - plain_median <= 0.025,
            }
        )

    delayed_plain = next(
        result for result in results if result["name"] == "outgoing-flight-plain"
    )
    delayed_mse = next(
        result for result in results if result["name"] == "outgoing-flight-rc4"
    )
    outgoing_setup = next(
        summary
        for summary in setup_summary
        if summary["direction"] == "rstorrent_initiates"
    )
    plain_delay = float(delayed_plain["setup_seconds"]) - (
        float(outgoing_setup["plain_median_millis"]) / 1000
    )
    mse_delay = float(delayed_mse["setup_seconds"]) - (
        float(outgoing_setup["mse_median_millis"]) / 1000
    )
    measured_extra = mse_delay - plain_delay
    expected_extra = flight_delay_seconds * 2
    if delayed_plain["delayed_turns"] != 2 or delayed_mse["delayed_turns"] != 4:
        raise ScenarioFailure("fixed-delay proxy did not observe the expected handshake flights")
    network_flight = {
        "one_way_delay_millis": flight_delay_seconds * 1000,
        "plain_delayed_turns": delayed_plain["delayed_turns"],
        "mse_delayed_turns": delayed_mse["delayed_turns"],
        "expected_extra_round_trip_millis": expected_extra * 1000,
        "measured_extra_millis": measured_extra * 1000,
        "within_20ms_tolerance": abs(measured_extra - expected_extra) <= 0.020,
    }
    if not network_flight["within_20ms_tolerance"]:
        raise ScenarioFailure(
            "fixed-delay MSE setup did not add exactly one round trip within tolerance"
        )

    return {
        "schema_version": 1,
        "scenario": "controlled-mse-peer-encryption",
        "status": "passed",
        "environment": {
            "libtorrent": lt.version,
            "repository_commit": subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=repository,
                capture_output=True,
                text=True,
                check=True,
            ).stdout.strip(),
        },
        "fixture": {
            "bytes": PAYLOAD_SIZE,
            "piece_size": PIECE_SIZE,
        },
        "elapsed_seconds": time.monotonic() - started,
        "cases": results,
        "setup_summary": setup_summary,
        "network_flight": network_flight,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    repository = Path(__file__).resolve().parents[2]
    try:
        report = run_matrix(repository)
    except (OSError, ScenarioFailure, subprocess.SubprocessError) as error:
        print(f"MSE interoperability failed: {error}", file=sys.stderr)
        return 1
    rendered = json.dumps(report, indent=2, sort_keys=True)
    if arguments.output is not None:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(rendered + "\n", encoding="utf-8")
    print(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
