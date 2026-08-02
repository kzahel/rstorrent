import { describe, expect, it } from "vitest";

import type { FileSet } from "../model";
import { buildScenarioSnapshot } from "./catalog";

describe("file progress demo", () => {
  it("regresses only unverified Done bytes after a hash failure and recovers", () => {
    const before = fileSetAt(34_000);
    const failed = fileSetAt(44_000);
    const recovered = fileSetAt(54_000);

    expect(total(before, "doneBytes")).toBeGreaterThan(total(failed, "doneBytes"));
    expect(total(failed, "verifiedBytes")).toBeGreaterThanOrEqual(
      total(before, "verifiedBytes"),
    );
    expect(total(recovered, "doneBytes")).toBeGreaterThan(total(before, "doneBytes"));
    expect(total(recovered, "verifiedBytes")).toBeGreaterThan(
      total(failed, "verifiedBytes"),
    );
  });
});

describe("piece map demos", () => {
  it("shows a failed attempt, a clean retry, and eventual verification", () => {
    const failed = pieceMapAt("piece-retry", 10_000);
    const retry = pieceMapAt("piece-retry", 16_000);
    const recovered = pieceMapAt("piece-retry", 25_000);

    expect(failed.active).toMatchObject([
      { id: "450:1", stage: "failed", error: "SHA-1 mismatch" },
    ]);
    expect(retry.active).toMatchObject([
      { id: "450:2", stage: "received", error: null },
    ]);
    expect(recovered.active).toEqual([]);
    expect(recovered.verified[450]).toBe(1);
  });

  it("keeps a 250,000-piece scale fixture bounded to one typed bitmap", () => {
    const map = pieceMapAt("large-swarm", 0);
    expect(map.pieceCount).toBe(250_000);
    expect(map.verified).toBeInstanceOf(Uint8Array);
    expect(map.verified).toHaveLength(250_000);
    expect(map.active).toHaveLength(6);
  });
});

function fileSetAt(elapsedMs: number): FileSet {
  const snapshot = buildScenarioSnapshot("file-progress", elapsedMs, false, 1);
  const torrentId = snapshot.torrentOrder[0];
  if (torrentId === undefined) throw new Error("file demo torrent is missing");
  const files = snapshot.filesByTorrent[torrentId];
  if (files === undefined) throw new Error("file demo catalog is missing");
  return files;
}

function total(fileSet: FileSet, field: "doneBytes" | "verifiedBytes"): bigint {
  return fileSet.order.reduce(
    (sum, id) => sum + BigInt(fileSet.rows[id]?.[field] ?? "0"),
    0n,
  );
}

function pieceMapAt(
  scenario: "piece-retry" | "large-swarm",
  elapsedMs: number,
) {
  const snapshot = buildScenarioSnapshot(scenario, elapsedMs, false, 1);
  const torrentId = snapshot.torrentOrder[0];
  if (torrentId === undefined) throw new Error("piece demo torrent is missing");
  const pieces = snapshot.piecesByTorrent[torrentId];
  if (pieces === undefined) throw new Error("piece demo map is missing");
  return pieces;
}
