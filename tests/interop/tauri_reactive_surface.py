#!/usr/bin/env python3
"""Exercise the Tauri command/channel surface with a controlled libtorrent seed."""

from __future__ import annotations

import gc
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import libtorrent as lt

from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    create_session,
    wait_for_listener,
)
from application_surface_harness import UPLOAD_RATE_LIMIT
from magnet_metadata import create_fixture, magnet_uri


def verify_payload(data_root: Path, expected_hash: str) -> Path:
    payloads = list(data_root.rglob("payload.bin"))
    if len(payloads) != 1:
        raise ScenarioFailure(
            f"Tauri profile contains {len(payloads)} published payloads: {payloads}"
        )
    actual = hashlib.sha1(payloads[0].read_bytes()).hexdigest()
    if actual != expected_hash:
        raise ScenarioFailure(
            f"Tauri payload hash differs: expected {expected_hash}, got {actual}"
        )
    return payloads[0]


def run() -> None:
    repository = Path(__file__).resolve().parents[2]
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-tauri-reactive-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    try:
        fixture = create_fixture(run_path)
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
        data_root = run_path / "xdg-data"
        config_root = run_path / "xdg-config"
        cache_root = run_path / "xdg-cache"
        runtime_root = run_path / "xdg-runtime"
        for path in (data_root, config_root, cache_root, runtime_root):
            path.mkdir()
        runtime_root.chmod(0o700)
        environment = os.environ.copy()
        environment.update(
            {
                "VITE_RSTORRENT_INTEROP_MAGNET": magnet_uri(
                    fixture.info_hash,
                    f"127.0.0.1:{port}",
                ),
                "XDG_DATA_HOME": str(data_root),
                "XDG_CONFIG_HOME": str(config_root),
                "XDG_CACHE_HOME": str(cache_root),
                "XDG_RUNTIME_DIR": str(runtime_root),
                "NO_AT_BRIDGE": "1",
                "WEBKIT_DISABLE_DMABUF_RENDERER": "1",
            }
        )
        completed = subprocess.run(
            [
                "xvfb-run",
                "-a",
                "--server-args=-screen 0 1280x800x24",
                str(repository / "clients/web/node_modules/.bin/tauri"),
                "dev",
                "--config",
                str(repository / "clients/desktop/src-tauri/tauri.dev.conf.json"),
                "--no-watch",
            ],
            cwd=repository,
            capture_output=True,
            text=True,
            env=environment,
            timeout=120,
            check=False,
        )
        if completed.returncode != 0:
            raise ScenarioFailure(
                f"Tauri interop exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )
        payload = verify_payload(data_root, fixture.payload_hash)
        print(
            f"tauri=webkitgtk info_hash={fixture.info_hash} "
            f"metadata_size={len(fixture.info_bytes)} pieces=3 "
            f"payload_sha1={fixture.payload_hash} "
            f"profile_payload={payload.relative_to(data_root)} "
            "pause_resume=ok command_channel_completion=ok "
            "shutdown=joined cleanup=ok"
        )
    finally:
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
    print(f"libtorrent_binding_version={lt.__version__}")
    print(f"libtorrent_native_version={lt.version}")
    try:
        run()
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
