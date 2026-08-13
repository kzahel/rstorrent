#!/usr/bin/env python3
"""Compare independent BEP 52 fixtures with RSTorrent and pinned libtorrent."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import resource
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import libtorrent as lt


BLOCK = 16 * 1024
PAD_FLAG = int(lt.file_storage.flag_pad_file)


class OracleFailure(RuntimeError):
    pass


@dataclass(frozen=True)
class SourceFile:
    path: tuple[bytes, ...]
    data: bytes


@dataclass(frozen=True)
class Fixture:
    name: str
    torrent: bytes
    info: bytes
    expected: dict[str, Any]


def bencode(value: Any) -> bytes:
    if isinstance(value, int):
        return b"i" + str(value).encode("ascii") + b"e"
    if isinstance(value, bytes):
        return str(len(value)).encode("ascii") + b":" + value
    if isinstance(value, list):
        return b"l" + b"".join(bencode(item) for item in value) + b"e"
    if isinstance(value, dict):
        return b"d" + b"".join(
            bencode(key) + bencode(value[key]) for key in sorted(value)
        ) + b"e"
    raise TypeError(f"cannot bencode {type(value)!r}")


def pair_hash(left: bytes, right: bytes) -> bytes:
    return hashlib.sha256(left + right).digest()


def root_from_hashes(hashes: list[bytes], target_leaves: int | None = None) -> bytes:
    if not hashes:
        raise OracleFailure("cannot construct an empty Merkle tree")
    if target_leaves is None:
        target_leaves = 1 << (len(hashes) - 1).bit_length()
    if target_leaves < len(hashes) or target_leaves & (target_leaves - 1):
        raise OracleFailure("invalid independent Merkle target")
    level = list(hashes) + [bytes(32)] * (target_leaves - len(hashes))
    while len(level) != 1:
        level = [pair_hash(level[index], level[index + 1]) for index in range(0, len(level), 2)]
    return level[0]


def file_hashes(data: bytes, piece_length: int) -> tuple[bytes | None, list[bytes]]:
    if not data:
        return None, []
    blocks = [
        hashlib.sha256(data[offset : offset + BLOCK]).digest()
        for offset in range(0, len(data), BLOCK)
    ]
    file_root = root_from_hashes(blocks)
    if len(data) <= piece_length:
        return file_root, []
    blocks_per_piece = piece_length // BLOCK
    piece_roots = []
    for offset in range(0, len(data), piece_length):
        piece = data[offset : offset + piece_length]
        piece_blocks = [
            hashlib.sha256(piece[block : block + BLOCK]).digest()
            for block in range(0, len(piece), BLOCK)
        ]
        piece_roots.append(root_from_hashes(piece_blocks, blocks_per_piece))
    if root_from_hashes(piece_roots) != file_root:
        raise OracleFailure("independent piece layer did not reconstruct file root")
    return file_root, piece_roots


def geometry(files: list[SourceFile], piece_length: int) -> tuple[list[dict[str, Any]], int, int]:
    cursor = 0
    piece_index = 0
    normalized = []
    for source in files:
        length = len(source.data)
        if length:
            cursor = (cursor + piece_length - 1) // piece_length * piece_length
            count = (length + piece_length - 1) // piece_length
            offset = cursor
            start_piece = piece_index
            cursor += length
            piece_index += count
        else:
            count = 0
            offset = cursor
            start_piece = piece_index
        root, _ = file_hashes(source.data, piece_length)
        normalized.append(
            {
                "raw_path_hex": [component.hex() for component in source.path],
                "length": length,
                "logical_offset": offset,
                "start_piece": start_piece,
                "piece_count": count,
                "pieces_root": None if root is None else root.hex(),
            }
        )
    return normalized, cursor, piece_index


def make_file_tree(files: list[SourceFile], piece_length: int) -> tuple[dict[bytes, Any], dict[bytes, bytes]]:
    tree: dict[bytes, Any] = {}
    layers: dict[bytes, bytes] = {}
    for source in files:
        node = tree
        for component in source.path:
            node = node.setdefault(component, {})
        root, piece_roots = file_hashes(source.data, piece_length)
        leaf: dict[bytes, Any] = {b"length": len(source.data)}
        if root is not None:
            leaf[b"pieces root"] = root
        node[b""] = leaf
        if piece_roots:
            layers[root] = b"".join(piece_roots)
    return tree, layers


def expected_layers(layers: dict[bytes, bytes]) -> list[dict[str, Any]]:
    return [
        {
            "pieces_root": root.hex(),
            "hashes": [
                encoded[offset : offset + 32].hex()
                for offset in range(0, len(encoded), 32)
            ],
        }
        for root, encoded in sorted(layers.items())
    ]


def pure_v2_fixture(name: str, files: list[SourceFile], piece_length: int) -> Fixture:
    tree, layers = make_file_tree(files, piece_length)
    info_value = {
        b"file tree": tree,
        b"meta version": 2,
        b"name": b"root",
        b"piece length": piece_length,
    }
    info = bencode(info_value)
    torrent = bencode({b"info": info_value, b"piece layers": layers})
    normalized, logical_bytes, logical_pieces = geometry(files, piece_length)
    layer_rows = expected_layers(layers)
    return Fixture(
        name=name,
        torrent=torrent,
        info=info,
        expected={
            "implementation": "rstorrent",
            "format": "v2",
            "exact_info_bytes": len(info),
            "v1_info_hash": None,
            "v2_info_hash": hashlib.sha256(info).hexdigest(),
            "piece_length": piece_length,
            "payload_bytes": sum(len(source.data) for source in files),
            "logical_bytes": logical_bytes,
            "logical_pieces": logical_pieces,
            "files": normalized,
            "v1_files": [],
            "piece_layers": layer_rows,
            "retained_layer_hashes": sum(len(row["hashes"]) for row in layer_rows),
        },
    )


def hybrid_fixture(name: str, files: list[SourceFile], piece_length: int, tail_pad: bool) -> Fixture:
    tree, layers = make_file_tree(files, piece_length)
    normalized, logical_bytes, logical_pieces = geometry(files, piece_length)
    v1_entries = []
    v1_normalized = []
    v1_payload = bytearray()
    cursor = 0
    for source, geometry_row in zip(files, normalized, strict=True):
        gap = geometry_row["logical_offset"] - cursor
        if gap:
            pad_path = [b".pad", str(gap).encode("ascii")]
            v1_entries.append({b"attr": b"p", b"length": gap, b"path": pad_path})
            v1_normalized.append(
                {
                    "path": [".pad", str(gap)],
                    "length": gap,
                    "offset": cursor,
                    "padding": True,
                }
            )
            v1_payload.extend(bytes(gap))
            cursor += gap
        v1_entries.append({b"length": len(source.data), b"path": list(source.path)})
        v1_normalized.append(
            {
                "path": [component.decode("ascii") for component in source.path],
                "length": len(source.data),
                "offset": cursor,
                "padding": False,
            }
        )
        v1_payload.extend(source.data)
        cursor += len(source.data)
    tail_length = (-cursor) % piece_length
    if tail_pad and tail_length:
        pad_path = [b".pad", str(tail_length).encode("ascii")]
        v1_entries.append({b"attr": b"p", b"length": tail_length, b"path": pad_path})
        v1_normalized.append(
            {
                "path": [".pad", str(tail_length)],
                "length": tail_length,
                "offset": cursor,
                "padding": True,
            }
        )
        v1_payload.extend(bytes(tail_length))
    pieces = b"".join(
        hashlib.sha1(v1_payload[offset : offset + piece_length]).digest()
        for offset in range(0, len(v1_payload), piece_length)
    )
    info_value = {
        b"file tree": tree,
        b"files": v1_entries,
        b"meta version": 2,
        b"name": b"root",
        b"piece length": piece_length,
        b"pieces": pieces,
    }
    info = bencode(info_value)
    torrent = bencode({b"info": info_value, b"piece layers": layers})
    layer_rows = expected_layers(layers)
    return Fixture(
        name=name,
        torrent=torrent,
        info=info,
        expected={
            "implementation": "rstorrent",
            "format": "hybrid",
            "exact_info_bytes": len(info),
            "v1_info_hash": hashlib.sha1(info).hexdigest(),
            "v2_info_hash": hashlib.sha256(info).hexdigest(),
            "piece_length": piece_length,
            "payload_bytes": sum(len(source.data) for source in files),
            "logical_bytes": logical_bytes,
            "logical_pieces": logical_pieces,
            "files": normalized,
            "v1_files": v1_normalized,
            "piece_layers": layer_rows,
            "retained_layer_hashes": sum(len(row["hashes"]) for row in layer_rows),
        },
    )


def fixtures() -> list[Fixture]:
    single = [SourceFile((b"single.bin",), bytes((index * 17) % 251 for index in range(20_001)))]
    multi = [
        SourceFile((b"a-empty",), b""),
        SourceFile((b"b-small",), b"b"),
        SourceFile((b"c-large",), bytes((index * 29) % 253 for index in range(70_123))),
    ]
    hybrid = [
        SourceFile((b"a",), b"a"),
        SourceFile((b"b",), bytes((index * 31) % 247 for index in range(20_003))),
    ]
    return [
        pure_v2_fixture("pure-v2-single", single, BLOCK),
        pure_v2_fixture("pure-v2-multi-64k", multi, 64 * 1024),
        hybrid_fixture("hybrid-canonical-tail", hybrid, BLOCK, True),
        hybrid_fixture("hybrid-missing-tail", hybrid, BLOCK, False),
    ]


def run_rstorrent(binary: Path, path: Path, accept: bool = True) -> dict[str, Any] | None:
    result = subprocess.run(
        [str(binary), "bep52", str(path)],
        cwd=repository_root(),
        text=True,
        capture_output=True,
        check=False,
    )
    if accept and result.returncode != 0:
        raise OracleFailure(f"RSTorrent rejected {path.name}: {result.stderr.strip()}")
    if not accept:
        if result.returncode == 0:
            raise OracleFailure(f"RSTorrent unexpectedly accepted {path.name}")
        return None
    return json.loads(result.stdout)


def assert_rstorrent(fixture: Fixture, actual: dict[str, Any]) -> None:
    measured = {key: actual[key] for key in fixture.expected}
    if measured != fixture.expected:
        raise OracleFailure(
            f"RSTorrent normalization differs for {fixture.name}:\n"
            f"expected={json.dumps(fixture.expected, sort_keys=True)}\n"
            f"actual={json.dumps(measured, sort_keys=True)}"
        )


def assert_libtorrent(fixture: Fixture, path: Path) -> None:
    torrent = lt.torrent_info(str(path))
    if bytes(torrent.info_section()) != fixture.info:
        raise OracleFailure(f"libtorrent changed exact info bytes for {fixture.name}")
    hashes = torrent.info_hashes()
    if str(hashes.v2) != fixture.expected["v2_info_hash"]:
        raise OracleFailure(f"libtorrent v2 identity differs for {fixture.name}")
    if fixture.expected["v1_info_hash"] is not None and str(hashes.v1) != fixture.expected["v1_info_hash"]:
        raise OracleFailure(f"libtorrent v1 identity differs for {fixture.name}")
    if torrent.piece_length() != fixture.expected["piece_length"]:
        raise OracleFailure(f"libtorrent piece length differs for {fixture.name}")
    if torrent.num_pieces() != fixture.expected["logical_pieces"]:
        raise OracleFailure(f"libtorrent logical piece count differs for {fixture.name}")

    storage = torrent.orig_files()
    payload_files = []
    for index in range(storage.num_files()):
        if int(storage.file_flags(index)) & PAD_FLAG:
            continue
        path = storage.file_path(index)
        if path.startswith("root/"):
            path = path[len("root/") :]
        payload_files.append(
            {
                "path": path,
                "length": storage.file_size(index),
                "logical_offset": storage.file_offset(index),
            }
        )
    expected_files = [
        {
            "path": "/".join(bytes.fromhex(component).decode("ascii") for component in row["raw_path_hex"]),
            "length": row["length"],
            "logical_offset": row["logical_offset"],
        }
        for row in fixture.expected["files"]
    ]
    if payload_files != expected_files:
        raise OracleFailure(
            f"libtorrent files differ for {fixture.name}: {payload_files!r} != {expected_files!r}"
        )


def assert_strict_differences(binary: Path, directory: Path, base: Fixture) -> int:
    decoded = lt.bdecode(base.torrent)
    del decoded[b"piece layers"]
    missing = lt.bencode(decoded)
    missing_path = directory / "strict-missing-layers.torrent"
    missing_path.write_bytes(missing)
    lt.torrent_info(str(missing_path))
    run_rstorrent(binary, missing_path, accept=False)

    decoded = lt.bdecode(base.torrent)
    decoded[b"piece layers"] = {}
    incomplete = lt.bencode(decoded)
    incomplete_path = directory / "strict-incomplete-layers.torrent"
    incomplete_path.write_bytes(incomplete)
    lt.torrent_info(str(incomplete_path))
    run_rstorrent(binary, incomplete_path, accept=False)
    return len(missing) + len(incomplete)


def assert_zero_root_difference(binary: Path, directory: Path) -> int:
    info_value = {
        b"file tree": {
            b"zero.bin": {b"": {b"length": 1, b"pieces root": bytes(32)}}
        },
        b"meta version": 2,
        b"name": b"root",
        b"piece length": BLOCK,
    }
    torrent = bencode({b"info": info_value, b"piece layers": {}})
    path = directory / "all-zero-present-root.torrent"
    path.write_bytes(torrent)
    normalized = run_rstorrent(binary, path)
    assert normalized is not None
    if normalized["files"][0]["pieces_root"] != bytes(32).hex():
        raise OracleFailure("RSTorrent did not retain the present all-zero root")
    try:
        lt.torrent_info(str(path))
    except RuntimeError:
        return len(torrent)
    raise OracleFailure("pinned libtorrent unexpectedly retained an all-zero pieces root")


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    repository = repository_root()
    binary = repository / "target" / "debug" / "rstorrent-metainfo-compare"
    if not args.no_build:
        subprocess.run(
            ["cargo", "build", "-p", "rstorrent-engine", "--bin", "rstorrent-metainfo-compare"],
            cwd=repository,
            check=True,
        )
    if not binary.is_file():
        raise OracleFailure(f"missing comparer binary {binary}")

    cases = fixtures()
    peak_allocation = 0
    peak_hashes = 0
    temporary_bytes = 0
    with tempfile.TemporaryDirectory(prefix="rstorrent-bep52-oracle-") as temporary:
        directory = Path(temporary)
        for fixture in cases:
            path = directory / f"{fixture.name}.torrent"
            path.write_bytes(fixture.torrent)
            temporary_bytes += len(fixture.torrent)
            normalized = run_rstorrent(binary, path)
            assert normalized is not None
            assert_rstorrent(fixture, normalized)
            assert_libtorrent(fixture, path)
            peak_allocation = max(peak_allocation, normalized["transient_peak_bytes"])
            peak_hashes = max(peak_hashes, normalized["retained_layer_hashes"])
        temporary_bytes += assert_strict_differences(binary, directory, cases[0])
        temporary_bytes += assert_zero_root_difference(binary, directory)

    rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    child_rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    if os.uname().sysname != "Darwin":
        rss *= 1024
        child_rss *= 1024
    print(
        "oracle=bep52-metainfo "
        f"libtorrent_binding={lt.__version__} libtorrent_native={lt.version} "
        f"fixtures={len(cases)} strict_differences=3 "
        f"peak_retained_layer_hashes={peak_hashes} "
        f"peak_rstorrent_transient_bytes={peak_allocation} "
        f"oracle_rss_bytes={rss} child_peak_rss_bytes={child_rss} "
        f"temporary_disk_bytes={temporary_bytes} cleaned=true"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
