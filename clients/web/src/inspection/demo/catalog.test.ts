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
