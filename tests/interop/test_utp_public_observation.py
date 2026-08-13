#!/usr/bin/env python3

from __future__ import annotations

import contextlib
import io
import json
import unittest

from utp_public_observation import (
    EXPECTED_CAPABILITIES,
    EXPECTED_SETTINGS,
    PROFILE_SHA256,
    ObservationError,
    endpoint_free_summary,
    parse_args,
    validate_result,
)


def successful_result() -> dict:
    return {
        "schema_version": 2,
        "implementation": "rstorrent",
        "profile": "product-utp",
        "profile_sha256": PROFILE_SHA256,
        "input_mode": "magnet",
        "info_hash": "d" * 40,
        "outcome": "milestone_reached",
        "target": "metadata",
        "wall_seconds": 1.5,
        "milestones": {"metadata_verified": 1.25},
        "verified_piece_count": 0,
        "verified_bytes": 0,
        "integrity_verified": True,
        "cleanup_succeeded": True,
        "terminal_detail": "peer 192.0.2.1:6881",
        "effective_settings": EXPECTED_SETTINGS.copy(),
        "capabilities": EXPECTED_CAPABILITIES.copy(),
        "dht_evidence": {"families": []},
        "utp_evidence": {
            "active_connections_after_shutdown": 0,
            "connection_high_water": 0,
            "incoming_half_open_after_shutdown": 0,
            "incoming_half_open_high_water": 0,
            "incoming_stream_queue_high_water": 0,
            "connection_datagram_queue_high_water": 0,
            "malformed_datagrams": 0,
            "unknown_connection_datagrams": 0,
            "stale_generation_datagrams": 0,
            "connection_datagrams_dropped": 0,
            "datagrams_sent": 0,
            "datagram_bytes_sent": 0,
            "retransmission_datagrams_sent": 0,
            "retransmission_bytes_sent": 0,
            "retransmission_queue_high_water": 0,
            "delivered_byte_high_water": 0,
            "receive_reorder_packet_high_water": 0,
            "receive_buffered_byte_high_water": 0,
            "unsent_byte_high_water": 0,
            "sent_byte_high_water": 0,
            "slow_start_active_observed": False,
            "slow_start_threshold_byte_high_water": 0,
            "slow_start_acknowledgements_high_water": 0,
            "slow_start_exits_high_water": 0,
            "selected_mtu_min_bytes": None,
            "selected_mtu_max_bytes": None,
            "worker_panics": 0,
        },
        "udp_evidence": {
            "tasks_after_shutdown": 0,
            "task_high_water": 2,
            "queued_after_shutdown": 0,
            "queue_high_water": 3,
            "datagrams_received": 10,
            "datagram_bytes_received": 100,
            "datagrams_dropped": 0,
            "dht_datagrams_dropped": 0,
            "utp_queued_after_shutdown": 0,
            "utp_queue_high_water": 1,
            "utp_datagrams_classified": 1,
            "utp_datagram_bytes_classified": 20,
            "utp_datagrams_dropped": 0,
        },
        "diagnostics": {
            "tracker_response_batches": 1,
            "tracker_reported_peers": 2,
            "peer_dial_attempts": 3,
            "peer_methods": {
                "connected_high_water": 1,
                "tcp_high_water": 1,
                "utp_high_water": 0,
                "utp_endpoint_snapshots": 2,
                "utp_unknown_high_water": 2,
                "utp_advertised_high_water": 0,
                "utp_confirmed_high_water": 0,
                "utp_suppressed_high_water": 1,
                "utp_suppression_failures_high_water": 1,
            },
        },
    }


class UtpPublicObservationTests(unittest.TestCase):
    def test_success_contract_and_summary_are_endpoint_free(self) -> None:
        result = successful_result()
        entry = {"info_hash": "d" * 40}
        self.assertEqual(validate_result(result, entry, 0), "metadata_reached")
        rendered = json.dumps(endpoint_free_summary(result), sort_keys=True)
        self.assertNotIn("192.0.2.1", rendered)
        self.assertNotIn("terminal_detail", rendered)

    def test_unsafe_settings_and_retained_owner_are_rejected(self) -> None:
        result = successful_result()
        result["effective_settings"] = {**EXPECTED_SETTINGS, "upnp": True}
        with self.assertRaises(ObservationError):
            validate_result(result, {"info_hash": "d" * 40}, 0)

        result = successful_result()
        result["utp_evidence"]["active_connections_after_shutdown"] = 1
        with self.assertRaises(ObservationError):
            validate_result(result, {"info_hash": "d" * 40}, 0)

    def test_public_network_requires_explicit_opt_in(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            parse_args([])
        self.assertTrue(parse_args(["--allow-public-network"]).allow_public_network)


if __name__ == "__main__":
    unittest.main()
