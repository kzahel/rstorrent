#!/usr/bin/env python3
"""Bounded process and endpoint sampler used by Tactical 142 roles."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


MAX_SAMPLES = 43_260


class SamplerError(RuntimeError):
    pass


def process_sample(pid: int) -> tuple[int, float] | None:
    completed = subprocess.run(
        ["ps", "-o", "rss=", "-o", "%cpu=", "-o", "state=", "-p", str(pid)],
        capture_output=True,
        text=True,
        timeout=2,
        check=False,
    )
    if completed.returncode != 0 or not completed.stdout.strip():
        return None
    fields = completed.stdout.split()
    if len(fields) != 3:
        raise SamplerError("process sampler received malformed ps output")
    if fields[2].startswith("Z"):
        return None
    return int(fields[0]), float(fields[1].replace(",", "."))


def linux_endpoint_counters() -> dict[str, int] | None:
    stat = Path("/proc/stat")
    diskstats = Path("/proc/diskstats")
    if not stat.is_file() or not diskstats.is_file():
        return None
    cpu = stat.read_text().splitlines()[0].split()
    if len(cpu) < 6 or cpu[0] != "cpu":
        raise SamplerError("Linux CPU counters are malformed")
    sectors_read = 0
    sectors_written = 0
    block_names = {path.name for path in Path("/sys/block").iterdir()}
    for line in diskstats.read_text().splitlines():
        fields = line.split()
        if len(fields) < 14:
            continue
        name = fields[2]
        if name not in block_names or name.startswith(("loop", "ram", "zram")):
            continue
        sectors_read += int(fields[5])
        sectors_written += int(fields[9])
    return {
        "cpu_iowait_ticks": int(cpu[5]),
        "disk_sectors_read": sectors_read,
        "disk_sectors_written": sectors_written,
    }


def thermal_millicelsius() -> int | None:
    values: list[int] = []
    for path in Path("/sys/class/thermal").glob("thermal_zone*/temp"):
        try:
            value = int(path.read_text().strip())
        except (OSError, ValueError):
            continue
        if 0 < value < 200_000:
            values.append(value)
    return max(values, default=None)


def sample(pid: int, interval_seconds: float, max_seconds: int) -> dict[str, Any]:
    if pid <= 1 or not 0.25 <= interval_seconds <= 60 or not 1 <= max_seconds <= 43_260:
        raise SamplerError("sampler bounds are invalid")
    started = time.monotonic()
    endpoint_start = linux_endpoint_counters()
    samples = 0
    rss_high_water_kib = 0
    cpu_percent_high_water = 0.0
    cpu_percent_sum = 0.0
    load_one_high_water = 0.0
    thermal_high_water = thermal_millicelsius()
    while samples < MAX_SAMPLES and time.monotonic() - started <= max_seconds:
        current = process_sample(pid)
        if current is None:
            break
        rss_kib, cpu_percent = current
        samples += 1
        rss_high_water_kib = max(rss_high_water_kib, rss_kib)
        cpu_percent_high_water = max(cpu_percent_high_water, cpu_percent)
        cpu_percent_sum += cpu_percent
        try:
            load_one_high_water = max(load_one_high_water, os.getloadavg()[0])
        except OSError:
            pass
        temperature = thermal_millicelsius()
        if temperature is not None:
            thermal_high_water = max(thermal_high_water or 0, temperature)
        time.sleep(interval_seconds)
    endpoint_end = linux_endpoint_counters()
    endpoint_delta = None
    if endpoint_start is not None and endpoint_end is not None:
        endpoint_delta = {
            name: max(0, endpoint_end[name] - endpoint_start[name])
            for name in endpoint_start
        }
    return {
        "schema_version": 1,
        "samples": samples,
        "interval_seconds": interval_seconds,
        "wall_seconds": round(time.monotonic() - started, 6),
        "process": {
            "rss_high_water_kib": rss_high_water_kib,
            "cpu_percent_high_water": round(cpu_percent_high_water, 3),
            "cpu_percent_mean": round(cpu_percent_sum / samples, 3) if samples else None,
        },
        "endpoint": {
            "load_one_high_water": round(load_one_high_water, 3),
            "thermal_high_water_millicelsius": thermal_high_water,
            "linux_counter_delta": endpoint_delta,
        },
    }


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--interval-seconds", type=float, default=1.0)
    parser.add_argument("--max-seconds", type=int, required=True)
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    try:
        print(
            json.dumps(
                sample(arguments.pid, arguments.interval_seconds, arguments.max_seconds),
                sort_keys=True,
            )
        )
        return 0
    except (OSError, subprocess.SubprocessError, SamplerError) as error:
        print(f"WAN resource sampler failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
