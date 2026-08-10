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
from utp_remote_seed import MAPPING_DESCRIPTION, parse_mapping_entries
from utp_rstorrent_interop import (
    InteropFailure,
    RoleProcess,
    build_role_binary,
    validate_complete,
)


ROOT = Path(__file__).resolve().parents[2]
REMOTE_HELPER = ROOT / "tests/interop/utp_remote_seed.py"
SCENARIO_TIMEOUT_SECONDS = 90.0
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
class RemoteSeedProcess:
    process: subprocess.Popen[str]
    stdout: OutputPump
    stderr: OutputPump
    threads: tuple[threading.Thread, threading.Thread]

    @classmethod
    def start(
        cls,
        host: str,
        remote_run: str,
        expected_sha1: str,
    ) -> RemoteSeedProcess:
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
        return cls(process, stdout, stderr, (stdout.start(), stderr.start()))

    def read_event(self, deadline: float) -> dict[str, Any]:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise WanFailure("remote seed event exceeded the scenario deadline")
        try:
            line = self.stdout.lines.get(timeout=remaining)
        except queue.Empty as error:
            raise WanFailure("remote seed produced no bounded event") from error
        if line is None:
            raise WanFailure(
                "remote seed stopped before its next event: "
                f"{bounded_diagnostics(self.stderr.captured)}"
            )
        try:
            event = json.loads(line)
        except json.JSONDecodeError as error:
            raise WanFailure("remote seed emitted invalid JSON") from error
        if not isinstance(event, dict):
            raise WanFailure("remote seed emitted a non-object event")
        return event

    def command(self, value: str) -> None:
        if self.process.stdin is None:
            raise WanFailure("remote seed stdin is unavailable")
        self.process.stdin.write(value + "\n")
        self.process.stdin.flush()

    def wait_success(self, deadline: float) -> None:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise WanFailure("remote seed exceeded the scenario deadline")
        try:
            return_code = self.process.wait(timeout=remaining)
        except subprocess.TimeoutExpired as error:
            raise WanFailure("remote seed exceeded the scenario deadline") from error
        for thread in self.threads:
            thread.join(timeout=PROCESS_CLEANUP_SECONDS)
        if return_code != 0:
            raise WanFailure(
                f"remote seed exited {return_code}: "
                f"{bounded_diagnostics(self.stderr.captured)}"
            )

    def cleanup(self) -> None:
        if self.process.poll() is None:
            try:
                self.command("abort")
                self.process.wait(timeout=PROCESS_CLEANUP_SECONDS)
            except (BrokenPipeError, subprocess.TimeoutExpired, WanFailure):
                self.process.terminate()
                try:
                    self.process.wait(timeout=PROCESS_CLEANUP_SECONDS)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait(timeout=PROCESS_CLEANUP_SECONDS)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream is not None:
                stream.close()


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", required=True)
    return parser.parse_args()


def bounded_diagnostics(lines: list[str]) -> list[str]:
    result = []
    for line in lines[-MAX_DIAGNOSTICS:]:
        redacted = re.sub(r"(?<![0-9])(?:[0-9]{1,3}\.){3}[0-9]{1,3}(?![0-9])", "<ipv4>", line)
        result.append(redacted[:512])
    return result


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


def stage_remote_fixture(
    host: str,
    remote_run: str,
    metainfo: Path,
    payload: Path,
) -> None:
    run_ssh(host, f'mkdir "{remote_run}/seed"')
    for source, target in (
        (REMOTE_HELPER, f"{remote_run}/utp_remote_seed.py"),
        (metainfo, f"{remote_run}/forced-utp.torrent"),
        (payload, f"{remote_run}/seed/{PAYLOAD_NAME}"),
    ):
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


def verify_remote_cleanup(
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


def redacted_rstorrent(event: dict[str, Any]) -> dict[str, Any]:
    result = copy.deepcopy(event)
    result.pop("remote_peer_id", None)
    result["peer"] = "<public-ip>:<transient-port>"
    result["listen"] = "0.0.0.0:<transient-port>"
    return result


def run(host: str) -> dict[str, Any]:
    if not SSH_ALIAS_PATTERN.fullmatch(host) or host.startswith("-"):
        raise WanFailure("SSH host alias is malformed")
    binary = build_role_binary()
    started = time.monotonic()
    remote_run: str | None = None
    remote: RemoteSeedProcess | None = None
    role: RoleProcess | None = None
    remote_pid: int | None = None
    external_port: int | None = None
    result: dict[str, Any] | None = None
    run_error: Exception | None = None
    cleanup_error: Exception | None = None
    with tempfile.TemporaryDirectory(prefix="rstorrent-utp-wan-") as temporary:
        local_root = Path(temporary)
        torrent_info, seed_root, expected_sha1 = create_fixture(local_root)
        del torrent_info
        output = local_root / "rstorrent-leech" / PAYLOAD_NAME
        deadline = time.monotonic() + SCENARIO_TIMEOUT_SECONDS
        try:
            remote_run = create_remote_run(host)
            stage_remote_fixture(
                host,
                remote_run,
                local_root / "forced-utp.torrent",
                seed_root / PAYLOAD_NAME,
            )
            remote = RemoteSeedProcess.start(host, remote_run, expected_sha1)
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
                "seconds": round(time.monotonic() - started, 6),
            }
        except Exception as error:
            run_error = error
        finally:
            if role is not None:
                role.cleanup()
            if remote is not None:
                remote.cleanup()
            if remote_run is not None:
                try:
                    verify_remote_cleanup(
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
        raise run_error
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


def main() -> int:
    arguments = parse_arguments()
    print(json.dumps(run(arguments.host), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (WanFailure, subprocess.TimeoutExpired) as error:
        print(f"RSTorrent uTP WAN interoperability failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
