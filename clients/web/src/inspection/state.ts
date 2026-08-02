import { createStore, type StoreApi } from "zustand/vanilla";

import {
  DEFAULT_COLOR_THEME,
  DEFAULT_INTERFACE_SIZE,
  loadAppearancePreferences,
  saveAppearancePreferences,
  type AppearanceStorage,
  type ColorTheme,
  type InterfaceSize,
} from "./appearance";
import type {
  DetailTab,
  InspectionSnapshot,
  InspectionUpdate,
  LibraryCategory,
  LogRow,
  DiskSet,
  FileSet,
  PeerSet,
  TrackerSet,
  PieceMapSet,
  TorrentRow,
} from "./model";

export interface PresentationState {
  readonly category: LibraryCategory;
  readonly selectedTorrentId: string | null;
  readonly selectedPeerId: string | null;
  readonly activeTab: DetailTab;
  readonly detailPanePercent: number;
  readonly detailOpen: boolean;
  readonly sidebarOpen: boolean;
  readonly layout: "wide" | "compact" | "phone";
  readonly interfaceSize: InterfaceSize;
  readonly colorTheme: ColorTheme;
  readonly logCaptureProfile: "normal" | "detailed" | "trace";
  readonly logCaptureTorrentId: string | null;
  readonly logMinimumSeverity: LogRow["severity"];
  readonly logCategoryPrefix: string;
  readonly logSearch: string;
  readonly logDisplayScope: "all" | "selected";
  readonly logExpandedIds: readonly string[];
  readonly logClearThroughSequence: string | null;
  readonly logFollowing: boolean;
}

export interface InspectionState extends InspectionSnapshot {
  readonly presentation: PresentationState;
}

export interface InspectionActions {
  readonly applyUpdate: (update: InspectionUpdate) => void;
  readonly selectCategory: (category: LibraryCategory) => void;
  readonly selectTorrent: (torrentId: string) => void;
  readonly selectPeer: (connectionId: string | null) => void;
  readonly selectTab: (tab: DetailTab) => void;
  readonly setDetailPanePercent: (percent: number) => void;
  readonly closeDetail: () => void;
  readonly toggleSidebar: () => void;
  readonly closeSidebar: () => void;
  readonly setLayout: (layout: PresentationState["layout"]) => void;
  readonly setInterfaceSize: (interfaceSize: InterfaceSize) => void;
  readonly setColorTheme: (colorTheme: ColorTheme) => void;
  readonly setLogCaptureProfile: (
    profile: PresentationState["logCaptureProfile"],
  ) => void;
  readonly setLogCaptureTorrent: (torrentId: string | null) => void;
  readonly setLogMinimumSeverity: (severity: LogRow["severity"]) => void;
  readonly setLogCategoryPrefix: (prefix: string) => void;
  readonly setLogSearch: (search: string) => void;
  readonly setLogDisplayScope: (scope: "all" | "selected") => void;
  readonly toggleLogExpanded: (sequence: string) => void;
  readonly clearVisibleLogs: () => void;
  readonly setLogFollowing: (following: boolean) => void;
}

export type InspectionStore = InspectionState & InspectionActions;
export type InspectionStoreApi = StoreApi<InspectionStore>;

export const DEFAULT_DETAIL_PANE_PERCENT = 57;
export const MIN_DETAIL_PANE_PERCENT = 25;
export const MAX_DETAIL_PANE_PERCENT = 80;

const EMPTY_SNAPSHOT: InspectionSnapshot = {
  revision: 0,
  session: {
    connection: "offline",
    downloadRate: 0,
    uploadRate: 0,
    dhtNodes: null,
    knownPeers: 0,
  },
  demo: null,
  torrentOrder: [],
  torrents: {},
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
    library: { status: "not_requested" },
    torrentSummary: { status: "not_requested" },
    peers: { status: "not_requested" },
    files: { status: "not_requested" },
    trackers: { status: "not_requested" },
    pieces: { status: "not_requested" },
    disk: { status: "not_requested" },
    logs: { status: "not_requested" },
  },
};

const DEFAULT_PRESENTATION: PresentationState = {
  category: "all",
  selectedTorrentId: null,
  selectedPeerId: null,
  activeTab: "peers",
  detailPanePercent: DEFAULT_DETAIL_PANE_PERCENT,
  detailOpen: false,
  sidebarOpen: false,
  layout: "wide",
  interfaceSize: DEFAULT_INTERFACE_SIZE,
  colorTheme: DEFAULT_COLOR_THEME,
  logCaptureProfile: "normal",
  logCaptureTorrentId: null,
  logMinimumSeverity: "info",
  logCategoryPrefix: "",
  logSearch: "",
  logDisplayScope: "selected",
  logExpandedIds: [],
  logClearThroughSequence: null,
  logFollowing: true,
};

export function createInspectionStore(
  appearanceStorage?: AppearanceStorage | null,
): InspectionStoreApi {
  const appearance =
    appearanceStorage === undefined
      ? loadAppearancePreferences()
      : loadAppearancePreferences(appearanceStorage);
  const initialPresentation = {
    ...DEFAULT_PRESENTATION,
    ...appearance,
  };
  const persistAppearance = (preferences: {
    readonly interfaceSize: InterfaceSize;
    readonly colorTheme: ColorTheme;
  }) => {
    if (appearanceStorage === undefined) saveAppearancePreferences(preferences);
    else saveAppearancePreferences(preferences, appearanceStorage);
  };
  return createStore<InspectionStore>()((set) => ({
    ...EMPTY_SNAPSHOT,
    presentation: initialPresentation,
    applyUpdate: (update) => {
      set((state) => reduceInspectionUpdate(state, update));
    },
    selectCategory: (category) => {
      set((state) => ({
        presentation: {
          ...state.presentation,
          category,
          sidebarOpen: false,
        },
      }));
    },
    selectTorrent: (torrentId) => {
      set((state) => ({
        presentation: {
          ...state.presentation,
          selectedTorrentId: torrentId,
          selectedPeerId: null,
          detailOpen: true,
        },
      }));
    },
    selectPeer: (connectionId) => {
      set((state) => ({
        presentation: { ...state.presentation, selectedPeerId: connectionId },
      }));
    },
    selectTab: (activeTab) => {
      set((state) => ({
        presentation: { ...state.presentation, activeTab },
      }));
    },
    setDetailPanePercent: (percent) => {
      set((state) => ({
        presentation: {
          ...state.presentation,
          detailPanePercent: clampDetailPanePercent(percent),
        },
      }));
    },
    closeDetail: () => {
      set((state) => ({
        presentation: { ...state.presentation, detailOpen: false },
      }));
    },
    toggleSidebar: () => {
      set((state) => ({
        presentation: {
          ...state.presentation,
          sidebarOpen: !state.presentation.sidebarOpen,
        },
      }));
    },
    closeSidebar: () => {
      set((state) => ({
        presentation: { ...state.presentation, sidebarOpen: false },
      }));
    },
    setLayout: (layout) => {
      set((state) => ({
        presentation: { ...state.presentation, layout },
      }));
    },
    setInterfaceSize: (interfaceSize) => {
      set((state) => {
        persistAppearance({
          interfaceSize,
          colorTheme: state.presentation.colorTheme,
        });
        return {
          presentation: { ...state.presentation, interfaceSize },
        };
      });
    },
    setColorTheme: (colorTheme) => {
      set((state) => {
        persistAppearance({
          interfaceSize: state.presentation.interfaceSize,
          colorTheme,
        });
        return {
          presentation: { ...state.presentation, colorTheme },
        };
      });
    },
    setLogCaptureProfile: (logCaptureProfile) => {
      set((state) => ({
        presentation: { ...state.presentation, logCaptureProfile },
      }));
    },
    setLogCaptureTorrent: (logCaptureTorrentId) => {
      set((state) => ({
        presentation: { ...state.presentation, logCaptureTorrentId },
      }));
    },
    setLogMinimumSeverity: (logMinimumSeverity) => {
      set((state) => ({
        presentation: { ...state.presentation, logMinimumSeverity },
      }));
    },
    setLogCategoryPrefix: (logCategoryPrefix) => {
      set((state) => ({
        presentation: { ...state.presentation, logCategoryPrefix },
      }));
    },
    setLogSearch: (logSearch) => {
      set((state) => ({
        presentation: { ...state.presentation, logSearch },
      }));
    },
    setLogDisplayScope: (logDisplayScope) => {
      set((state) => ({
        presentation: { ...state.presentation, logDisplayScope },
      }));
    },
    toggleLogExpanded: (sequence) => {
      set((state) => {
        const expanded = state.presentation.logExpandedIds.includes(sequence);
        return {
          presentation: {
            ...state.presentation,
            logExpandedIds: expanded
              ? state.presentation.logExpandedIds.filter((id) => id !== sequence)
              : [...state.presentation.logExpandedIds, sequence],
          },
        };
      });
    },
    clearVisibleLogs: () => {
      set((state) => ({
        presentation: {
          ...state.presentation,
          logClearThroughSequence: state.logs.at(-1)?.id ?? null,
          logExpandedIds: [],
        },
      }));
    },
    setLogFollowing: (logFollowing) => {
      set((state) => ({
        presentation: { ...state.presentation, logFollowing },
      }));
    },
  }));
}

export function reduceInspectionUpdate(
  state: InspectionState,
  update: InspectionUpdate,
): Partial<InspectionState> {
  if (update.type === "snapshot") {
    const selected = state.presentation.selectedTorrentId;
    const nextSelected =
      selected !== null && update.snapshot.torrents[selected] !== undefined
        ? selected
        : (update.snapshot.torrentOrder[0] ?? null);
    return {
      ...update.snapshot,
      presentation: {
        ...state.presentation,
        selectedTorrentId: nextSelected,
        selectedPeerId: null,
        detailOpen:
          nextSelected === selected ? state.presentation.detailOpen : false,
      },
    };
  }

  let torrents = state.torrents;
  let torrentOrder = state.torrentOrder;
  let peersByTorrent = state.peersByTorrent;
  let filesByTorrent = state.filesByTorrent;
  let trackersByTorrent = state.trackersByTorrent;
  let piecesByTorrent = state.piecesByTorrent;
  let disk = state.disk;
  let logs = state.logs;
  let logLoss = state.logLoss;

  if (update.torrents !== undefined) {
    torrents = applyRows(
      state.torrents,
      update.torrents.upsert,
      update.torrents.removed,
      (row) => row.id,
    );
    torrentOrder =
      update.torrents.order ??
      state.torrentOrder.filter((id) => torrents[id] !== undefined);
  }

  if (update.peers !== undefined) {
    const nextPeerSets = { ...state.peersByTorrent };
    for (const patch of update.peers) {
      const current = state.peersByTorrent[patch.torrentId] ?? EMPTY_PEER_SET;
      const rows = applyRows(
        current.rows,
        patch.upsert,
        patch.removed,
        (row) => row.connectionId,
      );
      nextPeerSets[patch.torrentId] = {
        rows,
        order:
          patch.order ?? current.order.filter((id) => rows[id] !== undefined),
      };
    }
    peersByTorrent = nextPeerSets;
  }

  if (update.files !== undefined) {
    const nextFileSets = { ...state.filesByTorrent };
    for (const patch of update.files) {
      const current = state.filesByTorrent[patch.torrentId] ?? EMPTY_FILE_SET;
      const rows = applyRows(
        current.rows,
        patch.upsert,
        patch.removed,
        (row) => row.id,
      );
      nextFileSets[patch.torrentId] = {
        state: patch.state ?? current.state,
        filesystemContentBase:
          patch.filesystemContentBase === undefined
            ? current.filesystemContentBase
            : patch.filesystemContentBase,
        rows,
        order:
          patch.order ?? current.order.filter((id) => rows[id] !== undefined),
      };
    }
    filesByTorrent = nextFileSets;
  }

  if (update.trackers !== undefined) {
    const nextTrackerSets = { ...state.trackersByTorrent };
    for (const patch of update.trackers) {
      const current =
        state.trackersByTorrent[patch.torrentId] ?? EMPTY_TRACKER_SET;
      const rows = applyRows(
        current.rows,
        patch.upsert,
        patch.removed,
        (row) => row.id,
      );
      nextTrackerSets[patch.torrentId] = {
        state: patch.state ?? current.state,
        rows,
        order:
          patch.order ?? current.order.filter((id) => rows[id] !== undefined),
      };
    }
    trackersByTorrent = nextTrackerSets;
  }

  if (update.disk !== undefined) {
    disk = update.disk;
  }

  if (update.pieces !== undefined) {
    piecesByTorrent = update.pieces;
  }

  if (update.logs !== undefined) {
    const combined = [...state.logs, ...update.logs.append];
    const overflow = Math.max(0, combined.length - 2_048);
    logs = overflow === 0 ? combined : combined.slice(overflow);
    logLoss = {
      sourceEvictedCount: update.logs.sourceEvictedCount,
      retainedFromSequence: update.logs.retainedFromSequence,
      localEvictedCount: state.logLoss.localEvictedCount + overflow,
      deliveryResetCount: update.logs.deliveryResetCount,
      lastDeliveryResetReason: update.logs.lastDeliveryResetReason,
    };
  }

  const selectedId = state.presentation.selectedTorrentId;
  const nextSelected =
    selectedId !== null && torrents[selectedId] !== undefined
      ? selectedId
      : (torrentOrder[0] ?? null);

  return {
    revision: update.revision,
    session: update.session ?? state.session,
    demo: update.demo ?? state.demo,
    torrents,
    torrentOrder,
    peersByTorrent,
    filesByTorrent,
    trackersByTorrent,
    piecesByTorrent,
    disk,
    logs,
    logLoss,
    presentation: {
      ...state.presentation,
      selectedTorrentId: nextSelected,
      selectedPeerId:
        nextSelected === selectedId ? state.presentation.selectedPeerId : null,
      detailOpen:
        nextSelected === selectedId ? state.presentation.detailOpen : false,
    },
  };
}

export function torrentMatchesCategory(
  torrent: TorrentRow,
  category: LibraryCategory,
): boolean {
  if (category === "archived") return torrent.archived === true;
  if (torrent.archived === true) return false;
  switch (category) {
    case "all":
      return true;
    case "active":
      return torrent.status === "metadata" || torrent.status === "downloading";
    case "downloading":
      return torrent.status === "downloading";
    case "completed":
      return torrent.status === "complete";
    case "paused":
      return torrent.status === "paused";
    case "errors":
      return torrent.status === "error";
  }
}

function applyRows<T>(
  current: Readonly<Record<string, T>>,
  upsert: readonly T[],
  removed: readonly string[],
  getId: (row: T) => string,
): Readonly<Record<string, T>> {
  if (upsert.length === 0 && removed.length === 0) return current;
  const next = { ...current };
  for (const id of removed) delete next[id];
  for (const row of upsert) next[getId(row)] = row;
  return next;
}

const EMPTY_PEER_SET: PeerSet = { order: [], rows: {} };
const EMPTY_FILE_SET: FileSet = {
  state: "metadata_pending",
  filesystemContentBase: null,
  order: [],
  rows: {},
};
const EMPTY_TRACKER_SET: TrackerSet = {
  state: "available",
  order: [],
  rows: {},
};

export function emptyDiskSet(): DiskSet {
  return {
    pipeline: {
      pressure: "idle",
      intakeBackpressured: false,
      sampleMillis: 0,
      residentLimitBytes: 0,
      residentHighWatermarkBytes: 0,
      residentLowWatermarkBytes: 0,
      requestedBytes: 0,
      residentBytes: 0,
      queuedWriteBytes: 0,
      writingBytes: 0,
      hashingBytes: 0,
      storageJobsPending: 0,
      receivedBytesTotal: 0,
      storedBytesTotal: 0,
      verifiedBytesTotal: 0,
      receiveRateBytes: 0,
      writeRateBytes: 0,
      hashRateBytes: 0,
      writeOperationsStarted: 0,
      writeOperationsCompleted: 0,
      hashOperationsStarted: 0,
      hashOperationsCompleted: 0,
      writeQueueWaitMicros: 0,
      writeQueueWaitMaxMicros: 0,
      writeServiceMicros: 0,
      writeServiceMaxMicros: 0,
      hashQueueWaitMicros: 0,
      hashQueueWaitMaxMicros: 0,
      hashServiceMicros: 0,
      hashServiceMaxMicros: 0,
      pressureTransitionCount: 0,
      backpressuredMillisTotal: 0,
      lastError: null,
    },
    order: [],
    rows: {},
  };
}

export function emptyPieceMapSet(torrentId: string): PieceMapSet {
  return {
    torrentId,
    pieceCount: 0,
    verified: new Uint8Array(0),
    active: [],
    revision: 0,
  };
}

function clampDetailPanePercent(percent: number): number {
  if (!Number.isFinite(percent)) return DEFAULT_DETAIL_PANE_PERCENT;
  return Math.min(
    MAX_DETAIL_PANE_PERCENT,
    Math.max(MIN_DETAIL_PANE_PERCENT, Math.round(percent)),
  );
}
