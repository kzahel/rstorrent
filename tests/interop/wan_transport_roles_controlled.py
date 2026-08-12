#!/usr/bin/env python3
"""Controlled all-pairing gate for Tactical 142 WAN role adapters."""

from __future__ import annotations

import argparse
import json
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, TextIO

from public_compare_contract import comparison_profile, parse_metainfo, verify_payload
from wan_transport_fixture import Fixture, create_fixture
from wan_transport_matrix_contract import IMPLEMENTATIONS, MIB, TRANSPORTS


ROOT = Path(__file__).resolve().parents[2]
INTEROP = Path(__file__).resolve().parent
ORACLE_PYTHON = INTEROP / ".venv/bin/python"
LIBTORRENT_ROLE = INTEROP / "wan_transport_libtorrent_role.py"
PROCESS_GRACE_SECONDS = 30.0
MAX_CAPTURE_LINES = 100


class ControlledRoleError(RuntimeError):
    pass


@dataclass
class OutputPump:
    stream: TextIO
    lines: queue.Queue[str | None]
    captured: list[str]

    @classmethod
    def start(cls, stream: TextIO) -> tuple["OutputPump", threading.Thread]:
        pump = cls(stream=stream, lines=queue.Queue(), captured=[])
        thread = threading.Thread(target=pump._run, daemon=True)
        thread.start()
        return pump, thread

    def _run(self) -> None:
        try:
            for line in self.stream:
                value = line.rstrip("\n")
                self.captured.append(value)
                del self.captured[:-MAX_CAPTURE_LINES]
                self.lines.put(value)
        finally:
            self.lines.put(None)


@dataclass
class RoleProcess:
    process: subprocess.Popen[str]
    stdout: OutputPump
    stderr: OutputPump
    threads: tuple[threading.Thread, threading.Thread]
    label: str
    accepts_commands: bool

    @classmethod
    def start(
        cls,
        command: list[str],
        label: str,
        *,
        accepts_commands: bool,
    ) -> "RoleProcess":
        process = subprocess.Popen(
            command,
            cwd=ROOT,
            stdin=subprocess.PIPE if accepts_commands else subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        if process.stdout is None or process.stderr is None:
            process.kill()
            raise ControlledRoleError(f"{label} streams are unavailable")
        stdout, stdout_thread = OutputPump.start(process.stdout)
        stderr, stderr_thread = OutputPump.start(process.stderr)
        return cls(
            process=process,
            stdout=stdout,
            stderr=stderr,
            threads=(stdout_thread, stderr_thread),
            label=label,
            accepts_commands=accepts_commands,
        )

    def read_event(self, deadline: float) -> dict[str, Any]:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise ControlledRoleError(f"{self.label} exceeded its event deadline")
        try:
            line = self.stdout.lines.get(timeout=remaining)
        except queue.Empty as error:
            raise ControlledRoleError(f"{self.label} emitted no bounded event") from error
        if line is None:
            detail = "; ".join(self.stderr.captured[-10:])
            raise ControlledRoleError(f"{self.label} ended before its event: {detail}")
        try:
            event = json.loads(line)
        except json.JSONDecodeError as error:
            raise ControlledRoleError(f"{self.label} emitted invalid JSON") from error
        if not isinstance(event, dict):
            raise ControlledRoleError(f"{self.label} event is not an object")
        return event

    def command(self, value: str) -> None:
        if not self.accepts_commands or self.process.stdin is None:
            raise ControlledRoleError(f"{self.label} has no command channel")
        self.process.stdin.write(value + "\n")
        self.process.stdin.flush()

    def wait_success(self, deadline: float) -> None:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise ControlledRoleError(f"{self.label} exceeded its process deadline")
        try:
            code = self.process.wait(timeout=remaining)
        except subprocess.TimeoutExpired as error:
            raise ControlledRoleError(f"{self.label} did not terminate") from error
        for thread in self.threads:
            thread.join(timeout=PROCESS_GRACE_SECONDS)
        if code != 0:
            detail = "; ".join(self.stderr.captured[-10:])
            raise ControlledRoleError(f"{self.label} exited {code}: {detail}")

    def cleanup(self) -> None:
        if self.process.poll() is None and self.accepts_commands:
            try:
                self.command("abort")
                self.process.wait(timeout=PROCESS_GRACE_SECONDS)
            except (BrokenPipeError, subprocess.TimeoutExpired):
                self.process.terminate()
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=PROCESS_GRACE_SECONDS)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=PROCESS_GRACE_SECONDS)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream is not None:
                stream.close()


@dataclass(frozen=True)
class LocalBinaries:
    probe: Path
    seed: Path


def build_binaries() -> LocalBinaries:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "-p",
            "rstorrent-engine",
            "--bin",
            "rstorrent-public-probe",
            "-p",
            "rstorrent-session",
            "--bin",
            "rstorrent-incoming-seed",
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=20 * 60,
        check=False,
    )
    if completed.returncode != 0:
        raise ControlledRoleError("release WAN role binaries did not build")
    binaries = LocalBinaries(
        probe=ROOT / "target/release/rstorrent-public-probe",
        seed=ROOT / "target/release/rstorrent-incoming-seed",
    )
    if not binaries.probe.is_file() or not binaries.seed.is_file():
        raise ControlledRoleError("release WAN role binaries are missing")
    if not ORACLE_PYTHON.is_file():
        raise ControlledRoleError("pinned local libtorrent environment is missing")
    return binaries


def _fixture_arguments(fixture: Fixture, timeout_seconds: float) -> list[str]:
    return [
        "--metainfo",
        str(fixture.metainfo),
        "--transport",
        "unused",
        "--expected-sha1",
        fixture.sha1,
        "--expected-bytes",
        str(fixture.payload_bytes),
        "--expected-piece-bytes",
        str(fixture.piece_bytes),
        "--expected-pieces",
        str(fixture.piece_count),
        "--timeout-seconds",
        str(timeout_seconds),
    ]


def start_seed(
    implementation: str,
    transport: str,
    fixture: Fixture,
    case_root: Path,
    binaries: LocalBinaries,
    deadline: float,
) -> tuple[RoleProcess, str, int, dict[str, Any]]:
    if implementation == "libtorrent":
        arguments = _fixture_arguments(fixture, deadline - time.monotonic())
        arguments[arguments.index("unused")] = transport
        process = RoleProcess.start(
            [
                str(ORACLE_PYTHON),
                str(LIBTORRENT_ROLE),
                "seed",
                *arguments,
                "--storage-root",
                str(fixture.seed_root),
                "--network-scope",
                "loopback",
            ],
            f"libtorrent {transport} seed",
            accepts_commands=True,
        )
        started = process.read_event(deadline)
        ready = process.read_event(deadline)
        if started.get("event") != "started" or ready.get("event") != "ready":
            raise ControlledRoleError("libtorrent seed readiness sequence is invalid")
        return process, str(ready["local_address"]), int(ready["listen_port"]), ready
    if implementation != "rstorrent":
        raise ControlledRoleError("seed implementation is unknown")
    command = [
        str(binaries.seed),
        "--profile-root",
        str(case_root / "seed-profile"),
        "--storage-root",
        str(fixture.seed_root),
        "--metainfo",
        str(fixture.metainfo),
        "--controlled-local-network",
        "--encryption",
        "disabled",
        "--utp" if transport == "utp" else "--tcp-only",
    ]
    process = RoleProcess.start(
        command,
        f"RSTorrent {transport} seed",
        accepts_commands=True,
    )
    ready = process.read_event(deadline)
    if ready.get("event") != "ready" or ready.get("registrations") != 1:
        raise ControlledRoleError("RSTorrent seed did not reach registered readiness")
    field = "utp_listen" if transport == "utp" else "listen"
    endpoint = ready.get(field)
    if not isinstance(endpoint, str):
        raise ControlledRoleError("RSTorrent seed omitted the selected listener")
    address, separator, encoded_port = endpoint.rpartition(":")
    if not separator or not encoded_port.isdigit():
        raise ControlledRoleError("RSTorrent seed listener is malformed")
    if address == "0.0.0.0":
        address = "127.0.0.1"
    return process, address, int(encoded_port), ready


def run_leecher(
    implementation: str,
    transport: str,
    fixture: Fixture,
    output_root: Path,
    peer_address: str,
    peer_port: int,
    binaries: LocalBinaries,
    deadline: float,
) -> dict[str, Any]:
    if implementation == "libtorrent":
        arguments = _fixture_arguments(fixture, deadline - time.monotonic())
        arguments[arguments.index("unused")] = transport
        process = RoleProcess.start(
            [
                str(ORACLE_PYTHON),
                str(LIBTORRENT_ROLE),
                "leech",
                *arguments,
                "--storage-root",
                str(output_root),
                "--network-scope",
                "loopback",
                "--peer-address",
                peer_address,
                "--peer-port",
                str(peer_port),
            ],
            f"libtorrent {transport} leecher",
            accepts_commands=False,
        )
        try:
            started = process.read_event(deadline)
            ready = process.read_event(deadline)
            complete = process.read_event(deadline)
            try:
                process.wait_success(deadline)
            except ControlledRoleError as error:
                raise ControlledRoleError(
                    "libtorrent leecher failed: "
                    f"event={complete.get('event')}, reason={complete.get('reason')}"
                ) from error
        finally:
            process.cleanup()
        if (
            started.get("event") != "started"
            or ready.get("event") != "ready"
            or complete.get("event") != "complete"
        ):
            raise ControlledRoleError("libtorrent leecher event sequence is invalid")
        return complete
    if implementation != "rstorrent":
        raise ControlledRoleError("leecher implementation is unknown")
    profile = comparison_profile(f"wan-{transport}")
    process = RoleProcess.start(
        [
            str(binaries.probe),
            "--metainfo",
            str(fixture.metainfo),
            "--expected-info-hash",
            fixture.info_hash,
            "--output",
            str(output_root),
            "--profile",
            profile["name"],
            "--profile-sha256",
            profile["sha256"],
            "--target",
            "complete",
            "--timeout-seconds",
            str(max(1, int(deadline - time.monotonic()) - 35)),
            "--cleanup-seconds",
            "30",
            "--wire-payload-ceiling-bytes",
            str(fixture.payload_bytes * 2),
            "--peer-hint",
            f"{peer_address}:{peer_port}",
        ],
        f"RSTorrent {transport} leecher",
        accepts_commands=False,
    )
    try:
        result = process.read_event(deadline)
        try:
            process.wait_success(deadline)
        except ControlledRoleError as error:
            raise ControlledRoleError(
                "RSTorrent leecher failed: "
                f"outcome={result.get('outcome')}, "
                f"detail={result.get('terminal_detail')}, "
                f"methods={result.get('diagnostics', {}).get('peer_methods')}"
            ) from error
    finally:
        process.cleanup()
    if (
        result.get("outcome") != "milestone_reached"
        or result.get("cleanup_succeeded") is not True
        or result.get("integrity_verified") is not True
        or result.get("effective_settings") != profile["rstorrent"]
    ):
        raise ControlledRoleError("RSTorrent leecher terminal contract failed")
    evidence = result.get("diagnostics", {}).get("peer_methods", {})
    expected = {"tcp_high_water": 1, "utp_high_water": 0}
    if transport == "utp":
        expected = {"tcp_high_water": 0, "utp_high_water": 1}
    if any(evidence.get(name) != value for name, value in expected.items()):
        raise ControlledRoleError("RSTorrent leecher transport evidence is masked")
    milestones = result.get("milestones", {})
    first_payload = milestones.get("first_payload_byte")
    published = milestones.get("published")
    connected = (
        milestones.get("first_connection")
        or milestones.get("first_candidate")
        or milestones.get("torrent_admitted")
    )
    if not all(isinstance(value, (int, float)) for value in (first_payload, published, connected)):
        raise ControlledRoleError("RSTorrent leecher omitted direct timing milestones")
    active = float(published) - float(first_payload)
    connect = float(published) - float(connected)
    if active <= 0 or connect <= active:
        raise ControlledRoleError("RSTorrent leecher timing is invalid")
    return {
        "event": "complete",
        "role": "leech",
        "transport": transport,
        "payload": {
            "bytes": fixture.payload_bytes,
            "pieces": fixture.piece_count,
            "sha1": fixture.sha1,
        },
        "timing": {
            "connect_to_complete_seconds": round(connect, 6),
            "first_payload_seconds": round(float(first_payload) - float(connected), 6),
            "active_payload_seconds": round(active, 6),
        },
        "rstorrent": result,
    }


def stop_seed(
    process: RoleProcess,
    implementation: str,
    transport: str,
    fixture: Fixture,
    deadline: float,
) -> dict[str, Any]:
    process.command("stop")
    stopped = process.read_event(deadline)
    process.wait_success(deadline)
    expected_event = "stopped"
    if stopped.get("event") != expected_event:
        raise ControlledRoleError("seed omitted joined terminal evidence")
    if implementation == "libtorrent":
        stats = stopped.get("libtorrent_stats", {})
        if stats.get("net.sent_payload_bytes", 0) < fixture.payload_bytes:
            raise ControlledRoleError("libtorrent seed payload accounting is incomplete")
    else:
        if stopped.get("payload_bytes_sent", 0) < fixture.payload_bytes:
            raise ControlledRoleError("RSTorrent seed payload accounting is incomplete")
        if transport == "utp":
            utp = stopped.get("utp_before_shutdown", {})
            if (
                utp.get("connection_high_water") != 1
                or utp.get("worker_panics") != 0
            ):
                raise ControlledRoleError("RSTorrent seed uTP ownership did not terminate cleanly")
    return stopped


def run_case(
    seed_implementation: str,
    leech_implementation: str,
    transport: str,
    fixture: Fixture,
    binaries: LocalBinaries,
    root: Path,
    order: int,
    timeout_seconds: int,
) -> dict[str, Any]:
    case_root = root / f"case-{order:02d}-{seed_implementation}-{leech_implementation}-{transport}"
    case_root.mkdir()
    output_root = case_root / "leech"
    deadline = time.monotonic() + timeout_seconds
    seed: RoleProcess | None = None
    try:
        seed, peer_address, peer_port, ready = start_seed(
            seed_implementation,
            transport,
            fixture,
            case_root,
            binaries,
            deadline,
        )
        leech = run_leecher(
            leech_implementation,
            transport,
            fixture,
            output_root,
            peer_address,
            peer_port,
            binaries,
            deadline,
        )
        descriptor = parse_metainfo(fixture.metainfo.read_bytes())
        verification = verify_payload(descriptor, output_root)
        stopped = stop_seed(
            seed,
            seed_implementation,
            transport,
            fixture,
            deadline,
        )
        seed = None
        shutil.rmtree(output_root)
        return {
            "order": order,
            "seed": seed_implementation,
            "leech": leech_implementation,
            "transport": transport,
            "timing": leech["timing"],
            "integrity": {
                "verified": True,
                "pieces": verification["piece_count"],
                "bytes": verification["logical_bytes"],
            },
            "transport_evidence": (
                leech.get("libtorrent_stats")
                if leech_implementation == "libtorrent"
                else {
                    "peer_methods": leech["rstorrent"]
                    .get("diagnostics", {})
                    .get("peer_methods"),
                    "utp": leech["rstorrent"].get("utp_evidence"),
                    "udp": leech["rstorrent"].get("udp_evidence"),
                }
            ),
            "seed_terminal": {
                "event": stopped["event"],
                "payload_accounting_complete": True,
                "transport_evidence": (
                    stopped.get("libtorrent_stats")
                    if seed_implementation == "libtorrent"
                    else stopped.get("utp_before_shutdown")
                    if transport == "utp"
                    else {
                        "connection_high_water": stopped.get("connection_high_water"),
                        "established_high_water": stopped.get("established_high_water"),
                    }
                ),
            },
            "seed_ready": {
                "registered": ready.get("registrations", 1) == 1,
                "transport": transport,
            },
            "cleanup": {
                "succeeded": not output_root.exists(),
                "seed_joined": True,
                "leech_joined": True,
            },
        }
    finally:
        if seed is not None:
            seed.cleanup()
        if output_root.exists():
            shutil.rmtree(output_root, ignore_errors=True)


def run(
    size_mib: int = 8,
    *,
    seeds: tuple[str, ...] = IMPLEMENTATIONS,
    leeches: tuple[str, ...] = IMPLEMENTATIONS,
    transports: tuple[str, ...] = TRANSPORTS,
    case_timeout_seconds: int = 10 * 60,
) -> dict[str, Any]:
    binaries = build_binaries()
    started = time.monotonic()
    results: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="rstorrent-wan-roles-controlled-") as temporary:
        root = Path(temporary)
        fixture = create_fixture(root / "fixture", size_mib)
        order = 0
        for transport in transports:
            for seed in seeds:
                for leech in leeches:
                    order += 1
                    result = run_case(
                        seed,
                        leech,
                        transport,
                        fixture,
                        binaries,
                        root,
                        order,
                        case_timeout_seconds,
                    )
                    results.append(result)
                    print(
                        json.dumps(
                            {
                                "event": "controlled-case-complete",
                                "order": order,
                                "seed": seed,
                                "leech": leech,
                                "transport": transport,
                                "active_mib_per_second": round(
                                    fixture.payload_bytes
                                    / MIB
                                    / result["timing"]["active_payload_seconds"],
                                    6,
                                ),
                            },
                            sort_keys=True,
                        ),
                        file=sys.stderr,
                        flush=True,
                    )
    return {
        "schema_version": 1,
        "scenario": "wan-transport-role-controlled-matrix",
        "libtorrent_version": "2.0.13.0",
        "size_mib": size_mib,
        "piece_bytes": 256 * 1024,
        "cases": results,
        "seconds": round(time.monotonic() - started, 6),
        "cleanup": {"succeeded": True, "root_absent": True},
    }


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--size-mib", type=int, choices=(8, 64), default=8)
    parser.add_argument("--seed", choices=IMPLEMENTATIONS, action="append")
    parser.add_argument("--leech", choices=IMPLEMENTATIONS, action="append")
    parser.add_argument("--transport", choices=TRANSPORTS, action="append")
    parser.add_argument("--case-timeout-seconds", type=int, default=10 * 60)
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    try:
        if not 60 <= arguments.case_timeout_seconds <= 10 * 60:
            raise ControlledRoleError("controlled case timeout is outside 60--600 seconds")
        print(
            json.dumps(
                run(
                    arguments.size_mib,
                    seeds=tuple(arguments.seed or IMPLEMENTATIONS),
                    leeches=tuple(arguments.leech or IMPLEMENTATIONS),
                    transports=tuple(arguments.transport or TRANSPORTS),
                    case_timeout_seconds=arguments.case_timeout_seconds,
                ),
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    except (ControlledRoleError, OSError, subprocess.SubprocessError) as error:
        print(f"controlled WAN role matrix failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
