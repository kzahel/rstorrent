#!/usr/bin/env python3

from __future__ import annotations

import unittest

from controlled_tcp_diagnosis import (
    DiagnosisFailure,
    DiagnosisResult,
    classify_ratio,
    owner_order,
    summarize_results,
    validate_adapter_result,
)


def result(owner: str, throughput: float, run: int = 1) -> DiagnosisResult:
    return DiagnosisResult(
        size_bytes=1024 * 1024,
        piece_size=256 * 1024,
        piece_count=4,
        run=run,
        order=1,
        owner=owner,
        profile="matched-plain-30",
        version="fixture",
        published_seconds=1.0 / throughput,
        active_seconds=None,
        throughput_mib_s=throughput,
        cpu_seconds=1.0,
        cpu_core_equivalents=throughput,
        peak_rss_bytes=1024,
        payload_sha1="0" * 40,
        payload_bytes=1024 * 1024,
        payload_download_bytes=1024 * 1024,
        redundant_bytes=0,
        failed_bytes=0,
        wire_method="tcp-plaintext",
        connected_peer_high_water=1,
        tcp_peer_high_water=1,
        utp_peer_high_water=0,
        milestones={"published": 1.0 / throughput},
        diagnostics={},
        independent_verification={"verified": True},
        cleanup_succeeded=True,
    )


class ControlledTcpDiagnosisTests(unittest.TestCase):
    def test_owner_order_rotates_without_duplication(self) -> None:
        owners = ("focused", "resumable", "libtorrent")
        self.assertEqual(owner_order(1, owners), list(owners))
        self.assertEqual(
            owner_order(2, owners), ["resumable", "libtorrent", "focused"]
        )
        self.assertEqual(
            owner_order(3, owners), ["libtorrent", "focused", "resumable"]
        )

    def test_ratio_classification_has_ten_percent_near_parity_band(self) -> None:
        self.assertEqual(classify_ratio(0.899), "behind")
        self.assertEqual(classify_ratio(0.9), "near_parity")
        self.assertEqual(classify_ratio(1.1), "near_parity")
        self.assertEqual(classify_ratio(1.101), "ahead")

    def test_summary_compares_both_paths_to_one_reference(self) -> None:
        summary = summarize_results(
            [
                result("focused", 100.0),
                result("focused", 110.0, 2),
                result("resumable", 80.0),
                result("resumable", 90.0, 2),
                result("libtorrent", 100.0),
                result("libtorrent", 100.0, 2),
            ]
        )[0]
        self.assertEqual(summary["owners"]["focused"]["median_mib_s"], 105.0)
        self.assertEqual(
            summary["owners"]["focused"]["classification"], "near_parity"
        )
        self.assertEqual(summary["owners"]["resumable"]["classification"], "behind")
        self.assertAlmostEqual(summary["resumable_focused_ratio"], 85.0 / 105.0)

    def test_adapter_rejects_utp_or_more_than_one_peer(self) -> None:
        valid = {
            "outcome": "milestone_reached",
            "integrity_verified": True,
            "cleanup_succeeded": True,
            "diagnostics": {
                "peer_methods": {
                    "connected_high_water": 1,
                    "tcp_high_water": 1,
                    "utp_high_water": 0,
                    "payload_contributor_plaintext_stream": True,
                    "payload_contributor_plaintext_payload": False,
                    "payload_contributor_rc4": False,
                }
            },
        }
        validate_adapter_result("resumable", valid, "matched-plain-30")
        valid["diagnostics"]["peer_methods"]["utp_high_water"] = 1
        with self.assertRaisesRegex(DiagnosisFailure, "transport evidence"):
            validate_adapter_result("resumable", valid, "matched-plain-30")


if __name__ == "__main__":
    unittest.main()
