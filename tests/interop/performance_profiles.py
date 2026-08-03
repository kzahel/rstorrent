#!/usr/bin/env python3
"""Load and validate committed hardware-specific performance policy."""

from __future__ import annotations

import os
import platform
import re
import shutil
import subprocess
import tomllib
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1
PROFILE_ID_PATTERN = re.compile(r"^[a-z0-9][a-z0-9._-]{0,63}$")
CASE_ID_PATTERN = re.compile(r"^[a-z0-9][a-z0-9._-]{0,95}$")
TIERS = {"smoke", "full"}
APPLICATION_MODES = {
    "idle",
    "library",
    "general",
    "peers",
    "files",
    "trackers",
    "pieces",
    "disk",
    "logs-normal",
    "logs-detailed",
    "logs-trace",
    "all",
    "slow-all",
}
MAX_THROUGHPUT_CASES = 64
MAX_APPLICATION_CASES = 32
MAX_SIZE_MIB = 10 * 1024
MIN_PIECE_KIB = 16
MAX_PIECE_KIB = 256 * 1024


class PerformanceProfileError(ValueError):
    """A committed performance profile is malformed or inapplicable."""


def _run_text(command: list[str]) -> str | None:
    try:
        completed = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if completed.returncode != 0:
        return None
    value = completed.stdout.strip()
    return value or None


def _normalized_architecture(value: str) -> str:
    lowered = value.lower()
    return {
        "aarch64": "arm64",
        "amd64": "x86_64",
        "x64": "x86_64",
    }.get(lowered, lowered)


def _cpu_model(system: str) -> str | None:
    if system == "darwin":
        return _run_text(["sysctl", "-n", "machdep.cpu.brand_string"])
    if system == "linux":
        try:
            for line in Path("/proc/cpuinfo").read_text(encoding="utf-8").splitlines():
                if line.lower().startswith(("model name", "hardware")):
                    _, value = line.split(":", maxsplit=1)
                    if value.strip():
                        return value.strip()
        except (OSError, UnicodeError, ValueError):
            pass
    return platform.processor() or None


def _memory_bytes() -> int | None:
    try:
        pages = os.sysconf("SC_PHYS_PAGES")
        page_size = os.sysconf("SC_PAGE_SIZE")
    except (AttributeError, OSError, ValueError):
        return None
    if not isinstance(pages, int) or not isinstance(page_size, int):
        return None
    value = pages * page_size
    return value if value > 0 else None


def _filesystem_type(path: Path, system: str) -> str | None:
    if system == "linux":
        return _run_text(["stat", "-f", "-c", "%T", str(path)])
    if system == "darwin":
        # `mount` is stable enough for evidence but is deliberately not used as
        # an applicability gate. Resolve /tmp so its /private bind is visible.
        resolved = str(path.resolve())
        output = _run_text(["mount"])
        if output is not None:
            candidates: list[tuple[int, str]] = []
            for line in output.splitlines():
                match = re.search(r" on (.+) \(([^, )]+)", line)
                if match is None:
                    continue
                mountpoint, filesystem = match.groups()
                if resolved == mountpoint or resolved.startswith(mountpoint.rstrip("/") + "/"):
                    candidates.append((len(mountpoint), filesystem))
            if candidates:
                return max(candidates)[1]
    return None


def collect_hardware_environment(temporary_root: Path) -> dict[str, Any]:
    """Collect stable matching fields and richer report-only evidence."""
    system = platform.system().lower()
    usage = shutil.disk_usage(temporary_root)
    memory = _memory_bytes()
    return {
        "system": system,
        "architecture": _normalized_architecture(platform.machine()),
        "hostname": platform.node() or None,
        "cpu_model": _cpu_model(system),
        "logical_cpu_count": os.cpu_count(),
        "memory_bytes": memory,
        "temporary_free_bytes": usage.free,
        "temporary_total_bytes": usage.total,
        "temporary_filesystem": _filesystem_type(temporary_root, system),
        "github_actions": os.environ.get("GITHUB_ACTIONS", "").lower() == "true",
        "github_runner_os": os.environ.get("RUNNER_OS"),
        "github_runner_arch": os.environ.get("RUNNER_ARCH"),
        "github_runner_environment": os.environ.get("RUNNER_ENVIRONMENT"),
        "github_image_os": os.environ.get("ImageOS"),
        "github_image_version": os.environ.get("ImageVersion"),
    }


def _expect_table(value: Any, name: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise PerformanceProfileError(f"{name} must be a table")
    return value


def _expect_string(value: Any, name: str, pattern: re.Pattern[str] | None = None) -> str:
    if not isinstance(value, str) or not value:
        raise PerformanceProfileError(f"{name} must be a nonempty string")
    if pattern is not None and pattern.fullmatch(value) is None:
        raise PerformanceProfileError(f"{name} has an invalid value {value!r}")
    return value


def _expect_int(value: Any, name: str, minimum: int, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise PerformanceProfileError(f"{name} must be an integer")
    if not minimum <= value <= maximum:
        raise PerformanceProfileError(
            f"{name} must be between {minimum} and {maximum}"
        )
    return value


def _expect_number(value: Any, name: str, minimum: float = 0.0) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise PerformanceProfileError(f"{name} must be a number")
    parsed = float(value)
    if parsed < minimum:
        raise PerformanceProfileError(f"{name} must be at least {minimum}")
    return parsed


def _expect_string_list(value: Any, name: str) -> list[str]:
    if not isinstance(value, list) or not value:
        raise PerformanceProfileError(f"{name} must be a nonempty array")
    output: list[str] = []
    for index, item in enumerate(value):
        text = _expect_string(item, f"{name}[{index}]")
        if text in output:
            raise PerformanceProfileError(f"{name} contains duplicate {text!r}")
        output.append(text)
    return output


def _validate_tiers(value: Any, name: str) -> list[str]:
    tiers = _expect_string_list(value, name)
    unknown = set(tiers) - TIERS
    if unknown:
        raise PerformanceProfileError(f"{name} contains unknown tiers {sorted(unknown)}")
    return tiers


def _validate_piece_size(value: Any, name: str) -> int:
    piece_kib = _expect_int(value, name, MIN_PIECE_KIB, MAX_PIECE_KIB)
    if piece_kib & (piece_kib - 1):
        raise PerformanceProfileError(f"{name} must be a power of two")
    return piece_kib


def _validate_requirements(profile: dict[str, Any]) -> None:
    requirements = _expect_table(profile.get("requirements"), "requirements")
    allowed = {
        "systems",
        "architectures",
        "cpu_model_contains",
        "minimum_logical_cpus",
        "minimum_memory_gib",
        "minimum_temporary_free_gib",
        "github_actions",
        "github_runner_os",
        "github_runner_arch",
        "github_image_os",
    }
    unknown = requirements.keys() - allowed
    if unknown:
        raise PerformanceProfileError(
            f"requirements contains unknown fields {sorted(unknown)}"
        )
    systems = _expect_string_list(requirements.get("systems"), "requirements.systems")
    requirements["systems"] = [value.lower() for value in systems]
    architectures = _expect_string_list(
        requirements.get("architectures"), "requirements.architectures"
    )
    requirements["architectures"] = [
        _normalized_architecture(value) for value in architectures
    ]
    if "cpu_model_contains" in requirements:
        _expect_string(requirements["cpu_model_contains"], "requirements.cpu_model_contains")
    for field in ("minimum_logical_cpus",):
        if field in requirements:
            _expect_int(requirements[field], f"requirements.{field}", 1, 1024)
    for field in ("minimum_memory_gib", "minimum_temporary_free_gib"):
        if field in requirements:
            _expect_number(requirements[field], f"requirements.{field}", 0.0)
    if "github_actions" in requirements and not isinstance(
        requirements["github_actions"], bool
    ):
        raise PerformanceProfileError("requirements.github_actions must be boolean")
    for field in ("github_runner_os", "github_runner_arch", "github_image_os"):
        if field in requirements:
            _expect_string(requirements[field], f"requirements.{field}")


def _validate_observed(value: Any, name: str, allowed: set[str]) -> None:
    observed = _expect_table(value, name)
    unknown = observed.keys() - allowed
    if unknown:
        raise PerformanceProfileError(f"{name} contains unknown fields {sorted(unknown)}")
    for field, item in observed.items():
        if field in {"commit", "date", "note"}:
            _expect_string(item, f"{name}.{field}")
        elif field == "runs":
            _expect_int(item, f"{name}.{field}", 1, 100)
        else:
            _expect_number(item, f"{name}.{field}", 0.0)


def _validate_throughput(profile: dict[str, Any]) -> None:
    throughput = _expect_table(profile.get("throughput"), "throughput")
    allowed = {"smoke_runs", "full_runs", "timeout_seconds", "cases"}
    unknown = throughput.keys() - allowed
    if unknown:
        raise PerformanceProfileError(
            f"throughput contains unknown fields {sorted(unknown)}"
        )
    _expect_int(throughput.get("smoke_runs"), "throughput.smoke_runs", 1, 5)
    _expect_int(throughput.get("full_runs"), "throughput.full_runs", 1, 5)
    _expect_int(
        throughput.get("timeout_seconds"),
        "throughput.timeout_seconds",
        1,
        4 * 60 * 60,
    )
    cases = throughput.get("cases")
    if not isinstance(cases, list) or not 1 <= len(cases) <= MAX_THROUGHPUT_CASES:
        raise PerformanceProfileError(
            f"throughput.cases must contain 1..={MAX_THROUGHPUT_CASES} cases"
        )
    ids: set[str] = set()
    keys: set[tuple[int, int, int, int]] = set()
    for index, item in enumerate(cases):
        name = f"throughput.cases[{index}]"
        case = _expect_table(item, name)
        allowed_case = {
            "id",
            "tiers",
            "size_mib",
            "piece_size_kib",
            "write_concurrency",
            "hash_concurrency",
            "observed",
            "required",
        }
        unknown_case = case.keys() - allowed_case
        if unknown_case:
            raise PerformanceProfileError(
                f"{name} contains unknown fields {sorted(unknown_case)}"
            )
        case_id = _expect_string(case.get("id"), f"{name}.id", CASE_ID_PATTERN)
        if case_id in ids:
            raise PerformanceProfileError(f"duplicate throughput case ID {case_id!r}")
        ids.add(case_id)
        _validate_tiers(case.get("tiers"), f"{name}.tiers")
        size_mib = _expect_int(case.get("size_mib"), f"{name}.size_mib", 1, MAX_SIZE_MIB)
        piece_kib = _validate_piece_size(
            case.get("piece_size_kib"), f"{name}.piece_size_kib"
        )
        write = _expect_int(
            case.get("write_concurrency"), f"{name}.write_concurrency", 1, 8
        )
        hashes = _expect_int(
            case.get("hash_concurrency"), f"{name}.hash_concurrency", 1, 8
        )
        key = (size_mib, piece_kib, write, hashes)
        if key in keys:
            raise PerformanceProfileError(f"duplicate throughput geometry {key}")
        keys.add(key)
        if "observed" in case:
            _validate_observed(
                case["observed"],
                f"{name}.observed",
                {
                    "median_mib_s",
                    "libtorrent_median_mib_s",
                    "libtorrent_ratio",
                    "runs",
                    "commit",
                    "date",
                    "note",
                },
            )
        required = _expect_table(case.get("required"), f"{name}.required")
        unknown_required = required.keys() - {
            "minimum_mib_s",
            "minimum_libtorrent_ratio",
        }
        if unknown_required:
            raise PerformanceProfileError(
                f"{name}.required contains unknown fields {sorted(unknown_required)}"
            )
        if not required:
            raise PerformanceProfileError(f"{name}.required must not be empty")
        for field, value in required.items():
            _expect_number(value, f"{name}.required.{field}", 0.0)


def _validate_application(profile: dict[str, Any]) -> None:
    application = _expect_table(
        profile.get("application_observation"), "application_observation"
    )
    allowed = {
        "size_mib",
        "piece_size_kib",
        "smoke_runs",
        "full_runs",
        "timeout_seconds",
        "slow_consumer_delay_millis",
        "cases",
    }
    unknown = application.keys() - allowed
    if unknown:
        raise PerformanceProfileError(
            f"application_observation contains unknown fields {sorted(unknown)}"
        )
    _expect_int(
        application.get("size_mib"),
        "application_observation.size_mib",
        1,
        MAX_SIZE_MIB,
    )
    _validate_piece_size(
        application.get("piece_size_kib"),
        "application_observation.piece_size_kib",
    )
    _expect_int(
        application.get("smoke_runs"),
        "application_observation.smoke_runs",
        1,
        5,
    )
    _expect_int(
        application.get("full_runs"),
        "application_observation.full_runs",
        1,
        5,
    )
    _expect_int(
        application.get("timeout_seconds"),
        "application_observation.timeout_seconds",
        1,
        4 * 60 * 60,
    )
    _expect_int(
        application.get("slow_consumer_delay_millis"),
        "application_observation.slow_consumer_delay_millis",
        1,
        60_000,
    )
    cases = application.get("cases")
    if not isinstance(cases, list) or not 1 <= len(cases) <= MAX_APPLICATION_CASES:
        raise PerformanceProfileError(
            f"application_observation.cases must contain 1..={MAX_APPLICATION_CASES} cases"
        )
    modes: set[str] = set()
    ids: set[str] = set()
    for index, item in enumerate(cases):
        name = f"application_observation.cases[{index}]"
        case = _expect_table(item, name)
        allowed_case = {"id", "mode", "tiers", "observed", "required"}
        unknown_case = case.keys() - allowed_case
        if unknown_case:
            raise PerformanceProfileError(
                f"{name} contains unknown fields {sorted(unknown_case)}"
            )
        case_id = _expect_string(case.get("id"), f"{name}.id", CASE_ID_PATTERN)
        if case_id in ids:
            raise PerformanceProfileError(f"duplicate application case ID {case_id!r}")
        ids.add(case_id)
        mode = _expect_string(case.get("mode"), f"{name}.mode")
        if mode not in APPLICATION_MODES:
            raise PerformanceProfileError(f"{name}.mode is unknown: {mode!r}")
        if mode in modes:
            raise PerformanceProfileError(f"duplicate application mode {mode!r}")
        modes.add(mode)
        _validate_tiers(case.get("tiers"), f"{name}.tiers")
        if "observed" in case:
            _validate_observed(
                case["observed"],
                f"{name}.observed",
                {
                    "median_mib_s",
                    "idle_ratio",
                    "median_cpu_seconds",
                    "median_peak_rss_bytes",
                    "runs",
                    "commit",
                    "date",
                    "note",
                },
            )
        required = _expect_table(case.get("required"), f"{name}.required")
        unknown_required = required.keys() - {
            "minimum_mib_s",
            "minimum_idle_ratio",
            "maximum_resets",
            "maximum_queue_high_water_bytes",
        }
        if unknown_required:
            raise PerformanceProfileError(
                f"{name}.required contains unknown fields {sorted(unknown_required)}"
            )
        if not required:
            raise PerformanceProfileError(f"{name}.required must not be empty")
        for field, value in required.items():
            if field in {"maximum_resets", "maximum_queue_high_water_bytes"}:
                _expect_int(value, f"{name}.required.{field}", 0, 1 << 40)
            else:
                _expect_number(value, f"{name}.required.{field}", 0.0)
    if "idle" not in modes:
        raise PerformanceProfileError("application_observation requires an idle case")


def validate_profile(profile: dict[str, Any]) -> dict[str, Any]:
    allowed = {
        "schema_version",
        "profile_id",
        "description",
        "calibration_status",
        "requirements",
        "throughput",
        "application_observation",
    }
    unknown = profile.keys() - allowed
    if unknown:
        raise PerformanceProfileError(f"profile contains unknown fields {sorted(unknown)}")
    if profile.get("schema_version") != SCHEMA_VERSION:
        raise PerformanceProfileError(
            f"schema_version must be {SCHEMA_VERSION}"
        )
    _expect_string(profile.get("profile_id"), "profile_id", PROFILE_ID_PATTERN)
    _expect_string(profile.get("description"), "description")
    calibration = _expect_string(profile.get("calibration_status"), "calibration_status")
    if calibration not in {"calibrated", "provisional", "uncalibrated"}:
        raise PerformanceProfileError(
            "calibration_status must be calibrated, provisional or uncalibrated"
        )
    _validate_requirements(profile)
    _validate_throughput(profile)
    _validate_application(profile)
    return profile


def load_performance_profile(repository: Path, selection: str) -> tuple[dict[str, Any], Path]:
    candidate = Path(selection)
    named_profile = (
        not candidate.is_absolute()
        and candidate.parent == Path(".")
        and candidate.suffix.lower() != ".toml"
    )
    if named_profile:
        candidate = repository / "tests" / "perf" / "baselines" / f"{selection}.toml"
    elif not candidate.is_absolute():
        candidate = repository / candidate
    try:
        with candidate.open("rb") as source:
            profile = tomllib.load(source)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise PerformanceProfileError(
            f"cannot load performance profile {selection!r}: {error}"
        ) from error
    validated = validate_profile(profile)
    if named_profile:
        if validated["profile_id"] != selection:
            raise PerformanceProfileError(
                f"profile {selection!r} declares ID {validated['profile_id']!r}"
            )
    return validated, candidate.resolve()


def validate_environment(
    profile: dict[str, Any], environment: dict[str, Any]
) -> list[str]:
    requirements = profile["requirements"]
    failures: list[str] = []
    if environment.get("system") not in requirements["systems"]:
        failures.append(
            f"system {environment.get('system')!r} is not one of {requirements['systems']}"
        )
    if environment.get("architecture") not in requirements["architectures"]:
        failures.append(
            "architecture "
            f"{environment.get('architecture')!r} is not one of "
            f"{requirements['architectures']}"
        )
    cpu_contains = requirements.get("cpu_model_contains")
    cpu_model = environment.get("cpu_model")
    if cpu_contains is not None and (
        not isinstance(cpu_model, str) or cpu_contains.lower() not in cpu_model.lower()
    ):
        failures.append(
            f"CPU model {cpu_model!r} does not contain {cpu_contains!r}"
        )
    minimum_cpus = requirements.get("minimum_logical_cpus")
    actual_cpus = environment.get("logical_cpu_count")
    if minimum_cpus is not None and (
        not isinstance(actual_cpus, int) or actual_cpus < minimum_cpus
    ):
        failures.append(
            f"logical CPU count {actual_cpus!r} is below {minimum_cpus}"
        )
    for requirement, field in (
        ("minimum_memory_gib", "memory_bytes"),
        ("minimum_temporary_free_gib", "temporary_free_bytes"),
    ):
        minimum_gib = requirements.get(requirement)
        actual = environment.get(field)
        minimum_bytes = (
            None if minimum_gib is None else int(float(minimum_gib) * 1024**3)
        )
        if minimum_bytes is not None and (
            not isinstance(actual, int) or actual < minimum_bytes
        ):
            failures.append(
                f"{field} {actual!r} is below required {minimum_bytes} bytes"
            )
    expected_github = requirements.get("github_actions")
    if expected_github is not None and environment.get("github_actions") != expected_github:
        failures.append(
            f"github_actions={environment.get('github_actions')!r} "
            f"does not match required {expected_github!r}"
        )
    for requirement, field in (
        ("github_runner_os", "github_runner_os"),
        ("github_runner_arch", "github_runner_arch"),
        ("github_image_os", "github_image_os"),
    ):
        expected = requirements.get(requirement)
        actual = environment.get(field)
        if expected is not None and actual != expected:
            failures.append(f"{field}={actual!r} does not match required {expected!r}")
    return failures


def selected_throughput_cases(profile: dict[str, Any], tier: str) -> list[dict[str, Any]]:
    if tier not in TIERS:
        raise PerformanceProfileError(f"unknown profile tier {tier!r}")
    cases = [case for case in profile["throughput"]["cases"] if tier in case["tiers"]]
    if not cases:
        raise PerformanceProfileError(f"profile has no throughput cases for tier {tier}")
    return cases


def selected_application_cases(profile: dict[str, Any], tier: str) -> list[dict[str, Any]]:
    if tier not in TIERS:
        raise PerformanceProfileError(f"unknown profile tier {tier!r}")
    cases = [
        case
        for case in profile["application_observation"]["cases"]
        if tier in case["tiers"]
    ]
    if not cases:
        raise PerformanceProfileError(f"profile has no application cases for tier {tier}")
    if not any(case["mode"] == "idle" for case in cases):
        raise PerformanceProfileError(
            f"profile tier {tier} must include idle application control"
        )
    return cases


def profile_runs(section: dict[str, Any], tier: str) -> int:
    if tier not in TIERS:
        raise PerformanceProfileError(f"unknown profile tier {tier!r}")
    return int(section[f"{tier}_runs"])
