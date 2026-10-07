#!/usr/bin/env python3
"""Controlled isolated Android/SAF qualification through Machine Control.

This is debug sideload evidence. It never qualifies a Play installation/update,
modifies inherited packages, clears device logs, or runs a public swarm.
"""
from __future__ import annotations
import argparse
import base64
import struct
import xml.etree.ElementTree as ET
import gc
import json
import re
import shlex
import subprocess
import tempfile
import time
import sys
import hashlib
import uuid
from types import SimpleNamespace
from pathlib import Path

import android_reactive_surface as product
from first_verified_piece import add_seed, create_session, wait_for_listener
from magnet_metadata import create_fixture, magnet_uri

PACKAGE = "org.rstorrent.qualification253"
ACTIVITY = f"{PACKAGE}/org.rstorrent.bootstrap.MainActivity"
FOLDER = "RSTorrentQualification253"
ROOT = f"/sdcard/Download/{FOLDER}"


def create_owned_fixture(path, run_id, phase, payload_size):
    # Concurrent devices must not discover and seed each other's deterministic
    # fixture through the normal product discovery paths. Name is in infohash.
    return create_fixture(path, payload_size=payload_size,
                          root_name=f"qualification253-{run_id}-{phase}")


def observation_payload_size(seconds):
    # 28 MiB is almost exactly an hour at 8 KiB/s, with no scheduling margin.
    # Keep transfer active for the requested hour; release the limit afterward.
    return 40 * 1024 * 1024 if seconds == 3600 else 28 * 1024 * 1024


def reset_owned_profile(adb):
    # Clear storage also revokes runtime permissions. Every new repetition
    # must restore the explicitly selected notification-eligible test setup.
    adb.shell("pm", "clear", PACKAGE)
    adb.shell("pm", "grant", PACKAGE, "android.permission.POST_NOTIFICATIONS")


def observation_result(seconds, transferred, verified, sha1):
    return {
        "status": ("pass" if seconds == 3600 else "bounded_short_run") if verified else "fail",
        "seconds": seconds,
        "seed_payload_upload": transferred,
        "completion": "pass" if verified else "fail",
        "sha1": sha1 if verified else None,
    }


class RemoteAdb:
    def __init__(self, command: list[str]):
        self.command = command

    def run(self, *arguments: str, timeout: float = 30, check: bool = True):
        if arguments[:2] == ("logcat", "-c"):
            raise RuntimeError("device-wide log clearing is prohibited")
        arguments = tuple("/data/local/tmp/rstorrent253-ui.xml" if value == "/sdcard/rstorrent-window.xml" else value for value in arguments)
        if arguments[0] == "shell":
            arguments = ("shell", "-n", shlex.join(arguments[1:]))
        remote = "export PATH=/bin:/usr/bin:/usr/local/bin:/usr/sbin:/sbin:$PATH\n" + shlex.join(["adb", "-s", "127.0.0.1:5555", *arguments]) + " </dev/null\n"
        result = subprocess.run([*self.command, "testbed", "--", "shell"], input=remote, capture_output=True, text=True, timeout=timeout)
        result.stdout = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", result.stdout).replace("\r", "")
        if check and result.returncode:
            raise RuntimeError(
                f"owned Android command failed ({result.returncode}): "
                f"stdout={result.stdout[-1500:]!r}; stderr={result.stderr[-1500:]!r}"
            )
        return result

    def shell(self, *arguments: str, **options):
        return self.run("shell", *arguments, **options)


def finish_first_use(adb):
    # A cold ARC launch plus several fresh UIAutomator snapshots can exceed
    # twenty seconds even when each observed transition succeeds.
    deadline = time.monotonic()+45
    ready_snapshots = 0
    while time.monotonic() < deadline:
        nodes = list(product.dump_ui(adb).iter())
        save = product.click_labeled(nodes, {"Save and continue"})
        if save is not None:
            ready_snapshots = 0
            enabled = next((node for node in nodes if node.get("checkable") == "true" and node.get("checked") == "true"), None)
            if enabled is not None:
                product.tap_bounds(adb, enabled.get("bounds"))
                # Compose may reflow the dialog after changing consent. A tap
                # must use the next snapshot's button, not the old geometry.
            else:
                product.tap_bounds(adb, save.get("bounds"))
        elif product.click_labeled(nodes, product.STORAGE_SELECTION_LABELS) is not None:
            ready_snapshots += 1
            if ready_snapshots >= 2:
                return
        else:
            ready_snapshots = 0
        time.sleep(0.5)
    raise RuntimeError("first-use disclosure did not resolve within its stage budget")


def unique_observed_control(adb, field, value):
    nodes = [node for node in product.dump_ui(adb).iter() if node.get(field) == value]
    if len(nodes) != 1:
        raise RuntimeError(f"owned control is absent or ambiguous: {value}")
    return nodes[0]


def configure_background_observation(adb, *, keep_seeding=False):
    """Use the real owned app's switches; do not change device power policy."""
    def selected(field, value):
        return unique_observed_control(adb, field, value)
    for field, value in [("content-desc", "More options"), ("text", "Settings"),
                         ("text", "Power Management")]:
        product.tap_bounds(adb, selected(field, value).get("bounds"))
    description = "Continue downloads in background"
    switch = selected("content-desc", description)
    if switch.get("checked") != "false" or switch.get("enabled") != "true":
        raise RuntimeError("background qualification requires the observed default-off switch")
    product.tap_bounds(adb, switch.get("bounds"))
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if selected("content-desc", description).get("checked") == "true":
            break
        time.sleep(.5)
    else:
        raise RuntimeError("background preference did not become enabled")
    preferences = ET.fromstring(adb.shell("run-as", PACKAGE, "cat", "shared_prefs/product_lifecycle.xml").stdout)
    if not any(n.get("name") == "background_downloads_enabled" and n.get("value") == "true" for n in preferences):
        raise RuntimeError("background preference was not persisted")
    listener_port = None
    if keep_seeding:
        switch = selected("content-desc", "Keep seeding in background")
        if switch.get("checked") != "false" or switch.get("enabled") != "true":
            raise RuntimeError("owned seeding policy is not initially default-off and enabled")
        product.tap_bounds(adb, switch.get("bounds"))
        selected("text", "Keep seeding in background?")
        product.tap_bounds(adb, selected("text", "Keep seeding").get("bounds"))
        if selected("content-desc", "Keep seeding in background").get("checked") != "true":
            raise RuntimeError("owned seeding switch did not enable")
        preferences = ET.fromstring(adb.shell("run-as", PACKAGE, "cat", "shared_prefs/product_lifecycle.xml").stdout)
        if not any(n.get("name") == "background_completion_policy" and n.text == "keep_seeding" for n in preferences):
            raise RuntimeError("owned seeding preference did not persist")
        adb.shell("input", "keyevent", "KEYCODE_BACK")
        product.tap_bounds(adb, selected("text", "Network & Privacy").get("bounds"))
        switch = selected("content-desc", "Incoming connections")
        if switch.get("checked") not in ("false", "true") or switch.get("enabled") != "true":
            raise RuntimeError("owned listener control is not observed and eligible")
        if switch.get("checked") == "false":
            product.tap_bounds(adb, switch.get("bounds"))
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            nodes = list(product.dump_ui(adb).iter())
            ports = {int(match.group(1)) for node in nodes
                     for match in re.finditer(r"(?:Listening\([^)]*\bport=|Listening on [^\n]*, port )(\d+)", node.get("text", ""))}
            if len(ports) == 1 and all(0 < port < 65536 for port in ports):
                listener_port = ports.pop()
                break
            time.sleep(.5)
        else:
            raise RuntimeError("actual owned listener port was not observed")
    # Detach the actual view. The existing foreground service owns the transfer.
    adb.shell("input", "keyevent", "KEYCODE_HOME")
    return listener_port


def verify_completed_upload(adb, fixture, registry, target, listener_port):
    """Reuse the existing controlled leecher and its bounded forward cleanup."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "clients/android"))
    import run_bootstrap as bootstrap
    host = json.loads(registry.read_text())["targets"][target]["environment"]["CHROMEBOOK_HOST"]
    class ForwardedAdb:
        def __init__(self):
            self.host = host
        def run(self, arguments, **options):
            return adb.run(*arguments, **options)
    root = fixture.payload_path.parent
    upload_fixture = SimpleNamespace(
        torrent_path=fixture.torrent_path,
        name=root.name,
        expected_file_hashes={str(path.relative_to(root)): hashlib.sha1(path.read_bytes()).hexdigest()
                              for path in root.rglob("*") if path.is_file()},
    )
    return bootstrap.verify_product_upload(ForwardedAdb(), upload_fixture, device_port=listener_port)


def disable_seeding_and_verify_joined_restart(adb, destination, expected_hash):
    uid = re.search(r"\buserId=(\d+)\b", adb.shell("dumpsys", "package", PACKAGE).stdout)
    if uid is None:
        raise RuntimeError("owned restart package UID is unavailable")
    log_args = ("logcat", "-d", "-v", "threadtime", f"--uid={uid[1]}", "-t", "1000")
    baseline = set(adb.run(*log_args).stdout.splitlines())
    def saf_registry():
        preferences = ET.fromstring(adb.shell("run-as", PACKAGE, "cat", "shared_prefs/product-saf.xml").stdout)
        values = [node.text for node in preferences if node.get("name") == "root-registry-v1"]
        if len(values) != 1 or not values[0]:
            raise RuntimeError("owned retained SAF registry is absent or ambiguous")
        return values[0]
    registry_before = saf_registry()
    adb.shell("am", "start", "-W", "-n", ACTIVITY)
    for field, value in [("content-desc", "More options"), ("text", "Settings"), ("text", "Power Management")]:
        product.tap_bounds(adb, unique_observed_control(adb, field, value).get("bounds"))
    switch = unique_observed_control(adb, "content-desc", "Keep seeding in background")
    if switch.get("checked") != "true":
        raise RuntimeError("owned seeding switch was not retained")
    product.tap_bounds(adb, switch.get("bounds"))
    if unique_observed_control(adb, "content-desc", "Keep seeding in background").get("checked") != "false":
        raise RuntimeError("owned seeding switch did not disable")
    adb.shell("input", "keyevent", "KEYCODE_HOME")
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        logs = adb.run(*log_args).stdout.splitlines()
        joined = [line for line in logs if line not in baseline and "product_shutdown_complete" in line]
        services = adb.shell("dumpsys", "activity", "services", PACKAGE).stdout
        if joined and "ServiceRecord{" not in services:
            break
        time.sleep(1)
    else:
        raise RuntimeError("owned completed-file service did not join shutdown")
    adb.shell("am", "start", "-W", "-n", ACTIVITY)
    actual = adb.shell("sha1sum", destination).stdout.split(" ", 1)[0].strip()
    if actual != expected_hash:
        raise RuntimeError("joined restart changed completed payload bytes")
    if saf_registry() != registry_before:
        raise RuntimeError("joined restart changed the retained SAF registry")
    # A delivered launch and unchanged filesystem bytes do not prove that
    # the restarted client finished restoring its retained library.
    expected_name = Path(destination).parent.name
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        labels = {node.get("text", "") for node in product.dump_ui(adb).iter()}
        if "Live" in labels and expected_name in labels:
            break
        time.sleep(.5)
    else:
        raise RuntimeError("joined restart did not restore the live owned library")
    return {"shutdown": joined[-1], "service_absent_after_join": True,
            "reopen_sha1": actual, "retained_saf_registry": "unchanged",
            "reopen_live_library": True}


def native_maximize_arguments(result):
    # The platform adapter writes human-readable queries to stderr. Duplicate
    # ARC accessibility roots may describe the very same native caption button.
    output = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", result.stdout + "\n" + result.stderr)
    count = re.search(r"(?m)^(\d+) matches$", output)
    if count is None:
        raise RuntimeError("native window query has no match count")
    if int(count[1]) == 0:
        return None
    controls = re.findall(r'(?m)^\[button\] "Maximize" at \((-?\d+),(-?\d+)\) (\d+)x(\d+)$', output)
    if len(controls) != int(count[1]) or len(set(controls)) != 1 or any(int(v) <= 0 for v in controls[0][2:]):
        raise RuntimeError("product window has ambiguous native Maximize controls")
    return ["--nth", "1"]


def select_owned_tree(adb):
    # Maximize the owning product window before DocumentsUI inherits its ARC
    # geometry. Maximizing a previous picker does not persist across tasks.
    window = subprocess.run(
        [*adb.command, "testbed", "--", "desktop-find", "^Maximize$", "--role", "button"],
        capture_output=True, text=True, check=True, timeout=30,
    )
    selection = native_maximize_arguments(window)
    if selection is not None:
        subprocess.run(
            [*adb.command, "testbed", "--", "desktop-action", "^Maximize$", "doDefault", "--role", "button", *selection],
            capture_output=True, text=True, check=True, timeout=30,
        )

    def recover_picker():
        # ARC can place DocumentsUI behind the shelf. Its Android button has
        # zero bounds; use the observed native window action, never a guess.
        result = subprocess.run(
            [*adb.command, "testbed", "--", "desktop-find", "^Maximize$", "--role", "button"],
            capture_output=True, text=True, check=True, timeout=30,
        )
        selection = native_maximize_arguments(result)
        if selection is None:
            raise RuntimeError("clipped SAF picker lacks one native Maximize control")
        subprocess.run(
            [*adb.command, "testbed", "--", "desktop-action", "^Maximize$", "doDefault", "--role", "button", *selection],
            capture_output=True, text=True, check=True, timeout=30,
        )
    try:
        product.select_controlled_tree(adb, recover_picker=recover_picker)
    except product.ScenarioFailure as error:
        # The old helper predates 194's binary retained-root registry. Keep its
        # real picker interaction, then inspect the current independently owned
        # registry rather than expecting a plaintext legacy tree-uri value.
        if str(error) != "product did not persist the controlled SAF tree URI":
            raise
    preferences = ET.fromstring(adb.shell("run-as", PACKAGE, "cat", "shared_prefs/product-saf.xml").stdout)
    encoded = next(node.text for node in preferences if node.get("name") == "root-registry-v1")
    registry = base64.urlsafe_b64decode(encoded + "=" * (-len(encoded) % 4))
    if struct.unpack(">II", registry[:8]) != (1, 1) or FOLDER.encode() not in registry:
        raise RuntimeError("owned SAF binding did not commit in the retained-root registry")


def confirm_intake(adb):
    nodes = list(product.dump_ui(adb).iter())
    labels = {"Add"} if any(n.get("text") == "Magnet link from another app" for n in nodes) else {"Download"}
    confirm = product.click_labeled([n for n in nodes if n.get("enabled") == "true"], labels)
    if confirm is not None:
        product.tap_bounds(adb, confirm.get("bounds"))
    return confirm is not None and labels == {"Download"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--machine-control", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--seed-address", required=True, help="explicit controller LAN address")
    parser.add_argument("--seed-port", type=int, default=0, help="explicit permitted TCP port, or 0 for a preflighted dynamic port")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--screenshots", type=Path, help="retain actual ChromeOS captures at product checkpoints")
    parser.add_argument("--observe-seconds", type=int, default=3600)
    parser.add_argument("--observation-lifetime", choices=("foreground", "background"), default="foreground",
                        help="background uses the actual app setting and detaches the Android view")
    parser.add_argument("--diagnostics", type=Path, help="new owned directory for bounded, package-scoped failure/observation logs")
    parser.add_argument("--completed-upload", action="store_true",
                        help="real seeding/listener UI, independent background leecher and joined restart")
    args = parser.parse_args()
    if not 0 <= args.seed_port <= 65535:
        parser.error("seed port must be 0..65535")
    if not 0 <= args.observe_seconds <= 3600:
        parser.error("observation is bounded to 0..3600 seconds; short runs do not close endurance")
    if args.completed_upload and (args.observation_lifetime != "background" or not args.observe_seconds):
        parser.error("completed upload requires a nonzero background observation")
    command = [str(args.machine_control), "--registry", str(args.registry), "--target", args.target]
    adb = RemoteAdb(command)
    package_uid = None
    if args.diagnostics:
        args.diagnostics.mkdir(parents=True, exist_ok=False)
        package_info = adb.shell("dumpsys", "package", PACKAGE).stdout
        uid = re.search(r"\buserId=(\d+)\b", package_info)
        if uid is None:
            raise RuntimeError("owned qualification package UID is unavailable")
        package_uid = uid[1]
    # Existing picker/durable-grant assertions are reused with exclusively owned
    # identities. No fixed-package install/reset function is invoked.
    product.PACKAGE = PACKAGE
    product.ACTIVITY = ACTIVITY
    product.GRANT_FOLDER = FOLDER
    product.GRANT_PATH = ROOT
    product.require_unlocked(adb)
    if not adb.shell("pm", "path", PACKAGE).stdout.strip().startswith("package:"):
        raise RuntimeError("install and inspect the isolated package before running")
    if adb.shell("test", "-e", ROOT, check=False).returncode == 0:
        raise RuntimeError("qualification folder already exists; refusing inherited payload deletion")
    if adb.shell("test", "-e", "/data/local/tmp/rstorrent253-ui.xml", check=False).returncode == 0:
        raise RuntimeError("qualification UI artifact exists; refusing to overwrite")
    run_id = uuid.uuid4().hex
    records = []
    report = {"schema": "chromeos-android-qualification/v1", "cohort": args.target,
              "delivery": "isolated_debug_sideload", "play_installation": "unrun",
              "notification_setup": "granted_after_each_owned_profile_reset",
              "observation_lifetime": args.observation_lifetime,
              "completed_upload_requested": args.completed_upload, "fixture_run_id": run_id,
              "repetitions": records, "result": "fail", "cleanup": "pending"}
    if args.screenshots:
        args.screenshots.mkdir(parents=True, exist_ok=False)
        report["screenshots"] = []

    def capture(name):
        if args.screenshots is None:
            return
        destination = (args.screenshots / f"{name}.png").resolve()
        subprocess.run([*command, "testbed", "--", "screenshot", str(destination)],
                       check=True, capture_output=True, text=True, timeout=45)
        if not destination.is_file() or destination.stat().st_size == 0:
            raise RuntimeError("product screenshot did not produce an artifact")
        report["screenshots"].append(destination.name)

    def diagnostic(name, *, failure=False):
        if args.diagnostics is None:
            return
        operations = [
            ("logcat", ("logcat", "-d", "-v", "threadtime", f"--uid={package_uid}", "-t", "1000")),
            ("activities", ("shell", "dumpsys", "activity", "-p", PACKAGE, "activities")),
            ("services", ("shell", "dumpsys", "activity", "services", PACKAGE)),
        ]
        if failure:
            operations += [
                ("crash-logcat", ("logcat", "-b", "crash", "-d", "-v", "threadtime", f"--uid={package_uid}", "-t", "1000")),
                ("exit-info", ("shell", "dumpsys", "activity", "exit-info", PACKAGE)),
            ]
        for label, arguments in operations:
            try:
                result = adb.run(*arguments, check=False)
                output = (result.stdout + result.stderr)[-65536:]
            except Exception as error:
                output = f"Diagnostic capture failed: {type(error).__name__}"
            (args.diagnostics / f"{name}-{label}.txt").write_text(output)

    session = create_session()
    session.apply_settings({"listen_interfaces": f"{args.seed_address}:{args.seed_port}", "upload_rate_limit": 8 * 1024, "ignore_limits_on_local_network": False})
    handles = []
    try:
        adb.shell("mkdir", "-p", ROOT)
        adb.shell("pm", "grant", PACKAGE, "android.permission.POST_NOTIFICATIONS")
        adb.shell("am", "start", "-W", "-n", ACTIVITY)
        finish_first_use(adb)
        capture("01-first-use-complete")
        select_owned_tree(adb)
        capture("02-download-folder-selected")
        print(json.dumps({"stage": "owned_saf_ready"}), flush=True)
        with tempfile.TemporaryDirectory(prefix="rstorrent-253-seeds-") as temporary:
            directory = Path(temporary)
            port = wait_for_listener(session, [])
            for ordinal in range(1, 4):
                start = time.monotonic()
                fixture = create_owned_fixture(directory / str(ordinal), run_id, str(ordinal), 256 * 1024)
                handle = add_seed(session, fixture.torrent_info, fixture.seed_directory, [])
                handles.append(handle)
                # Cold launch retains the same app/profile/grant. Unique payload
                # names keep repetition records and files independently owned.
                adb.shell("am", "force-stop", PACKAGE)
                adb.shell("am", "start", "-W", "-n", ACTIVITY, "-a", "android.intent.action.VIEW", "-d", magnet_uri(fixture.info_hash, f"{args.seed_address}:{port}"))
                destination = f"{ROOT}/{fixture.payload_path.parent.name}/payload.bin"
                expected = fixture.payload_hash
                deadline = time.monotonic() + 300
                actual = ""
                confirmed = False
                while time.monotonic() < deadline:
                    if not confirmed:
                        confirmed = confirm_intake(adb)
                    actual = adb.shell("sha1sum", destination, check=False).stdout.split(" ", 1)[0].strip()
                    if actual == expected: break
                    time.sleep(2)
                else: raise RuntimeError(f"repetition {ordinal} byte verification timed out")
                capture(f"03-{ordinal}-verified-download")
                # Source-offline restart must retain the verified bytes and grant.
                handle.pause()
                adb.shell("am", "force-stop", PACKAGE)
                adb.shell("am", "start", "-W", "-n", ACTIVITY)
                actual = adb.shell("sha1sum", destination).stdout.split(" ", 1)[0].strip()
                if actual != expected: raise RuntimeError("restart changed downloaded bytes")
                capture(f"04-{ordinal}-source-offline-restart")
                records.append({"ordinal": ordinal, "elapsed_seconds": round(time.monotonic()-start, 2), "bytes": 256*1024, "sha1": actual, "source_offline_restart": "pass"})
                print(json.dumps({"stage": "cold_launch_verified", **records[-1]}), flush=True)
                # Different deterministic torrents use the same exact single-file
                # name. Retire only this owned catalog before the next run.
                adb.shell("am", "force-stop", PACKAGE)
                reset_owned_profile(adb)
                adb.shell("rm", "-f", destination)
                if ordinal < 3 or args.observe_seconds:
                    adb.shell("am", "start", "-W", "-n", ACTIVITY)
                    finish_first_use(adb)
                    select_owned_tree(adb)
            report["result"] = "pass_bounded_repetitions"
            report["observation"] = {"status": "unrun", "seconds": 0}
            if args.observe_seconds:
                # Large metainfo is unrelated to public swarms. Slow the controlled
                # seed so the initial window contains continuous real transfer.
                fixture = create_owned_fixture(directory / "observation", run_id, "observation", observation_payload_size(args.observe_seconds))
                destination = f"{ROOT}/{fixture.payload_path.parent.name}/payload.bin"
                report["observation_fixture"] = {"info_hash": fixture.info_hash, "payload_bytes": fixture.payload_path.stat().st_size, "root_name": fixture.payload_path.parent.name}
                handle = add_seed(session, fixture.torrent_info, fixture.seed_directory, [])
                handles.append(handle)
                # Qualify the real background policy before intake. ARC can
                # detach the view during metadata acquisition; default-off
                # shutdown during that wait is not background-mode evidence.
                if args.observation_lifetime == "background":
                    listener_port = configure_background_observation(adb, keep_seeding=args.completed_upload)
                adb.shell("am", "start", "-W", "-n", ACTIVITY, "-a", "android.intent.action.VIEW", "-d", magnet_uri(fixture.info_hash, f"{args.seed_address}:{port}"))
                deadline = time.monotonic()+300
                while time.monotonic() < deadline:
                    if confirm_intake(adb):
                        break
                    time.sleep(1)
                else: raise RuntimeError("observation intake confirmation timed out")
                if args.observation_lifetime == "background":
                    adb.shell("input", "keyevent", "KEYCODE_HOME")
                    capture("05-background-view-detached")
                beginning = time.monotonic()
                samples = []
                while time.monotonic()-beginning < args.observe_seconds:
                    transferred = int(handle.status().total_payload_upload)
                    samples.append(transferred)
                    report["observation"] = {"status": "running", "seconds": round(time.monotonic()-beginning), "seed_payload_upload": transferred, "completion": "unrun"}
                    if len(samples) % 4 == 1:
                        diagnostic(f"observation-{len(samples):03}")
                    if len(samples) == 3:
                        capture("05-controlled-observation")
                    if len(samples) >= 5 and samples[-1] <= samples[-5]:
                        raise RuntimeError("controlled observation stopped moving for two minutes")
                    print(json.dumps({"stage": "observation", "seconds": round(time.monotonic()-beginning), "seed_payload_upload": transferred}), flush=True)
                    time.sleep(max(0, min(30, args.observe_seconds-(time.monotonic()-beginning))))
                if len(samples) < 2 or samples[-1] <= samples[0]:
                    raise RuntimeError("observation did not show controlled payload movement")
                session.apply_settings({"upload_rate_limit": 0})
                report["completion_seed_limit"] = "released_after_observation"
                verified = False
                deadline = time.monotonic()+300
                while time.monotonic() < deadline:
                    actual = adb.shell("sha1sum", destination, check=False).stdout.split(" ", 1)[0].strip()
                    if actual == fixture.payload_hash:
                        verified = True
                        break
                    time.sleep(2)
                report["observation"] = observation_result(args.observe_seconds, samples[-1], verified, actual)
                if verified:
                    capture("06-observation-verified")
                if not verified:
                    raise RuntimeError("observation completion did not verify within the recovery budget")
                if args.completed_upload:
                    handle.pause()
                    uploaded = verify_completed_upload(adb, fixture, args.registry, args.target, listener_port)
                    report["completed_background_upload"] = {"payload_bytes": uploaded, "sha1": fixture.payload_hash,
                                                             "actual_listener_port": listener_port}
                    diagnostic("completed-background-upload")
                    capture("07-completed-background-upload")
                    report["joined_completion_restart"] = disable_seeding_and_verify_joined_restart(
                        adb, destination, fixture.payload_hash)
                    diagnostic("joined-restart")
                    capture("08-joined-completion-restart")
    except BaseException:
        report["result"] = "fail"
        if report.get("observation", {}).get("status") == "running":
            report["observation"]["status"] = "fail"
        diagnostic("failure", failure=True)
        try:
            capture("failure")
        except Exception as error:
            report["screenshot_failure"] = type(error).__name__
        raise
    finally:
        cleaned = True
        for arguments in [("am", "force-stop", PACKAGE), ("pm", "clear", PACKAGE),
                          ("rm", "-rf", ROOT), ("rm", "-f", "/sdcard/rstorrent-window.xml")]:
            try:
                cleaned = adb.shell(*arguments, check=False).returncode == 0 and cleaned
            except Exception:
                cleaned = False
        session.pause()
        for handle in handles: session.remove_torrent(handle)
        handles.clear()
        session = None
        gc.collect()
        try:
            cleaned = adb.shell("test", "-e", ROOT, check=False).returncode == 1 and cleaned
        except Exception:
            cleaned = False
        report["cleanup"] = "ok" if cleaned else "fail"
        if not cleaned:
            report["result"] = "fail"
        args.output.write_text(json.dumps(report, indent=2)+"\n")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
