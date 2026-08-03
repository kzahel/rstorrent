#!/usr/bin/env python3

from __future__ import annotations

import tempfile
import textwrap
import unittest
from pathlib import Path

from performance_profiles import (
    PerformanceProfileError,
    load_performance_profile,
    profile_runs,
    selected_application_cases,
    selected_throughput_cases,
    validate_environment,
)


PROFILE = """
schema_version = 1
profile_id = "fixture"
description = "test profile"
calibration_status = "calibrated"

[requirements]
systems = ["darwin"]
architectures = ["arm64"]
cpu_model_contains = "Example CPU"
minimum_logical_cpus = 4
minimum_memory_gib = 8
minimum_temporary_free_gib = 4
github_actions = false

[throughput]
smoke_runs = 1
full_runs = 3
timeout_seconds = 300

[[throughput.cases]]
id = "one-gib"
tiers = ["smoke", "full"]
size_mib = 1024
piece_size_kib = 256
write_concurrency = 4
hash_concurrency = 4

[throughput.cases.observed]
median_mib_s = 500.0
runs = 3
commit = "abc123"

[throughput.cases.required]
minimum_mib_s = 250.0
minimum_libtorrent_ratio = 0.5

[application_observation]
size_mib = 512
piece_size_kib = 256
smoke_runs = 1
full_runs = 3
timeout_seconds = 300
slow_consumer_delay_millis = 1000

[[application_observation.cases]]
id = "idle"
mode = "idle"
tiers = ["smoke", "full"]

[application_observation.cases.required]
minimum_mib_s = 20.0

[[application_observation.cases]]
id = "all"
mode = "all"
tiers = ["smoke", "full"]

[application_observation.cases.required]
minimum_idle_ratio = 0.25
maximum_resets = 0

[[application_observation.cases]]
id = "disk"
mode = "disk"
tiers = ["full"]

[application_observation.cases.required]
minimum_idle_ratio = 0.5
"""


class PerformanceProfileTests(unittest.TestCase):
    def load(self, source: str = PROFILE):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        repository = Path(temporary.name)
        profile_root = repository / "tests" / "perf" / "baselines"
        profile_root.mkdir(parents=True)
        (profile_root / "fixture.toml").write_text(
            textwrap.dedent(source), encoding="utf-8"
        )
        profile, path = load_performance_profile(repository, "fixture")
        self.assertEqual(path, (profile_root / "fixture.toml").resolve())
        return profile

    def test_selects_tiers_and_runs(self) -> None:
        profile = self.load()
        self.assertEqual(
            [case["id"] for case in selected_throughput_cases(profile, "smoke")],
            ["one-gib"],
        )
        self.assertEqual(
            [case["id"] for case in selected_application_cases(profile, "smoke")],
            ["idle", "all"],
        )
        self.assertEqual(
            [case["id"] for case in selected_application_cases(profile, "full")],
            ["idle", "all", "disk"],
        )
        self.assertEqual(profile_runs(profile["throughput"], "full"), 3)

    def test_committed_profiles_load_and_have_adversarial_smoke(self) -> None:
        repository = Path(__file__).resolve().parents[2]
        for profile_id in (
            "kmacbook-m4pro",
            "github-ubuntu-24.04-x64",
        ):
            with self.subTest(profile_id=profile_id):
                profile, _ = load_performance_profile(repository, profile_id)
                modes = {
                    case["mode"]
                    for case in selected_application_cases(profile, "smoke")
                }
                self.assertEqual(modes, {"idle", "all", "slow-all"})

    def test_environment_validation_is_fail_closed(self) -> None:
        profile = self.load()
        environment = {
            "system": "darwin",
            "architecture": "arm64",
            "cpu_model": "Example CPU 1",
            "logical_cpu_count": 8,
            "memory_bytes": 16 * 1024**3,
            "temporary_free_bytes": 10 * 1024**3,
            "github_actions": False,
            "github_runner_os": None,
            "github_runner_arch": None,
            "github_image_os": None,
        }
        self.assertEqual(validate_environment(profile, environment), [])
        environment["architecture"] = "x86_64"
        environment["temporary_free_bytes"] = 1
        failures = validate_environment(profile, environment)
        self.assertTrue(any("architecture" in failure for failure in failures))
        self.assertTrue(any("temporary_free_bytes" in failure for failure in failures))

    def test_rejects_duplicate_geometry(self) -> None:
        duplicate = PROFILE.replace(
            "[application_observation]",
            """
[[throughput.cases]]
id = "duplicate"
tiers = ["full"]
size_mib = 1024
piece_size_kib = 256
write_concurrency = 4
hash_concurrency = 4
[throughput.cases.required]
minimum_mib_s = 1.0

[application_observation]
""",
        )
        with self.assertRaisesRegex(PerformanceProfileError, "duplicate throughput geometry"):
            self.load(duplicate)

    def test_rejects_application_tier_without_idle_control(self) -> None:
        source = PROFILE.replace('tiers = ["smoke", "full"]\n\n[application_observation.cases.required]\nminimum_mib_s = 20.0', 'tiers = ["smoke"]\n\n[application_observation.cases.required]\nminimum_mib_s = 20.0', 1)
        profile = self.load(source)
        with self.assertRaisesRegex(PerformanceProfileError, "idle application control"):
            selected_application_cases(profile, "full")


if __name__ == "__main__":
    unittest.main()
