#!/usr/bin/env python3

from __future__ import annotations

import unittest

from wan_transport_mapping import MappingError, validate_installed


class WanTransportMappingTests(unittest.TestCase):
    def test_exact_finite_mapping_passes(self) -> None:
        self.assertEqual(
            validate_installed(
                {
                    "NewInternalClient": "192.168.1.9",
                    "NewInternalPort": "42000",
                    "NewEnabled": "1",
                    "NewPortMappingDescription": "RSTorrent-matrix",
                    "NewLeaseDuration": "3600",
                },
                local_address="192.168.1.9",
                port=42000,
            ),
            3600,
        )

    def test_foreign_or_permanent_mapping_fails(self) -> None:
        base = {
            "NewInternalClient": "192.168.1.9",
            "NewInternalPort": "42000",
            "NewEnabled": "1",
            "NewPortMappingDescription": "foreign",
            "NewLeaseDuration": "0",
        }
        with self.assertRaises(MappingError):
            validate_installed(base, local_address="192.168.1.9", port=42000)


if __name__ == "__main__":
    unittest.main()
