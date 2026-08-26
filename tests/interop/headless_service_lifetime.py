#!/usr/bin/env python3
"""Prove detached transfer and completed seeding through an installed headless service."""

from __future__ import annotations

import argparse
import gc
import hashlib
import ipaddress
import json
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from urllib.parse import urlparse

import libtorrent as lt

from first_verified_piece import ScenarioFailure, write_deterministic_payload


PAYLOAD_NAME = "headless-lifetime.bin"
PAYLOAD_BYTES = 8 * 1024 * 1024
PIECE_BYTES = 64 * 1024
UPLOAD_RATE_BYTES = 256 * 1024
STARTUP_SECONDS = 10
TRANSFER_SECONDS = 120


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gateway-url", required=True)
    parser.add_argument("--upstream", required=True)
    parser.add_argument("--username", required=True)
    parser.add_argument("--password-file", type=Path, required=True)
    parser.add_argument("--expected-version", required=True)
    parser.add_argument("--rstorrent-peer-address", required=True)
    parser.add_argument("--rstorrent-peer-port", type=int, required=True)
    parser.add_argument("--fixture-root", type=Path)
    parser.add_argument("--initial-peer-address")
    parser.add_argument("--initial-peer-port", type=int)
    values = parser.parse_args()
    gateway = urlparse(values.gateway_url)
    upstream = urlparse(values.upstream)
    if (
        gateway.scheme != "https"
        or gateway.hostname != "127.0.0.1"
        or gateway.port is None
        or gateway.path not in {"", "/"}
        or gateway.params
        or gateway.query
        or gateway.fragment
    ):
        parser.error("--gateway-url must be one exact loopback HTTPS origin")
    if (
        upstream.scheme != "http"
        or upstream.hostname is None
        or upstream.port is None
        or upstream.path not in {"", "/"}
        or upstream.params
        or upstream.query
        or upstream.fragment
        or not ipaddress.ip_address(upstream.hostname).is_private
    ):
        parser.error("--upstream must be one exact private HTTP origin")
    try:
        peer_address = ipaddress.ip_address(values.rstorrent_peer_address)
    except ValueError:
        parser.error("--rstorrent-peer-address must be a numeric address")
    if (
        not isinstance(peer_address, ipaddress.IPv4Address)
        or not peer_address.is_private
        or not 1 <= values.rstorrent_peer_port <= 65_535
    ):
        parser.error("RSTorrent peer endpoint must be private IPv4")
    if not values.password_file.is_absolute() or not values.password_file.is_file():
        parser.error("--password-file must be an absolute regular file")
    external_seed = (
        values.fixture_root,
        values.initial_peer_address,
        values.initial_peer_port,
    )
    if any(value is not None for value in external_seed) and not all(
        value is not None for value in external_seed
    ):
        parser.error("external seed mode requires fixture root and initial peer endpoint")
    if values.fixture_root is not None:
        if not values.fixture_root.is_absolute() or not values.fixture_root.is_dir():
            parser.error("--fixture-root must be an absolute directory")
        try:
            initial_address = ipaddress.ip_address(values.initial_peer_address)
        except ValueError:
            parser.error("--initial-peer-address must be numeric")
        if (
            not isinstance(initial_address, ipaddress.IPv4Address)
            or not (initial_address.is_loopback or initial_address.is_private)
            or not 1 <= values.initial_peer_port <= 65_535
        ):
            parser.error("initial peer endpoint must be loopback or private IPv4")
    return values


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def route_address(peer_address: str) -> str:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.connect((peer_address, 9))
        value = str(probe.getsockname()[0])
    address = ipaddress.ip_address(value)
    if not isinstance(address, ipaddress.IPv4Address) or not address.is_private:
        raise ScenarioFailure("controlled target has no private IPv4 source route")
    return value


def session_settings(local_address: str, *, upload_limit: int = 0) -> dict[str, object]:
    return {
        "listen_interfaces": f"{local_address}:0",
        "enable_dht": False,
        "enable_lsd": False,
        "enable_upnp": False,
        "enable_natpmp": False,
        "enable_incoming_utp": False,
        "enable_outgoing_utp": False,
        "enable_incoming_tcp": True,
        "enable_outgoing_tcp": True,
        "allow_multiple_connections_per_ip": False,
        "connections_limit": 1,
        "upload_rate_limit": upload_limit,
        "alert_queue_size": 512,
    }


def create_fixture(root: Path) -> tuple[Path, Path, str, lt.torrent_info]:
    seed_root = root / "seed"
    seed_root.mkdir()
    payload = seed_root / PAYLOAD_NAME
    expected_sha1 = write_deterministic_payload(payload, PAYLOAD_BYTES)
    files = lt.file_storage()
    files.add_file(PAYLOAD_NAME, PAYLOAD_BYTES)
    creator = lt.create_torrent(
        files,
        piece_size=PIECE_BYTES,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(seed_root))
    metainfo = root / "headless-lifetime.torrent"
    metainfo.write_bytes(bytes(lt.bencode(creator.generate())))
    torrent_info = lt.torrent_info(str(metainfo))
    if (
        torrent_info.total_size() != PAYLOAD_BYTES
        or torrent_info.piece_length() != PIECE_BYTES
        or torrent_info.num_pieces() != PAYLOAD_BYTES // PIECE_BYTES
        or torrent_info.num_files() != 1
        or any(True for _ in torrent_info.trackers())
    ):
        raise ScenarioFailure("headless lifetime fixture geometry changed")
    return metainfo, seed_root, expected_sha1, torrent_info


def load_fixture(root: Path) -> tuple[Path, Path, str, lt.torrent_info]:
    metainfo = root / "headless-lifetime.torrent"
    seed_root = root / "seed"
    payload = seed_root / PAYLOAD_NAME
    if not metainfo.is_file() or not payload.is_file():
        raise ScenarioFailure("prepared headless lifetime fixture is incomplete")
    torrent_info = lt.torrent_info(str(metainfo))
    expected_sha1 = hashlib.sha1(payload.read_bytes()).hexdigest()
    if (
        payload.stat().st_size != PAYLOAD_BYTES
        or torrent_info.total_size() != PAYLOAD_BYTES
        or torrent_info.piece_length() != PIECE_BYTES
        or torrent_info.num_pieces() != PAYLOAD_BYTES // PIECE_BYTES
        or torrent_info.num_files() != 1
        or any(True for _ in torrent_info.trackers())
    ):
        raise ScenarioFailure("prepared headless lifetime fixture geometry changed")
    return metainfo, seed_root, expected_sha1, torrent_info


def add_torrent(
    session: lt.session,
    torrent_info: lt.torrent_info,
    storage_root: Path,
    *,
    seed: bool,
) -> lt.torrent_handle:
    parameters = lt.add_torrent_params()
    parameters.ti = torrent_info
    parameters.save_path = str(storage_root)
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    if seed:
        parameters.flags |= lt.torrent_flags.seed_mode
    return session.add_torrent(parameters)


def wait_seed_ready(session: lt.session, handle: lt.torrent_handle) -> int:
    deadline = time.monotonic() + STARTUP_SECONDS
    while time.monotonic() < deadline:
        status = handle.status()
        if status.errc.value() != 0:
            raise ScenarioFailure(f"controlled seed failed: {status.errc.message()}")
        if status.is_seeding and session.is_listening() and session.listen_port() > 0:
            return int(session.listen_port())
        session.pop_alerts()
        time.sleep(0.05)
    raise ScenarioFailure("controlled seed did not become ready")


def run_presentation_transfer(
    arguments: argparse.Namespace,
    torrent_info: lt.torrent_info,
    seed_port: int,
    seed_address: str,
) -> subprocess.CompletedProcess[str]:
    info_hash = str(torrent_info.info_hashes().v1)
    magnet = f"magnet:?xt=urn:btih:{info_hash}&x.pe={seed_address}:{seed_port}"
    command = [
        "node",
        str(repository_root() / "scripts/verify-headless-service.mjs"),
        "--upstream",
        arguments.upstream,
        "--public-origin",
        arguments.gateway_url,
        "--username",
        arguments.username,
        "--password-file",
        str(arguments.password_file),
        "--expected-version",
        arguments.expected_version,
        "--magnet",
        magnet,
    ]
    completed = subprocess.run(
        command,
        cwd=repository_root(),
        capture_output=True,
        text=True,
        timeout=TRANSFER_SECONDS,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "headless presentation transfer failed\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    if "detached transfer, and fresh presentation" not in completed.stdout:
        raise ScenarioFailure("headless verifier omitted detached-transfer evidence")
    return completed


def verify_rstorrent_seed(
    arguments: argparse.Namespace,
    root: Path,
    torrent_info: lt.torrent_info,
    expected_sha1: str,
    local_address: str,
) -> int:
    output_root = root / "leecher"
    output_root.mkdir()
    session = lt.session(session_settings(local_address))
    handle = add_torrent(session, torrent_info, output_root, seed=False)
    try:
        handle.connect_peer(
            (arguments.rstorrent_peer_address, arguments.rstorrent_peer_port)
        )
        deadline = time.monotonic() + TRANSFER_SECONDS
        peer_high_water = 0
        while time.monotonic() < deadline:
            status = handle.status()
            if status.errc.value() != 0:
                raise ScenarioFailure(f"RSTorrent seed leech failed: {status.errc.message()}")
            peer_high_water = max(peer_high_water, len(handle.get_peer_info()))
            if status.is_seeding:
                break
            session.pop_alerts()
            time.sleep(0.05)
        else:
            raise ScenarioFailure("completed RSTorrent registration did not seed")
        payload = output_root / PAYLOAD_NAME
        actual_sha1 = hashlib.sha1(payload.read_bytes()).hexdigest()
        if actual_sha1 != expected_sha1:
            raise ScenarioFailure("RSTorrent seeded payload failed integrity verification")
        if peer_high_water != 1:
            raise ScenarioFailure("completed RSTorrent seed did not use one bounded peer")
        return int(handle.status().total_payload_download)
    finally:
        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def run(arguments: argparse.Namespace) -> None:
    if lt.version != "2.0.13.0":
        raise ScenarioFailure(f"libtorrent {lt.version} is not the pinned 2.0.13.0 oracle")
    owns_root = arguments.fixture_root is None
    root = (
        Path(tempfile.mkdtemp(prefix="rstorrent-headless-lifetime-"))
        if owns_root
        else arguments.fixture_root
    )
    seed_session: lt.session | None = None
    seed_handle: lt.torrent_handle | None = None
    try:
        _, seed_root, expected_sha1, torrent_info = (
            create_fixture(root) if owns_root else load_fixture(root)
        )
        local_address = route_address(arguments.rstorrent_peer_address)
        if owns_root:
            seed_session = lt.session(
                session_settings(local_address, upload_limit=UPLOAD_RATE_BYTES)
            )
            seed_handle = add_torrent(seed_session, torrent_info, seed_root, seed=True)
            seed_port = wait_seed_ready(seed_session, seed_handle)
            seed_address = local_address
        else:
            seed_address = arguments.initial_peer_address
            seed_port = arguments.initial_peer_port
        transfer_started = time.monotonic()
        completed = run_presentation_transfer(
            arguments, torrent_info, seed_port, seed_address
        )
        first_upload_bytes: int | None = None
        if seed_handle is not None and seed_session is not None:
            first_upload_bytes = int(seed_handle.status().total_payload_upload)
            if first_upload_bytes < PAYLOAD_BYTES:
                raise ScenarioFailure("controlled seed did not upload the complete payload")
            seed_session.remove_torrent(seed_handle)
            seed_session.pause()
            seed_handle = None
            seed_session = None
            gc.collect()

        seeded_bytes = verify_rstorrent_seed(
            arguments, root, torrent_info, expected_sha1, local_address
        )
        print(
            json.dumps(
                {
                    "scenario": "headless-service-lifetime",
                    "payload_bytes": PAYLOAD_BYTES,
                    "piece_count": torrent_info.num_pieces(),
                    "controlled_seed_upload_bytes": first_upload_bytes,
                    "initial_seed_source": "host" if owns_root else "external-controlled",
                    "rstorrent_seed_download_bytes": seeded_bytes,
                    "detached_presentation": True,
                    "completed_seed_registration": True,
                    "integrity": "sha1-match",
                    "elapsed_seconds": round(time.monotonic() - transfer_started, 3),
                    "proxy_verification": completed.stdout.strip(),
                    "cleanup": "joined",
                },
                sort_keys=True,
                separators=(",", ":"),
            )
        )
    finally:
        if seed_session is not None and seed_handle is not None and seed_handle.is_valid():
            seed_session.remove_torrent(seed_handle)
        if seed_session is not None:
            seed_session.pause()
        seed_handle = None
        seed_session = None
        gc.collect()
        if owns_root:
            shutil.rmtree(root, ignore_errors=True)


def main() -> int:
    try:
        run(parse_arguments())
        return 0
    except (ScenarioFailure, OSError, subprocess.SubprocessError, ValueError) as error:
        print(f"headless service lifetime failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
