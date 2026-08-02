import { describe, expect, it } from "vitest";

import type { TorrentView, UpdateBatch } from "./api";
import {
  reduceUpdateBatch,
  ViewSetContinuityError,
  type ViewSetState,
} from "./view-set-reducer";

const torrentId = "000102030405060708090a0b0c0d0e0f10111213";

function torrent(verified: number): TorrentView {
  return {
    torrent_id: torrentId,
    state: verified === 3 ? "complete" : "downloading",
    storage_state: verified === 3 ? "published" : "staging",
    metadata_available: true,
    piece_count: 3,
    verified_piece_count: verified,
    requested_bytes: "16384",
    received_bytes: "16384",
    stored_bytes: "16384",
    active_peer_connections: 0,
    payload_download_rate_bytes: "0",
    progress: {
      disposition: verified === 3 ? "inactive" : "active",
      phase: verified === 3 ? "publication" : "transfer",
      reason: verified === 3 ? "complete" : "transferring_pieces",
      actions: [],
    },
    archived: false,
    delete_managed_data_supported: true,
  };
}

function batch(
  baseCursor: string,
  cursor: string,
  updates: UpdateBatch["updates"],
  epoch = "7",
): UpdateBatch {
  return {
    api_version: 1,
    view_set_id: "vs_000102030405060708090a0b0c0d0e0f",
    epoch,
    base_cursor: baseCursor,
    cursor,
    durable_revision: cursor,
    updates,
  };
}

describe("view-set reducer", () => {
  it("replaces the hash-only row when verified metadata supplies a name", () => {
    let state = reduceUpdateBatch(
      undefined,
      batch("0", "1", [
        {
          type: "snapshot",
          view_id: "library",
          snapshot: { type: "torrent_list", torrents: [torrent(0)] },
        },
      ]),
    );
    state = reduceUpdateBatch(
      state,
      batch("1", "2", [
        {
          type: "patch",
          view_id: "library",
          patch: {
            type: "torrent_list",
            upsert: [{ ...torrent(0), display_name: "Verified torrent" }],
            removed: [],
          },
        },
      ]),
    );
    expect(state.views.library).toMatchObject({
      type: "torrent_list",
      torrents: [{ display_name: "Verified torrent" }],
    });
  });

  it("applies complete keyed file rows without losing catalog metadata", () => {
    const first = {
      file_id: "0",
      file_index: 0,
      path: ["video", "movie.mkv"],
      length_bytes: "9007199254740993",
      torrent_offset_bytes: "0",
      first_piece: 0,
      last_piece: 9,
      selection: "wanted" as const,
      padding: false,
      done_bytes: "16384",
      verified_bytes: "0",
    };
    let state = reduceUpdateBatch(
      undefined,
      batch("0", "1", [
        {
          type: "snapshot",
          view_id: "files",
          snapshot: {
            type: "files",
            torrent_id: torrentId,
            state: "available",
            filesystem_content_base: "/tmp/content",
            files: [first],
          },
        },
      ]),
    );
    state = reduceUpdateBatch(
      state,
      batch("1", "2", [
        {
          type: "patch",
          view_id: "files",
          patch: {
            type: "files",
            torrent_id: torrentId,
            upsert: [{ ...first, done_bytes: "32768", verified_bytes: "16384" }],
            removed: [],
          },
        },
      ]),
    );
    expect(state.views.files).toMatchObject({
      type: "files",
      state: "available",
      filesystem_content_base: "/tmp/content",
      files: [{ file_id: "0", done_bytes: "32768", verified_bytes: "16384" }],
    });
  });

  it("replaces disk pipeline state and applies keyed piece changes", () => {
    const pipeline = diskPipeline("normal");
    const first = diskPiece("torrent-a:3:1", 3, "receiving");
    let state = reduceUpdateBatch(
      undefined,
      batch("0", "1", [
        {
          type: "snapshot",
          view_id: "disk",
          snapshot: { type: "session_disk", pipeline, pieces: [first] },
        },
      ]),
    );
    const replacement = diskPiece("torrent-a:4:1", 4, "writing");
    state = reduceUpdateBatch(
      state,
      batch("1", "2", [
        {
          type: "patch",
          view_id: "disk",
          patch: {
            type: "session_disk",
            pipeline: diskPipeline("backpressured"),
            upsert: [replacement],
            removed: [first.row_id],
          },
        },
      ]),
    );
    expect(state.views.disk).toMatchObject({
      type: "session_disk",
      pipeline: { pressure: "backpressured", intake_backpressured: true },
      pieces: [{ row_id: "torrent-a:4:1", piece_index: 4, stage: "writing" }],
    });
  });

  it("applies compact verified changes and keyed active piece retries", () => {
    const first = activePiece(0, 1, "received");
    let state = reduceUpdateBatch(
      undefined,
      batch("0", "1", [
        {
          type: "snapshot",
          view_id: "pieces",
          snapshot: {
            type: "piece_activity",
            torrent_id: torrentId,
            piece_count: 3,
            verified: [],
            active: [first],
          },
        },
      ]),
    );
    state = reduceUpdateBatch(
      state,
      batch("1", "2", [
        {
          type: "patch",
          view_id: "pieces",
          patch: {
            type: "piece_activity",
            torrent_id: torrentId,
            piece_count: 3,
            verified: [{ start: 1, end_exclusive: 2 }],
            cleared: [],
            active_upsert: [activePiece(0, 2, "requested")],
            active_removed: [first.piece_id],
          },
        },
      ]),
    );
    expect(state.views.pieces).toMatchObject({
      type: "piece_activity",
      verified: [{ start: 1, end_exclusive: 2 }],
      active: [{ piece_id: "0:2", attempt: 2, stage: "requested" }],
    });
  });

  it("reduces snapshots, keyed patches, removals, and later upserts", () => {
    let state = reduceUpdateBatch(
      undefined,
      batch("0", "1", [
        {
          type: "snapshot",
          view_id: "library",
          snapshot: { type: "torrent_list", torrents: [torrent(0)] },
        },
      ]),
    );
    state = reduceUpdateBatch(
      state,
      batch("1", "2", [
        {
          type: "patch",
          view_id: "library",
          patch: { type: "torrent_list", upsert: [], removed: [torrentId] },
        },
      ]),
    );
    expect(state.views.library).toEqual({ type: "torrent_list", torrents: [] });
    state = reduceUpdateBatch(
      state,
      batch("2", "3", [
        {
          type: "patch",
          view_id: "library",
          patch: { type: "torrent_list", upsert: [torrent(3)], removed: [] },
        },
      ]),
    );
    expect(state.views.library).toEqual({
      type: "torrent_list",
      torrents: [torrent(3)],
    });
    state = reduceUpdateBatch(
      state,
      batch("3", "4", [{ type: "view_removed", view_id: "library" }]),
    );
    expect(state.views.library).toBeUndefined();
  });

  it("treats an already-applied replay as idempotent", () => {
    const initial = batch("0", "1", [
      {
        type: "snapshot",
        view_id: "library",
        snapshot: { type: "torrent_list", torrents: [] },
      },
    ]);
    const state = reduceUpdateBatch(undefined, initial);
    expect(reduceUpdateBatch(state, initial)).toBe(state);
  });

  it("clears stale state and accepts snapshots after an epoch reset", () => {
    const state = reduceUpdateBatch(
      undefined,
      batch("0", "1", [
        {
          type: "snapshot",
          view_id: "library",
          snapshot: { type: "torrent_list", torrents: [torrent(0)] },
        },
      ]),
    );
    const reset = reduceUpdateBatch(
      state,
      batch(
        "2",
        "3",
        [
          { type: "reset_required", reason: "queue_overflow" },
          {
            type: "snapshot",
            view_id: "pieces",
            snapshot: {
              type: "piece_activity",
              torrent_id: torrentId,
              piece_count: 3,
              verified: [{ start: 0, end_exclusive: 1 }],
              active: [],
            },
          },
        ],
        "8",
      ),
    );
    expect(reset.views.library).toBeUndefined();
    expect(reset.views.pieces?.type).toBe("piece_activity");
    expect(reset.cursor).toBe("3");
    expect(reset.deliveryResetCount).toBe(1);
    expect(reset.lastDeliveryResetReason).toBe("queue_overflow");
  });

  it("rejects gaps, wrong identities, and patches without snapshots", () => {
    const state: ViewSetState = {
      viewSetId: "vs_000102030405060708090a0b0c0d0e0f",
      epoch: "7",
      cursor: "1",
      durableRevision: "1",
      views: {},
      deliveryResetCount: 0,
      lastDeliveryResetReason: null,
    };
    expect(() =>
      reduceUpdateBatch(state, batch("2", "3", [])),
    ).toThrow(ViewSetContinuityError);
    expect(() =>
      reduceUpdateBatch(
        state,
        batch("1", "2", [
          {
            type: "patch",
            view_id: "missing",
            patch: { type: "torrent_list", upsert: [], removed: [] },
          },
        ]),
      ),
    ).toThrow(ViewSetContinuityError);
  });
});

function diskPipeline(pressure: "normal" | "backpressured") {
  return {
    pressure,
    intake_backpressured: pressure === "backpressured",
    sample_millis: "1000",
    resident_limit_bytes: "1048576",
    resident_high_watermark_bytes: "786432",
    resident_low_watermark_bytes: "524288",
    requested_bytes: "65536",
    resident_bytes: "32768",
    queued_write_bytes: "16384",
    writing_bytes: "16384",
    hashing_bytes: "0",
    storage_jobs_pending: "1",
    received_bytes_total: "32768",
    stored_bytes_total: "16384",
    verified_bytes_total: "0",
    receive_rate_bytes: "32768",
    write_rate_bytes: "16384",
    hash_rate_bytes: "0",
    write_operations_started: "1",
    write_operations_completed: "0",
    hash_operations_started: "0",
    hash_operations_completed: "0",
    write_queue_wait_micros: "100",
    write_queue_wait_max_micros: "100",
    write_service_micros: "0",
    write_service_max_micros: "0",
    hash_queue_wait_micros: "0",
    hash_queue_wait_max_micros: "0",
    hash_service_micros: "0",
    hash_service_max_micros: "0",
    pressure_transition_count: pressure === "backpressured" ? "1" : "0",
    backpressured_millis_total: "0",
  };
}

function activePiece(
  pieceIndex: number,
  attempt: number,
  stage: "requested" | "received",
) {
  return {
    piece_id: `${pieceIndex}:${attempt}`,
    piece_index: pieceIndex,
    attempt,
    piece_length: 262144,
    stage,
    requested: stage === "requested" ? [{ start: 0, end_exclusive: 16384 }] : [],
    received: stage === "received" ? [{ start: 0, end_exclusive: 16384 }] : [],
    stored: [],
    age_millis: "100",
  };
}

function diskPiece(
  rowId: string,
  pieceIndex: number,
  stage: "receiving" | "writing",
) {
  return {
    row_id: rowId,
    torrent_id: torrentId,
    torrent_name: "Test torrent",
    piece_index: pieceIndex,
    piece_length: 262144,
    attempt: 1,
    stage,
    requested_bytes: "16384",
    received_bytes: "16384",
    stored_bytes: "0",
    stage_age_millis: "10",
    age_millis: "20",
  };
}
