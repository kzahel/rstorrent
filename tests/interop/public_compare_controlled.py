#!/usr/bin/env python3
"""Exercise both public-comparator adapters against one controlled local seed."""

from __future__ import annotations

import argparse
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


PIECE_LENGTH = 1024 * 1024


class ControlledFailure(RuntimeError):
    pass


def write_payload(path: Path, size: int, salt: int) -> str:
    path.parent.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha1()
    block = bytes((((offset * 73) ^ (offset >> 3) ^ salt) & 0xFF) for offset in range(1024 * 1024))
    with path.open("wb") as output:
        remaining = size
        while remaining:
            chunk = block[: min(remaining, len(block))]
            output.write(chunk)
            digest.update(chunk)
            remaining -= len(chunk)
    return digest.hexdigest()


def create_fixture(
    root: Path, payload_bytes: int
) -> tuple[lt.torrent_info, Path, Path, dict[str, str]]:
    seed = root / "seed"
    torrent_root = seed / "controlled-comparison"
    expected: dict[str, str] = {}
    file_storage = lt.file_storage()
    layouts = (
        ("video/clip.bin", payload_bytes - 64 * 1024),
        ("notes/readme.bin", 64 * 1024),
    )
    for index, (relative, size) in enumerate(layouts):
        path = torrent_root / relative
        expected[relative] = write_payload(path, size, index + 17)
        file_storage.add_file(f"controlled-comparison/{relative}", size)
    creator = lt.create_torrent(
        file_storage, piece_size=PIECE_LENGTH, flags=lt.create_torrent.v1_only
    )
    lt.set_piece_hashes(creator, str(seed))
    torrent_path = root / "controlled.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    return lt.torrent_info(str(torrent_path)), torrent_path, seed, expected


def verify_output(output_root: Path, expected: dict[str, str], *, includes_root: bool) -> None:
    for relative, digest in expected.items():
        base = output_root / "controlled-comparison" if includes_root else output_root
        path = base / relative
        if not path.is_file():
            raise ControlledFailure(f"missing published file {path}")
        actual = hashlib.sha1(path.read_bytes()).hexdigest()
        if actual != digest:
            raise ControlledFailure(f"published file hash mismatch for {relative}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--payload-mib", type=int, default=1)
    parser.add_argument(
        "--profiles", choices=("plaintext", "rc4", "both"), default="both"
    )
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 1 <= args.payload_mib <= 2048:
        parser.error("--payload-mib must be between 1 and 2048")
    return args


def main() -> int:
    args = parse_args()
    repository = repository_root()
    binary = build_probe(repository)
    session: lt.session | None = None
    seed_handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    with tempfile.TemporaryDirectory(prefix="rstorrent-controlled-compare-") as temporary:
        owned = Path(temporary)
        torrent_info, torrent_path, seed, expected = create_fixture(
            owned, args.payload_mib * 1024 * 1024
        )
        info_hash = str(torrent_info.info_hashes().v1)
        try:
            session = create_session()
            port = wait_for_listener(session, diagnostics)
            seed_handle = add_seed(session, torrent_info, seed, diagnostics)
            magnet = f"magnet:?xt=urn:btih:{info_hash}&x.pe=127.0.0.1:{port}"
            profiles = {
                "plaintext": ("matched-plain-30",),
                "rc4": ("matched-rc4-30",),
                "both": ("matched-plain-30", "matched-rc4-30"),
            }[args.profiles]
            runs = []
            failures = []
            for profile in profiles:
                rst_output = owned / profile / "rstorrent"
                lib_output = owned / profile / "libtorrent"
                wire_ceiling = args.payload_mib * 1024 * 1024 * 2
                rstorrent = run_rstorrent(
                    binary,
                    torrent_path,
                    profile,
                    "complete",
                    300,
                    10,
                    rst_output,
                    info_hash,
                    wire_ceiling,
                    peer_hints=[f"127.0.0.1:{port}"],
                )
                if rstorrent.get("outcome") == "milestone_reached":
                    verify_output(rst_output, expected, includes_root=True)
                libtorrent = run_libtorrent(
                    torrent_path,
                    info_hash,
                    profile,
                    "complete",
                    300,
                    lib_output,
                    10,
                    wire_ceiling,
                    peer_hints=[f"127.0.0.1:{port}"],
                )
                if libtorrent.get("outcome") == "milestone_reached":
                    verify_output(lib_output, expected, includes_root=True)
                classification = classify_pair(rstorrent, libtorrent)
                runs.append(
                    {
                        "profile": profile,
                        "order": ["rstorrent", "libtorrent"],
                        "classification": classification,
                        "implementations": {
                            "rstorrent": rstorrent,
                            "libtorrent": libtorrent,
                        },
                    }
                )
                if classification != "both_reached":
                    failures.append(f"{profile}: {classification}")
                profile_root = owned / profile
                if profile_root.is_dir():
                    shutil.rmtree(profile_root)
            report = {
                "schema_version": 2,
                "config": {
                    "profiles": list(profiles),
                    "target": "complete",
                    "controlled": True,
                    "info_hash": info_hash,
                    "payload_bytes": args.payload_mib * 1024 * 1024,
                },
                "runs": runs,
                "summaries": {
                    run["profile"]: summarize([run], "complete") for run in runs
                },
            }
            rendered = json.dumps(report, indent=2, sort_keys=True)
            if args.output:
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(rendered + "\n", encoding="utf-8")
            else:
                print(rendered)
            if failures:
                raise ControlledFailure("unexpected classifications: " + ", ".join(failures))
        finally:
            if session is not None and seed_handle is not None and seed_handle.is_valid():
                session.remove_torrent(seed_handle)
            if session is not None:
                session.pause()
            seed_handle = None
            session = None
            gc.collect()
            for profile in ("matched-plain-30", "matched-rc4-30"):
                child = owned / profile
                if child.is_dir():
                    shutil.rmtree(child)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ControlledFailure as error:
        print(f"controlled comparison failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
