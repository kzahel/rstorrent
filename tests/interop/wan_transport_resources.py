#!/usr/bin/env python3
"""Process ownership wrapper for Tactical 142 resource samplers."""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from wan_transport_resource_sampler import process_sample


ROOT = Path(__file__).resolve().parents[2]
INTEROP = Path(__file__).resolve().parent
LOCAL_PYTHON = INTEROP / ".venv/bin/python"
LOCAL_SAMPLER = INTEROP / "wan_transport_resource_sampler.py"
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
REMOTE_PYTHON = (
    "$HOME/.local/share/rstorrent-oracles/"
    "libtorrent-2.0.13-py313-aarch64/bin/python"
)
REMOTE_SAMPLER = (
    "$HOME/.local/share/rstorrent-oracles/tactical-142/"
    "source/tests/interop/wan_transport_resource_sampler.py"
)


class ResourceError(RuntimeError):
    pass


@dataclass
class ResourceSampler:
    process: subprocess.Popen[str]
    location: str
    initial_sample: tuple[int, float] | None = None

    @classmethod
    def local(cls, pid: int, max_seconds: int) -> "ResourceSampler":
        initial = process_sample(pid)
        return cls._start(
            [
                str(LOCAL_PYTHON),
                str(LOCAL_SAMPLER),
                "--pid",
                str(pid),
                "--max-seconds",
                str(max_seconds),
            ],
            "local",
            initial_sample=initial,
        )

    @classmethod
    def remote(
        cls, host: str, pid: int, max_seconds: int
    ) -> "ResourceSampler":
        command = (
            f'exec "{REMOTE_PYTHON}" "{REMOTE_SAMPLER}" '
            f"--pid {pid} --max-seconds {max_seconds}"
        )
        return cls._start(["ssh", *SSH_OPTIONS, host, command], "remote")

    @classmethod
    def _start(
        cls,
        command: list[str],
        location: str,
        *,
        initial_sample: tuple[int, float] | None = None,
    ) -> "ResourceSampler":
        process = subprocess.Popen(
            command,
            cwd=ROOT,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        return cls(process, location, initial_sample)

    def finish(self) -> dict[str, Any]:
        try:
            stdout, _stderr = self.process.communicate(timeout=45)
        except subprocess.TimeoutExpired as error:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
            raise ResourceError(f"{self.location} resource sampler did not join") from error
        if self.process.returncode != 0:
            raise ResourceError(f"{self.location} resource sampler failed")
        try:
            result = json.loads(stdout)
        except json.JSONDecodeError as error:
            raise ResourceError(f"{self.location} resource sampler emitted invalid JSON") from error
        if not isinstance(result, dict) or result.get("schema_version") != 1:
            raise ResourceError(f"{self.location} resource sampler contract failed")
        if not isinstance(result.get("samples"), int):
            raise ResourceError(f"{self.location} resource sampler sample count is invalid")
        if result["samples"] < 1 and self.initial_sample is None:
            raise ResourceError(f"{self.location} resource sampler captured no process sample")
        if self.initial_sample is not None:
            rss_kib, cpu_percent = self.initial_sample
            process = result.get("process")
            if not isinstance(process, dict):
                raise ResourceError(f"{self.location} resource sampler omitted process evidence")
            process["rss_high_water_kib"] = max(process["rss_high_water_kib"], rss_kib)
            process["cpu_percent_high_water"] = max(
                process["cpu_percent_high_water"], cpu_percent
            )
            if result["samples"] < 1:
                result["samples"] = 1
                process["cpu_percent_mean"] = cpu_percent
        result["location"] = self.location
        return result

    def cleanup(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
