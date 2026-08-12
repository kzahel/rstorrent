#!/usr/bin/env python3
"""Resumable direct-WAN cross-engine transport matrix for Tactical 142."""

from __future__ import annotations

import argparse
import ipaddress
import json
import re
import shlex
import shutil
import subprocess
import sys
import time
from pathlib import Path, PurePosixPath
from typing import Any

from public_compare_contract import comparison_profile, parse_metainfo, verify_payload
from wan_transport_fixture import Fixture, create_fixture
from wan_transport_mapping import add_mapping, remove_mapping
from wan_transport_matrix_contract import (
    DIRECTIONS,
    IMPLEMENTATIONS,
    SIZES_MIB,
    TRANSPORTS,
    CaseKey,
    Journal,
    assert_redacted,
    manifest,
    pending_cases,
    select_cases,
    summarize,
    validate_ssh_alias,
)
from wan_transport_roles_controlled import (
    ControlledRoleError,
    LocalBinaries,
    RoleProcess,
    build_binaries,
    run_leecher,
    start_seed,
    stop_seed,
)


ROOT = Path(__file__).resolve().parents[2]
INTEROP = Path(__file__).resolve().parent
SSH_OPTIONS = (
    "-o",
    "BatchMode=yes",
    "-o",
    "ConnectTimeout=15",
    "-o",
    "ConnectionAttempts=1",
    "-o",
    "ServerAliveInterval=5",
    "-o",
    "ServerAliveCountMax=6",
)
REMOTE_BASE = "$HOME/.local/share/rstorrent-oracles/tactical-142"
REMOTE_SOURCE = f"{REMOTE_BASE}/source"
REMOTE_FIXTURES = f"{REMOTE_BASE}/fixtures"
REMOTE_PYTHON = (
    "$HOME/.local/share/rstorrent-oracles/"
    "libtorrent-2.0.13-py313-aarch64/bin/python"
)
REMOTE_RUN_PATTERN = re.compile(r"^/tmp/rstorrent-wan-matrix\.[A-Za-z0-9]{6}$")
PROCESS_GRACE_SECONDS = 30.0
MAX_REMOTE_OUTPUT_BYTES = 1024 * 1024


class WanMatrixError(RuntimeError):
    pass


class WanMatrixCleanupError(WanMatrixError):
    pass


def _bounded_text(value: str) -> str:
    if len(value.encode()) > MAX_REMOTE_OUTPUT_BYTES:
        raise WanMatrixError("remote command output exceeded its bound")
    return value


def run_ssh(
    host: str,
    command: str,
    *,
    timeout_seconds: float = 60.0,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        ["ssh", *SSH_OPTIONS, host, command],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=timeout_seconds,
        check=False,
    )
    _bounded_text(completed.stdout)
    _bounded_text(completed.stderr)
    if check and completed.returncode != 0:
        raise WanMatrixError("bounded remote command failed")
    return completed


def _json_output(completed: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    try:
        value = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise WanMatrixError("remote helper emitted invalid JSON") from error
    if not isinstance(value, dict):
        raise WanMatrixError("remote helper emitted a non-object result")
    return value


def repository_revision() -> str:
    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=10,
        check=True,
    )
    if status.stdout:
        raise WanMatrixError("remote staging requires a clean committed worktree")
    return subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=10,
        check=True,
    ).stdout.strip()


def prepare_remote(host: str, sizes_mib: tuple[int, ...]) -> dict[str, Any]:
    revision = repository_revision()
    run_ssh(
        host,
        f'set -eu; mkdir -p "{REMOTE_SOURCE}" "{REMOTE_FIXTURES}"',
    )
    command = [
        "rsync",
        "-az",
        "--delete",
        "--exclude",
        ".git/",
        "--exclude",
        "target/",
        "--exclude",
        "reference/",
        "--exclude",
        "node_modules/",
        "--exclude",
        "tests/interop/.venv/",
        "--exclude",
        ".venv/",
        "--exclude",
        "*.pyc",
        "--exclude",
        "__pycache__/",
        "-e",
        "ssh " + " ".join(SSH_OPTIONS),
        "./",
        f"{host}:.local/share/rstorrent-oracles/tactical-142/source/",
    ]
    staged = subprocess.run(
        command,
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30 * 60,
        check=False,
    )
    if staged.returncode != 0:
        raise WanMatrixError("remote source staging failed")
    run_ssh(
        host,
        "set -eu; "
        f'printf "%s\\n" {shlex.quote(revision)} > "{REMOTE_BASE}/staged-revision"; '
        f'cd "{REMOTE_SOURCE}"; '
        '"$HOME/.cargo/bin/cargo" build --release '
        "-p rstorrent-engine --bin rstorrent-public-probe "
        "-p rstorrent-session --bin rstorrent-incoming-seed",
        timeout_seconds=2 * 60 * 60,
    )
    fixture_hashes: dict[int, str] = {}
    for size_mib in sizes_mib:
        completed = run_ssh(
            host,
            "set -eu; "
            f'cd "{REMOTE_SOURCE}/tests/interop"; '
            f'exec "{REMOTE_PYTHON}" wan_transport_fixture.py create '
            f'--root "{REMOTE_FIXTURES}/{size_mib}m" --size-mib {size_mib}',
            timeout_seconds={8: 5 * 60, 64: 10 * 60, 256: 30 * 60, 1024: 2 * 60 * 60}[
                size_mib
            ],
        )
        fixture = _json_output(completed)
        sha1 = fixture.get("sha1")
        if not isinstance(sha1, str) or not re.fullmatch(r"[0-9a-f]{40}", sha1):
            raise WanMatrixError("remote fixture omitted its exact SHA-1")
        fixture_hashes[size_mib] = sha1
    versions = run_ssh(
        host,
        "set -eu; "
        '"$HOME/.cargo/bin/rustc" --version; '
        f'"{REMOTE_PYTHON}" -c \'import libtorrent as lt; print(lt.version)\'; '
        f'cat "{REMOTE_BASE}/staged-revision"',
    ).stdout.splitlines()
    if len(versions) != 3 or versions[1] != "2.0.13.0" or versions[2] != revision:
        raise WanMatrixError("remote version or revision evidence is inconsistent")
    return {
        "revision": revision,
        "rust_version": versions[0],
        "libtorrent_version": versions[1],
        "fixture_sha1": fixture_hashes,
    }


def create_remote_run(host: str) -> str:
    remote_run = run_ssh(host, "mktemp -d /tmp/rstorrent-wan-matrix.XXXXXX").stdout.strip()
    if not REMOTE_RUN_PATTERN.fullmatch(remote_run):
        raise WanMatrixError("remote mktemp returned an ineligible directory")
    if PurePosixPath(remote_run).parent != PurePosixPath("/tmp"):
        raise WanMatrixError("remote run escaped its exact parent")
    return remote_run


def cleanup_remote_run(host: str, remote_run: str) -> None:
    if not REMOTE_RUN_PATTERN.fullmatch(remote_run):
        raise WanMatrixError("refusing to remove an ineligible remote run")
    run_ssh(host, f'rm -r -- "{remote_run}"')
    absent = run_ssh(host, f'test ! -e "{remote_run}"', check=False)
    if absent.returncode != 0:
        raise WanMatrixError("remote run survived exact cleanup")


def _remote_role(host: str, command: str, label: str, accepts_commands: bool) -> RoleProcess:
    return RoleProcess.start(
        ["ssh", *SSH_OPTIONS, host, command],
        label,
        accepts_commands=accepts_commands,
    )


def _remote_fixture_arguments(fixture: Fixture, timeout_seconds: float) -> str:
    root = f"{REMOTE_FIXTURES}/{fixture.size_mib}m"
    return (
        f'--metainfo "{root}/fixture.torrent" '
        f"--expected-sha1 {fixture.sha1} "
        f"--expected-bytes {fixture.payload_bytes} "
        f"--expected-piece-bytes {fixture.piece_bytes} "
        f"--expected-pieces {fixture.piece_count} "
        f"--timeout-seconds {timeout_seconds}"
    )


def start_remote_seed(
    host: str,
    implementation: str,
    transport: str,
    fixture: Fixture,
    remote_run: str,
    deadline: float,
) -> tuple[RoleProcess, int, dict[str, Any]]:
    fixture_root = f"{REMOTE_FIXTURES}/{fixture.size_mib}m"
    if implementation == "libtorrent":
        command = (
            f'exec "{REMOTE_PYTHON}" '
            f'"{REMOTE_SOURCE}/tests/interop/wan_transport_libtorrent_role.py" seed '
            f"{_remote_fixture_arguments(fixture, deadline - time.monotonic())} "
            f'--storage-root "{fixture_root}/seed" --network-scope wan '
            f"--transport {transport}"
        )
        process = _remote_role(host, command, "remote libtorrent seed", True)
        started = process.read_event(deadline)
        ready = process.read_event(deadline)
        if started.get("event") != "started" or ready.get("event") != "ready":
            raise WanMatrixError("remote libtorrent seed readiness failed")
        port = ready.get("listen_port")
    elif implementation == "rstorrent":
        selected = "--utp" if transport == "utp" else "--tcp-only"
        command = (
            f'exec "{REMOTE_SOURCE}/target/release/rstorrent-incoming-seed" '
            f'--profile-root "{remote_run}/seed-profile" '
            f'--storage-root "{fixture_root}/seed" '
            f'--metainfo "{fixture_root}/fixture.torrent" '
            f"--controlled-local-network --encryption disabled {selected}"
        )
        process = _remote_role(host, command, "remote RSTorrent seed", True)
        ready = process.read_event(deadline)
        field = "utp_listen" if transport == "utp" else "listen"
        endpoint = ready.get(field)
        if ready.get("event") != "ready" or not isinstance(endpoint, str):
            raise WanMatrixError("remote RSTorrent seed readiness failed")
        _, separator, encoded_port = endpoint.rpartition(":")
        port = int(encoded_port) if separator and encoded_port.isdigit() else None
    else:
        raise WanMatrixError("remote seed implementation is unknown")
    if not isinstance(port, int) or not 1 <= port <= 65_535:
        process.cleanup()
        raise WanMatrixError("remote seed reported an invalid listener port")
    return process, port, ready


def _rstorrent_timing(result: dict[str, Any]) -> dict[str, float]:
    milestones = result.get("milestones", {})
    first_payload = milestones.get("first_payload_byte")
    published = milestones.get("published")
    connected = milestones.get("first_connection") or milestones.get("torrent_admitted")
    if not all(isinstance(value, (int, float)) for value in (first_payload, published, connected)):
        raise WanMatrixError("RSTorrent leecher omitted timing milestones")
    active = float(published) - float(first_payload)
    connect = float(published) - float(connected)
    if active <= 0 or connect < active:
        raise WanMatrixError("RSTorrent leecher timing is invalid")
    return {
        "connect_to_complete_seconds": round(connect, 6),
        "first_payload_seconds": round(float(first_payload) - float(connected), 6),
        "active_payload_seconds": round(active, 6),
    }


def _rstorrent_transport_evidence(result: dict[str, Any]) -> dict[str, Any]:
    diagnostics = result.get("diagnostics", {})
    safe_diagnostics = {
        name: diagnostics.get(name)
        for name in (
            "outstanding_request_high_water",
            "payload_high_water",
            "storage_jobs_high_water",
            "storage_command_queue_high_water",
            "storage_completion_queue_high_water",
            "storage_write_queue_wait_max_micros",
            "storage_write_service_max_micros",
            "storage_write_batch_blocks_high_water",
            "storage_write_batch_bytes_high_water",
            "storage_hash_queue_wait_max_micros",
            "storage_hash_service_max_micros",
            "peer_methods",
        )
    }
    return {
        "effective_settings": result.get("effective_settings"),
        "utp": result.get("utp_evidence"),
        "udp": result.get("udp_evidence"),
        "diagnostics": safe_diagnostics,
    }


def run_remote_leecher(
    host: str,
    implementation: str,
    transport: str,
    fixture: Fixture,
    remote_run: str,
    peer_address: str,
    peer_port: int,
    deadline: float,
) -> dict[str, Any]:
    output_root = f"{remote_run}/leech"
    if implementation == "libtorrent":
        command = (
            f'exec "{REMOTE_PYTHON}" '
            f'"{REMOTE_SOURCE}/tests/interop/wan_transport_libtorrent_role.py" leech '
            f"{_remote_fixture_arguments(fixture, deadline - time.monotonic())} "
            f'--storage-root "{output_root}" --network-scope wan '
            f"--transport {transport} --peer-address {peer_address} --peer-port {peer_port}"
        )
        process = _remote_role(host, command, "remote libtorrent leecher", False)
        try:
            started = process.read_event(deadline)
            ready = process.read_event(deadline)
            complete = process.read_event(deadline)
            process.wait_success(deadline)
        finally:
            process.cleanup()
        if not (
            started.get("event") == "started"
            and ready.get("event") == "ready"
            and complete.get("event") == "complete"
            and complete.get("peer_high_water") == 1
        ):
            raise WanMatrixError("remote libtorrent leecher terminal contract failed")
        return {
            "timing": complete.get("timing"),
            "transport_evidence": complete.get("libtorrent_stats"),
        }
    if implementation != "rstorrent":
        raise WanMatrixError("remote leecher implementation is unknown")
    profile = comparison_profile(f"wan-{transport}")
    command = (
        f'exec "{REMOTE_SOURCE}/target/release/rstorrent-public-probe" '
        f'--metainfo "{REMOTE_FIXTURES}/{fixture.size_mib}m/fixture.torrent" '
        f"--expected-info-hash {fixture.info_hash} "
        f'--output "{output_root}" --profile {profile["name"]} '
        f'--profile-sha256 {profile["sha256"]} --target complete '
        f"--timeout-seconds {max(1, int(deadline - time.monotonic()) - 35)} "
        f"--cleanup-seconds 30 --wire-payload-ceiling-bytes {fixture.payload_bytes * 2} "
        f"--peer-hint {peer_address}:{peer_port}"
    )
    process = _remote_role(host, command, "remote RSTorrent leecher", False)
    try:
        result = process.read_event(deadline)
        process.wait_success(deadline)
    finally:
        process.cleanup()
    if not (
        result.get("outcome") == "milestone_reached"
        and result.get("cleanup_succeeded") is True
        and result.get("integrity_verified") is True
        and result.get("effective_settings") == profile["rstorrent"]
    ):
        raise WanMatrixError("remote RSTorrent leecher terminal contract failed")
    methods = result.get("diagnostics", {}).get("peer_methods", {})
    if methods.get("tcp_high_water") != (transport == "tcp") or methods.get(
        "utp_high_water"
    ) != (transport == "utp"):
        raise WanMatrixError("remote RSTorrent leecher transport was masked")
    return {
        "timing": _rstorrent_timing(result),
        "transport_evidence": _rstorrent_transport_evidence(result),
    }


def remote_mapping(host: str, action: str, port: int, protocol: str) -> dict[str, Any]:
    completed = run_ssh(
        host,
        f'exec "{REMOTE_PYTHON}" '
        f'"{REMOTE_SOURCE}/tests/interop/wan_transport_mapping.py" {action} '
        f"--port {port} --protocol {protocol}",
    )
    return _json_output(completed)


def verify_remote_route(host: str, address: str) -> str:
    parsed = ipaddress.ip_address(address)
    if not isinstance(parsed, ipaddress.IPv4Address) or not parsed.is_global:
        raise WanMatrixError("public endpoint is not eligible IPv4")
    completed = run_ssh(host, f"ip -4 route get {address}")
    match = re.search(r"\bdev\s+(\S+)", completed.stdout)
    if match is None:
        raise WanMatrixError("remote route inspection failed")
    interface = match.group(1)
    if interface.lower().startswith(("tailscale", "tun", "tap", "wg", "lo")):
        raise WanMatrixError("remote payload route uses an overlay")
    return "ordinary-internet"


def verify_remote_payload(host: str, remote_run: str, fixture: Fixture) -> None:
    if not REMOTE_RUN_PATTERN.fullmatch(remote_run):
        raise WanMatrixError("remote verification root is ineligible")
    completed = run_ssh(
        host,
        f'set -eu; sha1sum "{remote_run}/leech/payload.bin"; '
        f'stat -c %s "{remote_run}/leech/payload.bin"',
    )
    lines = completed.stdout.splitlines()
    if len(lines) != 2 or lines[0].split(maxsplit=1)[0] != fixture.sha1:
        raise WanMatrixError("independent remote payload hash failed")
    if lines[1] != str(fixture.payload_bytes):
        raise WanMatrixError("independent remote payload size failed")


def seed_transport_evidence(
    stopped: dict[str, Any], implementation: str, transport: str
) -> Any:
    if implementation == "libtorrent":
        return stopped.get("libtorrent_stats")
    if transport == "utp":
        return stopped.get("utp_before_shutdown")
    return {
        "connection_high_water": stopped.get("connection_high_water"),
        "established_high_water": stopped.get("established_high_water"),
    }


def execute_case(
    case: CaseKey,
    host: str,
    binaries: LocalBinaries,
    fixture: Fixture,
    work_root: Path,
) -> dict[str, Any]:
    case_root = work_root / "runs" / case.case_id
    case_root.mkdir(parents=True, exist_ok=False)
    remote_run: str | None = None
    deadline = time.monotonic() + case.timeout_seconds
    seed_process: RoleProcess | None = None
    mapping_attempted = False
    mapping_port: int | None = None
    mapping_protocol = "TCP" if case.transport == "tcp" else "UDP"
    cleanup_errors: list[str] = []
    stopped: dict[str, Any] | None = None
    try:
        remote_run = create_remote_run(host)
        if case.direction == "local-seed":
            seed_process, _, mapping_port, _ = start_seed(
                case.seed,
                case.transport,
                fixture,
                case_root,
                binaries,
                deadline,
                network_scope="wan",
            )
            mapping_attempted = True
            mapping = add_mapping(mapping_port, mapping_protocol)
        else:
            seed_process, mapping_port, _ = start_remote_seed(
                host,
                case.seed,
                case.transport,
                fixture,
                remote_run,
                deadline,
            )
            mapping_attempted = True
            mapping = remote_mapping(host, "map", mapping_port, mapping_protocol)
        peer_address = mapping.get("external_address")
        peer_port = mapping.get("external_port")
        if not isinstance(peer_address, str) or not isinstance(peer_port, int):
            raise WanMatrixError("mapping helper omitted the public endpoint")
        if case.direction == "local-seed":
            verify_remote_route(host, peer_address)
            leech = run_remote_leecher(
                host,
                case.leech,
                case.transport,
                fixture,
                remote_run,
                peer_address,
                peer_port,
                deadline,
            )
            verify_remote_payload(host, remote_run, fixture)
        else:
            from utp_rstorrent_wan import verify_direct_route

            verify_direct_route(host, peer_address)
            local_output = case_root / "leech"
            local = run_leecher(
                case.leech,
                case.transport,
                fixture,
                local_output,
                peer_address,
                peer_port,
                binaries,
                deadline,
                network_scope="wan",
            )
            descriptor = parse_metainfo(fixture.metainfo.read_bytes())
            verification = verify_payload(descriptor, local_output)
            if verification["logical_bytes"] != fixture.payload_bytes:
                raise WanMatrixError("independent local payload verification failed")
            leech = {
                "timing": local["timing"],
                "transport_evidence": (
                    local.get("libtorrent_stats")
                    if case.leech == "libtorrent"
                    else _rstorrent_transport_evidence(local["rstorrent"])
                ),
            }
        stopped = stop_seed(
            seed_process,
            case.seed,
            case.transport,
            fixture,
            deadline,
        )
        seed_process = None
        return {
            "timing": leech["timing"],
            "integrity": {
                "verified": True,
                "bytes": fixture.payload_bytes,
                "pieces": fixture.piece_count,
            },
            "transport_evidence": leech["transport_evidence"],
            "seed_transport_evidence": seed_transport_evidence(
                stopped, case.seed, case.transport
            ),
            "route_class": "ordinary-internet",
            "mapping": {
                "verified": True,
                "protocol": mapping_protocol,
                "lease_seconds": mapping.get("lease_seconds"),
            },
            "versions": {
                "libtorrent_version": "2.0.13.0",
                "revision": subprocess.run(
                    ["git", "rev-parse", "HEAD"],
                    cwd=ROOT,
                    capture_output=True,
                    text=True,
                    check=True,
                ).stdout.strip(),
            },
        }
    finally:
        if seed_process is not None:
            try:
                seed_process.cleanup()
            except BaseException:
                cleanup_errors.append("seed-process")
        if mapping_attempted and mapping_port is not None:
            try:
                if case.direction == "local-seed":
                    remove_mapping(mapping_port, mapping_protocol)
                else:
                    remote_mapping(host, "remove", mapping_port, mapping_protocol)
            except BaseException:
                cleanup_errors.append("mapping")
        if remote_run is not None:
            try:
                cleanup_remote_run(host, remote_run)
            except BaseException:
                cleanup_errors.append("remote-run")
        try:
            shutil.rmtree(case_root)
        except OSError:
            cleanup_errors.append("local-run")
        if cleanup_errors:
            raise WanMatrixCleanupError(
                "case cleanup failed for " + ",".join(sorted(cleanup_errors))
            )


def run_matrix(arguments: argparse.Namespace) -> dict[str, Any]:
    host = validate_ssh_alias(arguments.host)
    cases = select_cases(
        manifest(arguments.epoch, arguments.repetitions),
        sizes_mib=arguments.size_mib,
        directions=arguments.direction,
        seeds=arguments.seed,
        leeches=arguments.leech,
        transports=arguments.transport,
        case_ids=arguments.case_id,
    )
    if arguments.limit is not None:
        cases = cases[: arguments.limit]
    journal = Journal(arguments.journal)
    cases = pending_cases(cases, journal)
    if not arguments.allow_public_network:
        return {
            "schema_version": 1,
            "selected": len(cases),
            "case_ids": [case.case_id for case in cases],
            "execution": "disabled-without-explicit-flag",
        }
    sizes = tuple(sorted({case.size_mib for case in cases}))
    if arguments.prepare_remote:
        prepared = prepare_remote(host, sizes)
    else:
        prepared = {"status": "assumed-prepared"}
    binaries = build_binaries()
    fixtures = {
        size: create_fixture(arguments.work_root / "fixtures" / f"{size}m", size)
        for size in sizes
    }
    for case in cases:
        started = time.monotonic()
        try:
            result = execute_case(
                case,
                host,
                binaries,
                fixtures[case.size_mib],
                arguments.work_root,
            )
            record = {
                "schema_version": 1,
                "event": "case-terminal",
                "case": case.public_dict(),
                "status": "complete",
                "result": result,
                "cleanup": {"succeeded": True},
            }
        except WanMatrixCleanupError:
            raise
        except (ControlledRoleError, WanMatrixError, OSError, subprocess.SubprocessError) as error:
            record = {
                "schema_version": 1,
                "event": "case-terminal",
                "case": case.public_dict(),
                "status": "failed",
                "failure": {
                    "class": type(error).__name__,
                    "wall_seconds": round(time.monotonic() - started, 6),
                },
                "cleanup": {"succeeded": True},
            }
        assert_redacted(record, forbidden_strings=(host,))
        journal.append(record)
        print(
            json.dumps(
                {
                    "event": "wan-case-terminal",
                    "case_id": case.case_id,
                    "status": record["status"],
                },
                sort_keys=True,
            ),
            file=sys.stderr,
            flush=True,
        )
    return {
        "schema_version": 1,
        "scenario": "wan-transport-performance-matrix",
        "prepared": prepared,
        "summary": summarize(journal.load(repair_truncated_tail=True)),
    }


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True)
    parser.add_argument("--epoch", default="baseline-pre-fix")
    parser.add_argument("--journal", type=Path, required=True)
    parser.add_argument("--work-root", type=Path, required=True)
    parser.add_argument("--repetitions", type=int, choices=(1, 2, 3), default=1)
    parser.add_argument("--size-mib", type=int, choices=SIZES_MIB, action="append")
    parser.add_argument("--direction", choices=DIRECTIONS, action="append")
    parser.add_argument("--seed", choices=IMPLEMENTATIONS, action="append")
    parser.add_argument("--leech", choices=IMPLEMENTATIONS, action="append")
    parser.add_argument("--transport", choices=TRANSPORTS, action="append")
    parser.add_argument("--case-id", action="append")
    parser.add_argument("--limit", type=int)
    parser.add_argument("--prepare-remote", action="store_true")
    parser.add_argument("--allow-public-network", action="store_true")
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    try:
        if arguments.limit is not None and not 1 <= arguments.limit <= 192:
            raise WanMatrixError("case limit is outside its bound")
        print(json.dumps(run_matrix(arguments), indent=2, sort_keys=True))
        return 0
    except (ControlledRoleError, WanMatrixError, OSError, subprocess.SubprocessError) as error:
        print(f"WAN matrix failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
