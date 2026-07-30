#!/usr/bin/env python3
"""Exercise the rendered browser surface against a controlled libtorrent seed."""

from __future__ import annotations

import gc
import json
import os
import selectors
import shutil
import signal
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
    ORIGIN,
    TOKEN,
    UPLOAD_RATE_LIMIT,
    build_gateway,
    start_gateway,
    stop_gateway,
    verify_payload,
)
from magnet_metadata import create_fixture, magnet_uri


def start_vite(
    repository: Path,
    environment: dict[str, str],
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
            "5173",
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
                with urllib.request.urlopen(ORIGIN, timeout=0.2) as response:
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
    raise ScenarioFailure("Vite did not start\n" + "\n".join(diagnostics))


def stop_process(process: subprocess.Popen[str], label: str) -> None:
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
    try:
        process.communicate(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.communicate(timeout=5)
        raise ScenarioFailure(f"{label} did not terminate")


def run_chrome(
    chrome: Path,
    profile: Path,
    repository: Path,
) -> str:
    process = subprocess.Popen(
        [
            str(chrome),
            "--headless=new",
            "--no-sandbox",
            "--disable-gpu",
            "--disable-dev-shm-usage",
            f"--user-data-dir={profile}",
            "--remote-debugging-port=0",
            "--remote-allow-origins=*",
            ORIGIN,
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    if process.stderr is None:
        raise ScenarioFailure("Chrome diagnostics are unavailable")
    selector = selectors.DefaultSelector()
    selector.register(process.stderr, selectors.EVENT_READ)
    deadline = time.monotonic() + 10
    browser_url: str | None = None
    diagnostics: list[str] = []
    try:
        while time.monotonic() < deadline:
            if not selector.select(deadline - time.monotonic()):
                break
            line = process.stderr.readline()
            if not line:
                break
            diagnostics.append(line.rstrip())
            prefix = "DevTools listening on "
            if line.startswith(prefix):
                browser_url = line[len(prefix) :].strip()
                break
    finally:
        selector.close()
    if browser_url is None:
        process.terminate()
        process.communicate(timeout=5)
        raise ScenarioFailure(
            "Chrome did not announce DevTools\n" + "\n".join(diagnostics)
        )
    port = urlparse(browser_url).port
    if port is None:
        process.terminate()
        process.communicate(timeout=5)
        raise ScenarioFailure(f"Chrome reported an invalid DevTools URL: {browser_url}")
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
                    and item.get("url", "").startswith(ORIGIN)
                ),
                None,
            )
            if target is not None:
                break
        except OSError:
            pass
        time.sleep(0.05)
    if target is None:
        process.terminate()
        process.communicate(timeout=5)
        raise ScenarioFailure("Chrome page target did not appear")
    completed = subprocess.run(
        [
            "node",
            str(repository / "tests/interop/chrome_wait.mjs"),
            target["webSocketDebuggerUrl"],
            "45000",
        ],
        capture_output=True,
        text=True,
        timeout=50,
        check=False,
    )
    process.terminate()
    try:
        _, chrome_stderr = process.communicate(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        _, chrome_stderr = process.communicate(timeout=5)
    if completed.returncode != 0:
        raise ScenarioFailure(
            f"headless browser observer exited with status {completed.returncode}\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}\n"
            f"Chrome diagnostics:\n{chrome_stderr}"
        )
    result = json.loads(completed.stdout)
    if 'id="interop-result"' not in result["html"]:
        raise ScenarioFailure(
            "rendered browser surface did not reach controlled completion\n"
            f"DOM:\n{result['html'][-4000:]}\n"
            f"Chrome diagnostics:\n{chrome_stderr[-4000:]}"
        )
    for field in ("requested", "received", "stored"):
        if int(result[field]) <= 0:
            raise ScenarioFailure(f"browser did not render positive {field} bytes")
    return result["html"]


def run(chrome: Path) -> None:
    repository = Path(__file__).resolve().parents[2]
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
        storage = run_path / "downloads"
        gateway, address = start_gateway(
            build_gateway(repository),
            run_path / "profile",
            storage,
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
            }
        )
        vite = start_vite(repository, environment)
        dom = run_chrome(chrome, run_path / "chrome-profile", repository)
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
            "cleanup=ok"
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
    chrome = Path(os.environ.get("CHROME", "/usr/bin/google-chrome"))
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    try:
        run(chrome)
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
