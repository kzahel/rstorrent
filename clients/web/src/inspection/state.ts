import { createStore, type StoreApi } from "zustand/vanilla";

import type {
  DetailTab,
  InspectionSnapshot,
  InspectionUpdate,
  LibraryCategory,
  LogRow,
  PeerSet,
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
  logs: [],
  droppedLogs: 0,
  viewStatus: {
    library: { status: "not_requested" },
    torrentSummary: { status: "not_requested" },
    peers: { status: "not_requested" },
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
};

export function createInspectionStore(): InspectionStoreApi {
  return createStore<InspectionStore>()((set) => ({
    ...EMPTY_SNAPSHOT,
    presentation: DEFAULT_PRESENTATION,
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
  let logs = state.logs;
  let droppedLogs = state.droppedLogs;

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

  if (update.logs !== undefined) {
    const combined = [...state.logs, ...update.logs.append];
    const overflow = Math.max(0, combined.length - 256);
    logs = overflow === 0 ? combined : combined.slice(overflow);
    droppedLogs = state.droppedLogs + update.logs.dropped + overflow;
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
    logs,
    droppedLogs,
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

export function visibleLogs(
  logs: readonly LogRow[],
  torrentId: string | null,
): readonly LogRow[] {
  return logs.filter(
    (row) => row.torrentId === null || row.torrentId === torrentId,
  );
}

function clampDetailPanePercent(percent: number): number {
  if (!Number.isFinite(percent)) return DEFAULT_DETAIL_PANE_PERCENT;
  return Math.min(
    MAX_DETAIL_PANE_PERCENT,
    Math.max(MIN_DETAIL_PANE_PERCENT, Math.round(percent)),
  );
}
