#!/usr/bin/env python3

from __future__ import annotations

import argparse
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from wan_transport_matrix import (
    WanMatrixRevisionError,
    _rstorrent_timing,
    _rstorrent_transport_evidence,
    require_repository_revision,
    required_remote_rstorrent_binaries,
    run_matrix,
    seed_transport_evidence,
)


class WanTransportMatrixTests(unittest.TestCase):
    def test_remote_staging_builds_only_selected_rstorrent_roles(self) -> None:
        case = lambda direction, seed, leech: argparse.Namespace(
            direction=direction, seed=seed, leech=leech
        )

        self.assertEqual(
            required_remote_rstorrent_binaries(
                [case("local-seed", "rstorrent", "libtorrent")]
            ),
            (),
        )
        self.assertEqual(
            required_remote_rstorrent_binaries(
                [case("local-seed", "libtorrent", "rstorrent")]
            ),
            ("rstorrent-public-probe",),
        )
        self.assertEqual(
            required_remote_rstorrent_binaries(
                [case("remote-seed", "rstorrent", "libtorrent")]
            ),
            ("rstorrent-incoming-seed",),
        )

    def test_seed_utp_evidence_retains_only_aggregate_lifecycle_diagnostics(self) -> None:
        evidence = seed_transport_evidence(
            {
                "utp_before_shutdown": {"connections_started": 4},
                "rejection_counts": {"NoRequestTimeout": 3},
                "payload_bytes_sent": 64 * 1024 * 1024,
                "recent_rejections": [{"remote": "forbidden"}],
            },
            "rstorrent",
            "utp",
        )

        self.assertEqual(evidence["connections_started"], 4)
        self.assertEqual(evidence["incoming_rejection_counts"], {"NoRequestTimeout": 3})
        self.assertEqual(evidence["payload_bytes_sent"], 64 * 1024 * 1024)
        self.assertNotIn("recent_rejections", evidence)

    def test_rstorrent_timing_separates_connect_and_payload(self) -> None:
        self.assertEqual(
            _rstorrent_timing(
                {
                    "milestones": {
                        "torrent_admitted": 1.0,
                        "first_connection": 2.0,
                        "first_payload_byte": 2.5,
                        "published": 6.5,
                    }
                }
            ),
            {
                "connect_to_complete_seconds": 4.5,
                "first_payload_seconds": 0.5,
                "active_payload_seconds": 4.0,
            },
        )

    def test_rstorrent_evidence_excludes_peer_detail_arrays(self) -> None:
        evidence = _rstorrent_transport_evidence(
            {
                "effective_settings": {"outgoing_utp": True},
                "utp_evidence": {"connection_high_water": 1},
                "udp_evidence": {"utp_datagrams_classified": 2},
                "diagnostics": {
                    "peer_methods": {"utp_high_water": 1},
                    "content_peers": [{"remote": "forbidden"}],
                    "storage_jobs_high_water": 3,
                },
            }
        )
        self.assertNotIn("content_peers", evidence["diagnostics"])
        self.assertEqual(evidence["diagnostics"]["storage_jobs_high_water"], 3)

    def test_dry_run_selects_without_network_or_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            arguments = argparse.Namespace(
                host="pimom",
                epoch="dry-contract",
                repetitions=1,
                size_mib=[8],
                direction=["local-seed"],
                seed=["rstorrent"],
                leech=["libtorrent"],
                transport=["tcp"],
                case_id=None,
                limit=None,
                journal=root / "journal.jsonl",
                work_root=root / "work",
                allow_public_network=False,
                prepare_remote=False,
            )
            result = run_matrix(arguments)
            self.assertEqual(result["selected"], 1)
            self.assertEqual(result["execution"], "disabled-without-explicit-flag")
            self.assertFalse(arguments.journal.exists())
            self.assertFalse(arguments.work_root.exists())

    def test_revision_guard_rejects_a_moved_checkout(self) -> None:
        with patch(
            "wan_transport_matrix.repository_revision", return_value="new-revision"
        ):
            with self.assertRaises(WanMatrixRevisionError):
                require_repository_revision("selected-revision")


if __name__ == "__main__":
    unittest.main()
