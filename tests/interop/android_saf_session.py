#!/usr/bin/env python3
"""Prove durable Android SAF resume and publication against libtorrent."""

from __future__ import annotations

import argparse
import gc
import hashlib
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

from android_reactive_surface import (
    ACTIVITY,
    PACKAGE,
    TRACE_TAG,
    Adb,
    bounds_area,
    dump_ui,
    find_control,
    product_logs,
    require_unlocked,
    tap_bounds,
    verify_activity_independence,
)
from first_verified_piece import ScenarioFailure, add_seed, create_session, wait_for_listener
from headless_avd import OwnedHeadlessAvd, default_adb, default_emulator
from magnet_metadata import create_fixture, magnet_uri


GRANT_FOLDER = "RSTorrentSafSessionGrant"
GRANT_PATH = f"/sdcard/Download/{GRANT_FOLDER}"
PAYLOAD_SIZE = 256 * 1024
UPLOAD_RATE_LIMIT = 12 * 1024
DOWNLOAD_TIMEOUT_SECONDS = 90
SAF_RESUME_TIMEOUT_SECONDS = 45
CRASH_AFTER_RENAME_EXTRA = "product_crash_after_saf_rename"
RELEASE_GRANT_EXTRA = "product_release_saf_grant"
STORAGE_SELECTION_LABELS = {
    "Choose a download folder",
    "Select download folder",
    "Select folder",
    "Repair",
}


def parse_arguments() -> argparse.Namespace:
    repository = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--serial", help="explicit authorized ADB serial")
    target.add_argument("--avd", help="AVD name for a harness-owned emulator")
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
    return parser.parse_args()


def install_clean_app(adb: Adb, apk: Path) -> None:
    if not apk.is_file():
        raise ScenarioFailure(f"Android APK does not exist: {apk}")
    adb.run("install", "-r", str(apk), timeout=60)
    adb.shell("pm", "clear", PACKAGE)
    api = int(adb.shell("getprop", "ro.build.version.sdk").stdout.strip())
    if api >= 33:
        adb.shell("pm", "grant", PACKAGE, "android.permission.POST_NOTIFICATIONS")
    adb.shell("rm", "-rf", GRANT_PATH)
    adb.shell("mkdir", "-p", GRANT_PATH)
    adb.shell("cmd", "statusbar", "collapse", check=False)
    adb.run("logcat", "-c")
    started = adb.shell(
        "am",
        "start",
        "-W",
        "--activity-clear-task",
        "-n",
        ACTIVITY,
        timeout=30,
    )
    if "Status: ok" not in started.stdout:
        raise ScenarioFailure(f"product Activity did not start:\n{started.stdout}")


def click_labeled(nodes: list[ET.Element], labels: set[str]) -> ET.Element | None:
    wanted = {label.casefold() for label in labels}
    candidates = []
    for node in nodes:
        if node.attrib.get("clickable") != "true":
            continue
        values = {
            descendant.attrib.get("text", "").strip().casefold()
            for descendant in node.iter()
        }
        values.add(node.attrib.get("content-desc", "").strip().casefold())
        if values & wanted:
            candidates.append(node)
    return min(
        candidates,
        key=lambda node: bounds_area(node.attrib["bounds"]),
        default=None,
    )


def select_controlled_tree(adb: Adb) -> None:
    deadline = time.monotonic() + 15
    control: ET.Element | None = None
    while time.monotonic() < deadline:
        control = click_labeled(list(dump_ui(adb).iter()), STORAGE_SELECTION_LABELS)
        if control is not None:
            break
        time.sleep(0.2)
    if control is None:
        raise ScenarioFailure("Android product UI did not expose SAF root selection")
    tap_bounds(adb, control.attrib["bounds"])
    deadline = time.monotonic() + 60
    entered = False
    accepted = False
    last_xml = ""
    while time.monotonic() < deadline:
        root = dump_ui(adb)
        nodes = list(root.iter())
        last_xml = ET.tostring(root, encoding="unicode")
        showing_documents = any(
            "documentsui" in node.attrib.get("package", "").casefold()
            for node in nodes
        )
        if not showing_documents and not accepted:
            retry = click_labeled(nodes, STORAGE_SELECTION_LABELS)
            if retry is not None:
                tap_bounds(adb, retry.attrib["bounds"])
                time.sleep(0.4)
                continue
        if any(
            node.attrib.get("text") == GRANT_FOLDER
            and node.attrib.get("resource-id", "").endswith(":id/breadcrumb_text")
            for node in nodes
        ):
            entered = True
        if not entered:
            entries = [
                node
                for node in nodes
                if node.attrib.get("resource-id") == "android:id/title"
                and node.attrib.get("text") == GRANT_FOLDER
            ]
            if entries:
                tap_bounds(adb, entries[0].attrib["bounds"])
                entered = True
                time.sleep(0.4)
                continue
        if entered and not accepted:
            use = click_labeled(nodes, {"Use this folder", "Select"})
            if use is not None:
                tap_bounds(adb, use.attrib["bounds"])
                accepted = True
                time.sleep(0.4)
                continue
        if accepted:
            allow = click_labeled(nodes, {"Allow"})
            if allow is not None:
                tap_bounds(adb, allow.attrib["bounds"])
                time.sleep(0.4)
                continue
            if not showing_documents:
                break
        time.sleep(0.25)
    else:
        raise ScenarioFailure(f"could not grant controlled SAF tree:\n{last_xml}")

    preferences = adb.shell(
        "run-as",
        PACKAGE,
        "cat",
        "shared_prefs/product-saf.xml",
    ).stdout
    if GRANT_FOLDER not in preferences:
        raise ScenarioFailure("product did not persist the controlled SAF tree URI")


def verify_revoked_grant_fails_closed(adb: Adb) -> None:
    released = adb.shell(
        "am",
        "start",
        "-W",
        "--activity-single-top",
        "-n",
        ACTIVITY,
        "--ez",
        RELEASE_GRANT_EXTRA,
        "true",
        timeout=30,
    )
    if "Status: ok" not in released.stdout:
        raise ScenarioFailure(f"could not inject controlled SAF grant loss:\n{released.stdout}")
    adb.shell("am", "force-stop", PACKAGE)
    started = adb.shell("am", "start", "-W", "-n", ACTIVITY, timeout=30)
    if "Status: ok" not in started.stdout:
        raise ScenarioFailure(f"product did not restart after SAF grant loss:\n{started.stdout}")
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if click_labeled(list(dump_ui(adb).iter()), STORAGE_SELECTION_LABELS) is not None:
            break
        time.sleep(0.2)
    else:
        raise ScenarioFailure("revoked SAF grant was still presented as usable")
    preferences = adb.shell(
        "run-as",
        PACKAGE,
        "cat",
        "shared_prefs/product-saf.xml",
    ).stdout
    if GRANT_FOLDER not in preferences:
        raise ScenarioFailure("grant-loss check removed stable SAF identity")


def add_magnet(adb: Adb, magnet: str) -> None:
    started = adb.shell(
        "am",
        "start",
        "-W",
        "--activity-single-top",
        "-n",
        ACTIVITY,
        "--es",
        "product_magnet",
        shlex.quote(magnet),
        timeout=30,
    )
    if "Status: ok" not in started.stdout:
        raise ScenarioFailure(f"product magnet Activity did not start:\n{started.stdout}")


def verified_count(trace: str) -> int:
    values = [int(value) for value in re.findall(r"(?:^| )verified=(\d+)", trace)]
    return max(values, default=0)


def verify_saf_pause_resume(adb: Adb) -> None:
    trace = product_logs(adb)
    paused_before = trace.count("state=PAUSED")
    tap_bounds(adb, find_control(adb, "Pause").attrib["bounds"])
    deadline = time.monotonic() + SAF_RESUME_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        trace = product_logs(adb)
        if trace.count("state=PAUSED") > paused_before:
            break
        time.sleep(0.1)
    else:
        raise ScenarioFailure(f"Android SAF UI did not reach PAUSED:\n{trace}")

    downloading_before = trace.count("state=DOWNLOADING")
    tap_bounds(adb, find_control(adb, "Resume").attrib["bounds"])
    deadline = time.monotonic() + SAF_RESUME_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        trace = product_logs(adb)
        if trace.count("state=DOWNLOADING") > downloading_before:
            return
        time.sleep(0.1)
    raise ScenarioFailure(f"Android SAF UI did not reacquire storage after Resume:\n{trace}")


def wait_for_checkpoint(adb: Adb) -> str:
    deadline = time.monotonic() + DOWNLOAD_TIMEOUT_SECONDS
    control_checked = False
    lifecycle_checked = False
    trace = ""
    while time.monotonic() < deadline:
        trace = product_logs(adb)
        if "FATAL EXCEPTION" in trace or f"E/{TRACE_TAG}" in trace:
            raise ScenarioFailure(f"Android SAF client failed:\n{trace}")
        if not control_checked and "state=DOWNLOADING" in trace:
            verify_saf_pause_resume(adb)
            control_checked = True
        if control_checked and not lifecycle_checked:
            verify_activity_independence(adb)
            lifecycle_checked = True
            adb.shell("am", "start", "-W", "-n", ACTIVITY, timeout=30)
        if control_checked and lifecycle_checked and verified_count(trace) >= 2:
            return trace
        time.sleep(0.1)
    raise ScenarioFailure(f"Android SAF download did not checkpoint two pieces:\n{trace}")


def force_stop_and_resume(adb: Adb) -> tuple[int, str]:
    trace = product_logs(adb)
    claims = verified_count(trace)
    if claims < 2:
        raise ScenarioFailure("process-death test has fewer than two durable claims")
    adb.shell("am", "force-stop", PACKAGE)
    if adb.shell("pidof", PACKAGE, check=False).stdout.strip():
        raise ScenarioFailure("forced Android process remained alive")
    adb.run("logcat", "-c")
    started = adb.shell(
        "am",
        "start",
        "-W",
        "-n",
        ACTIVITY,
        "--ez",
        CRASH_AFTER_RENAME_EXTRA,
        "true",
        timeout=30,
    )
    if "Status: ok" not in started.stdout:
        raise ScenarioFailure(f"product did not restart:\n{started.stdout}")
    restarted_pid = adb.shell("pidof", PACKAGE).stdout.strip()
    if not restarted_pid:
        raise ScenarioFailure("product restart has no process")
    deadline = time.monotonic() + DOWNLOAD_TIMEOUT_SECONDS
    restart_trace = ""
    rename_crashed = False
    crash_restarted = False
    while time.monotonic() < deadline:
        restart_trace = product_logs(adb)
        if "FATAL EXCEPTION" in restart_trace or f"E/{TRACE_TAG}" in restart_trace:
            raise ScenarioFailure(f"Android SAF restart failed:\n{restart_trace}")
        if "saf_test_crash_after_rename" in restart_trace:
            rename_crashed = True
        if rename_crashed and not crash_restarted:
            current_pid = adb.shell("pidof", PACKAGE, check=False).stdout.strip()
            if current_pid != restarted_pid:
                crash_restarted = True
                if not current_pid:
                    resumed = adb.shell("am", "start", "-W", "-n", ACTIVITY, timeout=30)
                    if "Status: ok" not in resumed.stdout:
                        raise ScenarioFailure(
                            f"product did not restart after provider rename:\n{resumed.stdout}"
                        )
        if (
            crash_restarted
            and "saf_publication_confirmed" in restart_trace
            and "state=CHECKING storage=PUBLISHED" in restart_trace
            and "diagnostic=recheck_started" in restart_trace
            and "diagnostic=have_rechecked" in restart_trace
            and "state=COMPLETE" in restart_trace
            and restart_trace.count("saf_publication_begin") >= 2
        ):
            return claims, restart_trace
        time.sleep(0.1)
    raise ScenarioFailure(f"Android SAF restart did not complete:\n{restart_trace}")


def verify_shared_payload(adb: Adb, expected_hash: str) -> None:
    payload = f"{GRANT_PATH}/magnet-fixture/payload.bin"
    completed = adb.shell("sha1sum", payload)
    actual = completed.stdout.split(maxsplit=1)[0] if completed.stdout else ""
    if actual != expected_hash:
        raise ScenarioFailure(
            f"published SAF payload differs: expected {expected_hash}, got {actual}"
        )


def cleanup(adb: Adb) -> None:
    adb.shell("cmd", "statusbar", "collapse", check=False)
    adb.shell("pm", "clear", PACKAGE, check=False)
    adb.shell("rm", "-rf", GRANT_PATH, check=False)
    if adb.shell("test", "-e", GRANT_PATH, check=False).returncode == 0:
        raise ScenarioFailure(f"controlled SAF folder survived cleanup: {GRANT_PATH}")
    adb.shell("rm", "-f", "/sdcard/rstorrent-window.xml", check=False)


def stop_foreground_service(adb: Adb) -> None:
    services = adb.shell("dumpsys", "activity", "services", PACKAGE).stdout
    if ".ProductEngineService" not in services or "isForeground=true" not in services:
        raise ScenarioFailure("RSTorrent foreground service or notification is absent")
    adb.shell(
        "am",
        "start",
        "-a",
        "android.intent.action.MAIN",
        "-c",
        "android.intent.category.HOME",
    )
    unbind_deadline = time.monotonic() + 5
    while time.monotonic() < unbind_deadline:
        services = adb.shell("dumpsys", "activity", "services", PACKAGE).stdout
        if "ConnectionRecord" not in services:
            break
        time.sleep(0.1)
    if "ConnectionRecord" in services:
        raise ScenarioFailure("Activity remained bound before foreground Stop")
    try:
        adb.shell(
            "run-as",
            PACKAGE,
            "am",
            "startservice",
            "--user",
            "0",
            "-n",
            f"{PACKAGE}/.ProductEngineService",
            "-a",
            f"{PACKAGE}.PRODUCT_STOP",
        )
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            services = adb.shell("dumpsys", "activity", "services", PACKAGE).stdout
            if ".ProductEngineService" not in services:
                return
            time.sleep(0.1)
        raise ScenarioFailure("foreground service did not terminate after its Stop action")
    except ScenarioFailure as error:
        raise ScenarioFailure(f"{error}\nlogs:\n{product_logs(adb)}") from error


def run(arguments: argparse.Namespace) -> None:
    adb = Adb(arguments.adb, arguments.serial)
    require_unlocked(adb)
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-android-saf-session-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    port: int | None = None
    diagnostics: list[str] = []
    app_installed = False
    try:
        fixture = create_fixture(run_path, payload_size=PAYLOAD_SIZE)
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
        install_clean_app(adb, arguments.apk.resolve())
        app_installed = True
        select_controlled_tree(adb)
        verify_revoked_grant_fails_closed(adb)
        select_controlled_tree(adb)
        add_magnet(adb, magnet_uri(fixture.info_hash, f"127.0.0.1:{port}"))
        before = wait_for_checkpoint(adb)
        uploaded_before_restart = int(handle.status().total_upload)
        claims, after = force_stop_and_resume(adb)
        uploaded_after_restart = int(handle.status().total_upload) - uploaded_before_restart
        if uploaded_after_restart <= 0:
            raise ScenarioFailure("libtorrent uploaded no payload after process restart")
        if "storage=PREPARED" not in after:
            raise ScenarioFailure("Android trace did not expose prepared storage")
        verify_shared_payload(adb, fixture.payload_hash)
        stop_foreground_service(adb)
        shutdown_trace = product_logs(adb)
        if "FATAL EXCEPTION" in shutdown_trace:
            raise ScenarioFailure(
                f"Android runtime failed during joined shutdown:\n{shutdown_trace}"
            )
        if "product_shutdown_complete" not in shutdown_trace:
            raise ScenarioFailure(
                f"Android runtime did not report joined shutdown:\n{shutdown_trace}"
            )
        cleanup(adb)
        print(
            f"android_serial={arguments.serial} info_hash={fixture.info_hash} "
            f"metadata_size={len(fixture.info_bytes)} "
            f"pieces={fixture.torrent_info.num_pieces()} "
            f"checkpoint_claims={claims} restart_upload_bytes={uploaded_after_restart} "
            f"view_updates={before.count('view_update') + after.count('view_update')} "
            f"activity_recreation=ok activity_background=ok pause_resume=ok "
            f"grant_loss=fail_closed rename_crash=recovered "
            f"publication=fresh_piece_rechecked payload_sha1={fixture.payload_hash} "
            "foreground_stop=joined cleanup=ok"
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
        if app_installed:
            cleanup(adb)
        shutil.rmtree(run_path, ignore_errors=True)


def main() -> int:
    arguments = parse_arguments()
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
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
