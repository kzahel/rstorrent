#!/usr/bin/env python3
"""Own one exact finite UPnP mapping for a Tactical 142 seed role."""

from __future__ import annotations

import argparse
import ipaddress
import json
import socket
import sys
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any

from upnp_external_seeding import (
    GateFailure,
    delete_mapping,
    discover_control,
    local_name,
    query_mapping,
)


MAPPING_DESCRIPTION = "RSTorrent-matrix"
LEASE_SECONDS = 3_600
MAX_SOAP_BYTES = 256 * 1024


class MappingError(RuntimeError):
    pass


def local_route_address() -> str:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.connect(("192.0.2.1", 9))
        value = str(probe.getsockname()[0])
    address = ipaddress.ip_address(value)
    if not isinstance(address, ipaddress.IPv4Address) or not address.is_private:
        raise MappingError("ordinary route has no private IPv4 source")
    return value


def _soap_values(
    control: str,
    service: str,
    action: str,
    arguments: tuple[tuple[str, str], ...] = (),
) -> dict[str, str]:
    fields = "".join(f"<{name}>{value}</{name}>" for name, value in arguments)
    body = (
        '<?xml version="1.0"?>'
        '<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">'
        f'<s:Body><u:{action} xmlns:u="{service}">{fields}'
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
            payload = response.read(MAX_SOAP_BYTES + 1)
    except urllib.error.HTTPError as error:
        payload = error.read(MAX_SOAP_BYTES + 1)
    if len(payload) > MAX_SOAP_BYTES:
        raise MappingError("UPnP action exceeded its response bound")
    try:
        document = ET.fromstring(payload)
    except ET.ParseError as error:
        raise MappingError("UPnP action returned malformed XML") from error
    values = {
        local_name(element.tag): (element.text or "").strip()
        for element in document.iter()
    }
    if error_code := values.get("errorCode"):
        raise MappingError(f"UPnP {action} returned fault {error_code}")
    return values


def external_address(control: str, service: str) -> str:
    values = _soap_values(control, service, "GetExternalIPAddress")
    value = values.get("NewExternalIPAddress", "")
    try:
        address = ipaddress.ip_address(value)
    except ValueError as error:
        raise MappingError("UPnP returned an invalid external address") from error
    if not isinstance(address, ipaddress.IPv4Address) or not address.is_global:
        raise MappingError("UPnP external address is not public IPv4")
    return value


def validate_installed(
    installed: dict[str, str] | None,
    *,
    local_address: str,
    port: int,
) -> int:
    if installed is None or not (
        installed.get("NewInternalClient") == local_address
        and installed.get("NewInternalPort") == str(port)
        and installed.get("NewEnabled") == "1"
        and installed.get("NewPortMappingDescription") == MAPPING_DESCRIPTION
    ):
        raise MappingError("independent query rejected the exact matrix mapping")
    try:
        lease = int(installed.get("NewLeaseDuration", ""))
    except ValueError as error:
        raise MappingError("matrix mapping lease is malformed") from error
    if not 0 < lease <= LEASE_SECONDS:
        raise MappingError("matrix mapping lease is outside its bound")
    return lease


def add_mapping(port: int, protocol: str) -> dict[str, Any]:
    if not 1 <= port <= 65_535 or protocol not in {"TCP", "UDP"}:
        raise MappingError("matrix mapping endpoint is invalid")
    local_address = local_route_address()
    control, service = discover_control(local_address)
    if query_mapping(control, service, port, protocol) is not None:
        raise MappingError("matrix external port is already mapped")
    _soap_values(
        control,
        service,
        "AddPortMapping",
        (
            ("NewRemoteHost", ""),
            ("NewExternalPort", str(port)),
            ("NewProtocol", protocol),
            ("NewInternalPort", str(port)),
            ("NewInternalClient", local_address),
            ("NewEnabled", "1"),
            ("NewPortMappingDescription", MAPPING_DESCRIPTION),
            ("NewLeaseDuration", str(LEASE_SECONDS)),
        ),
    )
    try:
        lease = validate_installed(
            query_mapping(control, service, port, protocol),
            local_address=local_address,
            port=port,
        )
        public_address = external_address(control, service)
    except BaseException:
        delete_mapping(control, service, port, protocol)
        raise
    return {
        "event": "mapped",
        "protocol": protocol,
        "local_address": local_address,
        "local_port": port,
        "external_address": public_address,
        "external_port": port,
        "lease_seconds": lease,
        "description": MAPPING_DESCRIPTION,
    }


def remove_mapping(port: int, protocol: str) -> dict[str, Any]:
    if not 1 <= port <= 65_535 or protocol not in {"TCP", "UDP"}:
        raise MappingError("matrix mapping endpoint is invalid")
    local_address = local_route_address()
    control, service = discover_control(local_address)
    installed = query_mapping(control, service, port, protocol)
    if installed is not None:
        if installed.get("NewPortMappingDescription") != MAPPING_DESCRIPTION:
            return {
                "event": "foreign-mapping-preserved",
                "protocol": protocol,
                "external_port": port,
            }
        validate_installed(installed, local_address=local_address, port=port)
        delete_mapping(control, service, port, protocol)
    if query_mapping(control, service, port, protocol) is not None:
        raise MappingError("matrix mapping survived exact deletion")
    return {"event": "mapping-absent", "protocol": protocol, "external_port": port}


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("map", "remove"))
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--protocol", choices=("TCP", "UDP"), required=True)
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    try:
        result = (
            add_mapping(arguments.port, arguments.protocol)
            if arguments.action == "map"
            else remove_mapping(arguments.port, arguments.protocol)
        )
        print(json.dumps(result, sort_keys=True))
        return 0
    except (GateFailure, MappingError, OSError, urllib.error.URLError) as error:
        print(f"WAN mapping failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
