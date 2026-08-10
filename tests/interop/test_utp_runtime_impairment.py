#!/usr/bin/env python3
"""Deterministic policy contracts for the real-socket uTP relay."""

from __future__ import annotations

import unittest

from utp_runtime_impairment import ImpairmentFailure, RelayPolicy, utp_packet_type


def packet(packet_type: int, length: int = 20) -> bytes:
    return bytes([(packet_type << 4) | 1]) + bytes(length - 1)


class UtpRuntimeImpairmentTests(unittest.TestCase):
    def test_packet_shape_is_shallow_and_version_bound(self) -> None:
        self.assertEqual(utp_packet_type(packet(0)), 0)
        self.assertEqual(utp_packet_type(packet(4)), 4)
        self.assertIsNone(utp_packet_type(b"short"))
        self.assertIsNone(utp_packet_type(bytes([0]) + bytes(19)))
        self.assertIsNone(utp_packet_type(packet(5)))

    def test_sparse_and_burst_loss_use_exact_data_ordinals(self) -> None:
        sparse = RelayPolicy("sparse-loss")
        sparse_drops = [
            ordinal
            for ordinal in range(1, 202)
            if sparse.decide("target-to-client", packet(0)).drop
        ]
        self.assertEqual(sparse_drops, [100, 200])

        burst = RelayPolicy("burst-loss")
        burst_drops = [
            ordinal
            for ordinal in range(1, 70)
            if burst.decide("target-to-client", packet(0)).drop
        ]
        self.assertEqual(burst_drops, [64, 65, 66])

    def test_duplicate_reorder_and_mtu_profiles_are_exact(self) -> None:
        policy = RelayPolicy("duplicate-reorder")
        decisions = [
            policy.decide("target-to-client", packet(0)) for _ in range(159)
        ]
        self.assertTrue(decisions[52].reordered)
        self.assertEqual(decisions[78].delays_seconds, (0.002, 0.003))
        self.assertTrue(decisions[105].reordered)
        self.assertEqual(decisions[157].delays_seconds, (0.002, 0.003))

        mtu = RelayPolicy("mtu-black-hole")
        self.assertFalse(mtu.decide("target-to-client", packet(0, 1280)).drop)
        self.assertTrue(mtu.decide("target-to-client", packet(0, 1281)).drop)
        self.assertFalse(mtu.decide("client-to-target", packet(0, 1400)).drop)

    def test_delay_jitter_alternates_independently_each_way(self) -> None:
        policy = RelayPolicy("delay-jitter")
        self.assertEqual(
            [policy.decide("client-to-target", packet(2)).delays_seconds for _ in range(4)],
            [(0.005,), (0.025,), (0.005,), (0.025,)],
        )
        self.assertEqual(
            [policy.decide("target-to-client", packet(0)).delays_seconds for _ in range(4)],
            [(0.005,), (0.025,), (0.005,), (0.025,)],
        )
        self.assertEqual(policy.packet_ordinal, 8)

    def test_unknown_relay_direction_is_rejected(self) -> None:
        with self.assertRaisesRegex(ImpairmentFailure, "unknown relay direction"):
            RelayPolicy("clean").decide("unexpected", packet(0))


if __name__ == "__main__":
    unittest.main()
