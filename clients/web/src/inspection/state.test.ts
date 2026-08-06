import { describe, expect, it } from "vitest";

import { DEFAULT_CLIENT_SETTINGS_RUNTIME_VIEW } from "../api";
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

  it("clears a removed singleton selection without inventing a fallback", () => {
    const store = createInspectionStore();
    store.getState().applyUpdate({
      type: "snapshot",
      snapshot: snapshot([
        row("first", "downloading"),
        row("second", "paused"),
      ]),
    });
    store.getState().openTorrentDetail("second");
    store.getState().applyUpdate({
      type: "patch",
      revision: 2,
      torrents: { upsert: [], removed: ["second"], order: ["first"] },
    });
    expect(store.getState().presentation.currentTorrentId).toBeNull();
    expect(store.getState().presentation.selectedTorrentIds).toEqual([]);
    expect(store.getState().presentation.detailOpen).toBe(false);
  });

  it("keeps current constrained to the checked torrent selection", () => {
    const store = createInspectionStore(null);
    store.getState().applyUpdate({
      type: "snapshot",
      snapshot: snapshot([
        row("first", "downloading"),
        row("second", "paused"),
        row("third", "complete"),
      ]),
    });
    expect(store.getState().presentation.currentTorrentId).toBe("first");
    expect(store.getState().presentation.selectedTorrentIds).toEqual(["first"]);

    store
      .getState()
      .setTorrentSelection(["first", "second", "second", "third"], "second");
    expect(store.getState().presentation.currentTorrentId).toBe("second");
    expect(store.getState().presentation.selectedTorrentIds).toEqual([
      "first",
      "second",
      "third",
    ]);
    store.getState().openTorrentDetail("first");
    expect(store.getState().presentation.currentTorrentId).toBe("first");
    expect(store.getState().presentation.selectedTorrentIds).toEqual([
      "first",
      "second",
      "third",
    ]);
    expect(store.getState().presentation.detailOpen).toBe(true);

    store.getState().applyUpdate({
      type: "patch",
      revision: 2,
      torrents: {
        upsert: [],
        removed: ["first"],
        order: ["second", "third"],
      },
    });
    expect(store.getState().presentation.selectedTorrentIds).toEqual([
      "second",
      "third",
    ]);
    expect(store.getState().presentation.currentTorrentId).toBe("second");
    expect(store.getState().presentation.detailOpen).toBe(false);

    store.getState().selectOnlyTorrent("third");
    expect(store.getState().presentation.currentTorrentId).toBe("third");
    expect(store.getState().presentation.selectedTorrentIds).toEqual(["third"]);
    store.getState().clearTorrentSelection();
    expect(store.getState().presentation.currentTorrentId).toBeNull();
    expect(store.getState().presentation.selectedTorrentIds).toEqual([]);
    store.getState().applyUpdate({
      type: "patch",
      revision: 3,
      torrents: {
        upsert: [{ ...row("third", "paused"), progress: 0.5 }],
        removed: [],
      },
    });
    expect(store.getState().presentation.currentTorrentId).toBeNull();
    expect(store.getState().presentation.selectedTorrentIds).toEqual([]);
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
    expect(
      createInspectionStore(storage).getState().presentation,
    ).toMatchObject({
      destination: "library",
      transfersCategory: "paused",
      workbenchCategory: "errors",
      libraryCategory: "available",
    });
  });

  it("opens an errored torrent directly at its General error detail", () => {
    const store = createInspectionStore();
    const failed = {
      ...row("failed", "error"),
      error: "Write failed: destination has no free space",
    };
    store.getState().applyUpdate({
      type: "snapshot",
      snapshot: snapshot([row("other", "paused"), failed]),
    });
    store.getState().setTorrentSelection(["other", "failed"], "other");

    store.getState().openTorrentErrorDetail("failed");

    expect(store.getState().presentation).toMatchObject({
      destination: "workbench",
      currentTorrentId: "failed",
      selectedTorrentIds: ["failed"],
      activeTab: "general",
      detailOpen: true,
      detailTarget: { type: "torrent_error", torrentId: "failed" },
    });
    store.getState().clearDetailTarget();
    expect(store.getState().presentation.detailTarget).toBeNull();
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
    expect(store.getState().presentation.dataUnits).toBe("decimal");

    store.getState().setColorTheme("dark");
    store.getState().setDataUnits("binary");
    store.getState().setInterfaceSize("spacious");
    expect(store.getState().presentation.interfaceSize).toBe("spacious");
    expect(store.getState().presentation.colorTheme).toBe("dark");
    expect(store.getState().presentation.dataUnits).toBe("binary");
    expect(
      JSON.parse(values.get("rstorrent.presentation.appearance") ?? "null"),
    ).toEqual({
      version: 3,
      interfaceSize: "spacious",
      colorTheme: "dark",
      dataUnits: "binary",
    });
    const restored = createInspectionStore(storage).getState().presentation;
    expect(restored.interfaceSize).toBe("spacious");
    expect(restored.colorTheme).toBe("dark");
    expect(restored.dataUnits).toBe("binary");
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
    forceRecheckAvailable: true,
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
    storage: { roots: [], defaultRoot: null, showAddOptions: true },
    clientSettings: structuredClone(DEFAULT_CLIENT_SETTINGS_RUNTIME_VIEW),
    torrentOrder: rows.map((item) => item.id),
    torrents: Object.fromEntries(rows.map((item) => [item.id, item])),
    peersByTorrent: {},
    swarmByTorrent: {},
    filesByTorrent: {},
    trackersByTorrent: {},
    piecesByTorrent: {},
    disk: emptyDiskSet(),
    dht: null,
    speed: null,
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
      swarm: { status: "not_requested" },
      files: { status: "not_requested" },
      trackers: { status: "not_requested" },
      pieces: { status: "not_requested" },
      disk: { status: "not_requested" },
      dht: { status: "not_requested" },
      speed: { status: "not_requested" },
      logs: { status: "not_requested" },
    },
  };
}
