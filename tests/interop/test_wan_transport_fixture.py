#!/usr/bin/env python3

from __future__ import annotations

import hashlib
import tempfile
import unittest
from pathlib import Path

from wan_transport_fixture import (
    MIB,
    SOURCE_BLOCK,
    FixtureError,
    create_fixture,
    hash_file,
    inspect_fixture,
    write_payload,
)


class FixtureContractTests(unittest.TestCase):
    def test_source_block_is_deterministic_and_nonconstant(self) -> None:
        self.assertEqual(len(SOURCE_BLOCK), MIB)
        self.assertGreater(len(set(SOURCE_BLOCK)), 200)
        self.assertEqual(
            hashlib.sha256(SOURCE_BLOCK).hexdigest(),
            "0392711a898375041d66270a6f59b62daab7003bb9b6808bf82981b876b2555d",
        )

    def test_payload_writer_is_exact_and_exclusive(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "payload.bin"
            expected = hashlib.sha1(SOURCE_BLOCK * 2).hexdigest()
            self.assertEqual(write_payload(path, 2 * MIB), expected)
            self.assertEqual(hash_file(path), expected)
            with self.assertRaises(FixtureError):
                write_payload(path, 2 * MIB)

    def test_smallest_matrix_fixture_is_idempotent_and_exact(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "fixture"
            first = create_fixture(root, 8)
            second = inspect_fixture(root, 8)
            third = create_fixture(root, 8)
            self.assertEqual(first, second)
            self.assertEqual(second, third)
            self.assertEqual(first.payload_bytes, 8 * MIB)
            self.assertEqual(first.piece_count, 32)


if __name__ == "__main__":
    unittest.main()
