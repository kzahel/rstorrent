#!/usr/bin/env python3
"""Render a scheduled tracker retry and diagnostics in headless Chrome."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from browser_reactive_surface import (
    discover_chrome,
    reserve_loopback_port,
    run_chrome,
    start_vite,
    stop_process,
)
from first_verified_piece import ScenarioFailure
from gateway_reactive_surface import TOKEN, build_gateway, start_gateway, stop_gateway


TORRENT_ID = "000102030405060708090a0b0c0d0e0f10111213"
MAGNET = (
    f"magnet:?xt=urn:btih:{TORRENT_ID}"
    "&tr=udp%3A%2F%2F192.0.2.1%3A6969%2Fannounce"
)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chrome", type=Path)
    parser.add_argument("--screenshot", type=Path)
    parser.add_argument("--dom-output", type=Path)
    return parser.parse_args()


def run(
    chrome: Path,
    screenshot: Path | None,
    dom_output: Path | None,
) -> None:
    repository = Path(__file__).resolve().parents[2]
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-browser-diagnostics-"))
    gateway: subprocess.Popen[str] | None = None
    vite: subprocess.Popen[str] | None = None
    failure: BaseException | None = None
    try:
        vite_port = reserve_loopback_port()
        origin = f"http://127.0.0.1:{vite_port}"
        gateway, address = start_gateway(
            build_gateway(repository),
            run_path / "profile",
            run_path / "downloads",
            origin,
        )
        environment = os.environ.copy()
        environment.update(
            {
                "VITE_RSTORRENT_INTEROP_MAGNET": MAGNET,
                "VITE_RSTORRENT_INTEROP_GATEWAY_URL": f"ws://{address}/control",
                "VITE_RSTORRENT_INTEROP_GATEWAY_TOKEN": TOKEN,
                "VITE_RSTORRENT_INTEROP_EXPECT_TRACKER_RETRY": "1",
            }
        )
        vite = start_vite(repository, environment, origin, vite_port)
        result = run_chrome(
            chrome,
            run_path / "chrome-profile",
            repository,
            origin,
            screenshot,
            "retry",
        )
        if dom_output is not None:
            dom_output.parent.mkdir(parents=True, exist_ok=True)
            dom_output.write_text(result["html"], encoding="utf-8")
        stop_process(vite, "Vite")
        vite = None
        stop_gateway(gateway)
        gateway = None
        print(
            "browser=chrome scenario=tracker_retry "
            f"info_hash={TORRENT_ID} progress={result['progress']} "
            f"reason={result['reason']} diagnostic=tracker_retry_scheduled "
            "ui_filters=profile,category public_socket=none "
            f"origin={origin} screenshot={screenshot or 'disabled'} cleanup=ok"
        )
    except BaseException as error:
        failure = error
        raise
    finally:
        if vite is not None:
            try:
                stop_process(vite, "Vite")
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"Vite cleanup failed: {cleanup_error}", file=sys.stderr)
        if gateway is not None:
            try:
                stop_gateway(gateway)
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"gateway cleanup failed: {cleanup_error}", file=sys.stderr)
        shutil.rmtree(run_path, ignore_errors=True)


def main() -> int:
    arguments = parse_arguments()
    try:
        run(
            discover_chrome(arguments.chrome),
            arguments.screenshot.resolve() if arguments.screenshot else None,
            arguments.dom_output.resolve() if arguments.dom_output else None,
        )
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
