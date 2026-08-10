#!/usr/bin/env python3
"""Pure contracts for the paired public-download performance harness."""

from __future__ import annotations

import hashlib
import json
import math
import os
import re
import stat
from dataclasses import dataclass
from pathlib import Path
from typing import Any, BinaryIO, Iterable
from urllib.parse import urlsplit


CATALOG_SCHEMA_VERSION = 2
REPORT_SCHEMA_VERSION = 2
MAX_METAINFO_BYTES = 64 * 1024 * 1024
MAX_PAYLOAD_BYTES = 16 * 1024 * 1024 * 1024
MAX_PAIRS = 20
MAX_OWNER_TIMEOUT_SECONDS = 4 * 60 * 60
MAX_NETWORK_BYTES = 64 * 1024 * 1024 * 1024
MAX_REPORT_BYTES = 32 * 1024 * 1024
MAX_DIAGNOSTIC_BYTES = 256 * 1024
VERIFY_BUFFER_BYTES = 1024 * 1024
MIN_FREE_SPACE_BYTES = 2 * 1024 * 1024 * 1024
CANONICAL_PROFILES = (
    "matched-plain-30",
    "matched-rc4-30",
    "product-default",
    "dht-only",
)
PROFILE_ALIASES = {
    "common": "matched-plain-30",
    "full-reference": "product-default",
    "dht": "dht-only",
}
INPUT_MODES = ("metainfo", "magnet")
CATALOG_ROLES = (
    "small-primary",
    "medium-distro",
    "large-distro",
    "dht-only",
    "tracker-breadth",
    "small-breadth",
)
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


class ContractError(ValueError):
    pass


@dataclass(frozen=True)
class MetainfoFile:
    relative_parts: tuple[str, ...]
    length: int
    padding: bool

    @property
    def relative_path(self) -> Path:
        return Path(*self.relative_parts)


@dataclass(frozen=True)
class MetainfoDescriptor:
    outer_sha256: str
    info_hash: str
    name: str
    piece_length: int
    piece_hashes: tuple[bytes, ...]
    files: tuple[MetainfoFile, ...]
    private: bool
    tracker_tiers: tuple[tuple[str, ...], ...]
    web_seeds: tuple[str, ...]
    raw_info: bytes

    @property
    def payload_bytes(self) -> int:
        return sum(file.length for file in self.files)

    @property
    def padding_bytes(self) -> int:
        return sum(file.length for file in self.files if file.padding)

    @property
    def piece_count(self) -> int:
        return len(self.piece_hashes)

    @property
    def file_count(self) -> int:
        return len(self.files)

    def normalized_geometry(self) -> dict[str, Any]:
        return {
            "payload_bytes": self.payload_bytes,
            "piece_length": self.piece_length,
            "piece_count": self.piece_count,
            "file_count": self.file_count,
            "private": self.private,
            "padding_bytes": self.padding_bytes,
            "tracker_tiers": [list(tier) for tier in self.tracker_tiers],
            "web_seeds": list(self.web_seeds),
        }


class _BencodeDecoder:
    def __init__(self, payload: bytes) -> None:
        self.payload = payload
        self.offset = 0
        self.nodes = 0
        self.info_span: tuple[int, int] | None = None

    def decode(self) -> Any:
        value = self._parse(0, top_level=True)
        if self.offset != len(self.payload):
            raise ContractError("metainfo has trailing bencode bytes")
        return value

    def _parse(self, depth: int, *, top_level: bool = False) -> Any:
        if depth > 64:
            raise ContractError("metainfo bencode nesting exceeds 64")
        self.nodes += 1
        if self.nodes > 2_500_000:
            raise ContractError("metainfo bencode node count exceeds limit")
        if self.offset >= len(self.payload):
            raise ContractError("truncated bencode value")
        marker = self.payload[self.offset]
        if marker == ord("i"):
            return self._integer()
        if marker == ord("l"):
            return self._list(depth)
        if marker == ord("d"):
            return self._dictionary(depth, top_level=top_level)
        if ord("0") <= marker <= ord("9"):
            return self._bytes()
        raise ContractError(f"invalid bencode marker at byte {self.offset}")

    def _integer(self) -> int:
        start = self.offset
        end = self.payload.find(b"e", start + 1)
        if end < 0:
            raise ContractError("unterminated bencode integer")
        encoded = self.payload[start + 1 : end]
        if not encoded or encoded == b"-0" or encoded.startswith(b"+"):
            raise ContractError("invalid bencode integer")
        digits = encoded[1:] if encoded.startswith(b"-") else encoded
        if not digits.isdigit() or (len(digits) > 1 and digits.startswith(b"0")):
            raise ContractError("noncanonical bencode integer")
        self.offset = end + 1
        return int(encoded)

    def _bytes(self) -> bytes:
        colon = self.payload.find(b":", self.offset)
        if colon < 0:
            raise ContractError("unterminated bencode byte length")
        encoded_length = self.payload[self.offset : colon]
        if (
            not encoded_length
            or not encoded_length.isdigit()
            or (len(encoded_length) > 1 and encoded_length.startswith(b"0"))
        ):
            raise ContractError("noncanonical bencode byte length")
        length = int(encoded_length)
        start = colon + 1
        end = start + length
        if end > len(self.payload):
            raise ContractError("truncated bencode byte string")
        self.offset = end
        return self.payload[start:end]

    def _list(self, depth: int) -> list[Any]:
        self.offset += 1
        values: list[Any] = []
        while True:
            if self.offset >= len(self.payload):
                raise ContractError("unterminated bencode list")
            if self.payload[self.offset] == ord("e"):
                self.offset += 1
                return values
            values.append(self._parse(depth + 1))

    def _dictionary(self, depth: int, *, top_level: bool) -> dict[bytes, Any]:
        self.offset += 1
        values: dict[bytes, Any] = {}
        previous: bytes | None = None
        while True:
            if self.offset >= len(self.payload):
                raise ContractError("unterminated bencode dictionary")
            if self.payload[self.offset] == ord("e"):
                self.offset += 1
                return values
            key = self._bytes()
            if previous is not None and key <= previous:
                raise ContractError("bencode dictionary keys are duplicate or unsorted")
            previous = key
            value_start = self.offset
            value = self._parse(depth + 1)
            if top_level and key == b"info":
                self.info_span = (value_start, self.offset)
            values[key] = value


def parse_metainfo(payload: bytes) -> MetainfoDescriptor:
    if not payload or len(payload) > MAX_METAINFO_BYTES:
        raise ContractError(
            f"metainfo size must be between 1 and {MAX_METAINFO_BYTES} bytes"
        )
    decoder = _BencodeDecoder(payload)
    root = decoder.decode()
    if not isinstance(root, dict) or decoder.info_span is None:
        raise ContractError("metainfo root must be a dictionary containing info")
    info = root.get(b"info")
    if not isinstance(info, dict):
        raise ContractError("metainfo info value must be a dictionary")
    raw_info = payload[slice(*decoder.info_span)]
    name = _path_component(info.get(b"name"), "info.name")
    piece_length = _positive_integer(info.get(b"piece length"), "info.piece length")
    if piece_length > 512 * 1024 * 1024:
        raise ContractError("metainfo piece length exceeds the supported v1 limit")
    pieces = info.get(b"pieces")
    if not isinstance(pieces, bytes) or not pieces or len(pieces) % 20 != 0:
        raise ContractError("info.pieces must contain a nonempty sequence of SHA-1 values")
    piece_hashes = tuple(pieces[index : index + 20] for index in range(0, len(pieces), 20))
    has_single = b"length" in info
    has_multi = b"files" in info
    if has_single == has_multi:
        raise ContractError("v1 metainfo must contain exactly one of length or files")
    files: list[MetainfoFile] = []
    if has_single:
        length = _nonnegative_integer(info[b"length"], "info.length")
        files.append(MetainfoFile((name,), length, _is_padding(info)))
    else:
        encoded_files = info[b"files"]
        if not isinstance(encoded_files, list) or not encoded_files:
            raise ContractError("info.files must be a nonempty list")
        for index, encoded_file in enumerate(encoded_files):
            if not isinstance(encoded_file, dict):
                raise ContractError(f"info.files[{index}] must be a dictionary")
            length = _nonnegative_integer(
                encoded_file.get(b"length"), f"info.files[{index}].length"
            )
            encoded_path = encoded_file.get(b"path")
            if not isinstance(encoded_path, list) or not encoded_path:
                raise ContractError(f"info.files[{index}].path must be nonempty")
            components = tuple(
                _path_component(component, f"info.files[{index}].path")
                for component in encoded_path
            )
            files.append(
                MetainfoFile((name, *components), length, _is_padding(encoded_file))
            )
    payload_bytes = sum(file.length for file in files)
    if not 0 < payload_bytes <= MAX_PAYLOAD_BYTES:
        raise ContractError(
            f"metainfo payload must be between 1 and {MAX_PAYLOAD_BYTES} bytes"
        )
    expected_pieces = math.ceil(payload_bytes / piece_length)
    if len(piece_hashes) != expected_pieces:
        raise ContractError(
            f"metainfo has {len(piece_hashes)} piece hashes but geometry requires "
            f"{expected_pieces}"
        )
    private_value = info.get(b"private", 0)
    if not isinstance(private_value, int) or private_value not in (0, 1):
        raise ContractError("info.private must be absent, 0, or 1")
    return MetainfoDescriptor(
        outer_sha256=hashlib.sha256(payload).hexdigest(),
        info_hash=hashlib.sha1(raw_info).hexdigest(),
        name=name,
        piece_length=piece_length,
        piece_hashes=piece_hashes,
        files=tuple(files),
        private=private_value == 1,
        tracker_tiers=_tracker_tiers(root),
        web_seeds=_web_seeds(root),
        raw_info=raw_info,
    )


def _positive_integer(value: Any, label: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise ContractError(f"{label} must be a positive integer")
    return value


def _nonnegative_integer(value: Any, label: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < 0:
        raise ContractError(f"{label} must be a nonnegative integer")
    return value


def _path_component(value: Any, label: str) -> str:
    if not isinstance(value, bytes):
        raise ContractError(f"{label} must contain byte strings")
    try:
        decoded = value.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ContractError(f"{label} must be valid UTF-8 for this harness") from error
    if (
        not decoded
        or decoded in (".", "..")
        or "/" in decoded
        or "\\" in decoded
        or "\x00" in decoded
        or Path(decoded).is_absolute()
    ):
        raise ContractError(f"{label} contains an unsafe path component")
    return decoded


def _is_padding(value: dict[bytes, Any]) -> bool:
    attribute = value.get(b"attr", b"")
    return isinstance(attribute, bytes) and b"p" in attribute


def _decode_url(value: Any, label: str) -> str:
    if not isinstance(value, bytes):
        raise ContractError(f"{label} must be a byte string")
    try:
        decoded = value.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ContractError(f"{label} is not valid UTF-8") from error
    if not urlsplit(decoded).scheme:
        raise ContractError(f"{label} is not an absolute URL")
    return decoded


def _tracker_tiers(root: dict[bytes, Any]) -> tuple[tuple[str, ...], ...]:
    tiers: list[tuple[str, ...]] = []
    encoded_tiers = root.get(b"announce-list")
    if encoded_tiers is not None:
        if not isinstance(encoded_tiers, list):
            raise ContractError("announce-list must be a list")
        for tier_index, encoded_tier in enumerate(encoded_tiers):
            if not isinstance(encoded_tier, list) or not encoded_tier:
                raise ContractError(f"announce-list tier {tier_index} must be nonempty")
            tier = tuple(
                _decode_url(value, f"announce-list[{tier_index}]")
                for value in encoded_tier
            )
            tiers.append(tuple(dict.fromkeys(tier)))
    announce = root.get(b"announce")
    if announce is not None:
        primary = _decode_url(announce, "announce")
        if not tiers:
            tiers.append((primary,))
        elif all(primary not in tier for tier in tiers):
            tiers.insert(0, (primary,))
    return tuple(tiers)


def _web_seeds(root: dict[bytes, Any]) -> tuple[str, ...]:
    encoded = root.get(b"url-list", [])
    if isinstance(encoded, bytes):
        values = [encoded]
    elif isinstance(encoded, list):
        values = encoded
    else:
        raise ContractError("url-list must be a byte string or list")
    return tuple(dict.fromkeys(_decode_url(value, "url-list") for value in values))


def verify_payload(descriptor: MetainfoDescriptor, output_root: Path) -> dict[str, Any]:
    root = output_root.resolve()
    if not root.is_dir():
        raise ContractError(f"publication root is missing: {root}")
    expected_files = {
        file.relative_path for file in descriptor.files if not file.padding
    }
    expected_directories = {
        parent
        for path in expected_files
        for parent in path.parents
        if parent != Path(".")
    }
    observed_files: set[Path] = set()
    observed_directories: set[Path] = set()
    for current, directories, files in os.walk(root, topdown=True, followlinks=False):
        current_path = Path(current)
        relative_current = current_path.relative_to(root)
        for directory in list(directories):
            path = current_path / directory
            mode = path.lstat().st_mode
            if stat.S_ISLNK(mode) or not stat.S_ISDIR(mode):
                raise ContractError(f"publication contains unsafe directory {path}")
            relative = relative_current / directory
            observed_directories.add(relative)
        for filename in files:
            path = current_path / filename
            mode = path.lstat().st_mode
            if stat.S_ISLNK(mode) or not stat.S_ISREG(mode):
                raise ContractError(f"publication contains non-regular file {path}")
            observed_files.add(relative_current / filename)
    if observed_files != expected_files:
        missing = sorted(str(path) for path in expected_files - observed_files)
        extra = sorted(str(path) for path in observed_files - expected_files)
        raise ContractError(f"publication file set mismatch; missing={missing} extra={extra}")
    if not observed_directories.issubset(expected_directories):
        extra = sorted(str(path) for path in observed_directories - expected_directories)
        raise ContractError(f"publication contains unexpected directories: {extra}")

    piece_index = 0
    piece_remaining = min(descriptor.piece_length, descriptor.payload_bytes)
    digest = hashlib.sha1()
    logical_offset = 0
    bytes_read = 0
    zeroes = bytes(VERIFY_BUFFER_BYTES)
    open_input: BinaryIO | None = None
    try:
        for file in descriptor.files:
            file_remaining = file.length
            if not file.padding:
                path = root / file.relative_path
                actual_length = path.stat().st_size
                if actual_length != file.length:
                    raise ContractError(
                        f"publication length mismatch for {file.relative_path}: "
                        f"expected {file.length}, got {actual_length}"
                    )
                open_input = path.open("rb")
            try:
                while file_remaining:
                    take = min(file_remaining, piece_remaining, VERIFY_BUFFER_BYTES)
                    if file.padding:
                        chunk = zeroes[:take]
                    else:
                        assert open_input is not None
                        chunk = open_input.read(take)
                        if len(chunk) != take:
                            raise ContractError(
                                f"short read while verifying {file.relative_path}"
                            )
                        bytes_read += take
                    digest.update(chunk)
                    file_remaining -= take
                    piece_remaining -= take
                    logical_offset += take
                    if piece_remaining == 0:
                        if digest.digest() != descriptor.piece_hashes[piece_index]:
                            raise ContractError(f"piece {piece_index} SHA-1 mismatch")
                        piece_index += 1
                        digest = hashlib.sha1()
                        remaining_payload = descriptor.payload_bytes - logical_offset
                        piece_remaining = min(descriptor.piece_length, remaining_payload)
            finally:
                if open_input is not None:
                    open_input.close()
                    open_input = None
    finally:
        if open_input is not None:
            open_input.close()
    if piece_index != descriptor.piece_count or logical_offset != descriptor.payload_bytes:
        raise ContractError("publication verification ended at inconsistent geometry")
    return {
        "verified": True,
        "piece_count": piece_index,
        "logical_bytes": logical_offset,
        "physical_bytes_read": bytes_read,
        "buffer_limit_bytes": VERIFY_BUFFER_BYTES,
    }


def normalize_profile(profile: str) -> str:
    canonical = PROFILE_ALIASES.get(profile, profile)
    if canonical not in CANONICAL_PROFILES:
        raise ContractError(f"unknown comparison profile {profile!r}")
    return canonical


def comparison_profile(profile: str) -> dict[str, Any]:
    profile = normalize_profile(profile)
    matched = profile in ("matched-plain-30", "matched-rc4-30")
    dht_only = profile == "dht-only"
    product = profile == "product-default"
    encryption = "required-rc4" if profile == "matched-rc4-30" else (
        "disabled" if matched else "allow"
    )
    rstorrent = {
        "network_policy": "online",
        "address_families": ["ipv4", "ipv6"],
        "tracker": not dht_only,
        "dht": dht_only or product,
        "pex": product or dht_only,
        "lsd": False,
        "upnp": False,
        "natpmp": False,
        "web_seed": False,
        "incoming_connections": False,
        "outgoing_tcp": True,
        "outgoing_utp": False,
        "session_connection_limit": 30 if matched else 200,
        "torrent_connection_limit": 30,
        "pending_dial_limit": 30,
        "connection_attempts_per_second": 30,
        "peer_connect_timeout_seconds": 15,
        "request_timeout_seconds": 60,
        "request_queue_time_seconds": 3,
        "max_outgoing_request_queue": 500,
        "download_rate_limit_bytes_per_second": 0,
        "upload_rate_limit_bytes_per_second": 0,
        "upload_slots": 8,
        "encryption": encryption,
    }
    libtorrent = {
        "listen_interfaces": "0.0.0.0:0,[::]:0",
        "enable_dht": dht_only or product,
        "enable_lsd": False,
        "enable_upnp": False,
        "enable_natpmp": False,
        "enable_incoming_utp": False,
        "enable_incoming_tcp": False,
        "enable_outgoing_utp": product or dht_only,
        "enable_outgoing_tcp": True,
        "connections_limit": 30 if matched else 200,
        "connection_speed": 30,
        "peer_connect_timeout": 15,
        "request_timeout": 60,
        "request_queue_time": 3,
        "max_out_request_queue": 500,
        "download_rate_limit": 0,
        "upload_rate_limit": 0,
        "unchoke_slots_limit": 8,
        "out_enc_policy": "forced" if profile == "matched-rc4-30" else (
            "disabled" if matched else "enabled"
        ),
        "in_enc_policy": "forced" if profile == "matched-rc4-30" else (
            "disabled" if matched else "enabled"
        ),
        "allowed_enc_level": "rc4" if profile == "matched-rc4-30" else "both",
        "prefer_rc4": profile == "matched-rc4-30",
        "pex": product or dht_only,
        "web_seed": product,
        "tracker": not dht_only,
    }
    result = {
        "name": profile,
        "semantic_version": 1,
        "rstorrent": rstorrent,
        "libtorrent": libtorrent,
        "comparison_kind": "matched" if matched else "product-capability",
    }
    result["sha256"] = hashlib.sha256(
        json.dumps(result, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    return result


def load_catalog_document(path: Path) -> dict[str, Any]:
    try:
        catalog = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"could not read catalog {path}: {error}") from error
    validate_catalog_document(catalog)
    return catalog


def validate_catalog_document(catalog: Any) -> None:
    if not isinstance(catalog, dict) or catalog.get("schema_version") != CATALOG_SCHEMA_VERSION:
        raise ContractError(
            f"catalog schema_version must be {CATALOG_SCHEMA_VERSION}"
        )
    torrents = catalog.get("torrents")
    if not isinstance(torrents, list) or not torrents:
        raise ContractError("catalog torrents must be a nonempty list")
    slugs: set[str] = set()
    hashes: set[str] = set()
    for entry in torrents:
        _validate_catalog_entry(entry, slugs, hashes)


def _validate_catalog_entry(
    entry: Any, slugs: set[str], hashes: set[str]
) -> None:
    if not isinstance(entry, dict):
        raise ContractError("catalog torrent entries must be objects")
    slug = entry.get("slug")
    name = entry.get("name")
    info_hash = entry.get("info_hash")
    if not isinstance(slug, str) or not slug or slug in slugs:
        raise ContractError(f"invalid or duplicate catalog slug {slug!r}")
    slugs.add(slug)
    if not isinstance(name, str) or not name:
        raise ContractError(f"catalog entry {slug} has no name")
    if not isinstance(info_hash, str) or not HEX40.fullmatch(info_hash):
        raise ContractError(f"catalog entry {slug} has an invalid lowercase v1 info hash")
    if info_hash in hashes:
        raise ContractError(f"catalog entry {slug} duplicates info hash {info_hash}")
    hashes.add(info_hash)
    source = entry.get("source")
    if not isinstance(source, dict):
        raise ContractError(f"catalog entry {slug} has no source object")
    for key in ("organization", "page", "retrieved", "license_note"):
        if not isinstance(source.get(key), str) or not source[key]:
            raise ContractError(f"catalog entry {slug} has invalid source.{key}")
    if urlsplit(source["page"]).scheme != "https":
        raise ContractError(f"catalog entry {slug} source.page must use HTTPS")
    roles = entry.get("roles")
    if (
        not isinstance(roles, list)
        or not roles
        or len(set(roles)) != len(roles)
        or any(role not in CATALOG_ROLES for role in roles)
    ):
        raise ContractError(f"catalog entry {slug} has invalid roles")
    modes = entry.get("input_modes")
    if (
        not isinstance(modes, list)
        or not modes
        or len(set(modes)) != len(modes)
        or any(mode not in INPUT_MODES for mode in modes)
    ):
        raise ContractError(f"catalog entry {slug} has invalid input_modes")
    magnet = entry.get("magnet")
    if "magnet" in modes:
        _validate_magnet(slug, info_hash, magnet)
    elif magnet is not None:
        raise ContractError(f"catalog entry {slug} has an unused magnet")
    expected = entry.get("expected")
    if not isinstance(expected, dict):
        raise ContractError(f"catalog entry {slug} has no expected geometry")
    for key in ("payload_bytes", "piece_length", "piece_count", "file_count"):
        value = expected.get(key)
        if value is not None and (
            not isinstance(value, int) or isinstance(value, bool) or value <= 0
        ):
            raise ContractError(f"catalog entry {slug} has invalid expected.{key}")
    if expected.get("private") not in (None, False):
        raise ContractError(f"catalog entry {slug} may not be private")
    padding = expected.get("padding_bytes")
    if padding is not None and (
        not isinstance(padding, int) or isinstance(padding, bool) or padding < 0
    ):
        raise ContractError(f"catalog entry {slug} has invalid expected.padding_bytes")
    for key in ("tracker_tiers", "web_seeds"):
        if key not in expected:
            raise ContractError(f"catalog entry {slug} is missing expected.{key}")
    metainfo = entry.get("metainfo")
    if "metainfo" in modes:
        _validate_catalog_metainfo(slug, metainfo, expected, info_hash)
    elif metainfo is not None:
        raise ContractError(f"catalog entry {slug} has unused metainfo configuration")
    limits = entry.get("limits")
    if not isinstance(limits, dict):
        raise ContractError(f"catalog entry {slug} has no limits")
    if limits.get("max_target") not in (
        "metadata",
        "first-piece",
        "50-percent",
        "95-percent",
        "99-percent",
        "complete",
    ):
        raise ContractError(f"catalog entry {slug} has invalid limits.max_target")
    timeout = limits.get("owner_timeout_seconds")
    if not isinstance(timeout, int) or not 1 <= timeout <= MAX_OWNER_TIMEOUT_SECONDS:
        raise ContractError(f"catalog entry {slug} has invalid owner timeout")
    wire_ceiling = limits.get("wire_payload_ceiling_bytes")
    payload_bytes = expected.get("payload_bytes")
    if payload_bytes is None:
        if wire_ceiling is not None:
            raise ContractError(f"catalog entry {slug} has a wire ceiling without geometry")
    elif wire_ceiling != wire_payload_ceiling(payload_bytes):
        raise ContractError(f"catalog entry {slug} has an incorrect wire payload ceiling")


def _validate_magnet(slug: str, info_hash: str, magnet: Any) -> None:
    if not isinstance(magnet, str) or not magnet.startswith("magnet:?"):
        raise ContractError(f"catalog entry {slug} has an invalid magnet")
    query = urlsplit(magnet).query.lower()
    if f"xt=urn%3abtih%3a{info_hash}" not in query and f"xt=urn:btih:{info_hash}" not in query:
        raise ContractError(f"catalog entry {slug} magnet does not match its info hash")


def _validate_catalog_metainfo(
    slug: str, metainfo: Any, expected: dict[str, Any], info_hash: str
) -> None:
    if not isinstance(metainfo, dict):
        raise ContractError(f"catalog entry {slug} has no metainfo recipe")
    url = metainfo.get("url")
    hosts = metainfo.get("allowed_hosts")
    sha256 = metainfo.get("sha256")
    if not isinstance(url, str) or urlsplit(url).scheme != "https":
        raise ContractError(f"catalog entry {slug} metainfo URL must use HTTPS")
    if (
        not isinstance(hosts, list)
        or not hosts
        or any(not isinstance(host, str) or not host for host in hosts)
        or urlsplit(url).hostname not in hosts
    ):
        raise ContractError(f"catalog entry {slug} has invalid metainfo allowed_hosts")
    if not isinstance(sha256, str) or not HEX64.fullmatch(sha256):
        raise ContractError(f"catalog entry {slug} has invalid metainfo SHA-256")
    required = ("payload_bytes", "piece_length", "piece_count", "file_count", "private")
    if any(expected.get(key) is None for key in required):
        raise ContractError(f"catalog entry {slug} metainfo mode requires exact geometry")
    if not HEX40.fullmatch(info_hash):
        raise ContractError(f"catalog entry {slug} metainfo mode requires v1 identity")


def wire_payload_ceiling(payload_bytes: int) -> int:
    if not 0 < payload_bytes <= MAX_PAYLOAD_BYTES:
        raise ContractError("payload bytes are outside the harness limit")
    return max(payload_bytes * 3 // 2, payload_bytes + 256 * 1024 * 1024)


def required_free_space(payload_bytes: int) -> int:
    if not 0 < payload_bytes <= MAX_PAYLOAD_BYTES:
        raise ContractError("payload bytes are outside the harness limit")
    return payload_bytes + max(MIN_FREE_SPACE_BYTES, payload_bytes // 4)


def invocation_network_budget(payload_bytes: int, pairs: int, owners: int = 2) -> int:
    if not 1 <= pairs <= MAX_PAIRS:
        raise ContractError(f"pair count must be between 1 and {MAX_PAIRS}")
    if owners not in (1, 2):
        raise ContractError("owners must be one or two")
    budget = wire_payload_ceiling(payload_bytes) * pairs * owners
    if budget > MAX_NETWORK_BYTES:
        raise ContractError(
            f"invocation worst-case network budget {budget} exceeds {MAX_NETWORK_BYTES}"
        )
    return budget


def validate_output_ancestry(owned_parent: Path, target: Path) -> Path:
    parent = owned_parent.resolve(strict=True)
    resolved = target.resolve(strict=False)
    if resolved == parent or parent not in resolved.parents:
        raise ContractError(f"cleanup target escapes owned parent: {resolved}")
    return resolved


_FORBIDDEN_REPORT_KEYS = {
    "endpoint",
    "endpoints",
    "peer_ip",
    "peer_ips",
    "peer_address",
    "peer_addresses",
    "dns_answers",
    "interface_addresses",
    "save_path",
    "output_root",
    "temporary_root",
    "raw_log",
    "packet_capture",
}


def validate_retained_report(report: Any) -> None:
    encoded = json.dumps(report, sort_keys=True, separators=(",", ":")).encode()
    if len(encoded) > MAX_REPORT_BYTES:
        raise ContractError(f"report exceeds {MAX_REPORT_BYTES} bytes")

    def walk(value: Any, path: str) -> None:
        if isinstance(value, dict):
            for key, child in value.items():
                if not isinstance(key, str):
                    raise ContractError(f"report key at {path} is not text")
                if key.lower() in _FORBIDDEN_REPORT_KEYS:
                    raise ContractError(f"report contains privacy-forbidden field {path}.{key}")
                walk(child, f"{path}.{key}")
        elif isinstance(value, list):
            for index, child in enumerate(value):
                walk(child, f"{path}[{index}]")
        elif isinstance(value, str):
            if len(value.encode()) > MAX_DIAGNOSTIC_BYTES:
                raise ContractError(f"report string at {path} exceeds diagnostic limit")

    walk(report, "report")


def median_absolute_deviation(values: Iterable[float]) -> float | None:
    ordered = sorted(float(value) for value in values)
    if not ordered:
        return None
    middle = _median(ordered)
    return _median(sorted(abs(value - middle) for value in ordered))


def distribution(values: Iterable[float]) -> dict[str, float | int | None]:
    ordered = sorted(float(value) for value in values)
    if not ordered:
        return {
            "count": 0,
            "min": None,
            "median": None,
            "mean": None,
            "p90": None,
            "mad": None,
            "max": None,
        }
    p90 = ordered[math.ceil(len(ordered) * 0.9) - 1] if len(ordered) >= 4 else None
    return {
        "count": len(ordered),
        "min": ordered[0],
        "median": _median(ordered),
        "mean": sum(ordered) / len(ordered),
        "p90": p90,
        "mad": median_absolute_deviation(ordered),
        "max": ordered[-1],
    }


def _median(ordered: list[float]) -> float:
    midpoint = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[midpoint]
    return (ordered[midpoint - 1] + ordered[midpoint]) / 2
