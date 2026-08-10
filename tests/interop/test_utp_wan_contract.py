#!/usr/bin/env python3
"""Deterministic contracts for the mapped uTP WAN harness."""

from __future__ import annotations

import ipaddress
import subprocess
import unittest
from unittest.mock import patch

from utp_remote_seed import eligible_public_ipv4, parse_mapping_entries
from utp_rstorrent_wan import (
    WanFailure,
    aborted_remote_summary,
    bounded_diagnostics,
    eligible_public_endpoint,
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


if __name__ == "__main__":
    unittest.main()
