#!/usr/bin/env python3
"""Deterministic fixed-geometry fixtures for the Tactical 142 matrix."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import libtorrent as lt

from wan_transport_matrix_contract import MIB, PIECE_BYTES, SIZES_MIB


PAYLOAD_NAME = "payload.bin"
TORRENT_NAME = "fixture.torrent"
SOURCE_BLOCK_BYTES = MIB
SOURCE_BLOCK = bytes(
    ((offset * 73) ^ (offset >> 3) ^ (offset >> 11) ^ 0xA5) & 0xFF
    for offset in range(SOURCE_BLOCK_BYTES)
)


class FixtureError(RuntimeError):
    pass


@dataclass(frozen=True)
class Fixture:
    size_mib: int
    payload_bytes: int
    piece_bytes: int
    piece_count: int
    sha1: str
    info_hash: str
    root: Path
    seed_root: Path
    payload: Path
    metainfo: Path

    def private_dict(self) -> dict[str, Any]:
        return {
            "size_mib": self.size_mib,
            "payload_bytes": self.payload_bytes,
            "piece_bytes": self.piece_bytes,
            "piece_count": self.piece_count,
            "sha1": self.sha1,
            "info_hash": self.info_hash,
            "root": str(self.root),
            "seed_root": str(self.seed_root),
            "payload": str(self.payload),
            "metainfo": str(self.metainfo),
        }


def write_payload(path: Path, payload_bytes: int) -> str:
    if payload_bytes <= 0 or payload_bytes % SOURCE_BLOCK_BYTES != 0:
        raise FixtureError("matrix payload must be a positive whole number of MiB")
    if path.exists():
        raise FixtureError("matrix payload path already exists")
    digest = hashlib.sha1()
    remaining = payload_bytes
    with path.open("xb", buffering=0) as output:
        while remaining:
            chunk = SOURCE_BLOCK[: min(remaining, len(SOURCE_BLOCK))]
            written = output.write(chunk)
            if written != len(chunk):
                raise FixtureError("matrix payload write was short")
            digest.update(chunk)
            remaining -= written
        output.flush()
        os.fsync(output.fileno())
    return digest.hexdigest()


def hash_file(path: Path) -> str:
    digest = hashlib.sha1()
    with path.open("rb") as source:
        while chunk := source.read(MIB):
            digest.update(chunk)
    return digest.hexdigest()


def inspect_fixture(root: Path, size_mib: int) -> Fixture:
    if size_mib not in SIZES_MIB:
        raise FixtureError("fixture size is outside the matrix set")
    seed_root = root / "seed"
    payload = seed_root / PAYLOAD_NAME
    metainfo = root / TORRENT_NAME
    if not payload.is_file() or not metainfo.is_file():
        raise FixtureError("matrix fixture is incomplete")
    payload_bytes = size_mib * MIB
    if payload.stat().st_size != payload_bytes:
        raise FixtureError("matrix fixture payload size is wrong")
    sha1 = hash_file(payload)
    torrent_info = lt.torrent_info(str(metainfo))
    piece_count = payload_bytes // PIECE_BYTES
    if (
        lt.version != "2.0.13.0"
        or torrent_info.total_size() != payload_bytes
        or torrent_info.piece_length() != PIECE_BYTES
        or torrent_info.num_pieces() != piece_count
        or torrent_info.num_files() != 1
        or torrent_info.name() != PAYLOAD_NAME
        or any(True for _ in torrent_info.trackers())
        or any(True for _ in torrent_info.web_seeds())
    ):
        raise FixtureError("matrix metainfo geometry or sources are wrong")
    return Fixture(
        size_mib=size_mib,
        payload_bytes=payload_bytes,
        piece_bytes=PIECE_BYTES,
        piece_count=piece_count,
        sha1=sha1,
        info_hash=str(torrent_info.info_hash()),
        root=root,
        seed_root=seed_root,
        payload=payload,
        metainfo=metainfo,
    )


def create_fixture(root: Path, size_mib: int) -> Fixture:
    if size_mib not in SIZES_MIB:
        raise FixtureError("fixture size is outside the matrix set")
    if root.exists():
        return inspect_fixture(root, size_mib)
    root.mkdir(parents=True)
    seed_root = root / "seed"
    seed_root.mkdir()
    payload = seed_root / PAYLOAD_NAME
    try:
        expected_sha1 = write_payload(payload, size_mib * MIB)
        files = lt.file_storage()
        files.add_file(PAYLOAD_NAME, size_mib * MIB)
        creator = lt.create_torrent(
            files,
            piece_size=PIECE_BYTES,
            flags=lt.create_torrent.v1_only,
        )
        lt.set_piece_hashes(creator, str(seed_root))
        (root / TORRENT_NAME).write_bytes(bytes(lt.bencode(creator.generate())))
        fixture = inspect_fixture(root, size_mib)
        if fixture.sha1 != expected_sha1:
            raise FixtureError("matrix fixture SHA-1 changed during creation")
        return fixture
    except BaseException:
        for path in (root / TORRENT_NAME, payload):
            path.unlink(missing_ok=True)
        try:
            seed_root.rmdir()
            root.rmdir()
        except OSError:
            pass
        raise


def materialize_payload(root: Path, metainfo: Path, expected_sha1: str) -> Fixture:
    torrent_info = lt.torrent_info(str(metainfo))
    payload_bytes = torrent_info.total_size()
    if payload_bytes % MIB != 0:
        raise FixtureError("remote fixture is not an exact MiB size")
    size_mib = payload_bytes // MIB
    if size_mib not in SIZES_MIB:
        raise FixtureError("remote fixture size is outside the matrix set")
    if not root.exists():
        root.mkdir(parents=True)
    seed_root = root / "seed"
    seed_root.mkdir(exist_ok=False)
    payload = seed_root / PAYLOAD_NAME
    sha1 = write_payload(payload, payload_bytes)
    if sha1 != expected_sha1:
        raise FixtureError("materialized payload differs from the expected SHA-1")
    target_metainfo = root / TORRENT_NAME
    target_metainfo.write_bytes(metainfo.read_bytes())
    return inspect_fixture(root, size_mib)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="action", required=True)
    create = subparsers.add_parser("create")
    create.add_argument("--root", type=Path, required=True)
    create.add_argument("--size-mib", type=int, choices=SIZES_MIB, required=True)
    inspect = subparsers.add_parser("inspect")
    inspect.add_argument("--root", type=Path, required=True)
    inspect.add_argument("--size-mib", type=int, choices=SIZES_MIB, required=True)
    materialize = subparsers.add_parser("materialize")
    materialize.add_argument("--root", type=Path, required=True)
    materialize.add_argument("--metainfo", type=Path, required=True)
    materialize.add_argument("--expected-sha1", required=True)
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    try:
        if arguments.action == "create":
            fixture = create_fixture(arguments.root, arguments.size_mib)
        elif arguments.action == "inspect":
            fixture = inspect_fixture(arguments.root, arguments.size_mib)
        else:
            fixture = materialize_payload(
                arguments.root, arguments.metainfo, arguments.expected_sha1
            )
        print(json.dumps(fixture.private_dict(), sort_keys=True))
        return 0
    except (FixtureError, OSError, RuntimeError) as error:
        print(f"WAN fixture failed: {error}", file=os.sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
