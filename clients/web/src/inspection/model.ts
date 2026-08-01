export type TorrentStatus =
  | "metadata"
  | "downloading"
  | "paused"
  | "complete"
  | "checking"
  | "error";

export type PeerState =
  | "connecting"
  | "connected"
  | "choked"
  | "stalled"
  | "disconnected";

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
  readonly uploadRate: number;
  readonly dhtNodes: number | null;
  readonly knownPeers: number;
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
  readonly uploadRate: number;
  readonly downloadedBytes: number;
  readonly uploadedBytes: number;
  readonly peersConnected: number;
  readonly peersKnown: number;
  readonly etaSeconds: number | null;
  readonly addedAtMs: number;
  readonly archived: boolean;
  readonly infoHash: string;
  readonly error: string | null;
  readonly progressReason: string;
}

export interface PeerRow {
  readonly connectionId: string;
  readonly torrentId: string;
  readonly state: PeerState;
  readonly endpoint: string;
  readonly client: string;
  readonly source: "tracker" | "dht" | "pex" | "manual";
  readonly progress: number | null;
  readonly downloadRate: number;
  readonly uploadRate: number;
  readonly downloadedBytes: number;
  readonly uploadedBytes: number;
  readonly requestsPending: number;
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

export interface InspectionSnapshot {
  readonly revision: number;
  readonly session: SessionSummary;
  readonly demo: DemoState | null;
  readonly torrentOrder: readonly string[];
  readonly torrents: Readonly<Record<string, TorrentRow>>;
  readonly peersByTorrent: Readonly<Record<string, PeerSet>>;
  readonly logs: readonly LogRow[];
  readonly droppedLogs: number;
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
      readonly logs?: {
        readonly append: readonly LogRow[];
        readonly dropped: number;
      };
    };

export type InspectionCommand =
  | { readonly type: "pause"; readonly torrentId: string }
  | { readonly type: "resume"; readonly torrentId: string }
  | { readonly type: "archive"; readonly torrentId: string }
  | { readonly type: "unarchive"; readonly torrentId: string }
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
