#!/usr/bin/env python3
"""Run production RSTorrent uTP roles through a bounded 160 ms RTT relay."""

from __future__ import annotations

import json
import shutil
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

from public_compare_contract import parse_metainfo, verify_payload
from utp_runtime_impairment import DeterministicUdpRelay, LONG_RTT_PROFILE
from wan_transport_fixture import create_fixture
from wan_transport_roles_controlled import (
    ControlledRoleError,
    RoleProcess,
    build_binaries,
    run_leecher,
    start_seed,
    stop_seed,
)


SIZE_MIB = 64
CASE_TIMEOUT_SECONDS = 5 * 60
MINIMUM_MIB_PER_SECOND = 0.35
ZERO_TERMINAL_FIELDS = (
    "connection_datagrams_dropped",
    "retry_exhausted_connections",
    "worker_panics",
)
MAX_CONNECTION_DATAGRAM_QUEUE = 256


class LongRttProductError(ControlledRoleError):
    pass


def require_clean_single_connection(label: str, evidence: dict[str, Any]) -> None:
    if evidence.get("connections_started") != 1:
        raise LongRttProductError(f"{label} did not retain one uTP connection")
    nonzero = {
        field: evidence.get(field)
        for field in ZERO_TERMINAL_FIELDS
        if evidence.get(field) != 0
    }
    if nonzero:
        raise LongRttProductError(f"{label} has terminal uTP failures: {nonzero}")


def run() -> dict[str, Any]:
    binaries = build_binaries()
    started = time.monotonic()
    with tempfile.TemporaryDirectory(
        prefix="rstorrent-utp-long-rtt-product-"
    ) as temporary:
        root = Path(temporary)
        fixture = create_fixture(root / "fixture", SIZE_MIB)
        case_root = root / "case"
        case_root.mkdir()
        output_root = case_root / "leech"
        deadline = time.monotonic() + CASE_TIMEOUT_SECONDS
        seed: RoleProcess | None = None
        relay: DeterministicUdpRelay | None = None
        try:
            seed, seed_address, seed_port, ready = start_seed(
                "rstorrent",
                "utp",
                fixture,
                case_root,
                binaries,
                deadline,
            )
            relay = DeterministicUdpRelay(
                (seed_address, seed_port), LONG_RTT_PROFILE
            )
            relay.start()
            transfer_started = time.monotonic()
            leech = run_leecher(
                "rstorrent",
                "utp",
                fixture,
                output_root,
                relay.endpoint[0],
                relay.endpoint[1],
                binaries,
                deadline,
                collect_resources=True,
            )
            active_seconds = time.monotonic() - transfer_started
            verification = verify_payload(
                parse_metainfo(fixture.metainfo.read_bytes()), output_root
            )
            stopped = stop_seed(seed, "rstorrent", "utp", fixture, deadline)
            seed = None
            relay.wait_idle(1.0)
            relay_snapshot = relay.snapshot()

            leech_utp = leech["rstorrent"].get("utp_evidence") or {}
            seed_utp = stopped.get("utp_before_shutdown") or {}
            require_clean_single_connection("leecher", leech_utp)
            require_clean_single_connection("seed", seed_utp)
            if (
                leech_utp.get("connection_datagram_queue_high_water", 0)
                > MAX_CONNECTION_DATAGRAM_QUEUE
            ):
                raise LongRttProductError(
                    "leecher exceeded the bounded per-connection datagram queue"
                )
            mib_per_second = SIZE_MIB / active_seconds
            if mib_per_second < MINIMUM_MIB_PER_SECOND:
                raise LongRttProductError(
                    f"production uTP rate remained low at {mib_per_second:.3f} MiB/s"
                )
            if relay_snapshot["dropped_datagrams"] != 0:
                raise LongRttProductError("clean long-RTT relay dropped traffic")
            if (
                verification["logical_bytes"] != fixture.payload_bytes
                or verification["piece_count"] != fixture.piece_count
            ):
                raise LongRttProductError("production payload verification is incomplete")

            shutil.rmtree(output_root)
            return {
                "schema_version": 1,
                "profile": "rstorrent-product-utp-long-rtt-160ms",
                "payload": {
                    "bytes": fixture.payload_bytes,
                    "pieces": fixture.piece_count,
                    "sha1": fixture.sha1,
                },
                "timing": {
                    "active_seconds": round(active_seconds, 6),
                    "mib_per_second": round(mib_per_second, 6),
                },
                "seed_ready": ready,
                "seed_terminal": stopped,
                "leech_terminal": leech,
                "relay": relay_snapshot,
                "cleanup": {
                    "succeeded": not output_root.exists(),
                    "seed_joined": True,
                    "leech_joined": True,
                },
                "seconds": round(time.monotonic() - started, 6),
            }
        finally:
            if seed is not None:
                seed.cleanup()
            if relay is not None:
                terminal = relay.stop()
                if terminal["queued_datagrams"] or terminal["queued_bytes"]:
                    raise LongRttProductError(
                        "relay retained terminal queue ownership"
                    )
            if output_root.exists():
                shutil.rmtree(output_root, ignore_errors=True)


def main() -> int:
    print(json.dumps(run(), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (LongRttProductError, OSError, RuntimeError) as error:
        print(f"long-RTT product uTP failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
