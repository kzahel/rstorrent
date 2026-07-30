#!/usr/bin/env python3
"""Exercise the Android foreground client against a controlled libtorrent seed."""

from __future__ import annotations

import argparse
import gc
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET
from pathlib import Path

import libtorrent as lt

from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    create_session,
    wait_for_listener,
)
from magnet_metadata import create_fixture, magnet_uri


PACKAGE = "org.rstorrent.bootstrap"
ACTIVITY = f"{PACKAGE}/.MainActivity"
TRACE_TAG = "RSTorrentProduct"
DOWNLOAD_TIMEOUT_SECONDS = 45
UPLOAD_RATE_LIMIT = 8 * 1024
ANDROID_PAYLOAD_SIZE = 128 * 1024
BOUNDS_PATTERN = re.compile(r"\[(\d+),(\d+)]\[(\d+),(\d+)]")


class Adb:
    def __init__(self, executable: Path, serial: str) -> None:
        self.command = [str(executable), "-s", serial]

    def run(
        self,
        *arguments: str,
        timeout: float = 15,
        check: bool = True,
    ) -> subprocess.CompletedProcess[str]:
        completed = subprocess.run(
            [*self.command, *arguments],
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        if check and completed.returncode != 0:
            raise ScenarioFailure(
                f"adb {' '.join(arguments)} failed\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
        return completed

    def shell(
        self,
        *arguments: str,
        timeout: float = 15,
        check: bool = True,
    ) -> subprocess.CompletedProcess[str]:
        return self.run("shell", *arguments, timeout=timeout, check=check)


def parse_arguments() -> argparse.Namespace:
    repository = Path(__file__).resolve().parents[2]
    default_sdk = Path.home() / "Android" / "Sdk"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True, help="authorized ADB serial")
    parser.add_argument(
        "--adb",
        type=Path,
        default=default_sdk / "platform-tools" / "adb",
    )
    parser.add_argument(
        "--apk",
        type=Path,
        default=(
            repository
            / "experiments/android-engine-bootstrap/app/build/outputs/apk/debug/app-debug.apk"
        ),
    )
    return parser.parse_args()


def require_unlocked(adb: Adb) -> None:
    policy = adb.shell("dumpsys", "window", "policy").stdout
    locked_markers = (
        "isStatusBarKeyguard=true",
        "mDreamingLockscreen=true",
        "mKeyguardShowing=true",
        "mShowingLockscreen=true",
    )
    if any(marker in policy for marker in locked_markers):
        raise ScenarioFailure(
            "device is locked; unlock it manually before running this test"
        )


def install_and_start(
    adb: Adb,
    apk: Path,
    magnet: str,
) -> None:
    if not apk.is_file():
        raise ScenarioFailure(f"Android APK does not exist: {apk}")
    adb.run("install", "-r", str(apk), timeout=60)
    adb.shell("pm", "clear", PACKAGE)
    api = int(adb.shell("getprop", "ro.build.version.sdk").stdout.strip())
    if api >= 33:
        adb.shell(
            "pm",
            "grant",
            PACKAGE,
            "android.permission.POST_NOTIFICATIONS",
        )
    adb.run("logcat", "-c")
    started = adb.shell(
        "am",
        "start",
        "-W",
        "-n",
        ACTIVITY,
        "--es",
        "product_magnet",
        shlex.quote(magnet),
        timeout=30,
    )
    if "Status: ok" not in started.stdout:
        raise ScenarioFailure(f"product Activity did not start:\n{started.stdout}")


def product_logs(adb: Adb) -> str:
    return adb.run(
        "logcat",
        "-d",
        "-v",
        "brief",
        f"{TRACE_TAG}:I",
        "AndroidRuntime:E",
        "*:S",
    ).stdout


def wait_for_download(adb: Adb) -> tuple[str, bool, bool]:
    deadline = time.monotonic() + DOWNLOAD_TIMEOUT_SECONDS
    trace = ""
    lifecycle_checked = False
    control_checked = False
    while time.monotonic() < deadline:
        trace = product_logs(adb)
        if "FATAL EXCEPTION" in trace or f"E/{TRACE_TAG}" in trace:
            raise ScenarioFailure(f"Android product client failed:\n{trace}")
        if not control_checked and positive_counter(trace, "requested"):
            verify_pause_resume(adb)
            control_checked = True
            trace = product_logs(adb)
        if control_checked and not lifecycle_checked:
            verify_activity_independence(adb)
            lifecycle_checked = True
        if "state=COMPLETE" in trace:
            if not lifecycle_checked or not control_checked:
                raise ScenarioFailure(
                    "controlled download completed before control/lifecycle validation"
                )
            return trace, lifecycle_checked, control_checked
        time.sleep(0.1)
    raise ScenarioFailure(f"Android download did not complete:\n{trace}")


def positive_counter(trace: str, field: str) -> bool:
    return any(
        int(value) > 0
        for value in re.findall(rf"(?:^| ){re.escape(field)}=(\d+)", trace)
    )


def find_control(adb: Adb, label: str) -> ET.Element:
    root = dump_ui(adb)
    controls = [
        node
        for node in root.iter()
        if node.attrib.get("clickable") == "true"
        and any(descendant.attrib.get("text") == label for descendant in node.iter())
    ]
    control = min(controls, key=lambda node: bounds_area(node.attrib["bounds"]), default=None)
    if control is None:
        raise ScenarioFailure(f"Android product UI has no clickable {label} control")
    return control


def bounds_area(bounds: str) -> int:
    match = BOUNDS_PATTERN.fullmatch(bounds)
    if match is None:
        raise ScenarioFailure(f"invalid UI bounds: {bounds}")
    left, top, right, bottom = (int(value) for value in match.groups())
    return (right - left) * (bottom - top)


def wait_for_new_state(adb: Adb, state: str, previous_count: int) -> str:
    deadline = time.monotonic() + 10
    trace = ""
    marker = f"state={state}"
    while time.monotonic() < deadline:
        trace = product_logs(adb)
        if trace.count(marker) > previous_count:
            return trace
        time.sleep(0.1)
    raise ScenarioFailure(f"Android UI did not reach {state}:\n{trace}")


def verify_pause_resume(adb: Adb) -> None:
    trace = product_logs(adb)
    paused_before = trace.count("state=PAUSED")
    tap_bounds(adb, find_control(adb, "Pause").attrib["bounds"])
    trace = wait_for_new_state(adb, "PAUSED", paused_before)
    downloading_before = trace.count("state=DOWNLOADING")
    tap_bounds(adb, find_control(adb, "Resume").attrib["bounds"])
    wait_for_new_state(adb, "DOWNLOADING", downloading_before)


def verify_activity_independence(adb: Adb) -> None:
    pid_before = adb.shell("pidof", PACKAGE).stdout.strip()
    if not pid_before:
        raise ScenarioFailure("Android product process disappeared")
    adb.shell("cmd", "window", "user-rotation", "lock", "1")
    time.sleep(0.5)
    adb.shell("cmd", "window", "user-rotation", "free")
    time.sleep(0.5)
    pid_after = adb.shell("pidof", PACKAGE).stdout.strip()
    if pid_after != pid_before:
        raise ScenarioFailure("Activity recreation replaced the engine process")

    adb.shell(
        "am",
        "start",
        "-a",
        "android.intent.action.MAIN",
        "-c",
        "android.intent.category.HOME",
    )
    deadline = time.monotonic() + 3
    services = ""
    while time.monotonic() < deadline:
        services = adb.shell("dumpsys", "activity", "services", PACKAGE).stdout
        if "ConnectionRecord" not in services:
            break
        time.sleep(0.1)
    if ".ProductEngineService" not in services or "isForeground=true" not in services:
        raise ScenarioFailure("foreground service did not survive Activity collection")
    if "ConnectionRecord" in services:
        raise ScenarioFailure("Activity remained bound after it entered the background")


def verify_payload(
    adb: Adb,
    info_hash: str,
    expected_hash: str,
) -> None:
    relative = f"files/product-downloads/{info_hash}/payload.bin"
    completed = adb.shell("run-as", PACKAGE, "sha1sum", relative)
    actual = completed.stdout.split(maxsplit=1)[0] if completed.stdout else ""
    if actual != expected_hash:
        raise ScenarioFailure(
            f"Android payload hash differs: expected {expected_hash}, got {actual}"
        )


def dump_ui(adb: Adb) -> ET.Element:
    adb.shell("uiautomator", "dump", "/sdcard/rstorrent-window.xml")
    xml = adb.shell("cat", "/sdcard/rstorrent-window.xml").stdout
    return ET.fromstring(xml)


def tap_bounds(adb: Adb, bounds: str) -> None:
    match = BOUNDS_PATTERN.fullmatch(bounds)
    if match is None:
        raise ScenarioFailure(f"invalid UI bounds: {bounds}")
    left, top, right, bottom = (int(value) for value in match.groups())
    adb.shell(
        "input",
        "tap",
        str((left + right) // 2),
        str((top + bottom) // 2),
    )


def stop_from_notification(adb: Adb) -> None:
    adb.shell("cmd", "statusbar", "expand-notifications")
    time.sleep(0.5)
    root = dump_ui(adb)
    product_row = next(
        (
            row
            for row in root.iter()
            if row.attrib.get("resource-id")
            == "com.android.systemui:id/expandableNotificationRow"
            and any(node.attrib.get("text") == "RSTorrent" for node in row.iter())
        ),
        None,
    )
    if product_row is None:
        raise ScenarioFailure("RSTorrent foreground notification is absent")
    expand = next(
        (
            node
            for node in product_row.iter()
            if node.attrib.get("content-desc") == "Expand"
        ),
        None,
    )
    if expand is not None:
        tap_bounds(adb, expand.attrib["bounds"])
        time.sleep(0.25)
        root = dump_ui(adb)
    stop = next(
        (
            node
            for node in root.iter()
            if node.attrib.get("text") == "Stop"
            and node.attrib.get("clickable") == "true"
        ),
        None,
    )
    if stop is None:
        raise ScenarioFailure("foreground notification has no Stop action")
    tap_bounds(adb, stop.attrib["bounds"])
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        services = adb.shell("dumpsys", "activity", "services", PACKAGE).stdout
        if ".ProductEngineService" not in services:
            adb.shell("cmd", "statusbar", "collapse")
            return
        time.sleep(0.1)
    raise ScenarioFailure("foreground service did not terminate after Stop")


def run(arguments: argparse.Namespace) -> None:
    adb = Adb(arguments.adb, arguments.serial)
    require_unlocked(adb)
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-android-reactive-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    port: int | None = None
    diagnostics: list[str] = []
    try:
        fixture = create_fixture(run_path, payload_size=ANDROID_PAYLOAD_SIZE)
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
        adb.run("reverse", f"tcp:{port}", f"tcp:{port}")
        install_and_start(
            adb,
            arguments.apk.resolve(),
            magnet_uri(fixture.info_hash, f"127.0.0.1:{port}"),
        )
        trace, _, _ = wait_for_download(adb)
        for counter in ("requested", "received", "stored"):
            if not positive_counter(trace, counter):
                raise ScenarioFailure(
                    f"Android trace never exposed positive {counter} bytes:\n{trace}"
                )
        verify_payload(adb, fixture.info_hash, fixture.payload_hash)
        stop_from_notification(adb)
        if "FATAL EXCEPTION" in product_logs(adb):
            raise ScenarioFailure("Android runtime failed during joined shutdown")
        updates = trace.count("view_update")
        print(
            f"android_serial={arguments.serial} info_hash={fixture.info_hash} "
            f"metadata_size={len(fixture.info_bytes)} "
            f"pieces={fixture.torrent_info.num_pieces()} "
            f"view_updates={updates} activity_recreation=ok "
            f"activity_background=ok payload_sha1={fixture.payload_hash} "
            "pause_resume=ok foreground_stop=joined cleanup=ok"
        )
    finally:
        if port is not None:
            adb.run("reverse", "--remove", f"tcp:{port}", check=False)
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
    arguments = parse_arguments()
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    try:
        run(arguments)
    except (ScenarioFailure, OSError, subprocess.SubprocessError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
