import type {
  ActivePiece,
  DiagnosticEvent,
  IndexRange,
  TorrentView,
  ViewPatch,
  ViewSnapshot,
  ViewUpdate,
} from "./generated/contract";

export interface PieceActivityState {
  torrentId: string;
  pieceCount: number;
  verified: IndexRange[];
  active: ActivePiece | null;
}

interface StreamPosition {
  epoch: string;
  sequence: string;
  revision: string;
}

export interface ApplicationViewState {
  torrents: Record<string, TorrentView>;
  pieces: Record<string, PieceActivityState>;
  diagnostics: DiagnosticEvent[];
  diagnosticDropped: string;
  streams: Record<string, StreamPosition>;
}

export class ContinuityError extends Error {}
export class ResetRequiredError extends Error {}

export function emptyApplicationViewState(): ApplicationViewState {
  return {
    torrents: {},
    pieces: {},
    diagnostics: [],
    diagnosticDropped: "0",
    streams: {},
  };
}

export function reduceViewUpdate(
  state: ApplicationViewState,
  update: ViewUpdate,
): ApplicationViewState {
  const current = state.streams[update.stream_id];
  if (update.type === "reset_required") {
    throw new ResetRequiredError(update.reason);
  }
  if (update.type === "patch") {
    if (
      current === undefined ||
      current.epoch !== update.epoch ||
      BigInt(update.sequence) !== BigInt(current.sequence) + 1n ||
      current.revision !== update.base_revision
    ) {
      throw new ContinuityError("view patch does not continue its stream");
    }
  }

  const next: ApplicationViewState = {
    torrents: { ...state.torrents },
    pieces: { ...state.pieces },
    diagnostics: [...state.diagnostics],
    diagnosticDropped: state.diagnosticDropped,
    streams: {
      ...state.streams,
      [update.stream_id]: {
        epoch: update.epoch,
        sequence: update.sequence,
        revision: update.revision,
      },
    },
  };
  if (update.type === "snapshot") {
    applySnapshot(next, update.snapshot);
  } else {
    applyPatch(next, update.patch);
  }
  return next;
}

function applySnapshot(
  state: ApplicationViewState,
  snapshot: ViewSnapshot,
): void {
  switch (snapshot.type) {
    case "torrent_list":
      state.torrents = Object.fromEntries(
        snapshot.torrents.map((torrent) => [torrent.torrent_id, torrent]),
      );
      break;
    case "torrent":
      if (snapshot.torrent !== null) {
        state.torrents[snapshot.torrent.torrent_id] = snapshot.torrent;
      }
      break;
    case "piece_activity":
      state.pieces[snapshot.torrent_id] = {
        torrentId: snapshot.torrent_id,
        pieceCount: snapshot.piece_count,
        verified: snapshot.verified,
        active: snapshot.active,
      };
      break;
    case "diagnostics":
      state.diagnostics = snapshot.events.slice(-512);
      state.diagnosticDropped = snapshot.dropped_count;
      break;
  }
}

function applyPatch(state: ApplicationViewState, patch: ViewPatch): void {
  switch (patch.type) {
    case "torrent_list":
      for (const torrentId of patch.removed) delete state.torrents[torrentId];
      for (const torrent of patch.upsert) {
        state.torrents[torrent.torrent_id] = torrent;
      }
      break;
    case "torrent":
      if (patch.torrent !== null) {
        state.torrents[patch.torrent.torrent_id] = patch.torrent;
      }
      break;
    case "piece_activity": {
      const previous = state.pieces[patch.torrent_id];
      let verified = previous?.verified ?? [];
      for (const range of patch.cleared) {
        verified = removeRange(verified, range);
      }
      for (const range of patch.verified) {
        verified = insertRange(verified, range);
      }
      state.pieces[patch.torrent_id] = {
        torrentId: patch.torrent_id,
        pieceCount: patch.piece_count,
        verified,
        active: patch.active,
      };
      break;
    }
    case "diagnostics": {
      const bySequence = new Map(
        state.diagnostics.map((event) => [event.sequence, event]),
      );
      for (const event of patch.events) bySequence.set(event.sequence, event);
      state.diagnostics = [...bySequence.values()]
        .sort((left, right) =>
          BigInt(left.sequence) < BigInt(right.sequence) ? -1 : 1,
        )
        .slice(-512);
      state.diagnosticDropped = patch.dropped_count;
      break;
    }
  }
}

function insertRange(
  ranges: readonly IndexRange[],
  inserted: IndexRange,
): IndexRange[] {
  const output: IndexRange[] = [];
  let merged = { ...inserted };
  let placed = false;
  for (const range of ranges) {
    if (range.end_exclusive < merged.start) {
      output.push(range);
    } else if (merged.end_exclusive < range.start) {
      if (!placed) {
        output.push(merged);
        placed = true;
      }
      output.push(range);
    } else {
      merged = {
        start: Math.min(merged.start, range.start),
        end_exclusive: Math.max(merged.end_exclusive, range.end_exclusive),
      };
    }
  }
  if (!placed) output.push(merged);
  return output;
}

function removeRange(
  ranges: readonly IndexRange[],
  removed: IndexRange,
): IndexRange[] {
  const output: IndexRange[] = [];
  for (const range of ranges) {
    if (
      range.end_exclusive <= removed.start ||
      range.start >= removed.end_exclusive
    ) {
      output.push(range);
      continue;
    }
    if (range.start < removed.start) {
      output.push({
        start: range.start,
        end_exclusive: removed.start,
      });
    }
    if (range.end_exclusive > removed.end_exclusive) {
      output.push({
        start: removed.end_exclusive,
        end_exclusive: range.end_exclusive,
      });
    }
  }
  return output;
}
