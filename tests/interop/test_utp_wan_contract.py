#!/usr/bin/env python3
"""Deterministic contracts for the mapped uTP WAN harness."""

from __future__ import annotations

import ipaddress
import json
import subprocess
import unittest
from pathlib import Path
from unittest.mock import patch

from utp_remote_seed import eligible_public_ipv4, parse_mapping_entries
from utp_rstorrent_wan import (
    WanFailure,
    aborted_remote_summary,
    audit_local_mapping,
    bounded_diagnostics,
    eligible_local_seed_endpoint,
    eligible_public_endpoint,
    redacted_rstorrent,
    validate_mapping_intent,
    validate_remote_leecher_complete,
    validate_remote_leecher_ready,
    verify_direct_route,
)


class UtpWanContractTests(unittest.TestCase):
    def test_mapping_inventory_parser_preserves_finite_udp_identity(self) -> None:
        entries = parse_mapping_entries(
            " 0 UDP 42000->192.168.1.108:42000 "
            "'libtorrent' '' 3599\n"
            " 1 TCP 43000->192.168.1.10:43000 'other' '' 0\n"
        )
        self.assertEqual(len(entries), 2)
        self.assertEqual(entries[0].protocol, "UDP")
        self.assertEqual(entries[0].external_port, 42000)
        self.assertEqual(entries[0].internal_address, "192.168.1.108")
        self.assertEqual(entries[0].internal_port, 42000)
        self.assertEqual(entries[0].lease_seconds, 3599)

    def test_public_endpoint_rejects_special_use_and_accepts_exact_mapping(self) -> None:
        for address in (
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "192.0.2.1",
            "192.168.1.1",
            "198.51.100.1",
            "203.0.113.1",
        ):
            self.assertFalse(eligible_public_ipv4(address), address)
        event = {
            "event": "ready",
            "role": "remote-seed",
            "pid": 123,
            "listen_port": 42000,
            "external_address": "8.8.8.8",
            "external_port": 42000,
            "libtorrent_version": "2.0.13.0",
            "mapping": {
                "protocol": "UDP",
                "transport": "UPnP",
                "lease_seconds": 3599,
            },
        }
        self.assertEqual(
            eligible_public_endpoint(event),
            ("8.8.8.8", 42000, 42000, 123),
        )
        event["external_address"] = "100.64.0.1"
        with self.assertRaises(WanFailure):
            eligible_public_endpoint(event)

    def test_route_proof_rejects_overlay_interface(self) -> None:
        ordinary = subprocess.CompletedProcess(
            ["route"],
            0,
            stdout="   route to: 8.8.8.8\n interface: en0\n",
            stderr="",
        )
        overlay = subprocess.CompletedProcess(
            ["route"],
            0,
            stdout="   route to: 8.8.8.8\n interface: utun4\n",
            stderr="",
        )
        with (
            patch(
                "utp_rstorrent_wan.ssh_control_address",
                return_value=ipaddress.IPv4Address("100.64.0.2"),
            ),
            patch("utp_rstorrent_wan.shutil.which", return_value="/sbin/route"),
            patch("utp_rstorrent_wan.subprocess.run", return_value=ordinary),
        ):
            self.assertEqual(verify_direct_route("pimom", "8.8.8.8"), "en0")
        with (
            patch(
                "utp_rstorrent_wan.ssh_control_address",
                return_value=ipaddress.IPv4Address("100.64.0.2"),
            ),
            patch("utp_rstorrent_wan.shutil.which", return_value="/sbin/route"),
            patch("utp_rstorrent_wan.subprocess.run", return_value=overlay),
        ):
            with self.assertRaises(WanFailure):
                verify_direct_route("pimom", "8.8.8.8")

    def test_diagnostics_redact_addresses_and_bound_lines(self) -> None:
        lines = [f"peer 104.20.30.40 line {index}" for index in range(60)]
        redacted = bounded_diagnostics(lines)
        self.assertEqual(len(redacted), 50)
        self.assertTrue(all("104.20.30.40" not in line for line in redacted))
        self.assertTrue(all("<ipv4>" in line for line in redacted))

    def test_abort_summary_is_bounded_to_transport_counts(self) -> None:
        summary = aborted_remote_summary(
            {
                "event": "aborted",
                "mapping_deleted": True,
                "libtorrent_stats": {
                    "utp.utp_packets_in": 7,
                    "utp.utp_packets_out": 2,
                    "peer.num_utp_peers": 1,
                },
            }
        )
        self.assertEqual(
            summary,
            "remote abort evidence: utp_in=7, utp_out=2, "
            "utp_peers=1, payload_sent=missing, mapping_deleted=True",
        )

    def test_local_seed_mapping_and_remote_leecher_contracts(self) -> None:
        intent = {
            "event": "mapping-intent",
            "role": "wan-seed",
            "local_port": 42000,
            "external_port": 42000,
            "protocol": "UDP",
        }
        self.assertEqual(validate_mapping_intent(intent, 42000), 42000)
        ready = {
            "event": "ready",
            "role": "wan-seed",
            "external_address": "8.8.8.8",
            "external_port": 42000,
            "mapping": {
                "protocol": "UDP",
                "transport": "UPnP",
                "lease_seconds": 3600,
            },
        }
        self.assertEqual(
            eligible_local_seed_endpoint(ready, 42000), ("8.8.8.8", 42000)
        )
        validate_remote_leecher_ready(
            {
                "event": "ready",
                "role": "remote-leecher",
                "pid": 123,
                "listen_port": 43000,
                "libtorrent_version": "2.0.13.0",
                "route_class": "ordinary-internet",
            },
            123,
        )
        validate_remote_leecher_complete(
            {
                "event": "complete",
                "role": "remote-leecher",
                "peer_high_water": 1,
                "payload": {
                    "bytes": 2 * 1024 * 1024 + 731,
                    "pieces": 33,
                    "sha1": "a" * 40,
                },
                "libtorrent_stats": {
                    "peer.num_tcp_peers": 0,
                    "utp.utp_packets_in": 1,
                    "utp.utp_packets_out": 1,
                    "net.recv_payload_bytes": 2 * 1024 * 1024 + 731,
                },
                "diagnostics": [],
            },
            "a" * 40,
        )

    def test_mapping_audit_requires_verified_absence(self) -> None:
        complete = subprocess.CompletedProcess(
            ["rstorrent-utp-interop"],
            0,
            stdout=json.dumps(
                {
                    "event": "mapping-audit",
                    "role": "wan-mapping-audit",
                    "owned_mapping_found": True,
                    "owned_mapping_deleted": True,
                    "foreign_mapping_preserved": False,
                    "owned_mapping_absent": True,
                }
            ),
            stderr="",
        )
        with patch("utp_rstorrent_wan.subprocess.run", return_value=complete):
            event = audit_local_mapping(Path("role"), 42000, 42000)
        self.assertTrue(event["owned_mapping_deleted"])

        foreign = subprocess.CompletedProcess(
            ["rstorrent-utp-interop"],
            0,
            stdout=json.dumps(
                {
                    "event": "mapping-audit",
                    "role": "wan-mapping-audit",
                    "owned_mapping_found": False,
                    "owned_mapping_deleted": False,
                    "foreign_mapping_preserved": True,
                    "owned_mapping_absent": True,
                }
            ),
            stderr="",
        )
        with patch("utp_rstorrent_wan.subprocess.run", return_value=foreign):
            with self.assertRaises(WanFailure):
                audit_local_mapping(Path("role"), 42000, 42000)

    def test_rstorrent_seed_redaction_removes_peer_endpoints(self) -> None:
        event = {
            "event": "complete",
            "role": "wan-seed",
            "peer_evidence": {"endpoints": ["104.20.30.40:45000"]},
        }
        redacted = redacted_rstorrent(event)
        self.assertEqual(
            redacted["peer_evidence"]["endpoints"],
            ["<public-ip>:<transient-port>"],
        )


if __name__ == "__main__":
    unittest.main()
