"""Owned no-window Android emulator lifecycle for repository UI tests."""

from __future__ import annotations

import os
import signal
import socket
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

from first_verified_piece import ScenarioFailure


EMULATOR_PORT_MIN = 5554
EMULATOR_PORT_MAX = 5682
BOOT_TIMEOUT_SECONDS = 180


def android_sdk_root() -> Path:
    configured = os.environ.get("ANDROID_HOME") or os.environ.get(
        "ANDROID_SDK_ROOT"
    )
    return Path(configured) if configured else Path.home() / "Android" / "Sdk"


def default_adb() -> Path:
    return android_sdk_root() / "platform-tools" / "adb"


def default_emulator() -> Path:
    return android_sdk_root() / "emulator" / "emulator"


def adb_devices(adb: Path) -> dict[str, str]:
    completed = subprocess.run(
        [str(adb), "devices"],
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to list ADB devices\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    devices: dict[str, str] = {}
    for line in completed.stdout.splitlines()[1:]:
        fields = line.split()
        if len(fields) >= 2:
            devices[fields[0]] = fields[1]
    return devices


def running_avd_name(adb: Path, serial: str) -> str | None:
    completed = subprocess.run(
        [str(adb), "-s", serial, "emu", "avd", "name"],
        capture_output=True,
        text=True,
        timeout=10,
        check=False,
    )
    if completed.returncode != 0:
        return None
    names = [
        line.strip()
        for line in completed.stdout.splitlines()
        if line.strip() and line.strip() != "OK"
    ]
    return names[0] if names else None


def available_avds(emulator: Path) -> set[str]:
    completed = subprocess.run(
        [str(emulator), "-list-avds"],
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    if completed.returncode != 0:
        raise ScenarioFailure(
            "failed to list Android virtual devices\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    return {line.strip() for line in completed.stdout.splitlines() if line.strip()}


def port_pair_available(port: int) -> bool:
    sockets: list[socket.socket] = []
    try:
        for candidate in (port, port + 1):
            listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind(("127.0.0.1", candidate))
            sockets.append(listener)
        return True
    except OSError:
        return False
    finally:
        for listener in sockets:
            listener.close()


def select_emulator_port(adb: Path) -> int:
    devices = adb_devices(adb)
    for port in range(EMULATOR_PORT_MIN, EMULATOR_PORT_MAX + 1, 2):
        if f"emulator-{port}" not in devices and port_pair_available(port):
            return port
    raise ScenarioFailure("no free Android emulator console/ADB port pair is available")


def adb_shell(
    adb: Path,
    serial: str,
    *arguments: str,
    timeout: float = 15,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        [str(adb), "-s", serial, "shell", *arguments],
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
    if check and completed.returncode != 0:
        raise ScenarioFailure(
            f"adb {serial} shell {' '.join(arguments)} failed\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    return completed


def terminate_process_group(process: subprocess.Popen[str]) -> None:
    if process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    try:
        process.wait(timeout=15)
    except subprocess.TimeoutExpired:
        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        process.wait(timeout=10)


@dataclass
class OwnedHeadlessAvd:
    name: str
    serial: str
    adb: Path
    process: subprocess.Popen[str]
    log_path: Path

    @classmethod
    def start(
        cls,
        name: str,
        adb: Path,
        emulator: Path,
        work_directory: Path,
    ) -> "OwnedHeadlessAvd":
        if not adb.is_file():
            raise ScenarioFailure(f"ADB is unavailable at {adb}")
        if not emulator.is_file():
            raise ScenarioFailure(f"Android emulator is unavailable at {emulator}")
        if name not in available_avds(emulator):
            raise ScenarioFailure(f"Android virtual device {name!r} is unavailable")

        started = subprocess.run(
            [str(adb), "start-server"],
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
        if started.returncode != 0:
            raise ScenarioFailure(
                "failed to start the ADB server\n"
                f"stdout:\n{started.stdout}\nstderr:\n{started.stderr}"
            )
        for serial, state in adb_devices(adb).items():
            if (
                state == "device"
                and serial.startswith("emulator-")
                and running_avd_name(adb, serial) == name
            ):
                raise ScenarioFailure(
                    f"AVD {name!r} is already running as {serial}; refusing to "
                    "take ownership of an existing emulator"
                )

        port = select_emulator_port(adb)
        serial = f"emulator-{port}"
        log_path = work_directory / "headless-avd.log"
        log_handle = log_path.open("w", encoding="utf-8")
        try:
            process = subprocess.Popen(
                [
                    str(emulator),
                    "-avd",
                    name,
                    "-port",
                    str(port),
                    "-no-window",
                    "-no-audio",
                    "-no-boot-anim",
                    "-no-snapshot",
                    "-read-only",
                    "-gpu",
                    "swiftshader_indirect",
                ],
                cwd=work_directory,
                stdout=log_handle,
                stderr=subprocess.STDOUT,
                text=True,
                start_new_session=True,
            )
        finally:
            log_handle.close()

        owned = cls(name, serial, adb, process, log_path)
        try:
            owned.wait_until_ready()
            return owned
        except BaseException:
            owned.close()
            raise

    def wait_until_ready(self) -> None:
        deadline = time.monotonic() + BOOT_TIMEOUT_SECONDS
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise ScenarioFailure(
                    "headless AVD exited during startup\n" + self.log_tail()
                )
            if adb_devices(self.adb).get(self.serial) == "device":
                booted = adb_shell(
                    self.adb,
                    self.serial,
                    "getprop",
                    "sys.boot_completed",
                    check=False,
                ).stdout.strip()
                if booted == "1":
                    self.prepare_test_device()
                    return
            time.sleep(1)
        raise ScenarioFailure(
            f"headless AVD {self.name!r} did not boot before timeout\n"
            + self.log_tail()
        )

    def prepare_test_device(self) -> None:
        adb_shell(
            self.adb,
            self.serial,
            "input",
            "keyevent",
            "KEYCODE_WAKEUP",
            check=False,
        )
        adb_shell(self.adb, self.serial, "wm", "dismiss-keyguard", check=False)
        for setting in (
            "window_animation_scale",
            "transition_animation_scale",
            "animator_duration_scale",
        ):
            adb_shell(
                self.adb,
                self.serial,
                "settings",
                "put",
                "global",
                setting,
                "0",
            )
        adb_shell(
            self.adb,
            self.serial,
            "cmd",
            "package",
            "list",
            "packages",
            timeout=30,
        )

    def log_tail(self, limit: int = 8_000) -> str:
        try:
            content = self.log_path.read_text(encoding="utf-8", errors="replace")
        except FileNotFoundError:
            return "(emulator log is absent)"
        return content[-limit:]

    def close(self) -> None:
        try:
            subprocess.run(
                [str(self.adb), "-s", self.serial, "emu", "kill"],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
        except subprocess.TimeoutExpired:
            pass
        terminate_process_group(self.process)
