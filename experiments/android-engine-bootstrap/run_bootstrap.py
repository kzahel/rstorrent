#!/usr/bin/env python3
"""Run Tactical 004 profiles against an explicit Android target."""

from __future__ import annotations

import argparse
import base64
import gc
import hashlib
import importlib.util
import json
import os
import re
import shlex
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType
from typing import Any, Sequence
from urllib.parse import quote


PACKAGE = "org.rstorrent.bootstrap"
ACTIVITY = f"{PACKAGE}/.MainActivity"
RECEIVER = f"{PACKAGE}/.CommandReceiver"
ACTION_START = "org.rstorrent.bootstrap.START"
ACTION_CANCEL = "org.rstorrent.bootstrap.CANCEL"
ACTION_OBSERVE = "org.rstorrent.bootstrap.OBSERVE"
ACTION_VERIFY = "org.rstorrent.bootstrap.VERIFY"
EXPECTED_INTERFACE = "rstorrent-android/0.3.0;uniffi/0.31.0"
PAYLOAD_LIMIT = 32 * 1024
CANCELLATION_STORAGE_DELAY_MILLIS = 5_000
RESULT_TIMEOUT_SECONDS = 45
PROFILE_CHOICES = (
    "success",
    "product-dynamic-saf",
    "product-saf-grant-repair",
    "product-https-tracker",
    "product-https-platform-trust",
    "product-mse",
    "product-concurrent-downloads",
    "product-ipv6-policy",
    "slow-storage",
    "cancellation",
    "peer-failure",
    "duplicate-start",
    "activity-recreation",
    "preexisting-artifacts",
)


class BootstrapFailure(RuntimeError):
    pass


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def bootstrap_root() -> Path:
    return Path(__file__).resolve().parent


def load_module(name: str, path: Path) -> ModuleType:
    specification = importlib.util.spec_from_file_location(name, path)
    if specification is None or specification.loader is None:
        raise BootstrapFailure(f"could not load module at {path}")
    module = importlib.util.module_from_spec(specification)
    sys.modules[name] = module
    specification.loader.exec_module(module)
    return module


def ensure_interop_environment() -> None:
    try:
        import libtorrent  # noqa: F401
    except ModuleNotFoundError:
        if os.environ.get("RSTORRENT_BOOTSTRAP_UV") == "1":
            raise BootstrapFailure(
                "libtorrent is unavailable inside the pinned interop environment"
            )
        environment = os.environ.copy()
        environment["RSTORRENT_BOOTSTRAP_UV"] = "1"
        command = [
            "uv",
            "run",
            "--project",
            str(repository_root() / "tests" / "interop"),
            "python",
            str(Path(__file__).resolve()),
            *sys.argv[1:],
        ]
        os.execvpe(command[0], command, environment)


def load_support() -> tuple[ModuleType, ModuleType, ModuleType]:
    interop_root = repository_root() / "tests" / "interop"
    if str(interop_root) not in sys.path:
        sys.path.insert(0, str(interop_root))
    probe = load_module(
        "rstorrent_storage_probe_support",
        repository_root() / "experiments" / "android-storage-probe" / "run_probe.py",
    )
    interop = load_module(
        "rstorrent_interop_support",
        repository_root() / "tests" / "interop" / "first_verified_piece.py",
    )
    tracker = load_module(
        "rstorrent_http_tracker_support",
        interop_root / "http_tracker_application.py",
    )
    return probe, interop, tracker


def build_apk() -> Path:
    completed = subprocess.run(
        [str(bootstrap_root() / "build.sh")],
        cwd=repository_root(),
        capture_output=True,
        text=True,
        timeout=420,
        check=False,
    )
    if completed.returncode != 0:
        raise BootstrapFailure(
            "Android bootstrap build failed\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    lines = completed.stdout.strip().splitlines()
    apk = Path(lines[-1]) if lines else Path()
    if not apk.is_file():
        raise BootstrapFailure("build did not report an APK")
    return apk


@dataclass
class SeedFixture:
    run_path: Path
    torrent_path: Path
    expected_file_hashes: dict[str, str]
    piece_hashes: list[str]
    info_hash: str
    name: str
    session: Any
    handle: Any
    host_port: int
    alerts: list[str]

    @classmethod
    def create(
        cls,
        interop: ModuleType,
        label: str,
        *,
        force_rc4: bool = False,
        root_name: str | None = None,
        content_offset: int = 0,
    ) -> "SeedFixture":
        run_path = Path(tempfile.mkdtemp(prefix=f"rstorrent-android-{label}-"))
        alerts: list[str] = []
        (
            torrent_path,
            seed_directory,
            torrent_info,
            expected_file_hashes,
            piece_hashes,
        ) = interop.create_selective_fixture(
            run_path,
            root_name or interop.SELECTIVE_ROOT_NAME,
            content_offset,
        )
        session = interop.create_session()
        if force_rc4:
            import libtorrent as lt

            session.apply_settings(
                {
                    "in_enc_policy": int(lt.enc_policy.pe_forced),
                    "out_enc_policy": int(lt.enc_policy.pe_forced),
                    "allowed_enc_level": int(lt.enc_level.pe_rc4),
                    "prefer_rc4": True,
                }
            )
        host_port = interop.wait_for_listener(session, alerts)
        handle = interop.add_seed(
            session,
            torrent_info,
            seed_directory,
            alerts,
        )
        return cls(
            run_path=run_path,
            torrent_path=torrent_path,
            expected_file_hashes=expected_file_hashes,
            piece_hashes=piece_hashes,
            info_hash=str(torrent_info.info_hashes().v1),
            name=str(torrent_info.name()),
            session=session,
            handle=handle,
            host_port=host_port,
            alerts=alerts,
        )

    def close(self) -> None:
        try:
            self.alerts.extend(
                alert.message() for alert in self.session.pop_alerts()
            )
        except Exception:
            pass
        try:
            if self.handle.is_valid():
                self.session.remove_torrent(self.handle)
        except Exception:
            pass
        try:
            self.session.pause()
        except Exception:
            pass
        self.handle = None
        self.session = None
        gc.collect()
        shutil.rmtree(self.run_path)


class RequestClosingProxy:
    def __init__(self, upstream_port: int) -> None:
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(1)
        self.port = self.listener.getsockname()[1]
        self.upstream_port = upstream_port
        self.saw_request = threading.Event()
        self.finished = threading.Event()
        self.failure: BaseException | None = None
        self.thread = threading.Thread(
            target=self._run,
            name="rstorrent-request-closing-proxy",
            daemon=True,
        )
        self.thread.start()

    def _run(self) -> None:
        client: socket.socket | None = None
        upstream: socket.socket | None = None
        try:
            self.listener.settimeout(90)
            client, _ = self.listener.accept()
            upstream = socket.create_connection(
                ("127.0.0.1", self.upstream_port),
                timeout=10,
            )
            stop = threading.Event()
            server_thread = threading.Thread(
                target=self._relay,
                args=(upstream, client, stop, False),
                daemon=True,
            )
            server_thread.start()
            self._relay(client, upstream, stop, True)
            stop.set()
            server_thread.join(timeout=2)
        except BaseException as error:
            self.failure = error
        finally:
            for stream in (client, upstream):
                if stream is not None:
                    try:
                        stream.shutdown(socket.SHUT_RDWR)
                    except OSError:
                        pass
                    stream.close()
            self.finished.set()

    def _relay(
        self,
        source: socket.socket,
        destination: socket.socket,
        stop: threading.Event,
        inspect_requests: bool,
    ) -> None:
        buffered = bytearray()
        handshake_remaining = 68
        source.settimeout(1)
        while not stop.is_set():
            try:
                chunk = source.recv(64 * 1024)
            except TimeoutError:
                continue
            if not chunk:
                return
            destination.sendall(chunk)
            if not inspect_requests:
                continue
            buffered.extend(chunk)
            if handshake_remaining:
                consumed = min(handshake_remaining, len(buffered))
                del buffered[:consumed]
                handshake_remaining -= consumed
                if handshake_remaining:
                    continue
            while len(buffered) >= 4:
                frame_length = struct.unpack(">I", buffered[:4])[0]
                if len(buffered) < 4 + frame_length:
                    break
                frame = bytes(buffered[4 : 4 + frame_length])
                del buffered[: 4 + frame_length]
                if frame and frame[0] == 6:
                    self.saw_request.set()
                    stop.set()
                    return

    def close(self) -> None:
        try:
            self.listener.close()
        except OSError:
            pass
        self.thread.join(timeout=5)
        if self.thread.is_alive():
            return
        if self.failure is not None and not self.saw_request.is_set():
            raise BootstrapFailure(f"peer-failure proxy failed: {self.failure}")


@dataclass
class ReverseTransport:
    target: Any
    device_port: int
    chrome_tunnel: subprocess.Popen[str] | None = None

    @classmethod
    def create(
        cls,
        target: Any,
        target_kind: str,
        host_port: int,
        ordinal: int,
        slot: int = 0,
    ) -> "ReverseTransport":
        if not 0 <= slot <= 7:
            raise BootstrapFailure("reverse transport slot must be between zero and seven")
        device_port = 39_000 + (ordinal % 500) * 8 + slot
        chrome_tunnel: subprocess.Popen[str] | None = None
        if target_kind == "chromeos":
            chrome_tunnel = subprocess.Popen(
                [
                    "ssh",
                    "-N",
                    "-o",
                    "ExitOnForwardFailure=yes",
                    "-R",
                    f"127.0.0.1:{device_port}:127.0.0.1:{host_port}",
                    "chromeroot",
                ],
                cwd=repository_root(),
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
                text=True,
            )
            time.sleep(0.4)
            if chrome_tunnel.poll() is not None:
                detail = chrome_tunnel.stderr.read() if chrome_tunnel.stderr else ""
                raise BootstrapFailure(
                    f"ChromeOS reverse SSH tunnel failed: {detail}"
                )
            reverse_host_port = device_port
        else:
            reverse_host_port = host_port
        result = target.run(
            [
                "reverse",
                f"tcp:{device_port}",
                f"tcp:{reverse_host_port}",
            ],
            timeout=20,
            check=False,
        )
        if result.returncode != 0:
            if chrome_tunnel is not None:
                chrome_tunnel.terminate()
                chrome_tunnel.wait(timeout=5)
            raise BootstrapFailure(
                "adb reverse failed\n"
                f"stdout:\n{result.stdout}\n"
                f"stderr:\n{result.stderr}"
            )
        return cls(target, device_port, chrome_tunnel)

    def close(self) -> None:
        self.target.run(
            ["reverse", "--remove", f"tcp:{self.device_port}"],
            timeout=15,
            check=False,
        )
        if self.chrome_tunnel is not None:
            self.chrome_tunnel.terminate()
            try:
                self.chrome_tunnel.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.chrome_tunnel.kill()
                self.chrome_tunnel.wait(timeout=5)


def clear_application(target: Any) -> None:
    target.shell(["am", "force-stop", PACKAGE], check=False)
    cleared = target.shell(["pm", "clear", PACKAGE], check=False)
    if cleared.returncode != 0 or "Success" not in cleared.stdout:
        raise BootstrapFailure(
            f"could not clear bootstrap app data: {cleared.stdout}"
        )
    target.run(["logcat", "-c"], check=False)


def launch(
    target: Any,
    action: str,
    *,
    run_id: str,
    fixture: SeedFixture | None = None,
    peer_port: int | None = None,
    scenario: str | None = None,
    storage_delay_millis: int = 0,
    collision: str = "",
    storage: str = "private",
    tree_initial_uri: str | None = None,
    via_receiver: bool = False,
) -> None:
    arguments = ["am", "broadcast" if via_receiver else "start"]
    arguments.extend(
        [
            "-a",
            action,
            "-n",
            RECEIVER if via_receiver else ACTIVITY,
        ]
    )
    if not via_receiver:
        arguments.extend(
            [
                "--ez",
                "finish_activity",
                "true",
            ]
        )
    arguments.extend(
        [
        "--es",
        "run_id",
        run_id,
        ]
    )
    if action == ACTION_START:
        if fixture is None or peer_port is None:
            raise BootstrapFailure("start action requires fixture and peer port")
        metainfo = base64.b64encode(fixture.torrent_path.read_bytes()).decode("ascii")
        arguments.extend(
            [
                "--es",
                "scenario",
                scenario or "success",
                "--es",
                "metainfo_base64",
                metainfo,
                "--ei",
                "peer_port",
                str(peer_port),
                "--el",
                "timeout_seconds",
                "45",
                "--el",
                "max_buffered_payload_bytes",
                str(PAYLOAD_LIMIT),
                "--el",
                "storage_write_delay_millis",
                str(storage_delay_millis),
                "--es",
                "skip_files",
                "1,2",
                "--es",
                "materialize_files",
                "2",
                "--es",
                "storage",
                storage,
            ]
        )
        if tree_initial_uri:
            arguments.extend(["--es", "tree_initial_uri", tree_initial_uri])
        if collision:
            arguments.extend(["--es", "collision", collision])
    completed = target.shell(arguments, timeout=30, check=False)
    if completed.returncode != 0 or "Error:" in completed.stdout:
        raise BootstrapFailure(
            f"activity launch failed for {action}\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )


def app_text(target: Any, relative_path: str) -> str | None:
    result = target.shell(
        ["run-as", PACKAGE, "cat", relative_path],
        timeout=10,
        check=False,
    )
    if result.returncode != 0:
        return None
    return result.stdout


def app_bytes(target: Any, relative_path: str) -> bytes | None:
    completed = subprocess.run(
        [
            *target.prefix,
            "exec-out",
            "run-as",
            PACKAGE,
            "cat",
            relative_path,
        ],
        cwd=repository_root(),
        capture_output=True,
        timeout=20,
        check=False,
    )
    if completed.returncode != 0:
        return None
    return completed.stdout


def app_exists(target: Any, relative_path: str) -> bool:
    result = target.shell(
        ["run-as", PACKAGE, "ls", "-d", relative_path],
        timeout=10,
        check=False,
    )
    return result.returncode == 0


def wait_result(
    target: Any,
    run_id: str,
    *,
    restart: bool = False,
) -> dict[str, Any]:
    suffix = "-restart" if restart else ""
    relative = f"files/results/{run_id}{suffix}.json"
    deadline = time.monotonic() + RESULT_TIMEOUT_SECONDS
    last = ""
    while time.monotonic() < deadline:
        text = app_text(target, relative)
        if text:
            last = text.strip()
            try:
                return json.loads(last)
            except json.JSONDecodeError:
                pass
        time.sleep(0.2)
    logcat = target.run(
        [
            "logcat",
            "-d",
            "-s",
            "RSTorrentBootstrap:*",
            "AndroidRuntime:E",
            "*:S",
        ],
        timeout=20,
        check=False,
    ).stdout
    raise BootstrapFailure(
        f"timed out waiting for {relative}; last={last!r}\n"
        f"logcat tail:\n{logcat}"
    )


def read_events(target: Any, run_id: str) -> list[dict[str, Any]]:
    text = app_text(target, f"files/sessions/{run_id}/events.jsonl")
    if not text:
        return []
    events = []
    for line in text.splitlines():
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError as error:
            raise BootstrapFailure(f"invalid event JSON: {line!r}") from error
    return events


def wait_for_event(
    target: Any,
    run_id: str,
    predicate: Any,
    description: str,
    timeout_seconds: float = 15,
    use_activity: bool = False,
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        launch(
            target,
            ACTION_OBSERVE,
            run_id=run_id,
            via_receiver=not use_activity,
        )
        for event in read_events(target, run_id):
            if predicate(event):
                return event
        time.sleep(0.15)
    logcat = target.run(
        ["logcat", "-d", "-t", "250"],
        timeout=20,
        check=False,
    ).stdout
    raise BootstrapFailure(
        f"timed out waiting for event: {description}; "
        f"events={json.dumps(read_events(target, run_id), sort_keys=True)}\n"
        f"logcat tail:\n{logcat}"
    )


def wait_recorded_event(
    target: Any,
    run_id: str,
    predicate: Any,
    description: str,
    timeout_seconds: float = 10,
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        for event in read_events(target, run_id):
            if predicate(event):
                return event
        time.sleep(0.1)
    raise BootstrapFailure(f"timed out waiting for recorded event: {description}")


def validate_common(result: dict[str, Any], identity: dict[str, str]) -> None:
    if result.get("interface_version") != EXPECTED_INTERFACE:
        raise BootstrapFailure("result reported the wrong native interface")
    if not result.get("joined"):
        raise BootstrapFailure("engine task was not joined")
    snapshot = result.get("snapshot", {})
    if snapshot.get("task_alive"):
        raise BootstrapFailure("terminal result still reports a live engine task")
    if snapshot.get("buffered_payload_bytes") != 0:
        raise BootstrapFailure("terminal payload reservation is nonzero")
    if snapshot.get("payload_high_water", 0) > PAYLOAD_LIMIT:
        raise BootstrapFailure("payload high water exceeded the configured limit")
    requested = snapshot.get("requested_bytes", -1)
    received = snapshot.get("received_bytes", -1)
    stored = snapshot.get("stored_bytes", -1)
    if not requested >= received >= stored >= 0:
        raise BootstrapFailure(
            "byte counters are not ordered as requested >= received >= stored"
        )
    device = result.get("device", {})
    if str(device.get("api")) != identity["api"]:
        raise BootstrapFailure("application and ADB API identities differ")
    if device.get("model") != identity["model"]:
        raise BootstrapFailure("application and ADB model identities differ")
    if device.get("fingerprint") != identity["fingerprint"]:
        raise BootstrapFailure("application and ADB fingerprints differ")


def sha1_bytes(payload: bytes) -> str:
    return hashlib.sha1(payload).hexdigest()


def validate_success(
    target: Any,
    result: dict[str, Any],
    fixture: SeedFixture,
    run_id: str,
    identity: dict[str, str],
) -> None:
    validate_common(result, identity)
    terminal = result.get("terminal", {})
    if terminal.get("outcome") != "SUCCEEDED":
        raise BootstrapFailure(
            "successful profile ended unexpectedly: "
            f"{json.dumps(terminal, sort_keys=True)}; "
            f"events={json.dumps(read_events(target, run_id), sort_keys=True)}"
        )
    report = terminal.get("report", {})
    expected = {
        "info_hash": fixture.info_hash,
        "final_piece_hash": fixture.piece_hashes[-1],
        "bytes_written": 97_232,
        "block_count": 7,
        "payload_limit": PAYLOAD_LIMIT,
        "verification_buffer": 16 * 1024,
        "piece_count": 5,
        "verified_piece_count": 4,
        "skipped_piece_count": 1,
        "selected_file_bytes": 73_000,
        "skipped_file_bytes": 57_000,
        "padding_bytes": 3_304,
        "selected_written_bytes": 73_000,
        "part_written_bytes": 24_232,
        "materialized_bytes": 7_000,
        "part_slots_before": 2,
        "part_slots_after": 2,
        "part_reopened": True,
    }
    for key, value in expected.items():
        if report.get(key) != value:
            raise BootstrapFailure(
                f"report {key}={report.get(key)!r}, expected {value!r}"
            )
    if not 0 < report.get("payload_high_water", 0) <= PAYLOAD_LIMIT:
        raise BootstrapFailure("successful payload high water is invalid")
    snapshot = result["snapshot"]
    for counter in ("requested_bytes", "received_bytes", "stored_bytes"):
        if snapshot.get(counter) != expected["bytes_written"]:
            raise BootstrapFailure(
                f"successful {counter}={snapshot.get(counter)!r}, "
                f"expected {expected['bytes_written']}"
            )

    root = f"files/sessions/{run_id}"
    for relative_path, _, padding in fixture_files():
        output_path = f"{root}/downloaded/{relative_path}"
        if padding or relative_path == "skip/large.bin":
            if app_exists(target, output_path):
                raise BootstrapFailure(
                    f"skipped or padding path was published: {relative_path}"
                )
            continue
        payload = app_bytes(target, output_path)
        if payload is None:
            raise BootstrapFailure(f"wanted output is absent: {relative_path}")
        if sha1_bytes(payload) != fixture.expected_file_hashes[relative_path]:
            raise BootstrapFailure(f"wanted output hash differs: {relative_path}")
    if app_exists(target, f"{root}/.downloaded.rstorrent-staging"):
        raise BootstrapFailure("published staging root survived")
    if not app_exists(target, f"{root}/.downloaded.rstorrent-parts"):
        raise BootstrapFailure("validated part file is absent")


def validate_saf_prepared(
    target: Any,
    result: dict[str, Any],
    fixture: SeedFixture,
    run_id: str,
    identity: dict[str, str],
) -> None:
    validate_common(result, identity)
    terminal = result.get("terminal", {})
    if terminal.get("outcome") != "PREPARED":
        raise BootstrapFailure(
            "SAF native execution was not prepared: "
            f"{json.dumps(terminal, sort_keys=True)}; "
            f"events={json.dumps(read_events(target, run_id), sort_keys=True)}"
        )
    if result.get("platform", {}).get("status") != "AWAITING_RESTART":
        raise BootstrapFailure(
            f"SAF provider publication failed: {result.get('platform')}"
        )
    report = terminal.get("report", {})
    expected_scalars = {
        "info_hash": fixture.info_hash,
        "final_piece_hash": fixture.piece_hashes[-1],
        "bytes_written": 97_232,
        "block_count": 7,
        "payload_limit": PAYLOAD_LIMIT,
        "verification_buffer": 16 * 1024,
        "piece_count": 5,
        "verified_piece_count": 4,
        "skipped_piece_count": 1,
        "selected_file_bytes": 73_000,
        "skipped_file_bytes": 57_000,
        "padding_bytes": 3_304,
        "selected_written_bytes": 73_000,
        "part_written_bytes": 24_232,
        "materialized_bytes": 7_000,
        "part_slots_before": 2,
        "part_slots_after": 2,
        "part_reopened": True,
        "part_path": None,
    }
    for key, expected in expected_scalars.items():
        if report.get(key) != expected:
            raise BootstrapFailure(
                f"SAF report {key}={report.get(key)!r}, expected {expected!r}"
            )
    expected_hashes = {
        index: fixture.expected_file_hashes[path]
        for index, (path, _, padding) in enumerate(fixture_files())
        if not padding and index != 1
    }
    prepared = {
        entry["file_index"]: entry["sha1"]
        for entry in report.get("prepared_files", [])
    }
    if prepared != expected_hashes:
        raise BootstrapFailure(
            f"prepared hash manifest differs: {prepared!r} != {expected_hashes!r}"
        )


def validate_saf_restart(
    result: dict[str, Any],
    fixture: SeedFixture,
    identity: dict[str, str],
) -> None:
    if result.get("interface_version") != EXPECTED_INTERFACE:
        raise BootstrapFailure("restart reported the wrong native interface")
    if result.get("status") != "SUCCEEDED":
        raise BootstrapFailure(
            f"SAF restart verification failed: {json.dumps(result, sort_keys=True)}"
        )
    device = result.get("device", {})
    if (
        str(device.get("api")) != identity["api"]
        or device.get("model") != identity["model"]
        or device.get("fingerprint") != identity["fingerprint"]
    ):
        raise BootstrapFailure("restart identity differs from the selected target")
    expected_hashes = {
        index: fixture.expected_file_hashes[path]
        for index, (path, _, padding) in enumerate(fixture_files())
        if not padding and index != 1
    }
    verified = {
        entry["file_index"]: entry["sha1"]
        for entry in result.get("verified_files", [])
    }
    if verified != expected_hashes:
        raise BootstrapFailure(
            f"restart hash manifest differs: {verified!r} != {expected_hashes!r}"
        )
    if not result.get("published_deleted") or not result.get("part_deleted"):
        raise BootstrapFailure("restart did not clean exact SAF artifacts")


def fixture_files() -> Sequence[tuple[str, int, bool]]:
    return (
        ("wanted/start.bin", 20_000, False),
        ("skip/large.bin", 50_000, False),
        ("later.bin", 7_000, False),
        ("wanted/end.bin", 18_000, False),
        ("wanted/empty.bin", 0, False),
        (".pad/3304", 3_304, True),
        ("tail.bin", 35_000, False),
    )


def cleanup_run(target: Any, run_id: str) -> None:
    if not run_id or any(
        character not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-"
        for character in run_id
    ):
        raise BootstrapFailure(f"refusing unsafe cleanup run ID {run_id!r}")
    for path in (
        f"files/sessions/{run_id}",
        f"files/results/{run_id}.json",
        f"files/results/{run_id}-restart.json",
        f"files/results/.{run_id}.json.tmp",
        f"files/results/.{run_id}-restart.json.tmp",
    ):
        target.shell(
            ["run-as", PACKAGE, "rm", "-rf", path],
            timeout=15,
            check=False,
        )
        if app_exists(target, path):
            raise BootstrapFailure(f"app-private cleanup failed for {path}")


def peer_count(fixture: SeedFixture) -> int:
    try:
        return int(fixture.handle.status().num_peers)
    except Exception:
        return -1


def run_standard_profile(
    target: Any,
    target_kind: str,
    identity: dict[str, str],
    probe: ModuleType,
    interop: ModuleType,
    profile: str,
    ordinal: int,
    storage: str,
) -> dict[str, Any]:
    label = f"{target_kind}-{profile}-{ordinal}"
    run_id = f"{profile.replace('-', '_')}-{ordinal}"
    fixture = SeedFixture.create(interop, label)
    proxy: RequestClosingProxy | None = None
    transport: ReverseTransport | None = None
    saf_storage = storage.startswith("saf-")
    grant_storage = "sdcard" if storage == "saf-sdcard" else "internal"
    try:
        clear_application(target)
        if saf_storage:
            probe.prepare_grant_folder(target, grant_storage)
        host_port = fixture.host_port
        if profile == "peer-failure":
            proxy = RequestClosingProxy(host_port)
            host_port = proxy.port
        transport = ReverseTransport.create(
            target,
            target_kind,
            host_port,
            ordinal,
        )
        delay = {
            "slow-storage": 2_000,
            "duplicate-start": 750,
            "activity-recreation": 2_000,
        }.get(profile, 0)
        launch(
            target,
            ACTION_START,
            run_id=run_id,
            fixture=fixture,
            peer_port=transport.device_port,
            scenario=profile,
            storage_delay_millis=delay,
            storage=storage,
            tree_initial_uri=(
                probe.MOTO_SD_INITIAL_URI
                if storage == "saf-sdcard"
                else None
            ),
        )
        if saf_storage:
            probe.automate_tree_grant(target, grant_storage)

        observed: dict[str, Any] | None = None
        if profile == "slow-storage":
            observed = wait_for_event(
                target,
                run_id,
                lambda event: (
                    event.get("event") == "activity_observed"
                    and event.get("requested_bytes", 0)
                    >= event.get("received_bytes", 0)
                    > event.get("stored_bytes", 0)
                    and event.get("buffered_payload_bytes", 0) > 0
                    and event.get("task_alive") is True
                ),
                "received payload waiting on slow storage",
            )
        elif profile == "duplicate-start":
            wait_recorded_event(
                target,
                run_id,
                lambda event: event.get("event") == "engine_start",
                "engine start before duplicate command",
            )
            launch(
                target,
                ACTION_START,
                run_id=run_id,
                fixture=fixture,
                peer_port=transport.device_port,
                scenario=profile,
                storage_delay_millis=delay,
                storage=storage,
                via_receiver=True,
            )
        elif profile == "activity-recreation":
            wait_recorded_event(
                target,
                run_id,
                lambda event: event.get("event") == "engine_start",
                "engine start before activity recreation",
            )
            observed = wait_for_event(
                target,
                run_id,
                lambda event: (
                    event.get("event") == "activity_observed"
                    and event.get("task_alive") is True
                ),
                "activity recreation while engine is active",
                use_activity=True,
            )

        result = wait_result(target, run_id)
        restart_result: dict[str, Any] | None = None
        if profile == "peer-failure":
            validate_common(result, identity)
            terminal = result.get("terminal", {})
            if terminal.get("outcome") != "FAILED":
                raise BootstrapFailure("peer-failure profile did not fail")
            if terminal.get("failure_kind") != "PEER":
                raise BootstrapFailure(
                    "peer-failure profile was not typed as PEER"
                )
            snapshot = result["snapshot"]
            if snapshot["requested_bytes"] <= 0:
                raise BootstrapFailure("peer failed before request accounting")
            if proxy is None or not proxy.saw_request.wait(timeout=2):
                raise BootstrapFailure("peer failed before a request was observed")
            if saf_storage:
                if result.get("platform", {}).get("status") != "NOT_PREPARED":
                    raise BootstrapFailure("failed SAF transfer was publishable")
            else:
                assert_unverified_cleanup(target, run_id)
        else:
            if saf_storage:
                validate_saf_prepared(target, result, fixture, run_id, identity)
                target.shell(["am", "force-stop", PACKAGE])
                launch(target, ACTION_VERIFY, run_id=run_id)
                restart_result = wait_result(target, run_id, restart=True)
                validate_saf_restart(restart_result, fixture, identity)
            else:
                validate_success(target, result, fixture, run_id, identity)
        events = read_events(target, run_id)
        if profile == "duplicate-start" and not any(
            event.get("event") == "duplicate_start"
            and event.get("disposition") == "BUSY"
            for event in events
        ):
            raise BootstrapFailure("duplicate start was not rejected as BUSY")
        if profile == "activity-recreation" and observed is None:
            raise BootstrapFailure("activity recreation was not observed")
        if profile == "slow-storage":
            if observed is None:
                raise BootstrapFailure("slow storage was not observed in flight")
            if result["snapshot"]["payload_high_water"] > PAYLOAD_LIMIT:
                raise BootstrapFailure("slow storage exceeded its payload limit")

        output = {
            "target": target_kind,
            "profile": profile,
            "run": ordinal,
            "result": result,
            "restart": restart_result,
            "events": events,
            "host_peer_count_after": peer_count(fixture),
        }
        cleanup_run(target, run_id)
        return output
    finally:
        if transport is not None:
            transport.close()
        if proxy is not None:
            proxy.close()
        if saf_storage:
            probe.remove_grant_folder(target, grant_storage)
        fixture.close()


def assert_unverified_cleanup(target: Any, run_id: str) -> None:
    root = f"files/sessions/{run_id}"
    for path in (
        f"{root}/downloaded",
        f"{root}/.downloaded.rstorrent-staging",
        f"{root}/.downloaded.rstorrent-parts",
    ):
        if app_exists(target, path):
            raise BootstrapFailure(
                f"unverified artifact survived terminal cleanup: {path}"
            )


def run_cancellation_profile(
    target: Any,
    target_kind: str,
    identity: dict[str, str],
    probe: ModuleType,
    interop: ModuleType,
    ordinal: int,
    storage: str,
) -> dict[str, Any]:
    results = []
    saf_storage = storage.startswith("saf-")
    grant_storage = "sdcard" if storage == "saf-sdcard" else "internal"
    for phase, after_progress in (("before", False), ("after", True)):
        run_id = f"cancellation_{phase}-{ordinal}"
        fixture = SeedFixture.create(
            interop,
            f"{target_kind}-cancellation-{phase}-{ordinal}",
        )
        transport: ReverseTransport | None = None
        try:
            clear_application(target)
            if saf_storage:
                probe.prepare_grant_folder(target, grant_storage)
            transport = ReverseTransport.create(
                target,
                target_kind,
                fixture.host_port,
                ordinal + (100 if after_progress else 0),
            )
            launch(
                target,
                ACTION_START,
                run_id=run_id,
                fixture=fixture,
                peer_port=transport.device_port,
                scenario=f"cancellation-{phase}",
                storage_delay_millis=CANCELLATION_STORAGE_DELAY_MILLIS,
                storage=storage,
                tree_initial_uri=(
                    probe.MOTO_SD_INITIAL_URI
                    if storage == "saf-sdcard"
                    else None
                ),
            )
            if saf_storage:
                probe.automate_tree_grant(target, grant_storage)
            wait_recorded_event(
                target,
                run_id,
                lambda event: event.get("event") == "engine_start",
                f"engine start before {phase} cancellation",
                timeout_seconds=30,
            )
            if after_progress:
                wait_for_event(
                    target,
                    run_id,
                    lambda event: (
                        event.get("event") == "activity_observed"
                        and event.get("stored_bytes", 0) >= 16_384
                        and event.get("task_alive") is True
                    ),
                    "accepted block before cancellation",
                )
            launch(
                target,
                ACTION_CANCEL,
                run_id=run_id,
                via_receiver=True,
            )
            result = wait_result(target, run_id)
            validate_common(result, identity)
            terminal = result.get("terminal", {})
            if terminal.get("outcome") != "CANCELLED":
                raise BootstrapFailure(
                    f"{phase} cancellation ended as {terminal.get('outcome')}"
                )
            stored = result["snapshot"]["stored_bytes"]
            if after_progress and stored < 16_384:
                raise BootstrapFailure(
                    "after-progress cancellation stored no complete block"
                )
            if not after_progress and stored != 0:
                raise BootstrapFailure(
                    "before-progress cancellation accepted payload"
                )
            if saf_storage:
                if result.get("platform", {}).get("status") != "NOT_PREPARED":
                    raise BootstrapFailure("cancelled SAF transfer was publishable")
            else:
                assert_unverified_cleanup(target, run_id)

            original = app_bytes(
                target,
                f"files/results/{run_id}.json",
            )
            launch(
                target,
                ACTION_CANCEL,
                run_id=run_id,
                via_receiver=True,
            )
            time.sleep(0.5)
            repeated = app_bytes(
                target,
                f"files/results/{run_id}.json",
            )
            if original != repeated:
                raise BootstrapFailure(
                    "repeated terminal cancellation changed the result"
                )
            results.append(
                {
                    "phase": phase,
                    "result": result,
                    "events": read_events(target, run_id),
                }
            )
            cleanup_run(target, run_id)
        finally:
            if transport is not None:
                transport.close()
            if saf_storage:
                probe.remove_grant_folder(target, grant_storage)
            fixture.close()
    return {
        "target": target_kind,
        "profile": "cancellation",
        "run": ordinal,
        "phases": results,
    }


def collision_bytes(
    target: Any,
    run_id: str,
    collision: str,
) -> bytes | None:
    root = f"files/sessions/{run_id}"
    paths = {
        "output": f"{root}/downloaded",
        "staging": f"{root}/.downloaded.rstorrent-staging/sentinel",
        "part": f"{root}/.downloaded.rstorrent-parts",
        "result": f"files/results/{run_id}.json",
    }
    return app_bytes(target, paths[collision])


def run_preexisting_profile(
    target: Any,
    target_kind: str,
    identity: dict[str, str],
    interop: ModuleType,
    ordinal: int,
) -> dict[str, Any]:
    collisions = []
    for collision_index, collision in enumerate(
        ("output", "staging", "part", "result"),
        start=1,
    ):
        run_id = f"preexisting_{collision}-{ordinal}"
        fixture = SeedFixture.create(
            interop,
            f"{target_kind}-{run_id}",
        )
        transport: ReverseTransport | None = None
        try:
            clear_application(target)
            transport = ReverseTransport.create(
                target,
                target_kind,
                fixture.host_port,
                ordinal + 200 + collision_index,
            )
            launch(
                target,
                ACTION_START,
                run_id=run_id,
                fixture=fixture,
                peer_port=transport.device_port,
                scenario="preexisting-artifacts",
                collision=collision,
            )
            expected = f"RSTORRENT_SENTINEL:{collision}".encode()
            if collision == "result":
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    if collision_bytes(target, run_id, collision) == expected:
                        break
                    time.sleep(0.2)
                else:
                    raise BootstrapFailure(
                        "preexisting result sentinel was not preserved"
                    )
                result: dict[str, Any] | None = None
            else:
                result = wait_result(target, run_id)
                validate_common(result, identity)
                terminal = result.get("terminal", {})
                if terminal.get("outcome") != "FAILED":
                    raise BootstrapFailure(
                        f"{collision} collision did not fail"
                    )
                if terminal.get("failure_kind") != "PREEXISTING_ARTIFACT":
                    raise BootstrapFailure(
                        f"{collision} collision had kind "
                        f"{terminal.get('failure_kind')}"
                    )
            if collision_bytes(target, run_id, collision) != expected:
                raise BootstrapFailure(
                    f"{collision} sentinel changed during refusal"
                )
            time.sleep(0.3)
            if peer_count(fixture) not in (0, -1):
                raise BootstrapFailure(
                    f"{collision} refusal connected to the peer"
                )
            collisions.append(
                {
                    "collision": collision,
                    "result": result,
                    "sentinel_preserved": True,
                    "peer_connections": peer_count(fixture),
                }
            )
            cleanup_run(target, run_id)
        finally:
            if transport is not None:
                transport.close()
            fixture.close()
    return {
        "target": target_kind,
        "profile": "preexisting-artifacts",
        "run": ordinal,
        "collisions": collisions,
    }


def product_fd_count(target: Any) -> int:
    pid_text = target.shell(["pidof", PACKAGE], check=False).stdout.strip()
    if not pid_text:
        processes = target.shell(["ps", "-A", "-o", "PID,NAME"], check=False)
        for line in processes.stdout.splitlines():
            fields = line.split()
            if len(fields) >= 2 and fields[-1] == PACKAGE:
                pid_text = fields[0]
                break
    if not pid_text:
        return 0
    pid = pid_text.split()[0]
    listing = target.shell(
        ["run-as", PACKAGE, "ls", f"/proc/{pid}/fd"],
        check=False,
    )
    if listing.returncode != 0:
        return 0
    return len(listing.stdout.split())


def product_logs(target: Any) -> str:
    return target.run(
        ["logcat", "-d", "-v", "brief", "RSTorrentProduct:I", "*:S"],
        timeout=15,
        check=False,
    ).stdout


def launch_product_ipv6_policy(target: Any, mode: str) -> None:
    result = target.shell(
        [
            "am",
            "start",
            "-S",
            "-W",
            "-n",
            ACTIVITY,
            "--es",
            "product_ipv6_policy",
            mode,
        ],
        timeout=30,
        check=False,
    )
    if "Error:" in result.stdout or "Error:" in result.stderr or (
        result.returncode != 0 and "Starting:" not in result.stdout
    ):
        raise BootstrapFailure(
            f"could not exercise Android IPv6 policy mode {mode}: "
            f"code={result.returncode} stdout={result.stdout!r} stderr={result.stderr!r}"
        )


def wait_product_ipv6_policy(target: Any, mode: str) -> dict[str, str]:
    marker = f"ipv6_settings mode={mode} "
    deadline = time.monotonic() + 20
    logs = ""
    while time.monotonic() < deadline:
        logs = product_logs(target)
        rows = [line for line in logs.splitlines() if marker in line]
        if rows:
            fields = dict(re.findall(r"([a-z_]+)=([^ ]+)", rows[-1]))
            if fields.get("application") == "APPLYING":
                time.sleep(0.2)
                continue
            return fields
        time.sleep(0.2)
    raise BootstrapFailure(f"timed out waiting for Android IPv6 policy {mode}\n{logs}")


def run_product_ipv6_policy_profile(
    target: Any,
    target_kind: str,
    identity: dict[str, str],
    ordinal: int,
) -> dict[str, Any]:
    target.run(["logcat", "-c"], timeout=15, check=False)
    target.shell(
        ["pm", "grant", PACKAGE, "android.permission.POST_NOTIFICATIONS"],
        check=False,
    )
    launch_product_ipv6_policy(target, "disable_sequence")
    initial = wait_product_ipv6_policy(target, "initial")
    if initial.get("configured") != "true":
        raise BootstrapFailure("fresh Android profile did not default IPv6 to enabled")

    disabled = wait_product_ipv6_policy(target, "disabled")
    if not (
        disabled.get("configured") == "false"
        and disabled.get("effective") == "false"
        and disabled.get("application") == "APPLIED"
        and disabled.get("tcp") == "none"
        and disabled.get("udp") == "none"
    ):
        raise BootstrapFailure(f"Android IPv6 disable did not converge: {disabled}")

    target.shell(["am", "force-stop", PACKAGE], check=False)
    time.sleep(1)
    launch_product_ipv6_policy(target, "enable_sequence")
    restarted = wait_product_ipv6_policy(target, "restarted")
    if not (
        restarted.get("configured") == "false"
        and restarted.get("effective") == "false"
        and restarted.get("application") == "APPLIED"
    ):
        raise BootstrapFailure(f"Android IPv6 setting did not survive restart: {restarted}")

    enabled = wait_product_ipv6_policy(target, "reenabled")
    if enabled.get("configured") != "true" or enabled.get("application") not in (
        "APPLIED",
        "DEGRADED",
    ):
        raise BootstrapFailure(f"Android IPv6 re-enable did not terminate: {enabled}")
    if enabled.get("effective") == "false" and not (
        enabled.get("application") == "DEGRADED"
        and enabled.get("tcp") == "none"
        and enabled.get("udp") == "none"
    ):
        raise BootstrapFailure(
            f"Android absent IPv6 address did not degrade to IPv4-only: {enabled}"
        )

    target.shell(["am", "force-stop", PACKAGE], check=False)
    return {
        "target": target_kind,
        "profile": "product-ipv6-policy",
        "run": ordinal,
        "identity": identity,
        "fresh_default_enabled": True,
        "disabled_applied": True,
        "disabled_survived_restart": True,
        "reenabled_effective": enabled.get("effective") == "true",
        "reenabled_application": enabled.get("application"),
    }


def start_product_tracker_evidence(target: Any, torrent_id: str) -> None:
    result = target.shell(
        [
            "am",
            "start",
            "-n",
            ACTIVITY,
            "--es",
            "product_tracker_evidence_torrent",
            torrent_id,
        ],
        timeout=30,
        check=False,
    )
    if "Error:" in result.stdout:
        raise BootstrapFailure("could not start tracker evidence subscription")


def wait_product_tracker_row(
    target: Any,
    torrent_id: str,
    security: str,
    status: str,
    *,
    error: bool,
    timeout_seconds: float,
) -> str:
    start_product_tracker_evidence(target, torrent_id)
    marker = (
        f"tracker_evidence torrent={torrent_id} security={security} "
        f"status={status}"
    )
    error_marker = f"error={'true' if error else 'false'}"
    deadline = time.monotonic() + timeout_seconds
    logs = ""
    while time.monotonic() < deadline:
        logs = product_logs(target)
        if any(
            marker in line and error_marker in line
            for line in logs.splitlines()
        ):
            return logs
        time.sleep(0.2)
    raise BootstrapFailure(
        "timed out waiting for Android tracker evidence "
        f"torrent={torrent_id} security={security} status={status} "
        f"error={error}\n{logs}"
    )


def prepare_product_saf(target: Any, probe: ModuleType, grant_storage: str) -> None:
    deadline = time.monotonic() + 15
    while True:
        try:
            probe.prepare_grant_folder(target, grant_storage)
            break
        except probe.ProbeFailure:
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.2)
    target.shell(
        ["pm", "grant", PACKAGE, "android.permission.POST_NOTIFICATIONS"],
        check=False,
    )
    selected = target.shell(
        [
            "am",
            "start",
            "-n",
            ACTIVITY,
            "--ez",
            "product_select_saf",
            "true",
        ],
        timeout=30,
        check=False,
    )
    if "Error:" in selected.stdout or (
        selected.returncode != 0 and "Starting:" not in selected.stdout
    ):
        raise BootstrapFailure("could not launch product SAF picker")
    probe.automate_tree_grant(target, grant_storage)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if app_text(target, "shared_prefs/product-saf.xml"):
            break
        time.sleep(0.2)
    else:
        raise BootstrapFailure("product SAF grant was not persisted")
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if "saf_tree_ready" in product_logs(target):
            return
        time.sleep(0.2)
    raise BootstrapFailure("product service did not activate the persisted SAF tree")


def launch_product_tracker_magnet(
    target: Any,
    magnet: str,
    policy: str,
    *,
    start_content: bool,
) -> None:
    command = [
        "am",
        "start",
        "-n",
        ACTIVITY,
        "--es",
        "product_magnet",
        shlex.quote(magnet),
        "--es",
        "product_tracker_https_policy",
        policy,
        "--ez",
        "product_start_content",
        "true" if start_content else "false",
    ]
    result = target.shell(command, timeout=30, check=False)
    if "Error:" in result.stdout or (
        result.returncode != 0 and "Starting:" not in result.stdout
    ):
        raise BootstrapFailure("could not add Android product tracker magnet")


def wait_product_publication(
    target: Any,
    torrent_id: str,
    baseline_fds: int,
    sample: Any | None = None,
) -> tuple[dict[str, int], int]:
    deadline = time.monotonic() + 90
    high_water_fds = baseline_fds
    metric_pattern = re.compile(
        rf"saf_storage_metrics torrent={re.escape(torrent_id)} "
        r"limit=(\d+) owned_high_water=(\d+) pending_high_water=(\d+)"
    )
    logs = ""
    while time.monotonic() < deadline:
        if sample is not None:
            sample()
        high_water_fds = max(high_water_fds, product_fd_count(target))
        logs = target.run(
            ["logcat", "-d", "-v", "brief", "RSTorrentProduct:I", "*:S"],
            timeout=15,
            check=False,
        ).stdout
        if f"saf_publication_confirmed torrent={torrent_id}" in logs:
            match = metric_pattern.search(logs)
            if match is None:
                raise BootstrapFailure("dynamic SAF publication omitted storage metrics")
            return (
                {
                    "limit": int(match.group(1)),
                    "owned_high_water": int(match.group(2)),
                    "pending_high_water": int(match.group(3)),
                },
                high_water_fds,
            )
        if "product service initialization failed" in logs:
            raise BootstrapFailure(f"product service failed\n{logs}")
        time.sleep(0.2)
    raise BootstrapFailure(
        "dynamic SAF product download did not publish before timeout\n" + logs
    )


def run_product_dynamic_saf_profile(
    target: Any,
    target_kind: str,
    identity: dict[str, str],
    probe: ModuleType,
    interop: ModuleType,
    ordinal: int,
    storage: str,
    tracker_support: ModuleType | None = None,
) -> dict[str, Any]:
    profile = (
        "product-https-tracker"
        if tracker_support is not None
        else "product-dynamic-saf"
    )
    if not storage.startswith("saf-"):
        raise BootstrapFailure(f"{profile} requires a SAF storage mode")
    grant_storage = "sdcard" if storage == "saf-sdcard" else "internal"
    fixture = SeedFixture.create(interop, f"{target_kind}-{profile}-{ordinal}")
    peer_transport: ReverseTransport | None = None
    tracker_transport: ReverseTransport | None = None
    controlled_tracker: Any | None = None
    output_root = f"{probe.grant_path(grant_storage)}/{fixture.name}"
    staging_root = f"{probe.grant_path(grant_storage)}/.{fixture.name}.rstorrent-staging"
    part_path = f"{probe.grant_path(grant_storage)}/.{fixture.name}.rstorrent-parts"
    try:
        clear_application(target)
        probe.prepare_grant_folder(target, grant_storage)
        target.shell(
            ["pm", "grant", PACKAGE, "android.permission.POST_NOTIFICATIONS"],
            check=False,
        )
        selected = target.shell(
            [
                "am",
                "start",
                "-n",
                ACTIVITY,
                "--ez",
                "product_select_saf",
                "true",
            ],
            timeout=30,
            check=False,
        )
        if "Error:" in selected.stdout or (
            selected.returncode != 0 and "Starting:" not in selected.stdout
        ):
            raise BootstrapFailure(
                "could not launch product SAF picker: "
                f"code={selected.returncode} stdout={selected.stdout} stderr={selected.stderr}"
            )
        probe.automate_tree_grant(target, grant_storage)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if app_text(target, "shared_prefs/product-saf.xml"):
                break
            time.sleep(0.2)
        else:
            raise BootstrapFailure("product SAF grant was not persisted")

        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            logs = target.run(
                ["logcat", "-d", "-v", "brief", "RSTorrentProduct:I", "*:S"],
                timeout=15,
                check=False,
            ).stdout
            if "saf_tree_ready" in logs:
                break
            time.sleep(0.2)
        else:
            raise BootstrapFailure("product service did not activate the persisted SAF tree")

        baseline_fds = product_fd_count(target)
        peer_transport = ReverseTransport.create(
            target,
            target_kind,
            fixture.host_port,
            ordinal,
        )
        if tracker_support is None:
            magnet = (
                f"magnet:?xt=urn:btih:{fixture.info_hash}"
                f"&dn={fixture.name}&x.pe=127.0.0.1:{peer_transport.device_port}"
            )
        else:
            controlled_tracker = tracker_support.ControlledHttpTracker(
                fixture.info_hash,
                peer_transport.device_port,
                https=True,
                certificate_root=fixture.run_path / "tracker-certificate",
            )
            controlled_tracker.start()
            tracker_transport = ReverseTransport.create(
                target,
                target_kind,
                controlled_tracker.port,
                ordinal,
                slot=1,
            )
            tracker_url = controlled_tracker.url_for_port(
                tracker_transport.device_port
            )
            magnet = (
                f"magnet:?xt=urn:btih:{fixture.info_hash}"
                f"&dn={fixture.name}&tr={quote(tracker_url, safe='')}"
            )
        start_command = [
            "am",
            "start",
            "-n",
            ACTIVITY,
            "--es",
            "product_magnet",
            shlex.quote(magnet),
        ]
        if controlled_tracker is not None:
            start_command.extend(
                ["--es", "product_tracker_https_policy", "disabled"]
            )
        started = target.shell(
            start_command,
            timeout=30,
            check=False,
        )
        if "Error:" in started.stdout or (
            started.returncode != 0 and "Starting:" not in started.stdout
        ):
            raise BootstrapFailure(
                "could not add product magnet: "
                f"code={started.returncode} stdout={started.stdout} stderr={started.stderr}"
            )
        metrics, fd_high_water = wait_product_publication(
            target,
            fixture.info_hash,
            baseline_fds,
        )
        if controlled_tracker is not None:
            controlled_tracker.wait_for_event("started")
            evidence = target.shell(
                [
                    "am",
                    "start",
                    "-n",
                    ACTIVITY,
                    "--es",
                    "product_tracker_evidence_torrent",
                    fixture.info_hash,
                ],
                timeout=30,
                check=False,
            )
            if "Error:" in evidence.stdout:
                raise BootstrapFailure("could not start tracker evidence subscription")
            deadline = time.monotonic() + 10
            logs = ""
            while time.monotonic() < deadline:
                logs = target.run(
                    ["logcat", "-d", "-v", "brief", "RSTorrentProduct:I", "*:S"],
                    timeout=15,
                    check=False,
                ).stdout
                if (
                    "security=ENCRYPTED_UNAUTHENTICATED" in logs
                    and "status=REANNOUNCE_WAIT" in logs
                ):
                    break
                time.sleep(0.2)
            if (
                "tracker_https_settings configured=DISABLED "
                "effective=DISABLED application=APPLIED" not in logs
            ):
                raise BootstrapFailure(
                    "Android product did not apply disabled tracker authentication"
                )
            if (
                "security=ENCRYPTED_UNAUTHENTICATED" not in logs
                or "status=REANNOUNCE_WAIT" not in logs
            ):
                raise BootstrapFailure(
                    "Android product did not project the completed unauthenticated HTTPS row\n"
                    + logs
                )
        if metrics["limit"] != 40:
            raise BootstrapFailure(f"unexpected storage handle limit: {metrics}")
        if metrics["owned_high_water"] > metrics["limit"]:
            raise BootstrapFailure(f"storage handle limit was exceeded: {metrics}")
        if metrics["pending_high_water"] > 16:
            raise BootstrapFailure(f"platform request queue bound was exceeded: {metrics}")
        if baseline_fds and fd_high_water - baseline_fds > 48:
            raise BootstrapFailure(
                "process descriptor delta exceeded Rust plus provider bounds: "
                f"baseline={baseline_fds} high_water={fd_high_water}"
            )

        for relative_path, _, padding in fixture_files():
            path = f"{output_root}/{relative_path}"
            exists = target.shell(["test", "-f", path], check=False).returncode == 0
            if padding:
                if exists:
                    raise BootstrapFailure(f"padding file was published: {relative_path}")
                continue
            if not exists:
                raise BootstrapFailure(f"product output is absent: {relative_path}")
            digest = target.shell(["sha1sum", path]).stdout.split()[0]
            if digest != fixture.expected_file_hashes[relative_path]:
                raise BootstrapFailure(f"product output hash differs: {relative_path}")
        for unexpected in (staging_root, part_path, f"{probe.grant_path(grant_storage)}/{fixture.info_hash}"):
            if target.shell(["test", "-e", unexpected], check=False).returncode == 0:
                raise BootstrapFailure(f"unexpected managed artifact survived: {unexpected}")

        return {
            "target": target_kind,
            "profile": profile,
            "run": ordinal,
            "identity": identity,
            "torrent_id": fixture.info_hash,
            "publication_name": fixture.name,
            "storage_metrics": metrics,
            "process_fds": {
                "baseline": baseline_fds,
                "high_water": fd_high_water,
                "final": product_fd_count(target),
            },
            "peer_connections": peer_count(fixture),
            "tracker_security": (
                "encrypted_unauthenticated"
                if controlled_tracker is not None
                else None
            ),
            "tracker_requests": (
                len(controlled_tracker.requests)
                if controlled_tracker is not None
                else 0
            ),
        }
    finally:
        target.shell(["am", "force-stop", PACKAGE], check=False)
        for exact_path in (output_root, staging_root, part_path):
            target.shell(["rm", "-rf", exact_path], check=False)
        probe.remove_grant_folder(target, grant_storage)
        if tracker_transport is not None:
            tracker_transport.close()
        if controlled_tracker is not None:
            controlled_tracker.close()
        if peer_transport is not None:
            peer_transport.close()
        fixture.close()


def run_product_saf_grant_repair_profile(
    target: Any,
    target_kind: str,
    identity: dict[str, str],
    probe: ModuleType,
    storage: str,
) -> dict[str, Any]:
    if not storage.startswith("saf-"):
        raise BootstrapFailure("product-saf-grant-repair requires a SAF storage mode")
    grant_storage = "sdcard" if storage == "saf-sdcard" else "internal"

    def wait_for(marker: str, timeout_seconds: float = 15) -> None:
        deadline = time.monotonic() + timeout_seconds
        logs = ""
        while time.monotonic() < deadline:
            logs = product_logs(target)
            if marker in logs:
                return
            time.sleep(0.2)
        raise BootstrapFailure(f"timed out waiting for {marker}\n{logs}")

    try:
        clear_application(target)
        prepare_product_saf(target, probe, grant_storage)
        wait_for("saf_root_health source=selection available=true")

        target.run(["logcat", "-c"], check=False)
        released = target.shell(
            [
                "am",
                "start",
                "-n",
                ACTIVITY,
                "--ez",
                "product_release_saf_grant",
                "true",
            ],
            timeout=30,
            check=False,
        )
        if "Error:" in released.stdout:
            raise BootstrapFailure("could not trigger product SAF grant release")
        target.shell(["am", "force-stop", PACKAGE], check=False)
        target.run(["logcat", "-c"], check=False)
        restarted = target.shell(
            ["am", "start", "-W", "-n", ACTIVITY],
            timeout=30,
            check=False,
        )
        if "Error:" in restarted.stdout:
            raise BootstrapFailure("could not restart after SAF grant release")
        wait_for("saf_root_health source=startup available=false")
        if not app_text(target, "shared_prefs/product-saf.xml"):
            raise BootstrapFailure("revoked grant erased the stable platform identity")

        target.run(["logcat", "-c"], check=False)
        selected = target.shell(
            [
                "am",
                "start",
                "-n",
                ACTIVITY,
                "--ez",
                "product_select_saf",
                "true",
            ],
            timeout=30,
            check=False,
        )
        if "Error:" in selected.stdout:
            raise BootstrapFailure("could not relaunch SAF repair picker")
        probe.automate_tree_grant(target, grant_storage)
        wait_for("saf_root_health source=selection available=true")

        target.shell(["am", "force-stop", PACKAGE], check=False)
        target.run(["logcat", "-c"], check=False)
        target.shell(["am", "start", "-W", "-n", ACTIVITY], timeout=30)
        wait_for("saf_root_health source=startup available=true")
        return {
            "target": target_kind,
            "profile": "product-saf-grant-repair",
            "identity": identity,
            "startup_healthy": True,
            "revoked_startup_unavailable": True,
            "stable_root_identity_retained": True,
            "selection_repair_healthy": True,
            "repaired_restart_healthy": True,
        }
    finally:
        target.shell(["am", "force-stop", PACKAGE], check=False)
        probe.remove_grant_folder(target, grant_storage)


def run_product_mse_profile(
    target: Any,
    target_kind: str,
    identity: dict[str, str],
    probe: ModuleType,
    interop: ModuleType,
    ordinal: int,
) -> dict[str, Any]:
    import libtorrent as lt

    grant_storage = "internal"
    fixtures = [
        SeedFixture.create(
            interop,
            f"{target_kind}-product-mse-{ordinal}-{attempt}",
            force_rc4=True,
        )
        for attempt in range(1, 6)
    ]
    fixture = fixtures[0]
    transports: list[ReverseTransport] = []
    observed_rc4: set[int] = set()
    output_root = f"{probe.grant_path(grant_storage)}/{fixture.name}"
    staging_root = f"{probe.grant_path(grant_storage)}/.{fixture.name}.rstorrent-staging"
    part_path = f"{probe.grant_path(grant_storage)}/.{fixture.name}.rstorrent-parts"
    try:
        clear_application(target)
        prepare_product_saf(target, probe, grant_storage)
        baseline_fds = product_fd_count(target)
        transports = [
            ReverseTransport.create(
                target,
                target_kind,
                candidate.host_port,
                ordinal,
                slot=index,
            )
            for index, candidate in enumerate(fixtures)
        ]
        peer_hints = "".join(
            f"&x.pe=127.0.0.1:{transport.device_port}" for transport in transports
        )
        magnet = (
            f"magnet:?xt=urn:btih:{fixture.info_hash}&dn={fixture.name}{peer_hints}"
        )
        started = target.shell(
            [
                "am",
                "start",
                "-n",
                ACTIVITY,
                "--es",
                "product_magnet",
                shlex.quote(magnet),
                "--es",
                "product_encryption_policy",
                "required",
            ],
            timeout=30,
            check=False,
        )
        if "Error:" in started.stdout or (
            started.returncode != 0 and "Starting:" not in started.stdout
        ):
            raise BootstrapFailure("could not add Android product MSE magnet")

        def sample_oracle_methods() -> None:
            for index, candidate in enumerate(fixtures):
                try:
                    if any(
                        peer.flags & lt.peer_info.rc4_encrypted
                        for peer in candidate.handle.get_peer_info()
                    ):
                        observed_rc4.add(index)
                except Exception:
                    pass

        metrics, fd_high_water = wait_product_publication(
            target,
            fixture.info_hash,
            baseline_fds,
            sample_oracle_methods,
        )
        sample_oracle_methods()
        if len(observed_rc4) != len(fixtures):
            raise BootstrapFailure(
                "host oracle did not observe forced RC4 on all five attempts: "
                f"{sorted(index + 1 for index in observed_rc4)}"
            )
        evidence = target.shell(
            [
                "am",
                "start",
                "-n",
                ACTIVITY,
                "--ez",
                "product_mse_evidence",
                "true",
            ],
            timeout=30,
            check=False,
        )
        if "Error:" in evidence.stdout:
            raise BootstrapFailure("could not request Android MSE work evidence")
        pattern = re.compile(
            r"mse_dh_work waiting=(\d+) active=(\d+) high_water=(\d+) "
            r"tracked=(\d+) closed=(true|false)"
        )
        deadline = time.monotonic() + 10
        match = None
        logs = ""
        while time.monotonic() < deadline:
            logs = product_logs(target)
            match = pattern.search(logs)
            if match is not None:
                break
            time.sleep(0.2)
        if match is None:
            raise BootstrapFailure("Android MSE work evidence was not logged\n" + logs)
        waiting, active, high_water, tracked = (
            int(match.group(index)) for index in range(1, 5)
        )
        if not 1 <= high_water <= 4:
            raise BootstrapFailure(
                f"five Android MSE attempts observed invalid DH high-water {high_water}"
            )
        if waiting != 0 or active != 0 or tracked != 0:
            raise BootstrapFailure(
                "Android MSE work did not drain: "
                f"waiting={waiting} active={active} tracked={tracked}"
            )
        if "mse_settings configured=REQUIRED effective=REQUIRED application=APPLIED" not in logs:
            raise BootstrapFailure("Android product did not apply Required MSE policy")

        for relative_path, _, padding in fixture_files():
            path = f"{output_root}/{relative_path}"
            exists = target.shell(["test", "-f", path], check=False).returncode == 0
            if padding:
                if exists:
                    raise BootstrapFailure(f"padding file was published: {relative_path}")
                continue
            if not exists:
                raise BootstrapFailure(f"product MSE output is absent: {relative_path}")
            digest = target.shell(["sha1sum", path]).stdout.split()[0]
            if digest != fixture.expected_file_hashes[relative_path]:
                raise BootstrapFailure(f"product MSE output hash differs: {relative_path}")
        for unexpected in (staging_root, part_path):
            if target.shell(["test", "-e", unexpected], check=False).returncode == 0:
                raise BootstrapFailure(f"unexpected MSE artifact survived: {unexpected}")

        return {
            "target": target_kind,
            "profile": "product-mse",
            "run": ordinal,
            "identity": identity,
            "torrent_id": fixture.info_hash,
            "publication_name": fixture.name,
            "forced_rc4_attempts": len(observed_rc4),
            "mse_dh": {
                "waiting": waiting,
                "active": active,
                "high_water": high_water,
                "tracked": tracked,
            },
            "storage_metrics": metrics,
            "process_fds": {
                "baseline": baseline_fds,
                "high_water": fd_high_water,
                "final": product_fd_count(target),
            },
        }
    finally:
        target.shell(["am", "force-stop", PACKAGE], check=False)
        for exact_path in (output_root, staging_root, part_path):
            target.shell(["rm", "-rf", exact_path], check=False)
        probe.remove_grant_folder(target, grant_storage)
        for transport in reversed(transports):
            transport.close()
        for candidate in fixtures:
            candidate.close()


def request_download_admission_evidence(
    target: Any,
    mode: str,
) -> dict[str, int]:
    result = target.shell(
        [
            "am",
            "start",
            "-n",
            ACTIVITY,
            "--es",
            "product_download_admission_evidence",
            mode,
        ],
        timeout=30,
        check=False,
    )
    if "Error:" in result.stdout or (
        result.returncode != 0 and "Starting:" not in result.stdout
    ):
        raise BootstrapFailure("could not request Android download admission evidence")
    marker = f"download_admission mode={mode} "
    deadline = time.monotonic() + 15
    logs = ""
    while time.monotonic() < deadline:
        logs = product_logs(target)
        rows = [line for line in logs.splitlines() if marker in line]
        if rows:
            return {
                key: int(value)
                for key, value in re.findall(r"([a-z_]+)=(\d+)", rows[-1])
            }
        time.sleep(0.2)
    raise BootstrapFailure(
        f"timed out waiting for Android download admission {mode}\n{logs}"
    )


def run_product_concurrent_downloads_profile(
    target: Any,
    target_kind: str,
    identity: dict[str, str],
    probe: ModuleType,
    interop: ModuleType,
    ordinal: int,
) -> dict[str, Any]:
    grant_storage = "internal"
    fixtures = [
        SeedFixture.create(
            interop,
            f"{target_kind}-product-concurrent-{ordinal}-{index}",
            root_name=f"concurrent-{ordinal}-{index}",
            content_offset=index * 1_000_000,
        )
        for index in range(1, 4)
    ]
    transports: list[ReverseTransport] = []
    grant_root = probe.grant_path(grant_storage)
    output_roots = [f"{grant_root}/{fixture.name}" for fixture in fixtures]
    staging_roots = [f"{grant_root}/.{fixture.name}.rstorrent-staging" for fixture in fixtures]
    part_roots = [f"{grant_root}/.{fixture.name}.rstorrent-parts" for fixture in fixtures]
    try:
        clear_application(target)
        prepare_product_saf(target, probe, grant_storage)
        baseline_fds = product_fd_count(target)
        for fixture in fixtures:
            fixture.handle.set_upload_limit(16 * 1024)
        transports = [
            ReverseTransport.create(
                target,
                target_kind,
                fixture.host_port,
                ordinal,
                slot=index,
            )
            for index, fixture in enumerate(fixtures)
        ]
        for fixture, transport in zip(fixtures, transports, strict=True):
            magnet = (
                f"magnet:?xt=urn:btih:{fixture.info_hash}&dn={fixture.name}"
                f"&x.pe=127.0.0.1:{transport.device_port}"
            )
            started = target.shell(
                [
                    "am",
                    "start",
                    "-n",
                    ACTIVITY,
                    "--es",
                    "product_magnet",
                    shlex.quote(magnet),
                ],
                timeout=30,
                check=False,
            )
            if "Error:" in started.stdout or (
                started.returncode != 0 and "Starting:" not in started.stdout
            ):
                raise BootstrapFailure("could not add Android concurrent product magnet")
            time.sleep(0.1)

        active = request_download_admission_evidence(target, "active")
        expected_active = {
            "configured": 3,
            "effective": 2,
            "active": 2,
            "queued": 1,
            "registered": 2,
            "registered_high": 2,
        }
        if any(active.get(key) != value for key, value in expected_active.items()):
            raise BootstrapFailure(f"Android active admission evidence diverged: {active}")

        def validate_resource_ceilings(evidence: dict[str, int]) -> None:
            if evidence.get("request_high", 0) > 128 * 1024 * 1024:
                raise BootstrapFailure(f"Android request ceiling exceeded: {evidence}")
            if evidence.get("payload_high", 0) > 16 * 1024 * 1024:
                raise BootstrapFailure(f"Android payload ceiling exceeded: {evidence}")
            if evidence.get("piece_bytes_high", 0) > 128 * 1024 * 1024:
                raise BootstrapFailure(f"Android piece-byte ceiling exceeded: {evidence}")
            if evidence.get("pieces_high", 0) > 2_048:
                raise BootstrapFailure(f"Android piece-count ceiling exceeded: {evidence}")
            if evidence.get("writes_high", 0) > 4 or evidence.get("hashes_high", 0) > 4:
                raise BootstrapFailure(f"Android storage ceiling exceeded: {evidence}")

        validate_resource_ceilings(active)

        storage_metrics = []
        fd_high_water = baseline_fds
        for fixture in fixtures:
            metrics, observed_fds = wait_product_publication(
                target,
                fixture.info_hash,
                baseline_fds,
            )
            storage_metrics.append(metrics)
            fd_high_water = max(fd_high_water, observed_fds)
        terminal = request_download_admission_evidence(target, "terminal")
        if (
            terminal.get("active") != 0
            or terminal.get("queued") != 0
            or terminal.get("registered") != 0
            or terminal.get("registered_high") != 2
        ):
            raise BootstrapFailure(f"Android terminal admission did not drain: {terminal}")
        validate_resource_ceilings(terminal)

        for fixture, output_root in zip(fixtures, output_roots, strict=True):
            for relative_path, _, padding in fixture_files():
                path = f"{output_root}/{relative_path}"
                exists = target.shell(["test", "-f", path], check=False).returncode == 0
                if padding:
                    if exists:
                        raise BootstrapFailure(f"padding file was published: {path}")
                    continue
                if not exists:
                    raise BootstrapFailure(f"concurrent product output is absent: {path}")
                digest = target.shell(["sha1sum", path]).stdout.split()[0]
                if digest != fixture.expected_file_hashes[relative_path]:
                    raise BootstrapFailure(f"concurrent product hash differs: {path}")
            if fixture.handle.status().total_upload <= 0:
                raise BootstrapFailure(f"host oracle uploaded no payload for {fixture.info_hash}")

        return {
            "target": target_kind,
            "profile": "product-concurrent-downloads",
            "run": ordinal,
            "identity": identity,
            "torrents": [fixture.info_hash for fixture in fixtures],
            "active_admission": active,
            "terminal_admission": terminal,
            "storage_metrics": storage_metrics,
            "process_fds": {
                "baseline": baseline_fds,
                "high_water": fd_high_water,
                "final": product_fd_count(target),
            },
        }
    finally:
        target.shell(["am", "force-stop", PACKAGE], check=False)
        for exact_path in output_roots + staging_roots + part_roots:
            target.shell(["rm", "-rf", exact_path], check=False)
        probe.remove_grant_folder(target, grant_storage)
        for transport in reversed(transports):
            transport.close()
        for fixture in fixtures:
            fixture.close()


def run_product_https_platform_trust_profile(
    target: Any,
    target_kind: str,
    identity: dict[str, str],
    probe: ModuleType,
    tracker_support: ModuleType,
    ordinal: int,
    storage: str,
) -> dict[str, Any]:
    if not storage.startswith("saf-"):
        raise BootstrapFailure("product-https-platform-trust requires SAF storage")
    grant_storage = "sdcard" if storage == "saf-sdcard" else "internal"
    invalid_info_hash = "11" * 20
    valid_origin_info_hash = "22" * 20
    ubuntu_info_hash = "62a4d9e139f3315f8716bcccca0cc984a9809da1"
    certificate_root = Path(
        tempfile.mkdtemp(prefix="rstorrent-android-platform-trust-")
    )
    controlled_tracker: Any | None = None
    tracker_transport: ReverseTransport | None = None
    try:
        clear_application(target)
        prepare_product_saf(target, probe, grant_storage)

        controlled_tracker = tracker_support.ControlledHttpTracker(
            invalid_info_hash,
            1,
            https=True,
            certificate_root=certificate_root,
        )
        controlled_tracker.start()
        tracker_transport = ReverseTransport.create(
            target,
            target_kind,
            controlled_tracker.port,
            ordinal,
            slot=1,
        )
        invalid_url = controlled_tracker.url_for_port(tracker_transport.device_port)
        invalid_magnet = (
            f"magnet:?xt=urn:btih:{invalid_info_hash}"
            f"&dn=invalid-platform-trust&tr={quote(invalid_url, safe='')}"
        )
        launch_product_tracker_magnet(
            target,
            invalid_magnet,
            "system_trust",
            start_content=False,
        )
        invalid_logs = wait_product_tracker_row(
            target,
            invalid_info_hash,
            "ENCRYPTED_SYSTEM_TRUST",
            "RETRY_WAIT",
            error=True,
            timeout_seconds=30,
        )
        if controlled_tracker.requests:
            raise BootstrapFailure(
                "invalid Android platform-trust certificate reached HTTP"
            )
        if (
            "tracker_https_settings configured=SYSTEM_TRUST "
            "effective=SYSTEM_TRUST application=APPLIED" not in invalid_logs
        ):
            raise BootstrapFailure("Android system-trust policy was not effective")
        if not any(
            "error_detail=HTTP tracker request failed: "
            f"TLS failure: {category}" in invalid_logs
            for category in ("unknown_issuer", "certificate_rejected")
        ):
            raise BootstrapFailure(
                "Android untrusted certificate did not retain its stable TLS category"
            )
        target.shell(["am", "force-stop", PACKAGE], check=False)
        probe.remove_grant_folder(target, grant_storage)
        clear_application(target)
        prepare_product_saf(target, probe, grant_storage)

        ubuntu_tracker = "https://torrent.ubuntu.com/announce"
        ubuntu_magnet = (
            f"magnet:?xt=urn:btih:{ubuntu_info_hash}"
            "&dn=ubuntu-platform-trust"
            f"&tr={quote(ubuntu_tracker, safe='')}"
        )
        launch_product_tracker_magnet(
            target,
            ubuntu_magnet,
            "system_trust",
            start_content=False,
        )
        ubuntu_logs = wait_product_tracker_row(
            target,
            ubuntu_info_hash,
            "ENCRYPTED_SYSTEM_TRUST",
            "RETRY_WAIT",
            error=True,
            timeout_seconds=30,
        )
        public_error_marker = (
            "error_detail=HTTP tracker request failed: TLS failure: "
        )
        public_error_line = next(
            (line for line in ubuntu_logs.splitlines() if public_error_marker in line),
            None,
        )
        if public_error_line is None:
            raise BootstrapFailure("Ubuntu public smoke lacked a stable TLS category")
        public_tls_category = public_error_line.partition(public_error_marker)[2].strip()
        if public_tls_category not in {
            "unknown_issuer",
            "expired_or_not_yet_valid",
            "name_mismatch",
            "invalid_server_purpose",
            "certificate_rejected",
            "tls_protocol",
        }:
            raise BootstrapFailure("Ubuntu public smoke used an unknown TLS category")

        target.shell(["am", "force-stop", PACKAGE], check=False)
        probe.remove_grant_folder(target, grant_storage)
        clear_application(target)
        prepare_product_saf(target, probe, grant_storage)
        valid_origin = "https://example.com/announce"
        valid_origin_magnet = (
            f"magnet:?xt=urn:btih:{valid_origin_info_hash}"
            "&dn=valid-platform-trust"
            f"&tr={quote(valid_origin, safe='')}"
        )
        launch_product_tracker_magnet(
            target,
            valid_origin_magnet,
            "system_trust",
            start_content=False,
        )
        valid_logs = wait_product_tracker_row(
            target,
            valid_origin_info_hash,
            "ENCRYPTED_SYSTEM_TRUST",
            "RETRY_WAIT",
            error=True,
            timeout_seconds=30,
        )
        if "error_detail=HTTP tracker returned status 404" not in valid_logs:
            raise BootstrapFailure(
                "system-trusted Android origin did not reach its HTTP response"
            )

        grant_path = probe.grant_path(grant_storage)
        artifacts = target.shell(
            ["find", grant_path, "-mindepth", "1", "-maxdepth", "4", "-print"],
            timeout=15,
            check=False,
        ).stdout.strip()
        if artifacts:
            raise BootstrapFailure(
                "metadata-only Android platform-trust smoke created storage artifacts"
            )
        return {
            "target": target_kind,
            "profile": "product-https-platform-trust",
            "run": ordinal,
            "identity": identity,
            "invalid_chain_name": "rejected_before_http",
            "invalid_tracker_requests": len(controlled_tracker.requests),
            "valid_chain_name": "accepted_through_http_404",
            "valid_origin": "example.com",
            "public_tracker": "torrent.ubuntu.com",
            "public_info_hash": ubuntu_info_hash,
            "public_security": "encrypted_system_trust",
            "public_result": f"tls_{public_tls_category}",
            "start_content": False,
            "storage_artifacts": 0,
        }
    finally:
        target.shell(["am", "force-stop", PACKAGE], check=False)
        probe.remove_grant_folder(target, grant_storage)
        if tracker_transport is not None:
            tracker_transport.close()
        if controlled_tracker is not None:
            controlled_tracker.close()
        shutil.rmtree(certificate_root, ignore_errors=True)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--target",
        choices=["avd", "chromeos", "pixel7a", "motox4"],
        required=True,
    )
    parser.add_argument("--avd", default="jstorrent-tablet")
    parser.add_argument("--runs", type=int, choices=range(1, 6), default=1)
    parser.add_argument(
        "--storage",
        choices=["private", "saf-internal", "saf-sdcard"],
        default="private",
    )
    parser.add_argument(
        "--profile",
        action="append",
        choices=PROFILE_CHOICES,
        dest="profiles",
    )
    parser.add_argument("--no-build", action="store_true")
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    if arguments.storage == "saf-sdcard" and arguments.target != "motox4":
        print("SAF removable storage is available only on motox4", file=sys.stderr)
        return 1
    ensure_interop_environment()
    probe, interop, tracker_support = load_support()
    apk = (
        bootstrap_root() / "app" / "build" / "outputs" / "apk" / "debug" /
        "app-debug.apk"
        if arguments.no_build
        else build_apk()
    )
    if not apk.is_file():
        print(f"bootstrap APK is unavailable at {apk}", file=sys.stderr)
        return 1

    profiles = arguments.profiles or ["success"]
    avd_session = None
    target = None
    results: list[dict[str, Any]] = []
    failure: BaseException | None = None
    try:
        if arguments.target == "avd":
            avd_session = probe.start_avd(arguments.avd)
            target = avd_session.target
        elif arguments.target == "chromeos":
            target = probe.prepare_chromeos()
        elif arguments.target == "pixel7a":
            target = probe.prepare_pixel()
        else:
            target = probe.prepare_moto()
        identity = probe.verify_target(target, arguments.target)
        if arguments.storage != "private":
            identity.update(
                probe.verify_storage(
                    target,
                    arguments.target,
                    "sdcard"
                    if arguments.storage == "saf-sdcard"
                    else "internal",
                )
            )
        probe.install_apk(target, arguments.target, apk)

        for profile in profiles:
            repetitions = (
                arguments.runs
                if profile
                in (
                    "success",
                    "product-dynamic-saf",
                    "product-https-tracker",
                    "product-mse",
                    "product-concurrent-downloads",
                )
                else 1
            )
            for ordinal in range(1, repetitions + 1):
                if profile == "cancellation":
                    result = run_cancellation_profile(
                        target,
                        arguments.target,
                        identity,
                        probe,
                        interop,
                        ordinal,
                        arguments.storage,
                    )
                elif profile == "preexisting-artifacts":
                    result = run_preexisting_profile(
                        target,
                        arguments.target,
                        identity,
                        interop,
                        ordinal,
                    )
                elif profile == "product-dynamic-saf":
                    result = run_product_dynamic_saf_profile(
                        target,
                        arguments.target,
                        identity,
                        probe,
                        interop,
                        ordinal,
                        arguments.storage,
                    )
                elif profile == "product-saf-grant-repair":
                    result = run_product_saf_grant_repair_profile(
                        target,
                        arguments.target,
                        identity,
                        probe,
                        arguments.storage,
                    )
                elif profile == "product-https-tracker":
                    result = run_product_dynamic_saf_profile(
                        target,
                        arguments.target,
                        identity,
                        probe,
                        interop,
                        ordinal,
                        arguments.storage,
                        tracker_support,
                    )
                elif profile == "product-https-platform-trust":
                    result = run_product_https_platform_trust_profile(
                        target,
                        arguments.target,
                        identity,
                        probe,
                        tracker_support,
                        ordinal,
                        arguments.storage,
                    )
                elif profile == "product-mse":
                    result = run_product_mse_profile(
                        target,
                        arguments.target,
                        identity,
                        probe,
                        interop,
                        ordinal,
                    )
                elif profile == "product-concurrent-downloads":
                    result = run_product_concurrent_downloads_profile(
                        target,
                        arguments.target,
                        identity,
                        probe,
                        interop,
                        ordinal,
                    )
                elif profile == "product-ipv6-policy":
                    result = run_product_ipv6_policy_profile(
                        target,
                        arguments.target,
                        identity,
                        ordinal,
                    )
                else:
                    result = run_standard_profile(
                        target,
                        arguments.target,
                        identity,
                        probe,
                        interop,
                        profile,
                        ordinal,
                        arguments.storage,
                    )
                results.append(result)
                print(json.dumps(result, sort_keys=True), flush=True)
    except BaseException as error:
        failure = error
    finally:
        if target is not None:
            target.shell(["am", "force-stop", PACKAGE], check=False)
            target.shell(["pm", "clear", PACKAGE], check=False)
            target.run(["reverse", "--remove-all"], timeout=15, check=False)
            target.run(["uninstall", PACKAGE], timeout=30, check=False)
        if avd_session is not None:
            avd_session.close()

    if failure is not None:
        print(f"bootstrap failed: {failure}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "target": arguments.target,
                "profiles": profiles,
                "storage": arguments.storage,
                "results": len(results),
                "result": "pass",
                "cleanup": "ok",
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
