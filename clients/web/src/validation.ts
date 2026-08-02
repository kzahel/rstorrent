import type {
  ActivePiece,
  ApiErrorEnvelope,
  ApiHello,
  GatewayServerMessage,
  IndexRange,
  OpenViewSetResponse,
  ResponseEnvelope,
  ServiceSnapshot,
  TorrentView,
  UpdateBatch,
  ViewPatch,
  ViewSnapshot,
  ViewUpdate,
} from "./api/generated/v1";
import { assertApiSchema, SchemaError } from "./api/schema";

const MAX_FRAME_BYTES = 512 * 1024;
const MAX_HTTP_RESPONSE_BYTES = 16 * 1024 * 1024;
const MAX_COLLECTION = 100_000;
const MAX_ACTIVE_PEERS = 256;
const PEER_FLAGS = [
  "incoming",
  "encrypted",
  "download_allowed",
  "download_choked",
  "upload_allowed",
  "upload_choked",
  "extension_protocol",
  "metadata_extension",
  "utp",
  "hole_punched",
  "on_parole",
  "optimistic_unchoke",
  "snubbed",
  "upload_only",
  "endgame",
  "seed",
] as const;
const MAX_FILES = 4_096;
const MAX_TRACKERS = 32;
const MAX_DISK_PIECES = 16_384;
const MAX_ACTIVE_PIECES = 16_384;
const MAX_DIAGNOSTIC_EVENTS = 2_048;
const MAX_DIAGNOSTIC_PATCH_EVENTS = 128;
const MAX_U32 = 4_294_967_295;
const DECIMAL = /^(0|[1-9][0-9]*)$/;
const IDENTIFIER = /^[A-Za-z0-9._-]{1,128}$/;
const TORRENT_ID = /^[A-Fa-f0-9]{40}$/;
const DIAGNOSTIC_CATEGORY = /^[a-z0-9_-]+(?:\.[a-z0-9_-]+){0,3}$/;
const DIAGNOSTIC_IDENTIFIER = /^[a-z0-9_.-]{1,48}$/;

export class ContractError extends Error {}

function parseBoundedJson(source: string, maximum: number, label: string): unknown {
  if (new TextEncoder().encode(source).byteLength > maximum) {
    throw new ContractError(`${label} exceeds the client bound`);
  }
  try {
    return JSON.parse(source) as unknown;
  } catch {
    throw new ContractError(`${label} is not valid JSON`);
  }
}

function generated<T>(definition: string, value: unknown): T {
  try {
    assertApiSchema<T>(definition, value);
    return value;
  } catch (error) {
    if (error instanceof SchemaError) {
      throw new ContractError(error.message);
    }
    throw error;
  }
}

export function decodeGatewayServerMessage(
  source: string,
): GatewayServerMessage {
  const value = parseBoundedJson(source, MAX_FRAME_BYTES, "gateway frame");
  generated<GatewayServerMessage>("GatewayServerMessage", value);
  const record = asRecord(value, "gateway message");
  switch (string(record.type, "message type")) {
    case "authenticated":
      boundedInteger(record.contract_version, "contract version", 1, 65_535);
      break;
    case "response":
      validateResponse(record.response);
      break;
    case "subscribed":
    case "unsubscribed":
      identifier(record.request_id, "request ID");
      decimal(record.stream_id, "stream ID");
      break;
    case "update":
      validateUpdate(record.update);
      break;
    case "error":
      if (record.request_id !== undefined && record.request_id !== null) {
        identifier(record.request_id, "request ID");
      }
      oneOf(record.code, "gateway error code", [
        "authentication_required",
        "authentication_failed",
        "invalid_version",
        "invalid_message",
        "resource_limit",
        "unknown_subscription",
        "internal",
      ]);
      boundedString(record.message, "error message", 1_024);
      break;
    default:
      throw new ContractError("unknown gateway message type");
  }
  return value as GatewayServerMessage;
}

export function decodeApiHello(source: string): ApiHello {
  const value = generated<ApiHello>(
    "ApiHello",
    parseBoundedJson(source, MAX_FRAME_BYTES, "API hello response"),
  );
  if (value.api.minimum > 1 || value.api.current < 1) {
    throw new ContractError("API version 1 is not supported by the server");
  }
  decimal(value.limits.lease_millis, "view-set lease");
  boundedInteger(
    value.limits.max_view_sets_per_owner,
    "maximum view sets",
    1,
    65_535,
  );
  boundedInteger(value.limits.max_views_per_set, "maximum views", 1, 65_535);
  boundedInteger(
    value.limits.max_view_id_bytes,
    "maximum view ID bytes",
    1,
    65_535,
  );
  boundedInteger(value.limits.min_queue_bytes, "minimum queue bytes", 1, MAX_U32);
  boundedInteger(value.limits.max_queue_bytes, "maximum queue bytes", 1, MAX_U32);
  boundedInteger(
    value.limits.max_snapshot_bytes,
    "maximum snapshot bytes",
    value.limits.max_queue_bytes,
    MAX_U32,
  );
  boundedInteger(value.limits.max_wait_millis, "maximum wait", 0, MAX_U32);
  if (
    value.limits.min_queue_bytes > value.limits.default_queue_bytes ||
    value.limits.default_queue_bytes > value.limits.max_queue_bytes
  ) {
    throw new ContractError("API queue limits are inconsistent");
  }
  return value;
}

export function decodeResponseEnvelope(source: string): ResponseEnvelope {
  const value = generated<ResponseEnvelope>(
    "ResponseEnvelope",
    parseBoundedJson(source, MAX_FRAME_BYTES, "command response"),
  );
  validateResponse(value);
  return value;
}

export function decodeApiErrorEnvelope(source: string): ApiErrorEnvelope {
  const value = generated<ApiErrorEnvelope>(
    "ApiErrorEnvelope",
    parseBoundedJson(source, MAX_FRAME_BYTES, "API error response"),
  );
  boundedString(value.error.message, "API error message", 1_024);
  return value;
}

export function decodeOpenViewSetResponse(source: string): OpenViewSetResponse {
  const value = generated<OpenViewSetResponse>(
    "OpenViewSetResponse",
    parseBoundedJson(source, MAX_HTTP_RESPONSE_BYTES, "open view-set response"),
  );
  identifier(value.view_set_id, "view-set ID");
  decimal(value.lease_millis, "view-set lease");
  boundedInteger(value.effective_queue_bytes, "view-set queue bytes", 1, MAX_U32);
  validateUpdateBatch(value.initial);
  if (value.initial.view_set_id !== value.view_set_id) {
    throw new ContractError("initial batch belongs to another view set");
  }
  return value;
}

export function decodeUpdateBatch(source: string): UpdateBatch {
  const value = generated<UpdateBatch>(
    "UpdateBatch",
    parseBoundedJson(source, MAX_HTTP_RESPONSE_BYTES, "view-set update response"),
  );
  validateUpdateBatch(value);
  return value;
}

function validateUpdateBatch(batch: UpdateBatch): void {
  if (batch.api_version !== 1) {
    throw new ContractError("unsupported application API version");
  }
  identifier(batch.view_set_id, "view-set ID");
  decimal(batch.epoch, "view-set epoch");
  decimal(batch.base_cursor, "view-set base cursor");
  decimal(batch.cursor, "view-set cursor");
  decimal(batch.durable_revision, "durable revision");
  const base = BigInt(batch.base_cursor);
  const cursor = BigInt(batch.cursor);
  if (
    (batch.updates.length === 0 && cursor !== base) ||
    (batch.updates.length > 0 && cursor <= base)
  ) {
    throw new ContractError("view-set batch cursor does not match its updates");
  }
  for (const update of batch.updates) {
    if ("view_id" in update && update.view_id !== null) {
      identifier(update.view_id, "view ID");
    }
    switch (update.type) {
      case "snapshot":
        validateViewSnapshot(update.snapshot);
        break;
      case "patch":
        validateViewPatch(update.patch);
        break;
      case "view_removed":
      case "reset_required":
        break;
    }
  }
}

function validateResponse(value: unknown): void {
  const response = asRecord(value, "response");
  boundedInteger(response.version, "control version", 1, 65_535);
  identifier(response.request_id, "request ID");
  decimal(response.revision, "revision");
  const status = string(response.status, "response status");
  if (status === "success") {
    validateServiceSnapshot(response.snapshot);
  } else if (status === "error") {
    const error = asRecord(response.error, "control error");
    boundedString(error.code, "control error code", 64);
    boundedString(error.message, "control error message", 1_024);
  } else {
    throw new ContractError("unknown response status");
  }
}

function validateServiceSnapshot(value: unknown): void {
  const snapshot = asRecord(value, "service snapshot");
  identifier(snapshot.profile_id, "profile ID");
  decimal(snapshot.revision, "snapshot revision");
  const torrents = array(snapshot.torrents, "torrent snapshots");
  for (const item of torrents) {
    const torrent = asRecord(item, "torrent snapshot");
    torrentId(torrent.torrent_id);
    identifier(torrent.storage_root, "storage root");
    boundedString(torrent.state, "torrent state", 32);
    boundedString(torrent.storage_state, "storage state", 32);
    boolean(torrent.metadata_available, "metadata available");
    boundedInteger(torrent.piece_count, "piece count", 0, MAX_U32);
    boundedInteger(
      torrent.verified_piece_count,
      "verified piece count",
      0,
      MAX_U32,
    );
    array(torrent.skip_files, "skipped files").forEach((index) =>
      boundedInteger(index, "file index", 0, MAX_U32),
    );
    optionalString(torrent.error, "torrent error", 1_024);
  }
}

function validateUpdate(value: unknown): void {
  const update = asRecord(value, "view update");
  if (update.contract_version !== 2) {
    throw new ContractError("unsupported view contract version");
  }
  decimal(update.stream_id, "stream ID");
  decimal(update.epoch, "stream epoch");
  decimal(update.sequence, "stream sequence");
  decimal(update.base_revision, "base revision");
  decimal(update.revision, "view revision");
  switch (string(update.type, "view update type")) {
    case "snapshot":
      validateViewSnapshot(update.snapshot);
      break;
    case "patch":
      validateViewPatch(update.patch);
      break;
    case "reset_required":
      break;
    default:
      throw new ContractError("unknown view update type");
  }
}

function validateViewSnapshot(value: unknown): void {
  const snapshot = asRecord(value, "view snapshot");
  switch (string(snapshot.type, "view snapshot type")) {
    case "torrent_list":
      array(snapshot.torrents, "torrent list").forEach(validateTorrentView);
      break;
    case "torrent":
      if (snapshot.torrent !== null) validateTorrentView(snapshot.torrent);
      break;
    case "piece_activity": {
      torrentId(snapshot.torrent_id);
      const pieceCount = boundedInteger(
        snapshot.piece_count,
        "piece count",
        0,
        MAX_U32,
      );
      validateRanges(snapshot.verified, pieceCount, "verified pieces");
      validateActivePieces(snapshot.active, pieceCount, "active pieces");
      break;
    }
    case "session_disk": {
      validateDiskPipeline(snapshot.pipeline);
      const pieces = array(snapshot.pieces, "active disk pieces");
      if (pieces.length > MAX_DISK_PIECES) {
        throw new ContractError("disk piece view exceeds its row bound");
      }
      pieces.forEach(validateDiskPiece);
      break;
    }
    case "peers": {
      const owningTorrent = string(snapshot.torrent_id, "peer-view torrent ID");
      torrentId(owningTorrent);
      const peers = array(snapshot.peers, "active peers");
      if (peers.length > MAX_ACTIVE_PEERS) {
        throw new ContractError("active peer view exceeds its row bound");
      }
      peers.forEach((peer) => validatePeerView(peer, owningTorrent));
      break;
    }
    case "files": {
      torrentId(snapshot.torrent_id);
      oneOf(snapshot.state, "file catalog state", [
        "metadata_pending",
        "available",
        "torrent_missing",
      ]);
      optionalString(snapshot.filesystem_content_base, "filesystem content base", 16_384);
      const files = array(snapshot.files, "torrent files");
      if (files.length > MAX_FILES) {
        throw new ContractError("file view exceeds its row bound");
      }
      files.forEach(validateFileView);
      if (snapshot.state !== "available" && files.length !== 0) {
        throw new ContractError("unavailable file catalog contains rows");
      }
      break;
    }
    case "trackers": {
      torrentId(snapshot.torrent_id);
      oneOf(snapshot.state, "tracker catalog state", [
        "available",
        "torrent_missing",
      ]);
      const trackers = array(snapshot.trackers, "torrent trackers");
      if (trackers.length > MAX_TRACKERS) {
        throw new ContractError("tracker view exceeds its row bound");
      }
      trackers.forEach(validateTrackerView);
      if (snapshot.state !== "available" && trackers.length !== 0) {
        throw new ContractError("unavailable tracker catalog contains rows");
      }
      break;
    }
    case "diagnostics": {
      const events = array(snapshot.events, "diagnostic events");
      if (events.length > MAX_DIAGNOSTIC_EVENTS) {
        throw new ContractError("diagnostic snapshot exceeds its event bound");
      }
      events.forEach(validateDiagnosticEvent);
      validateDiagnosticRetention(snapshot.retention);
      break;
    }
    default:
      throw new ContractError("unknown view snapshot type");
  }
}

function validateViewPatch(value: unknown): void {
  const patch = asRecord(value, "view patch");
  switch (string(patch.type, "view patch type")) {
    case "torrent_list":
      array(patch.upsert, "torrent upserts").forEach(validateTorrentView);
      array(patch.removed, "torrent removals").forEach(torrentId);
      break;
    case "torrent":
      if (patch.torrent !== null) validateTorrentView(patch.torrent);
      break;
    case "piece_activity": {
      torrentId(patch.torrent_id);
      const pieceCount = boundedInteger(
        patch.piece_count,
        "piece count",
        0,
        MAX_U32,
      );
      validateRanges(patch.verified, pieceCount, "verified pieces");
      validateRanges(patch.cleared, pieceCount, "cleared pieces");
      validateActivePieces(patch.active_upsert, pieceCount, "active piece upserts");
      validatePieceIds(patch.active_removed, "active piece removals");
      break;
    }
    case "session_disk": {
      validateDiskPipeline(patch.pipeline);
      const upserts = array(patch.upsert, "active disk piece upserts");
      if (upserts.length > MAX_DISK_PIECES) {
        throw new ContractError("disk piece patch exceeds its row bound");
      }
      upserts.forEach(validateDiskPiece);
      array(patch.removed, "active disk piece removals").forEach((rowId) =>
        boundedString(rowId, "disk piece row ID", 256),
      );
      break;
    }
    case "peers": {
      const owningTorrent = string(patch.torrent_id, "peer-view torrent ID");
      torrentId(owningTorrent);
      const upserts = array(patch.upsert, "active peer upserts");
      if (upserts.length > MAX_ACTIVE_PEERS) {
        throw new ContractError("active peer patch exceeds its row bound");
      }
      upserts.forEach((peer) => validatePeerView(peer, owningTorrent));
      array(patch.removed, "active peer removals").forEach((connection) =>
        decimal(connection, "peer connection ID"),
      );
      break;
    }
    case "files": {
      torrentId(patch.torrent_id);
      const upserts = array(patch.upsert, "file upserts");
      if (upserts.length > MAX_FILES) {
        throw new ContractError("file patch exceeds its row bound");
      }
      upserts.forEach(validateFileView);
      array(patch.removed, "file removals").forEach((fileId) =>
        decimal(fileId, "file ID"),
      );
      break;
    }
    case "trackers": {
      torrentId(patch.torrent_id);
      const upserts = array(patch.upsert, "tracker upserts");
      if (upserts.length > MAX_TRACKERS) {
        throw new ContractError("tracker patch exceeds its row bound");
      }
      upserts.forEach(validateTrackerView);
      array(patch.removed, "tracker removals").forEach((trackerId) =>
        boundedString(trackerId, "tracker ID", 2_048),
      );
      break;
    }
    case "diagnostics": {
      const events = array(patch.events, "diagnostic events");
      if (events.length > MAX_DIAGNOSTIC_PATCH_EVENTS) {
        throw new ContractError("diagnostic patch exceeds its event bound");
      }
      events.forEach(validateDiagnosticEvent);
      validateDiagnosticRetention(patch.retention);
      break;
    }
    default:
      throw new ContractError("unknown view patch type");
  }
}

function validateTorrentView(value: unknown): asserts value is TorrentView {
  const torrent = asRecord(value, "torrent view");
  torrentId(torrent.torrent_id);
  optionalString(torrent.display_name, "torrent display name", 255);
  boolean(torrent.metadata_available, "metadata available");
  boundedInteger(torrent.piece_count, "piece count", 0, MAX_U32);
  boundedInteger(
    torrent.verified_piece_count,
    "verified piece count",
    0,
    MAX_U32,
  );
  decimal(torrent.requested_bytes, "requested bytes");
  decimal(torrent.received_bytes, "received bytes");
  decimal(torrent.stored_bytes, "stored bytes");
  optionalInteger(
    torrent.configured_tracker_count,
    "configured tracker count",
    MAX_TRACKERS,
  );
  const progress = asRecord(torrent.progress, "progress assessment");
  oneOf(progress.disposition, "progress disposition", [
    "active",
    "waiting",
    "blocked",
    "inactive",
  ]);
  oneOf(progress.phase, "progress phase", [
    "discovery",
    "metadata",
    "storage",
    "transfer",
    "verification",
    "publication",
  ]);
  boundedString(progress.reason, "progress reason", 64);
  array(progress.actions, "progress actions").forEach((action) =>
    boundedString(action, "progress action", 64),
  );
  optionalString(torrent.error, "torrent error", 1_024);
}

function validateFileView(value: unknown): void {
  const file = asRecord(value, "file view");
  decimal(file.file_id, "file ID");
  boundedInteger(file.file_index, "file index", 0, MAX_FILES - 1);
  const path = array(file.path, "file path");
  if (path.length === 0 || path.length > 64) {
    throw new ContractError("file path component count is invalid");
  }
  path.forEach((component) => boundedString(component, "file path component", 255));
  decimal(file.length_bytes, "file length");
  decimal(file.torrent_offset_bytes, "file torrent offset");
  optionalInteger(file.first_piece, "first file piece", MAX_U32);
  optionalInteger(file.last_piece, "last file piece", MAX_U32);
  if (file.selection !== null) {
    oneOf(file.selection, "file selection", ["wanted", "skipped"]);
  }
  boolean(file.padding, "file padding flag");
  if (file.padding && file.selection !== null) {
    throw new ContractError("padding file has a selection state");
  }
  decimal(file.done_bytes, "file done bytes");
  decimal(file.verified_bytes, "file verified bytes");
  const length = BigInt(string(file.length_bytes, "file length"));
  const done = BigInt(string(file.done_bytes, "file done bytes"));
  const verified = BigInt(string(file.verified_bytes, "file verified bytes"));
  if (verified > done || done > length) {
    throw new ContractError("file progress counters are inconsistent");
  }
}

function validateTrackerView(value: unknown): void {
  const tracker = asRecord(value, "tracker view");
  const id = boundedString(tracker.tracker_id, "tracker ID", 2_048);
  const url = boundedString(tracker.url, "tracker URL", 2_048);
  if (id !== url || !url.startsWith("udp://")) {
    throw new ContractError("tracker identity is not its canonical UDP URL");
  }
  oneOf(tracker.transport, "tracker transport", ["udp"]);
  oneOf(tracker.source, "tracker source", ["magnet"]);
  boundedInteger(tracker.tier, "tracker tier", 0, MAX_U32);
  oneOf(tracker.status, "tracker status", [
    "inactive",
    "idle",
    "announcing",
    "retry_wait",
    "reannounce_wait",
  ]);
  if (tracker.announce_event !== null) {
    oneOf(tracker.announce_event, "tracker announce event", ["started", "update"]);
  }
  boundedInteger(tracker.total_attempts, "tracker attempts", 0, MAX_U32);
  boundedInteger(tracker.consecutive_failures, "tracker failures", 0, 127);
  ["last_peer_count", "seeders", "leechers", "interval_seconds"].forEach(
    (field) => optionalInteger(tracker[field], `tracker ${field}`, MAX_U32),
  );
  if (tracker.next_action !== null) {
    oneOf(tracker.next_action, "tracker next action", [
      "announce",
      "retry",
      "reannounce",
    ]);
  }
  [
    "next_action_in_millis",
    "last_success_age_millis",
    "last_failure_age_millis",
  ].forEach((field) => optionalDecimal(tracker[field], `tracker ${field}`));
  optionalString(tracker.last_error, "tracker last error", 256);
}

function validatePeerView(value: unknown, owningTorrent: string): void {
  const peer = asRecord(value, "active peer");
  decimal(peer.connection_id, "peer connection ID");
  torrentId(peer.torrent_id);
  if (peer.torrent_id !== owningTorrent) {
    throw new ContractError("active peer belongs to another torrent");
  }
  optionalDecimal(peer.peer_record_id, "peer record ID");
  oneOf(peer.direction, "peer direction", ["incoming", "outgoing"]);
  oneOf(peer.transport, "peer transport", ["tcp", "utp"]);
  oneOf(peer.lifecycle, "peer lifecycle", [
    "transport_connecting",
    "protocol_handshaking",
    "connected",
    "disconnecting",
  ]);
  oneOf(peer.role, "peer role", ["metadata", "content"]);
  if (peer.peer_flags !== undefined) {
    const flags = array(peer.peer_flags, "peer flags");
    if (flags.length > PEER_FLAGS.length) {
      throw new ContractError("peer flags exceed their bound");
    }
    flags.forEach((flag) => oneOf(flag, "peer flag", PEER_FLAGS));
    if (new Set(flags).size !== flags.length) {
      throw new ContractError("peer flags contain duplicates");
    }
  }
  decimal(peer.lifecycle_age_millis, "peer lifecycle age");
  boundedString(peer.remote_endpoint, "peer remote endpoint", 128);
  optionalString(peer.local_endpoint, "peer local endpoint", 128);
  const sources = array(peer.sources, "peer sources");
  if (sources.length > 8) throw new ContractError("peer sources exceed their bound");
  sources.forEach((source) =>
    oneOf(source, "peer source", [
      "tracker",
      "peer_exchange",
      "dht",
      "local_discovery",
      "incoming",
      "manual",
      "magnet_hint",
      "cache",
    ]),
  );
  optionalString(peer.peer_id, "peer ID", 128);
  optionalString(peer.client_name, "peer client name", 128);
  [
    "supports_extensions",
    "supports_ut_metadata",
    "local_interested",
    "remote_interested",
    "remote_choking",
    "local_choking",
  ].forEach((field) => optionalBoolean(peer[field], `peer ${field}`));
  ["available_piece_count", "wanted_piece_count", "pending_requests", "target_requests"].forEach(
    (field) => optionalInteger(peer[field], `peer ${field}`, MAX_U32),
  );
  [
    "payload_download_rate_bytes",
    "payload_downloaded_bytes",
    "protocol_download_rate_bytes",
    "protocol_downloaded_bytes",
    "payload_upload_rate_bytes",
    "payload_uploaded_bytes",
    "queued_payload_bytes",
    "oldest_request_age_millis",
    "request_timeout_millis",
    "connected_age_millis",
    "last_useful_age_millis",
    "last_payload_age_millis",
  ].forEach((field) => optionalDecimal(peer[field], `peer ${field}`));
  if (peer.request_phase !== null) {
    oneOf(peer.request_phase, "peer request phase", [
      "slow_start",
      "steady",
      "stalled",
    ]);
  }
  if (peer.disconnect_reason !== null) {
    oneOf(peer.disconnect_reason, "peer disconnect reason", [
      "connect",
      "handshake",
      "protocol",
      "remote_closed",
    ]);
  }
  const capabilities = asRecord(peer.capabilities, "peer capabilities");
  [
    "local_endpoint",
    "client_name",
    "ut_metadata",
    "interest_directions",
    "local_choke",
    "piece_availability",
    "protocol_rates",
    "upload",
    "metadata_stage",
  ].forEach((field) =>
    oneOf(capabilities[field], `peer capability ${field}`, [
      "available",
      "unavailable",
      "unsupported",
    ]),
  );
}

function validateDiagnosticEvent(value: unknown): void {
  const event = asRecord(value, "diagnostic event");
  decimal(event.sequence, "diagnostic sequence");
  decimal(event.timestamp_millis, "diagnostic timestamp");
  oneOf(event.severity, "diagnostic severity", [
    "trace",
    "debug",
    "info",
    "warning",
    "error",
  ]);
  const category = boundedString(event.category, "diagnostic category", 64);
  if (!DIAGNOSTIC_CATEGORY.test(category)) {
    throw new ContractError("diagnostic category is invalid");
  }
  const code = boundedString(event.code, "diagnostic code", 48);
  if (!DIAGNOSTIC_IDENTIFIER.test(code)) {
    throw new ContractError("diagnostic code is invalid");
  }
  if (event.torrent_id !== undefined && event.torrent_id !== null) {
    torrentId(event.torrent_id);
  }
  boundedString(event.message, "diagnostic message", 1_280);
  const subjects = array(event.subjects, "diagnostic subjects");
  if (subjects.length > 4) {
    throw new ContractError("diagnostic subjects exceed their bound");
  }
  subjects.forEach(validateDiagnosticSubject);
  const fields = array(event.fields, "diagnostic fields");
  if (fields.length > 8) {
    throw new ContractError("diagnostic fields exceed their bound");
  }
  for (const field of fields) {
    const record = asRecord(field, "diagnostic field");
    const key = boundedString(record.key, "diagnostic field key", 48);
    if (!DIAGNOSTIC_IDENTIFIER.test(key)) {
      throw new ContractError("diagnostic field key is invalid");
    }
    validateDiagnosticValue(record.value);
  }
}

function validateDiagnosticRetention(value: unknown): void {
  const retention = asRecord(value, "diagnostic retention");
  decimal(retention.source_evicted_count, "diagnostic source eviction count");
  decimal(retention.retained_from_sequence, "diagnostic retained sequence");
}

function validateDiagnosticSubject(value: unknown): void {
  const subject = asRecord(value, "diagnostic subject");
  switch (string(subject.type, "diagnostic subject type")) {
    case "peer_connection":
      boundedString(subject.connection_id, "peer connection subject", 240);
      break;
    case "tracker":
      boundedString(subject.tracker_id, "tracker subject", 240);
      break;
    case "piece":
      boundedInteger(subject.piece_index, "piece subject index", 0, MAX_U32);
      optionalInteger(subject.attempt, "piece subject attempt", MAX_U32);
      break;
    case "file":
      boundedInteger(subject.file_index, "file subject index", 0, MAX_U32);
      break;
    case "task": {
      const kind = boundedString(subject.kind, "task subject kind", 48);
      if (!DIAGNOSTIC_IDENTIFIER.test(kind)) {
        throw new ContractError("task subject kind is invalid");
      }
      boundedString(subject.generation, "task subject generation", 240);
      break;
    }
    default:
      throw new ContractError("diagnostic subject type is unknown");
  }
}

function validateDiagnosticValue(value: unknown): void {
  const diagnosticValue = asRecord(value, "diagnostic value");
  switch (string(diagnosticValue.type, "diagnostic value type")) {
    case "boolean":
      boolean(diagnosticValue.value, "diagnostic boolean value");
      break;
    case "count":
    case "bytes":
    case "duration_millis":
      decimal(diagnosticValue.value, "diagnostic decimal value");
      break;
    case "error_code": {
      const code = boundedString(diagnosticValue.value, "diagnostic error code", 48);
      if (!DIAGNOSTIC_IDENTIFIER.test(code)) {
        throw new ContractError("diagnostic error code is invalid");
      }
      break;
    }
    case "text":
    case "endpoint":
      boundedString(diagnosticValue.value, "diagnostic string value", 960);
      break;
    default:
      throw new ContractError("diagnostic value type is unknown");
  }
}

function validateActivePiece(
  value: unknown,
  pieceCount: number,
): asserts value is ActivePiece {
  const active = asRecord(value, "active piece");
  const pieceIndex = boundedInteger(
    active.piece_index,
    "active piece index",
    0,
    MAX_U32,
  );
  if (pieceIndex >= pieceCount) {
    throw new ContractError("active piece index exceeds the torrent");
  }
  const pieceLength = boundedInteger(
    active.piece_length,
    "active piece length",
    1,
    MAX_U32,
  );
  const attempt = boundedInteger(active.attempt, "active piece attempt", 1, MAX_U32);
  const pieceId = boundedString(active.piece_id, "active piece ID", 64);
  if (pieceId !== `${pieceIndex}:${attempt}`) {
    throw new ContractError("active piece ID does not match its piece attempt");
  }
  oneOf(active.stage, "active piece stage", [
    "requested",
    "received",
    "stored",
    "hashing",
    "failed",
  ]);
  validateRanges(active.requested, pieceLength, "requested blocks");
  validateRanges(active.received, pieceLength, "received blocks");
  validateRanges(active.stored, pieceLength, "stored blocks");
  validateDisjointActiveRanges(active, pieceLength);
  decimal(active.age_millis, "active piece age");
  optionalString(active.error, "active piece error", 256);
}

function validateActivePieces(value: unknown, pieceCount: number, label: string): void {
  const pieces = array(value, label);
  if (pieces.length > MAX_ACTIVE_PIECES || pieces.length > pieceCount) {
    throw new ContractError(`${label} exceeds its bound`);
  }
  const ids = new Set<string>();
  const indices = new Set<number>();
  for (const value of pieces) {
    validateActivePiece(value, pieceCount);
    const piece = value as ActivePiece;
    if (ids.has(piece.piece_id) || indices.has(piece.piece_index)) {
      throw new ContractError(`${label} contains duplicate identity`);
    }
    ids.add(piece.piece_id);
    indices.add(piece.piece_index);
  }
}

function validatePieceIds(value: unknown, label: string): void {
  const values = array(value, label);
  if (values.length > MAX_ACTIVE_PIECES) {
    throw new ContractError(`${label} exceeds its bound`);
  }
  const ids = new Set<string>();
  for (const value of values) {
    const id = boundedString(value, "active piece ID", 64);
    if (!/^(0|[1-9][0-9]*):[1-9][0-9]*$/.test(id) || ids.has(id)) {
      throw new ContractError(`${label} contains an invalid or duplicate ID`);
    }
    ids.add(id);
  }
}

function validateDisjointActiveRanges(
  active: Record<string, unknown>,
  pieceLength: number,
): void {
  const ranges = ["requested", "received", "stored"]
    .flatMap((field) =>
      (active[field] as IndexRange[]).map((range) => ({ ...range, field })),
    )
    .sort((left, right) => left.start - right.start || left.end_exclusive - right.end_exclusive);
  let previousEnd = 0;
  for (const range of ranges) {
    if (range.end_exclusive > pieceLength || range.start < previousEnd) {
      throw new ContractError("active piece lifecycle ranges overlap");
    }
    previousEnd = range.end_exclusive;
  }
}

function validateDiskPipeline(value: unknown): void {
  const pipeline = asRecord(value, "disk pipeline");
  oneOf(pipeline.pressure, "disk pressure", [
    "idle",
    "normal",
    "backpressured",
    "draining",
    "error",
  ]);
  oneOf(pipeline.checkpoint_stage, "disk checkpoint stage", [
    "idle",
    "syncing",
    "committing",
    "error",
  ]);
  boolean(pipeline.intake_backpressured, "disk intake backpressure");
  [
    "sample_millis",
    "resident_limit_bytes",
    "resident_high_watermark_bytes",
    "resident_low_watermark_bytes",
    "requested_bytes",
    "resident_bytes",
    "queued_write_bytes",
    "writing_bytes",
    "hashing_bytes",
    "checkpoint_dirty_pieces",
    "checkpoint_dirty_bytes",
    "checkpoint_dirty_piece_high_water",
    "checkpoint_dirty_byte_high_water",
    "checkpoint_oldest_dirty_millis",
    "checkpoint_batches_started",
    "checkpoint_batches_completed",
    "checkpoint_pieces_completed",
    "checkpoint_sync_operations_completed",
    "checkpoint_sync_service_micros",
    "checkpoint_sync_service_max_micros",
    "checkpoint_commit_service_micros",
    "checkpoint_commit_service_max_micros",
    "storage_jobs_pending",
    "received_bytes_total",
    "stored_bytes_total",
    "verified_bytes_total",
    "receive_rate_bytes",
    "write_rate_bytes",
    "hash_rate_bytes",
    "write_operations_started",
    "write_operations_completed",
    "hash_operations_started",
    "hash_operations_completed",
    "write_queue_wait_micros",
    "write_queue_wait_max_micros",
    "write_service_micros",
    "write_service_max_micros",
    "hash_queue_wait_micros",
    "hash_queue_wait_max_micros",
    "hash_service_micros",
    "hash_service_max_micros",
    "pressure_transition_count",
    "backpressured_millis_total",
  ].forEach((field) => decimal(pipeline[field], `disk ${field}`));
  optionalDecimal(pipeline.checkpoint_active_micros, "disk checkpoint active time");
  optionalString(pipeline.last_error, "disk error", 256);
}

function validateDiskPiece(value: unknown): void {
  const piece = asRecord(value, "active disk piece");
  boundedString(piece.row_id, "disk piece row ID", 256);
  torrentId(piece.torrent_id);
  boundedString(piece.torrent_name, "disk piece torrent name", 255);
  boundedInteger(piece.piece_index, "disk piece index", 0, MAX_U32);
  const pieceLength = boundedInteger(
    piece.piece_length,
    "disk piece length",
    1,
    MAX_U32,
  );
  boundedInteger(piece.attempt, "disk piece attempt", 1, MAX_U32);
  oneOf(piece.stage, "disk piece stage", [
    "receiving",
    "queued",
    "writing",
    "stored",
    "hashing",
    "checkpoint_dirty",
    "checkpoint_syncing",
    "checkpoint_committing",
    "failed",
  ]);
  for (const field of ["requested_bytes", "received_bytes", "stored_bytes"]) {
    const bytes = decimal(piece[field], `disk piece ${field}`);
    if (BigInt(bytes) > BigInt(pieceLength)) {
      throw new ContractError(`disk piece ${field} exceeds the piece length`);
    }
  }
  decimal(piece.age_millis, "disk piece age");
  decimal(piece.stage_age_millis, "disk piece stage age");
  optionalString(piece.error, "disk piece error", 256);
}

function validateRanges(
  value: unknown,
  maximum: number,
  label: string,
): asserts value is IndexRange[] {
  const ranges = array(value, label);
  let previousEnd = 0;
  for (const [index, item] of ranges.entries()) {
    const range = asRecord(item, `${label} range`);
    const start = boundedInteger(range.start, `${label} start`, 0, MAX_U32);
    const end = boundedInteger(
      range.end_exclusive,
      `${label} end`,
      1,
      MAX_U32,
    );
    if (start >= end || end > maximum || (index !== 0 && start <= previousEnd)) {
      throw new ContractError(`${label} are not canonical and bounded`);
    }
    previousEnd = end;
  }
}

function asRecord(
  value: unknown,
  label: string,
): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new ContractError(`${label} must be an object`);
  }
  return value as Record<string, unknown>;
}

function array(value: unknown, label: string): unknown[] {
  if (!Array.isArray(value) || value.length > MAX_COLLECTION) {
    throw new ContractError(`${label} must be a bounded array`);
  }
  return value;
}

function string(value: unknown, label: string): string {
  if (typeof value !== "string") {
    throw new ContractError(`${label} must be a string`);
  }
  return value;
}

function boundedString(
  value: unknown,
  label: string,
  maximum: number,
): string {
  const result = string(value, label);
  if (new TextEncoder().encode(result).byteLength > maximum) {
    throw new ContractError(`${label} exceeds ${maximum} bytes`);
  }
  return result;
}

function optionalString(
  value: unknown,
  label: string,
  maximum: number,
): void {
  if (value !== undefined && value !== null) {
    boundedString(value, label, maximum);
  }
}

function optionalDecimal(value: unknown, label: string): void {
  if (value !== undefined && value !== null) decimal(value, label);
}

function optionalBoolean(value: unknown, label: string): void {
  if (value !== undefined && value !== null) boolean(value, label);
}

function optionalInteger(value: unknown, label: string, maximum: number): void {
  if (value !== undefined && value !== null) {
    boundedInteger(value, label, 0, maximum);
  }
}

function decimal(value: unknown, label: string): string {
  const result = string(value, label);
  if (!DECIMAL.test(result) || result.length > 20) {
    throw new ContractError(`${label} must be a bounded canonical decimal`);
  }
  return result;
}

function identifier(value: unknown, label: string): string {
  const result = string(value, label);
  if (!IDENTIFIER.test(result)) {
    throw new ContractError(`${label} is invalid`);
  }
  return result;
}

function torrentId(value: unknown): asserts value is string {
  if (typeof value !== "string" || !TORRENT_ID.test(value)) {
    throw new ContractError("torrent ID is invalid");
  }
}

function boolean(value: unknown, label: string): boolean {
  if (typeof value !== "boolean") {
    throw new ContractError(`${label} must be boolean`);
  }
  return value;
}

function boundedInteger(
  value: unknown,
  label: string,
  minimum: number,
  maximum: number,
): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < minimum ||
    value > maximum
  ) {
    throw new ContractError(`${label} must be an integer in range`);
  }
  return value;
}

function oneOf<T extends string>(
  value: unknown,
  label: string,
  choices: readonly T[],
): T {
  const result = string(value, label);
  if (!choices.includes(result as T)) {
    throw new ContractError(`${label} is unknown`);
  }
  return result as T;
}

export type ValidatedSnapshot = ServiceSnapshot;
export type ValidatedViewSnapshot = ViewSnapshot;
export type ValidatedViewPatch = ViewPatch;
export type ValidatedViewUpdate = ViewUpdate;
