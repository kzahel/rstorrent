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
    terminate_process,
)
from mse_peer_encryption import TcpProxy, assert_successful_wire_shape


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
    binary: Path, fixture: RuntimeFixture, *, require_mse: bool = False
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
    process: subprocess.Popen[str], expected_payload: int
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
    assert_resource_bounds(stopped, minimum_established=1)
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


def libtorrent_session(*, incoming: bool, require_mse: bool = False) -> lt.session:
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
) -> None:
    for index, source in enumerate(fixture.files):
        path = root / "root" / Path(
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
) -> tuple[list[str], str | None]:
    output_root.mkdir()
    session = libtorrent_session(incoming=False, require_mse=require_mse)
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    negotiated = None
    proxy = TcpProxy(parse_address(ready)) if require_mse else None
    try:
        parameters = lt.add_torrent_params()
        parameters.ti = fixture.torrent_info
        parameters.save_path = str(output_root)
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        handle = session.add_torrent(parameters)
        handle.connect_peer(proxy.endpoint if proxy is not None else parse_address(ready))
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
            raise ScenarioFailure(
                "libtorrent did not complete from the RSTorrent v2 seed\n"
                + "\n".join(diagnostics[-40:])
            )
        compare_libtorrent_files(fixture, output_root)
        if require_mse:
            assert_successful_wire_shape(proxy.traces(), "rc4")
            negotiated = "rc4"
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
) -> tuple[lt.session, lt.torrent_handle, list[str]]:
    session = libtorrent_session(incoming=True, require_mse=require_mse)
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
) -> tuple[dict[str, str], list[str], str | None]:
    output_root.mkdir()
    session, handle, diagnostics = start_libtorrent_seed(
        fixture, require_mse=require_mse
    )
    proxy = (
        TcpProxy(("127.0.0.1", session.listen_port())) if require_mse else None
    )
    try:
        command = [
            str(binary),
            "--metainfo",
            str(fixture.torrent_path),
            "--peer",
            (
                f"{proxy.endpoint[0]}:{proxy.endpoint[1]}"
                if proxy is not None
                else f"127.0.0.1:{session.listen_port()}"
            ),
            "--output",
            str(output_root),
            "--timeout-seconds",
            str(TRANSFER_TIMEOUT_SECONDS),
            "--max-buffered-payload-bytes",
            str(DEFAULT_PAYLOAD_ALLOWANCE),
        ]
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
        compare_files(fixture, output_root, skipped=skipped)
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
                )
                first_stop = stop_rstorrent_seed(seed_process, fixture.total_size)
            except BaseException:
                terminate_process(seed_process)
                raise

            restarted_process, restarted_ready = start_rstorrent_seed(
                seed_binary, fixture
            )
            try:
                restart_alerts, _ = leech_with_libtorrent(
                    fixture,
                    restarted_ready,
                    fixture.torrent_path.parent / "libtorrent-from-restarted-rstorrent",
                )
                restarted_stop = stop_rstorrent_seed(
                    restarted_process, fixture.total_size
                )
            except BaseException:
                terminate_process(restarted_process)
                raise

            full_fields, reverse_alerts, _ = leech_with_rstorrent(
                download_binary,
                fixture,
                fixture.torrent_path.parent / "rstorrent-from-libtorrent",
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
                )
            mse_evidence = None
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
                )
                mse_evidence = {
                    "libtorrent_initiates": incoming_method,
                    "rstorrent_initiates": outgoing_method,
                    "rstorrent_seed": seed_summary(mse_stop),
                    "rstorrent_leech": mse_fields,
                    "libtorrent_leech_alerts": mse_alerts,
                    "libtorrent_seed_alerts": outgoing_alerts,
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
            f"fixtures=2 roles=both restart=true mse=rc4-both-roles cleanup=true "
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
