#!/usr/bin/env python3
"""Prove the complete-source pure-v2 runtime against pinned libtorrent."""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import resource
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

import libtorrent as lt

from bep52_metainfo_oracle import BLOCK, SourceFile, pure_v2_fixture
from first_verified_piece import DEFAULT_PAYLOAD_ALLOWANCE, ScenarioFailure
from incoming_seeding import (
    assert_resource_bounds,
    integer_field,
    parse_address,
    read_json_line,
    seed_snapshot,
    terminate_process,
)
from mse_peer_encryption import TcpProxy, assert_successful_wire_shape
from dht_magnet import ControlledDhtRouter
from udp_tracker_magnet import OneShotUdpTracker
from utp_reference_oracle import stats_snapshot


TRANSFER_TIMEOUT_SECONDS = 30
PROCESS_TIMEOUT_SECONDS = 45


@dataclass(frozen=True)
class RuntimeFixture:
    name: str
    torrent_path: Path
    torrent_info: lt.torrent_info
    files: tuple[SourceFile, ...]
    storage_root: Path
    libtorrent_storage_root: Path
    profile_root: Path
    full_info_hash: str
    wire_info_hash: str

    @property
    def total_size(self) -> int:
        return sum(len(source.data) for source in self.files)


def deterministic_bytes(seed: int, length: int) -> bytes:
    return bytes(((seed + index) * 37 + index // 11) % 251 for index in range(length))


def make_fixture(
    root: Path,
    name: str,
    files: tuple[SourceFile, ...],
    piece_length: int,
) -> RuntimeFixture:
    fixture_root = root / name
    fixture_root.mkdir(parents=True)
    independent = pure_v2_fixture(name, list(files), piece_length)
    torrent_path = fixture_root / f"{name}.torrent"
    torrent_path.write_bytes(independent.torrent)
    torrent_info = lt.torrent_info(str(torrent_path))
    hashes = torrent_info.info_hashes()
    full_info_hash = str(hashes.v2)
    expected_full = independent.expected["v2_info_hash"]
    if full_info_hash != expected_full:
        raise ScenarioFailure(
            f"{name} libtorrent identity {full_info_hash} != {expected_full}"
        )
    if hashes.has_v1():
        raise ScenarioFailure(f"{name} unexpectedly has a v1 identity")
    if torrent_info.num_pieces() != independent.expected["logical_pieces"]:
        raise ScenarioFailure(f"{name} logical piece geometry changed")
    wire_info_hash = full_info_hash[:40]
    storage_root = fixture_root / "rstorrent-published"
    for source in files:
        path = storage_root / "root" / Path(
            *(component.decode("utf-8") for component in source.path)
        )
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(source.data)
    libtorrent_storage_root = fixture_root / "libtorrent-published"
    payload_index = 0
    storage = torrent_info.files()
    for index in range(storage.num_files()):
        if int(storage.file_flags(index)) & int(lt.file_storage.flag_pad_file):
            continue
        source = files[payload_index]
        path = libtorrent_storage_root / storage.file_path(index)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(source.data)
        payload_index += 1
    if payload_index != len(files):
        raise ScenarioFailure(f"{name} libtorrent payload file count changed")
    return RuntimeFixture(
        name=name,
        torrent_path=torrent_path,
        torrent_info=torrent_info,
        files=files,
        storage_root=storage_root,
        libtorrent_storage_root=libtorrent_storage_root,
        profile_root=fixture_root / "profile",
        full_info_hash=full_info_hash,
        wire_info_hash=wire_info_hash,
    )


def fixtures(root: Path) -> tuple[RuntimeFixture, ...]:
    single = (
        SourceFile((b"single.bin",), deterministic_bytes(3, BLOCK * 2 + 137)),
    )
    multi = (
        SourceFile((b"a-empty.bin",), b""),
        SourceFile((b"b-sub-block.bin",), deterministic_bytes(11, 137)),
        SourceFile((b"c-exact-block.bin",), deterministic_bytes(17, BLOCK)),
        SourceFile((b"d-exact-piece.bin",), deterministic_bytes(23, BLOCK * 4)),
        SourceFile((b"nested", b"e-multi-piece.bin"), deterministic_bytes(29, BLOCK * 4 + 731)),
    )
    return (
        make_fixture(root, "pure-v2-single", single, BLOCK),
        make_fixture(root, "pure-v2-aligned-multi", multi, BLOCK * 4),
    )


def build_binaries(repository: Path) -> tuple[Path, Path]:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "rstorrent-session",
            "--bin",
            "rstorrent-incoming-seed",
            "-p",
            "rstorrent-engine",
            "--bin",
            "rstorrent-download-piece",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=240,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to build pure-v2 runtime diagnostics\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    seed = repository / "target/debug/rstorrent-incoming-seed"
    download = repository / "target/debug/rstorrent-download-piece"
    if not seed.is_file() or not download.is_file():
        raise ScenarioFailure("pure-v2 runtime diagnostics were not created")
    return seed, download


def start_rstorrent_seed(
    binary: Path,
    fixture: RuntimeFixture,
    *,
    require_mse: bool = False,
    utp: bool = False,
) -> tuple[subprocess.Popen[str], dict[str, object]]:
    command = [
        str(binary),
        "--profile-root",
        str(fixture.profile_root),
        "--storage-root",
        str(fixture.storage_root),
        "--metainfo",
        str(fixture.torrent_path),
    ]
    if require_mse:
        command.extend(["--encryption", "required"])
    if utp:
        command.extend(["--utp", "--encryption", "disabled"])
    process = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        ready = read_json_line(process, 30)
        expected = {
            "event": "ready",
            "protocol": "v2",
            "info_hash": fixture.wire_info_hash,
            "full_info_hash": fixture.full_info_hash,
            "registrations": 1,
        }
        for field, value in expected.items():
            if ready.get(field) != value:
                raise ScenarioFailure(
                    f"{fixture.name} seed {field}={ready.get(field)!r}, expected {value!r}"
                )
        return process, ready
    except BaseException:
        terminate_process(process)
        raise


def stop_rstorrent_seed(
    process: subprocess.Popen[str], expected_payload: int, *, minimum_established: int = 1
) -> dict[str, object]:
    if process.stdin is None:
        raise ScenarioFailure("RSTorrent seed stdin is unavailable")
    process.stdin.write("\n")
    process.stdin.flush()
    stopped = read_json_line(process, 10)
    try:
        returncode = process.wait(timeout=10)
    except subprocess.TimeoutExpired as error:
        terminate_process(process)
        raise ScenarioFailure("RSTorrent seed did not join shutdown") from error
    stderr = process.stderr.read() if process.stderr is not None else ""
    if returncode != 0:
        raise ScenarioFailure(
            f"RSTorrent seed exited with {returncode}\nstderr:\n{stderr}"
        )
    if stopped.get("event") != "stopped":
        raise ScenarioFailure(f"unexpected seed stop observation: {stopped}")
    if integer_field(stopped, "payload_bytes_sent") < expected_payload:
        raise ScenarioFailure(
            "RSTorrent seed did not account for the complete payload: "
            f"{stopped}"
        )
    assert_resource_bounds(stopped, minimum_established=minimum_established)
    return stopped


def seed_summary(stopped: dict[str, object]) -> dict[str, object]:
    fields = (
        "payload_bytes_sent",
        "pending_high_water",
        "established_high_water",
        "connection_high_water",
        "queued_requests_high_water",
        "queued_bytes_high_water",
        "read_high_water",
        "read_bytes_high_water",
        "writer_send_buffer_high_water",
        "upload_slots_high_water",
    )
    return {field: stopped[field] for field in fields}


def libtorrent_session(
    *, incoming: bool, require_mse: bool = False, utp_only: bool = False
) -> lt.session:
    session = lt.session(
        {
            "listen_interfaces": "127.0.0.1:0",
            "enable_dht": False,
            "enable_lsd": False,
            "enable_upnp": False,
            "enable_natpmp": False,
            "enable_incoming_utp": False,
            "enable_outgoing_utp": False,
            "enable_incoming_tcp": incoming,
            "enable_outgoing_tcp": not incoming,
            "alert_queue_size": 1000,
            "alert_mask": int(lt.alert.category_t.all_categories),
        }
    )
    if require_mse:
        session.apply_settings(
            {
                "in_enc_policy": int(lt.enc_policy.pe_forced),
                "out_enc_policy": int(lt.enc_policy.pe_forced),
                "allowed_enc_level": int(lt.enc_level.pe_rc4),
                "prefer_rc4": True,
            }
        )
    if utp_only:
        session.apply_settings(
            {
                "enable_incoming_tcp": False,
                "enable_outgoing_tcp": False,
                "enable_incoming_utp": True,
                "enable_outgoing_utp": True,
            }
        )
    return session


def observed_encryption(handle: lt.torrent_handle) -> str | None:
    for peer in handle.get_peer_info():
        if peer.flags & lt.peer_info.rc4_encrypted:
            return "rc4"
        if peer.flags & lt.peer_info.plaintext_encrypted:
            return "plaintext_payload"
    return None


def compare_files(
    fixture: RuntimeFixture,
    root: Path,
    *,
    skipped: frozenset[int] = frozenset(),
    named_root: bool = True,
) -> None:
    content_root = root / "root" if named_root else root
    for index, source in enumerate(fixture.files):
        path = content_root / Path(
            *(component.decode("utf-8") for component in source.path)
        )
        if index in skipped:
            if path.exists():
                raise ScenarioFailure(f"skipped v2 file was materialized: {path}")
            continue
        if not path.is_file():
            raise ScenarioFailure(f"downloaded v2 file is missing: {path}")
        actual = path.read_bytes()
        if actual != source.data:
            raise ScenarioFailure(
                f"downloaded v2 file differs: {path} "
                f"sha256={hashlib.sha256(actual).hexdigest()}"
            )


def compare_libtorrent_files(fixture: RuntimeFixture, root: Path) -> None:
    payload_index = 0
    storage = fixture.torrent_info.files()
    for index in range(storage.num_files()):
        if int(storage.file_flags(index)) & int(lt.file_storage.flag_pad_file):
            continue
        path = root / storage.file_path(index)
        if not path.is_file():
            raise ScenarioFailure(f"downloaded libtorrent v2 file is missing: {path}")
        if path.read_bytes() != fixture.files[payload_index].data:
            raise ScenarioFailure(f"downloaded libtorrent v2 file differs: {path}")
        payload_index += 1
    if payload_index != len(fixture.files):
        raise ScenarioFailure("libtorrent output payload file count changed")


def leech_with_libtorrent(
    fixture: RuntimeFixture,
    ready: dict[str, object],
    output_root: Path,
    *,
    require_mse: bool = False,
    utp_only: bool = False,
    magnet_only: bool = False,
) -> tuple[list[str], str | None]:
    output_root.mkdir()
    session = libtorrent_session(
        incoming=False, require_mse=require_mse, utp_only=utp_only
    )
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    negotiated = None
    proxy = None
    try:
        peer_address = (
            parse_address({"listen": ready.get("utp_listen")})
            if utp_only
            else parse_address(ready)
        )
        if require_mse or magnet_only:
            proxy = TcpProxy(peer_address)
        endpoint = proxy.endpoint if proxy is not None else peer_address
        parameters = (
            lt.parse_magnet_uri(
                "magnet:?xt=urn:btmh:1220"
                f"{fixture.full_info_hash}&x.pe={endpoint[0]}:{endpoint[1]}"
            )
            if magnet_only
            else lt.add_torrent_params()
        )
        if not magnet_only:
            parameters.ti = fixture.torrent_info
        parameters.save_path = str(output_root)
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        handle = session.add_torrent(parameters)
        # The pinned Python binding retains the magnet identity but does not
        # schedule x.pe itself. Treat the parsed hint as the sole discovery
        # input and inject that exact endpoint through libtorrent's public API.
        handle.connect_peer(endpoint)
        deadline = time.monotonic() + TRANSFER_TIMEOUT_SECONDS
        while time.monotonic() < deadline:
            diagnostics.extend(alert.message() for alert in session.pop_alerts())
            negotiated = negotiated or observed_encryption(handle)
            status = handle.status()
            if status.errc.value() != 0:
                raise ScenarioFailure(
                    f"libtorrent v2 leech failed: {status.errc.message()}"
                )
            if status.is_seeding:
                break
            time.sleep(0.02)
        else:
            if proxy is not None:
                for index, trace in enumerate(proxy.traces()):
                    diagnostics.append(
                        f"proxy[{index}] client_to_upstream="
                        f"{bytes(trace.client_to_upstream)!r}"
                    )
                    diagnostics.append(
                        f"proxy[{index}] upstream_to_client="
                        f"{bytes(trace.upstream_to_client)!r}"
                    )
            raise ScenarioFailure(
                "libtorrent did not complete from the RSTorrent v2 seed\n"
                + "\n".join(diagnostics[-40:])
            )
        compare_libtorrent_files(fixture, output_root)
        if require_mse:
            assert_successful_wire_shape(proxy.traces(), "rc4")
            negotiated = "rc4"
        if utp_only:
            validate_libtorrent_utp(session, diagnostics)
        return diagnostics[-20:], negotiated
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        if proxy is not None:
            proxy.close()
        handle = None
        session = None
        gc.collect()


def start_libtorrent_seed(
    fixture: RuntimeFixture,
    *,
    require_mse: bool = False,
    utp_only: bool = False,
) -> tuple[lt.session, lt.torrent_handle, list[str]]:
    session = libtorrent_session(
        incoming=True, require_mse=require_mse, utp_only=utp_only
    )
    parameters = lt.add_torrent_params()
    parameters.ti = fixture.torrent_info
    parameters.save_path = str(fixture.libtorrent_storage_root)
    parameters.flags &= ~lt.torrent_flags.paused
    parameters.flags &= ~lt.torrent_flags.auto_managed
    handle = session.add_torrent(parameters)
    diagnostics: list[str] = []
    deadline = time.monotonic() + TRANSFER_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        status = handle.status()
        if status.errc.value() != 0:
            raise ScenarioFailure(
                f"libtorrent v2 seed failed: {status.errc.message()}"
            )
        if status.is_seeding and session.listen_port() > 0:
            return session, handle, diagnostics
        time.sleep(0.02)
    raise ScenarioFailure(
        "libtorrent did not validate the pure-v2 seed\n"
        + "\n".join(diagnostics[-40:])
    )


def leech_with_rstorrent(
    binary: Path,
    fixture: RuntimeFixture,
    output_root: Path,
    *,
    skipped: frozenset[int] = frozenset(),
    require_mse: bool = False,
    magnet_only: bool = False,
) -> tuple[dict[str, str], list[str], str | None]:
    session, handle, diagnostics = start_libtorrent_seed(
        fixture, require_mse=require_mse
    )
    proxy = (
        TcpProxy(("127.0.0.1", session.listen_port())) if require_mse else None
    )
    try:
        endpoint = (
            f"{proxy.endpoint[0]}:{proxy.endpoint[1]}"
            if proxy is not None
            else f"127.0.0.1:{session.listen_port()}"
        )
        source = ["--metainfo", str(fixture.torrent_path), "--peer", endpoint]
        if magnet_only:
            selected = [
                index for index in range(len(fixture.files)) if index not in skipped
            ]
            select_only = ",".join(str(index) for index in selected)
            source = [
                "--magnet",
                "magnet:?xt=urn:btmh:1220"
                f"{fixture.full_info_hash}&x.pe={endpoint}&so={select_only}",
            ]
        command = [
            str(binary),
            *source,
            "--output",
            str(output_root),
            "--timeout-seconds",
            str(TRANSFER_TIMEOUT_SECONDS),
            "--max-buffered-payload-bytes",
            str(DEFAULT_PAYLOAD_ALLOWANCE),
        ]
        if not magnet_only:
            for index in sorted(skipped):
                command.extend(["--skip-file", str(index)])
        if require_mse:
            command.extend(["--encryption", "required"])
        process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        negotiated = None
        deadline = time.monotonic() + PROCESS_TIMEOUT_SECONDS
        while process.poll() is None:
            negotiated = negotiated or observed_encryption(handle)
            if time.monotonic() >= deadline:
                process.kill()
                raise ScenarioFailure("RSTorrent v2 leech exceeded its process deadline")
            time.sleep(0.002)
        stdout, stderr = process.communicate()
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        if process.returncode != 0:
            raise ScenarioFailure(
                f"RSTorrent v2 leech exited with {process.returncode}\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}\n"
                + "\n".join(diagnostics[-40:])
            )
        fields = {
            key: value
            for token in stdout.split()
            if "=" in token
            for key, value in [token.split("=", 1)]
        }
        if fields.get("info_hash") != fixture.wire_info_hash:
            raise ScenarioFailure(
                f"RSTorrent v2 leech reported the wrong wire identity: {fields}"
            )
        if int(fields.get("payload_high_water", "-1")) > DEFAULT_PAYLOAD_ALLOWANCE:
            raise ScenarioFailure(f"RSTorrent exceeded its payload limit: {fields}")
        part_path = fields.get("part_path")
        if (
            fields.get("part_written_bytes") != "0"
            or part_path is None
            or (part_path != "-" and Path(part_path).exists())
        ):
            raise ScenarioFailure(f"pure-v2 leech used a part file: {fields}")
        if require_mse:
            assert_successful_wire_shape(proxy.traces(), "rc4")
            negotiated = "rc4"
        compare_files(fixture, output_root, skipped=skipped, named_root=False)
        return fields, diagnostics[-20:], negotiated
    finally:
        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        if proxy is not None:
            proxy.close()
        handle = None
        session = None
        gc.collect()


def discovery_fixture(
    fixture: RuntimeFixture,
    root: Path,
    *,
    tracker_url: str | None = None,
) -> RuntimeFixture:
    root.mkdir()
    outer = lt.bdecode(fixture.torrent_path.read_bytes())
    if not isinstance(outer, dict):
        raise ScenarioFailure("pure-v2 discovery fixture is not a dictionary")
    if tracker_url is not None:
        outer[b"announce"] = tracker_url.encode("utf-8")
    source = bytes(lt.bencode(outer))
    torrent_path = root / "discovery.torrent"
    torrent_path.write_bytes(source)
    torrent_info = lt.torrent_info(str(torrent_path))
    full_info_hash = str(torrent_info.info_hashes().v2)
    if full_info_hash != fixture.full_info_hash:
        raise ScenarioFailure("outer discovery fields changed the pure-v2 identity")
    storage_root = root / "rstorrent-download"
    storage_root.mkdir()
    return RuntimeFixture(
        name=root.name,
        torrent_path=torrent_path,
        torrent_info=torrent_info,
        files=fixture.files,
        storage_root=storage_root,
        libtorrent_storage_root=fixture.libtorrent_storage_root,
        profile_root=root / "profile",
        full_info_hash=full_info_hash,
        wire_info_hash=fixture.wire_info_hash,
    )


def start_rstorrent_download(
    binary: Path,
    fixture: RuntimeFixture,
    *,
    dht_bootstrap: tuple[str, int] | None = None,
) -> tuple[subprocess.Popen[str], dict[str, object]]:
    command = [
        str(binary),
        "--profile-root",
        str(fixture.profile_root),
        "--storage-root",
        str(fixture.storage_root),
        "--metainfo",
        str(fixture.torrent_path),
        "--download-fixture",
        "--utp",
        "--encryption",
        "disabled",
        "--download-rate-limit",
        str(BLOCK),
    ]
    if dht_bootstrap is not None:
        command.extend(
            ["--dht-bootstrap", f"{dht_bootstrap[0]}:{dht_bootstrap[1]}"]
        )
    process = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        ready = read_json_line(process, 30)
        if (
            ready.get("event") != "ready"
            or ready.get("protocol") != "v2"
            or ready.get("full_info_hash") != fixture.full_info_hash
            or ready.get("registrations") != 1
        ):
            raise ScenarioFailure(f"unexpected pure-v2 download readiness: {ready}")
        return process, ready
    except BaseException:
        terminate_process(process)
        raise


def wait_rstorrent_download(
    process: subprocess.Popen[str], fixture: RuntimeFixture
) -> tuple[dict[str, object], dict[str, object]]:
    deadline = time.monotonic() + TRANSFER_TIMEOUT_SECONDS
    last: dict[str, object] = {}
    peer_rows: dict[str, dict[str, object]] = {}
    while time.monotonic() < deadline:
        if process.poll() is not None:
            stderr = process.stderr.read() if process.stderr is not None else ""
            raise ScenarioFailure(
                f"pure-v2 discovery download stopped early\nstderr:\n{stderr}"
            )
        last = seed_snapshot(process)
        peers = last.get("peers")
        if isinstance(peers, dict):
            rows = peers.get("peers")
            if isinstance(rows, list):
                for row in rows:
                    if isinstance(row, dict) and isinstance(
                        row.get("connection_id"), str
                    ):
                        peer_rows[row["connection_id"]] = row.copy()
        summary = last.get("summary")
        torrent = summary.get("torrent") if isinstance(summary, dict) else None
        if (
            isinstance(torrent, dict)
            and torrent.get("state") == "complete"
            and torrent.get("storage_state") == "published"
        ):
            compare_files(fixture, fixture.storage_root)
            matching = [
                row
                for row in peer_rows.values()
                if row.get("direction") == "outgoing"
                and row.get("transport") == "utp"
            ]
            if not matching:
                raise ScenarioFailure(
                    f"pure-v2 discovery download did not observe outgoing uTP: {peer_rows}"
                )
            utp = last.get("utp")
            if (
                not isinstance(utp, dict)
                or not isinstance(utp.get("connections_started"), int)
                or utp["connections_started"] < 1
                or not isinstance(utp.get("datagrams_sent"), int)
                or utp["datagrams_sent"] < 1
                or utp.get("worker_panics") != 0
            ):
                raise ScenarioFailure(f"invalid pure-v2 uTP evidence: {utp}")
            return last, {
                "directions": sorted(
                    {
                        f"{row.get('direction')}:{row.get('transport')}"
                        for row in peer_rows.values()
                    }
                ),
                "connections_started": utp["connections_started"],
                "datagrams_sent": utp["datagrams_sent"],
                "connection_high_water": utp["connection_high_water"],
            }
        time.sleep(0.02)
    raise ScenarioFailure(f"pure-v2 discovery download timed out: {last}")


def stop_rstorrent_download(process: subprocess.Popen[str]) -> dict[str, object]:
    return stop_rstorrent_seed(process, 0, minimum_established=0)


def validate_libtorrent_utp(
    session: lt.session, diagnostics: list[str]
) -> dict[str, int]:
    stats = stats_snapshot(
        session, diagnostics, time.monotonic() + TRANSFER_TIMEOUT_SECONDS
    )
    if stats["peer.num_tcp_peers"] != 0:
        raise ScenarioFailure("pure-v2 discovery used a libtorrent TCP peer")
    if stats["utp.utp_packets_in"] < 1 or stats["utp.utp_packets_out"] < 1:
        raise ScenarioFailure("pure-v2 discovery lacked bidirectional uTP packets")
    return {
        "tcp_peers": stats["peer.num_tcp_peers"],
        "utp_packets_in": stats["utp.utp_packets_in"],
        "utp_packets_out": stats["utp.utp_packets_out"],
    }


def accepted_utp_evidence(stopped: dict[str, object]) -> dict[str, int]:
    utp = stopped.get("utp_before_shutdown")
    if (
        not isinstance(utp, dict)
        or not isinstance(utp.get("connections_started"), int)
        or utp["connections_started"] < 1
        or not isinstance(utp.get("datagrams_sent"), int)
        or utp["datagrams_sent"] < 1
        or utp.get("worker_panics") != 0
    ):
        raise ScenarioFailure(f"invalid accepted pure-v2 uTP evidence: {utp}")
    udp = stopped.get("udp_before_shutdown")
    if (
        not isinstance(udp, dict)
        or not isinstance(udp.get("utp_datagrams_classified"), int)
        or udp["utp_datagrams_classified"] < 1
    ):
        raise ScenarioFailure(f"accepted pure-v2 uTP bypassed session UDP: {udp}")
    return {
        "connections_started": utp["connections_started"],
        "datagrams_sent": utp["datagrams_sent"],
        "connection_high_water": utp["connection_high_water"],
        "classified_datagrams": udp["utp_datagrams_classified"],
    }


def tracker_utp_download(
    binary: Path, fixture: RuntimeFixture
) -> dict[str, object]:
    session, handle, diagnostics = start_libtorrent_seed(fixture, utp_only=True)
    tracker = OneShotUdpTracker(
        fixture.wire_info_hash,
        session.listen_port(),
        expected_left=fixture.total_size,
        expected_peer_id=None,
        expected_listen_port=None,
    )
    discovered = discovery_fixture(
        fixture,
        fixture.torrent_path.parent / "tracker-utp",
        tracker_url=f"udp://127.0.0.1:{tracker.port}/announce",
    )
    process: subprocess.Popen[str] | None = None
    tracker.start()
    try:
        process, ready = start_rstorrent_download(binary, discovered)
        _, application = wait_rstorrent_download(process, discovered)
        tracker.join()
        oracle = validate_libtorrent_utp(session, diagnostics)
        stopped = stop_rstorrent_download(process)
        process = None
        return {
            "source": "udp_tracker",
            "wire_info_hash": discovered.wire_info_hash,
            "announces": tracker.requests,
            "announced_port": tracker.observed_listen_port,
            "application": application,
            "oracle": oracle,
            "ready_udp": ready.get("utp_listen"),
            "cleanup": stopped.get("event") == "stopped",
        }
    finally:
        if process is not None:
            terminate_process(process)
        tracker.close()
        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def dht_utp_download(binary: Path, fixture: RuntimeFixture) -> dict[str, object]:
    session, handle, diagnostics = start_libtorrent_seed(fixture, utp_only=True)
    router = ControlledDhtRouter(fixture.wire_info_hash, session.listen_port())
    discovered = discovery_fixture(
        fixture, fixture.torrent_path.parent / "dht-utp"
    )
    process: subprocess.Popen[str] | None = None
    router.start()
    try:
        process, ready = start_rstorrent_download(
            binary, discovered, dht_bootstrap=("127.0.0.1", router.port)
        )
        _, application = wait_rstorrent_download(process, discovered)
        router.join()
        oracle = validate_libtorrent_utp(session, diagnostics)
        stopped = stop_rstorrent_download(process)
        process = None
        return {
            "source": "dht",
            "wire_info_hash": discovered.wire_info_hash,
            "find_node_queries": router.find_node_queries,
            "get_peers_queries": router.get_peers_queries,
            "application": application,
            "oracle": oracle,
            "ready_udp": ready.get("utp_listen"),
            "cleanup": stopped.get("event") == "stopped",
        }
    finally:
        if process is not None:
            terminate_process(process)
        router.close()
        if handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def run(repository: Path, *, no_build: bool) -> None:
    run_root = Path(tempfile.mkdtemp(prefix="rstorrent-pure-v2-runtime-"))
    failure: BaseException | None = None
    try:
        seed_binary = repository / "target/debug/rstorrent-incoming-seed"
        download_binary = repository / "target/debug/rstorrent-download-piece"
        if not no_build:
            seed_binary, download_binary = build_binaries(repository)
        if not seed_binary.is_file() or not download_binary.is_file():
            raise ScenarioFailure("pure-v2 runtime binaries are unavailable")
        for fixture in fixtures(run_root):
            seed_process, seed_ready = start_rstorrent_seed(seed_binary, fixture)
            try:
                first_alerts, _ = leech_with_libtorrent(
                    fixture,
                    seed_ready,
                    fixture.torrent_path.parent / "libtorrent-from-rstorrent",
                    magnet_only=True,
                )
                first_stop = stop_rstorrent_seed(seed_process, fixture.total_size)
            except BaseException as error:
                snapshot = seed_snapshot(seed_process)
                terminate_process(seed_process)
                raise ScenarioFailure(
                    f"{error}\nRSTorrent seed snapshot:\n"
                    f"{json.dumps(snapshot, indent=2, sort_keys=True)}"
                ) from error

            restarted_process, restarted_ready = start_rstorrent_seed(
                seed_binary, fixture
            )
            try:
                restart_alerts, _ = leech_with_libtorrent(
                    fixture,
                    restarted_ready,
                    fixture.torrent_path.parent / "libtorrent-from-restarted-rstorrent",
                    magnet_only=True,
                )
                restarted_stop = stop_rstorrent_seed(
                    restarted_process, fixture.total_size
                )
            except BaseException as error:
                snapshot = seed_snapshot(restarted_process)
                terminate_process(restarted_process)
                raise ScenarioFailure(
                    f"{error}\nRSTorrent restarted seed snapshot:\n"
                    f"{json.dumps(snapshot, indent=2, sort_keys=True)}"
                ) from error

            full_fields, reverse_alerts, _ = leech_with_rstorrent(
                download_binary,
                fixture,
                fixture.torrent_path.parent / "rstorrent-from-libtorrent",
                magnet_only=True,
            )
            skipped = (
                frozenset({1})
                if fixture.name == "pure-v2-aligned-multi"
                else frozenset()
            )
            selective_fields = None
            if skipped:
                selective_fields, _, _ = leech_with_rstorrent(
                    download_binary,
                    fixture,
                    fixture.torrent_path.parent
                    / "rstorrent-selective-from-libtorrent",
                    skipped=skipped,
                    magnet_only=True,
                )
            mse_evidence = None
            discovery_evidence = None
            if fixture.name == "pure-v2-single":
                mse_process, mse_ready = start_rstorrent_seed(
                    seed_binary, fixture, require_mse=True
                )
                try:
                    mse_alerts, incoming_method = leech_with_libtorrent(
                        fixture,
                        mse_ready,
                        fixture.torrent_path.parent / "libtorrent-mse-from-rstorrent",
                        require_mse=True,
                        magnet_only=True,
                    )
                    mse_stop = stop_rstorrent_seed(mse_process, fixture.total_size)
                except BaseException:
                    terminate_process(mse_process)
                    raise
                mse_fields, outgoing_alerts, outgoing_method = leech_with_rstorrent(
                    download_binary,
                    fixture,
                    fixture.torrent_path.parent / "rstorrent-mse-from-libtorrent",
                    require_mse=True,
                    magnet_only=True,
                )
                mse_evidence = {
                    "libtorrent_initiates": incoming_method,
                    "rstorrent_initiates": outgoing_method,
                    "rstorrent_seed": seed_summary(mse_stop),
                    "rstorrent_leech": mse_fields,
                    "libtorrent_leech_alerts": mse_alerts,
                    "libtorrent_seed_alerts": outgoing_alerts,
                }
                discovery_evidence = {
                    "tracker_utp": tracker_utp_download(seed_binary, fixture),
                    "dht_utp": dht_utp_download(seed_binary, fixture),
                }
                utp_process, utp_ready = start_rstorrent_seed(
                    seed_binary, fixture, utp=True
                )
                try:
                    utp_alerts, _ = leech_with_libtorrent(
                        fixture,
                        utp_ready,
                        fixture.torrent_path.parent / "libtorrent-utp-from-rstorrent",
                        utp_only=True,
                    )
                    utp_stop = stop_rstorrent_seed(utp_process, fixture.total_size)
                except BaseException:
                    terminate_process(utp_process)
                    raise
                discovery_evidence["accepted_utp"] = {
                    "application": accepted_utp_evidence(utp_stop),
                    "libtorrent_alerts": utp_alerts,
                    "ready_udp": utp_ready.get("utp_listen"),
                    "cleanup": utp_stop.get("event") == "stopped",
                }
            print(
                json.dumps(
                    {
                        "fixture": fixture.name,
                        "protocol": "v2",
                        "full_info_hash": fixture.full_info_hash,
                        "wire_info_hash": fixture.wire_info_hash,
                        "bytes": fixture.total_size,
                        "pieces": fixture.torrent_info.num_pieces(),
                        "files": len(fixture.files),
                        "rstorrent_seed": seed_summary(first_stop),
                        "restarted_rstorrent_seed": seed_summary(restarted_stop),
                        "rstorrent_leech": full_fields,
                        "rstorrent_selective_leech": selective_fields,
                        "mse": mse_evidence,
                        "discovery": discovery_evidence,
                        "libtorrent_seed_alerts": reverse_alerts,
                        "libtorrent_leech_alerts": first_alerts,
                        "libtorrent_restart_alerts": restart_alerts,
                    },
                    sort_keys=True,
                )
            )
        rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        child_rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        if sys.platform != "darwin":
            rss *= 1024
            child_rss *= 1024
        print(
            "interop=pure-v2-runtime "
            f"libtorrent_binding={lt.__version__} libtorrent_native={lt.version} "
            f"fixtures=2 roles=both restart=true tracker=true dht=true "
            f"utp=both-roles mse=rc4-both-roles cleanup=true "
            f"oracle_rss_bytes={rss} child_peak_rss_bytes={child_rss}"
        )
    except BaseException as error:
        failure = error
        raise
    finally:
        try:
            shutil.rmtree(run_root)
        except OSError as cleanup_error:
            if failure is None:
                raise
            print(f"cleanup failed: {cleanup_error}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-build", action="store_true")
    arguments = parser.parse_args()
    try:
        run(Path(__file__).resolve().parents[2], no_build=arguments.no_build)
    except ScenarioFailure as error:
        print(f"pure-v2 runtime failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
