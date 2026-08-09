import { describe, expect, it } from "vitest";

import type { FileSet } from "../model";
import { buildScenarioSnapshot } from "./catalog";

describe("torrent ETA demos", () => {
  it("provides every typed state explicitly", () => {
    expect(primaryEta("healthy-download", 0)).toEqual({ state: "unavailable" });
    expect(primaryEta("healthy-download", 7_500)).toEqual({
      state: "warming_up",
    });
    expect(primaryEta("healthy-download", 42_000)).toMatchObject({
      state: "estimate",
    });
    expect(primaryEta("swarm-lifecycle", 11_000)).toEqual({ state: "stalled" });
  });
});

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

describe("DHT observatory demo", () => {
  it("retains every shape-changing lifecycle and routing state", () => {
    const offline = dhtAt(0);
    expect(offline.dht).toMatchObject({
      lifecycle: "offline",
      network_policy: "offline",
    });
    expect(offline.dht?.families.find((family) => family.family === "ipv4")).toMatchObject({
      routing_nodes: 0,
    });
    expect(dhtAt(4_000).dht?.lifecycle).toBe("bootstrap_empty");
    expect(dhtAt(8_000).dht).toMatchObject({
      lifecycle: "participating",
    });
    expect(
      dhtAt(8_000).dht?.families.find((family) => family.family === "ipv4"),
    ).toMatchObject({
      routing_nodes: 14,
      occupied_buckets: 4,
      deepest_shared_prefix_bits: 24,
    });

    const active = dhtAt(30_000).dht;
    expect(active).toMatchObject({
      active_lookups: 1,
    });
    const activeIpv4 = active?.families.find((family) => family.family === "ipv4");
    expect(activeIpv4).toMatchObject({
      routing_nodes: 171,
      occupied_buckets: 25,
      deepest_shared_prefix_bits: 24,
    });
    expect(activeIpv4?.buckets).toHaveLength(160);
    expect(activeIpv4?.buckets.map((bucket) => bucket.bucket_index)).toEqual(
      Array.from({ length: 160 }, (_, index) => index),
    );
    expect(active?.lookups[0]).toMatchObject({
      closest_responded_prefix_bits: 24,
      responded_candidates: 44,
    });

    expect(dhtAt(42_000).dht).toMatchObject({
      malformed_received: "3",
      rate_limited: "17",
      active_lookups: 0,
    });
    expect(
      dhtAt(50_000).dht?.families.find((family) => family.family === "ipv4"),
    ).toMatchObject({
      routing_nodes: 172,
      occupied_buckets: 26,
      deepest_shared_prefix_bits: 39,
    });
    expect(dhtAt(62_000).viewStatus.dht.status).toBe("stale");
    expect(dhtAt(68_000).dht).toMatchObject({
      lifecycle: "inactive",
      active_transactions: 0,
      active_lookups: 0,
    });
    expect(
      dhtAt(68_000).dht?.families.find((family) => family.family === "ipv4"),
    ).toMatchObject({ routing_nodes: 0 });
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

function dhtAt(elapsedMs: number) {
  return buildScenarioSnapshot("dht-observatory", elapsedMs, false, 1);
}

function primaryEta(
  scenario: "healthy-download" | "swarm-lifecycle",
  elapsedMs: number,
) {
  const snapshot = buildScenarioSnapshot(scenario, elapsedMs, false, 1);
  return snapshot.torrents[snapshot.torrentOrder[0]!]?.eta;
}
