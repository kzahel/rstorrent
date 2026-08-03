#!/usr/bin/env python3
"""Build and run the tactical 003 Android storage probe on an explicit target."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence


PACKAGE = "org.rstorrent.storageprobe"
ACTIVITY = f"{PACKAGE}/.MainActivity"
EXPECTED_AVD = "jstorrent-tablet"
EXPECTED_AVD_API = "34"
EXPECTED_CHROMEOS_API = "33"
EXPECTED_CHROMEOS_MODEL = "nami"
EXPECTED_CHROMEOS_DEVICE = "nami_cheets"
CHROMEOS_SERIAL = "emulator-5554"
PIXEL_SERIAL = "33031JEHN17672"
EXPECTED_PIXEL_API = "37"
EXPECTED_PIXEL_MODEL = "Pixel 7a"
EXPECTED_PIXEL_DEVICE = "lynx"
MOTO_SERIAL = "ZY224JN8D2"
EXPECTED_MOTO_API = "28"
EXPECTED_MOTO_MODEL = "moto x4"
EXPECTED_MOTO_DEVICE = "payton_sprout"
MOTO_SD_VOLUME = "F69D-D340"
MOTO_SD_INITIAL_URI = (
    f"content://com.android.externalstorage.documents/root/{MOTO_SD_VOLUME}"
)
MOTO_SD_ROOT_LABELS = [MOTO_SD_VOLUME, "SD card", "New Volume"]
RESULT_PATH = "files/result.json"
POLL_SECONDS = 45
GRANT_FOLDER = "RSTorrentStorageProbeGrant"


class ProbeFailure(RuntimeError):
    pass


@dataclass
class CommandResult:
    stdout: str
    stderr: str
    returncode: int


class AdbTarget:
    def __init__(self, prefix: Sequence[str], name: str) -> None:
        self.prefix = list(prefix)
        self.name = name

    def run(
        self,
        arguments: Sequence[str],
        *,
        timeout: float = 30,
        check: bool = True,
    ) -> CommandResult:
        completed = subprocess.run(
            [*self.prefix, *arguments],
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        result = CommandResult(
            stdout=completed.stdout,
            stderr=completed.stderr,
            returncode=completed.returncode,
        )
        if check and completed.returncode != 0:
            raise ProbeFailure(
                f"{self.name} command failed: {' '.join(arguments)}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
        return result

    def shell(
        self,
        arguments: Sequence[str],
        *,
        timeout: float = 30,
        check: bool = True,
    ) -> CommandResult:
        return self.run(["shell", *arguments], timeout=timeout, check=check)

    def property(self, name: str) -> str:
        return self.shell(["getprop", name]).stdout.strip()


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def probe_root() -> Path:
    return Path(__file__).resolve().parent


def run_host(
    arguments: Sequence[str],
    *,
    timeout: float = 120,
    check: bool = True,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        list(arguments),
        cwd=repository_root(),
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
        env=env,
    )
    if check and completed.returncode != 0:
        raise ProbeFailure(
            f"host command failed: {' '.join(arguments)}\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    return completed


def build_probe() -> Path:
    completed = run_host(
        [str(probe_root() / "build_probe.sh")],
        timeout=300,
    )
    output = completed.stdout.strip().splitlines()
    apk = Path(output[-1]) if output else Path()
    if not apk.is_file():
        raise ProbeFailure(
            "build completed without an APK path\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    return apk


def local_adb_path() -> Path:
    configured = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    sdk = Path(configured) if configured else Path.home() / "Android" / "Sdk"
    adb = sdk / "platform-tools" / "adb"
    if not adb.is_file():
        raise ProbeFailure(f"ADB is unavailable at {adb}")
    return adb


def emulator_path() -> Path:
    configured = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    sdk = Path(configured) if configured else Path.home() / "Android" / "Sdk"
    emulator = sdk / "emulator" / "emulator"
    if not emulator.is_file():
        raise ProbeFailure(f"Android emulator is unavailable at {emulator}")
    return emulator


def adb_devices(adb: Path) -> list[str]:
    completed = run_host([str(adb), "devices"], timeout=15)
    devices = []
    for line in completed.stdout.splitlines()[1:]:
        fields = line.split()
        if len(fields) >= 2 and fields[1] == "device":
            devices.append(fields[0])
    return devices


def running_avd_name(adb: Path, serial: str) -> str | None:
    completed = run_host(
        [str(adb), "-s", serial, "emu", "avd", "name"],
        timeout=10,
        check=False,
    )
    if completed.returncode != 0:
        return None
    lines = [
        line.strip()
        for line in completed.stdout.splitlines()
        if line.strip() and line.strip() != "OK"
    ]
    return lines[0] if lines else None


@dataclass
class AvdSession:
    target: AdbTarget
    process: subprocess.Popen[str]
    log_path: Path

    def close(self) -> None:
        self.target.run(["emu", "kill"], timeout=15, check=False)
        try:
            self.process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        try:
            self.log_path.unlink()
        except FileNotFoundError:
            pass


def start_avd(avd_name: str) -> AvdSession:
    if avd_name != EXPECTED_AVD:
        raise ProbeFailure(
            f"refusing unlisted AVD {avd_name!r}; expected {EXPECTED_AVD!r}"
        )
    adb = local_adb_path()
    emulator = emulator_path()
    run_host([str(adb), "start-server"], timeout=15)
    for serial in adb_devices(adb):
        if serial.startswith("emulator-") and running_avd_name(adb, serial) == avd_name:
            raise ProbeFailure(
                f"{avd_name} is already running as {serial}; stop it so the probe "
                "can own and clean a fresh emulator session"
            )

    log_handle = tempfile.NamedTemporaryFile(
        prefix="rstorrent-avd-",
        suffix=".log",
        delete=False,
        mode="w",
        encoding="utf-8",
    )
    log_path = Path(log_handle.name)
    process = subprocess.Popen(
        [
            str(emulator),
            f"@{avd_name}",
            "-no-window",
            "-no-audio",
            "-no-boot-anim",
            "-no-snapshot",
            "-wipe-data",
            "-gpu",
            "swiftshader_indirect",
        ],
        cwd=repository_root(),
        stdout=log_handle,
        stderr=subprocess.STDOUT,
        text=True,
    )
    log_handle.close()

    deadline = time.monotonic() + 180
    serial: str | None = None
    while time.monotonic() < deadline:
        if process.poll() is not None:
            detail = log_path.read_text(encoding="utf-8", errors="replace")
            raise ProbeFailure(f"AVD exited during startup\n{detail}")
        for candidate in adb_devices(adb):
            if (
                candidate.startswith("emulator-")
                and running_avd_name(adb, candidate) == avd_name
            ):
                serial = candidate
                break
        if serial:
            target = AdbTarget([str(adb), "-s", serial], f"AVD {serial}")
            if target.property("sys.boot_completed") == "1":
                target.shell(["wm", "dismiss-keyguard"], check=False)
                return AvdSession(target=target, process=process, log_path=log_path)
        time.sleep(1)

    process.terminate()
    detail = log_path.read_text(encoding="utf-8", errors="replace")
    raise ProbeFailure(f"AVD did not boot before timeout\n{detail}")


def prepare_chromeos() -> AdbTarget:
    testbed = Path.home() / "code" / "chromeos-testbed" / "bin" / "chromeos"
    if not testbed.is_file():
        raise ProbeFailure(f"ChromeOS testbed is unavailable at {testbed}")
    run_host([str(testbed), "doctor"], timeout=60)
    run_host([str(testbed), "adb-connect"], timeout=30)
    target = AdbTarget(
        ["ssh", "chromeroot", "adb", "-s", CHROMEOS_SERIAL],
        "Chromebook ARCVM",
    )
    if target.run(["get-state"]).stdout.strip() != "device":
        raise ProbeFailure("Chromebook ARCVM ADB target is not ready")
    return target


def prepare_pixel() -> AdbTarget:
    adb = local_adb_path()
    if PIXEL_SERIAL not in adb_devices(adb):
        raise ProbeFailure(
            f"the expected Pixel 7a is not ready as serial {PIXEL_SERIAL}"
        )
    target = AdbTarget(
        [str(adb), "-s", PIXEL_SERIAL],
        f"Pixel 7a {PIXEL_SERIAL}",
    )
    if target.run(["get-state"]).stdout.strip() != "device":
        raise ProbeFailure("the expected Pixel 7a ADB target is not ready")
    return target


def prepare_moto() -> AdbTarget:
    adb = local_adb_path()
    if MOTO_SERIAL not in adb_devices(adb):
        raise ProbeFailure(
            f"the expected Moto X4 is not ready as serial {MOTO_SERIAL}"
        )
    target = AdbTarget(
        [str(adb), "-s", MOTO_SERIAL],
        f"Moto X4 {MOTO_SERIAL}",
    )
    if target.run(["get-state"]).stdout.strip() != "device":
        raise ProbeFailure("the expected Moto X4 ADB target is not ready")
    return target


def verify_target(target: AdbTarget, kind: str) -> dict[str, str]:
    api = target.property("ro.build.version.sdk")
    model = target.property("ro.product.model")
    device = target.property("ro.product.device")
    abis = target.property("ro.product.cpu.abilist")
    fingerprint = target.property("ro.build.fingerprint")
    if kind == "avd":
        variants = (
            (EXPECTED_AVD_API, "sdk_gphone64_x86_64", "emu64xa", "x86_64"),
            (EXPECTED_AVD_API, "sdk_gphone64_arm64", "emu64a", "arm64-v8a"),
        )
        matched = next(
            (
                variant
                for variant in variants
                if (api, model, device) == variant[:3]
            ),
            None,
        )
        if matched is None:
            raise ProbeFailure(
                f"refusing unexpected avd target: api={api}, model={model}, "
                f"device={device}; expected one of {variants}"
            )
        expected_abi = matched[3]
        if expected_abi not in abis.split(","):
            raise ProbeFailure(f"target lacks packaged {expected_abi} ABI: {abis}")
        return {
            "api": api,
            "model": model,
            "device": device,
            "abis": abis,
            "fingerprint": fingerprint,
        }
    expected = {
        "chromeos": (
            EXPECTED_CHROMEOS_API,
            EXPECTED_CHROMEOS_MODEL,
            EXPECTED_CHROMEOS_DEVICE,
            "x86_64",
        ),
        "pixel7a": (
            EXPECTED_PIXEL_API,
            EXPECTED_PIXEL_MODEL,
            EXPECTED_PIXEL_DEVICE,
            "arm64-v8a",
        ),
        "motox4": (
            EXPECTED_MOTO_API,
            EXPECTED_MOTO_MODEL,
            EXPECTED_MOTO_DEVICE,
            "arm64-v8a",
        ),
    }
    expected_api, expected_model, expected_device, expected_abi = expected[kind]
    if (
        api != expected_api
        or model != expected_model
        or device != expected_device
    ):
        raise ProbeFailure(
            f"refusing unexpected {kind} target: api={api}, model={model}, "
            f"device={device}; expected api={expected_api}, "
            f"model={expected_model}, device={expected_device}"
        )
    if expected_abi not in abis.split(","):
        raise ProbeFailure(f"target lacks packaged {expected_abi} ABI: {abis}")
    return {
        "api": api,
        "model": model,
        "device": device,
        "abis": abis,
        "fingerprint": fingerprint,
    }


def verify_storage(
    target: AdbTarget,
    kind: str,
    storage: str,
) -> dict[str, str]:
    if storage == "internal":
        return {"storage_volume": "primary"}
    if kind != "motox4":
        raise ProbeFailure("removable storage is supported only on motox4")
    volumes = target.shell(["sm", "list-volumes", "all"]).stdout
    mounted = False
    for line in volumes.splitlines():
        fields = line.split()
        if (
            len(fields) >= 3
            and fields[0].startswith("public:")
            and fields[1:] == ["mounted", MOTO_SD_VOLUME]
        ):
            mounted = True
            break
    if not mounted:
        raise ProbeFailure(
            f"removable volume {MOTO_SD_VOLUME} is not mounted\n{volumes}"
        )
    mount_path = f"/mnt/media_rw/{MOTO_SD_VOLUME}"
    underlying_filesystem = ""
    mounts = target.shell(["cat", "/proc/mounts"]).stdout
    for line in mounts.splitlines():
        fields = line.split()
        if len(fields) >= 3 and fields[1] == mount_path:
            underlying_filesystem = fields[2]
            break
    if not underlying_filesystem:
        raise ProbeFailure(
            f"could not resolve the filesystem mounted at {mount_path}"
        )
    return {
        "storage_volume": MOTO_SD_VOLUME,
        "underlying_filesystem": underlying_filesystem,
    }


def install_apk(target: AdbTarget, kind: str, apk: Path) -> None:
    if kind == "chromeos":
        testbed = Path.home() / "code" / "chromeos-testbed" / "bin" / "chromeos"
        run_host([str(testbed), "install-apk", str(apk)], timeout=120)
    else:
        target.run(["install", "-r", str(apk)], timeout=120)


def parse_bounds(value: str) -> tuple[int, int]:
    match = re.fullmatch(r"\[(\d+),(\d+)]\[(\d+),(\d+)]", value)
    if not match:
        raise ProbeFailure(f"invalid UI bounds {value!r}")
    left, top, right, bottom = map(int, match.groups())
    return ((left + right) // 2, (top + bottom) // 2)


def ui_nodes(target: AdbTarget) -> list[ET.Element]:
    remote = "/sdcard/rstorrent-storage-probe-window.xml"
    target.shell(["uiautomator", "dump", remote], timeout=15, check=False)
    xml = target.shell(["cat", remote], timeout=10, check=False).stdout
    target.shell(["rm", "-f", remote], timeout=10, check=False)
    if not xml.lstrip().startswith("<?xml"):
        return []
    try:
        return list(ET.fromstring(xml).iter("node"))
    except ET.ParseError:
        return []


def click_from_nodes(
    target: AdbTarget,
    nodes: Sequence[ET.Element],
    labels: Sequence[str],
) -> bool:
    wanted = {label.casefold() for label in labels}
    for node in nodes:
        text = node.attrib.get("text", "").strip().casefold()
        description = node.attrib.get("content-desc", "").strip().casefold()
        if text not in wanted and description not in wanted:
            continue
        if node.attrib.get("enabled", "true") != "true":
            continue
        x, y = parse_bounds(node.attrib.get("bounds", ""))
        target.shell(["input", "tap", str(x), str(y)])
        return True
    return False


def click_document_entry(
    target: AdbTarget,
    nodes: Sequence[ET.Element],
    labels: Sequence[str],
) -> bool:
    entries = [
        node
        for node in nodes
        if node.attrib.get("resource-id") == "android:id/title"
    ]
    return click_from_nodes(target, entries, labels)


def grant_path(storage: str) -> str:
    if storage == "sdcard":
        return f"/storage/{MOTO_SD_VOLUME}/{GRANT_FOLDER}"
    return f"/sdcard/Download/{GRANT_FOLDER}"


def remove_grant_folder(target: AdbTarget, storage: str) -> None:
    path = grant_path(storage)
    exists = target.shell(["test", "-e", path], check=False)
    if exists.returncode != 0:
        return
    directory = target.shell(["test", "-d", path], check=False)
    if directory.returncode != 0:
        raise ProbeFailure(f"probe grant path is not a directory: {path}")
    removed = target.shell(["rmdir", path], check=False)
    if removed.returncode != 0:
        raise ProbeFailure(f"probe grant directory is not empty: {path}")


def prepare_grant_folder(target: AdbTarget, storage: str) -> None:
    remove_grant_folder(target, storage)
    path = grant_path(storage)
    created = target.shell(["mkdir", path], check=False)
    if created.returncode != 0:
        raise ProbeFailure(
            f"could not create probe grant directory: {path}\n"
            f"stdout:\n{created.stdout}\n"
            f"stderr:\n{created.stderr}"
        )


def automate_tree_grant(target: AdbTarget, storage: str) -> None:
    target.shell(
        ["input", "keyevent", "KEYCODE_WAKEUP"],
        timeout=10,
        check=False,
    )
    time.sleep(0.5)
    deadline = time.monotonic() + 60
    used_folder = False
    entered_folder = False
    opened_roots = False
    documents_ui_since: float | None = None
    while time.monotonic() < deadline:
        nodes = ui_nodes(target)
        if not nodes:
            time.sleep(0.4)
            continue
        showing_documents_ui = any(
            "documentsui" in node.attrib.get("package", "")
            for node in nodes
        )
        if showing_documents_ui and documents_ui_since is None:
            documents_ui_since = time.monotonic()
        if any(
            node.attrib.get("text", "").strip() == GRANT_FOLDER
            and node.attrib.get("resource-id", "").endswith(
                ":id/breadcrumb_text"
            )
            for node in nodes
        ):
            entered_folder = True
        if entered_folder and not used_folder and click_from_nodes(
            target,
            nodes,
            ["Use this folder", "USE THIS FOLDER", "Select", "SELECT"],
        ):
            used_folder = True
            time.sleep(0.5)
            continue
        if used_folder and click_from_nodes(target, nodes, ["Allow", "ALLOW"]):
            return
        if used_folder and not any(
            "documentsui" in node.attrib.get("package", "")
            for node in nodes
        ):
            return
        if (
            not used_folder
            and click_document_entry(target, nodes, [GRANT_FOLDER])
        ):
            entered_folder = True
            time.sleep(0.5)
            continue
        root_labels = (
            ["Download", "Downloads"]
            if storage == "internal"
            else MOTO_SD_ROOT_LABELS
        )
        drawer_start = next(
            (
                index
                for index, node in enumerate(nodes)
                if node.attrib.get("text") == "Open from"
            ),
            None,
        )
        if opened_roots and drawer_start is not None:
            drawer_nodes = nodes[drawer_start:]
            if click_document_entry(
                target,
                drawer_nodes,
                [*root_labels, "My files"],
            ):
                time.sleep(0.5)
                continue
            if storage == "internal":
                local_roots = [
                    node
                    for node in drawer_nodes
                    if node.attrib.get("resource-id") == "android:id/title"
                ]
                if local_roots and click_from_nodes(
                    target,
                    local_roots[:1],
                    [local_roots[0].attrib.get("text", "")],
                ):
                    time.sleep(0.5)
                    continue
        if (
            not entered_folder
            and drawer_start is None
            and click_document_entry(target, nodes, root_labels)
        ):
            time.sleep(0.5)
            continue
        if (
            not entered_folder
            and not opened_roots
            and documents_ui_since is not None
            and time.monotonic() - documents_ui_since >= 4
            and click_from_nodes(target, nodes, ["Show roots"])
        ):
            opened_roots = True
            time.sleep(0.5)
            continue
        time.sleep(0.4)
    raise ProbeFailure(
        f"could not grant the {storage} document tree through system UI"
    )


def remove_result(target: AdbTarget) -> None:
    target.shell(
        ["run-as", PACKAGE, "rm", "-f", RESULT_PATH],
        check=False,
    )


def read_result(target: AdbTarget, expected_phase: str) -> dict:
    deadline = time.monotonic() + POLL_SECONDS
    last_output = ""
    while time.monotonic() < deadline:
        completed = target.shell(
            ["run-as", PACKAGE, "cat", RESULT_PATH],
            timeout=10,
            check=False,
        )
        last_output = completed.stdout.strip()
        if last_output:
            try:
                result = json.loads(last_output)
            except json.JSONDecodeError:
                time.sleep(0.3)
                continue
            if result.get("phase") == expected_phase:
                return result
        time.sleep(0.3)
    logcat = target.run(
        ["logcat", "-d", "-t", "300"],
        timeout=20,
        check=False,
    ).stdout
    raise ProbeFailure(
        f"timed out waiting for phase {expected_phase}; last result={last_output!r}\n"
        f"logcat tail:\n{logcat}"
    )


def launch_phase(target: AdbTarget, phase: str, storage: str) -> None:
    arguments = [
        "am",
        "start",
        "--activity-clear-task",
        "-n",
        ACTIVITY,
        "--es",
        "mode",
        phase,
        "--es",
        "storage",
        storage,
    ]
    if phase == "acquire" and storage == "sdcard":
        arguments.extend(["--es", "tree_initial_uri", MOTO_SD_INITIAL_URI])
    launched = target.shell(arguments, timeout=30)
    if "Starting:" not in launched.stdout or "Error:" in launched.stdout:
        raise ProbeFailure(
            f"activity launch did not report success for phase {phase}\n"
            f"stdout:\n{launched.stdout}\n"
            f"stderr:\n{launched.stderr}"
        )


def cleanup_after_failure(target: AdbTarget, storage: str) -> None:
    try:
        remove_result(target)
        launch_phase(target, "cleanup", storage)
        read_result(target, "cleanup")
    except Exception as error:
        print(f"probe application cleanup failed: {error}", file=sys.stderr)
    try:
        remove_grant_folder(target, storage)
    except Exception as error:
        print(f"probe grant cleanup failed: {error}", file=sys.stderr)


def run_cycle(
    target: AdbTarget,
    target_name: str,
    storage: str,
    ordinal: int,
) -> dict:
    target.shell(["am", "force-stop", PACKAGE], check=False)
    cleared = target.shell(["pm", "clear", PACKAGE], check=False)
    if cleared.returncode != 0 or "Success" not in cleared.stdout:
        raise ProbeFailure(f"could not clear probe application data: {cleared.stdout}")
    target.run(["logcat", "-c"], check=False)
    prepare_grant_folder(target, storage)
    remove_result(target)
    launch_phase(target, "acquire", storage)
    automate_tree_grant(target, storage)
    initial = read_result(target, "initial")
    if not initial.get("success"):
        raise ProbeFailure(f"initial probe failed: {json.dumps(initial, indent=2)}")
    document_id = initial.get("provider", {}).get("document_id", "")
    if target_name == "motox4":
        expected_prefix = (
            f"raw:/storage/emulated/0/Download/{GRANT_FOLDER}/"
            if storage == "internal"
            else f"{MOTO_SD_VOLUME}:{GRANT_FOLDER}/"
        )
    else:
        expected_prefix = "primary:"
    if not document_id.startswith(expected_prefix):
        raise ProbeFailure(
            f"{storage} grant resolved to unexpected document {document_id!r}; "
            f"expected prefix {expected_prefix!r}"
        )

    target.shell(["am", "force-stop", PACKAGE])
    remove_result(target)
    launch_phase(target, "verify", storage)
    restart = read_result(target, "restart")
    if not restart.get("success"):
        raise ProbeFailure(f"restart probe failed: {json.dumps(restart, indent=2)}")
    if not restart.get("probe_tree_deleted"):
        raise ProbeFailure("restart phase did not delete its SAF probe tree")

    target.shell(["am", "force-stop", PACKAGE], check=False)
    target.shell(["pm", "clear", PACKAGE], check=False)
    remove_grant_folder(target, storage)
    return {
        "target": target_name,
        "storage": storage,
        "run": ordinal,
        "initial": initial,
        "restart": restart,
    }


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--target",
        choices=["avd", "chromeos", "pixel7a", "motox4"],
        required=True,
    )
    parser.add_argument("--avd", default=EXPECTED_AVD)
    parser.add_argument(
        "--storage",
        choices=["internal", "sdcard"],
        default="internal",
    )
    parser.add_argument("--runs", type=int, choices=range(1, 6), default=1)
    parser.add_argument("--no-build", action="store_true")
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    if arguments.storage == "sdcard" and arguments.target != "motox4":
        print(
            "the removable SD-card profile is available only on motox4",
            file=sys.stderr,
        )
        return 1
    apk = (
        probe_root() / "app" / "build" / "outputs" / "apk" / "debug" / "app-debug.apk"
        if arguments.no_build
        else build_probe()
    )
    if not apk.is_file():
        print(f"probe APK is unavailable at {apk}", file=sys.stderr)
        return 1

    avd_session: AvdSession | None = None
    target: AdbTarget | None = None
    results: list[dict] = []
    failure: BaseException | None = None
    try:
        if arguments.target == "avd":
            avd_session = start_avd(arguments.avd)
            target = avd_session.target
        elif arguments.target == "chromeos":
            target = prepare_chromeos()
        elif arguments.target == "pixel7a":
            target = prepare_pixel()
        else:
            target = prepare_moto()
        identity = verify_target(target, arguments.target)
        identity.update(
            verify_storage(
                target,
                arguments.target,
                arguments.storage,
            )
        )
        install_apk(target, arguments.target, apk)
        for ordinal in range(1, arguments.runs + 1):
            try:
                result = run_cycle(
                    target,
                    arguments.target,
                    arguments.storage,
                    ordinal,
                )
            except BaseException:
                cleanup_after_failure(target, arguments.storage)
                raise
            result["identity"] = identity
            results.append(result)
            print(json.dumps(result, sort_keys=True), flush=True)
    except BaseException as error:
        failure = error
    finally:
        if target is not None:
            target.run(["uninstall", PACKAGE], timeout=30, check=False)
        if avd_session is not None:
            avd_session.close()

    if failure is not None:
        print(f"probe failed: {failure}", file=sys.stderr)
        return 1
    summary = {
        "target": arguments.target,
        "storage": arguments.storage,
        "runs": len(results),
        "result": "pass",
        "cleanup": "ok",
    }
    print(json.dumps(summary, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
