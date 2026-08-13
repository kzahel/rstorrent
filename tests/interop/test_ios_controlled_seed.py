from __future__ import annotations

import hashlib
import shutil
import tempfile
import unittest
from pathlib import Path

from ios_controlled_seed import MIB, create_fixture


class IOSControlledSeedTests(unittest.TestCase):
    def test_multifile_fixture_has_cross_file_pieces_and_exact_manifest(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="rstorrent-ios-seed-test-"))
        try:
            torrent, metainfo, combined_sha1, manifest = create_fixture(
                root,
                2 * MIB,
                "multifile",
            )
            self.assertTrue(metainfo.is_file())
            self.assertEqual(torrent.num_files(), 3)
            self.assertEqual(torrent.total_size(), 2 * MIB)
            self.assertEqual(torrent.piece_length(), 256 * 1024)
            self.assertNotEqual(manifest[0]["length"] % torrent.piece_length(), 0)
            self.assertNotEqual(
                (manifest[0]["length"] + manifest[1]["length"])
                % torrent.piece_length(),
                0,
            )

            combined = hashlib.sha1()
            for entry in manifest:
                payload = root / "seed" / entry["path"]
                data = payload.read_bytes()
                self.assertEqual(len(data), entry["length"])
                self.assertEqual(hashlib.sha1(data).hexdigest(), entry["sha1"])
                combined.update(data)
            self.assertEqual(combined.hexdigest(), combined_sha1)
        finally:
            shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
