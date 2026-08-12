#!/usr/bin/env python3

from __future__ import annotations

import unittest

from wan_transport_libtorrent_role import (
    LibtorrentRoleError,
    milestone_thresholds,
    local_route_address,
    parse_arguments,
    transport_settings,
    transport_valid,
    verify_direct_route,
)


class LibtorrentRoleContractTests(unittest.TestCase):
    def test_arguments_are_role_specific(self) -> None:
        common = [
            "--metainfo",
            "fixture.torrent",
            "--storage-root",
            "out",
            "--transport",
            "utp",
            "--expected-sha1",
            "11" * 20,
            "--expected-bytes",
            str(8 * 1024 * 1024),
            "--expected-piece-bytes",
            str(256 * 1024),
            "--expected-pieces",
            "32",
            "--timeout-seconds",
            "900",
        ]
        seed = parse_arguments(["seed", *common])
        self.assertEqual(seed.role, "seed")
        leech = parse_arguments(
            ["leech", *common, "--peer-address", "198.51.100.1", "--peer-port", "1"]
        )
        self.assertEqual(leech.role, "leech")
        with self.assertRaises(SystemExit):
            parse_arguments(["leech", *common])
        with self.assertRaises(SystemExit):
            parse_arguments(
                ["seed", *common, "--peer-address", "198.51.100.1", "--peer-port", "1"]
            )

    def test_transport_settings_are_mutually_exclusive_and_single_peer(self) -> None:
        tcp = transport_settings("192.168.1.2", 6000, "tcp")
        utp = transport_settings("192.168.1.2", 6000, "utp")
        for settings in (tcp, utp):
            self.assertEqual(settings["connections_limit"], 1)
            self.assertEqual(settings["connection_speed"], 1)
            self.assertFalse(settings["allow_multiple_connections_per_ip"])
            self.assertFalse(settings["enable_dht"])
            self.assertFalse(settings["enable_lsd"])
            self.assertFalse(settings["enable_upnp"])
        self.assertTrue(tcp["enable_outgoing_tcp"])
        self.assertFalse(tcp["enable_outgoing_utp"])
        self.assertFalse(utp["enable_outgoing_tcp"])
        self.assertTrue(utp["enable_outgoing_utp"])
        with self.assertRaises(LibtorrentRoleError):
            transport_settings("192.168.1.2", 6000, "mixed")

    def test_transport_evidence_rejects_masking(self) -> None:
        zero_stats = {
            "peer.num_tcp_peers": 0,
            "peer.num_utp_peers": 0,
            "utp.utp_packets_in": 0,
            "utp.utp_packets_out": 0,
        }
        self.assertTrue(transport_valid("tcp", 1, zero_stats))
        utp_stats = dict(zero_stats)
        utp_stats.update({"utp.utp_packets_in": 2, "utp.utp_packets_out": 3})
        self.assertTrue(transport_valid("utp", 1, utp_stats))
        self.assertFalse(transport_valid("utp", 2, utp_stats))

    def test_milestones_round_up(self) -> None:
        self.assertEqual(milestone_thresholds(3), {"25": 1, "50": 2, "75": 3})

    def test_loopback_scope_is_explicit(self) -> None:
        self.assertEqual(local_route_address("loopback"), "127.0.0.1")
        self.assertEqual(verify_direct_route("127.0.0.1", "loopback"), "loopback")
        with self.assertRaises(LibtorrentRoleError):
            verify_direct_route("127.0.0.1", "wan")


if __name__ == "__main__":
    unittest.main()
