import type {
  ActivePiece,
  GatewayServerMessage,
  IndexRange,
  ServiceSnapshot,
  TorrentView,
  ViewPatch,
  ViewSnapshot,
  ViewUpdate,
} from "./generated/contract";

const MAX_FRAME_BYTES = 512 * 1024;
const MAX_COLLECTION = 100_000;
const MAX_U32 = 4_294_967_295;
const DECIMAL = /^(0|[1-9][0-9]*)$/;
const IDENTIFIER = /^[A-Za-z0-9._-]{1,128}$/;
const TORRENT_ID = /^[A-Fa-f0-9]{40}$/;

export class ContractError extends Error {}

export function decodeGatewayServerMessage(
  source: string,
): GatewayServerMessage {
  if (new TextEncoder().encode(source).byteLength > MAX_FRAME_BYTES) {
    throw new ContractError("gateway frame exceeds the client bound");
  }
  let value: unknown;
  try {
    value = JSON.parse(source);
  } catch {
    throw new ContractError("gateway frame is not valid JSON");
  }
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
  if (update.contract_version !== 1) {
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
      if (update.reason !== "queue_overflow") {
        throw new ContractError("unknown reset reason");
      }
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
      validateActivePiece(snapshot.active, pieceCount);
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
      validateActivePiece(patch.active, pieceCount);
      break;
    }
    default:
      throw new ContractError("unknown view patch type");
  }
}

function validateTorrentView(value: unknown): asserts value is TorrentView {
  const torrent = asRecord(value, "torrent view");
  torrentId(torrent.torrent_id);
  oneOf(torrent.state, "torrent state", [
    "awaiting_metadata",
    "checking",
    "downloading",
    "paused",
    "complete",
    "needs_repair",
    "error",
  ]);
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
  optionalString(torrent.error, "torrent error", 1_024);
}

function validateActivePiece(
  value: unknown,
  pieceCount: number,
): asserts value is ActivePiece | null {
  if (value === null) return;
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
  validateRanges(active.requested, pieceLength, "requested blocks");
  validateRanges(active.received, pieceLength, "received blocks");
  validateRanges(active.stored, pieceLength, "stored blocks");
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
