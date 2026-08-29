import { describe, expect, it } from "vitest";

import fixture from "./fixtures/view-set-trace.json";
import {
  ContractError,
  decodeOpenViewSetResponse,
  decodeUpdateBatch,
} from "./validation";
import { reduceOpenViewSet, reduceUpdateBatch } from "./view-set-reducer";

describe("Rust view-set fixture", () => {
  it("validates and converges across a generated patch and reset", () => {
    const open = decodeOpenViewSetResponse(JSON.stringify(fixture.open));
    let state = reduceOpenViewSet(open);
    for (const encoded of fixture.updates) {
      const update = decodeUpdateBatch(JSON.stringify(encoded));
      state = reduceUpdateBatch(state, update);
    }
    expect(state.epoch).toBe("11");
    expect(state.cursor).toBe("3");
    expect(state.views.library).toMatchObject({
      type: "torrent_list",
      torrents: [
        {
          verified_piece_count: 3,
          state: "complete",
          required_payload_bytes: "49152",
          remaining_payload_bytes: "0",
          eta_payload_download_rate_bytes: "0",
          eta: { state: "unavailable" },
        },
      ],
    });
  });

  it("accepts additive fields and generated storage states", () => {
    const encoded = JSON.stringify({
      ...fixture.updates[0],
      future_server_field: { enabled: true },
    }).replace('"storage_state":"available"', '"storage_state":"unavailable"');
    expect(decodeUpdateBatch(encoded).updates[0]?.type).toBe("patch");
  });

  it("accepts required nullable ETA work before metadata", () => {
    const pending = JSON.parse(JSON.stringify(fixture.open)) as {
      initial: {
        updates: Array<{
          snapshot?: { torrents?: Array<Record<string, unknown>> };
        }>;
      };
    };
    const torrent = pending.initial.updates[0]?.snapshot?.torrents?.[0];
    if (torrent === undefined) throw new Error("fixture torrent is missing");
    torrent.required_payload_bytes = null;
    torrent.remaining_payload_bytes = null;
    torrent.eta_payload_download_rate_bytes = "0";
    torrent.eta = { state: "unavailable" };
    expect(
      decodeOpenViewSetResponse(JSON.stringify(pending)).initial.updates,
    ).toHaveLength(1);
  });

  it("rejects unknown generated enum and tagged variants", () => {
    const unknownStorage = JSON.stringify(fixture.updates[0]).replace(
      '"storage_state":"available"',
      '"storage_state":"invented"',
    );
    expect(() => decodeUpdateBatch(unknownStorage)).toThrow(ContractError);

    const unknownUpdate = JSON.parse(
      JSON.stringify(fixture.updates[0]),
    ) as { updates: Array<Record<string, unknown>> };
    const first = unknownUpdate.updates[0];
    if (first === undefined) throw new Error("fixture update missing");
    first.type = "invented";
    expect(() => decodeUpdateBatch(JSON.stringify(unknownUpdate))).toThrow(
      ContractError,
    );
  });
});
