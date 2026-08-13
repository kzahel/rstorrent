#!/usr/bin/env python3
"""Deterministic contracts for the mapped uTP WAN harness."""

from __future__ import annotations

import argparse
import ipaddress
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from product_utp_reachability import (
    gateway_preflight,
    product_receive_summary,
    validate_product_stop,
    validate_udp_mapping,
)
from utp_remote_seed import eligible_public_ipv4, parse_mapping_entries
from utp_remote_leecher import (
    create_transport_session,
    milestone_thresholds,
    transport_settings,
)
from utp_reference_oracle import create_fixture
from utp_rstorrent_wan import (
    WanFailure,
    aborted_remote_summary,
    audit_local_mapping,
    bounded_diagnostics,
    cohort_size,
    eligible_local_seed_endpoint,
    eligible_public_endpoint,
    redacted_rstorrent,
    summarize_samples,
    validate_mapping_intent,
    validate_remote_leecher_complete,
    validate_remote_leecher_ready,
    verify_direct_route,
)


class UtpWanContractTests(unittest.TestCase):
    def test_remote_transport_settings_are_mutually_exclusive(self) -> None:
        tcp = transport_settings("127.0.0.1", 42000, "tcp")
        utp = transport_settings("127.0.0.1", 42001, "utp")
        self.assertTrue(tcp["enable_incoming_tcp"])
        self.assertTrue(tcp["enable_outgoing_tcp"])
        self.assertFalse(tcp["enable_incoming_utp"])
        self.assertFalse(tcp["enable_outgoing_utp"])
        self.assertFalse(utp["enable_incoming_tcp"])
        self.assertFalse(utp["enable_outgoing_tcp"])
        self.assertTrue(utp["enable_incoming_utp"])
        self.assertTrue(utp["enable_outgoing_utp"])
        self.assertEqual(tcp["proxy_type"], 0)
        self.assertEqual(utp["proxy_type"], 0)
        self.assertFalse(tcp["allow_multiple_connections_per_ip"])
        self.assertFalse(utp["allow_multiple_connections_per_ip"])
        self.assertEqual(tcp["connections_limit"], 1)
        self.assertEqual(utp["connections_limit"], 1)

        tcp_session, tcp_applied = create_transport_session(
            "127.0.0.1", 42000, "tcp"
        )
        utp_session, utp_applied = create_transport_session(
            "127.0.0.1", 42001, "utp"
        )
        try:
            self.assertTrue(tcp_applied["enable_outgoing_tcp"])
            self.assertFalse(tcp_applied["enable_outgoing_utp"])
            self.assertFalse(utp_applied["enable_outgoing_tcp"])
            self.assertTrue(utp_applied["enable_outgoing_utp"])
        finally:
            tcp_session.pause()
            utp_session.pause()

    def test_comparator_fixture_geometry_and_milestones_are_exact(self) -> None:
        payload_bytes = 8 * 1024 * 1024 + 731
        piece_bytes = 256 * 1024
        with tempfile.TemporaryDirectory() as temporary:
            torrent_info, seed_root, expected_sha1 = create_fixture(
                Path(temporary),
                payload_size=payload_bytes,
                piece_size=piece_bytes,
            )
            self.assertEqual(torrent_info.total_size(), payload_bytes)
            self.assertEqual(torrent_info.piece_length(), piece_bytes)
            self.assertEqual(torrent_info.num_pieces(), 33)
            self.assertEqual((seed_root / "payload.bin").stat().st_size, payload_bytes)
            self.assertEqual(len(expected_sha1), 40)
        self.assertEqual(
            milestone_thresholds(payload_bytes),
            {
                "1": 83_894,
                "25": 2_097_335,
                "50": 4_194_670,
                "75": 6_292_005,
            },
        )

    def test_product_udp_mapping_targets_the_actual_utp_listener(self) -> None:
        ready = {
            "utp_listen": "192.168.1.20:42001",
            "udp_mapping": {
                "type": "mapped",
                "local_address": "192.168.1.20",
                "local_port": 42001,
                "external_address": "8.8.8.8",
                "external_port": 48001,
                "lease_seconds": 3600,
            },
        }
        self.assertEqual(
            validate_udp_mapping(ready),
            ("192.168.1.20", 42001, "8.8.8.8", 48001, 3600),
        )
        ready["utp_listen"] = "0.0.0.0:42001"
        self.assertEqual(
            validate_udp_mapping(ready),
            ("192.168.1.20", 42001, "8.8.8.8", 48001, 3600),
        )
        ready["utp_listen"] = "192.168.1.20:42002"
        with self.assertRaises(WanFailure):
            validate_udp_mapping(ready)

    def test_product_stop_requires_exact_utp_and_mapping_termination(self) -> None:
        stopped = {
            "connection_high_water": 1,
            "mapping_tasks_after_shutdown": 0,
            "mappings_after_shutdown": 0,
            "utp_before_shutdown": {
                "connection_high_water": 1,
                "worker_panics": 0,
                "datagrams_sent": 4,
            },
        }
        validate_product_stop(stopped)
        stopped["mappings_after_shutdown"] = 1
        with self.assertRaises(WanFailure):
            validate_product_stop(stopped)

    def test_product_receive_summary_is_bounded_to_counters(self) -> None:
        stopped = {
            "udp_before_shutdown": {
                "datagrams_received": 3,
                "utp_datagrams_classified": 2,
                "utp_datagrams_dropped": 1,
            },
            "utp_before_shutdown": {
                "incoming_half_open_high_water": 1,
                "connection_high_water": 1,
                "datagrams_sent": 4,
            },
        }
        self.assertEqual(
            product_receive_summary(stopped),
            "product receive evidence: udp_received=3, utp_classified=2, "
            "utp_dropped=1, utp_half_open=1, utp_connections=1, utp_sent=4",
        )

    def test_product_gateway_preflight_rejects_owned_residue(self) -> None:
        with (
            patch("product_utp_reachability.local_route_address", return_value="192.168.1.20"),
            patch(
                "product_utp_reachability.discover_control",
                return_value=("http://gateway/control", "service"),
            ),
            patch(
                "product_utp_reachability.list_mappings",
                return_value=[
                    {
                        "NewInternalClient": "192.168.1.20",
                        "NewPortMappingDescription": "RSTorrent",
                    }
                ],
            ),
        ):
            with self.assertRaises(WanFailure):
                gateway_preflight()

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
                "transfer_seconds": 80.0,
            },
            "a" * 40,
        )

    def test_cohort_bound_and_summary_are_deterministic(self) -> None:
        self.assertEqual(cohort_size("3"), 3)
        with self.assertRaises(argparse.ArgumentTypeError):
            cohort_size("4")
        utp_names = (
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
            "receive_reorder_packet_high_water",
            "receive_buffered_byte_high_water",
            "unsent_byte_high_water",
            "sent_byte_high_water",
            "retransmission_datagrams_sent",
            "retransmission_bytes_sent",
            "loss_reduction_high_water",
            "timeout_collapse_high_water",
            "slow_start_threshold_byte_high_water",
            "slow_start_acknowledgements_high_water",
            "slow_start_exits_high_water",
        )
        oracle_names = (
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
        )

        def sample(seconds: int) -> dict[str, object]:
            utp = {name: seconds for name in utp_names}
            utp.update(
                {
                    "datagrams_sent": seconds,
                    "datagram_bytes_sent": seconds,
                    "slow_start_active_observed": True,
                }
            )
            return {
                "seconds": seconds,
                "active_transfer_seconds": seconds - 1,
                "rstorrent": {
                    "payload": {"choke_retries": 0, "duplicate_blocks": 0},
                    "resources": {
                        "live_utp": utp,
                        "live_udp": {
                            "utp_datagrams_classified": seconds,
                            "utp_datagram_bytes_classified": seconds,
                        },
                    }
                },
                "remote": {
                    "libtorrent_stats": {name: seconds for name in oracle_names}
                },
            }

        summary = summarize_samples([sample(3), sample(1), sample(2)])
        self.assertEqual(summary["samples"], 3)
        self.assertEqual(
            summary["metrics"]["case_seconds"],
            {"min": 1, "median": 2, "max": 3},
        )

    def test_remote_leecher_failure_evidence_is_bounded(self) -> None:
        with self.assertRaisesRegex(
            WanFailure,
            "progress_ppm=500000.*wanted_done=1048576.*timeouts=2",
        ):
            validate_remote_leecher_complete(
                {
                    "event": "failed",
                    "role": "remote-leecher",
                    "reason": "transfer-timeout",
                    "progress_ppm": 500000,
                    "wanted_done_bytes": 1048576,
                    "libtorrent_stats": {
                        "utp.utp_packets_in": 10,
                        "utp.utp_packets_out": 11,
                        "utp.utp_packet_loss": 1,
                        "utp.utp_timeout": 2,
                        "utp.utp_packet_resend": 3,
                    },
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
