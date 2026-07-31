#!/usr/bin/env python3
"""Exercise both public-comparator adapters against one controlled local seed."""

from __future__ import annotations

import gc
import hashlib
import json
import shutil
import sys
import tempfile
from pathlib import Path

import libtorrent as lt

from first_verified_piece import add_seed, create_session, wait_for_listener
from public_compare import (
    build_probe,
    classify_pair,
    repository_root,
    run_libtorrent,
    run_rstorrent,
    summarize,
)


PIECE_LENGTH = 32 * 1024
FILES = (("video/clip.bin", 70_000), ("notes/readme.txt", 9_000))


class ControlledFailure(RuntimeError):
    pass


def write_payload(path: Path, size: int, salt: int) -> str:
    path.parent.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha1()
    with path.open("wb") as output:
        for offset in range(size):
            value = bytes((((offset * 73) ^ (offset >> 3) ^ salt) & 0xFF,))
            output.write(value)
            digest.update(value)
    return digest.hexdigest()


def create_fixture(root: Path) -> tuple[lt.torrent_info, Path, dict[str, str]]:
    seed = root / "seed"
    torrent_root = seed / "controlled-comparison"
    expected: dict[str, str] = {}
    files = lt.file_storage()
    for index, (relative, size) in enumerate(FILES):
        path = torrent_root / relative
        expected[relative] = write_payload(path, size, index + 17)
        files.add_file(f"controlled-comparison/{relative}", size)
    creator = lt.create_torrent(files, piece_size=PIECE_LENGTH, flags=lt.create_torrent.v1_only)
    lt.set_piece_hashes(creator, str(seed))
    torrent_path = root / "controlled.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    return lt.torrent_info(str(torrent_path)), seed, expected


def verify_output(output_root: Path, expected: dict[str, str], *, includes_root: bool) -> None:
    for relative, digest in expected.items():
        base = output_root / "controlled-comparison" if includes_root else output_root
        path = base / relative
        if not path.is_file():
            raise ControlledFailure(f"missing published file {path}")
        actual = hashlib.sha1(path.read_bytes()).hexdigest()
        if actual != digest:
            raise ControlledFailure(f"published file hash mismatch for {relative}")


def main() -> int:
    repository = repository_root()
    binary = build_probe(repository)
    session: lt.session | None = None
    seed_handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    with tempfile.TemporaryDirectory(prefix="rstorrent-controlled-compare-") as temporary:
        owned = Path(temporary)
        torrent_info, seed, expected = create_fixture(owned)
        info_hash = str(torrent_info.info_hashes().v1)
        try:
            session = create_session()
            port = wait_for_listener(session, diagnostics)
            seed_handle = add_seed(session, torrent_info, seed, diagnostics)
            magnet = f"magnet:?xt=urn:btih:{info_hash}&x.pe=127.0.0.1:{port}"
            rst_output = owned / "rstorrent"
            lib_output = owned / "libtorrent"
            rstorrent = run_rstorrent(binary, magnet, "common", "complete", 30, 10, rst_output)
            verify_output(rst_output, expected, includes_root=False)
            libtorrent = run_libtorrent(
                magnet,
                info_hash,
                "common",
                "complete",
                30,
                lib_output,
            )
            verify_output(lib_output, expected, includes_root=True)
            classification = classify_pair(rstorrent, libtorrent)
            run = {
                "ordinal": 0,
                "order": ["rstorrent", "libtorrent"],
                "classification": classification,
                "implementations": {
                    "rstorrent": rstorrent,
                    "libtorrent": libtorrent,
                },
            }
            report = {
                "schema_version": 1,
                "config": {
                    "profile": "common",
                    "target": "complete",
                    "controlled": True,
                    "info_hash": info_hash,
                },
                "runs": [run],
                "summary": summarize([run], "complete"),
            }
            print(json.dumps(report, indent=2, sort_keys=True))
            if classification != "both_reached":
                raise ControlledFailure(f"unexpected classification {classification}")
        finally:
            if session is not None and seed_handle is not None and seed_handle.is_valid():
                session.remove_torrent(seed_handle)
            if session is not None:
                session.pause()
            seed_handle = None
            session = None
            gc.collect()
            for child in (owned / "rstorrent", owned / "libtorrent"):
                if child.is_dir():
                    shutil.rmtree(child)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ControlledFailure as error:
        print(f"controlled comparison failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
