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
    progress: {
      disposition: verified === 3 ? "inactive" : "active",
      phase: verified === 3 ? "publication" : "transfer",
      reason: verified === 3 ? "complete" : "transferring_pieces",
      actions: [],
    },
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
        "0",
        "1",
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
              active: null,
            },
          },
        ],
        "8",
      ),
    );
    expect(reset.views.library).toBeUndefined();
    expect(reset.views.pieces?.type).toBe("piece_activity");
  });

  it("rejects gaps, wrong identities, and patches without snapshots", () => {
    const state: ViewSetState = {
      viewSetId: "vs_000102030405060708090a0b0c0d0e0f",
      epoch: "7",
      cursor: "1",
      durableRevision: "1",
      views: {},
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
