#!/usr/bin/env python3
"""Deterministic policy contracts for the real-socket uTP relay."""

from __future__ import annotations

import unittest

from utp_runtime_impairment import (
    DIAGNOSTIC_MTU_PROFILE,
    PRODUCT_MTU_1280_PROFILE,
    ImpairmentFailure,
    RelayPolicy,
    parse_process_time,
    utp_packet_type,
)


def packet(packet_type: int, length: int = 20, sequence: int = 0) -> bytes:
    payload = bytearray([(packet_type << 4) | 1]) + bytearray(length - 1)
    payload[16:18] = sequence.to_bytes(2, "big")
    return bytes(payload)


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

    def test_diagnostic_mtu_drops_first_oversized_sequence_then_retries(self) -> None:
        for profile in (DIAGNOSTIC_MTU_PROFILE, PRODUCT_MTU_1280_PROFILE):
            with self.subTest(profile=profile):
                mtu = RelayPolicy(profile)
                self.assertFalse(
                    mtu.decide("target-to-client", packet(0, 1280, 10)).drop
                )
                protected = mtu.decide(
                    "target-to-client", packet(0, 1281, 10)
                )
                self.assertTrue(protected.drop)
                retry = mtu.decide("target-to-client", packet(0, 1281, 10))
                self.assertFalse(retry.drop)
                self.assertTrue(retry.fragmentable_mtu_retry)
                next_probe = mtu.decide(
                    "target-to-client", packet(0, 1400, 11)
                )
                self.assertTrue(next_probe.drop)
                self.assertFalse(
                    mtu.decide("client-to-target", packet(0, 1400, 12)).drop
                )

    def test_process_time_parser_accepts_bsd_and_linux_shapes(self) -> None:
        self.assertEqual(parse_process_time("0:00.05"), 0.05)
        self.assertEqual(parse_process_time("01:02:03"), 3723.0)
        self.assertEqual(parse_process_time("2-01:02:03"), 176523.0)
        with self.assertRaisesRegex(ImpairmentFailure, "unexpected process CPU"):
            parse_process_time("unexpected")

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
