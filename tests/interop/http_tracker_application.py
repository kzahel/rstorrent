#!/usr/bin/env python3
"""Download from a pinned libtorrent seed discovered only through HTTP."""

from __future__ import annotations

import gc
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import quote, unquote_to_bytes, urlsplit

import libtorrent as lt

from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    compare_payloads,
    create_session,
    wait_for_listener,
)
from magnet_metadata import create_fixture
from session_resume import (
    build_binary,
    envelope,
    exchange,
    start_process,
    stop_process,
    wait_for_complete,
)


TRANSFER_TIMEOUT_SECONDS = 45


class ControlledHttpTracker:
    def __init__(self, info_hash: str, peer_port: int) -> None:
        self.info_hash = bytes.fromhex(info_hash)
        self.peer_port = peer_port
        self.events: list[str] = []
        self.requests: list[str] = []
        self.failure: BaseException | None = None
        self.changed = threading.Condition()
        tracker = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API
                try:
                    tracker.handle(self)
                except BaseException as error:
                    tracker.failure = error
                    with tracker.changed:
                        tracker.changed.notify_all()
                    self.send_error(500)

            def log_message(self, _format: str, *_arguments: object) -> None:
                return

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.port = int(self.server.server_address[1])
        self.thread = threading.Thread(
            target=self.server.serve_forever,
            name=f"rstorrent-http-tracker-{self.port}",
        )

    @property
    def url(self) -> str:
        return f"http://127.0.0.1:{self.port}/announce/private-token?passkey=fixture"

    def start(self) -> None:
        self.thread.start()

    def wait_for_event(self, expected: str) -> None:
        deadline = time.monotonic() + 10
        with self.changed:
            while expected not in self.events and time.monotonic() < deadline:
                self.raise_failure()
                self.changed.wait(timeout=0.1)
        self.raise_failure()
        if expected not in self.events:
            raise ScenarioFailure(f"HTTP tracker did not observe {expected}")

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=3)
        if self.thread.is_alive():
            raise ScenarioFailure("controlled HTTP tracker did not terminate")
        self.raise_failure()

    def raise_failure(self) -> None:
        if self.failure is not None:
            raise ScenarioFailure(f"controlled HTTP tracker failed: {self.failure}")

    def handle(self, request: BaseHTTPRequestHandler) -> None:
        target = urlsplit(request.path)
        if target.path != "/announce/private-token":
            raise ScenarioFailure("HTTP tracker request lost its passkey path")
        fields: dict[str, bytes] = {}
        for pair in target.query.split("&"):
            name, separator, value = pair.partition("=")
            if not separator or name in fields:
                raise ScenarioFailure("HTTP tracker query is malformed or duplicated")
            fields[name] = unquote_to_bytes(value)
        if fields.pop("passkey", None) != b"fixture":
            raise ScenarioFailure("HTTP tracker request lost its existing query")
        required = {
            "info_hash",
            "peer_id",
            "port",
            "uploaded",
            "downloaded",
            "left",
            "compact",
            "no_peer_id",
            "key",
            "numwant",
        }
        if not required.issubset(fields):
            raise ScenarioFailure("HTTP tracker request omitted announce fields")
        if fields["info_hash"] != self.info_hash:
            raise ScenarioFailure("HTTP tracker request used the wrong info hash")
        if not fields["peer_id"].startswith(b"-RS0001-"):
            raise ScenarioFailure("HTTP tracker request used the wrong peer ID")
        if fields["compact"] != b"1" or fields["no_peer_id"] != b"1":
            raise ScenarioFailure("HTTP tracker request omitted compact response flags")
        if int(fields["key"]) == 0:
            raise ScenarioFailure("HTTP tracker request used a zero key")
        event = fields.get("event", b"update").decode("ascii")
        if event == "stopped":
            if fields["numwant"] != b"0":
                raise ScenarioFailure("stopped announce requested peers")
            peers = b""
        else:
            if fields["numwant"] != b"200":
                raise ScenarioFailure("active announce used the wrong peer limit")
            peers = socket.inet_aton("127.0.0.1") + struct.pack("!H", self.peer_port)
        body = bytes(
            lt.bencode(
                {
                    b"interval": 900,
                    b"peers": peers,
                    b"tracker id": b"controlled",
                }
            )
        )
        request.send_response(200)
        request.send_header("Content-Type", "text/plain")
        request.send_header("Content-Length", str(len(body)))
        request.send_header("Connection", "close")
        request.end_headers()
        request.wfile.write(body)
        with self.changed:
            self.requests.append(request.path)
            self.events.append(event)
            self.changed.notify_all()


def tracker_magnet(info_hash: str, tracker_url: str) -> str:
    return f"magnet:?xt=urn:btih:{info_hash}&tr={quote(tracker_url, safe='')}"


def run(binary: Path, root: Path) -> dict[str, Any]:
    fixture = create_fixture(root)
    diagnostics: list[str] = []
    session = create_session()
    handle: lt.torrent_handle | None = None
    process: subprocess.Popen[str] | None = None
    tracker: ControlledHttpTracker | None = None
    try:
        peer_port = wait_for_listener(session, diagnostics)
        handle = add_seed(
            session,
            fixture.torrent_info,
            fixture.seed_directory,
            diagnostics,
        )
        tracker = ControlledHttpTracker(fixture.info_hash, peer_port)
        tracker.start()
        payload_root = root / "payload"
        process = start_process(
            binary,
            root / "profile",
            payload_root,
            timeout_seconds=TRANSFER_TIMEOUT_SECONDS,
        )
        exchange(
            process,
            envelope(
                "enable-loopback-listener",
                {
                    "type": "set_client_settings",
                    "settings": {
                        "listener": {"type": "automatic_loopback"},
                        "preferred_listen_port": 6881,
                        "port_mapping": "disabled",
                        "peer_connection_limit": 200,
                        "upload_slots": 8,
                    },
                },
            ),
        )
        exchange(process, envelope("restart", {"type": "shutdown"}))
        process.wait(timeout=10)
        process = start_process(
            binary,
            root / "profile",
            payload_root,
            timeout_seconds=TRANSFER_TIMEOUT_SECONDS,
        )
        exchange(
            process,
            envelope(
                "add-http-tracker",
                {
                    "type": "add_magnet",
                    "magnet": tracker_magnet(fixture.info_hash, tracker.url),
                    "storage_root": "downloads",
                    "start_content": True,
                    "skip_files": [],
                },
            ),
        )
        completion = wait_for_complete(
            process,
            fixture,
            timeout_seconds=TRANSFER_TIMEOUT_SECONDS,
        )
        tracker.wait_for_event("started")
        tracker.wait_for_event("completed")
        output = payload_root / fixture.torrent_info.name() / "payload.bin"
        payload_hash = compare_payloads(fixture.payload_path, output)
        if payload_hash != fixture.payload_hash:
            raise ScenarioFailure("HTTP tracker payload differs from libtorrent seed")
        exchange(process, envelope("shutdown", {"type": "shutdown"}), timeout_seconds=10)
        process.wait(timeout=10)
        process = None
        tracker.wait_for_event("stopped")
        return {
            "info_hash": fixture.info_hash,
            "revision": int(completion["revision"]),
            "payload_sha1": payload_hash,
            "events": list(tracker.events),
            "requests": len(tracker.requests),
            "libtorrent_binding": lt.__version__,
            "libtorrent_native": lt.version,
        }
    finally:
        if process is not None:
            stop_process(process, graceful=False)
        if tracker is not None:
            tracker.close()
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def main() -> int:
    if len(sys.argv) != 1:
        raise ScenarioFailure("this controlled gate accepts no arguments")
    repository = Path(__file__).resolve().parents[2]
    root = Path(tempfile.mkdtemp(prefix="rstorrent-http-tracker-application-"))
    try:
        result = run(build_binary(repository), root)
        print(f"libtorrent_binding_version={result['libtorrent_binding']}")
        print(f"libtorrent_native_version={result['libtorrent_native']}")
        print(
            "http_tracker_application=verified metadata=verified content=verified "
            "payload_sha1=verified"
        )
        print(
            f"tracker_requests={result['requests']} "
            f"tracker_events={','.join(result['events'])} "
            f"revision={result['revision']}"
        )
        return 0
    finally:
        shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ScenarioFailure as error:
        print(f"failure={error}", file=sys.stderr)
        raise SystemExit(1) from error
