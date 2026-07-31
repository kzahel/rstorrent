#!/usr/bin/env python3
"""Render a scheduled tracker retry on an isolated Android target."""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET
from pathlib import Path

from android_reactive_surface import (
    ACTIVITY,
    PACKAGE,
    Adb,
    add_magnet,
    dump_ui,
    find_control,
    install_and_start,
    product_logs,
    require_unlocked,
    select_controlled_tree,
    stop_from_notification,
    tap_bounds,
    verify_activity_independence,
)
from first_verified_piece import ScenarioFailure
from headless_avd import OwnedHeadlessAvd, default_adb, default_emulator


TORRENT_ID = "000102030405060708090a0b0c0d0e0f10111213"
MAGNET = (
    f"magnet:?xt=urn:btih:{TORRENT_ID}"
    "&tr=udp%3A%2F%2F0.0.0.0%3A6969%2Fannounce"
)


def parse_arguments() -> argparse.Namespace:
    repository = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--serial")
    target.add_argument("--avd")
    parser.add_argument("--headless", action="store_true")
    parser.add_argument("--adb", type=Path, default=default_adb())
    parser.add_argument("--emulator", type=Path, default=default_emulator())
    parser.add_argument(
        "--apk",
        type=Path,
        default=(
            repository
            / "experiments/android-engine-bootstrap/app/build/outputs/apk/debug/app-debug.apk"
        ),
    )
    parser.add_argument("--screenshot", type=Path)
    return parser.parse_args()


def scroll_to_text(adb: Adb, label: str) -> ET.Element:
    last_root: ET.Element | None = None
    size = adb.shell("wm", "size").stdout
    dimensions = next(
        (
            value
            for value in reversed(size.split())
            if "x" in value and all(part.isdigit() for part in value.split("x", 1))
        ),
        None,
    )
    if dimensions is None:
        raise ScenarioFailure(f"could not determine Android display size: {size!r}")
    width, height = (int(part) for part in dimensions.split("x", 1))
    x = str(width // 2)
    upward = (
        x,
        str(height * 4 // 5),
        x,
        str(height // 4),
    )
    downward = (
        x,
        str(height // 4),
        x,
        str(height * 4 // 5),
    )
    gestures = [upward for _ in range(16)] + [downward for _ in range(16)]
    for gesture in gestures:
        root = dump_ui(adb)
        last_root = root
        match = next(
            (
                node
                for node in root.iter()
                if node.attrib.get("text", "").casefold() == label.casefold()
            ),
            None,
        )
        if match is not None:
            return match
        adb.shell("input", "swipe", *gesture, "220")
        time.sleep(0.2)
    visible = (
        " | ".join(
            node.attrib.get("text", "")
            for node in last_root.iter()
            if node.attrib.get("text")
        )
        if last_root is not None
        else "(no UI)"
    )
    raise ScenarioFailure(
        f"Android UI did not expose {label!r} after scrolling; visible: {visible}"
    )


def click_text(adb: Adb, label: str) -> None:
    scroll_to_text(adb, label)
    tap_bounds(adb, find_control(adb, label).attrib["bounds"])
    time.sleep(0.3)


def wait_for_tracker_retry(adb: Adb) -> str:
    deadline = time.monotonic() + 15
    trace = ""
    while time.monotonic() < deadline:
        trace = product_logs(adb)
        if "FATAL EXCEPTION" in trace:
            raise ScenarioFailure(f"Android product client failed:\n{trace}")
        if (
            "progress=WAITING" in trace
            and "reason=WAITING_FOR_DISCOVERY" in trace
            and "diagnostic=tracker_retry_scheduled" in trace
        ):
            return trace
        time.sleep(0.1)
    raise ScenarioFailure(
        "Android client did not render a scheduled tracker retry:\n" + trace
    )


def run(arguments: argparse.Namespace) -> None:
    if arguments.serial is None:
        raise ScenarioFailure("Android target serial was not resolved")
    adb = Adb(arguments.adb, arguments.serial)
    require_unlocked(adb)
    screenshot = arguments.screenshot.resolve() if arguments.screenshot else None
    if screenshot is not None:
        screenshot.unlink(missing_ok=True)
    try:
        install_and_start(adb, arguments.apk.resolve())
        select_controlled_tree(adb)
        add_magnet(adb, MAGNET)
        wait_for_tracker_retry(adb)
        scroll_to_text(adb, "Diagnostics")
        click_text(adb, "detailed")
        click_text(adb, "tracker")
        wait_for_tracker_retry(adb)
        root = dump_ui(adb)
        rendered = " ".join(
            node.attrib.get("text", "") for node in root.iter()
        ).casefold()
        for expected in (
            "diagnostics",
            "selected progress · waiting",
            "tracker_retry_scheduled",
        ):
            if expected not in rendered:
                raise ScenarioFailure(
                    f"Android diagnostic surface omitted {expected!r}:\n{rendered}"
                )
        if screenshot is not None:
            adb.capture_screenshot(screenshot)
        api = adb.shell("getprop", "ro.build.version.sdk").stdout.strip()
        abi = adb.shell("getprop", "ro.product.cpu.abi").stdout.strip()
        verify_activity_independence(adb)
        stop_from_notification(adb)
        adb.shell("pm", "clear", PACKAGE)
        print(
            f"android_serial={arguments.serial} scenario=tracker_retry "
            f"api={api} abi={abi} info_hash={TORRENT_ID} progress=waiting "
            "reason=waiting_for_discovery "
            "diagnostic=tracker_retry_scheduled ui_filters=profile,category "
            "activity_recreation=ok activity_background=ok "
            f"screenshot={screenshot or 'disabled'} foreground_stop=joined cleanup=ok"
        )
    except BaseException:
        if screenshot is not None and not screenshot.exists():
            try:
                adb.capture_screenshot(screenshot)
            except BaseException as screenshot_error:
                print(
                    f"Android failure screenshot unavailable: {screenshot_error}",
                    file=sys.stderr,
                )
        raise
    finally:
        adb.shell("am", "force-stop", PACKAGE, check=False)
        adb.shell("pm", "clear", PACKAGE, check=False)
        adb.shell("rm", "-rf", "/sdcard/Download/RSTorrentReactiveGrant", check=False)
        adb.shell("rm", "-f", "/sdcard/rstorrent-window.xml", check=False)


def main() -> int:
    arguments = parse_arguments()
    owned_avd: OwnedHeadlessAvd | None = None
    avd_work: Path | None = None
    try:
        if arguments.avd is not None:
            if not arguments.headless:
                raise ScenarioFailure("--avd requires --headless")
            avd_work = Path(tempfile.mkdtemp(prefix="rstorrent-headless-avd-"))
            owned_avd = OwnedHeadlessAvd.start(
                arguments.avd,
                arguments.adb.resolve(),
                arguments.emulator.resolve(),
                avd_work,
            )
            arguments.serial = owned_avd.serial
            print(
                f"android_target=headless-avd name={owned_avd.name} "
                f"serial={owned_avd.serial}"
            )
        elif arguments.headless:
            raise ScenarioFailure("--headless is only valid with --avd")
        run(arguments)
    except (ScenarioFailure, OSError, subprocess.SubprocessError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    finally:
        if owned_avd is not None:
            owned_avd.close()
        if avd_work is not None:
            shutil.rmtree(avd_work, ignore_errors=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
