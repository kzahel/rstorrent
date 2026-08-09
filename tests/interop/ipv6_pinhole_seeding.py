#!/usr/bin/env python3
"""Opt-in physical IPv6 firewall-pinhole and off-LAN seeding gate.

Set RSTORRENT_OFF_LAN_SSH_TARGET to an operator-controlled SSH destination.
The destination value, IPv6 address, and gateway identity are consumed only in
memory and are never printed or persisted by this gate. SSH, Python, and IPv6
socket readiness are checked before fixture creation, build, listener startup,
or gateway mutation.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

from upnp_external_seeding import (
    GateFailure,
    PIECE_LENGTH,
    PROCESS_TIMEOUT,
    build_seed,
    create_fixture,
    finish_remote,
    read_json_line,
    require_remote_ready,
    rows,
    start_remote,
    stop_seed,
    terminate,
)


def command_seed(
    process: subprocess.Popen[str], command: str, timeout: float = PROCESS_TIMEOUT
) -> dict[str, Any]:
    if process.stdin is None:
        raise GateFailure("seed stdin is unavailable")
    process.stdin.write(command + "\n")
    process.stdin.flush()
    return read_json_line(process, timeout)


def start_staged_seed(
    binary: Path, fixture: dict[str, object]
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
            "--staged-ipv6-pinhole",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        ready = read_json_line(process, PROCESS_TIMEOUT)
        if ready.get("event") != "pre_pinhole" or ready.get("registrations") != 1:
            raise GateFailure("seed did not reach pre-pinhole IPv6 readiness")
        if ready.get("ipv6_pinhole") != {"type": "disabled"}:
            raise GateFailure("pre-pinhole runtime did not report disabled control")
        endpoint = ready.get("ipv6_listener")
        if not isinstance(endpoint, str) or not endpoint.startswith("["):
            raise GateFailure("seed did not expose its in-memory IPv6 listener endpoint")
        return process, ready
    except BaseException as error:
        terminate(process)
        stderr = process.stderr.read() if process.stderr is not None else ""
        detail = " ".join(stderr.strip().split())[:240]
        raise GateFailure(
            "seed failed before pre-pinhole readiness"
            + (f": {detail}" if detail else "")
        ) from error


def split_ipv6_endpoint(endpoint: str) -> tuple[str, int]:
    closing = endpoint.rfind("]:")
    if not endpoint.startswith("[") or closing < 2:
        raise GateFailure("IPv6 listener endpoint is malformed")
    host = endpoint[1:closing]
    try:
        port = int(endpoint[closing + 2 :])
    except ValueError as error:
        raise GateFailure("IPv6 listener port is malformed") from error
    if not host or not 1_024 <= port <= 65_535:
        raise GateFailure("IPv6 listener endpoint is outside its bound")
    return host, port


def verifier_config(
    host: str, port: int, fixture: dict[str, object]
) -> dict[str, object]:
    return {
        "host": host,
        "port": port,
        "info_hash": fixture["info_hash"],
        "total_length": fixture["total_length"],
        "piece_length": PIECE_LENGTH,
        "piece_hashes": fixture["piece_hashes"],
        "payload_sha256": fixture["payload_sha256"],
        "hold_seconds": 2,
    }


def terminal_pinhole_summary(status: object) -> str:
    if not isinstance(status, dict):
        return "malformed"
    kind = status.get("type")
    if not isinstance(kind, str) or not re.fullmatch(r"[a-z_]{1,32}", kind):
        return "malformed"
    parts = [kind]
    stage = status.get("stage")
    if isinstance(stage, str) and re.fullmatch(r"[a-z_]{1,32}", stage):
        parts.append(f"stage={stage}")
    detail = status.get("detail")
    if isinstance(detail, str):
        fault = re.search(r"(?:fault|error)(?: code)?[^0-9]{0,16}([67][0-9]{2})\b", detail, re.I)
        if fault is not None:
            parts.append(f"fault={fault.group(1)}")
    remaining = status.get("remaining_lease_seconds")
    if isinstance(remaining, int) and not isinstance(remaining, bool):
        parts.append(f"uncertain_seconds={min(max(remaining, 0), 86_400)}")
    return " ".join(parts)


def run(repository: Path) -> dict[str, object]:
    target = os.environ.get("RSTORRENT_OFF_LAN_SSH_TARGET")
    if not target:
        return {"status": "skipped", "reason": "off-LAN SSH target is not configured"}
    require_remote_ready(target)
    remote_source = (repository / "tests/interop/off_lan_peer_wire.py").read_text()
    root = Path(tempfile.mkdtemp(prefix="rstorrent-ipv6-pinhole-"))
    seed: subprocess.Popen[str] | None = None
    remote_processes: list[subprocess.Popen[str]] = []
    try:
        fixture = create_fixture(root)
        seed, pre_pinhole = start_staged_seed(build_seed(repository), fixture)
        endpoint = pre_pinhole["ipv6_listener"]
        if not isinstance(endpoint, str):
            raise GateFailure("pre-pinhole endpoint changed type")
        host, port = split_ipv6_endpoint(endpoint)

        negative = start_remote(
            target,
            remote_source,
            {"host": host, "port": port, "expect_connect_failure": True},
        )
        remote_processes.append(negative)
        finish_remote(negative, "unreachable")

        pinholed = command_seed(seed, "enable-pinhole")
        status = pinholed.get("ipv6_pinhole")
        if pinholed.get("event") == "pinhole_terminal":
            raise GateFailure(
                "coordinator reached terminal pinhole state: "
                + terminal_pinhole_summary(status)
            )
        if (
            pinholed.get("event") != "pinholed"
            or pinholed.get("ipv6_listener") != endpoint
            or not isinstance(status, dict)
            or status.get("type") != "pinholed"
            or status.get("internal_address") != host
            or status.get("internal_port") != port
            or status.get("lease_seconds") not in (3_600, 3_601)
        ):
            raise GateFailure("coordinator did not publish the exact finite IPv6 pinhole")

        remote = start_remote(
            target,
            remote_source,
            verifier_config(host, port, fixture),
        )
        remote_processes.append(remote)
        observed_peer = False
        observed_swarm = False
        deadline = time.monotonic() + PROCESS_TIMEOUT
        while remote.poll() is None and time.monotonic() < deadline:
            observation = command_seed(seed, "snapshot", 5)
            for peer in rows(observation, "peers"):
                flags = peer.get("peer_flags")
                if (
                    peer.get("direction") == "incoming"
                    and peer.get("transport") == "tcp"
                    and isinstance(flags, list)
                    and "incoming" in flags
                    and peer.get("remote_interested") is True
                    and peer.get("local_choking") is False
                    and int(peer.get("payload_uploaded_bytes") or "0") > 0
                ):
                    observed_peer = True
            for peer in rows(observation, "swarm"):
                if "incoming" in (peer.get("sources") or []):
                    observed_swarm = True
            time.sleep(0.03)
        if remote.poll() is None:
            raise GateFailure("off-LAN positive verifier exceeded its transfer deadline")
        result = finish_remote(remote, "verified")
        if not observed_peer or not observed_swarm:
            raise GateFailure("ordinary Peers/Swarm views missed the incoming IPv6 peer")
        if (
            result.get("bytes") != fixture["total_length"]
            or result.get("sha256") != fixture["payload_sha256"]
        ):
            raise GateFailure("off-LAN IPv6 verifier did not prove the exact payload")

        packets = command_seed(seed, "pinhole-packets")
        if (
            packets.get("event") != "pinhole_packets"
            or packets.get("type") != "packets"
            or not isinstance(packets.get("packets"), int)
            or int(packets["packets"]) <= 0
        ):
            raise GateFailure("post-traffic pinhole packet count was not positive")

        disabled = command_seed(seed, "disable-pinhole")
        if (
            disabled.get("event") != "pinhole_disabled"
            or disabled.get("ipv6_listener") != endpoint
            or disabled.get("ipv6_pinhole") != {"type": "disabled"}
        ):
            raise GateFailure("coordinator did not retain the listener after pinhole cleanup")
        deleted = command_seed(seed, "deleted-pinhole-packets")
        if (
            deleted.get("event") != "deleted_pinhole_packets"
            or deleted.get("type") != "fault"
            or deleted.get("code") != 704
        ):
            raise GateFailure("deleted pinhole did not produce authoritative fault 704")

        unreachable = start_remote(
            target,
            remote_source,
            {"host": host, "port": port, "expect_connect_failure": True},
        )
        remote_processes.append(unreachable)
        finish_remote(unreachable, "unreachable")

        stopped = stop_seed(seed)
        seed = None
        if not (
            stopped.get("payload_bytes_sent") == fixture["total_length"]
            and int(stopped.get("queued_requests_high_water") or 0) > 0
            and int(stopped.get("read_high_water") or 0) > 0
            and stopped.get("mapping_tasks_after_shutdown") == 0
            and stopped.get("mappings_after_shutdown") == 0
            and stopped.get("pinholes_after_shutdown") == 0
        ):
            raise GateFailure("seed terminal accounting or owner counts are not exact")
        return {
            "status": "passed",
            "mechanism": "upnp_igd_v2_ipv6_firewall_control_v1",
            "negative_control": True,
            "payload_bytes": fixture["total_length"],
            "pieces": len(fixture["piece_hashes"]),
            "pinhole_packets_positive": True,
            "delete_fault": 704,
            "post_delete_unreachable": True,
            "terminal_tasks": 0,
            "terminal_mappings": 0,
            "terminal_pinholes": 0,
        }
    finally:
        for remote_process in remote_processes:
            terminate(remote_process)
        if seed is not None:
            try:
                if seed.poll() is None:
                    stop_seed(seed)
            except BaseException:
                terminate(seed)
        shutil.rmtree(root, ignore_errors=True)


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    try:
        print(json.dumps(run(repository), separators=(",", ":")))
        return 0
    except GateFailure as error:
        print(f"IPv6 pinhole seeding gate failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
