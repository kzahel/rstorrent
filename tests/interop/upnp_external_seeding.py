#!/usr/bin/env python3
"""Opt-in physical UPnP and independent off-LAN seeding gate.

Set RSTORRENT_OFF_LAN_SSH_TARGET to an operator-controlled SSH destination.
The destination value and observed network identities are never printed or
persisted. SSH carries only verifier control; payload traffic dials the mapped
public endpoint directly.
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import selectors
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any


PIECE_LENGTH = 16 * 1024
PAYLOAD_LENGTH = 4 * 1024 * 1024 + 731
PROCESS_TIMEOUT = 60
SSDP_ENDPOINT = ("239.255.255.250", 1900)
SERVICE_TYPE = "urn:schemas-upnp-org:service:WANIPConnection:2"
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


class GateFailure(RuntimeError):
    pass


def bencode(value: object) -> bytes:
    if isinstance(value, bytes):
        return str(len(value)).encode() + b":" + value
    if isinstance(value, int) and not isinstance(value, bool):
        return b"i" + str(value).encode() + b"e"
    if isinstance(value, list):
        return b"l" + b"".join(bencode(item) for item in value) + b"e"
    if isinstance(value, dict):
        items = sorted(value.items())
        return b"d" + b"".join(bencode(key) + bencode(item) for key, item in items) + b"e"
    raise TypeError(f"unsupported bencode value {type(value).__name__}")


def deterministic_payload(length: int) -> bytes:
    return bytes((index * 37 + 11) % 251 for index in range(length))


def create_fixture(root: Path) -> dict[str, object]:
    storage = root / "payload"
    profile = root / "profile"
    storage.mkdir()
    payload = deterministic_payload(PAYLOAD_LENGTH)
    payload_path = storage / "external-seed.bin"
    payload_path.write_bytes(payload)
    hashes = [
        hashlib.sha1(payload[offset : offset + PIECE_LENGTH]).digest()
        for offset in range(0, len(payload), PIECE_LENGTH)
    ]
    info = {
        b"length": len(payload),
        b"name": b"external-seed.bin",
        b"piece length": PIECE_LENGTH,
        b"pieces": b"".join(hashes),
    }
    raw_info = bencode(info)
    torrent = root / "external-seed.torrent"
    torrent.write_bytes(bencode({b"info": info}))
    return {
        "storage": storage,
        "profile": profile,
        "torrent": torrent,
        "info_hash": hashlib.sha1(raw_info).hexdigest(),
        "piece_hashes": [piece.hex() for piece in hashes],
        "payload_sha256": hashlib.sha256(payload).hexdigest(),
        "total_length": len(payload),
    }


def build_seed(repository: Path) -> Path:
    completed = subprocess.run(
        [
            "cargo",
            "build",
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
        raise GateFailure("the UPnP seed diagnostic did not build")
    binary = repository / "target/debug/rstorrent-incoming-seed"
    if not binary.is_file():
        raise GateFailure("the UPnP seed diagnostic binary is missing")
    return binary


def read_json_line(process: subprocess.Popen[str], timeout_seconds: float) -> dict[str, Any]:
    if process.stdout is None:
        raise GateFailure("seed stdout is unavailable")
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    try:
        if not selector.select(timeout_seconds):
            raise GateFailure("seed observation timed out")
        line = process.stdout.readline()
    finally:
        selector.close()
    if not line:
        raise GateFailure("seed exited before the expected observation")
    try:
        value = json.loads(line)
    except json.JSONDecodeError as error:
        raise GateFailure("seed emitted malformed JSON") from error
    if not isinstance(value, dict):
        raise GateFailure("seed observation is not an object")
    return value


def start_seed(binary: Path, fixture: dict[str, object]) -> tuple[subprocess.Popen[str], dict[str, Any]]:
    process = subprocess.Popen(
        [
            str(binary),
            "--profile-root",
            str(fixture["profile"]),
            "--storage-root",
            str(fixture["storage"]),
            "--metainfo",
            str(fixture["torrent"]),
            "--upnp",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        ready = read_json_line(process, PROCESS_TIMEOUT)
        if ready.get("event") != "ready" or ready.get("registrations") != 1:
            raise GateFailure("seed did not reach mapped registered readiness")
        mapping = ready.get("mapping")
        if not isinstance(mapping, dict) or mapping.get("type") != "mapped":
            raise GateFailure("seed did not publish a verified mapped endpoint")
        return process, ready
    except BaseException as error:
        terminate(process)
        stderr = process.stderr.read() if process.stderr is not None else ""
        detail = " ".join(stderr.strip().split())[:240]
        raise GateFailure(
            "seed failed before mapped readiness" + (f": {detail}" if detail else "")
        ) from error


def command_seed(process: subprocess.Popen[str], command: str) -> dict[str, Any]:
    if process.stdin is None:
        raise GateFailure("seed stdin is unavailable")
    process.stdin.write(command + "\n")
    process.stdin.flush()
    return read_json_line(process, 5)


def stop_seed(process: subprocess.Popen[str]) -> dict[str, Any]:
    stopped = command_seed(process, "stop")
    try:
        returncode = process.wait(timeout=15)
    except subprocess.TimeoutExpired as error:
        process.kill()
        process.wait(timeout=5)
        raise GateFailure("seed did not join shutdown") from error
    if returncode != 0 or stopped.get("event") != "stopped":
        raise GateFailure("seed shutdown failed")
    return stopped


def terminate(process: subprocess.Popen[str]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=3)


def local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def discover_control(local_address: str) -> tuple[str, str]:
    request = (
        "M-SEARCH * HTTP/1.1\r\n"
        "HOST: 239.255.255.250:1900\r\n"
        'MAN: "ssdp:discover"\r\n'
        "MX: 2\r\n"
        "ST: upnp:rootdevice\r\n\r\n"
    ).encode()
    candidates: list[tuple[str, str]] = []
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as discovery:
        discovery.bind((local_address, 0))
        discovery.settimeout(1)
        for _ in range(3):
            discovery.sendto(request, SSDP_ENDPOINT)
            deadline = time.monotonic() + 1
            while time.monotonic() < deadline:
                try:
                    payload, source = discovery.recvfrom(8 * 1024 + 1)
                except TimeoutError:
                    break
                if len(payload) > 8 * 1024:
                    continue
                headers: dict[str, str] = {}
                for line in payload.decode(errors="strict").split("\r\n")[1:65]:
                    if not line:
                        break
                    name, separator, value = line.partition(":")
                    if separator:
                        headers[name.lower()] = value.strip()
                location = headers.get("location")
                if location is not None:
                    parsed = urllib.parse.urlsplit(location)
                    if parsed.scheme == "http" and parsed.hostname == source[0]:
                        candidates.append((location, source[0]))
            if candidates:
                break
    for location, source in candidates[:8]:
        try:
            with urllib.request.urlopen(location, timeout=5) as response:
                document = ET.fromstring(response.read(256 * 1024 + 1))
        except (OSError, ET.ParseError, urllib.error.URLError):
            continue
        base = location
        for element in document.iter():
            if local_name(element.tag) == "URLBase" and element.text:
                base = element.text.strip()
                break
        for service in document.iter():
            if local_name(service.tag) != "service":
                continue
            values = {
                local_name(child.tag): (child.text or "").strip()
                for child in service
            }
            if values.get("serviceType") != SERVICE_TYPE:
                continue
            control = urllib.parse.urljoin(base, values.get("controlURL", ""))
            parsed = urllib.parse.urlsplit(control)
            if parsed.scheme == "http" and parsed.hostname == source:
                return control, SERVICE_TYPE
    raise GateFailure("independent query could not select the mapped IGD v2 service")


def query_mapping(
    control: str,
    service: str,
    port: int,
    protocol: str = "TCP",
) -> dict[str, str] | None:
    if protocol not in {"TCP", "UDP"}:
        raise GateFailure("independent mapping query protocol is invalid")
    action = "GetSpecificPortMappingEntry"
    body = (
        '<?xml version="1.0"?>'
        '<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">'
        f'<s:Body><u:{action} xmlns:u="{service}">'
        f"<NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort>"
        f"<NewProtocol>{protocol}</NewProtocol></u:{action}></s:Body></s:Envelope>"
    ).encode()
    request = urllib.request.Request(
        control,
        data=body,
        headers={
            "Content-Type": 'text/xml; charset="utf-8"',
            "SOAPAction": f'"{service}#{action}"',
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            payload = response.read(256 * 1024 + 1)
    except urllib.error.HTTPError as error:
        payload = error.read(256 * 1024 + 1)
    if len(payload) > 256 * 1024:
        raise GateFailure("independent mapping query exceeded its body bound")
    try:
        document = ET.fromstring(payload)
    except ET.ParseError as error:
        raise GateFailure("independent mapping query returned malformed XML") from error
    values = {
        local_name(element.tag): (element.text or "").strip()
        for element in document.iter()
    }
    if values.get("errorCode") == "714":
        return None
    required = (
        "NewInternalClient",
        "NewInternalPort",
        "NewEnabled",
        "NewPortMappingDescription",
        "NewLeaseDuration",
    )
    if not all(field in values for field in required):
        raise GateFailure("independent mapping query omitted an authoritative field")
    return values


def delete_mapping(
    control: str,
    service: str,
    port: int,
    protocol: str,
) -> None:
    if protocol not in {"TCP", "UDP"}:
        raise GateFailure("independent mapping delete protocol is invalid")
    action = "DeletePortMapping"
    body = (
        '<?xml version="1.0"?>'
        '<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">'
        f'<s:Body><u:{action} xmlns:u="{service}">'
        f"<NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort>"
        f"<NewProtocol>{protocol}</NewProtocol></u:{action}></s:Body></s:Envelope>"
    ).encode()
    request = urllib.request.Request(
        control,
        data=body,
        headers={
            "Content-Type": 'text/xml; charset="utf-8"',
            "SOAPAction": f'"{service}#{action}"',
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            payload = response.read(256 * 1024 + 1)
    except urllib.error.HTTPError as error:
        payload = error.read(256 * 1024 + 1)
    if len(payload) > 256 * 1024:
        raise GateFailure("independent mapping delete exceeded its body bound")
    try:
        document = ET.fromstring(payload)
    except ET.ParseError as error:
        raise GateFailure("independent mapping delete returned malformed XML") from error
    values = {
        local_name(element.tag): (element.text or "").strip()
        for element in document.iter()
    }
    error_code = values.get("errorCode")
    if error_code not in {None, "714"}:
        raise GateFailure("independent mapping delete returned a gateway fault")


def list_mappings(
    control: str,
    service: str,
    maximum_entries: int = 256,
) -> list[dict[str, str]]:
    if not 1 <= maximum_entries <= 256:
        raise GateFailure("independent mapping inventory bound is invalid")
    entries: list[dict[str, str]] = []
    action = "GetGenericPortMappingEntry"
    for index in range(maximum_entries):
        body = (
            '<?xml version="1.0"?>'
            '<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">'
            f'<s:Body><u:{action} xmlns:u="{service}">'
            f"<NewPortMappingIndex>{index}</NewPortMappingIndex>"
            f"</u:{action}></s:Body></s:Envelope>"
        ).encode()
        request = urllib.request.Request(
            control,
            data=body,
            headers={
                "Content-Type": 'text/xml; charset="utf-8"',
                "SOAPAction": f'"{service}#{action}"',
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=5) as response:
                payload = response.read(256 * 1024 + 1)
        except urllib.error.HTTPError as error:
            payload = error.read(256 * 1024 + 1)
        if len(payload) > 256 * 1024:
            raise GateFailure("independent mapping inventory exceeded its body bound")
        try:
            document = ET.fromstring(payload)
        except ET.ParseError as error:
            raise GateFailure("independent mapping inventory returned malformed XML") from error
        values = {
            local_name(element.tag): (element.text or "").strip()
            for element in document.iter()
        }
        error_code = values.get("errorCode")
        if error_code == "713":
            return entries
        if error_code is not None:
            raise GateFailure("independent mapping inventory returned a gateway fault")
        required = (
            "NewExternalPort",
            "NewProtocol",
            "NewInternalClient",
            "NewInternalPort",
            "NewEnabled",
            "NewPortMappingDescription",
            "NewLeaseDuration",
        )
        if not all(field in values for field in required):
            raise GateFailure("independent mapping inventory omitted an authoritative field")
        entries.append(values)
    raise GateFailure("independent mapping inventory exceeded its entry bound")


def remote_command(source: str) -> str:
    encoded = base64.b64encode(source.encode()).decode()
    return f"'import base64;exec(base64.b64decode(\"{encoded}\"))'"


def require_remote_ready(target: str) -> None:
    source = (
        "import socket,sys;"
        "sys.stdout.write('ready' if socket.has_ipv6 else 'ipv6-unavailable')"
    )
    try:
        completed = subprocess.run(
            ["ssh", *SSH_OPTIONS, target, "python3", "-c", remote_command(source)],
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise GateFailure("off-LAN verifier SSH preflight timed out") from error
    if completed.returncode != 0:
        raise GateFailure("off-LAN verifier SSH/Python preflight failed")
    if completed.stdout != "ready":
        raise GateFailure("off-LAN verifier does not provide IPv6 sockets")


def start_remote(
    target: str,
    source: str,
    config: dict[str, object],
) -> subprocess.Popen[str]:
    process = subprocess.Popen(
        ["ssh", *SSH_OPTIONS, target, "python3", "-c", remote_command(source)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if process.stdin is None:
        raise GateFailure("off-LAN verifier stdin is unavailable")
    process.stdin.write(json.dumps(config, separators=(",", ":")))
    process.stdin.close()
    process.stdin = None
    return process


def finish_remote(process: subprocess.Popen[str], expected_status: str) -> dict[str, Any]:
    try:
        returncode = process.wait(timeout=PROCESS_TIMEOUT)
    except subprocess.TimeoutExpired as error:
        process.kill()
        process.wait(timeout=5)
        raise GateFailure("off-LAN verifier timed out") from error
    stdout = process.stdout.read() if process.stdout is not None else ""
    if returncode != 0:
        stderr = process.stderr.read() if process.stderr is not None else ""
        detail = " ".join(stderr.strip().split())[:240]
        if not detail.startswith("off-LAN peer verification failed:"):
            detail = ""
        raise GateFailure(
            "off-LAN verifier failed" + (f": {detail}" if detail else "")
        )
    try:
        result = json.loads(stdout)
    except json.JSONDecodeError as error:
        raise GateFailure("off-LAN verifier returned malformed JSON") from error
    if not isinstance(result, dict) or result.get("status") != expected_status:
        raise GateFailure("off-LAN verifier returned the wrong terminal state")
    return result


def rows(snapshot: dict[str, Any], key: str) -> list[dict[str, Any]]:
    value = snapshot.get(key)
    if not isinstance(value, dict) or value.get("type") != key:
        raise GateFailure(f"seed {key} snapshot is malformed")
    result = value.get("peers")
    if not isinstance(result, list) or not all(isinstance(row, dict) for row in result):
        raise GateFailure(f"seed {key} rows are malformed")
    return result


def run(repository: Path) -> dict[str, object]:
    target = os.environ.get("RSTORRENT_OFF_LAN_SSH_TARGET")
    if not target:
        return {"status": "skipped", "reason": "off-LAN SSH target is not configured"}
    remote_source = (repository / "tests/interop/off_lan_peer_wire.py").read_text()
    root = Path(tempfile.mkdtemp(prefix="rstorrent-upnp-external-"))
    seed: subprocess.Popen[str] | None = None
    remote_processes: list[subprocess.Popen[str]] = []
    try:
        fixture = create_fixture(root)
        seed, ready = start_seed(build_seed(repository), fixture)
        process = seed
        mapping = ready["mapping"]
        if not isinstance(mapping, dict):
            raise GateFailure("mapped readiness is malformed")
        local_address = mapping.get("local_address")
        local_port = mapping.get("local_port")
        external_address = mapping.get("external_address")
        external_port = mapping.get("external_port")
        lease_seconds = mapping.get("lease_seconds")
        if not (
            isinstance(local_address, str)
            and isinstance(local_port, int)
            and isinstance(external_address, str)
            and isinstance(external_port, int)
            and isinstance(lease_seconds, int)
            and lease_seconds > 0
        ):
            raise GateFailure("mapped readiness fields are invalid")
        control, service = discover_control(local_address)
        installed = query_mapping(control, service, external_port)
        if installed is None or not (
            installed["NewInternalClient"] == local_address
            and int(installed["NewInternalPort"]) == local_port
            and installed["NewEnabled"] == "1"
            and installed["NewPortMappingDescription"] == "RSTorrent"
            and int(installed["NewLeaseDuration"]) > 0
        ):
            raise GateFailure("independent query did not verify the exact finite mapping")

        remote = start_remote(
            target,
            remote_source,
            {
                "host": external_address,
                "port": external_port,
                "info_hash": fixture["info_hash"],
                "total_length": fixture["total_length"],
                "piece_length": PIECE_LENGTH,
                "piece_hashes": fixture["piece_hashes"],
                "payload_sha256": fixture["payload_sha256"],
                "hold_seconds": 2,
            },
        )
        remote_processes.append(remote)
        observed_peer = False
        observed_swarm = False
        deadline = time.monotonic() + PROCESS_TIMEOUT
        while remote.poll() is None and time.monotonic() < deadline:
            observation = command_seed(process, "snapshot")
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
        result = finish_remote(remote, "verified")
        if not observed_peer or not observed_swarm:
            raise GateFailure("ordinary Peers/Swarm views missed the external incoming peer")
        if (
            result.get("bytes") != fixture["total_length"]
            or result.get("sha256") != fixture["payload_sha256"]
        ):
            raise GateFailure("off-LAN verifier did not prove the exact payload")

        deadline = time.monotonic() + 10
        retained_swarm = False
        while time.monotonic() < deadline:
            observation = command_seed(process, "snapshot")
            if not rows(observation, "peers") and any(
                "incoming" in (peer.get("sources") or [])
                and peer.get("state") == "not_connectable"
                and peer.get("connectable") is False
                for peer in rows(observation, "swarm")
            ):
                retained_swarm = True
                break
            time.sleep(0.03)
        if not retained_swarm:
            raise GateFailure("external peer did not settle into retained Swarm history")

        stopped = stop_seed(process)
        seed = None
        if query_mapping(control, service, external_port) is not None:
            raise GateFailure("independent query found the mapping after joined shutdown")
        unreachable = start_remote(
            target,
            remote_source,
            {
                "host": external_address,
                "port": external_port,
                "expect_connect_failure": True,
            },
        )
        remote_processes.append(unreachable)
        finish_remote(unreachable, "unreachable")
        if not (
            stopped.get("payload_bytes_sent") == fixture["total_length"]
            and int(stopped.get("queued_requests_high_water") or 0) > 0
            and int(stopped.get("read_high_water") or 0) > 0
            and stopped.get("mapping_tasks_after_shutdown") == 0
            and stopped.get("mappings_after_shutdown") == 0
        ):
            raise GateFailure("seed terminal accounting or owner counts are not exact")
        return {
            "status": "passed",
            "mechanism": "upnp_igd_v2",
            "payload_bytes": fixture["total_length"],
            "pieces": len(fixture["piece_hashes"]),
            "mapping_deleted": True,
            "post_delete_unreachable": True,
            "terminal_tasks": 0,
            "terminal_mappings": 0,
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
        print(f"UPnP external seeding gate failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
