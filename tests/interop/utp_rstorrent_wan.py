#!/usr/bin/env python3
"""Run one direct public-path RSTorrent-to-pinned-libtorrent uTP transfer."""

from __future__ import annotations

import argparse
import copy
import ipaddress
import json
import queue
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, TextIO

from utp_reference_oracle import (
    MAX_DIAGNOSTICS,
    PAYLOAD_NAME,
    PAYLOAD_SIZE,
    create_fixture,
    hash_file,
)
from utp_remote_seed import (
    MAPPING_DESCRIPTION,
    eligible_public_ipv4,
    parse_mapping_entries,
)
from utp_rstorrent_interop import (
    InteropFailure,
    RoleProcess,
    build_role_binary,
    validate_complete,
)


ROOT = Path(__file__).resolve().parents[2]
REMOTE_SEED_HELPER = ROOT / "tests/interop/utp_remote_seed.py"
REMOTE_LEECHER_HELPER = ROOT / "tests/interop/utp_remote_leecher.py"
SCENARIO_TIMEOUT_SECONDS = 210.0
PROCESS_CLEANUP_SECONDS = 5.0
SSH_OPTIONS = (
    "-o",
    "BatchMode=yes",
    "-o",
    "ConnectTimeout=10",
    "-o",
    "ConnectionAttempts=1",
    "-o",
    "ServerAliveInterval=5",
    "-o",
    "ServerAliveCountMax=2",
)
REMOTE_RUN_PATTERN = re.compile(r"^/tmp/rstorrent-utp-wan\.[A-Za-z0-9]{6}$")
SSH_ALIAS_PATTERN = re.compile(r"^[A-Za-z0-9_.-]+$")
ORACLE_PYTHON = "$HOME/.local/share/rstorrent-oracles/"
ORACLE_PYTHON += "libtorrent-2.0.13-py313-aarch64/bin/python"


class WanFailure(InteropFailure):
    pass


@dataclass
class OutputPump:
    stream: TextIO
    lines: queue.Queue[str | None]
    captured: list[str]

    @classmethod
    def new(cls, stream: TextIO) -> OutputPump:
        return cls(stream, queue.Queue(), [])

    def start(self) -> threading.Thread:
        thread = threading.Thread(target=self._run, daemon=True)
        thread.start()
        return thread

    def _run(self) -> None:
        try:
            for line in self.stream:
                text = line.rstrip("\n")
                self.captured.append(text)
                del self.captured[:-MAX_DIAGNOSTICS]
                self.lines.put(text)
        finally:
            self.lines.put(None)


@dataclass
class RemoteProcess:
    process: subprocess.Popen[str]
    stdout: OutputPump
    stderr: OutputPump
    threads: tuple[threading.Thread, threading.Thread]
    label: str
    accepts_commands: bool

    @classmethod
    def start_seed(
        cls,
        host: str,
        remote_run: str,
        expected_sha1: str,
    ) -> RemoteProcess:
        command = (
            f'exec "{ORACLE_PYTHON}" "{remote_run}/utp_remote_seed.py" '
            f'--metainfo "{remote_run}/forced-utp.torrent" '
            f'--seed-root "{remote_run}/seed" '
            f'--expected-sha1 "{expected_sha1}"'
        )
        process = subprocess.Popen(
            ["ssh", *SSH_OPTIONS, host, command],
            cwd=ROOT,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        if process.stdout is None or process.stderr is None:
            raise WanFailure("failed to capture remote seed output")
        stdout = OutputPump.new(process.stdout)
        stderr = OutputPump.new(process.stderr)
        return cls(
            process,
            stdout,
            stderr,
            (stdout.start(), stderr.start()),
            "remote seed",
            True,
        )

    @classmethod
    def start_leecher(
        cls,
        host: str,
        remote_run: str,
        expected_sha1: str,
        external_address: str,
        external_port: int,
    ) -> RemoteProcess:
        command = (
            f'cd "{remote_run}" && exec "{ORACLE_PYTHON}" '
            f'"{remote_run}/utp_remote_leecher.py" '
            f'--metainfo "{remote_run}/forced-utp.torrent" '
            f'--output-root "{remote_run}/leech" '
            f'--peer-address "{external_address}" '
            f'--peer-port "{external_port}" '
            f'--expected-sha1 "{expected_sha1}"'
        )
        process = subprocess.Popen(
            ["ssh", *SSH_OPTIONS, host, command],
            cwd=ROOT,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        if process.stdout is None or process.stderr is None:
            raise WanFailure("failed to capture remote leecher output")
        stdout = OutputPump.new(process.stdout)
        stderr = OutputPump.new(process.stderr)
        return cls(
            process,
            stdout,
            stderr,
            (stdout.start(), stderr.start()),
            "remote leecher",
            False,
        )

    def read_event(self, deadline: float) -> dict[str, Any]:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise WanFailure(f"{self.label} event exceeded the scenario deadline")
        try:
            line = self.stdout.lines.get(timeout=remaining)
        except queue.Empty as error:
            raise WanFailure(f"{self.label} produced no bounded event") from error
        if line is None:
            raise WanFailure(
                f"{self.label} stopped before its next event: "
                f"{bounded_diagnostics(self.stderr.captured)}"
            )
        try:
            event = json.loads(line)
        except json.JSONDecodeError as error:
            raise WanFailure(f"{self.label} emitted invalid JSON") from error
        if not isinstance(event, dict):
            raise WanFailure(f"{self.label} emitted a non-object event")
        return event

    def command(self, value: str) -> None:
        if not self.accepts_commands or self.process.stdin is None:
            raise WanFailure(f"{self.label} stdin is unavailable")
        self.process.stdin.write(value + "\n")
        self.process.stdin.flush()

    def wait_success(self, deadline: float) -> None:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise WanFailure(f"{self.label} exceeded the scenario deadline")
        try:
            return_code = self.process.wait(timeout=remaining)
        except subprocess.TimeoutExpired as error:
            raise WanFailure(f"{self.label} exceeded the scenario deadline") from error
        for thread in self.threads:
            thread.join(timeout=PROCESS_CLEANUP_SECONDS)
        if return_code != 0:
            raise WanFailure(
                f"{self.label} exited {return_code}: "
                f"{bounded_diagnostics(self.stderr.captured)}"
            )

    def cleanup(self) -> dict[str, Any] | None:
        terminal = None
        if self.process.poll() is None and self.accepts_commands:
            try:
                self.command("abort")
                terminal = self.read_event(time.monotonic() + PROCESS_CLEANUP_SECONDS)
                self.process.wait(timeout=PROCESS_CLEANUP_SECONDS)
            except (BrokenPipeError, subprocess.TimeoutExpired, WanFailure, queue.Empty):
                self.process.terminate()
                try:
                    self.process.wait(timeout=PROCESS_CLEANUP_SECONDS)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait(timeout=PROCESS_CLEANUP_SECONDS)
        elif self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=PROCESS_CLEANUP_SECONDS)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=PROCESS_CLEANUP_SECONDS)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream is not None:
                stream.close()
        return terminal


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", required=True)
    parser.add_argument(
        "--direction",
        choices=("remote-seed", "local-seed", "both"),
    )
    parser.add_argument("--cohort", type=cohort_size, default=1)
    return parser.parse_args()


def cohort_size(value: str) -> int:
    try:
        count = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("cohort must be an integer") from error
    if not 1 <= count <= 3:
        raise argparse.ArgumentTypeError("cohort must be between 1 and 3")
    return count


def bounded_diagnostics(lines: list[str]) -> list[str]:
    result = []
    for line in lines[-MAX_DIAGNOSTICS:]:
        redacted = re.sub(r"(?<![0-9])(?:[0-9]{1,3}\.){3}[0-9]{1,3}(?![0-9])", "<ipv4>", line)
        result.append(redacted[:512])
    return result


def read_role_event(
    role: RoleProcess, deadline: float, expected: str
) -> dict[str, Any]:
    try:
        return role.read_event(deadline)
    except InteropFailure as error:
        raise WanFailure(f"while awaiting {expected}: {error}") from error


def run_ssh(
    host: str,
    command: str,
    *,
    timeout_seconds: float = 15.0,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        ["ssh", *SSH_OPTIONS, host, command],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=timeout_seconds,
        check=False,
    )
    if check and completed.returncode != 0:
        raise WanFailure(
            "bounded remote command failed: "
            f"{bounded_diagnostics(completed.stderr.splitlines())}"
        )
    return completed


def create_remote_run(host: str) -> str:
    completed = run_ssh(host, "mktemp -d /tmp/rstorrent-utp-wan.XXXXXX")
    remote_run = completed.stdout.strip()
    if not REMOTE_RUN_PATTERN.fullmatch(remote_run):
        raise WanFailure("remote mktemp returned an ineligible run directory")
    path = PurePosixPath(remote_run)
    if path.parent != PurePosixPath("/tmp"):
        raise WanFailure("remote run directory escaped /tmp")
    return remote_run


def copy_remote_file(host: str, source: Path, target: str) -> None:
    completed = subprocess.run(
        ["scp", *SSH_OPTIONS, str(source), f"{host}:{target}"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    if completed.returncode != 0:
        raise WanFailure(f"failed to stage bounded remote file {source.name}")


def stage_remote_seed_fixture(
    host: str,
    remote_run: str,
    metainfo: Path,
    payload: Path,
) -> None:
    run_ssh(host, f'mkdir "{remote_run}/seed"')
    for source, target in (
        (REMOTE_SEED_HELPER, f"{remote_run}/utp_remote_seed.py"),
        (metainfo, f"{remote_run}/forced-utp.torrent"),
        (payload, f"{remote_run}/seed/{PAYLOAD_NAME}"),
    ):
        copy_remote_file(host, source, target)


def stage_remote_leecher_fixture(host: str, remote_run: str, metainfo: Path) -> None:
    for source, target in (
        (REMOTE_SEED_HELPER, f"{remote_run}/utp_remote_seed.py"),
        (REMOTE_LEECHER_HELPER, f"{remote_run}/utp_remote_leecher.py"),
        (metainfo, f"{remote_run}/forced-utp.torrent"),
    ):
        copy_remote_file(host, source, target)


def eligible_public_endpoint(event: dict[str, Any]) -> tuple[str, int, int, int]:
    if event.get("event") != "ready" or event.get("role") != "remote-seed":
        raise WanFailure("remote seed did not emit mapped readiness")
    if event.get("libtorrent_version") != "2.0.13.0":
        raise WanFailure("remote seed reported the wrong libtorrent version")
    address = event.get("external_address")
    port = event.get("external_port")
    listen_port = event.get("listen_port")
    pid = event.get("pid")
    if not isinstance(address, str):
        raise WanFailure("remote seed omitted its external address")
    try:
        parsed = ipaddress.ip_address(address)
    except ValueError as error:
        raise WanFailure("remote seed reported an invalid external address") from error
    if not isinstance(parsed, ipaddress.IPv4Address) or not parsed.is_global:
        raise WanFailure("remote seed external address is not public IPv4")
    if not all(isinstance(value, int) for value in (port, listen_port, pid)):
        raise WanFailure("remote seed readiness omitted bounded integer fields")
    assert isinstance(port, int)
    assert isinstance(listen_port, int)
    assert isinstance(pid, int)
    if not 1 <= port <= 65535 or port != listen_port or pid <= 1:
        raise WanFailure("remote seed readiness fields are inconsistent")
    mapping = event.get("mapping")
    if not isinstance(mapping, dict) or not (
        mapping.get("protocol") == "UDP"
        and mapping.get("transport") == "UPnP"
        and isinstance(mapping.get("lease_seconds"), int)
        and 0 < mapping["lease_seconds"] <= 3_600
    ):
        raise WanFailure("remote seed mapping evidence is invalid")
    return address, port, listen_port, pid


def remote_started_pid(event: dict[str, Any]) -> int:
    if event.get("event") != "started" or event.get("role") != "remote-seed":
        raise WanFailure("remote seed did not emit its ownership event")
    pid = event.get("pid")
    if not isinstance(pid, int) or pid <= 1:
        raise WanFailure("remote seed ownership event has an invalid PID")
    return pid


def remote_leecher_started_pid(event: dict[str, Any]) -> int:
    if event.get("event") != "started" or event.get("role") != "remote-leecher":
        raise WanFailure("remote leecher did not emit its ownership event")
    pid = event.get("pid")
    if not isinstance(pid, int) or pid <= 1:
        raise WanFailure("remote leecher ownership event has an invalid PID")
    return pid


def validate_remote_leecher_ready(event: dict[str, Any], expected_pid: int) -> None:
    if event.get("event") != "ready" or event.get("role") != "remote-leecher":
        raise WanFailure("remote leecher did not become ready")
    if (
        event.get("pid") != expected_pid
        or event.get("libtorrent_version") != "2.0.13.0"
        or event.get("route_class") != "ordinary-internet"
    ):
        raise WanFailure("remote leecher readiness evidence is inconsistent")
    port = event.get("listen_port")
    if not isinstance(port, int) or not 1 <= port <= 65535:
        raise WanFailure("remote leecher reported an invalid listener port")


def ssh_control_address(host: str) -> ipaddress.IPv4Address | None:
    completed = subprocess.run(
        ["ssh", "-G", host],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=5,
        check=False,
    )
    if completed.returncode != 0:
        raise WanFailure("could not resolve SSH control configuration")
    hostname = next(
        (
            line.split(maxsplit=1)[1]
            for line in completed.stdout.splitlines()
            if line.startswith("hostname ")
        ),
        None,
    )
    if hostname is None:
        raise WanFailure("SSH control configuration omitted its hostname")
    try:
        address = ipaddress.ip_address(hostname)
    except ValueError:
        return None
    return address if isinstance(address, ipaddress.IPv4Address) else None


def verify_direct_route(host: str, external_address: str) -> str:
    external = ipaddress.ip_address(external_address)
    control = ssh_control_address(host)
    if control is not None and control == external:
        raise WanFailure("uTP endpoint equals the SSH control endpoint")
    if shutil.which("route") is not None:
        completed = subprocess.run(
            ["route", "-n", "get", external_address],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
        match = re.search(r"^\s*interface:\s*(\S+)\s*$", completed.stdout, re.MULTILINE)
        interface = match.group(1) if completed.returncode == 0 and match else None
    elif shutil.which("ip") is not None:
        completed = subprocess.run(
            ["ip", "-4", "route", "get", external_address],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
        match = re.search(r"\bdev\s+(\S+)", completed.stdout)
        interface = match.group(1) if completed.returncode == 0 and match else None
    else:
        raise WanFailure("local host has no supported route inspection command")
    if interface is None:
        raise WanFailure("could not resolve the local route to the public endpoint")
    lowered = interface.lower()
    if lowered.startswith(("utun", "tailscale", "tun", "tap", "wg", "lo")):
        raise WanFailure("public uTP endpoint routes through an overlay interface")
    return interface


def validate_wan_ready(event: dict[str, Any]) -> None:
    if event.get("event") != "ready" or event.get("role") != "wan-leecher":
        raise WanFailure("RSTorrent WAN leecher did not become ready")
    listen = event.get("listen")
    if not isinstance(listen, str) or not listen.startswith("0.0.0.0:"):
        raise WanFailure("RSTorrent WAN leecher did not bind wildcard IPv4")


def validate_remote_complete(event: dict[str, Any]) -> None:
    if event.get("event") != "complete" or event.get("role") != "remote-seed":
        raise WanFailure("remote seed did not emit terminal evidence")
    if event.get("peer_high_water") != 1 or event.get("mapping_deleted") is not True:
        raise WanFailure("remote seed peer or mapping cleanup evidence failed")
    diagnostics = event.get("diagnostics")
    if not isinstance(diagnostics, list) or len(diagnostics) > MAX_DIAGNOSTICS:
        raise WanFailure("remote seed diagnostics exceeded their bound")
    stats = event.get("libtorrent_stats")
    if not isinstance(stats, dict) or not (
        stats.get("peer.num_tcp_peers") == 0
        and stats.get("utp.utp_packets_in", 0) > 0
        and stats.get("utp.utp_packets_out", 0) > 0
        and stats.get("net.sent_payload_bytes", 0) >= PAYLOAD_SIZE
    ):
        raise WanFailure("remote seed did not prove forced-uTP payload transfer")


def validate_remote_leecher_complete(
    event: dict[str, Any], expected_sha1: str
) -> None:
    if event.get("event") != "complete" or event.get("role") != "remote-leecher":
        raise WanFailure("remote leecher did not emit terminal evidence")
    if event.get("peer_high_water") != 1:
        raise WanFailure("remote leecher exceeded or missed its one-peer bound")
    payload = event.get("payload")
    if not isinstance(payload, dict) or not (
        payload.get("bytes") == PAYLOAD_SIZE
        and payload.get("pieces") == 33
        and payload.get("sha1") == expected_sha1
    ):
        raise WanFailure("remote leecher payload evidence failed")
    diagnostics = event.get("diagnostics")
    if not isinstance(diagnostics, list) or len(diagnostics) > MAX_DIAGNOSTICS:
        raise WanFailure("remote leecher diagnostics exceeded their bound")
    stats = event.get("libtorrent_stats")
    if not isinstance(stats, dict) or not (
        stats.get("peer.num_tcp_peers") == 0
        and stats.get("utp.utp_packets_in", 0) > 0
        and stats.get("utp.utp_packets_out", 0) > 0
        and stats.get("net.recv_payload_bytes", 0) >= PAYLOAD_SIZE
    ):
        raise WanFailure("remote leecher did not prove forced-uTP payload transfer")
    transfer_seconds = event.get("transfer_seconds")
    if not isinstance(transfer_seconds, (int, float)) or not (
        0 < transfer_seconds <= 180
    ):
        raise WanFailure("remote leecher transfer duration is invalid")


def validate_local_seed_bound(event: dict[str, Any]) -> int:
    if event.get("event") != "bound" or event.get("role") != "wan-seed":
        raise WanFailure("RSTorrent WAN seed did not emit its bound event")
    listen = event.get("listen")
    if not isinstance(listen, str):
        raise WanFailure("RSTorrent WAN seed omitted its bound endpoint")
    address_text, separator, port_text = listen.rpartition(":")
    try:
        address = ipaddress.ip_address(address_text)
        port = int(port_text)
    except ValueError as error:
        raise WanFailure("RSTorrent WAN seed bound endpoint is invalid") from error
    if (
        separator != ":"
        or not isinstance(address, ipaddress.IPv4Address)
        or address.is_unspecified
        or address.is_loopback
        or not 1 <= port <= 65535
    ):
        raise WanFailure("RSTorrent WAN seed bound an ineligible endpoint")
    return port


def validate_mapping_intent(event: dict[str, Any], local_port: int) -> int:
    if event.get("event") != "mapping-intent" or event.get("role") != "wan-seed":
        raise WanFailure("RSTorrent WAN seed omitted exact mapping intent")
    external_port = event.get("external_port")
    if not (
        event.get("local_port") == local_port
        and external_port == local_port
        and event.get("protocol") == "UDP"
    ):
        raise WanFailure("RSTorrent WAN seed mapping intent is inconsistent")
    assert isinstance(external_port, int)
    return external_port


def eligible_local_seed_endpoint(
    event: dict[str, Any], expected_port: int
) -> tuple[str, int]:
    if event.get("event") != "ready" or event.get("role") != "wan-seed":
        raise WanFailure("RSTorrent WAN seed did not emit mapped readiness")
    address = event.get("external_address")
    port = event.get("external_port")
    if not isinstance(address, str) or not eligible_public_ipv4(address):
        raise WanFailure("RSTorrent WAN seed mapping is not public IPv4")
    if port != expected_port:
        raise WanFailure("RSTorrent WAN seed changed its exact external port")
    mapping = event.get("mapping")
    if not isinstance(mapping, dict) or not (
        mapping.get("protocol") == "UDP"
        and mapping.get("transport") == "UPnP"
        and isinstance(mapping.get("lease_seconds"), int)
        and 0 < mapping["lease_seconds"] <= 3_600
    ):
        raise WanFailure("RSTorrent WAN seed mapping evidence is invalid")
    return address, expected_port


def validate_local_seed_complete(event: dict[str, Any], expected_sha1: str) -> None:
    validate_complete(event, "wan-seed", expected_sha1)
    if event.get("mapping_deleted") is not True:
        raise WanFailure("RSTorrent WAN seed did not delete its UDP mapping")
    peer_evidence = event.get("peer_evidence")
    if not isinstance(peer_evidence, dict) or not (
        peer_evidence.get("connection_high_water") == 1
        and peer_evidence.get("utp_high_water") == 1
        and peer_evidence.get("tcp_high_water") == 0
    ):
        raise WanFailure("RSTorrent WAN seed peer transport evidence failed")
    terminal = event.get("resources", {}).get("terminal_incoming", {})
    if not (
        terminal.get("pending") == 0
        and terminal.get("established") == 0
        and terminal.get("connections") == 0
        and terminal.get("registrations") == 0
    ):
        raise WanFailure("RSTorrent WAN seed retained incoming-peer ownership")


def aborted_remote_summary(event: dict[str, Any] | None) -> str:
    if not isinstance(event, dict) or event.get("event") != "aborted":
        return "remote abort evidence unavailable"
    stats = event.get("libtorrent_stats")
    if not isinstance(stats, dict):
        return "remote abort statistics unavailable"
    return (
        "remote abort evidence: "
        f"utp_in={stats.get('utp.utp_packets_in', 'missing')}, "
        f"utp_out={stats.get('utp.utp_packets_out', 'missing')}, "
        f"utp_peers={stats.get('peer.num_utp_peers', 'missing')}, "
        f"payload_sent={stats.get('net.sent_payload_bytes', 'missing')}, "
        f"mapping_deleted={event.get('mapping_deleted') is True}"
    )


def verify_remote_seed_cleanup(
    host: str,
    remote_run: str,
    pid: int | None,
    external_port: int | None,
) -> None:
    if pid is not None:
        process_check = run_ssh(host, f"kill -0 {pid} 2>/dev/null", check=False)
        if process_check.returncode == 0:
            raise WanFailure("remote seed process survived cleanup")
    inventory = run_ssh(host, "upnpc -l", timeout_seconds=12)
    entries = parse_mapping_entries(inventory.stdout + inventory.stderr)
    retained = [
        entry
        for entry in entries
        if entry.protocol == "UDP"
        and entry.description == MAPPING_DESCRIPTION
    ]
    if len(retained) > 1:
        raise WanFailure("multiple owned UDP mappings survived cleanup")
    if retained:
        retained_port = retained[0].external_port
        if external_port is not None and retained_port != external_port:
            cleanup_mismatch = True
        else:
            cleanup_mismatch = False
        run_ssh(host, f"upnpc -d {retained_port} UDP", timeout_seconds=12, check=False)
        inventory = run_ssh(host, "upnpc -l", timeout_seconds=12)
        entries = parse_mapping_entries(inventory.stdout + inventory.stderr)
        if any(entry.description == MAPPING_DESCRIPTION for entry in entries):
            raise WanFailure("remote UDP mapping survived exact cleanup")
        if cleanup_mismatch:
            raise WanFailure("remote cleanup found an unexpected owned UDP port")
    if not REMOTE_RUN_PATTERN.fullmatch(remote_run):
        raise WanFailure("refusing to remove an ineligible remote run directory")
    run_ssh(host, f'rm -r -- "{remote_run}"')
    absent = run_ssh(host, f'test ! -e "{remote_run}"', check=False)
    if absent.returncode != 0:
        raise WanFailure("remote run directory survived cleanup")


def verify_remote_leecher_cleanup(
    host: str,
    remote_run: str,
    pid: int | None,
) -> None:
    if pid is not None:
        process_check = run_ssh(host, f"kill -0 {pid} 2>/dev/null", check=False)
        if process_check.returncode == 0:
            raise WanFailure("remote leecher process survived cleanup")
    if not REMOTE_RUN_PATTERN.fullmatch(remote_run):
        raise WanFailure("refusing to remove an ineligible remote run directory")
    run_ssh(host, f'rm -r -- "{remote_run}"')
    absent = run_ssh(host, f'test ! -e "{remote_run}"', check=False)
    if absent.returncode != 0:
        raise WanFailure("remote run directory survived cleanup")


def audit_local_mapping(binary: Path, local_port: int, external_port: int) -> dict[str, Any]:
    completed = subprocess.run(
        [
            str(binary),
            "wan-mapping-audit",
            "--local-port",
            str(local_port),
            "--external-port",
            str(external_port),
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    if completed.returncode != 0:
        raise WanFailure(
            "local exact mapping audit failed: "
            f"{bounded_diagnostics(completed.stderr.splitlines())}"
        )
    try:
        event = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise WanFailure("local exact mapping audit emitted invalid JSON") from error
    if not isinstance(event, dict) or not (
        event.get("event") == "mapping-audit"
        and event.get("role") == "wan-mapping-audit"
        and event.get("owned_mapping_absent") is True
    ):
        raise WanFailure("local exact mapping audit did not prove absence")
    if event.get("foreign_mapping_preserved") is True:
        raise WanFailure("local exact port became foreign during the bounded run")
    return event


def redacted_rstorrent(event: dict[str, Any]) -> dict[str, Any]:
    result = copy.deepcopy(event)
    result.pop("remote_peer_id", None)
    if "peer" in result:
        result["peer"] = "<public-ip>:<transient-port>"
    if "listen" in result:
        result["listen"] = "<local-ip>:<transient-port>"
    peer_evidence = result.get("peer_evidence")
    if isinstance(peer_evidence, dict) and "endpoints" in peer_evidence:
        peer_evidence["endpoints"] = ["<public-ip>:<transient-port>"]
    return result


def run_remote_seed_direction(host: str, binary: Path) -> dict[str, Any]:
    started = time.monotonic()
    remote_run: str | None = None
    remote: RemoteProcess | None = None
    role: RoleProcess | None = None
    remote_pid: int | None = None
    external_port: int | None = None
    result: dict[str, Any] | None = None
    run_error: Exception | None = None
    cleanup_error: Exception | None = None
    remote_abort: dict[str, Any] | None = None
    with tempfile.TemporaryDirectory(prefix="rstorrent-utp-wan-") as temporary:
        local_root = Path(temporary)
        torrent_info, seed_root, expected_sha1 = create_fixture(local_root)
        del torrent_info
        output = local_root / "rstorrent-leech" / PAYLOAD_NAME
        deadline = time.monotonic() + SCENARIO_TIMEOUT_SECONDS
        try:
            remote_run = create_remote_run(host)
            stage_remote_seed_fixture(
                host,
                remote_run,
                local_root / "forced-utp.torrent",
                seed_root / PAYLOAD_NAME,
            )
            remote = RemoteProcess.start_seed(host, remote_run, expected_sha1)
            remote_pid = remote_started_pid(remote.read_event(deadline))
            ready = remote.read_event(deadline)
            external_address, external_port, _, ready_pid = eligible_public_endpoint(ready)
            if ready_pid != remote_pid:
                raise WanFailure("remote seed ownership changed before readiness")
            verify_direct_route(host, external_address)

            role = RoleProcess.start(
                binary,
                [
                    "wan-leecher",
                    "--metainfo",
                    str(local_root / "forced-utp.torrent"),
                    "--peer",
                    f"{external_address}:{external_port}",
                    "--output",
                    str(output),
                ],
            )
            transfer_started = time.monotonic()
            validate_wan_ready(role.read_event(deadline))
            complete = role.read_event(deadline)
            role.wait_success(deadline)
            validate_complete(complete, "wan-leecher", expected_sha1)
            if (
                not output.is_file()
                or output.stat().st_size != PAYLOAD_SIZE
                or hash_file(output) != expected_sha1
                or complete.get("payload", {}).get("sha1") != expected_sha1
            ):
                raise WanFailure("RSTorrent WAN output failed exact verification")

            remote.command("stop")
            remote_complete = remote.read_event(deadline)
            remote.wait_success(deadline)
            validate_remote_complete(remote_complete)
            result = {
                "schema_version": 1,
                "oracle": "rstorrent-to-pinned-libtorrent-mapped-utp-wan",
                "direction": "local-rstorrent-leecher-to-remote-libtorrent-seed",
                "libtorrent_version": "2.0.13.0",
                "transport": {
                    "tcp_incoming": False,
                    "tcp_outgoing": False,
                    "mse": False,
                    "dht": False,
                    "lsd": False,
                    "natpmp": False,
                    "upnp_udp_mapping": True,
                    "ssh_data_path": False,
                    "route_class": "ordinary-internet",
                    "endpoint": "<public-ip>:<transient-port>",
                },
                "payload": {
                    "bytes": PAYLOAD_SIZE,
                    "piece_bytes": 64 * 1024,
                    "sha1": expected_sha1,
                },
                "remote": {
                    "peer_high_water": remote_complete["peer_high_water"],
                    "libtorrent_stats": remote_complete["libtorrent_stats"],
                    "mapping": {
                        "protocol": "UDP",
                        "transport": "UPnP",
                        "lease_seconds": ready["mapping"]["lease_seconds"],
                        "deleted": True,
                    },
                    "diagnostics": bounded_diagnostics(remote_complete["diagnostics"]),
                },
                "rstorrent": redacted_rstorrent(complete),
                "active_transfer_seconds": round(
                    time.monotonic() - transfer_started, 6
                ),
                "seconds": round(time.monotonic() - started, 6),
            }
        except Exception as error:
            run_error = error
            if remote is not None and remote.process.poll() is None:
                try:
                    remote.command("abort")
                    remote_abort = remote.read_event(deadline)
                    remote.wait_success(deadline)
                except Exception as abort_error:
                    run_error = WanFailure(
                        f"{error}; remote abort failed: {abort_error}"
                    )
        finally:
            if role is not None:
                role.cleanup()
            if remote is not None:
                cleanup_terminal = remote.cleanup()
                if cleanup_terminal is not None:
                    remote_abort = cleanup_terminal
            if remote_run is not None:
                try:
                    verify_remote_seed_cleanup(
                        host,
                        remote_run,
                        remote_pid,
                        external_port,
                    )
                except Exception as error:
                    cleanup_error = error
    if cleanup_error is not None:
        raise WanFailure(f"WAN cleanup failed: {cleanup_error}") from cleanup_error
    if run_error is not None:
        raise WanFailure(f"{run_error}; {aborted_remote_summary(remote_abort)}") from run_error
    if result is None:
        raise WanFailure("mapped WAN case produced no result")
    result["cleanup"] = {
        "succeeded": True,
        "remote_mapping_deleted": True,
        "remote_process_absent": True,
        "remote_run_directory_removed": True,
        "local_temporary_directory_removed": True,
    }
    result["seconds"] = round(time.monotonic() - started, 6)
    return result


def run_local_seed_direction(host: str, binary: Path) -> dict[str, Any]:
    started = time.monotonic()
    remote_run: str | None = None
    remote: RemoteProcess | None = None
    role: RoleProcess | None = None
    remote_pid: int | None = None
    local_port: int | None = None
    external_port: int | None = None
    local_complete: dict[str, Any] | None = None
    result: dict[str, Any] | None = None
    run_error: Exception | None = None
    cleanup_errors: list[str] = []
    audit: dict[str, Any] | None = None
    with tempfile.TemporaryDirectory(prefix="rstorrent-utp-wan-") as temporary:
        local_root = Path(temporary)
        torrent_info, seed_root, expected_sha1 = create_fixture(local_root)
        del torrent_info
        deadline = time.monotonic() + SCENARIO_TIMEOUT_SECONDS
        try:
            remote_run = create_remote_run(host)
            stage_remote_leecher_fixture(
                host,
                remote_run,
                local_root / "forced-utp.torrent",
            )
            role = RoleProcess.start(
                binary,
                [
                    "wan-seed",
                    "--metainfo",
                    str(local_root / "forced-utp.torrent"),
                    "--storage-root",
                    str(seed_root),
                ],
            )
            local_port = validate_local_seed_bound(
                read_role_event(role, deadline, "local bind ownership")
            )
            external_port = local_port
            validate_mapping_intent(
                read_role_event(role, deadline, "local exact mapping intent"),
                local_port,
            )
            ready = read_role_event(role, deadline, "local mapped readiness")
            external_address, external_port = eligible_local_seed_endpoint(
                ready, external_port
            )

            remote = RemoteProcess.start_leecher(
                host,
                remote_run,
                expected_sha1,
                external_address,
                external_port,
            )
            remote_pid = remote_leecher_started_pid(remote.read_event(deadline))
            validate_remote_leecher_ready(remote.read_event(deadline), remote_pid)
            remote_complete = remote.read_event(deadline)
            remote.wait_success(deadline)
            validate_remote_leecher_complete(remote_complete, expected_sha1)

            role.send_stop()
            local_complete = role.read_event(deadline)
            role.wait_success(deadline)
            validate_local_seed_complete(local_complete, expected_sha1)
            result = {
                "schema_version": 1,
                "oracle": "pinned-libtorrent-to-rstorrent-mapped-utp-wan",
                "direction": "remote-libtorrent-leecher-to-local-rstorrent-seed",
                "libtorrent_version": "2.0.13.0",
                "transport": {
                    "tcp_incoming": False,
                    "tcp_outgoing": False,
                    "mse": False,
                    "dht": False,
                    "lsd": False,
                    "natpmp": False,
                    "upnp_udp_mapping": True,
                    "mapping_owner": "local-rstorrent",
                    "ssh_data_path": False,
                    "route_class": "ordinary-internet",
                    "endpoint": "<public-ip>:<transient-port>",
                },
                "payload": {
                    "bytes": PAYLOAD_SIZE,
                    "piece_bytes": 64 * 1024,
                    "sha1": expected_sha1,
                },
                "remote": {
                    "peer_high_water": remote_complete["peer_high_water"],
                    "libtorrent_stats": remote_complete["libtorrent_stats"],
                    "diagnostics": bounded_diagnostics(
                        remote_complete["diagnostics"]
                    ),
                },
                "rstorrent": redacted_rstorrent(local_complete),
                "active_transfer_seconds": remote_complete["transfer_seconds"],
                "mapping": {
                    "protocol": "UDP",
                    "transport": "UPnP",
                    "lease_seconds": ready["mapping"]["lease_seconds"],
                    "deleted": True,
                },
                "seconds": round(time.monotonic() - started, 6),
            }
        except Exception as error:
            run_error = error
        finally:
            if role is not None and role.process.poll() is None:
                try:
                    role.send_stop()
                    cleanup_terminal = role.read_event(
                        time.monotonic() + PROCESS_CLEANUP_SECONDS
                    )
                    role.wait_success(time.monotonic() + PROCESS_CLEANUP_SECONDS)
                    if local_complete is None:
                        local_complete = cleanup_terminal
                except Exception:
                    pass
            if role is not None:
                role.cleanup()
            if remote is not None:
                remote.cleanup()
            if remote_run is not None:
                try:
                    verify_remote_leecher_cleanup(host, remote_run, remote_pid)
                except Exception as error:
                    cleanup_errors.append(str(error))
            if local_port is not None and external_port is not None:
                try:
                    audit = audit_local_mapping(binary, local_port, external_port)
                    if (
                        local_complete is not None
                        and local_complete.get("mapping_deleted") is True
                        and audit.get("owned_mapping_found") is True
                    ):
                        cleanup_errors.append(
                            "normal shutdown claimed deletion but audit found the mapping"
                        )
                except Exception as error:
                    cleanup_errors.append(str(error))
    if cleanup_errors:
        cleanup = "; ".join(cleanup_errors)
        raise WanFailure(f"WAN cleanup failed: {cleanup}")
    if run_error is not None:
        raise WanFailure(str(run_error)) from run_error
    if result is None or audit is None:
        raise WanFailure("mapped WAN case produced no result")
    result["cleanup"] = {
        "succeeded": True,
        "local_mapping_absent": audit["owned_mapping_absent"],
        "local_mapping_recovered_by_audit": audit["owned_mapping_deleted"],
        "remote_process_absent": True,
        "remote_run_directory_removed": True,
        "local_temporary_directory_removed": True,
    }
    result["seconds"] = round(time.monotonic() - started, 6)
    return result


def run(host: str, direction: str = "remote-seed") -> dict[str, Any]:
    if not SSH_ALIAS_PATTERN.fullmatch(host) or host.startswith("-"):
        raise WanFailure("SSH host alias is malformed")
    binary = build_role_binary()
    if direction == "remote-seed":
        return run_remote_seed_direction(host, binary)
    if direction == "local-seed":
        return run_local_seed_direction(host, binary)
    raise WanFailure("WAN direction is invalid")


def sample_metrics(sample: dict[str, Any]) -> dict[str, int | float]:
    rstorrent = sample["rstorrent"]
    resources = rstorrent["resources"]
    utp = resources["live_utp"]
    udp = resources["live_udp"]
    oracle = sample["remote"]["libtorrent_stats"]
    metrics: dict[str, int | float] = {
        "case_seconds": sample["seconds"],
        "active_transfer_seconds": sample["active_transfer_seconds"],
        "rstorrent_datagrams_sent": utp["datagrams_sent"],
        "rstorrent_datagram_bytes_sent": utp["datagram_bytes_sent"],
        "rstorrent_datagrams_received": udp["utp_datagrams_classified"],
        "rstorrent_datagram_bytes_received": udp[
            "utp_datagram_bytes_classified"
        ],
        "rstorrent_choke_retries": rstorrent["payload"].get("choke_retries", 0),
        "rstorrent_duplicate_blocks": rstorrent["payload"].get("duplicate_blocks", 0),
    }
    for name in (
        "smoothed_rtt_min_micros",
        "smoothed_rtt_max_micros",
        "effective_rto_min_micros",
        "effective_rto_max_micros",
        "base_delay_min_micros",
        "base_delay_max_micros",
        "queue_delay_min_micros",
        "queue_delay_max_micros",
        "congestion_window_min_bytes",
        "congestion_window_max_bytes",
        "advertised_receive_window_min_bytes",
        "advertised_receive_window_max_bytes",
        "selected_mtu_min_bytes",
        "selected_mtu_max_bytes",
        "connection_datagram_queue_high_water",
        "retransmission_queue_high_water",
        "delivered_byte_high_water",
        "unsent_byte_high_water",
        "sent_byte_high_water",
        "retransmission_datagrams_sent",
        "retransmission_bytes_sent",
        "loss_reduction_high_water",
        "timeout_collapse_high_water",
    ):
        metrics[f"rstorrent_{name}"] = utp[name]
    for name in (
        "net.sent_payload_bytes",
        "net.recv_payload_bytes",
        "utp.utp_packets_in",
        "utp.utp_packets_out",
        "utp.utp_payload_pkts_in",
        "utp.utp_payload_pkts_out",
        "utp.utp_packet_loss",
        "utp.utp_timeout",
        "utp.utp_fast_retransmit",
        "utp.utp_packet_resend",
    ):
        metrics[f"libtorrent_{name.replace('.', '_')}"] = oracle[name]
    return metrics


def summarize_samples(samples: list[dict[str, Any]]) -> dict[str, Any]:
    metrics = [sample_metrics(sample) for sample in samples]
    names = metrics[0].keys()
    summary = {}
    for name in names:
        values = [sample[name] for sample in metrics]
        summary[name] = {
            "min": min(values),
            "median": statistics.median(values),
            "max": max(values),
        }
    return {
        "samples": len(samples),
        "metrics": summary,
    }


def run_cohort(
    host: str,
    count: int,
    direction: str | None = None,
) -> dict[str, Any]:
    if not SSH_ALIAS_PATTERN.fullmatch(host) or host.startswith("-"):
        raise WanFailure("SSH host alias is malformed")
    if not 1 <= count <= 3:
        raise WanFailure("WAN cohort is outside its one-to-three sample bound")
    if direction in (None, "both"):
        directions = ("remote-seed", "local-seed")
    elif direction in ("remote-seed", "local-seed"):
        directions = (direction,)
    else:
        raise WanFailure("WAN cohort direction is invalid")
    binary = build_role_binary()
    started = time.monotonic()
    samples: list[dict[str, Any]] = []
    for _ in range(count):
        for selected in directions:
            if selected == "remote-seed":
                samples.append(run_remote_seed_direction(host, binary))
            else:
                samples.append(run_local_seed_direction(host, binary))
    grouped = {
        selected: [
            sample
            for sample in samples
            if (
                selected == "remote-seed"
                and sample["direction"].startswith("local-rstorrent-leecher")
            )
            or (
                selected == "local-seed"
                and sample["direction"].startswith("remote-libtorrent-leecher")
            )
        ]
        for selected in directions
    }
    return {
        "schema_version": 1,
        "oracle": "rstorrent-pinned-libtorrent-mapped-utp-wan-cohort",
        "libtorrent_version": "2.0.13.0",
        "samples_per_direction": count,
        "direction_order": list(directions),
        "samples": samples,
        "summaries": {
            selected: summarize_samples(grouped[selected]) for selected in directions
        },
        "cleanup": {
            "succeeded": all(sample["cleanup"]["succeeded"] for sample in samples),
            "all_mappings_absent": True,
            "all_processes_absent": True,
            "all_run_directories_removed": True,
        },
        "seconds": round(time.monotonic() - started, 6),
    }


def main() -> int:
    arguments = parse_arguments()
    if arguments.cohort == 1 and arguments.direction not in (None, "both"):
        result = run(arguments.host, arguments.direction)
    elif arguments.cohort == 1 and arguments.direction is None:
        result = run(arguments.host)
    else:
        result = run_cohort(arguments.host, arguments.cohort, arguments.direction)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (WanFailure, subprocess.TimeoutExpired) as error:
        print(f"RSTorrent uTP WAN interoperability failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
