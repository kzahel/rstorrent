#!/usr/bin/env python3
"""Application-owner identity helpers shared by interop evidence."""

from __future__ import annotations

from typing import Any

HAVE_STATE_HEADER_LENGTH = 8 + 2 + 16 + 32 + 4


def canonical_torrent_id(value: object) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 35
        or not value.startswith("t1-")
        or any(character not in "0123456789abcdef" for character in value[3:])
    ):
        raise ValueError(f"invalid canonical torrent ID: {value!r}")
    return value


def torrent_id_from_add(response: dict[str, Any]) -> str:
    result = response.get("result")
    add = (
        result.get("result")
        if isinstance(result, dict) and result.get("type") == "add_torrent"
        else None
    )
    torrent_id = add.get("torrent_id") if isinstance(add, dict) else None
    try:
        return canonical_torrent_id(torrent_id)
    except ValueError as error:
        raise ValueError(
            f"add response lacks a canonical torrent ID: {response}"
        ) from error


def torrent_id_blob(torrent_id: str) -> bytes:
    return bytes.fromhex(canonical_torrent_id(torrent_id)[3:])
