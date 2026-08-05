#!/usr/bin/env python3
"""Prove a scarce piece is verified first in a controlled mixed swarm."""

from __future__ import annotations

import gc
import select
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

import libtorrent as lt

from first_verified_piece import (
    DEFAULT_PAYLOAD_ALLOWANCE,
    ScenarioConfig,
    ScenarioFailure,
    add_seed,
    build_diagnostic,
    compare_payloads,
    create_fixture,
    create_session,
    parse_diagnostic,
    wait_for_listener,
)
from multi_peer_liveness import AdversePeer


CONFIG = ScenarioConfig(
    name="rarest-first",
    payload_size=512 * 1024,
    piece_size=64 * 1024,
    payload_allowance=DEFAULT_PAYLOAD_ALLOWANCE,
    diagnostic_timeout_seconds=15,
    process_timeout_seconds=20,
)


class GatedProxy:
    """Delay the libtorrent handshake until the common peer joins content."""

    def __init__(self, target_port: int, gate: threading.Event) -> None:
        self.target_port = target_port
        self.gate = gate
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(4)
        self.listener.settimeout(0.1)
        self.address = f"127.0.0.1:{self.listener.getsockname()[1]}"
        self.stop_requested = threading.Event()
        self.started = threading.Event()
        self.thread = threading.Thread(target=self._run, name="gated-libtorrent-proxy")
        self.sockets: list[socket.socket] = []
        self.error: BaseException | None = None

    def start(self) -> None:
        self.thread.start()
        if not self.started.wait(1):
            raise ScenarioFailure("gated libtorrent proxy did not start")

    def shutdown(self) -> None:
        self.stop_requested.set()
        try:
            self.listener.close()
        except OSError:
            pass
        for stream in self.sockets:
            try:
                stream.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            stream.close()
        self.thread.join(2)
        if self.thread.is_alive():
            raise ScenarioFailure("gated libtorrent proxy did not terminate")
        if self.error is not None:
            raise ScenarioFailure(f"gated libtorrent proxy failed: {self.error}")

    def _run(self) -> None:
        self.started.set()
        try:
            while not self.stop_requested.is_set():
                try:
                    downstream, _ = self.listener.accept()
                except TimeoutError:
                    continue
                except OSError:
                    if self.stop_requested.is_set():
                        break
                    raise
                self.sockets.append(downstream)
                while not self.gate.wait(0.05):
                    if self.stop_requested.is_set():
                        return
                upstream = socket.create_connection(
                    ("127.0.0.1", self.target_port),
                    timeout=2,
                )
                self.sockets.append(upstream)
                self._forward(downstream, upstream)
                self.sockets.remove(downstream)
                self.sockets.remove(upstream)
                downstream.close()
                upstream.close()
        except BaseException as error:
            if not self.stop_requested.is_set():
                self.error = error

    def _forward(self, downstream: socket.socket, upstream: socket.socket) -> None:
        while not self.stop_requested.is_set():
            readable, _, _ = select.select([downstream, upstream], [], [], 0.1)
            for source in readable:
                payload = source.recv(64 * 1024)
                if not payload:
                    return
                target = upstream if source is downstream else downstream
                target.sendall(payload)


def run(repository: Path) -> None:
    run_path = Path(tempfile.mkdtemp(prefix="rstorrent-rarest-first-"))
    session: lt.session | None = None
    handle: lt.torrent_handle | None = None
    common_peer: AdversePeer | None = None
    proxy: GatedProxy | None = None
    diagnostics: list[str] = []
    failure: BaseException | None = None
    cleanup_errors: list[str] = []
    try:
        binary = build_diagnostic(repository)
        _, seed_directory, payload_path, expected_hash, torrent_info = create_fixture(
            run_path,
            CONFIG,
            require_single_piece=False,
        )
        piece_count = torrent_info.num_pieces()
        if piece_count != 8:
            raise ScenarioFailure(f"fixture has {piece_count} pieces instead of eight")
        scarce_piece = piece_count - 1
        info = bytes(torrent_info.info_section())
        info_hash = bytes.fromhex(str(torrent_info.info_hashes().v1))
        common_peer = AdversePeer(
            info_hash,
            info,
            piece_count,
            advertised_pieces=set(range(scarce_piece)),
        )
        common_peer.start()

        session = create_session()
        libtorrent_port = wait_for_listener(session, diagnostics)
        handle = add_seed(session, torrent_info, seed_directory, diagnostics)
        proxy = GatedProxy(libtorrent_port, common_peer.content_interested)
        proxy.start()
        magnet = (
            f"magnet:?xt=urn:btih:{info_hash.hex()}"
            f"&x.pe={common_peer.address}&x.pe={proxy.address}"
        )
        output_path = run_path / "downloaded.bin"
        completed = subprocess.run(
            [
                str(binary),
                "--magnet",
                magnet,
                "--output",
                str(output_path),
                "--timeout-seconds",
                str(CONFIG.diagnostic_timeout_seconds),
                "--max-buffered-payload-bytes",
                str(CONFIG.payload_allowance),
            ],
            capture_output=True,
            text=True,
            timeout=CONFIG.process_timeout_seconds,
            check=False,
        )
        diagnostics.extend(alert.message() for alert in session.pop_alerts())
        if completed.returncode != 0:
            raise ScenarioFailure(
                f"RSTorrent exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
            )
        fields = parse_diagnostic(completed.stdout, CONFIG)
        if fields.get("first_verified_piece") != str(scarce_piece):
            raise ScenarioFailure(
                "scarce piece was not verified first: "
                f"expected {scarce_piece}, got {fields.get('first_verified_piece')}\n"
                f"stdout:\n{completed.stdout}"
            )
        actual_hash = compare_payloads(payload_path, output_path)
        if actual_hash != expected_hash:
            raise ScenarioFailure("rarest-first run published unexpected payload bytes")
        if common_peer.interested_messages == 0:
            raise ScenarioFailure("common-piece peer never joined the content swarm")
        print(
            "scenario=rarest-first result=pass "
            f"pieces={piece_count} scarce_piece={scarce_piece} "
            f"first_verified_piece={fields['first_verified_piece']} "
            f"common_peer_interested={common_peer.interested_messages}"
        )
        print(f"diagnostic={completed.stdout.strip()}")
    except BaseException as error:
        failure = error
    finally:
        if proxy is not None:
            try:
                proxy.shutdown()
            except BaseException as error:
                cleanup_errors.append(str(error))
        if common_peer is not None:
            try:
                common_peer.shutdown()
            except BaseException as error:
                cleanup_errors.append(str(error))
        if session is not None:
            try:
                diagnostics.extend(alert.message() for alert in session.pop_alerts())
                if handle is not None and handle.is_valid():
                    session.remove_torrent(handle)
                session.pause()
            except BaseException as error:
                cleanup_errors.append(f"libtorrent cleanup failed: {error}")
        handle = None
        session = None
        gc.collect()
        try:
            shutil.rmtree(run_path)
        except OSError as error:
            cleanup_errors.append(f"temporary cleanup failed: {error}")

    if failure is not None or cleanup_errors:
        detail = str(failure) if failure is not None else "scenario cleanup failed"
        if cleanup_errors:
            detail += "; " + "; ".join(cleanup_errors)
        alerts = "\n".join(diagnostics[-100:]) or "(no libtorrent alerts)"
        raise ScenarioFailure(f"{detail}\nlibtorrent alerts:\n{alerts}") from failure


def main() -> int:
    repository = Path(__file__).resolve().parents[2]
    try:
        run(repository)
    except ScenarioFailure as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
