import type { PieceLifecycleStage, PieceMapSet } from "../model";

export const MAX_VISUAL_CELLS = 16_384;
export const MAX_CANVAS_CSS_HEIGHT = 1_024;
export const MAX_DEVICE_PIXEL_RATIO = 3;

export const enum PieceCellState {
  Missing = 1,
  Mixed = 2,
  Verified = 3,
  Requested = 4,
  Received = 5,
  Stored = 6,
  Hashing = 7,
  Failed = 8,
}

export interface PieceMapGeometry {
  readonly width: number;
  readonly height: number;
  readonly columns: number;
  readonly rows: number;
  readonly cellSize: number;
  readonly gap: number;
  readonly visualCellCount: number;
}

export interface PieceBuckets {
  readonly states: Uint8Array;
  readonly verifiedCount: number;
  readonly activeCount: number;
}

const CELL_SIZE = 7;
const CELL_GAP = 1;
const OVERVIEW_CSS_HEIGHT = 320;

export function pieceMapGeometry(
  availableWidth: number,
  pieceCount: number,
): PieceMapGeometry {
  const width = Math.max(CELL_SIZE, Math.floor(availableWidth));
  const stride = CELL_SIZE + CELL_GAP;
  const columns = Math.max(1, Math.floor((width + CELL_GAP) / stride));
  const maximumRows = Math.max(
    1,
    Math.floor(Math.min(OVERVIEW_CSS_HEIGHT, MAX_CANVAS_CSS_HEIGHT) / stride),
  );
  const visualCellCount = Math.min(
    Math.max(0, pieceCount),
    MAX_VISUAL_CELLS,
    columns * maximumRows,
  );
  const rows = visualCellCount === 0 ? 0 : Math.ceil(visualCellCount / columns);
  return {
    width,
    height: rows * stride,
    columns,
    rows,
    cellSize: CELL_SIZE,
    gap: CELL_GAP,
    visualCellCount,
  };
}

export function buildPieceBuckets(
  pieces: PieceMapSet,
  visualCellCount: number,
): PieceBuckets {
  if (pieces.pieceCount === 0 || visualCellCount === 0) {
    return {
      states: new Uint8Array(0),
      verifiedCount: 0,
      activeCount: pieces.active.length,
    };
  }
  const states = new Uint8Array(visualCellCount);
  const pieceCounts = new Uint32Array(visualCellCount);
  const verifiedCounts = new Uint32Array(visualCellCount);
  let verifiedCount = 0;
  for (let pieceIndex = 0; pieceIndex < pieces.pieceCount; pieceIndex += 1) {
    const cell = bucketForPiece(pieceIndex, pieces.pieceCount, visualCellCount);
    pieceCounts[cell] = (pieceCounts[cell] ?? 0) + 1;
    if (pieces.verified[pieceIndex] === 1) {
      verifiedCount += 1;
      verifiedCounts[cell] = (verifiedCounts[cell] ?? 0) + 1;
    }
  }
  for (let cell = 0; cell < visualCellCount; cell += 1) {
    const complete = pieceCounts[cell] ?? 0;
    const verified = verifiedCounts[cell] ?? 0;
    states[cell] =
      verified === 0
        ? PieceCellState.Missing
        : verified === complete
          ? PieceCellState.Verified
          : PieceCellState.Mixed;
  }
  for (const piece of pieces.active) {
    if (piece.pieceIndex < 0 || piece.pieceIndex >= pieces.pieceCount) continue;
    const cell = bucketForPiece(
      piece.pieceIndex,
      pieces.pieceCount,
      visualCellCount,
    );
    states[cell] = Math.max(states[cell] ?? 0, stageCellState(piece.stage));
  }
  return { states, verifiedCount, activeCount: pieces.active.length };
}

export function bucketForPiece(
  pieceIndex: number,
  pieceCount: number,
  visualCellCount: number,
): number {
  if (pieceCount <= 0 || visualCellCount <= 0) return 0;
  return Math.min(
    visualCellCount - 1,
    Math.floor((pieceIndex * visualCellCount) / pieceCount),
  );
}

function stageCellState(stage: PieceLifecycleStage): PieceCellState {
  switch (stage) {
    case "requested":
      return PieceCellState.Requested;
    case "received":
      return PieceCellState.Received;
    case "stored":
      return PieceCellState.Stored;
    case "hashing":
      return PieceCellState.Hashing;
    case "failed":
      return PieceCellState.Failed;
  }
}
