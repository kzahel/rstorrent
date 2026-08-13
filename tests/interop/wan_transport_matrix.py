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
import tempfile
import time
from pathlib import Path, PurePosixPath
from typing import Any

from public_compare_contract import comparison_profile, parse_metainfo, verify_payload
from wan_transport_fixture import Fixture, create_fixture
from wan_transport_mapping import MappingError, add_mapping, remove_mapping
from wan_transport_linux_builder import (
    DEFAULT_MACHINE_CONTROL,
    LinuxArm64Build,
    LinuxBuilderError,
    build_linux_arm64_binaries,
    parse_glibc_version,
)
from wan_transport_matrix_contract import (
    DIRECTIONS,
    IMPLEMENTATIONS,
    SIZES_MIB,
    TRANSPORTS,
    CaseKey,
    Journal,
    MatrixContractError,
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
from wan_transport_resources import ResourceError, ResourceSampler


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
REMOTE_RSTORRENT_BINARIES = (
    "rstorrent-incoming-seed",
    "rstorrent-public-probe",
)
REMOTE_BUILD_MANIFEST = f"{REMOTE_BASE}/rstorrent-build-manifest.json"


class WanMatrixError(RuntimeError):
    pass


class WanMatrixCleanupError(WanMatrixError):
    pass


class WanMatrixRevisionError(WanMatrixError):
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


def require_repository_revision(expected: str) -> None:
    if repository_revision() != expected:
        raise WanMatrixRevisionError(
            "repository revision changed during matrix execution"
        )


def remote_runtime(host: str) -> dict[str, str]:
    facts = run_ssh(
        host,
        "set -eu; uname -m; getconf GNU_LIBC_VERSION",
    ).stdout.splitlines()
    if len(facts) != 2 or facts[0] != "aarch64":
        raise WanMatrixError("remote runtime is not Linux ARM64")
    parse_glibc_version(facts[1])
    return {"architecture": facts[0], "glibc": facts[1]}


def _read_remote_build_manifest(host: str) -> dict[str, Any]:
    completed = run_ssh(host, f'set -eu; cat "{REMOTE_BUILD_MANIFEST}"')
    try:
        value = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise WanMatrixRevisionError("remote build manifest is invalid") from error
    if not isinstance(value, dict):
        raise WanMatrixRevisionError("remote build manifest is not an object")
    return value


def remote_environment(
    host: str,
    revision: str,
    required_binaries: tuple[str, ...],
) -> dict[str, Any]:
    versions = run_ssh(
        host,
        "set -eu; "
        f'"{REMOTE_PYTHON}" -c \'import libtorrent as lt; print(lt.version)\'; '
        f'cat "{REMOTE_BASE}/staged-revision"',
    ).stdout.splitlines()
    if (
        len(versions) != 2
        or versions[0] != "2.0.13.0"
        or versions[1] != revision
    ):
        raise WanMatrixRevisionError(
            "remote environment is not staged at the matrix revision"
        )
    build_manifest = _read_remote_build_manifest(host)
    if (
        build_manifest.get("schema_version") != 1
        or build_manifest.get("revision") != revision
    ):
        raise WanMatrixRevisionError("remote build manifest revision is stale")
    runtime = build_manifest.get("runtime")
    builder = build_manifest.get("builder")
    binaries = build_manifest.get("binaries")
    if (
        not isinstance(runtime, dict)
        or not isinstance(builder, dict)
        or not isinstance(binaries, dict)
    ):
        raise WanMatrixRevisionError("remote build manifest is incomplete")
    observed_runtime = remote_runtime(host)
    if runtime != observed_runtime:
        raise WanMatrixRevisionError("remote runtime changed after artifact staging")
    for binary in required_binaries:
        artifact = binaries.get(binary)
        if not isinstance(artifact, dict):
            raise WanMatrixRevisionError("required remote binary is not staged")
        digest = artifact.get("sha256")
        size_bytes = artifact.get("size_bytes")
        if (
            not isinstance(digest, str)
            or re.fullmatch(r"[0-9a-f]{64}", digest) is None
        ):
            raise WanMatrixRevisionError("remote binary digest is invalid")
        if not isinstance(size_bytes, int) or size_bytes <= 0:
            raise WanMatrixRevisionError("remote binary size is invalid")
        remote_binary = f"{REMOTE_SOURCE}/target/release/{binary}"
        observed = run_ssh(
            host,
            f'set -eu; test "$(stat -c %s "{remote_binary}")" = {size_bytes}; '
            f'sha256sum "{remote_binary}"',
        ).stdout.split()[0]
        if observed != digest:
            raise WanMatrixRevisionError(
                "remote binary digest does not match its manifest"
            )
    return {
        "revision": revision,
        "rust_version": builder.get("rustc"),
        "libtorrent_version": versions[0],
        "builder": builder,
        "runtime": runtime,
        "binaries": binaries,
    }


def required_remote_rstorrent_binaries(cases: list[CaseKey]) -> tuple[str, ...]:
    required: set[str] = set()
    for case in cases:
        if case.direction == "remote-seed" and case.seed == "rstorrent":
            required.add("rstorrent-incoming-seed")
        if case.direction == "local-seed" and case.leech == "rstorrent":
            required.add("rstorrent-public-probe")
    return tuple(binary for binary in REMOTE_RSTORRENT_BINARIES if binary in required)


def prepare_remote(
    host: str,
    sizes_mib: tuple[int, ...],
    revision: str,
    remote_binaries: tuple[str, ...],
    builder_command: Path,
) -> dict[str, Any]:
    require_repository_revision(revision)
    run_ssh(
        host,
        f'set -eu; mkdir -p "{REMOTE_SOURCE}" "{REMOTE_FIXTURES}"; '
        f'rm -f "{REMOTE_BASE}/staged-revision" "{REMOTE_BUILD_MANIFEST}"',
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
    require_repository_revision(revision)
    runtime = remote_runtime(host)
    build: LinuxArm64Build | None = None
    with tempfile.TemporaryDirectory(prefix="rstorrent-wan-builder-") as temporary:
        staging = Path(temporary)
        if remote_binaries:
            build = build_linux_arm64_binaries(
                builder_command,
                revision,
                remote_binaries,
                staging,
                runtime["glibc"],
            )
            run_ssh(
                host,
                f'set -eu; mkdir -p "{REMOTE_SOURCE}/target/release" '
                f'"{REMOTE_BASE}/staged-binaries/{revision}"',
            )
            try:
                for binary, artifact in build.artifacts.items():
                    remote_staged = (
                        f"{REMOTE_BASE}/staged-binaries/{revision}/{binary}"
                    )
                    uploaded = subprocess.run(
                        [
                            "rsync",
                            "-az",
                            "-e",
                            "ssh " + " ".join(SSH_OPTIONS),
                            str(artifact.path),
                            f"{host}:.local/share/rstorrent-oracles/"
                            f"tactical-142/staged-binaries/{revision}/{binary}",
                        ],
                        cwd=ROOT,
                        capture_output=True,
                        text=True,
                        timeout=10 * 60,
                        check=False,
                    )
                    _bounded_text(uploaded.stdout)
                    _bounded_text(uploaded.stderr)
                    if uploaded.returncode != 0:
                        raise WanMatrixError("remote binary upload failed")
                    destination = f"{REMOTE_SOURCE}/target/release/{binary}"
                    run_ssh(
                        host,
                        "set -eu; "
                        f'test "$(stat -c %s "{remote_staged}")" = '
                        f"{artifact.size_bytes}; "
                        f'test "$(sha256sum "{remote_staged}" | cut -d " " -f 1)" = '
                        f"{artifact.sha256}; "
                        f'file "{remote_staged}" | '
                        'grep -q "ELF 64-bit.*ARM aarch64"; '
                        f'! ldd "{remote_staged}" | grep -q "not found"; '
                        f'install -m 0755 "{remote_staged}" "{destination}.tmp"; '
                        f'mv "{destination}.tmp" "{destination}"',
                    )
            finally:
                run_ssh(
                    host,
                    f'rm -rf -- "{REMOTE_BASE}/staged-binaries/{revision}"',
                )
        manifest_value = {
            "schema_version": 1,
            "revision": revision,
            "builder": (
                build.public_provenance()
                if build is not None
                else {"kind": "not-required", "target": "linux"}
            ),
            "runtime": runtime,
            "binaries": (
                {
                    binary: {
                        "sha256": artifact.sha256,
                        "size_bytes": artifact.size_bytes,
                    }
                    for binary, artifact in build.artifacts.items()
                }
                if build is not None
                else {}
            ),
        }
        local_manifest = staging / "rstorrent-build-manifest.json"
        local_manifest.write_text(
            json.dumps(manifest_value, sort_keys=True) + "\n", encoding="utf-8"
        )
        uploaded_manifest = subprocess.run(
            [
                "rsync",
                "-az",
                "-e",
                "ssh " + " ".join(SSH_OPTIONS),
                str(local_manifest),
                f"{host}:.local/share/rstorrent-oracles/"
                "tactical-142/rstorrent-build-manifest.json.tmp",
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=2 * 60,
            check=False,
        )
        _bounded_text(uploaded_manifest.stdout)
        _bounded_text(uploaded_manifest.stderr)
        if uploaded_manifest.returncode != 0:
            raise WanMatrixError("remote build manifest upload failed")
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
    run_ssh(
        host,
        "set -eu; "
        f'mv "{REMOTE_BUILD_MANIFEST}.tmp" "{REMOTE_BUILD_MANIFEST}"; '
        f'printf "%s\\n" {shlex.quote(revision)} > '
        f'"{REMOTE_BASE}/staged-revision.tmp"; '
        f'mv "{REMOTE_BASE}/staged-revision.tmp" "{REMOTE_BASE}/staged-revision"',
    )
    result = remote_environment(host, revision, remote_binaries)
    result["fixture_sha1"] = fixture_hashes
    result["rstorrent_binaries_built"] = list(remote_binaries)
    return result


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


def remote_role_pid(host: str, pid_file: str) -> int:
    if not REMOTE_RUN_PATTERN.fullmatch(str(PurePosixPath(pid_file).parent)):
        raise WanMatrixError("remote role PID path escaped its run")
    completed = run_ssh(
        host,
        "set -eu; attempts=0; "
        f'while test ! -s "{pid_file}"; do '
        "attempts=$((attempts + 1)); test $attempts -le 100; sleep 0.1; done; "
        f'cat "{pid_file}"',
        timeout_seconds=30,
    )
    try:
        pid = int(completed.stdout.strip())
    except ValueError as error:
        raise WanMatrixError("remote role PID is malformed") from error
    if pid <= 1:
        raise WanMatrixError("remote role PID is outside its bound")
    return pid


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
) -> tuple[RoleProcess, int, int, dict[str, Any]]:
    fixture_root = f"{REMOTE_FIXTURES}/{fixture.size_mib}m"
    owner_prefix = f'printf "%s\\n" $$ > "{remote_run}/seed.pid"; '
    if implementation == "libtorrent":
        command = (
            owner_prefix
            +
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
            owner_prefix
            +
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
    return process, port, remote_role_pid(host, f"{remote_run}/seed.pid"), ready


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
            "content_last_error",
            "peer_methods",
        )
    }
    safe_diagnostics["utility_timeline"] = diagnostics.get("utility_timeline")
    safe_diagnostics["utility_timeline_coalesced_samples"] = diagnostics.get(
        "utility_timeline_coalesced_samples"
    )
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
    owner_prefix = f'printf "%s\\n" $$ > "{remote_run}/leech.pid"; '
    if implementation == "libtorrent":
        command = (
            owner_prefix
            +
            f'exec "{REMOTE_PYTHON}" '
            f'"{REMOTE_SOURCE}/tests/interop/wan_transport_libtorrent_role.py" leech '
            f"{_remote_fixture_arguments(fixture, deadline - time.monotonic())} "
            f'--storage-root "{output_root}" --network-scope wan '
            f"--transport {transport} --peer-address {peer_address} --peer-port {peer_port}"
        )
        process = _remote_role(host, command, "remote libtorrent leecher", False)
        sampler = ResourceSampler.remote(
            host,
            remote_role_pid(host, f"{remote_run}/leech.pid"),
            min(43_260, max(60, int(deadline - time.monotonic()))),
        )
        resource_evidence = None
        try:
            started = process.read_event(deadline)
            ready = process.read_event(deadline)
            complete = process.read_event(deadline)
            process.wait_success(deadline)
        finally:
            process.cleanup()
            resource_evidence = sampler.finish()
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
            "resource_evidence": resource_evidence,
        }
    if implementation != "rstorrent":
        raise WanMatrixError("remote leecher implementation is unknown")
    profile = comparison_profile(f"wan-{transport}")
    command = (
        owner_prefix
        +
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
    sampler = ResourceSampler.remote(
        host,
        remote_role_pid(host, f"{remote_run}/leech.pid"),
        min(43_260, max(60, int(deadline - time.monotonic()))),
    )
    resource_evidence = None
    try:
        result = process.read_event(deadline)
        process.wait_success(deadline)
    finally:
        process.cleanup()
        resource_evidence = sampler.finish()
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
        "resource_evidence": resource_evidence,
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
        evidence = dict(stopped.get("utp_before_shutdown") or {})
        evidence["incoming_rejection_counts"] = stopped.get("rejection_counts", {})
        evidence["payload_bytes_sent"] = stopped.get("payload_bytes_sent")
        return evidence
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
    revision: str,
) -> dict[str, Any]:
    case_root = work_root / "runs" / case.case_id
    case_root.mkdir(parents=True, exist_ok=False)
    remote_run: str | None = None
    deadline = time.monotonic() + case.timeout_seconds
    seed_process: RoleProcess | None = None
    mapping_owned = False
    mapping_port: int | None = None
    mapping_protocol = "TCP" if case.transport == "tcp" else "UDP"
    cleanup_errors: list[str] = []
    stopped: dict[str, Any] | None = None
    seed_sampler: ResourceSampler | None = None
    seed_resource_evidence: dict[str, Any] | None = None
    primary_error: BaseException | None = None
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
            seed_sampler = ResourceSampler.local(
                seed_process.process.pid,
                min(43_260, max(60, int(deadline - time.monotonic()))),
            )
            mapping = add_mapping(mapping_port, mapping_protocol)
            mapping_owned = True
        else:
            seed_process, mapping_port, seed_pid, _ = start_remote_seed(
                host,
                case.seed,
                case.transport,
                fixture,
                remote_run,
                deadline,
            )
            seed_sampler = ResourceSampler.remote(
                host,
                seed_pid,
                min(43_260, max(60, int(deadline - time.monotonic()))),
            )
            mapping = remote_mapping(host, "map", mapping_port, mapping_protocol)
            mapping_owned = True
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
                collect_resources=True,
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
                "resource_evidence": local.get("resource_evidence"),
            }
        stopped = stop_seed(
            seed_process,
            case.seed,
            case.transport,
            fixture,
            deadline,
        )
        seed_process = None
        if seed_sampler is not None:
            seed_resource_evidence = seed_sampler.finish()
            seed_sampler = None
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
            "resources": {
                "seed": seed_resource_evidence,
                "leech": leech.get("resource_evidence"),
            },
            "route_class": "ordinary-internet",
            "mapping": {
                "verified": True,
                "protocol": mapping_protocol,
                "lease_seconds": mapping.get("lease_seconds"),
            },
            "versions": {
                "libtorrent_version": "2.0.13.0",
                "revision": revision,
            },
        }
    except BaseException as error:
        primary_error = error
        raise
    finally:
        if seed_process is not None:
            try:
                seed_process.cleanup()
            except BaseException:
                cleanup_errors.append("seed-process")
        if seed_sampler is not None:
            seed_sampler.cleanup()
        if mapping_owned and mapping_port is not None:
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
            detail = "case cleanup failed for " + ",".join(sorted(cleanup_errors))
            if primary_error is not None:
                detail = f"case failed with {type(primary_error).__name__}; " + detail
            raise WanMatrixCleanupError(detail) from primary_error


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
    if not cases:
        return {
            "schema_version": 1,
            "scenario": "wan-transport-performance-matrix",
            "prepared": {"status": "not-needed"},
            "summary": summarize(journal.load(repair_truncated_tail=True)),
        }
    revision = repository_revision()
    sizes = tuple(sorted({case.size_mib for case in cases}))
    if arguments.prepare_remote:
        prepared = prepare_remote(
            host,
            sizes,
            revision,
            required_remote_rstorrent_binaries(cases),
            arguments.builder_command,
        )
    else:
        prepared = remote_environment(
            host,
            revision,
            required_remote_rstorrent_binaries(cases),
        )
        prepared["status"] = "verified-existing"
    require_repository_revision(revision)
    binaries = build_binaries()
    require_repository_revision(revision)
    fixtures = {
        size: create_fixture(arguments.work_root / "fixtures" / f"{size}m", size)
        for size in sizes
    }
    require_repository_revision(revision)
    for case in cases:
        started = time.monotonic()
        fatal_error: WanMatrixRevisionError | None = None
        try:
            require_repository_revision(revision)
            result = execute_case(
                case,
                host,
                binaries,
                fixtures[case.size_mib],
                arguments.work_root,
                revision,
            )
            require_repository_revision(revision)
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
        except (
            ControlledRoleError,
            MappingError,
            ResourceError,
            WanMatrixError,
            OSError,
            subprocess.SubprocessError,
        ) as error:
            if isinstance(error, WanMatrixRevisionError):
                fatal_error = error
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
        if fatal_error is not None:
            raise fatal_error
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
    parser.add_argument(
        "--builder-command",
        type=Path,
        default=DEFAULT_MACHINE_CONTROL,
        help="machine-control executable owning the Linux ARM64 build VM",
    )
    parser.add_argument("--allow-public-network", action="store_true")
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    try:
        if arguments.limit is not None and not 1 <= arguments.limit <= 192:
            raise WanMatrixError("case limit is outside its bound")
        print(json.dumps(run_matrix(arguments), indent=2, sort_keys=True))
        return 0
    except (
        ControlledRoleError,
        LinuxBuilderError,
        MatrixContractError,
        ResourceError,
        WanMatrixError,
        OSError,
        subprocess.SubprocessError,
    ) as error:
        print(f"WAN matrix failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
