#!/usr/bin/env python3
"""Exercise the leased polling view set against a controlled libtorrent seed."""

from __future__ import annotations

import gc
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import libtorrent as lt

from first_verified_piece import ScenarioFailure, add_seed, create_session, wait_for_listener
from gateway_reactive_surface import (
    ORIGIN,
    TOKEN,
    UPLOAD_RATE_LIMIT,
    build_gateway,
    start_gateway,
    stop_gateway,
    verify_payload,
)
from magnet_metadata import create_fixture, magnet_uri


def run_typescript_client(
    repository: Path,
    address: str,
    magnet: str,
    torrent_id: str,
) -> str:
    environment = os.environ.copy()
    environment.update(
        {
            "RSTORRENT_INTEROP_GATEWAY_URL": f"http://{address}",
            "RSTORRENT_INTEROP_GATEWAY_ORIGIN": ORIGIN,
            "RSTORRENT_INTEROP_GATEWAY_TOKEN": TOKEN,
            "RSTORRENT_INTEROP_MAGNET": magnet,
            "RSTORRENT_INTEROP_TORRENT_ID": torrent_id,
        }
    )
    completed = subprocess.run(
        [
            "npm",
            "test",
            "--prefix",
            "clients/web",
            "--",
            "src/view-set-interop.test.ts",
            "--disableConsoleIntercept",
        ],
        cwd=repository,
        capture_output=True,
        text=True,
        env=environment,
        timeout=60,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "TypeScript view-set client failed\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    if "view_set_interop" not in completed.stdout:
        raise ScenarioFailure(
            "TypeScript view-set client did not record its live trace\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    return next(
        line.strip()
        for line in completed.stdout.splitlines()
        if "view_set_interop" in line
    )


def run() -> None:
    repository = Path(__file__).resolve().parents[2]
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-gateway-view-set-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    gateway: subprocess.Popen[str] | None = None
    diagnostics: list[str] = []
    failure: BaseException | None = None
    try:
        fixture = create_fixture(run_path)
        session = create_session()
        session.apply_settings({"upload_rate_limit": UPLOAD_RATE_LIMIT})
        port = wait_for_listener(session, diagnostics)
        handle = add_seed(session, fixture.torrent_info, fixture.seed_directory, diagnostics)
        binary = build_gateway(repository)
        storage = run_path / "downloads"
        gateway, address = start_gateway(binary, run_path / "profile", storage)
        trace = run_typescript_client(
            repository,
            address,
            magnet_uri(fixture.info_hash, f"127.0.0.1:{port}"),
            fixture.info_hash,
        )
        verify_payload(storage, fixture.info_hash, fixture.payload_hash)
        stop_gateway(gateway)
        gateway = None
        print(
            f"{trace} metadata_size={len(fixture.info_bytes)} pieces=3 "
            f"payload_sha1={fixture.payload_hash} gateway_shutdown=joined cleanup=ok"
        )
    except BaseException as error:
        failure = error
        raise
    finally:
        if gateway is not None:
            try:
                stop_gateway(gateway)
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"gateway cleanup failed: {cleanup_error}", file=sys.stderr)
        if session is not None:
            if handle is not None:
                try:
                    session.remove_torrent(handle)
                except RuntimeError:
                    pass
            session.pause()
            session.pop_alerts()
        handle = None
        session = None
        gc.collect()
        shutil.rmtree(run_path, ignore_errors=True)


def main() -> int:
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    try:
        run()
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
