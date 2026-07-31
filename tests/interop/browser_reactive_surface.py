#!/usr/bin/env python3
"""Exercise the rendered browser surface against a controlled libtorrent seed."""

from __future__ import annotations

import argparse
import gc
import json
import os
import selectors
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
from urllib.parse import urlparse
from pathlib import Path

import libtorrent as lt

from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    create_session,
    wait_for_listener,
)
from gateway_reactive_surface import (
    TOKEN,
    UPLOAD_RATE_LIMIT,
    build_gateway,
    start_gateway,
    stop_gateway,
    verify_payload,
)
from magnet_metadata import create_fixture, magnet_uri


MACOS_CHROME = Path("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
LINUX_CHROME = Path("/usr/bin/google-chrome")


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--chrome",
        type=Path,
        help=(
            "headless Chrome executable; standard macOS/Linux paths are "
            "autodetected"
        ),
    )
    parser.add_argument(
        "--screenshot",
        type=Path,
        help="write a full-page PNG after the controlled UI reaches completion",
    )
    parser.add_argument(
        "--dom-output",
        type=Path,
        help="write the final rendered HTML for retained test evidence",
    )
    return parser.parse_args()


def discover_chrome(configured: Path | None) -> Path:
    candidates = (
        [configured]
        if configured is not None
        else [
            Path(os.environ["CHROME"]) if "CHROME" in os.environ else None,
            MACOS_CHROME,
            LINUX_CHROME,
        ]
    )
    for candidate in candidates:
        if (
            candidate is not None
            and candidate.is_file()
            and os.access(candidate, os.X_OK)
        ):
            return candidate.resolve()
    rendered = ", ".join(
        str(candidate) for candidate in candidates if candidate is not None
    )
    raise ScenarioFailure(f"headless Chrome is unavailable; checked: {rendered}")


def reserve_loopback_port() -> int:
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])
    finally:
        listener.close()


def start_vite(
    repository: Path,
    environment: dict[str, str],
    origin: str,
    port: int,
) -> subprocess.Popen[str]:
    process = subprocess.Popen(
        [
            "npm",
            "run",
            "dev",
            "--prefix",
            "clients/web",
            "--",
            "--host",
            "127.0.0.1",
            "--port",
            str(port),
            "--strictPort",
        ],
        cwd=repository,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        env=environment,
        start_new_session=True,
    )
    if process.stdout is None:
        raise ScenarioFailure("Vite output is unavailable")
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    deadline = time.monotonic() + 10
    diagnostics: list[str] = []
    try:
        while time.monotonic() < deadline:
            try:
                with urllib.request.urlopen(origin, timeout=0.2) as response:
                    if response.status == 200:
                        return process
            except OSError:
                pass
            if selector.select(0.1):
                line = process.stdout.readline()
                if not line:
                    break
                diagnostics.append(line.rstrip())
    finally:
        selector.close()
    stop_process(process, "Vite")
    raise ScenarioFailure("Vite did not start\n" + "\n".join(diagnostics))


def stop_process(process: subprocess.Popen[str], label: str) -> None:
    if process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    try:
        process.communicate(timeout=5)
    except subprocess.TimeoutExpired:
        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        process.communicate(timeout=5)
        raise ScenarioFailure(f"{label} did not terminate")


def run_chrome(
    chrome: Path,
    profile: Path,
    repository: Path,
    origin: str,
    screenshot: Path | None,
    scenario: str = "transfer",
) -> dict[str, str]:
    profile.mkdir(parents=True)
    chrome_log = profile.parent / "chrome.log"
    log_handle = chrome_log.open("w", encoding="utf-8")
    command = [
        str(chrome),
        "--headless=new",
        "--disable-background-networking",
        "--disable-component-update",
        "--disable-default-apps",
        "--disable-dev-shm-usage",
        "--disable-gpu",
        "--disable-sync",
        "--metrics-recording-only",
        "--no-default-browser-check",
        "--no-first-run",
        "--remote-debugging-port=0",
        "--remote-allow-origins=*",
        "--window-size=1440,1000",
        f"--user-data-dir={profile}",
        origin,
    ]
    if hasattr(os, "geteuid") and os.geteuid() == 0:
        command.insert(1, "--no-sandbox")
    process = subprocess.Popen(
        command,
        stdout=subprocess.DEVNULL,
        stderr=log_handle,
        text=True,
        start_new_session=True,
    )
    log_handle.close()
    failure: BaseException | None = None
    try:
        deadline = time.monotonic() + 10
        browser_url: str | None = None
        while time.monotonic() < deadline:
            if process.poll() is not None:
                break
            contents = chrome_log.read_text(encoding="utf-8", errors="replace")
            for line in contents.splitlines():
                prefix = "DevTools listening on "
                if line.startswith(prefix):
                    browser_url = line[len(prefix) :].strip()
                    break
            if browser_url is not None:
                break
            time.sleep(0.05)
        if browser_url is None:
            raise ScenarioFailure(
                "Chrome did not announce DevTools\n"
                + chrome_log.read_text(encoding="utf-8", errors="replace")[-4_000:]
            )
        port = urlparse(browser_url).port
        if port is None:
            raise ScenarioFailure(
                f"Chrome reported an invalid DevTools URL: {browser_url}"
            )
        list_url = f"http://127.0.0.1:{port}/json/list"
        target_deadline = time.monotonic() + 5
        target: dict[str, str] | None = None
        while time.monotonic() < target_deadline:
            try:
                with urllib.request.urlopen(list_url, timeout=0.5) as response:
                    targets = json.load(response)
                target = next(
                    (
                        item
                        for item in targets
                        if item.get("type") == "page"
                        and item.get("url", "").startswith(origin)
                    ),
                    None,
                )
                if target is not None:
                    break
            except (OSError, ValueError):
                pass
            time.sleep(0.05)
        if target is None:
            raise ScenarioFailure("Chrome page target did not appear")

        observer_command = [
            "node",
            str(repository / "tests/interop/chrome_wait.mjs"),
            target["webSocketDebuggerUrl"],
            "45000",
        ]
        if screenshot is not None:
            screenshot.parent.mkdir(parents=True, exist_ok=True)
        observer_command.extend(
            [str(screenshot) if screenshot is not None else "", scenario]
        )
        completed = subprocess.run(
            observer_command,
            capture_output=True,
            text=True,
            timeout=50,
            check=False,
        )
        chrome_stderr = chrome_log.read_text(encoding="utf-8", errors="replace")
        if completed.returncode != 0:
            raise ScenarioFailure(
                "headless browser observer exited with status "
                f"{completed.returncode}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}\n"
                f"Chrome diagnostics:\n{chrome_stderr}"
            )
        result = json.loads(completed.stdout)
        if result.get("browserExceptions"):
            raise ScenarioFailure(
                "headless browser reported page exceptions: "
                + json.dumps(result["browserExceptions"])
            )
        if 'id="interop-result"' not in result["html"]:
            raise ScenarioFailure(
                "rendered browser surface did not reach controlled completion\n"
                f"DOM:\n{result['html'][-4000:]}\n"
                f"Chrome diagnostics:\n{chrome_stderr[-4000:]}"
            )
        if scenario == "transfer":
            for field in ("requested", "received", "stored"):
                if int(result[field]) <= 0:
                    raise ScenarioFailure(
                        f"browser did not render positive {field} bytes"
                    )
            if result["control"] != "resumed":
                raise ScenarioFailure(
                    "browser did not render pause/resume completion"
                )
            if not result.get("pauseClicked") or not result.get("resumeClicked"):
                raise ScenarioFailure(
                    "headless browser did not click both rendered transfer controls"
                )
        elif (
            result.get("progress") != "blocked"
            or result.get("reason") != "no_enabled_discovery_source"
            or "discovery_exhausted" not in result.get("diagnosticCodes", [])
        ):
            raise ScenarioFailure(
                "browser did not render the blocked discovery assessment"
            )
        if not result.get("profileClicked") or not result.get("categoryClicked"):
            raise ScenarioFailure(
                "headless browser did not exercise diagnostic profile and category controls"
            )
        if screenshot is not None and not screenshot.is_file():
            raise ScenarioFailure(
                "headless browser did not create the requested screenshot"
            )
        return result
    except BaseException as error:
        failure = error
        raise
    finally:
        try:
            stop_process(process, "Chrome")
        except BaseException as cleanup_error:
            if failure is None:
                raise
            print(f"Chrome cleanup failed: {cleanup_error}", file=sys.stderr)


def run(
    chrome: Path,
    screenshot: Path | None,
    dom_output: Path | None,
) -> None:
    repository = Path(__file__).resolve().parents[2]
    if screenshot is not None:
        screenshot.unlink(missing_ok=True)
    if dom_output is not None:
        dom_output.unlink(missing_ok=True)
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-browser-reactive-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    gateway: subprocess.Popen[str] | None = None
    vite: subprocess.Popen[str] | None = None
    diagnostics: list[str] = []
    failure: BaseException | None = None
    try:
        fixture = create_fixture(run_path)
        session = create_session()
        session.apply_settings({"upload_rate_limit": UPLOAD_RATE_LIMIT})
        port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            fixture.torrent_info,
            fixture.seed_directory,
            diagnostics,
        )
        handle.set_upload_limit(UPLOAD_RATE_LIMIT)
        vite_port = reserve_loopback_port()
        origin = f"http://127.0.0.1:{vite_port}"
        storage = run_path / "downloads"
        gateway, address = start_gateway(
            build_gateway(repository),
            run_path / "profile",
            storage,
            origin,
        )
        environment = os.environ.copy()
        environment.update(
            {
                "VITE_RSTORRENT_INTEROP_MAGNET": magnet_uri(
                    fixture.info_hash,
                    f"127.0.0.1:{port}",
                ),
                "VITE_RSTORRENT_INTEROP_GATEWAY_URL": (
                    f"ws://{address}/control"
                ),
                "VITE_RSTORRENT_INTEROP_GATEWAY_TOKEN": TOKEN,
                "VITE_RSTORRENT_INTEROP_EXTERNAL_CONTROL": "1",
            }
        )
        vite = start_vite(repository, environment, origin, vite_port)
        result = run_chrome(
            chrome,
            run_path / "chrome-profile",
            repository,
            origin,
            screenshot,
        )
        dom = result["html"]
        if dom_output is not None:
            dom_output.parent.mkdir(parents=True, exist_ok=True)
            dom_output.write_text(dom, encoding="utf-8")
        verify_payload(storage, fixture.info_hash, fixture.payload_hash)
        stop_process(vite, "Vite")
        vite = None
        stop_gateway(gateway)
        gateway = None
        requested = result_attribute(dom, "requested")
        received = result_attribute(dom, "received")
        stored = result_attribute(dom, "stored")
        print(
            f"browser=chrome info_hash={fixture.info_hash} "
            f"requested={requested} received={received} stored={stored} "
            f"metadata_size={len(fixture.info_bytes)} pieces=3 "
            f"payload_sha1={fixture.payload_hash} gateway_shutdown=joined "
            f"origin={origin} screenshot={screenshot or 'disabled'} "
            "ui_clicks=pause,resume pause_resume=ok cleanup=ok"
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


def result_attribute(dom: str, name: str) -> int:
    prefix = f'data-{name}="'
    start = dom.index(prefix) + len(prefix)
    return int(dom[start : dom.index('"', start)])


def main() -> int:
    arguments = parse_arguments()
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    try:
        run(
            discover_chrome(arguments.chrome),
            (
                arguments.screenshot.resolve()
                if arguments.screenshot is not None
                else None
            ),
            (
                arguments.dom_output.resolve()
                if arguments.dom_output is not None
                else None
            ),
        )
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
