#!/usr/bin/env python3
"""Prove one-row hybrid lifecycle through the production browser."""

from __future__ import annotations

import gc
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from application_surface_harness import build_gateway, connection_metrics, start_gateway, stop_gateway
from browser_peer_inspection_surface import build_and_start_production_web
from browser_surface_harness import reserve_loopback_port, stop_process
from browser_torrent_file_intake import run_v2_magnet_playwright, verify_metrics
from first_verified_piece import ScenarioFailure
from hybrid_runtime import SKIPPED_FILE, make_fixture
from pure_v2_runtime import PlaintextBep52Proxy, start_libtorrent_seed


def verify_selected_output(fixture, storage: Path) -> None:
    output = storage / "root"
    for index, source in enumerate(fixture.files):
        relative = Path(*(component.decode("utf-8") for component in source.path))
        path = output / relative
        if index == SKIPPED_FILE:
            if path.exists():
                raise ScenarioFailure("hybrid browser published the skipped file")
        elif not path.is_file() or path.read_bytes() != source.data:
            raise ScenarioFailure(f"hybrid browser output differs: {relative}")


def run() -> None:
    repository = Path(__file__).resolve().parents[2]
    run_root = Path(tempfile.mkdtemp(prefix="rstorrent-browser-hybrid-"))
    gateway: subprocess.Popen[str] | None = None
    vite: subprocess.Popen[str] | None = None
    session = None
    handle = None
    proxy: PlaintextBep52Proxy | None = None
    failure: BaseException | None = None
    try:
        fixture = make_fixture(run_root)
        session, handle, _diagnostics = start_libtorrent_seed(fixture)
        proxy = PlaintextBep52Proxy(("127.0.0.1", session.listen_port()))
        profile = run_root / "profile"
        storage = run_root / "downloads"
        port = reserve_loopback_port()
        origin = f"http://127.0.0.1:{port}"
        selection = "0-2,4-5"
        torrent_name = "root"
        first = (
            f"magnet:?xt=urn:btih:{fixture.wire_info_hash}"
            f"&dn={fixture.name}&so={selection}"
        )
        second = (
            "magnet:?xt=urn:btmh:1220"
            f"{fixture.full_info_hash}&dn={fixture.name}"
            f"&x.pe={proxy.endpoint[0]}:{proxy.endpoint[1]}&so={selection}"
        )

        gateway, address = start_gateway(
            build_gateway(repository), profile, storage, origin, "loopback_only"
        )
        vite = build_and_start_production_web(repository, origin, port, address)
        added = run_v2_magnet_playwright(
            repository,
            origin,
            address,
            torrent_name,
            first,
            "add",
            skip_name="d-skipped-multi.bin",
            second_magnet=second,
            v1_info_hash=fixture.wire_info_hash,
            v2_info_hash=fixture.full_info_hash,
            file_count=len(fixture.files) + 1,
        )
        verify_selected_output(fixture, storage)
        before_restart = proxy.snapshot()
        requested = {int(piece) for piece, _, _ in before_restart["piece_messages"]}
        if (
            not before_restart["hash_requests"]
            or int(before_restart["hash_responses"]) < 1
            or requested & {3, 4}
        ):
            raise ScenarioFailure(
                f"hybrid browser wire evidence is incomplete: {before_restart}"
            )
        stop_process(vite, "Vite")
        vite = None
        first_diagnostics = stop_gateway(gateway)
        gateway = None
        first_metrics = connection_metrics(first_diagnostics)
        verify_metrics(first_metrics, 1, 0)

        gateway, address = start_gateway(
            build_gateway(repository), profile, storage, origin, "loopback_only"
        )
        vite = build_and_start_production_web(repository, origin, port, address)
        restarted = run_v2_magnet_playwright(
            repository,
            origin,
            address,
            torrent_name,
            first,
            "restart_remove",
            skip_name="d-skipped-multi.bin",
            second_magnet=second,
            v1_info_hash=fixture.wire_info_hash,
            v2_info_hash=fixture.full_info_hash,
            file_count=len(fixture.files) + 1,
        )
        after_restart = proxy.snapshot()
        if after_restart != before_restart:
            raise ScenarioFailure(
                "complete hybrid browser restart used the peer instead of local state: "
                f"before={before_restart} after={after_restart}"
            )
        if (storage / "root").exists():
            raise ScenarioFailure("hybrid browser removal retained managed payload")
        stop_process(vite, "Vite")
        vite = None
        restart_diagnostics = stop_gateway(gateway)
        gateway = None
        restart_metrics = connection_metrics(restart_diagnostics)
        verify_metrics(restart_metrics, 1, 0)
        print(
            f"{added} {restarted} interop=browser-hybrid-runtime "
            "rows=1 identities=2 entry=separate-btih-btmh "
            "selection=exact restart=local seeding=complete removal=exact "
            f"hash_requests={len(before_restart['hash_requests'])} "
            f"hash_responses={before_restart['hash_responses']} "
            f"piece_frames={before_restart['piece_frames']} "
            f"gateway_metrics={json.dumps(first_metrics, sort_keys=True, separators=(',', ':'))} "
            f"restart_metrics={json.dumps(restart_metrics, sort_keys=True, separators=(',', ':'))}"
        )
    except BaseException as error:
        failure = error
        if proxy is not None:
            print(
                "browser_hybrid_wire_debug "
                + json.dumps(proxy.snapshot(), sort_keys=True),
                file=sys.stderr,
            )
        raise
    finally:
        if vite is not None:
            try:
                stop_process(vite, "Vite")
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"Vite cleanup failed: {cleanup_error}", file=sys.stderr)
        if gateway is not None:
            try:
                stop_gateway(gateway)
            except BaseException as cleanup_error:
                if failure is None:
                    raise
                print(f"gateway cleanup failed: {cleanup_error}", file=sys.stderr)
        if proxy is not None:
            proxy.close()
        if session is not None and handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        if session is not None:
            session.pause()
        gc.collect()
        shutil.rmtree(run_root, ignore_errors=True)


def main() -> int:
    try:
        run()
    except (ScenarioFailure, OSError, subprocess.SubprocessError) as error:
        print(f"browser hybrid runtime failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
