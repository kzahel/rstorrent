#!/usr/bin/env python3
"""Run one isolated pinned-libtorrent leech against an explicit peer."""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

import libtorrent as lt


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--magnet")
    source.add_argument("--torrent", type=Path)
    parser.add_argument("--peer-host", required=True)
    parser.add_argument("--peer-port", required=True, type=int)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--timeout", type=float, default=30)
    arguments = parser.parse_args()

    session = lt.session(
        {
            "listen_interfaces": "127.0.0.1:0",
            "enable_dht": False,
            "enable_lsd": False,
            "enable_upnp": False,
            "enable_natpmp": False,
            "enable_incoming_utp": False,
            "enable_outgoing_utp": False,
            "enable_incoming_tcp": False,
            "enable_outgoing_tcp": True,
            "in_enc_policy": int(lt.enc_policy.pe_disabled),
            "out_enc_policy": int(lt.enc_policy.pe_disabled),
            "alert_queue_size": 1000,
        }
    )
    handle = None
    diagnostics: list[str] = []
    try:
        if arguments.magnet is not None:
            parameters = lt.parse_magnet_uri(arguments.magnet)
        else:
            parameters = lt.add_torrent_params()
            parameters.ti = lt.torrent_info(str(arguments.torrent))
        parameters.save_path = str(arguments.output)
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        handle = session.add_torrent(parameters)
        handle.connect_peer((arguments.peer_host, arguments.peer_port))
        deadline = time.monotonic() + arguments.timeout
        while time.monotonic() < deadline:
            diagnostics.extend(alert.message() for alert in session.pop_alerts())
            status = handle.status()
            if status.errc.value() != 0:
                raise RuntimeError(status.errc.message())
            if status.is_seeding:
                print(
                    json.dumps(
                        {
                            "payload_download": int(status.total_payload_download),
                            "progress": float(status.progress),
                            "alerts": diagnostics[-20:],
                        },
                        sort_keys=True,
                    )
                )
                return 0
            time.sleep(0.02)
        raise RuntimeError(
            "controlled leecher timed out\n" + "\n".join(diagnostics[-40:])
        )
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()


if __name__ == "__main__":
    raise SystemExit(main())
