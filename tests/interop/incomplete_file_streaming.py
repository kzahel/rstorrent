#!/usr/bin/env python3
"""Exercise incomplete HTTP ranges against the pinned libtorrent seed."""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import http.client
import json
import subprocess
import sys
import tempfile
import time
import urllib.request
from dataclasses import dataclass
from pathlib import Path

import libtorrent as lt

from first_verified_piece import ScenarioFailure, add_seed, create_session, wait_for_listener
from mse_peer_encryption import BT_HEADER, TcpProxy
from torrent_byte_intake import Gateway, repository_root, verify_reference


PIECE_SIZE = 32 * 1024
MEDIA_SIZE = 384 * 1024 + 333
UPLOAD_LIMIT = 96 * 1024
TIMEOUT_SECONDS = 30


@dataclass(frozen=True)
class Fixture:
    torrent_info: lt.torrent_info
    seed_root: Path
    info_hash: str
    media: bytes
    media_index: int
    media_offset: int


def deterministic_bytes(offset: int, length: int) -> bytes:
    return bytes(
        ((value * 73) ^ (value >> 3) ^ (value * value >> 11) ^ 0xA5) & 0xFF
        for value in range(offset, offset + length)
    )


def create_fixture(root: Path) -> Fixture:
    seed_root = root / "seed"
    content_root = seed_root / "stream-tree"
    content_root.mkdir(parents=True)
    specifications = (
        ("intro.bin", 7_777, False),
        ("media.bin", MEDIA_SIZE, False),
        (".pad/4096", 4_096, True),
        ("outro.bin", 9_111, False),
    )
    storage = lt.file_storage()
    offset = 0
    media = b""
    media_offset = 0
    for index, (name, length, padding) in enumerate(specifications):
        flags = lt.file_storage.flag_pad_file if padding else 0
        storage.add_file(f"stream-tree/{name}", length, flags)
        if not padding:
            payload = deterministic_bytes(offset, length)
            path = content_root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(payload)
            if index == 1:
                media = payload
                media_offset = offset
        offset += length
    creator = lt.create_torrent(
        storage,
        piece_size=PIECE_SIZE,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(seed_root))
    torrent_path = root / "streaming.torrent"
    torrent_path.write_bytes(bytes(lt.bencode(creator.generate())))
    torrent_info = lt.torrent_info(str(torrent_path))
    if torrent_info.piece_length() != PIECE_SIZE or not media:
        raise ScenarioFailure("streaming fixture geometry changed")
    return Fixture(
        torrent_info=torrent_info,
        seed_root=seed_root,
        info_hash=str(torrent_info.info_hashes().v1),
        media=media,
        media_index=1,
        media_offset=media_offset,
    )


def build_gateway(repository: Path) -> Path:
    completed = subprocess.run(
        ["cargo", "build", "-p", "rstorrent-gateway", "--bin", "rstorrent-gateway"],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to build streaming gateway\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    binary = repository / "target/debug/rstorrent-gateway"
    if not binary.is_file():
        raise ScenarioFailure("streaming gateway binary was not created")
    return binary


def media_url(gateway: Gateway, fixture: Fixture, proxy: TcpProxy) -> str:
    deadline = time.monotonic() + 10
    last_outcome: dict[str, object] | None = None
    while time.monotonic() < deadline:
        response = gateway.request(
            "POST",
            "/api/v1/media-urls",
            json.dumps(
                {"torrent_id": fixture.info_hash, "file_index": fixture.media_index},
                separators=(",", ":"),
            ).encode(),
            "application/json",
        )
        outcome = response["outcome"]
        last_outcome = outcome
        if outcome["type"] == "created":
            return str(outcome["url"])
        if outcome.get("reason") not in {
            "metadata_unavailable",
            "not_published",
            "storage_unavailable",
        }:
            raise ScenarioFailure(f"active media URL was rejected: {outcome}")
        time.sleep(0.05)
    snapshot = gateway.snapshot("streaming-url-timeout")
    torrent = next(
        (item for item in snapshot["torrents"] if item["torrent_id"] == fixture.info_hash),
        None,
    )
    raise ScenarioFailure(
        "active media URL did not become available: "
        f"outcome={last_outcome} torrent={torrent} proxy_connections={len(proxy.traces())}"
    )


def http_range(url: str, start: int, end: int) -> tuple[bytes, float]:
    request = urllib.request.Request(
        url,
        headers={"Range": f"bytes={start}-{end}"},
        method="GET",
    )
    started = time.monotonic()
    with urllib.request.urlopen(request, timeout=TIMEOUT_SECONDS) as response:
        body = response.read()
        if response.status != 206:
            raise ScenarioFailure(f"range {start}-{end} returned {response.status}")
        expected_content_range = f"bytes {start}-{end}/{MEDIA_SIZE}"
        if response.headers.get("Content-Range") != expected_content_range:
            raise ScenarioFailure(
                f"range {start}-{end} returned {response.headers.get('Content-Range')!r}"
            )
    return body, time.monotonic() - started


def head_range(url: str) -> None:
    request = urllib.request.Request(
        url,
        headers={"Range": "bytes=0-65535"},
        method="HEAD",
    )
    with urllib.request.urlopen(request, timeout=5) as response:
        if response.status != 206 or response.read() != b"":
            raise ScenarioFailure("range HEAD did not retain the empty 206 contract")


def wait_published(gateway: Gateway, fixture: Fixture) -> float:
    started = time.monotonic()
    deadline = started + TIMEOUT_SECONDS
    ordinal = 0
    torrent: dict[str, object] | None = None
    while time.monotonic() < deadline:
        snapshot = gateway.snapshot(f"streaming-published-{ordinal}")
        torrent = next(
            (
                item
                for item in snapshot["torrents"]
                if item["torrent_id"] == fixture.info_hash
            ),
            None,
        )
        if torrent is not None and (
            torrent["state"] == "complete"
            and torrent["storage_state"] == "published"
        ):
            return time.monotonic() - started
        ordinal += 1
        time.sleep(0.05)
    raise ScenarioFailure(f"streaming fixture did not publish: {torrent}")


def parse_requests(trace: bytes) -> list[tuple[int, int, int]]:
    if not trace.startswith(BT_HEADER):
        return []
    requests: list[tuple[int, int, int]] = []
    offset = 68
    while offset + 4 <= len(trace):
        length = int.from_bytes(trace[offset : offset + 4], "big")
        if offset + 4 + length > len(trace):
            break
        if length == 13 and trace[offset + 4] == 6:
            payload = trace[offset + 5 : offset + 17]
            requests.append(
                (
                    int.from_bytes(payload[0:4], "big"),
                    int.from_bytes(payload[4:8], "big"),
                    int.from_bytes(payload[8:12], "big"),
                )
            )
        offset += 4 + length
    return requests


def run(output: Path | None) -> dict[str, object]:
    repository = repository_root()
    reference = verify_reference(repository)
    gateway_binary = build_gateway(repository)
    with tempfile.TemporaryDirectory(prefix="rstorrent-incomplete-stream-") as temporary:
        root = Path(temporary)
        fixture = create_fixture(root)
        diagnostics: list[str] = []
        seed_session = create_session()
        seed_port = wait_for_listener(seed_session, diagnostics)
        seed = add_seed(seed_session, fixture.torrent_info, fixture.seed_root, diagnostics)
        seed.set_upload_limit(UPLOAD_LIMIT)
        proxy = TcpProxy(("127.0.0.1", seed_port))
        gateway: Gateway | None = None
        try:
            gateway = Gateway(
                gateway_binary,
                root / "profile",
                root / "downloads",
                "loopback_only",
                authentication="development-none",
                environment_overrides={"RSTORRENT_TEST_PEER_TRANSPORT": "tcp_only"},
            )
            magnet = (
                f"magnet:?xt=urn:btih:{fixture.info_hash}"
                f"&x.pe={proxy.endpoint[0]}:{proxy.endpoint[1]}"
            )
            gateway.command(
                "add-streaming-fixture",
                {
                    "type": "add_magnet",
                    "magnet": magnet,
                    "storage_root": "downloads",
                    "start_content": True,
                    "skip_files": [],
                },
            )
            url = media_url(gateway, fixture, proxy)
            head_range(url)
            baseline = sum(
                len(parse_requests(bytes(trace.client_to_upstream)))
                for trace in proxy.traces()
            )
            ranges = (
                (0, 65_535),
                (MEDIA_SIZE - 65_536, MEDIA_SIZE - 1),
                (131_072, 196_607),
                (163_840, 229_375),
            )
            with concurrent.futures.ThreadPoolExecutor(max_workers=len(ranges)) as executor:
                futures = [executor.submit(http_range, url, start, end) for start, end in ranges]
                observations = [future.result(timeout=TIMEOUT_SECONDS) for future in futures]
            for (start, end), (body, _) in zip(ranges, observations, strict=True):
                if body != fixture.media[start : end + 1]:
                    raise ScenarioFailure(f"range {start}-{end} returned inexact bytes")

            range_wire_requests = [
                request
                for trace in proxy.traces()
                for request in parse_requests(bytes(trace.client_to_upstream))
            ][baseline:]
            demanded_pieces = {
                piece
                for start, end in ranges
                for piece in range(
                    (fixture.media_offset + start) // PIECE_SIZE,
                    (fixture.media_offset + end) // PIECE_SIZE + 1,
                )
            }
            if not range_wire_requests:
                raise ScenarioFailure("player-shaped HTTP ranges produced no peer requests")
            if range_wire_requests[0][0] not in demanded_pieces:
                raise ScenarioFailure(
                    "the first post-range peer request did not target a demanded piece"
                )

            publication_seconds = wait_published(gateway, fixture)
            try:
                with urllib.request.urlopen(url, timeout=TIMEOUT_SECONDS) as response:
                    full = response.read()
                    if response.status != 200 or full != fixture.media:
                        raise ScenarioFailure("full streaming GET returned inexact bytes")
            except http.client.IncompleteRead as error:
                snapshot = gateway.snapshot("streaming-full-get-truncated")
                raise ScenarioFailure(
                    "full streaming GET was truncated: "
                    f"received={len(error.partial)} missing={error.expected} "
                    f"snapshot={snapshot}"
                ) from error

            all_requests = [
                request
                for trace in proxy.traces()
                for request in parse_requests(bytes(trace.client_to_upstream))
            ]
            demanded_requests = all_requests[baseline:]
            first_piece = fixture.media_offset // PIECE_SIZE
            last_piece = (fixture.media_offset + MEDIA_SIZE - 1) // PIECE_SIZE
            if not any(first_piece <= piece <= last_piece for piece, _, _ in demanded_requests):
                raise ScenarioFailure("player-shaped HTTP demand produced no media-piece request")
            result: dict[str, object] = {
                "reference": reference,
                "fixture": {
                    "files": 4,
                    "pieces": fixture.torrent_info.num_pieces(),
                    "media_bytes": MEDIA_SIZE,
                    "media_sha1": hashlib.sha1(fixture.media).hexdigest(),
                },
                "http": {
                    "ranges": [
                        {
                            "start": start,
                            "end": end,
                            "sha1": hashlib.sha1(body).hexdigest(),
                            "seconds": elapsed,
                        }
                        for (start, end), (body, elapsed) in zip(
                            ranges, observations, strict=True
                        )
                    ],
                    "full_sha1": hashlib.sha1(full).hexdigest(),
                    "publication_seconds_after_ranges": publication_seconds,
                },
                "peer_requests": {
                    "baseline_count": baseline,
                    "demand_count": len(demanded_requests),
                    "range_demand_count": len(range_wire_requests),
                    "range_piece_order": [
                        piece for piece, _, _ in range_wire_requests
                    ],
                    "demanded_pieces": sorted(demanded_pieces),
                    "piece_order": [piece for piece, _, _ in demanded_requests],
                    "first_media_piece": first_piece,
                    "last_media_piece": last_piece,
                },
            }
            if output is not None:
                output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
            return result
        finally:
            if gateway is not None:
                gateway.stop()
            proxy.close()
            seed_session.remove_torrent(seed)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    try:
        result = run(arguments.output)
    except (OSError, ScenarioFailure, subprocess.SubprocessError) as error:
        print(f"incomplete streaming failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
