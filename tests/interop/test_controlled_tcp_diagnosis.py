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
        checkpoint_sync=(
            "bypassed"
            if owner == "resumable-no-sync"
            else "enabled"
            if owner.startswith("resumable")
            else "not-applicable"
        ),
        activity_observation=(
            "summary"
            if owner == "resumable-summary-observation"
            else "detailed"
            if owner.startswith("resumable") or owner == "probe-nonresumable"
            else "not-applicable"
        ),
        execution_path=(
            "nonresumable"
            if owner in ("focused", "probe-nonresumable")
            else "resumable"
            if owner.startswith("resumable")
            else "libtorrent"
        ),
        payload_allowance_bytes=(
            {
                "resumable-buffer-8m": 8 * 1024 * 1024,
                "resumable-buffer-16m": 16 * 1024 * 1024,
                "resumable-buffer-32m": 32 * 1024 * 1024,
            }.get(owner, 64 * 1024 * 1024)
            if owner == "focused" or owner.startswith("resumable") or owner == "probe-nonresumable"
            else None
        ),
        storage_intake_high_watermark_bytes=(
            {
                "resumable-intake-1m": 1 * 1024 * 1024,
                "resumable-intake-2m": 2 * 1024 * 1024,
                "resumable-intake-4m": 4 * 1024 * 1024,
                "resumable-intake-6m": 6 * 1024 * 1024,
                "resumable-intake-8m": 8 * 1024 * 1024,
            }.get(owner, 48 * 1024 * 1024)
            if owner.startswith("resumable") or owner == "probe-nonresumable"
            else None
        ),
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

    def test_summary_compares_checkpoint_control_within_resumable_path(self) -> None:
        summary = summarize_results(
            [
                result("resumable", 80.0),
                result("resumable", 100.0, 2),
                result("resumable-no-sync", 100.0),
                result("resumable-no-sync", 120.0, 2),
            ]
        )[0]
        self.assertAlmostEqual(
            summary["checkpoint_bypass_enabled_ratio"], 110.0 / 90.0
        )
        self.assertEqual(
            summary["checkpoint_bypass_enabled_classification"], "ahead"
        )

    def test_summary_compares_activity_observation_within_resumable_path(self) -> None:
        summary = summarize_results(
            [
                result("resumable", 80.0),
                result("resumable", 100.0, 2),
                result("resumable-summary-observation", 81.0),
                result("resumable-summary-observation", 99.0, 2),
            ]
        )[0]
        self.assertAlmostEqual(summary["summary_detailed_observation_ratio"], 1.0)
        self.assertEqual(
            summary["summary_detailed_observation_classification"], "near_parity"
        )

    def test_summary_compares_probe_execution_paths(self) -> None:
        summary = summarize_results(
            [
                result("resumable", 80.0),
                result("resumable", 100.0, 2),
                result("probe-nonresumable", 100.0),
                result("probe-nonresumable", 120.0, 2),
            ]
        )[0]
        self.assertAlmostEqual(
            summary["resumable_probe_nonresumable_ratio"], 90.0 / 110.0
        )
        self.assertEqual(
            summary["resumable_probe_nonresumable_classification"], "behind"
        )

    def test_summary_compares_resumable_payload_allowances(self) -> None:
        summary = summarize_results(
            [
                result("resumable", 80.0),
                result("resumable", 100.0, 2),
                result("resumable-buffer-8m", 100.0),
                result("resumable-buffer-8m", 120.0, 2),
            ]
        )[0]
        self.assertAlmostEqual(summary["buffer_8m_64m_ratio"], 110.0 / 90.0)
        self.assertEqual(summary["buffer_8m_64m_classification"], "ahead")

    def test_summary_records_independent_intake_watermark(self) -> None:
        summary = summarize_results(
            [result("resumable-intake-1m", 100.0), result("libtorrent", 125.0)]
        )[0]
        intake = summary["owners"]["resumable-intake-1m"]
        self.assertEqual(intake["payload_allowance_bytes"], 64 * 1024 * 1024)
        self.assertEqual(
            intake["storage_intake_high_watermark_bytes"], 1024 * 1024
        )
        self.assertAlmostEqual(intake["libtorrent_ratio"], 0.8)

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
