import { describe, expect, it } from "vitest";

import type { ViewSnapshot } from "../../api";
import { mapPieceActivity, type MappedPieceActivity } from "./pieces";

type PieceActivity = Extract<ViewSnapshot, { type: "piece_activity" }>;

describe("piece activity projection", () => {
  it("updates the typed verified bitmap in place within one epoch", () => {
    const first = pieceActivity([{ start: 0, end_exclusive: 2 }, { start: 4, end_exclusive: 5 }]);
    const firstValue = mapPieceActivity(first, null, "epoch-1");
    const previous: MappedPieceActivity = {
      epoch: "epoch-1",
      source: first,
      value: firstValue,
    };
    const next = pieceActivity([{ start: 1, end_exclusive: 4 }]);

    const nextValue = mapPieceActivity(next, previous, "epoch-1");

    expect(nextValue.verified).toBe(firstValue.verified);
    expect([...nextValue.verified]).toEqual([0, 1, 1, 1, 0, 0]);
    expect(nextValue.revision).toBe(2);
    expect(nextValue.active[0]).toMatchObject({
      id: "3:2",
      pieceIndex: 3,
      attempt: 2,
      requestedBytes: 16_384,
      receivedBytes: 8_192,
      storedBytes: 0,
    });
  });

  it("rebuilds the bitmap when a recovered view-set changes epoch", () => {
    const first = pieceActivity([{ start: 0, end_exclusive: 3 }]);
    const firstValue = mapPieceActivity(first, null, "epoch-1");
    const replacement = pieceActivity([{ start: 4, end_exclusive: 6 }]);

    const replacementValue = mapPieceActivity(
      replacement,
      { epoch: "epoch-1", source: first, value: firstValue },
      "epoch-2",
    );

    expect(replacementValue.verified).not.toBe(firstValue.verified);
    expect([...replacementValue.verified]).toEqual([0, 0, 0, 0, 1, 1]);
    expect(replacementValue.revision).toBe(1);
  });
});

function pieceActivity(verified: PieceActivity["verified"]): PieceActivity {
  return {
    type: "piece_activity",
    torrent_id: "000102030405060708090a0b0c0d0e0f10111213",
    piece_count: 6,
    verified,
    active: [
      {
        piece_id: "3:2",
        piece_index: 3,
        attempt: 2,
        piece_length: 16_384,
        stage: "received",
        requested: [{ start: 0, end_exclusive: 16_384 }],
        received: [{ start: 0, end_exclusive: 8_192 }],
        stored: [],
        age_millis: "1200",
      },
    ],
  };
}
