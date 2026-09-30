#!/usr/bin/env python3
"""Prove a released JSTorrent -> RSTorrent upgrade on a fresh owned emulator.

The released APK's picker writes real roots/grants. Default mode seeds source
fixtures; --source ordinary drives old UI/intake writers and loopback downloads.
Both APKs and instrumentation use one disposable certificate; no release key.
"""
from __future__ import annotations
import argparse
import base64
from contextlib import ExitStack
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
ANDROID = ROOT / "clients/android"
PACKAGE = "com.jstorrent.app"
SHA256 = "a938b46825157668e804d1efbf01a2cce461a8e23ce1028e909aa919145cfa84"
URL = "https://github.com/kzahel/JSTorrent/releases/download/android-v1.0.24/app-release.apk"


def load_matrix():
    path = Path(__file__).with_name("run-localization-matrix.py")
    spec = importlib.util.spec_from_file_location("legacy_upgrade_matrix", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def run(args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def instrument(target, method, writer_spec=None):
    arguments = ["am", "instrument", "-w", "-r", "-e", "legacyUpgrade", "true", "-e", "class", f"org.rstorrent.bootstrap.LegacyAndroidUpgradeTest#{method}", f"{PACKAGE}.test/androidx.test.runner.AndroidJUnitRunner"]
    if writer_spec is not None: arguments[2:2] = ["-e", "writerSpec", base64.b64encode(json.dumps(writer_spec, separators=(",", ":")).encode()).decode()]
    result = target.shell(arguments, timeout=150)
    print(result.stdout, flush=True)
    if "OK (1 test)" not in result.stdout or "FAILURES!!!" in result.stdout:
        print(target.shell(["logcat", "-b", "crash", "-d"], check=False).stdout, flush=True)
        print(target.shell(["logcat", "-d", "-s", "RSTorrentProduct:I", "System.err:W"], check=False).stdout[-16000:], flush=True)
        raise RuntimeError(f"upgrade phase failed: {method}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--api", choices=["28", "35"], default="35")
    parser.add_argument("--source", choices=["seeded", "ordinary"], default="seeded")
    args = parser.parse_args()
    matrix = load_matrix()
    avdmanager = matrix.sdk_tool("avdmanager")
    name = f"rstorrent-legacy-upgrade-api{args.api}"
    created = False
    session = None
    sdk = Path(os.environ["ANDROID_HOME"])
    signer = sdk / "build-tools/36.0.0/apksigner"
    if not signer.exists(): signer = sdk / "build-tools/35.0.0/apksigner"
    try:
        with tempfile.TemporaryDirectory(prefix="rstorrent-legacy-upgrade-") as owned, ExitStack() as resources:
            owned = Path(owned)
            original = owned / "released.apk"
            urllib.request.urlretrieve(URL, original)
            if hashlib.sha256(original.read_bytes()).hexdigest() != SHA256: raise RuntimeError("released APK pin mismatch")
            run([str(signer), "verify", "--print-certs", str(original)])
            key = owned / "test.p12"
            run(["keytool", "-genkeypair", "-keystore", str(key), "-storepass", "upgrade-test", "-keypass", "upgrade-test", "-alias", "upgrade", "-keyalg", "RSA", "-keysize", "2048", "-validity", "2", "-dname", "CN=Disposable RSTorrent upgrade test"])
            # Generated native bindings/ABIs must already be built by build.sh.
            run(["./gradlew", "-PlegacyUpgradeTestPackage=" + PACKAGE, "assembleDebug", "assembleDebugAndroidTest"], cwd=ANDROID)
            apks = {}
            for label, source in [("old", original), ("new", ANDROID / "app/build/outputs/apk/debug/app-debug.apk"), ("test", ANDROID / "app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk"), ("seed", ANDROID / "legacy-upgrade-fixture/build/outputs/apk/debug/legacy-upgrade-fixture-debug.apk")]:
                signed = owned / f"{label}.apk"
                run([str(signer), "sign", "--ks", str(key), "--ks-pass", "pass:upgrade-test", "--ks-key-alias", "upgrade", "--out", str(signed), str(source)])
                run([str(signer), "verify", str(signed)])
                apks[label] = signed
            matrix.create_avd(avdmanager, name, args.api, matrix.system_image_abi()); created = True
            probe = matrix.load_probe()
            session = probe.start_avd(name, args.api)
            target = session.target
            if args.source == "ordinary":
                from legacy_writer_upgrade import WriterOracle, prepare
                oracle = WriterOracle(owned)
                resources.callback(oracle.close)
                target.run(["install", str(apks["old"])], timeout=120)
                try:
                    spec = prepare(target, probe, owned, oracle)
                except BaseException:
                    print([alert.message() for alert in oracle.session.pop_alerts()][-50:], flush=True)
                    print([(n.attrib.get("text"), n.attrib.get("content-desc")) for n in probe.ui_nodes(target) if n.attrib.get("text") or n.attrib.get("content-desc")], flush=True)
                    print(target.shell(["logcat", "-d", "-t", "500"], check=False).stdout[-24000:], flush=True)
                    raise
                # Android itself quiesces the running old package during install.
                target.run(["install", "-r", str(apks["new"])], timeout=120)
                target.run(["install", str(apks["test"])], timeout=120)
                if target.shell(["pidof", PACKAGE], check=False).stdout.strip():
                    raise RuntimeError("old process survived package replacement")
                oracle.release_rate_limit()
                instrument(target, "verifyOrdinaryWriterUpgrade", spec)
                target.shell(["am", "force-stop", PACKAGE])
                instrument(target, "verifyOrdinaryWriterUpgrade", spec)
                target.run(["reboot"])
                target.run(["wait-for-device"], timeout=90)
                deadline = time.monotonic() + 90
                while target.property("sys.boot_completed") != "1":
                    if time.monotonic() >= deadline: raise RuntimeError("upgrade reboot did not complete")
                    time.sleep(0.5)
                target.shell(["input", "keyevent", "KEYCODE_WAKEUP"])
                target.shell(["wm", "dismiss-keyguard"])
                instrument(target, "verifyOrdinaryWriterUpgrade", spec)
                print(json.dumps({"ordinary_writer_upgrade": {"api": int(args.api), "source": "android-v1.0.24", "source_apk_sha256": SHA256, "settings_writer": "released Compose UI", "torrent_writer": "released content intake and engine", "libtorrent_version": oracle.lt.__version__, "roots": 2, "controlled_download": "passed", "running_replacement": "passed", "partial_resume": "passed", "upgrade_restart_reboot": "passed"}}, indent=2))
                return
            target.run(["install", str(apks["old"])], timeout=120)
            target.run(["install", str(apks["seed"])], timeout=120)
            target.shell(["mkdir", "-p", "/sdcard/Download/JSTorrent"])
            target.shell(["am", "start", "-n", PACKAGE + "/com.jstorrent.app.AddRootActivity"])
            probe.automate_tree_grant(target, "internal", "JSTorrent")
            # Picker confirmation precedes the old activity's persisted writer.
            deadline = time.monotonic() + 30
            while "Added root: key=" not in target.shell(["logcat", "-d", "-s", "AddRootActivity:I"]).stdout:
                if time.monotonic() >= deadline: raise RuntimeError("released picker did not commit its root")
                time.sleep(0.25)
            seed = target.shell(["am", "instrument", "-w", "org.rstorrent.legacyfixture/org.rstorrent.legacyfixture.LegacySourceInstrumentation"], timeout=120)
            print(seed.stdout, flush=True)
            if "legacy_seed=passed" not in seed.stdout:
                print(target.shell(["logcat", "-b", "crash", "-d"], check=False).stdout, flush=True)
                raise RuntimeError("released source seed failed")
            target.shell(["am", "force-stop", PACKAGE])
            target.run(["install", "-r", str(apks["new"])], timeout=120)
            target.run(["install", str(apks["test"])], timeout=120)
            instrument(target, "verifyReplacement")
            target.shell(["am", "force-stop", PACKAGE])
            instrument(target, "verifyReplacement")
            target.shell(["am", "force-stop", PACKAGE])
            instrument(target, "revokeGrant")
            target.shell(["am", "force-stop", PACKAGE])
            instrument(target, "verifyRevoked")
            target.shell(["am", "force-stop", PACKAGE])
            target.shell(["logcat", "-c"])
            target.shell(["am", "start", "-n", PACKAGE + "/org.rstorrent.bootstrap.MainActivity", "--ez", "product_select_saf", "true"])
            probe.automate_tree_grant(target, "internal", "JSTorrent")
            deadline = time.monotonic() + 30
            while "saf_tree_ready root=" not in target.shell(["logcat", "-d", "-s", "RSTorrentProduct:I"]).stdout:
                if time.monotonic() >= deadline: raise RuntimeError("successor did not finish SAF regrant")
                time.sleep(0.25)
            target.shell(["am", "force-stop", PACKAGE])
            instrument(target, "verifyReplacement")
            target.shell(["am", "force-stop", PACKAGE])
            instrument(target, "verifyClearDoesNotReimport")
            target.shell(["am", "force-stop", PACKAGE])
            instrument(target, "verifyRootOnlyMigration")
            target.shell(["am", "force-stop", PACKAGE])
            instrument(target, "verifyFreshInstall")
            print(json.dumps({"legacy_upgrade": {"api": int(args.api), "source": "android-v1.0.24", "source_apk_sha256": SHA256, "package": PACKAGE, "seed": "independently authored source-format data; released picker writes root/grant", "upgrade": "passed", "restart": "passed", "revoked_grant": "passed", "regrant": "passed", "clear_no_resurrection": "passed", "roots_only": "passed", "fresh_install": "passed"}}, indent=2))
    finally:
        if session is not None: session.close()
        if created: matrix.delete_avd(avdmanager, name)
        # Leave build outputs under the normal incubation identity as well.
        run(["./gradlew", "assembleDebug", "assembleDebugAndroidTest"], cwd=ANDROID)


if __name__ == "__main__": main()
