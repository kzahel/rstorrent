export type TorrentStatus =
  | "metadata"
  | "downloading"
  | "paused"
  | "complete"
  | "checking"
  | "error";

export type PeerState =
  | "connecting"
  | "handshaking"
  | "connected"
  | "choked"
  | "stalled"
  | "disconnecting";

export type DetailTab =
  | "general"
  | "trackers"
  | "peers"
  | "swarm"
  | "files"
  | "pieces"
  | "disk"
  | "logs"
  | "speed"
  | "dht";

export type LibraryCategory =
  | "all"
  | "active"
  | "downloading"
  | "completed"
  | "paused"
  | "errors"
  | "archived";

export interface SessionSummary {
  readonly connection: "demo" | "connected" | "reconnecting" | "offline";
  readonly downloadRate: number;
  readonly uploadRate: number | null;
  readonly dhtNodes: number | null;
  readonly knownPeers: number | null;
}

export type ViewMaterialization =
  | { readonly status: "not_requested" }
  | { readonly status: "loading" }
  | { readonly status: "ready" }
  | { readonly status: "unavailable"; readonly reason: string }
  | { readonly status: "unsupported"; readonly reason: string }
  | { readonly status: "stale"; readonly reason: string };

export interface InspectionViewStatus {
  readonly library: ViewMaterialization;
  readonly torrentSummary: ViewMaterialization;
  readonly peers: ViewMaterialization;
  readonly files: ViewMaterialization;
  readonly logs: ViewMaterialization;
}

export interface DesiredInspectionViews {
  readonly library: boolean;
  readonly torrentId: string | null;
  readonly detail: "general" | "peers" | "files" | "logs" | null;
}

export interface DemoState {
  readonly scenarioId: DemoScenarioId;
  readonly elapsedMs: number;
  readonly running: boolean;
  readonly durationMs: number;
}

export interface TorrentRow {
  readonly id: string;
  readonly name: string;
  readonly status: TorrentStatus;
  readonly sizeBytes: number | null;
  readonly progress: number | null;
  readonly downloadRate: number;
  readonly uploadRate: number | null;
  readonly downloadedBytes: number;
  readonly uploadedBytes: number | null;
  readonly peersConnected: number;
  readonly peersKnown: number | null;
  readonly etaSeconds: number | null;
  readonly addedAtMs: number | null;
  readonly archived: boolean | null;
  readonly removalState: "pending" | "awaiting_platform" | "failed" | null;
  readonly deleteManagedDataSupported: boolean;
  readonly infoHash: string;
  readonly error: string | null;
  readonly progressReason: string;
}

export interface PeerRow {
  readonly connectionId: string;
  readonly torrentId: string;
  readonly state: PeerState;
  readonly endpoint: string;
  readonly client: string | null;
  readonly source:
    | "tracker"
    | "dht"
    | "pex"
    | "manual"
    | "incoming"
    | "cache"
    | "unknown";
  readonly progress: number | null;
  readonly downloadRate: number | null;
  readonly uploadRate: number | null;
  readonly downloadedBytes: number | null;
  readonly uploadedBytes: number | null;
  readonly requestsPending: number | null;
  readonly oldestRequestMs: number | null;
  readonly flags: string;
  readonly useful: boolean;
}

export interface LogRow {
  readonly id: string;
  readonly timestampMs: number;
  readonly severity: "debug" | "info" | "warning" | "error";
  readonly category: string;
  readonly summary: string;
  readonly torrentId: string | null;
}

export interface PeerSet {
  readonly order: readonly string[];
  readonly rows: Readonly<Record<string, PeerRow>>;
}

export interface FileRow {
  readonly id: string;
  readonly torrentId: string;
  readonly index: number;
  readonly path: readonly string[];
  readonly name: string;
  readonly folder: string;
  readonly extension: string;
  readonly lengthBytes: string;
  readonly torrentOffsetBytes: string;
  readonly firstPiece: number | null;
  readonly lastPiece: number | null;
  readonly selection: "wanted" | "skipped" | null;
  readonly padding: boolean;
  readonly doneBytes: string;
  readonly verifiedBytes: string;
  readonly storagePath: string | null;
}

export interface FileSet {
  readonly state: "metadata_pending" | "available" | "torrent_missing";
  readonly filesystemContentBase: string | null;
  readonly order: readonly string[];
  readonly rows: Readonly<Record<string, FileRow>>;
}

export interface InspectionSnapshot {
  readonly revision: number;
  readonly session: SessionSummary;
  readonly demo: DemoState | null;
  readonly torrentOrder: readonly string[];
  readonly torrents: Readonly<Record<string, TorrentRow>>;
  readonly peersByTorrent: Readonly<Record<string, PeerSet>>;
  readonly filesByTorrent: Readonly<Record<string, FileSet>>;
  readonly logs: readonly LogRow[];
  readonly droppedLogs: number;
  readonly viewStatus: InspectionViewStatus;
}

export interface KeyedPatch<T> {
  readonly upsert: readonly T[];
  readonly removed: readonly string[];
}

export type InspectionUpdate =
  | { readonly type: "snapshot"; readonly snapshot: InspectionSnapshot }
  | {
      readonly type: "patch";
      readonly revision: number;
      readonly session?: SessionSummary;
      readonly demo?: DemoState;
      readonly torrents?: KeyedPatch<TorrentRow> & {
        readonly order?: readonly string[];
      };
      readonly peers?: readonly (KeyedPatch<PeerRow> & {
        readonly torrentId: string;
        readonly order?: readonly string[];
      })[];
      readonly files?: readonly (KeyedPatch<FileRow> & {
        readonly torrentId: string;
        readonly state?: FileSet["state"];
        readonly filesystemContentBase?: string | null;
        readonly order?: readonly string[];
      })[];
      readonly logs?: {
        readonly append: readonly LogRow[];
        readonly dropped: number;
      };
    };

export type InspectionCommand =
  | { readonly type: "add_magnet"; readonly magnet: string }
  | { readonly type: "pause"; readonly torrentId: string }
  | { readonly type: "resume"; readonly torrentId: string }
  | { readonly type: "archive"; readonly torrentId: string }
  | { readonly type: "unarchive"; readonly torrentId: string }
  | {
      readonly type: "remove";
      readonly torrentId: string;
      readonly deleteData: boolean;
    }
  | { readonly type: "add_demo_torrent" }
  | { readonly type: "set_demo_scenario"; readonly scenarioId: DemoScenarioId }
  | { readonly type: "set_demo_running"; readonly running: boolean }
  | { readonly type: "advance_demo_clock"; readonly milliseconds: number }
  | { readonly type: "reset_demo" };

export type DemoScenarioId =
  | "healthy-download"
  | "stalled-metadata"
  | "tracker-recovery"
  | "endgame"
  | "large-swarm"
  | "file-progress"
  | "disk-error"
  | "empty-library";

export interface DemoScenarioSummary {
  readonly id: DemoScenarioId;
  readonly title: string;
  readonly description: string;
  readonly durationMs: number;
  readonly autoplay: boolean;
}

export interface CommandResult {
  readonly accepted: boolean;
  readonly message: string;
}
