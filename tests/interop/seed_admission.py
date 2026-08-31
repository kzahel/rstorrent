#!/usr/bin/env python3
"""Exercise durable seed goals and admission against pinned libtorrent."""

from __future__ import annotations

import gc
import hashlib
import json
import os
import shutil
import tempfile
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

import libtorrent as lt

from first_verified_piece import (
    ScenarioFailure,
    add_seed,
    create_session,
    wait_for_listener,
    write_deterministic_range,
)
from incoming_seeding import create_outbound_only_session
from torrent_byte_intake import Gateway, OWNER, TOKEN, build_binaries, verify_reference
from udp_tracker_magnet import OneShotUdpTracker


TORRENT_COUNT = 6
PAYLOAD_SIZE = 4 * 1024
PIECE_SIZE = 1024
DOWNLOAD_RATE = 4 * 1024
TIMEOUT_SECONDS = 90


@dataclass(frozen=True)
class Fixture:
    ordinal: int
    name: str
    seed_directory: Path
    payload_path: Path
    payload_hash: str
    torrent_source: bytes
    torrent_info: lt.torrent_info
    info_hash: str


def create_fixture(owned: Path, ordinal: int) -> Fixture:
    name = f"seed-admission-{ordinal}"
    fixture_root = owned / f"fixture-{ordinal}"
    seed_directory = fixture_root / "seed"
    payload_path = seed_directory / name / "payload.bin"
    payload_path.parent.mkdir(parents=True)
    payload_hash = write_deterministic_range(
        payload_path,
        ordinal * PAYLOAD_SIZE,
        PAYLOAD_SIZE,
    )
    files = lt.file_storage()
    files.add_file(f"{name}/payload.bin", PAYLOAD_SIZE)
    creator = lt.create_torrent(
        files,
        piece_size=PIECE_SIZE,
        flags=lt.create_torrent.v1_only,
    )
    lt.set_piece_hashes(creator, str(seed_directory))
    torrent_path = fixture_root / f"{name}.torrent"
    torrent_source = bytes(lt.bencode(creator.generate()))
    torrent_path.write_bytes(torrent_source)
    torrent_info = lt.torrent_info(str(torrent_path))
    info_hash = str(torrent_info.info_hashes().v1)
    if hashlib.sha1(bytes(torrent_info.info_section())).hexdigest() != info_hash:
        raise ScenarioFailure(f"fixture {ordinal} has an inconsistent v1 identity")
    return Fixture(
        ordinal=ordinal,
        name=name,
        seed_directory=seed_directory,
        payload_path=payload_path,
        payload_hash=payload_hash,
        torrent_source=torrent_source,
        torrent_info=torrent_info,
        info_hash=info_hash,
    )


def gateway_json(
    gateway: Gateway,
    method: str,
    path: str,
    payload: dict[str, object] | None = None,
    *,
    expected_status: int = 200,
) -> dict[str, Any]:
    encoded = None if payload is None else json.dumps(payload).encode()
    request = urllib.request.Request(
        f"http://127.0.0.1:{gateway.port}{path}",
        data=encoded,
        method=method,
        headers={
            "Authorization": f"Bearer {TOKEN}",
            "Content-Type": "application/json",
            "Origin": gateway.origin,
            "X-RSTorrent-Owner": OWNER,
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            status = response.status
            body = response.read()
    except urllib.error.HTTPError as error:
        raise ScenarioFailure(
            f"gateway {method} {path} failed with {error.code}: "
            f"{error.read().decode(errors='replace')}"
        ) from error
    if status != expected_status:
        raise ScenarioFailure(
            f"gateway {method} {path} returned {status}, expected {expected_status}"
        )
    if not body:
        return {}
    decoded = json.loads(body)
    if not isinstance(decoded, dict):
        raise ScenarioFailure("gateway response is not a JSON object")
    return decoded


def catalog(gateway: Gateway) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    opened = gateway_json(
        gateway,
        "POST",
        "/api/v1/view-sets",
        {
            "views": [
                {
                    "type": "torrent_list",
                    "view_id": "seed-admission",
                    "delivery": {"min_interval_millis": 0},
                }
            ],
            "options": {},
        },
        expected_status=201,
    )
    view_set_id = opened.get("view_set_id")
    if not isinstance(view_set_id, str):
        raise ScenarioFailure("seed-admission view lacks a view-set ID")
    try:
        initial = opened.get("initial")
        updates = initial.get("updates") if isinstance(initial, dict) else None
        if not isinstance(updates, list):
            raise ScenarioFailure("seed-admission view lacks initial updates")
        for update in updates:
            snapshot = update.get("snapshot") if isinstance(update, dict) else None
            if not isinstance(snapshot, dict) or snapshot.get("type") != "torrent_list":
                continue
            torrents = snapshot.get("torrents")
            settings = snapshot.get("client_settings")
            if isinstance(torrents, list) and isinstance(settings, dict):
                if not all(isinstance(row, dict) for row in torrents):
                    raise ScenarioFailure("torrent-list view contains a non-object row")
                return torrents, settings
        raise ScenarioFailure("seed-admission view lacks its torrent-list snapshot")
    finally:
        gateway_json(
            gateway,
            "DELETE",
            f"/api/v1/view-sets/{view_set_id}",
            expected_status=204,
        )


def wait_catalog(
    gateway: Gateway,
    predicate: Callable[[list[dict[str, Any]], dict[str, Any]], bool],
    label: str,
    timeout: float = TIMEOUT_SECONDS,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    deadline = time.monotonic() + timeout
    observed: tuple[list[dict[str, Any]], dict[str, Any]] | None = None
    while time.monotonic() < deadline:
        observed = catalog(gateway)
        if predicate(*observed):
            return observed
        time.sleep(0.05)
    raise ScenarioFailure(f"catalog did not reach {label}: {observed}")


def update_settings(gateway: Gateway, request_id: str, **patch: object) -> None:
    gateway.command(
        request_id,
        {"type": "update_client_settings", "patch": patch},
    )


def admissions(rows: list[dict[str, Any]]) -> dict[str, str]:
    return {
        row["torrent_id"]: row["seeding"]["admission"]
        for row in rows
        if row.get("state") == "complete"
    }


def goal_status(row: dict[str, Any]) -> str | None:
    seeding = row.get("seeding")
    goal = seeding.get("goal") if isinstance(seeding, dict) else None
    return goal.get("status") if isinstance(goal, dict) else None


def active_ids(rows: list[dict[str, Any]]) -> set[str]:
    return {
        torrent_id
        for torrent_id, admission in admissions(rows).items()
        if admission in ("active", "inactive_exempt")
    }


def row_by_id(rows: list[dict[str, Any]], torrent_id: str) -> dict[str, Any]:
    for row in rows:
        if row.get("torrent_id") == torrent_id:
            return row
    raise ScenarioFailure(f"torrent-list view omitted {torrent_id}")


def listener_port(settings: dict[str, Any]) -> int:
    listener = settings.get("listener_status")
    port = listener.get("port") if isinstance(listener, dict) else None
    if (
        not isinstance(listener, dict)
        or listener.get("type") != "listening"
        or listener.get("address") != "127.0.0.1"
        or not isinstance(port, int)
        or port == 0
    ):
        raise ScenarioFailure(f"seed listener is not active: {listener}")
    return port


def leech_from_rstorrent(
    fixture: Fixture,
    port: int,
    output: Path,
) -> dict[str, object]:
    session = create_outbound_only_session(1)
    handle: lt.torrent_handle | None = None
    diagnostics: list[str] = []
    try:
        output.mkdir(parents=True)
        parameters = lt.add_torrent_params()
        parameters.ti = fixture.torrent_info
        parameters.save_path = str(output)
        parameters.flags &= ~lt.torrent_flags.paused
        parameters.flags &= ~lt.torrent_flags.auto_managed
        handle = session.add_torrent(parameters)
        handle.connect_peer(("127.0.0.1", port))
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            diagnostics.extend(alert.message() for alert in session.pop_alerts())
            status = handle.status()
            if status.errc.value() != 0:
                raise ScenarioFailure(
                    f"libtorrent leecher failed: {status.errc.message()}"
                )
            if status.is_seeding:
                downloaded = output / fixture.name / "payload.bin"
                if not downloaded.is_file():
                    raise ScenarioFailure(f"libtorrent output is absent: {downloaded}")
                payload_hash = hashlib.sha1(downloaded.read_bytes()).hexdigest()
                if payload_hash != fixture.payload_hash:
                    raise ScenarioFailure(
                        f"libtorrent output hash differs for {fixture.name}"
                    )
                return {
                    "torrent": fixture.ordinal,
                    "payload_bytes": status.total_done,
                    "payload_sha1": payload_hash,
                    "peers": status.num_peers,
                }
            if status.num_peers == 0:
                handle.connect_peer(("127.0.0.1", port))
            time.sleep(0.02)
        raise ScenarioFailure(
            f"libtorrent did not complete {fixture.name}\n"
            + "\n".join(diagnostics[-30:])
        )
    finally:
        if handle is not None and handle.is_valid():
            session.remove_torrent(handle)
        session.pause()
        handle = None
        session = None
        gc.collect()


def close_oracle(
    session: lt.session | None,
    handles: list[lt.torrent_handle],
) -> None:
    if session is not None:
        for handle in handles:
            if handle.is_valid():
                session.remove_torrent(handle)
        session.pause()
    handles.clear()
    gc.collect()


def remove_all(gateway: Gateway, torrent_ids: list[str]) -> None:
    for ordinal, torrent_id in enumerate(torrent_ids):
        gateway.command(
            f"remove-{ordinal}",
            {
                "type": "remove_torrent",
                "torrent_id": torrent_id,
                "data": "delete_data",
            },
        )
    wait_catalog(gateway, lambda rows, _settings: not rows, "empty after removal")


def run() -> None:
    repository = Path(__file__).resolve().parents[2]
    owned = Path(tempfile.mkdtemp(prefix="rstorrent-seed-admission-"))
    profile = owned / "profile"
    storage = owned / "downloads"
    gateway: Gateway | None = None
    oracle: lt.session | None = None
    oracle_handles: list[lt.torrent_handle] = []
    trackers: list[OneShotUdpTracker] = []
    failure: BaseException | None = None
    cleanup_succeeded = False
    try:
        reference = verify_reference(repository)
        gateway_binary, _probe = build_binaries(repository)
        fixtures = [create_fixture(owned, ordinal) for ordinal in range(TORRENT_COUNT)]

        oracle = create_session()
        diagnostics: list[str] = []
        oracle_port = wait_for_listener(oracle, diagnostics)
        for fixture in fixtures:
            oracle_handles.append(
                add_seed(
                    oracle,
                    fixture.torrent_info,
                    fixture.seed_directory,
                    diagnostics,
                )
            )

        gateway = Gateway(gateway_binary, profile, storage, "loopback_only")
        update_settings(
            gateway,
            "seed-policy",
            listener={"type": "automatic_loopback"},
            active_downloads=6,
            active_seeds={"type": "limited", "torrents": 1},
            share_ratio_limit_percent=100,
            finished_download_ratio_limit_percent=2_147_483_647,
            finished_time_limit_seconds=2_147_483_647,
            download_rate_limit={
                "type": "limited",
                "bytes_per_second": DOWNLOAD_RATE,
            },
        )
        gateway.stop()
        gateway = Gateway(gateway_binary, profile, storage, "loopback_only")

        torrent_ids: list[str] = []
        tracker_evidence: list[dict[str, object]] = []
        for fixture in fixtures:
            tracker = OneShotUdpTracker(
                fixture.info_hash,
                oracle_port,
                seeders=2,
                leechers=10,
                expected_left=PAYLOAD_SIZE,
                expected_peer_id=None,
                expected_listen_port=None,
            )
            trackers.append(tracker)
            tracker.start()
            metainfo = lt.bdecode(fixture.torrent_source)
            metainfo[b"announce"] = (
                f"udp://127.0.0.1:{tracker.port}/announce".encode()
            )
            added = gateway.upload(
                f"add-{fixture.ordinal}",
                bytes(lt.bencode(metainfo)),
                start_content=True,
            )
            result = added.get("result")
            add_result = result.get("result") if isinstance(result, dict) else None
            torrent_id = (
                add_result.get("torrent_id")
                if isinstance(add_result, dict)
                else None
            )
            if not isinstance(torrent_id, str):
                raise ScenarioFailure(f"add response lacks torrent ID: {added}")
            torrent_ids.append(torrent_id)
            try:
                tracker.join()
            except ScenarioFailure as error:
                raise ScenarioFailure(
                    f"tracker failed for fixture {fixture.ordinal}: {error}; "
                    f"snapshot={gateway.snapshot(f'tracker-failed-{fixture.ordinal}')}"
                ) from error
            tracker_evidence.append(
                {
                    "torrent": fixture.ordinal,
                    "requests": tracker.requests,
                    "event": "started",
                    "complete": tracker.seeders,
                    "incomplete": tracker.leechers,
                }
            )

        rows, settings = wait_catalog(
            gateway,
            lambda rows, _settings: len(rows) == TORRENT_COUNT
            and all(
                row.get("state") == "complete"
                and goal_status(row) == "unmet"
                and row.get("configured_tracker_count") == 1
                and row.get("lifetime", {}).get("downloaded_payload_bytes")
                == str(PAYLOAD_SIZE)
                for row in rows
            ),
            "six complete goal-unmet torrents",
        )
        close_oracle(oracle, oracle_handles)
        oracle = None
        if len(active_ids(rows)) != 1 or list(admissions(rows).values()).count("queued") != 5:
            raise ScenarioFailure(f"active-seed limit one did not converge: {admissions(rows)}")
        if settings.get("active_seed_count") != 1:
            raise ScenarioFailure(f"runtime seed count differs at limit one: {settings}")
        port = listener_port(settings)

        crossings: list[dict[str, object]] = []
        active_limit = 1
        for crossing in range(TORRENT_COUNT):
            if crossing == 2:
                active_limit = 2
                update_settings(
                    gateway,
                    "active-seeds-two",
                    active_seeds={"type": "limited", "torrents": 2},
                )
                rows, settings = wait_catalog(
                    gateway,
                    lambda rows, settings: len(active_ids(rows)) == 2
                    and settings.get("active_seed_count") == 2,
                    "active-seed limit two",
                )
            candidates = [
                torrent_id
                for torrent_id in sorted(active_ids(rows))
                if goal_status(row_by_id(rows, torrent_id)) == "unmet"
            ]
            if not candidates:
                raise ScenarioFailure(
                    f"no active goal-unmet seed for crossing {crossing}: {admissions(rows)}"
                )
            torrent_id = candidates[0]
            fixture = fixtures[torrent_ids.index(torrent_id)]
            before_active = active_ids(rows)
            leech = leech_from_rstorrent(
                fixture,
                port,
                owned / f"libtorrent-leech-{crossing}",
            )
            gateway.snapshot(f"reconcile-goal-{crossing}")
            remaining_unmet = TORRENT_COUNT - crossing - 1
            rows, settings = wait_catalog(
                gateway,
                lambda rows, settings: goal_status(row_by_id(rows, torrent_id)) == "met"
                and row_by_id(rows, torrent_id)
                .get("lifetime", {})
                .get("uploaded_payload_bytes")
                == str(PAYLOAD_SIZE)
                and settings.get("active_seed_count") == active_limit,
                f"goal crossing {crossing}",
            )
            crossed = row_by_id(rows, torrent_id)
            goal = crossed["seeding"]["goal"]
            if (
                not goal.get("share_ratio_met")
                or crossed["lifetime"].get("share_ratio_hundredths") != "100"
                or leech.get("payload_bytes") != PAYLOAD_SIZE
            ):
                raise ScenarioFailure(
                    f"goal crossing {crossing} was not exact: row={crossed} leech={leech}"
                )
            after_active = active_ids(rows)
            if remaining_unmet >= active_limit:
                if torrent_id in after_active or after_active == before_active:
                    raise ScenarioFailure(
                        f"goal-met seed did not yield to unmet priority: "
                        f"before={before_active} after={after_active}"
                    )
            crossings.append(
                {
                    "torrent": fixture.ordinal,
                    "limit": active_limit,
                    "before_active": sorted(before_active),
                    "after_active": sorted(after_active),
                    "uploaded_payload_bytes": PAYLOAD_SIZE,
                    "share_ratio_hundredths": 100,
                    "payload_sha1": leech["payload_sha1"],
                }
            )

        rows, settings = wait_catalog(
            gateway,
            lambda rows, settings: all(goal_status(row) == "met" for row in rows)
            and len(active_ids(rows)) == 2
            and settings.get("active_seed_count") == 2,
            "all goals met without hard stop",
        )
        gateway.stop()
        gateway = Gateway(gateway_binary, profile, storage, "loopback_only")
        restarted_rows, restarted_settings = wait_catalog(
            gateway,
            lambda rows, settings: len(rows) == TORRENT_COUNT
            and all(
                row.get("state") == "complete"
                and goal_status(row) == "met"
                and row.get("lifetime", {}).get("uploaded_payload_bytes")
                == str(PAYLOAD_SIZE)
                and row.get("lifetime", {}).get("downloaded_payload_bytes")
                == str(PAYLOAD_SIZE)
                for row in rows
            )
            and len(active_ids(rows)) == 2
            and settings.get("active_seed_count") == 2,
            "durable restart",
        )
        restart_active = sorted(active_ids(restarted_rows))
        remove_all(gateway, torrent_ids)
        gateway.stop()
        gateway = None

        print(
            "seed_admission_milestone "
            + json.dumps(
                {
                    "oracle": reference,
                    "torrents": TORRENT_COUNT,
                    "payload_bytes_each": PAYLOAD_SIZE,
                    "initial_limit": 1,
                    "expanded_limit": 2,
                    "tracker": tracker_evidence,
                    "crossings": crossings,
                    "restart_active": restart_active,
                    "restart_counted": restarted_settings["active_seed_count"],
                    "goal_met_torrents": TORRENT_COUNT,
                    "terminal_torrents": 0,
                    "cleanup": "ok",
                },
                separators=(",", ":"),
            )
        )
    except BaseException as error:
        failure = error
    finally:
        for tracker in trackers:
            tracker.close()
        close_oracle(oracle, oracle_handles)
        if gateway is not None:
            try:
                gateway.stop()
            except BaseException as cleanup_error:
                failure = failure or cleanup_error
        try:
            shutil.rmtree(owned)
            cleanup_succeeded = not owned.exists()
        except BaseException as cleanup_error:
            failure = failure or cleanup_error
    if failure is not None:
        raise failure
    if not cleanup_succeeded:
        raise ScenarioFailure("seed-admission run directory survived cleanup")


if __name__ == "__main__":
    try:
        run()
    except ScenarioFailure as error:
        print(f"scenario_failed={error}", file=os.sys.stderr)
        raise SystemExit(1) from error
