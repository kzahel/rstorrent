#!/usr/bin/env python3
"""Pure manifest, journal, aggregation, and privacy contracts for Tactical 142."""

from __future__ import annotations

import ipaddress
import json
import math
import os
import re
import statistics
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable, Iterator


SCHEMA_VERSION = 1
MIB = 1024 * 1024
GIB = 1024 * MIB
SIZES_MIB = (8, 64, 256, 1024)
PIECE_BYTES = 256 * 1024
DIRECTIONS = ("local-seed", "remote-seed")
IMPLEMENTATIONS = ("rstorrent", "libtorrent")
TRANSPORTS = ("tcp", "utp")
MAX_REPETITIONS = 3
MAX_RESULT_BYTES = MIB
MAX_JOURNAL_BYTES = 128 * MIB
MAX_EPOCH_LENGTH = 64
EPOCH_PATTERN = re.compile(r"[a-z0-9][a-z0-9-]{0,63}")
CASE_ID_PATTERN = re.compile(r"[a-z0-9][a-z0-9-]{0,255}")
SSH_ALIAS_PATTERN = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}")
IPV4_TEXT = re.compile(r"(?<![0-9])(?:[0-9]{1,3}\.){3}[0-9]{1,3}(?![0-9])")
IPV6_TEXT = re.compile(r"(?<![0-9A-Fa-f:])(?:[0-9A-Fa-f]{0,4}:){2,}[0-9A-Fa-f:]{0,4}(?![0-9A-Fa-f:])")
PATH_MARKERS = ("/tmp/", "/private/", "/Users/", "/home/", "\\Users\\")
URL_MARKERS = ("http://", "https://", "file://", "ssh://")


class MatrixContractError(RuntimeError):
    pass


@dataclass(frozen=True, order=True)
class CaseKey:
    epoch: str
    repetition: int
    size_mib: int
    direction: str
    seed: str
    leech: str
    transport: str
    order: int

    def validate(self) -> None:
        if not EPOCH_PATTERN.fullmatch(self.epoch):
            raise MatrixContractError("matrix epoch is malformed")
        if not 1 <= self.repetition <= MAX_REPETITIONS:
            raise MatrixContractError("matrix repetition is outside its bound")
        if self.size_mib not in SIZES_MIB:
            raise MatrixContractError("matrix size is outside the fixed set")
        if self.direction not in DIRECTIONS:
            raise MatrixContractError("matrix direction is unknown")
        if self.seed not in IMPLEMENTATIONS or self.leech not in IMPLEMENTATIONS:
            raise MatrixContractError("matrix implementation is unknown")
        if self.transport not in TRANSPORTS:
            raise MatrixContractError("matrix transport is unknown")
        if not 1 <= self.order <= len(SIZES_MIB) * 16 * MAX_REPETITIONS:
            raise MatrixContractError("matrix order is outside its bound")

    @property
    def payload_bytes(self) -> int:
        return self.size_mib * MIB

    @property
    def piece_count(self) -> int:
        return math.ceil(self.payload_bytes / PIECE_BYTES)

    @property
    def timeout_seconds(self) -> int:
        return {
            8: 15 * 60,
            64: 45 * 60,
            256: 3 * 60 * 60,
            1024: 12 * 60 * 60,
        }[self.size_mib]

    @property
    def case_id(self) -> str:
        value = (
            f"{self.epoch}-r{self.repetition:02d}-o{self.order:03d}-"
            f"{self.size_mib}m-{self.direction}-{self.seed}-{self.leech}-"
            f"{self.transport}"
        )
        if not CASE_ID_PATTERN.fullmatch(value):
            raise MatrixContractError("generated case identity is malformed")
        return value

    def public_dict(self) -> dict[str, Any]:
        result = asdict(self)
        result.update(
            {
                "case_id": self.case_id,
                "payload_bytes": self.payload_bytes,
                "piece_bytes": PIECE_BYTES,
                "piece_count": self.piece_count,
                "timeout_seconds": self.timeout_seconds,
            }
        )
        return result


def _rotate(values: tuple[str, ...], offset: int) -> tuple[str, ...]:
    offset %= len(values)
    return values[offset:] + values[:offset]


def manifest(epoch: str, repetitions: int = 1) -> list[CaseKey]:
    if not EPOCH_PATTERN.fullmatch(epoch):
        raise MatrixContractError("matrix epoch is malformed")
    if not 1 <= repetitions <= MAX_REPETITIONS:
        raise MatrixContractError("matrix repetitions are outside their bound")
    cases: list[CaseKey] = []
    order = 0
    pairings = tuple(
        (seed, leech) for seed in IMPLEMENTATIONS for leech in IMPLEMENTATIONS
    )
    for repetition in range(1, repetitions + 1):
        sizes = SIZES_MIB if repetition % 2 else tuple(reversed(SIZES_MIB))
        for size_index, size_mib in enumerate(sizes):
            directions = _rotate(DIRECTIONS, repetition + size_index - 1)
            rotated_pairings = pairings[(repetition + size_index - 1) % len(pairings) :] + pairings[
                : (repetition + size_index - 1) % len(pairings)
            ]
            transports = _rotate(TRANSPORTS, repetition + size_index - 1)
            for direction in directions:
                for seed, leech in rotated_pairings:
                    for transport in transports:
                        order += 1
                        key = CaseKey(
                            epoch=epoch,
                            repetition=repetition,
                            size_mib=size_mib,
                            direction=direction,
                            seed=seed,
                            leech=leech,
                            transport=transport,
                            order=order,
                        )
                        key.validate()
                        cases.append(key)
    expected = repetitions * len(SIZES_MIB) * 16
    if len(cases) != expected or len({case.case_id for case in cases}) != expected:
        raise MatrixContractError("matrix manifest cardinality is not exact")
    return cases


def select_cases(
    cases: Iterable[CaseKey],
    *,
    sizes_mib: Iterable[int] | None = None,
    directions: Iterable[str] | None = None,
    seeds: Iterable[str] | None = None,
    leeches: Iterable[str] | None = None,
    transports: Iterable[str] | None = None,
    case_ids: Iterable[str] | None = None,
) -> list[CaseKey]:
    filters = {
        "size_mib": set(sizes_mib or SIZES_MIB),
        "direction": set(directions or DIRECTIONS),
        "seed": set(seeds or IMPLEMENTATIONS),
        "leech": set(leeches or IMPLEMENTATIONS),
        "transport": set(transports or TRANSPORTS),
    }
    wanted_ids = set(case_ids or ())
    selected = [
        case
        for case in cases
        if case.size_mib in filters["size_mib"]
        and case.direction in filters["direction"]
        and case.seed in filters["seed"]
        and case.leech in filters["leech"]
        and case.transport in filters["transport"]
        and (not wanted_ids or case.case_id in wanted_ids)
    ]
    if wanted_ids - {case.case_id for case in selected}:
        raise MatrixContractError("case selection contains an unknown identity")
    return selected


def validate_terminal_record(record: dict[str, Any]) -> None:
    if record.get("schema_version") != SCHEMA_VERSION:
        raise MatrixContractError("journal record has an unknown schema")
    if record.get("event") != "case-terminal":
        raise MatrixContractError("journal record has an unknown event")
    case = record.get("case")
    if not isinstance(case, dict) or not isinstance(case.get("case_id"), str):
        raise MatrixContractError("journal record omitted its case identity")
    if not CASE_ID_PATTERN.fullmatch(case["case_id"]):
        raise MatrixContractError("journal case identity is malformed")
    status = record.get("status")
    if status not in {"complete", "failed", "invalid"}:
        raise MatrixContractError("journal record has an unknown status")
    cleanup = record.get("cleanup")
    if not isinstance(cleanup, dict) or cleanup.get("succeeded") is not True:
        raise MatrixContractError("journal record lacks exact successful cleanup")
    if status == "complete":
        result = record.get("result")
        if not isinstance(result, dict):
            raise MatrixContractError("complete journal record omitted its result")
        timing = result.get("timing")
        if not isinstance(timing, dict):
            raise MatrixContractError("complete journal record omitted timing")
        for name in ("connect_to_complete_seconds", "active_payload_seconds"):
            value = timing.get(name)
            if isinstance(value, bool) or not isinstance(value, (int, float)) or value <= 0:
                raise MatrixContractError(f"complete journal timing {name} is invalid")
        integrity = result.get("integrity")
        if not isinstance(integrity, dict) or integrity.get("verified") is not True:
            raise MatrixContractError("complete journal result lacks exact integrity")


class Journal:
    """Single-writer JSONL journal with fsync and final-line crash repair."""

    def __init__(self, path: Path) -> None:
        self.path = path

    def _bounded_bytes(self) -> bytes:
        if not self.path.exists():
            return b""
        size = self.path.stat().st_size
        if size > MAX_JOURNAL_BYTES:
            raise MatrixContractError("matrix journal exceeds its byte bound")
        return self.path.read_bytes()

    def load(self, *, repair_truncated_tail: bool = False) -> list[dict[str, Any]]:
        payload = self._bounded_bytes()
        if not payload:
            return []
        records: list[dict[str, Any]] = []
        valid_bytes = 0
        lines = payload.splitlines(keepends=True)
        for index, encoded in enumerate(lines):
            terminal_line = index == len(lines) - 1
            if len(encoded) > MAX_RESULT_BYTES:
                raise MatrixContractError("matrix journal record exceeds its byte bound")
            if not encoded.endswith(b"\n"):
                if terminal_line and repair_truncated_tail:
                    break
                raise MatrixContractError("matrix journal has a truncated terminal record")
            try:
                decoded = json.loads(encoded)
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                if terminal_line and repair_truncated_tail:
                    break
                raise MatrixContractError("matrix journal contains invalid JSON") from error
            if not isinstance(decoded, dict):
                raise MatrixContractError("matrix journal record is not an object")
            validate_terminal_record(decoded)
            records.append(decoded)
            valid_bytes += len(encoded)
        if repair_truncated_tail and valid_bytes != len(payload):
            with self.path.open("r+b") as journal:
                journal.truncate(valid_bytes)
                journal.flush()
                os.fsync(journal.fileno())
        identities = [record["case"]["case_id"] for record in records]
        if len(identities) != len(set(identities)):
            raise MatrixContractError("matrix journal contains duplicate case identities")
        return records

    def append(self, record: dict[str, Any]) -> None:
        validate_terminal_record(record)
        existing = self.load(repair_truncated_tail=True)
        case_id = record["case"]["case_id"]
        if case_id in {item["case"]["case_id"] for item in existing}:
            raise MatrixContractError("matrix journal already contains this case")
        encoded = json.dumps(record, sort_keys=True, separators=(",", ":")).encode() + b"\n"
        if len(encoded) > MAX_RESULT_BYTES:
            raise MatrixContractError("matrix journal record exceeds its byte bound")
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with self.path.open("ab", buffering=0) as journal:
            journal.write(encoded)
            os.fsync(journal.fileno())

    def completed_case_ids(self) -> set[str]:
        return {
            record["case"]["case_id"]
            for record in self.load(repair_truncated_tail=True)
        }


def pending_cases(cases: Iterable[CaseKey], journal: Journal) -> list[CaseKey]:
    completed = journal.completed_case_ids()
    return [case for case in cases if case.case_id not in completed]


def _distribution(values: list[float]) -> dict[str, float | int | None]:
    if not values:
        return {"count": 0, "min": None, "median": None, "max": None}
    return {
        "count": len(values),
        "min": round(min(values), 6),
        "median": round(statistics.median(values), 6),
        "max": round(max(values), 6),
    }


def summarize(records: Iterable[dict[str, Any]]) -> dict[str, Any]:
    records = list(records)
    complete = [record for record in records if record.get("status") == "complete"]
    grouped: dict[tuple[Any, ...], list[float]] = {}
    for record in complete:
        case = record["case"]
        timing = record["result"]["timing"]
        rate = float(case["payload_bytes"]) / MIB / float(timing["active_payload_seconds"])
        key = (
            case["size_mib"],
            case["direction"],
            case["seed"],
            case["leech"],
            case["transport"],
        )
        grouped.setdefault(key, []).append(rate)
    cells = []
    for key, values in sorted(grouped.items()):
        size_mib, direction, seed, leech, transport = key
        cells.append(
            {
                "size_mib": size_mib,
                "direction": direction,
                "seed": seed,
                "leech": leech,
                "transport": transport,
                "active_mib_per_second": _distribution(values),
                "stable": len(values) >= 3,
            }
        )
    return {
        "schema_version": SCHEMA_VERSION,
        "records": len(records),
        "complete": len(complete),
        "failed": sum(record.get("status") == "failed" for record in records),
        "invalid": sum(record.get("status") == "invalid" for record in records),
        "cells": cells,
    }


def validate_ssh_alias(value: str) -> str:
    if value.startswith("-") or not SSH_ALIAS_PATTERN.fullmatch(value):
        raise MatrixContractError("SSH host alias is malformed")
    return value


def _looks_like_ip(value: str) -> bool:
    for candidate in IPV4_TEXT.findall(value):
        try:
            ipaddress.ip_address(candidate)
        except ValueError:
            continue
        return True
    for candidate in IPV6_TEXT.findall(value):
        try:
            ipaddress.ip_address(candidate)
        except ValueError:
            continue
        return True
    return False


def assert_redacted(value: Any, *, forbidden_strings: Iterable[str] = ()) -> None:
    forbidden = tuple(item for item in forbidden_strings if item)

    def inspect(item: Any, field: str | None = None) -> None:
        if isinstance(item, dict):
            for key, child in item.items():
                inspect(child, str(key))
            return
        if isinstance(item, list):
            for child in item:
                inspect(child, field)
            return
        if not isinstance(item, str):
            return
        if any(secret in item for secret in forbidden):
            raise MatrixContractError("matrix result retained a forbidden identity")
        if field in {"libtorrent_version", "rust_version", "kernel_version"}:
            return
        if _looks_like_ip(item):
            raise MatrixContractError("matrix result retained a network address")
        if any(marker in item for marker in PATH_MARKERS + URL_MARKERS):
            raise MatrixContractError("matrix result retained a path or URL")

    inspect(value)


def iter_matrix_rows(cases: Iterable[CaseKey]) -> Iterator[dict[str, Any]]:
    for case in cases:
        yield case.public_dict()
