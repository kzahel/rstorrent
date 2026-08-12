#!/usr/bin/env python3

from __future__ import annotations

import copy
import hashlib
import json
import os
import tempfile
import unittest
from pathlib import Path

from public_compare_contract import (
    MAX_NETWORK_BYTES,
    ContractError,
    comparison_profile,
    distribution,
    invocation_network_budget,
    load_catalog_document,
    normalize_profile,
    parse_metainfo,
    required_free_space,
    validate_catalog_document,
    validate_output_ancestry,
    validate_retained_report,
    verify_payload,
    wire_payload_ceiling,
)


def bencode(value: object) -> bytes:
    if isinstance(value, bytes):
        return str(len(value)).encode() + b":" + value
    if isinstance(value, int):
        return b"i" + str(value).encode() + b"e"
    if isinstance(value, list):
        return b"l" + b"".join(bencode(item) for item in value) + b"e"
    if isinstance(value, dict):
        items = sorted(value.items())
        return b"d" + b"".join(bencode(key) + bencode(item) for key, item in items) + b"e"
    raise TypeError(type(value).__name__)


def fixture_metainfo() -> tuple[bytes, bytes, bytes]:
    first = b"abcde"
    second = b"1234567"
    logical = first + bytes(3) + second
    pieces = hashlib.sha1(logical[:8]).digest() + hashlib.sha1(logical[8:]).digest()
    info = {
        b"files": [
            {b"length": len(first), b"path": [b"a.bin"]},
            {b"attr": b"p", b"length": 3, b"path": [b".pad", b"3"]},
            {b"length": len(second), b"path": [b"dir", b"b.bin"]},
        ],
        b"name": b"fixture",
        b"piece length": 8,
        b"pieces": pieces,
    }
    metainfo = {
        b"announce": b"https://tracker.example/announce",
        b"announce-list": [
            [b"https://tracker.example/announce"],
            [b"udp://tracker.example:80/announce"],
        ],
        b"info": info,
        b"url-list": [b"https://seed.example/fixture/"],
    }
    return bencode(metainfo), first, second


class PublicCompareContractTests(unittest.TestCase):
    def test_metainfo_identity_geometry_and_sources(self) -> None:
        payload, _, _ = fixture_metainfo()
        descriptor = parse_metainfo(payload)
        self.assertEqual(descriptor.outer_sha256, hashlib.sha256(payload).hexdigest())
        self.assertEqual(descriptor.payload_bytes, 15)
        self.assertEqual(descriptor.padding_bytes, 3)
        self.assertEqual(descriptor.piece_length, 8)
        self.assertEqual(descriptor.piece_count, 2)
        self.assertEqual(descriptor.file_count, 3)
        self.assertFalse(descriptor.private)
        self.assertEqual(
            descriptor.tracker_tiers,
            (
                ("https://tracker.example/announce",),
                ("udp://tracker.example:80/announce",),
            ),
        )
        self.assertEqual(descriptor.web_seeds, ("https://seed.example/fixture/",))
        self.assertEqual(descriptor.info_hash, hashlib.sha1(descriptor.raw_info).hexdigest())

    def test_metainfo_rejects_hostile_shape(self) -> None:
        payload, _, _ = fixture_metainfo()
        with self.assertRaises(ContractError):
            parse_metainfo(payload + b"junk")
        with self.assertRaises(ContractError):
            parse_metainfo(payload.replace(b"5:a.bin", b"5:../xx"))
        with self.assertRaises(ContractError):
            parse_metainfo(payload.replace(b"12:piece lengthi8e", b"12:piece lengthi0e"))
        duplicate = b"d4:infod4:name1:a4:name1:bee"
        with self.assertRaises(ContractError):
            parse_metainfo(duplicate)

    def test_independent_streaming_verifier_handles_padding_and_boundaries(self) -> None:
        payload, first, second = fixture_metainfo()
        descriptor = parse_metainfo(payload)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "fixture" / "dir").mkdir(parents=True)
            (root / "fixture" / "a.bin").write_bytes(first)
            (root / "fixture" / "dir" / "b.bin").write_bytes(second)
            result = verify_payload(descriptor, root)
            self.assertTrue(result["verified"])
            self.assertEqual(result["piece_count"], 2)
            self.assertEqual(result["logical_bytes"], 15)
            self.assertEqual(result["physical_bytes_read"], 12)
            (root / "fixture" / "dir" / "b.bin").write_bytes(b"x" + second[1:])
            with self.assertRaisesRegex(ContractError, "piece 1 SHA-1 mismatch"):
                verify_payload(descriptor, root)

    def test_verifier_rejects_extra_missing_wrong_kind_and_symlink(self) -> None:
        payload, first, second = fixture_metainfo()
        descriptor = parse_metainfo(payload)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "fixture" / "dir").mkdir(parents=True)
            (root / "fixture" / "a.bin").write_bytes(first)
            (root / "fixture" / "dir" / "b.bin").write_bytes(second)
            (root / "extra").write_bytes(b"no")
            with self.assertRaisesRegex(ContractError, "file set mismatch"):
                verify_payload(descriptor, root)
            (root / "extra").unlink()
            (root / "fixture" / "a.bin").unlink()
            os.symlink(root / "fixture" / "dir" / "b.bin", root / "fixture" / "a.bin")
            with self.assertRaisesRegex(ContractError, "non-regular file"):
                verify_payload(descriptor, root)

    def test_tracked_catalog_v2_is_valid(self) -> None:
        catalog = load_catalog_document(Path(__file__).parents[1] / "live" / "torrents.json")
        self.assertEqual(catalog["schema_version"], 2)
        self.assertEqual(len(catalog["torrents"]), 9)
        primary = catalog["torrents"][0]
        self.assertEqual(primary["roles"], ["small-primary"])
        self.assertEqual(
            primary["limits"]["wire_payload_ceiling_bytes"],
            wire_payload_ceiling(primary["expected"]["payload_bytes"]),
        )

    def test_catalog_rejects_duplicate_identity_and_unpinned_metainfo(self) -> None:
        catalog = load_catalog_document(Path(__file__).parents[1] / "live" / "torrents.json")
        duplicate = copy.deepcopy(catalog)
        duplicate["torrents"][1]["info_hash"] = duplicate["torrents"][0]["info_hash"]
        duplicate["torrents"][1]["magnet"] = duplicate["torrents"][0]["magnet"]
        with self.assertRaisesRegex(ContractError, "duplicates info hash"):
            validate_catalog_document(duplicate)
        unpinned = copy.deepcopy(catalog)
        entry = unpinned["torrents"][0]
        entry["metainfo"]["sha256"] = None
        with self.assertRaisesRegex(ContractError, "SHA-256"):
            validate_catalog_document(unpinned)

    def test_profiles_are_complete_hashed_and_aliases_are_stable(self) -> None:
        self.assertEqual(normalize_profile("common"), "matched-plain-30")
        plain = comparison_profile("matched-plain-30")
        rc4 = comparison_profile("matched-rc4-30")
        product = comparison_profile("product-default")
        self.assertEqual(plain["rstorrent"]["session_connection_limit"], 30)
        self.assertEqual(plain["libtorrent"]["connections_limit"], 30)
        self.assertEqual(plain["rstorrent"]["upload_slots"], 8)
        self.assertEqual(plain["libtorrent"]["unchoke_slots_limit"], 8)
        self.assertEqual(plain["rstorrent"]["encryption"], "disabled")
        self.assertEqual(rc4["libtorrent"]["allowed_enc_level"], "rc4")
        self.assertTrue(rc4["libtorrent"]["prefer_rc4"])
        self.assertTrue(product["libtorrent"]["enable_outgoing_utp"])
        self.assertFalse(product["rstorrent"]["outgoing_utp"])
        self.assertRegex(plain["sha256"], r"^[0-9a-f]{64}$")
        self.assertEqual(plain, comparison_profile("common"))

    def test_wan_profiles_are_direct_single_peer_and_transport_exact(self) -> None:
        tcp = comparison_profile("wan-tcp")
        utp = comparison_profile("wan-utp")
        for profile in (tcp, utp):
            self.assertEqual(profile["comparison_kind"], "wan-direct")
            self.assertEqual(profile["rstorrent"]["session_connection_limit"], 1)
            self.assertFalse(profile["rstorrent"]["tracker"])
            self.assertFalse(profile["rstorrent"]["dht"])
            self.assertFalse(profile["rstorrent"]["pex"])
            self.assertEqual(profile["libtorrent"]["connections_limit"], 1)
            self.assertFalse(profile["libtorrent"]["enable_dht"])
            self.assertFalse(profile["libtorrent"]["tracker"])
        self.assertTrue(tcp["libtorrent"]["enable_outgoing_tcp"])
        self.assertFalse(tcp["libtorrent"]["enable_outgoing_utp"])
        self.assertFalse(utp["libtorrent"]["enable_outgoing_tcp"])
        self.assertTrue(utp["libtorrent"]["enable_outgoing_utp"])
        self.assertTrue(utp["rstorrent"]["outgoing_tcp_fallback"])

    def test_resource_math_and_cleanup_ancestry_are_bounded(self) -> None:
        payload = 1024 * 1024 * 1024
        self.assertEqual(wire_payload_ceiling(payload), payload * 3 // 2)
        self.assertEqual(required_free_space(payload), payload + 2 * 1024 * 1024 * 1024)
        self.assertEqual(invocation_network_budget(payload, 2), payload * 6)
        with self.assertRaisesRegex(ContractError, "exceeds"):
            invocation_network_budget(16 * 1024 * 1024 * 1024, 20)
        self.assertEqual(MAX_NETWORK_BYTES, 64 * 1024 * 1024 * 1024)
        with tempfile.TemporaryDirectory() as temporary:
            parent = Path(temporary).resolve()
            child = parent / "run-0" / "rstorrent"
            self.assertEqual(validate_output_ancestry(parent, child), child)
            with self.assertRaisesRegex(ContractError, "escapes"):
                validate_output_ancestry(parent, parent)
            with self.assertRaisesRegex(ContractError, "escapes"):
                validate_output_ancestry(parent, parent.parent / "elsewhere")

    def test_report_privacy_and_distribution_contract(self) -> None:
        validate_retained_report({"runs": [{"connected_peers": 3, "detail": "bounded"}]})
        for key in ("peer_addresses", "output_root", "raw_log"):
            with self.assertRaisesRegex(ContractError, "privacy-forbidden"):
                validate_retained_report({key: "must not escape"})
        values = distribution([1.0, 2.0, 3.0, 10.0])
        self.assertEqual(values["median"], 2.5)
        self.assertEqual(values["p90"], 10.0)
        self.assertEqual(values["mad"], 1.0)
        self.assertIsNone(distribution([1.0, 2.0, 3.0])["p90"])
        self.assertIsNone(distribution([])["median"])


if __name__ == "__main__":
    unittest.main()
