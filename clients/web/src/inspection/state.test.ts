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
    expect(store.getState().presentation.selectedTorrentIds).toEqual(["first"]);
    expect(store.getState().presentation.detailOpen).toBe(false);
  });

  it("shares bounded multi-selection and repairs it after removals", () => {
    const store = createInspectionStore(null);
    store.getState().applyUpdate({
      type: "snapshot",
      snapshot: snapshot([
        row("first", "downloading"),
        row("second", "paused"),
        row("third", "complete"),
      ]),
    });
    store.getState().replaceTorrentSelection(["first", "second", "second"]);
    store.getState().toggleTorrentSelection("third");
    expect(store.getState().presentation.selectedTorrentIds).toEqual([
      "first",
      "second",
      "third",
    ]);
    expect(store.getState().presentation.selectedTorrentId).toBe("third");

    store.getState().applyUpdate({
      type: "patch",
      revision: 2,
      torrents: {
        upsert: [],
        removed: ["second", "third"],
        order: ["first"],
      },
    });
    expect(store.getState().presentation.selectedTorrentIds).toEqual(["first"]);
    expect(store.getState().presentation.selectedTorrentId).toBe("first");
  });

  it("keeps destination filters independent and persists navigation", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    };
    const store = createInspectionStore(storage);
    expect(store.getState().presentation.destination).toBe("transfers");

    store.getState().selectTorrentCategory("paused");
    store.getState().selectDestination("workbench");
    store.getState().selectTorrentCategory("errors");
    store.getState().selectDestination("library");
    store.getState().selectLibraryCategory("available");

    const presentation = store.getState().presentation;
    expect(presentation.transfersCategory).toBe("paused");
    expect(presentation.workbenchCategory).toBe("errors");
    expect(presentation.libraryCategory).toBe("available");
    expect(createInspectionStore(storage).getState().presentation).toMatchObject({
      destination: "library",
      transfersCategory: "paused",
      workbenchCategory: "errors",
      libraryCategory: "available",
    });
  });

  it("bounds retained diagnostic rows and reports drops", () => {
    const store = createInspectionStore();
    store.getState().applyUpdate({ type: "snapshot", snapshot: snapshot([]) });
    store.getState().applyUpdate({
      type: "patch",
      revision: 2,
      logs: {
        append: Array.from({ length: 10_000 }, (_, index) => ({
          id: String(index),
          timestampMs: index,
          severity: "info" as const,
          category: "test",
          code: "bounded",
          message: "bounded",
          torrentId: null,
          subjects: [],
          fields: [],
        })),
        sourceEvictedCount: 2,
        retainedFromSequence: "1",
        deliveryResetCount: 0,
        lastDeliveryResetReason: null,
      },
    });
    expect(store.getState().logs).toHaveLength(2_048);
    expect(store.getState().logLoss.sourceEvictedCount).toBe(2);
    expect(store.getState().logLoss.localEvictedCount).toBe(7_952);
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

  it("defaults and persists complete appearance presentation state", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    };
    const store = createInspectionStore(storage);
    expect(store.getState().presentation.interfaceSize).toBe("standard");
    expect(store.getState().presentation.colorTheme).toBe("auto");

    store.getState().setColorTheme("dark");
    store.getState().setInterfaceSize("spacious");
    expect(store.getState().presentation.interfaceSize).toBe("spacious");
    expect(store.getState().presentation.colorTheme).toBe("dark");
    expect(
      JSON.parse(values.get("rstorrent.presentation.appearance") ?? "null"),
    ).toEqual({
      version: 2,
      interfaceSize: "spacious",
      colorTheme: "dark",
    });
    const restored = createInspectionStore(storage).getState().presentation;
    expect(restored.interfaceSize).toBe("spacious");
    expect(restored.colorTheme).toBe("dark");
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
    logLoss: {
      sourceEvictedCount: 0,
      retainedFromSequence: "1",
      localEvictedCount: 0,
      deliveryResetCount: 0,
      lastDeliveryResetReason: null,
    },
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
