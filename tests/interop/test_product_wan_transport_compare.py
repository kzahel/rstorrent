#!/usr/bin/env python3
"""Pure contracts for the paired product WAN transport comparator."""

from __future__ import annotations

import unittest
from unittest.mock import call, patch

from product_wan_transport_compare import (
    PAYLOAD_BYTES,
    PIECES,
    WanFailure,
    assert_redacted_report,
    cleanup_owned_mappings,
    expected_transport_settings,
    pair_order,
    summarize_complete_pairs,
    validate_dual_mappings,
    validate_remote_complete,
    validate_remote_ready,
)


class ProductWanTransportCompareTests(unittest.TestCase):
    def test_pair_order_alternates_and_is_bounded(self) -> None:
        self.assertEqual(pair_order(1), ("tcp", "utp"))
        self.assertEqual(pair_order(2), ("utp", "tcp"))
        self.assertEqual(pair_order(3), ("tcp", "utp"))
        self.assertEqual(pair_order(4), ("utp", "tcp"))
        with self.assertRaises(WanFailure):
            pair_order(5)

    def test_dual_mapping_targets_both_actual_listeners(self) -> None:
        ready = {
            "listen": "0.0.0.0:42000",
            "utp_listen": "0.0.0.0:42001",
            "mapping": {
                "type": "mapped",
                "local_address": "192.168.1.20",
                "local_port": 42000,
                "external_address": "8.8.8.8",
                "external_port": 48000,
                "lease_seconds": 3600,
            },
            "udp_mapping": {
                "type": "mapped",
                "local_address": "192.168.1.20",
                "local_port": 42001,
                "external_address": "8.8.8.8",
                "external_port": 48001,
                "lease_seconds": 3599,
            },
        }
        mappings = validate_dual_mappings(ready, "192.168.1.20")
        self.assertEqual(mappings["tcp"].protocol, "TCP")
        self.assertEqual(mappings["utp"].protocol, "UDP")
        ready["udp_mapping"]["external_address"] = "1.1.1.1"
        with self.assertRaises(WanFailure):
            validate_dual_mappings(ready, "192.168.1.20")

    def _complete_event(self, transport: str) -> dict[str, object]:
        stats = {
            "peer.num_tcp_peers": 0,
            "peer.num_utp_peers": 0,
            "net.recv_payload_bytes": PAYLOAD_BYTES,
            "utp.utp_packets_in": 0,
            "utp.utp_packets_out": 0,
        }
        if transport == "utp":
            stats["utp.utp_packets_in"] = 10
            stats["utp.utp_packets_out"] = 11
        return {
            "event": "complete",
            "role": "remote-leecher",
            "transport": transport,
            "peer_high_water": 1,
            "applied_transport_settings": expected_transport_settings(transport),
            "payload": {
                "bytes": PAYLOAD_BYTES,
                "pieces": PIECES,
                "sha1": "a" * 40,
            },
            "timing": {
                "connect_to_complete_seconds": 1.0,
                "first_payload_seconds": 0.2,
                "active_payload_seconds": 0.8,
                "milestone_seconds": {
                    "1": 0.3,
                    "25": 0.4,
                    "50": 0.6,
                    "75": 0.8,
                    "100": 1.0,
                },
            },
            "libtorrent_stats": stats,
            "diagnostics": [],
        }

    def test_remote_evidence_accepts_each_unmasked_transport(self) -> None:
        for transport in ("tcp", "utp"):
            with self.subTest(transport=transport):
                validate_remote_ready(
                    {
                        "event": "ready",
                        "role": "remote-leecher",
                        "pid": 123,
                        "listen_port": 42000,
                        "libtorrent_version": "2.0.13.0",
                        "route_class": "ordinary-internet",
                        "transport": transport,
                        "applied_transport_settings": expected_transport_settings(
                            transport
                        ),
                    },
                    123,
                    transport,
                )
                result = validate_remote_complete(
                    self._complete_event(transport), transport, "a" * 40
                )
                self.assertGreater(result["rates_mib_per_second"]["active"], 0)

    def test_remote_evidence_rejects_masking_and_nonmonotonic_time(self) -> None:
        tcp = self._complete_event("tcp")
        tcp["libtorrent_stats"]["utp.utp_packets_in"] = 1
        with self.assertRaises(WanFailure):
            validate_remote_complete(tcp, "tcp", "a" * 40)
        utp = self._complete_event("utp")
        utp["timing"]["milestone_seconds"]["75"] = 0.1
        with self.assertRaises(WanFailure):
            validate_remote_complete(utp, "utp", "a" * 40)

    def test_summary_requires_three_complete_pairs_and_keeps_order_strata(self) -> None:
        attempts = [
            {
                "status": "complete",
                "order": ["tcp", "utp"],
                "ratios": {
                    "active_utp_over_tcp": ratio,
                    "connect_utp_over_tcp": ratio + 0.1,
                },
            }
            for ratio in (0.4, 0.6)
        ]
        self.assertIsNone(summarize_complete_pairs(attempts))
        attempts.insert(
            1,
            {
                "status": "complete",
                "order": ["utp", "tcp"],
                "ratios": {
                    "active_utp_over_tcp": 0.5,
                    "connect_utp_over_tcp": 0.6,
                },
            },
        )
        summary = summarize_complete_pairs(attempts)
        self.assertIsNotNone(summary)
        assert summary is not None
        self.assertEqual(summary["active_utp_over_tcp"]["median"], 0.5)
        self.assertEqual(summary["order_strata"]["tcp-then-utp"]["pairs"], 2)
        self.assertEqual(summary["order_strata"]["utp-then-tcp"]["pairs"], 1)

    def test_cleanup_deletes_only_exact_owned_protocol_entries(self) -> None:
        owned = [
            {
                "NewInternalClient": "192.168.1.20",
                "NewPortMappingDescription": "RSTorrent",
                "NewProtocol": "TCP",
                "NewExternalPort": "42000",
            },
            {
                "NewInternalClient": "192.168.1.20",
                "NewPortMappingDescription": "RSTorrent",
                "NewProtocol": "UDP",
                "NewExternalPort": "42001",
            },
        ]
        with (
            patch(
                "product_wan_transport_compare.list_mappings",
                side_effect=[owned, []],
            ),
            patch("product_wan_transport_compare.delete_mapping") as delete,
        ):
            cleanup_owned_mappings("control", "service", "192.168.1.20")
        self.assertEqual(
            delete.call_args_list,
            [
                call("control", "service", 42000, "TCP"),
                call("control", "service", 42001, "UDP"),
            ],
        )

    def test_report_rejects_control_or_network_identity(self) -> None:
        assert_redacted_report(
            {"status": "complete", "libtorrent_version": "2.0.13.0"}, "pimom"
        )
        with self.assertRaises(WanFailure):
            assert_redacted_report({"host": "pimom"}, "pimom")
        with self.assertRaises(WanFailure):
            assert_redacted_report({"endpoint": "8.8.8.8:42000"}, "pimom")


if __name__ == "__main__":
    unittest.main()
