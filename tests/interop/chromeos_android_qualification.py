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
from pathlib import Path

import android_reactive_surface as product
from first_verified_piece import add_seed, create_session, wait_for_listener
from magnet_metadata import create_fixture, magnet_uri

PACKAGE = "org.rstorrent.qualification253"
ACTIVITY = f"{PACKAGE}/org.rstorrent.bootstrap.MainActivity"
FOLDER = "RSTorrentQualification253"
ROOT = f"/sdcard/Download/{FOLDER}"

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
            raise RuntimeError(f"owned Android command failed ({result.returncode}): {result.stdout[-1500:]}")
        return result

    def shell(self, *arguments: str, **options):
        return self.run("shell", *arguments, **options)


def finish_first_use(adb):
    deadline = time.monotonic()+20
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


def select_owned_tree(adb):
    try:
        product.select_controlled_tree(adb)
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
    args = parser.parse_args()
    if not 0 <= args.seed_port <= 65535:
        parser.error("seed port must be 0..65535")
    if not 0 <= args.observe_seconds <= 3600:
        parser.error("observation is bounded to 0..3600 seconds; short runs do not close endurance")
    command = [str(args.machine_control), "--registry", str(args.registry), "--target", args.target]
    adb = RemoteAdb(command)
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
    records = []
    report = {"schema": "chromeos-android-qualification/v1", "cohort": args.target,
              "delivery": "isolated_debug_sideload", "play_installation": "unrun",
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
                fixture = create_fixture(directory / str(ordinal), payload_size=256 * 1024, root_name=f"qualification253-{ordinal}")
                handle = add_seed(session, fixture.torrent_info, fixture.seed_directory, [])
                handles.append(handle)
                # Cold launch retains the same app/profile/grant. Unique payload
                # names keep repetition records and files independently owned.
                adb.shell("am", "force-stop", PACKAGE)
                adb.shell("am", "start", "-W", "-n", ACTIVITY, "-a", "android.intent.action.VIEW", "-d", magnet_uri(fixture.info_hash, f"{args.seed_address}:{port}"))
                destination = f"{ROOT}/qualification253-{ordinal}/payload.bin"
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
                adb.shell("pm", "clear", PACKAGE)
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
                fixture = create_fixture(directory / "observation", payload_size=28*1024*1024, root_name="qualification253-observation")
                handle = add_seed(session, fixture.torrent_info, fixture.seed_directory, [])
                handles.append(handle)
                adb.shell("am", "start", "-W", "-n", ACTIVITY, "-a", "android.intent.action.VIEW", "-d", magnet_uri(fixture.info_hash, f"{args.seed_address}:{port}"))
                deadline = time.monotonic()+300
                while time.monotonic() < deadline:
                    if confirm_intake(adb):
                        break
                    time.sleep(1)
                else: raise RuntimeError("observation intake confirmation timed out")
                beginning = time.monotonic()
                samples = []
                while time.monotonic()-beginning < args.observe_seconds:
                    transferred = int(handle.status().total_payload_upload)
                    samples.append(transferred)
                    if len(samples) == 3:
                        capture("05-controlled-observation")
                    if len(samples) >= 5 and samples[-1] <= samples[-5]:
                        raise RuntimeError("controlled observation stopped moving for two minutes")
                    print(json.dumps({"stage": "observation", "seconds": round(time.monotonic()-beginning), "seed_payload_upload": transferred}), flush=True)
                    time.sleep(max(0, min(30, args.observe_seconds-(time.monotonic()-beginning))))
                if len(samples) < 2 or samples[-1] <= samples[0]:
                    raise RuntimeError("observation did not show controlled payload movement")
                verified = False
                deadline = time.monotonic()+300
                while time.monotonic() < deadline:
                    actual = adb.shell("sha1sum", f"{ROOT}/qualification253-observation/payload.bin", check=False).stdout.split(" ", 1)[0].strip()
                    if actual == fixture.payload_hash:
                        verified = True
                        break
                    time.sleep(2)
                report["observation"] = {"status": "pass" if args.observe_seconds == 3600 and verified else "bounded_short_run", "seconds": args.observe_seconds, "seed_payload_upload": samples[-1], "completion": "pass" if verified else "unrun", "sha1": actual if verified else None}
                if verified:
                    capture("06-observation-verified")
                if args.observe_seconds == 3600 and not verified:
                    raise RuntimeError("observation completion did not verify within the recovery budget")
    except BaseException:
        report["result"] = "fail"
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
