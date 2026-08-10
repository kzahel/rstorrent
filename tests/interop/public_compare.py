#!/usr/bin/env python3
"""Run bounded, alternating RSTorrent/libtorrent public-download comparisons."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Callable
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

from public_compare_contract import (
    CANONICAL_PROFILES,
    MAX_NETWORK_BYTES,
    MAX_OWNER_TIMEOUT_SECONDS,
    MAX_PAIRS,
    MAX_REPORT_BYTES,
    PROFILE_ALIASES,
    REPORT_SCHEMA_VERSION,
    ContractError,
    comparison_profile,
    distribution,
    invocation_network_budget,
    load_catalog_document,
    normalize_profile,
    parse_metainfo,
    required_free_space,
    validate_output_ancestry,
    validate_retained_report,
    verify_payload,
)


SCHEMA_VERSION = REPORT_SCHEMA_VERSION
TARGETS = (
    "metadata",
    "first-piece",
    "10-percent",
    "50-percent",
    "90-percent",
    "95-percent",
    "99-percent",
    "complete",
)
PROFILES = CANONICAL_PROFILES + tuple(PROFILE_ALIASES)
OWNERS = ("both", "rstorrent", "libtorrent")
INPUT_MODES = ("metainfo", "magnet")
SUITES = ("quick", "smoke", "standard", "large", "product", "encryption", "breadth")
MILESTONE_KEYS = {
    "metadata": "metadata_verified",
    "first-piece": "first_piece_verified",
    "10-percent": "10_percent_verified",
    "50-percent": "50_percent_verified",
    "90-percent": "90_percent_verified",
    "95-percent": "95_percent_verified",
    "99-percent": "99_percent_verified",
    "complete": "published",
}
PROCESS_SAMPLE_SECONDS = 0.1
OUTER_GRACE_SECONDS = 10
MAX_REDIRECTS = 5


class HarnessError(RuntimeError):
    pass


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def default_catalog_path() -> Path:
    return repository_root() / "tests" / "live" / "torrents.json"


def load_catalog(path: Path) -> dict[str, Any]:
    try:
        return load_catalog_document(path)
    except ContractError as error:
        raise HarnessError(str(error)) from error


def select_torrent(catalog: dict[str, Any], slug: str) -> dict[str, Any]:
    for entry in catalog["torrents"]:
        if entry["slug"] == slug:
            return entry
    choices = ", ".join(entry["slug"] for entry in catalog["torrents"])
    raise HarnessError(f"unknown torrent {slug!r}; choose one of: {choices}")


def select_role(catalog: dict[str, Any], role: str) -> dict[str, Any]:
    candidates = [entry for entry in catalog["torrents"] if role in entry["roles"]]
    if len(candidates) != 1:
        raise HarnessError(f"catalog role {role!r} must select exactly one torrent")
    return candidates[0]


def scenario_magnets(entry: dict[str, Any], profile: str) -> tuple[str, str]:
    profile = normalize_profile(profile)
    source = entry.get("magnet")
    if not isinstance(source, str):
        source = f"magnet:?xt=urn:btih:{entry['info_hash']}"
    if profile == "product-default":
        return source, source
    split = urlsplit(source)
    retained: list[tuple[str, str]] = []
    for key, value in parse_qsl(split.query, keep_blank_values=True):
        if key in ("xt", "dn"):
            retained.append((key, value))
        elif (
            profile in ("matched-plain-30", "matched-rc4-30")
            and key == "tr"
            and value.lower().startswith(("udp://", "http://", "https://"))
        ):
            retained.append((key, value))
    magnet = urlunsplit((split.scheme, split.netloc, split.path, urlencode(retained), ""))
    return magnet, magnet


def implementation_order(ordinal: int) -> list[str]:
    return (
        ["rstorrent", "libtorrent"]
        if ordinal % 4 in (0, 3)
        else ["libtorrent", "rstorrent"]
    )


def selected_implementations(ordinal: int, owner: str) -> list[str]:
    return implementation_order(ordinal) if owner == "both" else [owner]


def classify_pair(rstorrent: dict[str, Any], libtorrent: dict[str, Any]) -> str:
    outcomes = (rstorrent.get("outcome"), libtorrent.get("outcome"))
    if "harness_error" in outcomes:
        return "harness_error"
    reached = tuple(value == "milestone_reached" for value in outcomes)
    if reached == (True, True):
        return "both_reached"
    if reached[1]:
        return "reference_only"
    if reached[0]:
        return "rstorrent_only"
    return "both_incomplete"


def classify_owner(result: dict[str, Any]) -> str:
    if result.get("outcome") == "harness_error":
        return "harness_error"
    return "owner_reached" if result.get("outcome") == "milestone_reached" else "owner_incomplete"


def milestone_seconds(result: dict[str, Any], target: str) -> float | None:
    value = result.get("milestones", {}).get(MILESTONE_KEYS[target])
    return float(value) if isinstance(value, (int, float)) and value >= 0 else None


def active_transfer_seconds(result: dict[str, Any], target: str) -> float | None:
    started = result.get("milestones", {}).get("first_payload_byte")
    finished = milestone_seconds(result, target)
    if (
        not isinstance(started, (int, float))
        or started < 0
        or finished is None
        or finished < started
    ):
        return None
    return finished - float(started)


def summarize(
    runs: list[dict[str, Any]], target: str, owner: str = "both"
) -> dict[str, Any]:
    if owner != "both":
        names = ("owner_reached", "owner_incomplete", "harness_error")
        classifications = {name: sum(run["classification"] == name for run in runs) for name in names}
        times = [
            seconds
            for run in runs
            if run["classification"] == "owner_reached"
            for seconds in [milestone_seconds(run["implementations"][owner], target)]
            if seconds is not None
        ]
        return {
            "attempts": len(runs),
            "owner": owner,
            "classifications": classifications,
            "milestone_samples": len(times),
            "owner_seconds": distribution(times),
        }
    names = (
        "both_reached",
        "reference_only",
        "rstorrent_only",
        "both_incomplete",
        "harness_error",
    )
    classifications = {name: sum(run["classification"] == name for run in runs) for name in names}
    ratios: list[float] = []
    active_ratios: list[float] = []
    rst_times: list[float] = []
    lib_times: list[float] = []
    rst_active_times: list[float] = []
    lib_active_times: list[float] = []
    rst_discovery_times: list[float] = []
    lib_discovery_times: list[float] = []
    for run in runs:
        if run["classification"] != "both_reached":
            continue
        rst = milestone_seconds(run["implementations"]["rstorrent"], target)
        lib = milestone_seconds(run["implementations"]["libtorrent"], target)
        if rst is None or lib is None or lib <= 0:
            continue
        rst_times.append(rst)
        lib_times.append(lib)
        ratios.append(rst / lib)
        rst_active = active_transfer_seconds(run["implementations"]["rstorrent"], target)
        lib_active = active_transfer_seconds(run["implementations"]["libtorrent"], target)
        if rst_active is not None and lib_active is not None and lib_active > 0:
            rst_active_times.append(rst_active)
            lib_active_times.append(lib_active)
            active_ratios.append(rst_active / lib_active)
        rst_discovery = run["implementations"]["rstorrent"].get("milestones", {}).get(
            "first_payload_byte"
        )
        lib_discovery = run["implementations"]["libtorrent"].get("milestones", {}).get(
            "first_payload_byte"
        )
        if isinstance(rst_discovery, (int, float)) and rst_discovery >= 0:
            rst_discovery_times.append(float(rst_discovery))
        if isinstance(lib_discovery, (int, float)) and lib_discovery >= 0:
            lib_discovery_times.append(float(lib_discovery))
    return {
        "attempts": len(runs),
        "classifications": classifications,
        "comparable_samples": len(ratios),
        "rstorrent_seconds": distribution(rst_times),
        "libtorrent_seconds": distribution(lib_times),
        "rstorrent_over_libtorrent": distribution(ratios),
        "active_transfer_samples": len(active_ratios),
        "rstorrent_active_transfer_seconds": distribution(rst_active_times),
        "libtorrent_active_transfer_seconds": distribution(lib_active_times),
        "rstorrent_over_libtorrent_active_transfer": distribution(active_ratios),
        "rstorrent_discovery_seconds": distribution(rst_discovery_times),
        "libtorrent_discovery_seconds": distribution(lib_discovery_times),
    }


def write_report_atomic(path: Path, report: dict[str, Any]) -> None:
    validate_retained_report(report)
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    temporary_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as destination:
            temporary_path = Path(destination.name)
            destination.write(rendered)
            destination.flush()
            os.fsync(destination.fileno())
        os.replace(temporary_path, path)
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)


def build_probe(repository: Path) -> Path:
    completed = subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "-p",
            "rstorrent-engine",
            "--bin",
            "rstorrent-public-probe",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=300,
        check=False,
    )
    if completed.returncode != 0:
        raise HarnessError(
            "failed to build RSTorrent probe\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    binary = repository / "target" / "release" / "rstorrent-public-probe"
    if not binary.is_file():
        raise HarnessError(f"RSTorrent probe was not created at {binary}")
    return binary


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def command_text(command: list[str], cwd: Path) -> str | None:
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return completed.stdout.strip() if completed.returncode == 0 else None


def environment_snapshot(repository: Path, binary: Path | None) -> dict[str, Any]:
    worker = repository / "tests" / "interop" / "public_compare_libtorrent_worker.py"
    return {
        "repository_commit": command_text(["git", "rev-parse", "HEAD"], repository),
        "repository_dirty": bool(command_text(["git", "status", "--porcelain"], repository)),
        "platform": platform.platform(),
        "architecture": platform.machine(),
        "cpu_count": os.cpu_count(),
        "python": platform.python_version(),
        "rustc": command_text(["rustc", "--version"], repository),
        "libtorrent": command_text(
            [sys.executable, "-c", "import libtorrent as lt; print(lt.version)"], repository
        ),
        "rstorrent_release_sha256": sha256_file(binary) if binary else None,
        "libtorrent_worker_sha256": sha256_file(worker),
        "cargo_profile": "release",
        "cache_policy": "ordinary-uncontrolled-os-cache",
    }


def _parse_cpu_seconds(value: str) -> float | None:
    value = value.strip()
    if not value:
        return None
    try:
        days = 0
        if "-" in value:
            encoded_days, value = value.split("-", 1)
            days = int(encoded_days)
        parts = [float(part) for part in value.split(":")]
        if len(parts) == 3:
            hours, minutes, seconds = parts
        elif len(parts) == 2:
            hours, minutes, seconds = 0, parts[0], parts[1]
        else:
            return None
        return days * 86_400 + hours * 3_600 + minutes * 60 + seconds
    except ValueError:
        return None


def sample_process(pid: int) -> tuple[int | None, float | None]:
    try:
        completed = subprocess.run(
            ["ps", "-o", "rss=", "-o", "time=", "-p", str(pid)],
            capture_output=True,
            text=True,
            timeout=2,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None, None
    fields = completed.stdout.split()
    if completed.returncode != 0 or len(fields) < 2:
        return None, None
    try:
        rss = int(fields[0]) * 1024
    except ValueError:
        rss = None
    return rss, _parse_cpu_seconds(fields[1])


def run_owner_process(
    command: list[str], implementation: str, outer_timeout_seconds: int
) -> dict[str, Any]:
    started = time.monotonic()
    peak_rss = 0
    last_cpu: float | None = None
    samples = 0
    gaps = 0
    forced_termination = False
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        try:
            process = subprocess.Popen(command, stdout=stdout, stderr=stderr)
        except OSError as error:
            return harness_failure(implementation, 0.0, f"start worker: {type(error).__name__}")
        deadline = started + outer_timeout_seconds
        try:
            while process.poll() is None and time.monotonic() < deadline:
                rss, cpu = sample_process(process.pid)
                samples += 1
                if rss is None:
                    gaps += 1
                else:
                    peak_rss = max(peak_rss, rss)
                if cpu is not None:
                    last_cpu = cpu
                time.sleep(PROCESS_SAMPLE_SECONDS)
        except KeyboardInterrupt:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=OUTER_GRACE_SECONDS)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=OUTER_GRACE_SECONDS)
            raise
        if process.poll() is None:
            forced_termination = True
            process.terminate()
            try:
                process.wait(timeout=OUTER_GRACE_SECONDS)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=OUTER_GRACE_SECONDS)
        rss, cpu = sample_process(process.pid)
        if rss is not None:
            peak_rss = max(peak_rss, rss)
        if cpu is not None:
            last_cpu = cpu
        stdout.seek(0, os.SEEK_END)
        stdout_bytes = stdout.tell()
        stderr.seek(0, os.SEEK_END)
        stderr_bytes = stderr.tell()
        stderr.seek(0)
        stderr_sha256 = hashlib.sha256(stderr.read()).hexdigest() if stderr_bytes else None
        if forced_termination:
            result = harness_failure(
                implementation, time.monotonic() - started, "outer worker deadline expired"
            )
        elif stdout_bytes > MAX_REPORT_BYTES:
            result = harness_failure(
                implementation, time.monotonic() - started, "worker report exceeded size limit"
            )
        else:
            stdout.seek(0)
            encoded = stdout.read()
            lines = [line for line in encoded.splitlines() if line.strip()]
            if len(lines) != 1:
                result = harness_failure(
                    implementation,
                    time.monotonic() - started,
                    f"worker emitted {len(lines)} nonempty report lines",
                )
            else:
                try:
                    decoded = json.loads(lines[0])
                except (UnicodeDecodeError, json.JSONDecodeError):
                    decoded = None
                if not isinstance(decoded, dict) or decoded.get("schema_version") != SCHEMA_VERSION:
                    result = harness_failure(
                        implementation, time.monotonic() - started, "worker returned unknown schema"
                    )
                else:
                    result = decoded
        result["process"] = {
            "exit_code": process.returncode,
            "peak_rss_bytes": peak_rss or None,
            "cpu_seconds": last_cpu,
            "sample_interval_seconds": PROCESS_SAMPLE_SECONDS,
            "samples": samples,
            "sample_gaps": gaps,
            "forced_termination": forced_termination,
            "stderr_bytes": stderr_bytes,
            "stderr_sha256": stderr_sha256,
        }
        return result


def _input_request(
    input_source: str | Path,
    expected_info_hash: str,
) -> tuple[dict[str, Any], Any | None]:
    if isinstance(input_source, Path):
        payload = input_source.read_bytes()
        descriptor = parse_metainfo(payload)
        if descriptor.info_hash != expected_info_hash:
            raise HarnessError("direct metainfo identity does not match expected info hash")
        return {
            "mode": "metainfo",
            "path": str(input_source.resolve()),
            "sha256": descriptor.outer_sha256,
        }, descriptor
    return {"mode": "magnet", "magnet": input_source}, None


def run_rstorrent(
    binary: Path,
    input_source: str | Path,
    profile: str,
    target: str,
    timeout_seconds: int,
    cleanup_seconds: int,
    output_root: Path,
    expected_info_hash: str | None = None,
    wire_payload_ceiling_bytes: int | None = None,
    peer_hints: list[str] | None = None,
) -> dict[str, Any]:
    profile_contract = comparison_profile(profile)
    if expected_info_hash is None:
        expected_info_hash = magnet_info_hash(str(input_source))
    input_request, descriptor = _input_request(input_source, expected_info_hash)
    command = [
        str(binary),
        f"--{input_request['mode']}",
        input_request.get("magnet", input_request.get("path")),
        "--expected-info-hash",
        expected_info_hash,
        "--output",
        str(output_root),
        "--profile",
        profile_contract["name"],
        "--profile-sha256",
        profile_contract["sha256"],
        "--target",
        target,
        "--timeout-seconds",
        str(timeout_seconds),
        "--cleanup-seconds",
        str(cleanup_seconds),
        "--wire-payload-ceiling-bytes",
        str(wire_payload_ceiling_bytes or (descriptor.payload_bytes * 2 if descriptor else 512 * 1024 * 1024)),
    ]
    for peer_hint in peer_hints or []:
        command.extend(("--peer-hint", peer_hint))
    result = run_owner_process(
        command, "rstorrent", timeout_seconds + cleanup_seconds * 2 + OUTER_GRACE_SECONDS
    )
    validate_worker_result(result, profile_contract, expected_info_hash)
    verify_completed_result(result, descriptor, output_root)
    return result


def run_libtorrent(
    input_source: str | Path,
    expected_info_hash: str,
    profile: str,
    target: str,
    timeout_seconds: int,
    output_root: Path,
    cleanup_seconds: int = 10,
    wire_payload_ceiling_bytes: int | None = None,
    peer_hints: list[str] | None = None,
) -> dict[str, Any]:
    profile_contract = comparison_profile(profile)
    input_request, descriptor = _input_request(input_source, expected_info_hash)
    request = {
        "schema_version": SCHEMA_VERSION,
        "input": input_request,
        "expected_info_hash": expected_info_hash,
        "profile": profile_contract["name"],
        "profile_sha256": profile_contract["sha256"],
        "target": target,
        "timeout_seconds": timeout_seconds,
        "wire_payload_ceiling_bytes": wire_payload_ceiling_bytes
        or (descriptor.payload_bytes * 2 if descriptor else 512 * 1024 * 1024),
        "output_root": str(output_root),
        "peer_hints": peer_hints or [],
    }
    worker = repository_root() / "tests" / "interop" / "public_compare_libtorrent_worker.py"
    with tempfile.TemporaryDirectory(prefix="rstorrent-libtorrent-request-") as temporary:
        request_path = Path(temporary) / "request.json"
        request_path.write_text(
            json.dumps(request, sort_keys=True, separators=(",", ":")), encoding="utf-8"
        )
        result = run_owner_process(
            [sys.executable, str(worker), "--request", str(request_path)],
            "libtorrent",
            timeout_seconds + cleanup_seconds * 2 + OUTER_GRACE_SECONDS,
        )
    validate_worker_result(result, profile_contract, expected_info_hash)
    verify_completed_result(result, descriptor, output_root)
    return result


def validate_worker_result(
    result: dict[str, Any], profile_contract: dict[str, Any], expected_info_hash: str
) -> None:
    if result.get("outcome") == "harness_error":
        return
    implementation = result.get("implementation")
    expected_settings = profile_contract.get(implementation)
    if (
        implementation not in ("rstorrent", "libtorrent")
        or result.get("profile") != profile_contract["name"]
        or result.get("profile_sha256") != profile_contract["sha256"]
        or result.get("effective_settings") != expected_settings
        or result.get("info_hash") != expected_info_hash
    ):
        result["outcome"] = "harness_error"
        result["integrity_verified"] = False
        result["terminal_detail"] = "worker contract echo mismatch"
        return
    if profile_contract["name"] == "matched-rc4-30" and result.get("outcome") == "milestone_reached":
        evidence = result.get("diagnostics", {}).get("peer_methods", {})
        if implementation == "rstorrent":
            invalid = evidence.get("payload_contributor_plaintext_stream") or evidence.get(
                "payload_contributor_plaintext_payload"
            )
            valid = evidence.get("payload_contributor_rc4")
        else:
            invalid = evidence.get("payload_contributor_plaintext_stream") or evidence.get(
                "payload_contributor_plaintext_payload"
            )
            valid = evidence.get("payload_contributor_rc4")
        if invalid or not valid:
            result["outcome"] = "harness_error"
            result["integrity_verified"] = False
            result["terminal_detail"] = "forced-RC4 payload contributor evidence failed"


def verify_completed_result(result: dict[str, Any], descriptor: Any | None, output_root: Path) -> None:
    result["integrity_verified"] = False
    if result.get("outcome") != "milestone_reached" or result.get("target") != "complete":
        return
    if descriptor is None:
        result["terminal_detail"] = "complete magnet result lacks independent metainfo verifier"
        result["outcome"] = "harness_error"
        return
    try:
        result["independent_verification"] = verify_payload(descriptor, output_root)
    except ContractError as error:
        result["outcome"] = "integrity_failure"
        result["terminal_detail"] = str(error)
        return
    result["integrity_verified"] = True


def magnet_info_hash(magnet: str) -> str:
    for key, value in parse_qsl(urlsplit(magnet).query):
        if key == "xt" and value.lower().startswith("urn:btih:"):
            info_hash = value.rsplit(":", 1)[-1].lower()
            if len(info_hash) == 40 and all(character in "0123456789abcdef" for character in info_hash):
                return info_hash
    raise HarnessError("magnet lacks a lowercase hexadecimal v1 info hash")


def harness_failure(implementation: str, wall_seconds: float, detail: str) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "implementation": implementation,
        "outcome": "harness_error",
        "wall_seconds": wall_seconds,
        "milestones": {},
        "geometry": {},
        "verified_piece_count": 0,
        "verified_bytes": 0,
        "integrity_verified": False,
        "cleanup_succeeded": False,
        "terminal_detail": detail[:16_384],
        "capabilities": {},
        "diagnostics": {},
    }


def validate_catalog_observation(result: dict[str, Any], torrent: dict[str, Any]) -> None:
    if result.get("outcome") != "milestone_reached":
        return
    geometry = result.get("geometry", {})
    for catalog_key, result_key in (
        ("payload_bytes", "total_length"),
        ("piece_length", "piece_length"),
        ("piece_count", "piece_count"),
        ("file_count", "file_count"),
    ):
        expected = torrent.get("expected", {}).get(catalog_key)
        if expected is not None and geometry.get(result_key) != expected:
            result["outcome"] = "integrity_failure"
            result["integrity_verified"] = False
            result["terminal_detail"] = f"catalog {catalog_key} did not match observed geometry"
            return


class BoundedRedirectHandler(urllib.request.HTTPRedirectHandler):
    def __init__(self, allowed_hosts: set[str]) -> None:
        super().__init__()
        self.allowed_hosts = allowed_hosts
        self.redirects = 0

    def redirect_request(self, request: Any, file_pointer: Any, code: int, message: str, headers: Any, new_url: str) -> Any:
        self.redirects += 1
        if self.redirects > MAX_REDIRECTS:
            raise HarnessError("metainfo source exceeded redirect limit")
        split = urlsplit(new_url)
        if split.scheme != "https" or split.hostname not in self.allowed_hosts:
            raise HarnessError("metainfo redirect escaped the catalog HTTPS host allowlist")
        return super().redirect_request(request, file_pointer, code, message, headers, new_url)


def fetch_catalog_metainfo(entry: dict[str, Any], destination: Path) -> Any:
    recipe = entry.get("metainfo")
    if not isinstance(recipe, dict):
        raise HarnessError(f"catalog entry {entry['slug']} has no direct-metainfo recipe")
    allowed_hosts = set(recipe["allowed_hosts"])
    split = urlsplit(recipe["url"])
    if split.scheme != "https" or split.hostname not in allowed_hosts:
        raise HarnessError("metainfo source is outside its HTTPS host allowlist")
    request = urllib.request.Request(
        recipe["url"], headers={"User-Agent": "RSTorrent-public-compare/2"}
    )
    last_error: Exception | None = None
    for _attempt in range(2):
        try:
            opener = urllib.request.build_opener(BoundedRedirectHandler(allowed_hosts))
            with opener.open(request, timeout=60) as response:
                payload = response.read(64 * 1024 * 1024 + 1)
            break
        except (OSError, urllib.error.URLError) as error:
            last_error = error
    else:
        assert last_error is not None
        raise HarnessError(
            f"metainfo retrieval failed: {type(last_error).__name__}"
        ) from last_error
    descriptor = parse_metainfo(payload)
    if descriptor.outer_sha256 != recipe["sha256"] or descriptor.info_hash != entry["info_hash"]:
        raise HarnessError("official metainfo changed from the reviewed catalog identity")
    expected = entry["expected"]
    observed = descriptor.normalized_geometry()
    if any(
        value is not None and observed.get(key) != value
        for key, value in expected.items()
    ):
        raise HarnessError("official metainfo geometry or discovery set changed from catalog")
    if descriptor.private:
        raise HarnessError("private torrents are outside this harness")
    destination.write_bytes(payload)
    return descriptor


def suite_scenarios(catalog: dict[str, Any], suite: str) -> list[dict[str, Any]]:
    small = select_role(catalog, "small-primary")
    medium = select_role(catalog, "medium-distro") if suite == "standard" else None
    large = (
        select_role(catalog, "large-distro")
        if suite in ("quick", "large", "product")
        else None
    )
    if suite == "quick":
        return [
            {
                "torrent": small,
                "profile": "matched-plain-30",
                "target": "complete",
                "runs": 1,
                "timeout_seconds": 120,
            },
            {
                "torrent": large,
                "profile": "matched-plain-30",
                "target": "10-percent",
                "runs": 1,
                "timeout_seconds": 120,
            },
        ]
    if suite == "smoke":
        return [{"torrent": small, "profile": "matched-plain-30", "target": "complete", "runs": 1}]
    if suite == "standard":
        return [
            {"torrent": small, "profile": "matched-plain-30", "target": "complete", "runs": 4},
            {"torrent": medium, "profile": "matched-plain-30", "target": "complete", "runs": 4},
        ]
    if suite == "large":
        return [{"torrent": large, "profile": "matched-plain-30", "target": "complete", "runs": 2}]
    if suite == "product":
        return [
            {"torrent": small, "profile": "product-default", "target": "complete", "runs": 2},
            {"torrent": large, "profile": "product-default", "target": "complete", "runs": 2},
        ]
    if suite == "encryption":
        return [{"torrent": small, "profile": "matched-rc4-30", "target": "first-piece", "runs": 1}]
    if suite == "breadth":
        return [
            {
                "torrent": entry,
                "profile": "dht-only" if "dht-only" in entry["roles"] else "matched-plain-30",
                "target": entry["limits"]["max_target"],
                "runs": 1,
            }
            for entry in catalog["torrents"]
            if "small-primary" not in entry["roles"]
        ]
    raise HarnessError(f"unknown suite {suite!r}")


def parse_args(arguments: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=SUITES)
    parser.add_argument("--torrent", default="big-buck-bunny")
    parser.add_argument("--profile", choices=PROFILES, default="matched-plain-30")
    parser.add_argument("--owner", choices=OWNERS, default="both")
    parser.add_argument("--input-mode", choices=INPUT_MODES, default="metainfo")
    parser.add_argument("--target", choices=TARGETS, default="metadata")
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--timeout-seconds", type=int)
    parser.add_argument("--cleanup-seconds", type=int, default=10)
    parser.add_argument("--catalog", type=Path, default=default_catalog_path())
    parser.add_argument("--output", type=Path)
    parser.add_argument("--max-network-gib", type=float)
    parser.add_argument("--allow-public-network", action="store_true")
    parser.add_argument("--quiet", action="store_true")
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--inspect-candidate", type=Path)
    args = parser.parse_args(arguments)
    if not 1 <= args.runs <= MAX_PAIRS:
        parser.error(f"--runs must be between 1 and {MAX_PAIRS}")
    if args.timeout_seconds is not None and not 1 <= args.timeout_seconds <= MAX_OWNER_TIMEOUT_SECONDS:
        parser.error(f"--timeout-seconds must be between 1 and {MAX_OWNER_TIMEOUT_SECONDS}")
    if not 1 <= args.cleanup_seconds <= 60:
        parser.error("--cleanup-seconds must be between 1 and 60")
    if args.inspect_candidate is None and (
        not args.allow_public_network or args.output is None or args.max_network_gib is None
    ):
        parser.error(
            "public execution requires --allow-public-network, --output, and --max-network-gib"
        )
    return args


def run_one_scenario(
    scenario: dict[str, Any],
    args: argparse.Namespace,
    binary: Path,
    owned_root: Path,
    metainfo_path: Path | None,
    cohort: dict[str, Any],
    checkpoint: Callable[[], None],
) -> dict[str, Any]:
    torrent = scenario["torrent"]
    profile = normalize_profile(scenario["profile"])
    target = scenario["target"]
    runs = scenario["runs"]
    timeout_seconds = (
        args.timeout_seconds
        or scenario.get("timeout_seconds")
        or torrent["limits"]["owner_timeout_seconds"]
    )
    wire_ceiling = torrent["limits"]["wire_payload_ceiling_bytes"]
    rst_magnet, lib_magnet = scenario_magnets(torrent, profile)
    cohort.update(
        {
            "torrent": {
                "slug": torrent["slug"],
                "name": torrent["name"],
                "info_hash": torrent["info_hash"],
                "roles": torrent["roles"],
                "expected": torrent["expected"],
                "source": torrent["source"],
            },
            "profile": profile,
            "profile_sha256": comparison_profile(profile)["sha256"],
            "target": target,
            "input_mode": args.input_mode,
            "owner_timeout_seconds": timeout_seconds,
            "runs": [],
            "summary": summarize([], target, args.owner),
        }
    )
    runs_report = cohort["runs"]
    checkpoint()
    for ordinal in range(runs):
        implementations: dict[str, Any] = {}
        order = selected_implementations(ordinal, args.owner)
        run_report = {
            "ordinal": ordinal,
            "order": order,
            "classification": "in_progress",
            "implementations": implementations,
        }
        runs_report.append(run_report)
        checkpoint()
        for implementation in order:
            output_root = owned_root / torrent["slug"] / f"pair-{ordinal}" / implementation
            output_root.parent.mkdir(parents=True, exist_ok=True)
            input_source: str | Path = (
                metainfo_path
                if args.input_mode == "metainfo"
                else (rst_magnet if implementation == "rstorrent" else lib_magnet)
            )
            if input_source is None:
                raise HarnessError("direct metainfo input was not prepared")
            if implementation == "rstorrent":
                result = run_rstorrent(
                    binary,
                    input_source,
                    profile,
                    target,
                    timeout_seconds,
                    args.cleanup_seconds,
                    output_root,
                    torrent["info_hash"],
                    wire_ceiling,
                )
            else:
                result = run_libtorrent(
                    input_source,
                    torrent["info_hash"],
                    profile,
                    target,
                    timeout_seconds,
                    output_root,
                    args.cleanup_seconds,
                    wire_ceiling,
                )
            validate_catalog_observation(result, torrent)
            implementations[implementation] = result
            resolved = validate_output_ancestry(owned_root, output_root)
            shutil.rmtree(resolved, ignore_errors=True)
            checkpoint()
        classification = (
            classify_pair(implementations["rstorrent"], implementations["libtorrent"])
            if args.owner == "both"
            else classify_owner(implementations[args.owner])
        )
        run_report["classification"] = classification
        cohort["summary"] = summarize(runs_report, target, args.owner)
        checkpoint()
    return cohort


def run_campaign(args: argparse.Namespace) -> dict[str, Any]:
    repository = repository_root()
    catalog = load_catalog(args.catalog.resolve())
    scenarios = (
        suite_scenarios(catalog, args.suite)
        if args.suite
        else [
            {
                "torrent": select_torrent(catalog, args.torrent),
                "profile": args.profile,
                "target": args.target,
                "runs": args.runs,
            }
        ]
    )
    owners = 2 if args.owner == "both" else 1
    budget = sum(
        invocation_network_budget(
            scenario["torrent"]["expected"]["payload_bytes"], scenario["runs"], owners
        )
        for scenario in scenarios
    )
    authorized = int(args.max_network_gib * 1024**3)
    if authorized < budget or authorized > MAX_NETWORK_BYTES:
        raise HarnessError(
            f"authorized network budget must be at least {budget} and at most {MAX_NETWORK_BYTES} bytes"
        )
    total_payload = max(scenario["torrent"]["expected"]["payload_bytes"] for scenario in scenarios)
    output_parent = args.output.resolve().parent
    output_parent.mkdir(parents=True, exist_ok=True)
    if shutil.disk_usage(output_parent).free < required_free_space(total_payload):
        raise HarnessError("insufficient free space for the largest selected owner")
    binary = repository / "target" / "release" / "rstorrent-public-probe"
    if args.owner != "libtorrent" and not args.no_build:
        binary = build_probe(repository)
    elif args.owner != "libtorrent" and not binary.is_file():
        raise HarnessError(f"--no-build probe does not exist at {binary}")
    report = {
        "schema_version": SCHEMA_VERSION,
        "status": "running",
        "generated_at_unix_seconds": time.time(),
        "environment": environment_snapshot(repository, binary if binary.is_file() else None),
        "config": {
            "suite": args.suite,
            "owner": args.owner,
            "input_mode": args.input_mode,
            "cleanup_seconds": args.cleanup_seconds,
            "authorized_network_bytes": authorized,
            "worst_case_network_bytes": budget,
            "order": "ABBA" if args.owner == "both" else "single-owner",
        },
        "catalog_schema_version": catalog["schema_version"],
        "cohorts": [],
    }
    output_path = args.output.resolve()

    def checkpoint() -> None:
        report["checkpointed_at_unix_seconds"] = time.time()
        write_report_atomic(output_path, report)

    checkpoint()
    try:
        with tempfile.TemporaryDirectory(prefix="rstorrent-public-compare-") as temporary:
            owned_root = Path(temporary).resolve()
            for scenario in scenarios:
                metainfo_path: Path | None = None
                if args.input_mode == "metainfo":
                    metainfo_path = owned_root / f"{scenario['torrent']['slug']}.torrent"
                    fetch_catalog_metainfo(scenario["torrent"], metainfo_path)
                cohort: dict[str, Any] = {}
                report["cohorts"].append(cohort)
                checkpoint()
                completed = run_one_scenario(
                    scenario,
                    args,
                    binary,
                    owned_root,
                    metainfo_path,
                    cohort,
                    checkpoint,
                )
                if completed is not cohort:
                    raise HarnessError("scenario checkpoint identity changed")
                checkpoint()
    except KeyboardInterrupt:
        report["status"] = "interrupted"
        checkpoint()
        raise
    except (HarnessError, ContractError, OSError) as error:
        report["status"] = "failed"
        report["terminal_detail"] = f"{type(error).__name__}: {error}"
        checkpoint()
        raise
    report["status"] = "complete"
    report["completed_at_unix_seconds"] = time.time()
    checkpoint()
    return report


def inspect_candidate(args: argparse.Namespace) -> dict[str, Any]:
    descriptor = parse_metainfo(args.inspect_candidate.read_bytes())
    return {
        "sha256": descriptor.outer_sha256,
        "info_hash": descriptor.info_hash,
        "name": descriptor.name,
        "expected": descriptor.normalized_geometry(),
    }


def main(arguments: list[str]) -> int:
    args = parse_args(arguments)
    try:
        report = inspect_candidate(args) if args.inspect_candidate else run_campaign(args)
        validate_retained_report(report)
    except KeyboardInterrupt:
        print("interrupted; last completed owner checkpoint retained", file=sys.stderr)
        return 130
    except (HarnessError, ContractError, OSError) as error:
        print(f"harness error: {error}", file=sys.stderr)
        return 2
    rendered = json.dumps(report, indent=2, sort_keys=True)
    if args.output is not None:
        write_report_atomic(args.output, report)
    if not args.quiet:
        print(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
