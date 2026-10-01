"""Actual old-client updater handoff shared by claimed installed rehearsals."""
import json
import os
from pathlib import Path
import subprocess
import signal
import sys
import time


def owned_pids(app):
    if os.name == "nt":
        # Only exact executable paths in the isolated installation count.
        command = "Get-CimInstance Win32_Process | Select-Object ProcessId,ExecutablePath | ConvertTo-Json -Compress"
        rows = json.loads(subprocess.check_output(["powershell.exe", "-NoProfile", "-Command", command], timeout=30)) or []
        if isinstance(rows, dict):
            rows = [rows]
        return [int(row["ProcessId"]) for row in rows if (row.get("ExecutablePath") or "").lower().startswith(str(app).lower() + "\\")]
    if sys.platform == "darwin":
        rows = subprocess.check_output(["ps", "-ww", "-U", str(os.getuid()), "-o", "pid=,command="], timeout=10).decode().splitlines()
        return [int(row.strip().split(None, 1)[0]) for row in rows if len(row.strip().split(None, 1)) == 2 and row.strip().split(None, 1)[1].startswith(str(app) + "/")]
    result = []
    for path in Path("/proc").glob("[0-9]*/environ"):
        try:
            if path.stat().st_uid == os.getuid() and ("APPIMAGE=" + str(app)).encode() in path.read_bytes().split(b"\0"):
                result.append(int(path.parent.name))
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            pass
    return result


def stop_owned(app):
    """Join updater-spawned owners before restoring their installed files."""
    for force in (False, True):
        for pid in owned_pids(app):
            if os.name == "nt":
                command = ["taskkill.exe", "/PID", str(pid), "/T"]
                if force:
                    command.append("/F")
                subprocess.run(command, capture_output=True, timeout=15)
            else:
                try:
                    os.kill(pid, signal.SIGKILL if force else signal.SIGTERM)
                except ProcessLookupError:
                    pass
        end = time.monotonic() + 15
        while time.monotonic() < end:
            if not owned_pids(app):
                return
            time.sleep(0.1)
    raise RuntimeError("owned updater processes did not join; retain restoration backups")


def check_and_apply(args, native, app, launch, wait_until, phase, installed, results):
    raw = args.trial_installation_id.read_text().strip()
    import uuid
    assert str(uuid.UUID(raw)) == raw
    (native / "cfu-id").write_text(raw)
    result_path = native / "update-check-result.json"
    result_path.unlink(missing_ok=True)
    check = launch("legacy", ("--check-update",))
    wait_until(lambda: check.poll() is not None, seconds=180)
    assert check.wait() == 0
    status = json.loads(result_path.read_text())
    assert status.get("available") is True and status.get("version") == "0.3.0" and not status.get("error"), status
    assert not installed(), "check-only unexpectedly replaced the legacy app"
    results["checks"].append("old-unmodified-HTTPS-check-offers-pinned-successor")
    if args.trial_negative:
        before = set(owned_pids(app))
        assert not before, "negative update requires a stopped installation"
        negative = launch("legacy", ("--auto-update",))
        phase("negative-updating")
        if args.trial_negative == "interrupted":
            log = args.root / "legacy.log"
            def started():
                return log.exists() and "download progress:" in log.read_text(errors="replace")
            wait_until(started, seconds=120)
            assert not installed(), "download completed before interruption"
            if os.name == "nt":
                subprocess.run(["taskkill.exe", "/PID", str(negative.pid), "/T", "/F"], check=True, timeout=15)
            elif sys.platform == "linux":
                # The extract-and-run AppImage may have a launcher parent.
                # Its driver-created process group owns the whole attempt.
                os.killpg(negative.pid, signal.SIGTERM)
            else:
                os.kill(negative.pid, signal.SIGTERM)
        wait_until(lambda: negative.poll() is not None, seconds=180)
        if args.trial_negative == "wrong-signature":
            result = json.loads((native / "update-check-result.json").read_text())
            assert "Install failed" in result.get("error", "") and "signature" in result["error"].lower(), result
        wait_until(lambda: set(owned_pids(app)) == before, seconds=30)
        assert not installed()
        results["checks"].append(args.trial_negative + "-preserves-old-installation")
        phase("negative-complete")
        wait_until(lambda: (args.root / "allow-retry").is_file(), seconds=1800)
    arguments = ("--force-desktop", "--profile", args.rehearsal_profile) if args.trial_gui else ("--auto-update",)
    updating = launch("legacy", arguments)
    phase("signed-update-gui" if args.trial_gui else "signed-updating")
    wait_until(installed, seconds=600)
    results["checks"].append("old-updater-download-authenticates-and-installs-successor")
    # The ordinary updater owns OS restart. The controller uses native Quit;
    # the driver independently joins every executable in its installed scope.
    return updating
