#!/usr/bin/env python3
"""Exercise the focused direct driver through authenticated HTTPS trackers."""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
from pathlib import Path
from typing import Any

import libtorrent as lt

from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    compare_payloads,
    create_session,
    wait_for_listener,
)
from http_tracker_application import ControlledHttpTracker
from magnet_metadata import create_fixture


def run_test(
    repository: Path,
    test_name: str,
    environment: dict[str, str],
) -> None:
    completed = subprocess.run(
        [
            "cargo",
            "test",
            "-p",
            "rstorrent-engine",
            "--features",
            "test-platform-root",
            test_name,
            "--",
            "--ignored",
            "--exact",
        ],
        cwd=repository,
        env=environment,
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            f"direct HTTPS test failed: {test_name}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )


def run_authenticated(repository: Path, root: Path) -> dict[str, Any]:
    fixture = create_fixture(root)
    diagnostics: list[str] = []
    session = create_session()
    handle: lt.torrent_handle | None = None
    tracker: ControlledHttpTracker | None = None
    try:
        peer_port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            fixture.torrent_info,
            fixture.seed_directory,
            diagnostics,
        )
        tracker = ControlledHttpTracker(
            fixture.info_hash,
            peer_port,
            https=True,
            certificate_root=root / "tls",
            trusted_chain=True,
        )
        tracker.start()
        if tracker.root_certificate is None:
            raise ScenarioFailure("controlled trusted root is unavailable")
        output_root = root / "output"
        environment = os.environ.copy()
        environment.update(
            {
                "RSTORRENT_INTEROP_TRACKER_URL": tracker.url,
                "RSTORRENT_INTEROP_INFO_HASH": fixture.info_hash,
                "RSTORRENT_INTEROP_ROOT_PEM": str(tracker.root_certificate),
                "RSTORRENT_INTEROP_DIRECT_ROOT": str(output_root),
            }
        )
        run_test(
            repository,
            "driver::tests::discovery_metadata::authenticated_https_tracker_introduces_pinned_libtorrent_peer",
            environment,
        )
        for event in ("started", "completed", "stopped"):
            tracker.wait_for_event(event)
        output = output_root / fixture.torrent_info.name() / "payload.bin"
        payload_hash = compare_payloads(fixture.payload_path, output)
        if payload_hash != fixture.payload_hash:
            raise ScenarioFailure("direct HTTPS payload differs from libtorrent seed")
        return {
            "info_hash": fixture.info_hash,
            "payload_sha1": payload_hash,
            "events": list(tracker.events),
            "requests": len(tracker.requests),
        }
    finally:
        if tracker is not None:
            tracker.close()
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()


def run_rejected(repository: Path, root: Path) -> dict[str, Any]:
    tracker = ControlledHttpTracker(
        "7c" * 20,
        1,
        https=True,
        certificate_root=root / "tls",
    )
    try:
        tracker.start()
        environment = os.environ.copy()
        environment["RSTORRENT_INTEROP_TRACKER_URL"] = tracker.url
        run_test(
            repository,
            "driver::tests::discovery_metadata::system_trust_rejects_untrusted_https_before_http",
            environment,
        )
        if tracker.requests or tracker.events:
            raise ScenarioFailure(
                "untrusted direct HTTPS reached an accepted HTTP announce"
            )
        return {"http_requests": 0, "accepted_events": []}
    finally:
        tracker.close()


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    with tempfile.TemporaryDirectory(prefix="rstorrent-direct-https-") as temporary:
        root = Path(temporary)
        result = {
            "authenticated": run_authenticated(repository, root / "authenticated"),
            "untrusted": run_rejected(repository, root / "untrusted"),
            "libtorrent_binding": lt.__version__,
            "libtorrent_native": lt.version,
        }
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
