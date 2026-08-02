import type { ActivePiece, IndexRange, ViewSnapshot } from "../../api";
import type { ActivePieceSummary, PieceMapSet } from "../model";

export interface MappedPieceActivity {
  readonly epoch: string;
  readonly source: Extract<ViewSnapshot, { type: "piece_activity" }>;
  readonly value: PieceMapSet;
}

export function mapPieceActivity(
  snapshot: Extract<ViewSnapshot, { type: "piece_activity" }>,
  previous: MappedPieceActivity | null,
  epoch: string,
): PieceMapSet {
  const incremental =
    previous !== null &&
    previous.epoch === epoch &&
    previous.source.torrent_id === snapshot.torrent_id &&
    previous.source.piece_count === snapshot.piece_count;
  const verified = incremental
    ? previous.value.verified
    : new Uint8Array(snapshot.piece_count);
  if (incremental) {
    for (const range of subtractRanges(previous.source.verified, snapshot.verified)) {
      verified.fill(0, range.start, range.end_exclusive);
    }
    for (const range of subtractRanges(snapshot.verified, previous.source.verified)) {
      verified.fill(1, range.start, range.end_exclusive);
    }
  } else {
    for (const range of snapshot.verified) {
      verified.fill(1, range.start, range.end_exclusive);
    }
  }
  return {
    torrentId: snapshot.torrent_id,
    pieceCount: snapshot.piece_count,
    verified,
    active: snapshot.active.map(mapActivePiece),
    revision: incremental ? previous.value.revision + 1 : 1,
  };
}

function mapActivePiece(piece: ActivePiece): ActivePieceSummary {
  return {
    id: piece.piece_id,
    pieceIndex: piece.piece_index,
    attempt: piece.attempt,
    pieceLength: piece.piece_length,
    stage: piece.stage,
    requestedBytes: rangeBytes(piece.requested),
    receivedBytes: rangeBytes(piece.received),
    storedBytes: rangeBytes(piece.stored),
    ageMillis: safeNumber(piece.age_millis),
    error: piece.error ?? null,
  };
}

function subtractRanges(
  left: readonly IndexRange[],
  right: readonly IndexRange[],
): IndexRange[] {
  const output: IndexRange[] = [];
  let rightIndex = 0;
  for (const range of left) {
    let cursor = range.start;
    while (
      right[rightIndex] !== undefined &&
      right[rightIndex]!.end_exclusive <= cursor
    ) {
      rightIndex += 1;
    }
    let scan = rightIndex;
    while (right[scan] !== undefined && right[scan]!.start < range.end_exclusive) {
      const exclusion = right[scan]!;
      if (exclusion.start > cursor) {
        output.push({
          start: cursor,
          end_exclusive: Math.min(exclusion.start, range.end_exclusive),
        });
      }
      cursor = Math.max(cursor, exclusion.end_exclusive);
      if (cursor >= range.end_exclusive) break;
      scan += 1;
    }
    if (cursor < range.end_exclusive) {
      output.push({ start: cursor, end_exclusive: range.end_exclusive });
    }
  }
  return output;
}

function rangeBytes(ranges: readonly IndexRange[]): number {
  return ranges.reduce(
    (total, range) => total + range.end_exclusive - range.start,
    0,
  );
}

function safeNumber(value: string): number {
  try {
    const parsed = BigInt(value);
    if (parsed < 0n) return 0;
    return Number(
      parsed > BigInt(Number.MAX_SAFE_INTEGER)
        ? Number.MAX_SAFE_INTEGER
        : parsed,
    );
  } catch {
    return 0;
  }
}
