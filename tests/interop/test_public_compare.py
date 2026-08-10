#!/usr/bin/env python3

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

from public_compare import (
    DHT_BOOTSTRAP_NODES,
    HarnessError,
    MAX_UTILITY_SAMPLES,
    append_utility_sample,
    classify_owner,
    classify_pair,
    distribution,
    implementation_order,
    integer_distribution,
    libtorrent_utility_sample,
    libtorrent_settings,
    load_catalog,
    mark_percent_milestones,
    scenario_magnets,
    selected_implementations,
    summarize,
)


def outcome(value: str, seconds: float | None = None) -> dict:
    return {"outcome": value, "milestones": {"metadata_verified": seconds}}


class PublicCompareTests(unittest.TestCase):
    def test_reference_dht_uses_documented_backup_routers(self) -> None:
        tracker_settings, _ = libtorrent_settings("common")
        dht_settings, _ = libtorrent_settings("dht")
        self.assertEqual(tracker_settings["dht_bootstrap_nodes"], "")
        self.assertEqual(dht_settings["dht_bootstrap_nodes"], DHT_BOOTSTRAP_NODES)
        self.assertIn("router.bittorrent.com:6881", DHT_BOOTSTRAP_NODES)
        self.assertIn("dht.transmissionbt.com:6881", DHT_BOOTSTRAP_NODES)

    def test_catalog_and_derived_magnets(self) -> None:
        catalog = load_catalog(Path(__file__).parents[1] / "live" / "torrents.json")
        self.assertEqual(len(catalog["torrents"]), 5)
        source = catalog["torrents"][0]
        common_rst, common_lib = scenario_magnets(source, "common")
        self.assertEqual(common_rst, common_lib)
        self.assertIn("tr=udp", common_rst)
        self.assertNotIn("wss", common_rst)
        self.assertNotIn("ws=", common_rst)
        dht_rst, dht_lib = scenario_magnets(source, "dht")
        self.assertEqual(dht_rst, dht_lib)
        self.assertNotIn("tr=", dht_rst)
        self.assertIn(source["info_hash"], dht_rst)

    def test_catalog_rejects_duplicate_slug(self) -> None:
        catalog = {
            "schema_version": 1,
            "torrents": [
                {
                    "slug": "same",
                    "name": "one",
                    "info_hash": "a" * 40,
                    "magnet": f"magnet:?xt=urn:btih:{'a' * 40}",
                    "payload_bytes": None,
                    "piece_count": None,
                    "file_count": None,
                }
            ]
            * 2,
        }
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "catalog.json"
            path.write_text(json.dumps(catalog), encoding="utf-8")
            with self.assertRaises(HarnessError):
                load_catalog(path)

    def test_all_pair_classifications(self) -> None:
        reached = outcome("milestone_reached")
        timeout = outcome("timeout")
        error = outcome("error")
        harness = outcome("harness_error")
        self.assertEqual(classify_pair(reached, reached), "both_reached")
        self.assertEqual(classify_pair(timeout, reached), "reference_only")
        self.assertEqual(classify_pair(reached, error), "rstorrent_only")
        self.assertEqual(classify_pair(timeout, error), "both_incomplete")
        self.assertEqual(classify_pair(harness, reached), "harness_error")
        self.assertEqual(classify_owner(reached), "owner_reached")
        self.assertEqual(classify_owner(timeout), "owner_incomplete")
        self.assertEqual(classify_owner(harness), "harness_error")

    def test_order_alternates(self) -> None:
        self.assertEqual(implementation_order(0), ["rstorrent", "libtorrent"])
        self.assertEqual(implementation_order(1), ["libtorrent", "rstorrent"])
        self.assertEqual(implementation_order(2), ["libtorrent", "rstorrent"])
        self.assertEqual(implementation_order(3), ["rstorrent", "libtorrent"])
        self.assertEqual(selected_implementations(1, "both"), ["libtorrent", "rstorrent"])
        self.assertEqual(selected_implementations(1, "rstorrent"), ["rstorrent"])

    def test_thresholds_and_summary(self) -> None:
        milestones = {
            "50_percent_verified": None,
            "95_percent_verified": None,
            "99_percent_verified": None,
        }
        mark_percent_milestones(milestones, 949, 1000, 4.0)
        self.assertEqual(milestones["50_percent_verified"], 4.0)
        self.assertIsNone(milestones["95_percent_verified"])
        mark_percent_milestones(milestones, 990, 1000, 5.0)
        self.assertEqual(milestones["95_percent_verified"], 5.0)
        self.assertEqual(milestones["99_percent_verified"], 5.0)

        runs = [
            {
                "classification": "both_reached",
                "implementations": {
                    "rstorrent": outcome("milestone_reached", 4.0),
                    "libtorrent": outcome("milestone_reached", 2.0),
                },
            },
            {
                "classification": "reference_only",
                "implementations": {
                    "rstorrent": outcome("timeout"),
                    "libtorrent": outcome("milestone_reached", 3.0),
                },
            },
        ]
        summary = summarize(runs, "metadata")
        self.assertEqual(summary["comparable_samples"], 1)
        self.assertEqual(summary["rstorrent_over_libtorrent"]["median"], 2.0)
        self.assertEqual(summary["classifications"]["reference_only"], 1)

        owner_runs = [
            {
                "classification": "owner_reached",
                "implementations": {"rstorrent": outcome("milestone_reached", 4.0)},
            },
            {
                "classification": "owner_incomplete",
                "implementations": {"rstorrent": outcome("timeout")},
            },
        ]
        owner_summary = summarize(owner_runs, "metadata", "rstorrent")
        self.assertEqual(owner_summary["owner"], "rstorrent")
        self.assertEqual(owner_summary["classifications"]["owner_reached"], 1)
        self.assertEqual(owner_summary["owner_seconds"]["median"], 4.0)

    def test_distribution_uses_nearest_rank_p90(self) -> None:
        self.assertEqual(distribution(list(range(1, 11)))["p90"], 9)
        self.assertIsNone(distribution([])["median"])

    def test_integer_distribution_and_utility_bound(self) -> None:
        values = integer_distribution(list(range(10)))
        self.assertEqual(values["median"], 4)
        self.assertEqual(values["p90"], 8)
        self.assertIsNone(integer_distribution([])["max"])

        samples: list[dict] = []
        coalesced = 0
        for ordinal in range(MAX_UTILITY_SAMPLES + 1):
            coalesced += append_utility_sample(samples, {"elapsed_seconds": ordinal})
        self.assertLessEqual(len(samples), MAX_UTILITY_SAMPLES)
        self.assertEqual(samples[0]["elapsed_seconds"], 0)
        self.assertEqual(samples[-1]["elapsed_seconds"], MAX_UTILITY_SAMPLES)
        self.assertGreater(coalesced, 0)

    def test_libtorrent_utility_sample_is_aggregate_and_endpoint_free(self) -> None:
        status = SimpleNamespace(
            total_wanted_done=300,
            num_pieces=2,
            list_peers=7,
            connect_candidates=3,
            num_connections=2,
            num_peers=1,
            download_payload_rate=90,
        )
        peer = SimpleNamespace(
            flags=0,
            payload_down_speed=80,
            download_queue_length=4,
            total_download=250,
            queue_bytes=65_536,
            pending_disk_bytes=16_384,
            ip="203.0.113.9:6881",
            client="must not escape",
        )
        sample = libtorrent_utility_sample(status, [peer], 3.0, (1.0, 100))
        self.assertEqual(sample["verified_rate"], 100)
        self.assertEqual(sample["known_peers"], 7)
        self.assertEqual(sample["connecting_peers"], 1)
        self.assertEqual(sample["active_requests"], 4)
        self.assertEqual(sample["pending_disk_bytes"], 16_384)
        self.assertIsNone(sample["storage_queue_wait_micros"])
        self.assertIsNone(sample["storage_write_service_micros"])
        self.assertIsNone(sample["storage_hash_service_micros"])
        self.assertIsNone(sample["storage_write_blocks_completed"])
        self.assertIsNone(sample["storage_write_batch_blocks_high_water"])
        self.assertIsNone(sample["storage_write_batch_bytes_high_water"])
        self.assertIsNone(sample["storage_active_kind"])
        self.assertIsNone(sample["storage_active_age_micros"])
        rendered = json.dumps(sample)
        self.assertNotIn(peer.ip, rendered)
        self.assertNotIn(peer.client, rendered)


if __name__ == "__main__":
    unittest.main()
