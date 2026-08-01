import { describe, expect, it } from "vitest";

import type { InspectionSnapshot, TorrentRow } from "./model";
import { createInspectionStore } from "./state";

describe("inspection store", () => {
  it("preserves unrelated row references across keyed patches", () => {
    const store = createInspectionStore();
    const first = row("first", "downloading");
    const second = row("second", "paused");
    store.getState().applyUpdate({
      type: "snapshot",
      snapshot: snapshot([first, second]),
    });
    const retained = store.getState().torrents.second;
    store.getState().applyUpdate({
      type: "patch",
      revision: 2,
      torrents: {
        upsert: [{ ...first, progress: 0.7 }],
        removed: [],
      },
    });
    expect(store.getState().torrents.second).toBe(retained);
    expect(store.getState().torrents.first?.progress).toBe(0.7);
  });

  it("repairs selection when the selected torrent is removed", () => {
    const store = createInspectionStore();
    store.getState().applyUpdate({
      type: "snapshot",
      snapshot: snapshot([row("first", "downloading"), row("second", "paused")]),
    });
    store.getState().selectTorrent("second");
    store.getState().applyUpdate({
      type: "patch",
      revision: 2,
      torrents: { upsert: [], removed: ["second"], order: ["first"] },
    });
    expect(store.getState().presentation.selectedTorrentId).toBe("first");
    expect(store.getState().presentation.detailOpen).toBe(false);
  });

  it("bounds retained diagnostic rows and reports drops", () => {
    const store = createInspectionStore();
    store.getState().applyUpdate({ type: "snapshot", snapshot: snapshot([]) });
    store.getState().applyUpdate({
      type: "patch",
      revision: 2,
      logs: {
        append: Array.from({ length: 300 }, (_, index) => ({
          id: String(index),
          timestampMs: index,
          severity: "info" as const,
          category: "test",
          summary: "bounded",
          torrentId: null,
        })),
        dropped: 2,
      },
    });
    expect(store.getState().logs).toHaveLength(256);
    expect(store.getState().droppedLogs).toBe(46);
  });
});

function row(id: string, status: TorrentRow["status"]): TorrentRow {
  return {
    id,
    name: id,
    status,
    sizeBytes: 100,
    progress: 0.5,
    downloadRate: 1,
    uploadRate: 0,
    downloadedBytes: 50,
    uploadedBytes: 0,
    peersConnected: 1,
    peersKnown: 2,
    etaSeconds: 50,
    addedAtMs: 1,
    archived: false,
    infoHash: id,
    error: null,
    progressReason: "test",
  };
}

function snapshot(rows: readonly TorrentRow[]): InspectionSnapshot {
  return {
    revision: 1,
    session: {
      connection: "demo",
      downloadRate: 0,
      uploadRate: 0,
      dhtNodes: 0,
      knownPeers: 0,
    },
    demo: null,
    torrentOrder: rows.map((item) => item.id),
    torrents: Object.fromEntries(rows.map((item) => [item.id, item])),
    peersByTorrent: {},
    logs: [],
    droppedLogs: 0,
  };
}
