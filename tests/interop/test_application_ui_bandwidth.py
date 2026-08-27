from __future__ import annotations

import unittest

from application_ui_bandwidth import (
    ScenarioFailure,
    frame_metric_totals,
    validate_cross_check,
)


class ApplicationUiBandwidthTests(unittest.TestCase):
    def test_frame_metric_totals_sum_families(self) -> None:
        self.assertEqual(
            frame_metric_totals(
                {
                    "server_frames": {
                        "connected": {"messages": 1, "bytes": 200},
                        "view_batch": {"messages": 3, "bytes": 900},
                    }
                },
                "server_frames",
            ),
            (4, 1100),
        )

    def test_cross_check_accepts_exact_browser_and_gateway_totals(self) -> None:
        browser = {
            "schemaVersion": 1,
            "applicationUpgrades": 1,
            "semanticHttpRequests": [],
            "total": {
                "client_to_server": {"messages": 2, "payload_bytes": 80},
                "server_to_client": {"messages": 3, "payload_bytes": 900},
            },
        }
        gateway = {
            "accepted_connections": 1,
            "active_connections": 0,
            "heartbeat_timeouts": 0,
            "client_frames": {"connect": {"messages": 2, "bytes": 80}},
            "server_frames": {"view_batch": {"messages": 3, "bytes": 900}},
        }
        validate_cross_check(browser, gateway)

    def test_cross_check_rejects_byte_disagreement(self) -> None:
        browser = {
            "schemaVersion": 1,
            "applicationUpgrades": 1,
            "semanticHttpRequests": [],
            "total": {
                "client_to_server": {"messages": 1, "payload_bytes": 10},
                "server_to_client": {"messages": 1, "payload_bytes": 20},
            },
        }
        gateway = {
            "accepted_connections": 1,
            "active_connections": 0,
            "heartbeat_timeouts": 0,
            "client_frames": {"connect": {"messages": 1, "bytes": 10}},
            "server_frames": {"connected": {"messages": 1, "bytes": 21}},
        }
        with self.assertRaisesRegex(ScenarioFailure, "byte cross-check differs"):
            validate_cross_check(browser, gateway)


if __name__ == "__main__":
    unittest.main()
