import { describe, expect, it } from "vitest";

import type { PieceMapSet } from "../model";
import {
  bucketForPiece,
  buildPieceBuckets,
  MAX_CANVAS_CSS_HEIGHT,
  MAX_VISUAL_CELLS,
  PieceCellState,
  pieceMapGeometry,
} from "./PieceMap";

describe("piece map geometry", () => {
  it("bounds a 250,000-piece torrent without losing the final index", () => {
    const geometry = pieceMapGeometry(1_024, 250_000);
    expect(geometry.visualCellCount).toBeLessThanOrEqual(MAX_VISUAL_CELLS);
    expect(geometry.height).toBeLessThanOrEqual(MAX_CANVAS_CSS_HEIGHT);
    expect(bucketForPiece(249_999, 250_000, geometry.visualCellCount)).toBe(
      geometry.visualCellCount - 1,
    );
  });

  it("keeps complete, mixed, active, and failed buckets truthful", () => {
    const pieces: PieceMapSet = {
      torrentId: "test",
      pieceCount: 8,
      verified: Uint8Array.from([1, 1, 1, 0, 0, 0, 0, 0]),
      active: [
        activePiece(4, "received"),
        activePiece(6, "failed"),
      ],
      revision: 1,
    };

    const buckets = buildPieceBuckets(pieces, 4);

    expect([...buckets.states]).toEqual([
      PieceCellState.Verified,
      PieceCellState.Mixed,
      PieceCellState.Received,
      PieceCellState.Failed,
    ]);
    expect(buckets.verifiedCount).toBe(3);
    expect(buckets.activeCount).toBe(2);
  });
});

function activePiece(
  pieceIndex: number,
  stage: "received" | "failed",
) {
  return {
    id: `${pieceIndex}:1`,
    pieceIndex,
    attempt: 1,
    pieceLength: 16_384,
    stage,
    requestedBytes: 16_384,
    receivedBytes: 8_192,
    storedBytes: 0,
    ageMillis: 100,
    error: stage === "failed" ? "SHA-1 mismatch" : null,
  } as const;
}
