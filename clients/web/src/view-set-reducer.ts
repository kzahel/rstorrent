import type {
  DiagnosticEvent,
  IndexRange,
  OpenViewSetResponse,
  UpdateBatch,
  ViewPatch,
  ViewSnapshot,
} from "./api";

export interface ViewSetState {
  viewSetId: string;
  epoch: string;
  cursor: string;
  durableRevision: string;
  views: Record<string, ViewSnapshot>;
}

export class ViewSetContinuityError extends Error {}

export function reduceOpenViewSet(response: OpenViewSetResponse): ViewSetState {
  if (response.initial.view_set_id !== response.view_set_id) {
    throw new ViewSetContinuityError("initial batch belongs to another view set");
  }
  return reduceUpdateBatch(undefined, response.initial);
}

export function reduceUpdateBatch(
  state: ViewSetState | undefined,
  batch: UpdateBatch,
): ViewSetState {
  if (batch.api_version !== 1) {
    throw new ViewSetContinuityError("unsupported application API version");
  }
  if (state !== undefined && state.viewSetId !== batch.view_set_id) {
    throw new ViewSetContinuityError("update belongs to another view set");
  }
  if (state !== undefined && state.epoch === batch.epoch) {
    if (state.cursor === batch.cursor) return state;
    if (state.cursor !== batch.base_cursor) {
      throw new ViewSetContinuityError("update does not continue the view-set cursor");
    }
    if (batch.updates.some((update) => update.type === "reset_required")) {
      throw new ViewSetContinuityError("reset did not rotate the view-set epoch");
    }
  } else {
    if (state === undefined && batch.base_cursor !== "0") {
      throw new ViewSetContinuityError("new view-set epoch does not start at cursor zero");
    }
    if (
      state !== undefined &&
      batch.updates[0]?.type !== "reset_required"
    ) {
      throw new ViewSetContinuityError("epoch changed without an explicit reset");
    }
    if (
      state !== undefined &&
      BigInt(batch.cursor) <= BigInt(state.cursor)
    ) {
      throw new ViewSetContinuityError("reset reused an old view-set cursor");
    }
  }

  const next: ViewSetState = {
    viewSetId: batch.view_set_id,
    epoch: batch.epoch,
    cursor: batch.cursor,
    durableRevision: batch.durable_revision,
    views: state?.epoch === batch.epoch ? { ...state.views } : {},
  };
  for (const update of batch.updates) {
    switch (update.type) {
      case "reset_required":
        if (update.view_id === undefined || update.view_id === null) {
          next.views = {};
        } else {
          delete next.views[update.view_id];
        }
        break;
      case "view_removed":
        delete next.views[update.view_id];
        break;
      case "snapshot":
        next.views[update.view_id] = cloneSnapshot(update.snapshot);
        break;
      case "patch": {
        const previous = next.views[update.view_id];
        if (previous === undefined) {
          throw new ViewSetContinuityError(
            `patch for unknown view ${update.view_id}`,
          );
        }
        next.views[update.view_id] = applyPatch(previous, update.patch);
        break;
      }
    }
  }
  return next;
}

function cloneSnapshot(snapshot: ViewSnapshot): ViewSnapshot {
  switch (snapshot.type) {
    case "torrent_list":
      return { ...snapshot, torrents: [...snapshot.torrents] };
    case "torrent":
      return { ...snapshot };
    case "piece_activity":
      return {
        ...snapshot,
        verified: [...snapshot.verified],
        active: [...snapshot.active],
      };
    case "session_disk":
      return {
        ...snapshot,
        pipeline: { ...snapshot.pipeline },
        pieces: [...snapshot.pieces],
      };
    case "peers":
      return { ...snapshot, peers: [...snapshot.peers] };
    case "files":
      return { ...snapshot, files: [...snapshot.files] };
    case "trackers":
      return { ...snapshot, trackers: [...snapshot.trackers] };
    case "diagnostics":
      return { ...snapshot, events: [...snapshot.events] };
  }
}

function applyPatch(snapshot: ViewSnapshot, patch: ViewPatch): ViewSnapshot {
  if (snapshot.type !== patch.type) {
    throw new ViewSetContinuityError("patch projection does not match its view");
  }
  switch (patch.type) {
    case "torrent_list": {
      if (snapshot.type !== "torrent_list") throw new Error("unreachable");
      const torrents = new Map(
        snapshot.torrents.map((torrent) => [torrent.torrent_id, torrent]),
      );
      for (const torrentId of patch.removed) torrents.delete(torrentId);
      for (const torrent of patch.upsert) {
        torrents.set(torrent.torrent_id, torrent);
      }
      return { type: "torrent_list", torrents: [...torrents.values()] };
    }
    case "torrent":
      return { type: "torrent", torrent: patch.torrent };
    case "piece_activity": {
      if (snapshot.type !== "piece_activity") throw new Error("unreachable");
      let verified = snapshot.verified;
      for (const range of patch.cleared) verified = removeRange(verified, range);
      for (const range of patch.verified) verified = insertRange(verified, range);
      const active = new Map(
        snapshot.active.map((piece) => [piece.piece_id, piece]),
      );
      for (const pieceId of patch.active_removed) active.delete(pieceId);
      for (const piece of patch.active_upsert) active.set(piece.piece_id, piece);
      return {
        type: "piece_activity",
        torrent_id: patch.torrent_id,
        piece_count: patch.piece_count,
        verified,
        active: [...active.values()],
      };
    }
    case "session_disk": {
      if (snapshot.type !== "session_disk") throw new Error("unreachable");
      const pieces = new Map(
        snapshot.pieces.map((piece) => [piece.row_id, piece]),
      );
      for (const rowId of patch.removed) pieces.delete(rowId);
      for (const piece of patch.upsert) pieces.set(piece.row_id, piece);
      return {
        type: "session_disk",
        pipeline: patch.pipeline,
        pieces: [...pieces.values()],
      };
    }
    case "peers": {
      if (snapshot.type !== "peers") throw new Error("unreachable");
      const peers = new Map(
        snapshot.peers.map((peer) => [peer.connection_id, peer]),
      );
      for (const connectionId of patch.removed) peers.delete(connectionId);
      for (const peer of patch.upsert) peers.set(peer.connection_id, peer);
      return {
        type: "peers",
        torrent_id: patch.torrent_id,
        peers: [...peers.values()],
      };
    }
    case "files": {
      if (snapshot.type !== "files") throw new Error("unreachable");
      const files = new Map(
        snapshot.files.map((file) => [file.file_id, file]),
      );
      for (const fileId of patch.removed) files.delete(fileId);
      for (const file of patch.upsert) files.set(file.file_id, file);
      return {
        type: "files",
        torrent_id: patch.torrent_id,
        state: snapshot.state,
        filesystem_content_base: snapshot.filesystem_content_base,
        files: [...files.values()],
      };
    }
    case "trackers": {
      if (snapshot.type !== "trackers") throw new Error("unreachable");
      const trackers = new Map(
        snapshot.trackers.map((tracker) => [tracker.tracker_id, tracker]),
      );
      for (const trackerId of patch.removed) trackers.delete(trackerId);
      for (const tracker of patch.upsert) {
        trackers.set(tracker.tracker_id, tracker);
      }
      return {
        type: "trackers",
        torrent_id: patch.torrent_id,
        state: snapshot.state,
        trackers: [...trackers.values()],
      };
    }
    case "diagnostics": {
      if (snapshot.type !== "diagnostics") throw new Error("unreachable");
      return {
        type: "diagnostics",
        events: mergeDiagnostics(snapshot.events, patch.events),
        dropped_count: patch.dropped_count,
      };
    }
  }
}

function mergeDiagnostics(
  previous: readonly DiagnosticEvent[],
  updates: readonly DiagnosticEvent[],
): DiagnosticEvent[] {
  const events = new Map(previous.map((event) => [event.sequence, event]));
  for (const event of updates) events.set(event.sequence, event);
  return [...events.values()]
    .sort((left, right) =>
      BigInt(left.sequence) < BigInt(right.sequence) ? -1 : 1,
    )
    .slice(-512);
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
      output.push({ start: range.start, end_exclusive: removed.start });
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
