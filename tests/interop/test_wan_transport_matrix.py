#!/usr/bin/env python3

from __future__ import annotations

import argparse
import tempfile
import unittest
from pathlib import Path

from wan_transport_matrix import (
    _rstorrent_timing,
    _rstorrent_transport_evidence,
    run_matrix,
)


class WanTransportMatrixTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
