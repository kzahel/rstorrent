import { describe, expect, it } from "vitest";

import type { ViewUpdate } from "./api";
import rustTrace from "./fixtures/reactive-trace.json";
import {
  ContinuityError,
  emptyApplicationViewState,
  reduceViewUpdate,
} from "./reducer";

const torrentId = "000102030405060708090a0b0c0d0e0f10111213";

describe("reactive reducer", () => {
  it("converges on the Rust-produced trace", () => {
    let state = emptyApplicationViewState();
    for (const update of rustTrace as ViewUpdate[]) {
      state = reduceViewUpdate(state, update);
    }
    expect(state.pieces[torrentId]?.verified).toEqual([
      { start: 65_537, end_exclusive: 70_000 },
      { start: 900_000, end_exclusive: 900_001 },
    ]);
    expect(state.pieces[torrentId]?.active[0]?.piece_length).toBe(33_554_432);
  });

  it("keeps indices beyond 65535 and checks continuity", () => {
    const initial: ViewUpdate = {
      contract_version: 2,
      stream_id: "1",
      epoch: "2",
      sequence: "1",
      base_revision: "7",
      revision: "7",
      type: "snapshot",
      snapshot: {
        type: "piece_activity",
        torrent_id: torrentId,
        piece_count: 1_000_000,
        verified: [{ start: 65_536, end_exclusive: 70_000 }],
        active: [],
      },
    };
    const patched: ViewUpdate = {
      contract_version: 2,
      stream_id: "1",
      epoch: "2",
      sequence: "2",
      base_revision: "7",
      revision: "7",
      type: "patch",
      patch: {
        type: "piece_activity",
        torrent_id: torrentId,
        piece_count: 1_000_000,
        verified: [{ start: 900_000, end_exclusive: 900_001 }],
        cleared: [{ start: 65_536, end_exclusive: 65_537 }],
        active_upsert: [{
          piece_id: "900001:1",
          piece_index: 900_001,
          attempt: 1,
          piece_length: 33_554_432,
          stage: "requested",
          requested: [{ start: 0, end_exclusive: 16_384 }],
          received: [],
          stored: [],
          age_millis: "125",
        }],
        active_removed: [],
      },
    };
    const first = reduceViewUpdate(emptyApplicationViewState(), initial);
    const second = reduceViewUpdate(first, patched);
    expect(second.pieces[torrentId]?.verified).toEqual([
      { start: 65_537, end_exclusive: 70_000 },
      { start: 900_000, end_exclusive: 900_001 },
    ]);
    expect(second.pieces[torrentId]?.active[0]?.piece_index).toBe(900_001);

    expect(() =>
      reduceViewUpdate(second, { ...patched, sequence: "4" }),
    ).toThrow(ContinuityError);
  });

  it("reduces bounded diagnostic snapshots and patches", () => {
    const diagnostic = {
      sequence: "7",
      timestamp_millis: "1000",
      severity: "warning" as const,
      category: "discovery" as const,
      code: "discovery_exhausted",
      torrent_id: torrentId,
      summary: "No discovery source",
      context: [],
    };
    const initial: ViewUpdate = {
      contract_version: 2,
      stream_id: "9",
      epoch: "2",
      sequence: "1",
      base_revision: "0",
      revision: "0",
      type: "snapshot",
      snapshot: {
        type: "diagnostics",
        events: [diagnostic],
        dropped_count: "3",
      },
    };
    const patched: ViewUpdate = {
      ...initial,
      sequence: "2",
      type: "patch",
      patch: {
        type: "diagnostics",
        events: [
          { ...diagnostic, sequence: "8", code: "retry_scheduled" },
        ],
        dropped_count: "3",
      },
    };
    const state = reduceViewUpdate(
      reduceViewUpdate(emptyApplicationViewState(), initial),
      patched,
    );
    expect(state.diagnostics.map((event) => event.sequence)).toEqual(["7", "8"]);
    expect(state.diagnosticDropped).toBe("3");
  });
});
