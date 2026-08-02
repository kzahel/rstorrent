import { describe, expect, it } from "vitest";

import type { InspectionSnapshot, TorrentRow } from "./model";
import { createInspectionStore, emptyDiskSet } from "./state";

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

  it("keeps the detail pane size within its usable range", () => {
    const store = createInspectionStore();
    expect(store.getState().presentation.detailPanePercent).toBe(57);

    store.getState().setDetailPanePercent(10);
    expect(store.getState().presentation.detailPanePercent).toBe(25);
    store.getState().setDetailPanePercent(95);
    expect(store.getState().presentation.detailPanePercent).toBe(80);
    store.getState().setDetailPanePercent(Number.NaN);
    expect(store.getState().presentation.detailPanePercent).toBe(57);
  });

  it("defaults and persists interface size as presentation state", () => {
    let appearance: string | null = null;
    const storage = {
      getItem: () => appearance,
      setItem: (_key: string, value: string) => {
        appearance = value;
      },
    };
    const store = createInspectionStore(storage);
    expect(store.getState().presentation.interfaceSize).toBe("standard");

    store.getState().setInterfaceSize("spacious");
    expect(store.getState().presentation.interfaceSize).toBe("spacious");
    expect(
      createInspectionStore(storage).getState().presentation.interfaceSize,
    ).toBe("spacious");
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
    configuredTrackerCount: 0,
    etaSeconds: 50,
    addedAtMs: 1,
    archived: false,
    removalState: null,
    deleteManagedDataSupported: true,
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
    filesByTorrent: {},
    trackersByTorrent: {},
    piecesByTorrent: {},
    disk: emptyDiskSet(),
    logs: [],
    droppedLogs: 0,
    viewStatus: {
      library: { status: "ready" },
      torrentSummary: { status: "not_requested" },
      peers: { status: "not_requested" },
      files: { status: "not_requested" },
      trackers: { status: "not_requested" },
      pieces: { status: "not_requested" },
      disk: { status: "not_requested" },
      logs: { status: "not_requested" },
    },
  };
}
