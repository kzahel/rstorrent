#!/usr/bin/env python3

from __future__ import annotations

import unittest
from unittest.mock import patch

from upnp_external_seeding import GateFailure, SERVICE_TYPE_V1
from wan_transport_mapping import (
    MAPPING_SERVICE_TYPES,
    MappingError,
    add_mapping,
    validate_installed,
)


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

    def test_mapping_accepts_igd_v1_after_preferred_v2(self) -> None:
        installed = {
            "NewInternalClient": "192.168.1.9",
            "NewInternalPort": "42000",
            "NewEnabled": "1",
            "NewPortMappingDescription": "RSTorrent-matrix",
            "NewLeaseDuration": "3600",
        }
        with (
            patch(
                "wan_transport_mapping.local_route_address",
                return_value="192.168.1.9",
            ),
            patch(
                "wan_transport_mapping.discover_control",
                return_value=("http://gateway/control", SERVICE_TYPE_V1),
            ) as discover,
            patch(
                "wan_transport_mapping.query_mapping",
                side_effect=(None, installed),
            ),
            patch("wan_transport_mapping._soap_values"),
            patch(
                "wan_transport_mapping.external_address", return_value="8.8.8.8"
            ),
        ):
            result = add_mapping(42000, "UDP")
        discover.assert_called_once_with(
            "192.168.1.9", service_types=MAPPING_SERVICE_TYPES
        )
        self.assertEqual(result["lease_seconds"], 3600)

    def test_discovery_failure_is_typed_as_mapping_failure(self) -> None:
        with (
            patch(
                "wan_transport_mapping.local_route_address",
                return_value="192.168.1.9",
            ),
            patch(
                "wan_transport_mapping.discover_control",
                side_effect=GateFailure("no accepted IGD service"),
            ),
        ):
            with self.assertRaisesRegex(
                MappingError, "could not select an accepted mapping service"
            ):
                add_mapping(42000, "UDP")


if __name__ == "__main__":
    unittest.main()
