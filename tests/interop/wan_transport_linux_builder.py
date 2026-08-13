#!/usr/bin/env python3
"""Build exact-revision Linux ARM64 WAN role artifacts outside the Pi."""

from __future__ import annotations

import gzip
import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_MACHINE_CONTROL = ROOT.parent / "machine-control/bin/machine-control"
BUILDER_TARGET = "linux"
BUILDER_BASE = "/opt/rstorrent-builder"
BUILD_JOBS = 4
MINIMUM_FREE_KIB = 8 * 1024 * 1024
MAX_COMMAND_OUTPUT_BYTES = 1024 * 1024
REVISION_PATTERN = re.compile(r"^[0-9a-f]{40}$")
GLIBC_PATTERN = re.compile(r"^glibc ([0-9]+)\.([0-9]+)$")
REMOTE_BINARY_PACKAGES = {
    "rstorrent-incoming-seed": "rstorrent-session",
    "rstorrent-public-probe": "rstorrent-engine",
}


class LinuxBuilderError(RuntimeError):
    pass


@dataclass(frozen=True)
class LinuxArm64Artifact:
    path: Path
    sha256: str
    size_bytes: int


@dataclass(frozen=True)
class LinuxArm64Build:
    revision: str
    architecture: str
    glibc: str
    rustc: str
    cargo: str
    cmake: str
    build_jobs: int
    free_kib_before: int
    duration_seconds: float
    artifacts: dict[str, LinuxArm64Artifact]

    def public_provenance(self) -> dict[str, Any]:
        return {
            "kind": "machine-control-linux-arm64",
            "target": BUILDER_TARGET,
            "architecture": self.architecture,
            "glibc": self.glibc,
            "rustc": self.rustc,
            "cargo": self.cargo,
            "cmake": self.cmake,
            "build_jobs": self.build_jobs,
            "free_kib_before": self.free_kib_before,
            "duration_seconds": round(self.duration_seconds, 6),
        }


def parse_glibc_version(value: str) -> tuple[int, int]:
    matched = GLIBC_PATTERN.fullmatch(value)
    if matched is None:
        raise LinuxBuilderError("builder emitted an invalid glibc version")
    return int(matched.group(1)), int(matched.group(2))


def require_compatible_glibc(builder: str, runtime: str) -> None:
    if parse_glibc_version(builder) > parse_glibc_version(runtime):
        raise LinuxBuilderError("builder glibc is newer than the remote runtime")


def _bounded_output(completed: subprocess.CompletedProcess[str]) -> None:
    if (
        len(completed.stdout.encode()) > MAX_COMMAND_OUTPUT_BYTES
        or len(completed.stderr.encode()) > MAX_COMMAND_OUTPUT_BYTES
    ):
        raise LinuxBuilderError("builder command output exceeded its bound")


def _run(
    command: list[str],
    *,
    timeout_seconds: float,
    cwd: Path = ROOT,
    environment: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        command,
        cwd=cwd,
        env=environment,
        capture_output=True,
        text=True,
        timeout=timeout_seconds,
        check=False,
    )
    _bounded_output(completed)
    if completed.returncode != 0:
        raise LinuxBuilderError("bounded Linux ARM64 builder command failed")
    return completed


def _machine_control(
    executable: Path,
    arguments: list[str],
    *,
    timeout_seconds: float,
) -> subprocess.CompletedProcess[str]:
    if not executable.is_file() or not os.access(executable, os.X_OK):
        raise LinuxBuilderError("machine-control executable is unavailable")
    environment = os.environ.copy()
    environment["LINUXVM_EXEC_TIMEOUT"] = str(max(300, int(timeout_seconds)))
    return _run(
        [str(executable), "--target", BUILDER_TARGET, *arguments],
        timeout_seconds=timeout_seconds,
        environment=environment,
    )


def _testbed(
    executable: Path,
    arguments: list[str],
    *,
    timeout_seconds: float,
) -> subprocess.CompletedProcess[str]:
    return _machine_control(
        executable,
        ["testbed", "--", *arguments],
        timeout_seconds=timeout_seconds,
    )


def _builder_facts(executable: Path) -> dict[str, Any]:
    _machine_control(
        executable,
        ["target", "ensure-ready"],
        timeout_seconds=3 * 60,
    )
    doctor = _machine_control(
        executable,
        ["target", "doctor"],
        timeout_seconds=2 * 60,
    )
    try:
        readiness = json.loads(doctor.stdout)
    except json.JSONDecodeError as error:
        raise LinuxBuilderError("builder doctor emitted invalid JSON") from error
    if not isinstance(readiness, dict) or readiness.get("ready") is not True:
        raise LinuxBuilderError("Linux ARM64 builder is not ready")

    base = shlex.quote(BUILDER_BASE)
    completed = _testbed(
        executable,
        [
            "exec",
            "--",
            "sh",
            "-lc",
            "set -eu; "
            "uname -m; "
            "getconf GNU_LIBC_VERSION; "
            f'RUSTUP_HOME="{BUILDER_BASE}/rustup" '
            f'CARGO_HOME="{BUILDER_BASE}/cargo" '
            f'"{BUILDER_BASE}/cargo/bin/rustc" --version; '
            f'RUSTUP_HOME="{BUILDER_BASE}/rustup" '
            f'CARGO_HOME="{BUILDER_BASE}/cargo" '
            f'"{BUILDER_BASE}/cargo/bin/cargo" --version; '
            'cmake --version | sed -n "1p"; '
            f"df -Pk {base} | awk 'NR == 2 {{print $4}}'",
        ],
        timeout_seconds=2 * 60,
    )
    lines = completed.stdout.splitlines()
    if len(lines) != 6:
        raise LinuxBuilderError("builder facts were incomplete")
    try:
        free_kib = int(lines[5])
    except ValueError as error:
        raise LinuxBuilderError("builder free-space fact was invalid") from error
    facts = {
        "architecture": lines[0],
        "glibc": lines[1],
        "rustc": lines[2],
        "cargo": lines[3],
        "cmake": lines[4],
        "free_kib": free_kib,
    }
    if facts["architecture"] != "aarch64":
        raise LinuxBuilderError("builder is not Linux ARM64")
    if not facts["rustc"].startswith("rustc 1.97.0 "):
        raise LinuxBuilderError("builder does not have exact Rust 1.97.0")
    if not facts["cargo"].startswith("cargo 1.97.0 "):
        raise LinuxBuilderError("builder does not have exact Cargo 1.97.0")
    parse_glibc_version(facts["glibc"])
    if free_kib < MINIMUM_FREE_KIB:
        raise LinuxBuilderError("builder has less than 8 GiB free")
    return facts


def _validate_artifact(path: Path) -> LinuxArm64Artifact:
    if not path.is_file():
        raise LinuxBuilderError("builder artifact is missing")
    file_result = _run(["file", "-b", str(path)], timeout_seconds=10)
    description = file_result.stdout.strip()
    if "ELF 64-bit" not in description or "ARM aarch64" not in description:
        raise LinuxBuilderError("builder artifact is not a Linux ARM64 ELF")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return LinuxArm64Artifact(
        path=path,
        sha256=digest,
        size_bytes=path.stat().st_size,
    )


def build_linux_arm64_binaries(
    machine_control: Path,
    revision: str,
    binaries: tuple[str, ...],
    artifact_root: Path,
    runtime_glibc: str,
) -> LinuxArm64Build:
    if REVISION_PATTERN.fullmatch(revision) is None:
        raise LinuxBuilderError("builder revision is invalid")
    if not binaries:
        raise LinuxBuilderError("builder requires at least one binary")
    if any(binary not in REMOTE_BINARY_PACKAGES for binary in binaries):
        raise LinuxBuilderError("builder binary selection is invalid")
    facts = _builder_facts(machine_control)
    require_compatible_glibc(str(facts["glibc"]), runtime_glibc)

    artifact_root.mkdir(parents=True, exist_ok=True)
    archive = artifact_root / "source.tar.gz"
    _run(
        ["git", "archive", "--format=tar.gz", revision, "-o", str(archive)],
        timeout_seconds=2 * 60,
    )
    stage = f"{BUILDER_BASE}/stage/{revision}"
    quoted_stage = shlex.quote(stage)
    _testbed(
        machine_control,
        [
            "exec",
            "--",
            "sh",
            "-lc",
            f"rm -rf -- {quoted_stage}; mkdir -p {quoted_stage}/source",
        ],
        timeout_seconds=2 * 60,
    )
    started = time.monotonic()
    try:
        _testbed(
            machine_control,
            ["push", str(archive), f"{stage}/source.tar.gz"],
            timeout_seconds=5 * 60,
        )
        build_arguments = " ".join(
            f"-p {shlex.quote(REMOTE_BINARY_PACKAGES[binary])} "
            f"--bin {shlex.quote(binary)}"
            for binary in binaries
        )
        artifact_compression = "; ".join(
            f"gzip -1 -n -c \"$CARGO_TARGET_DIR/release/{binary}\" > "
            f"\"$stage/out/{binary}.gz\""
            for binary in binaries
        )
        artifact_files = " ".join(
            f'"$CARGO_TARGET_DIR/release/{binary}"' for binary in binaries
        )
        command = (
            "set -eu; "
            f"base={shlex.quote(BUILDER_BASE)}; "
            f"stage={quoted_stage}; "
            'exec 9>"$base/build.lock"; flock -w 30 9; '
            'tar -xzf "$stage/source.tar.gz" -C "$stage/source"; '
            'mkdir -p "$stage/out"; '
            'export RUSTUP_HOME="$base/rustup"; '
            'export CARGO_HOME="$base/cargo"; '
            'export CARGO_TARGET_DIR="$base/target"; '
            f"export CARGO_BUILD_JOBS={BUILD_JOBS}; "
            '"$CARGO_HOME/bin/cargo" build --locked --release --quiet '
            ' --manifest-path "$stage/source/Cargo.toml" '
            f"{build_arguments}; "
            f"{artifact_compression}; "
            f"file {artifact_files}"
        )
        build = _testbed(
            machine_control,
            ["exec", "--", "sh", "-lc", command],
            timeout_seconds=30 * 60,
        )
        if "ELF 64-bit" not in build.stdout or "ARM aarch64" not in build.stdout:
            raise LinuxBuilderError("guest rejected a builder artifact")
        for binary in binaries:
            local_artifact = artifact_root / binary
            compressed_artifact = artifact_root / f"{binary}.gz"
            _testbed(
                machine_control,
                [
                    "pull",
                    f"{stage}/out/{binary}.gz",
                    str(compressed_artifact),
                ],
                timeout_seconds=5 * 60,
            )
            with gzip.open(compressed_artifact, "rb") as source:
                with local_artifact.open("wb") as destination:
                    shutil.copyfileobj(source, destination)
        artifacts = {
            binary: _validate_artifact(artifact_root / binary) for binary in binaries
        }
    finally:
        _testbed(
            machine_control,
            ["exec", "--", "rm", "-rf", "--", stage],
            timeout_seconds=2 * 60,
        )
    return LinuxArm64Build(
        revision=revision,
        architecture=str(facts["architecture"]),
        glibc=str(facts["glibc"]),
        rustc=str(facts["rustc"]),
        cargo=str(facts["cargo"]),
        cmake=str(facts["cmake"]),
        build_jobs=BUILD_JOBS,
        free_kib_before=int(facts["free_kib"]),
        duration_seconds=time.monotonic() - started,
        artifacts=artifacts,
    )
