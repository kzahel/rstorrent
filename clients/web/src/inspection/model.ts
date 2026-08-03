import type {
  DiagnosticField,
  DiagnosticSubject,
  PeerFlagView,
} from "../api";

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

export type PeerFlag = PeerFlagView;

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

export type ApplicationDestination = "library" | "transfers" | "workbench";

export type LibraryCategory =
  | "all"
  | "recent"
  | "available"
  | "downloading";

export type TorrentCategory =
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

export interface DownloadRoot {
  readonly id: string;
  readonly label: string;
  readonly path: string | null;
  readonly availability: "available" | "unavailable";
}

export interface DownloadStorageSettings {
  readonly roots: readonly DownloadRoot[];
  readonly defaultRoot: string | null;
  readonly showAddOptions: boolean;
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
  readonly trackers: ViewMaterialization;
  readonly pieces: ViewMaterialization;
  readonly disk: ViewMaterialization;
  readonly logs: ViewMaterialization;
}

export interface DesiredInspectionViews {
  readonly library: boolean;
  readonly torrentId: string | null;
  readonly detail:
    | "general"
    | "trackers"
    | "peers"
    | "files"
    | "pieces"
    | "disk"
    | "logs"
    | null;
  readonly logCapture: {
    readonly profile: "normal" | "detailed" | "trace";
    readonly torrentId: string | null;
  } | null;
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
  readonly configuredTrackerCount: number | null;
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
  readonly flags: readonly PeerFlag[];
  readonly useful: boolean;
}

export interface LogRow {
  readonly id: string;
  readonly timestampMs: number;
  readonly severity: "trace" | "debug" | "info" | "warning" | "error";
  readonly category: string;
  readonly code: string;
  readonly message: string;
  readonly torrentId: string | null;
  readonly subjects: readonly DiagnosticSubject[];
  readonly fields: readonly DiagnosticField[];
}

export interface LogLoss {
  readonly sourceEvictedCount: number;
  readonly retainedFromSequence: string;
  readonly localEvictedCount: number;
  readonly deliveryResetCount: number;
  readonly lastDeliveryResetReason: string | null;
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

export interface TrackerRow {
  readonly id: string;
  readonly torrentId: string;
  readonly url: string;
  readonly transport: "udp";
  readonly source: "magnet";
  readonly tier: number;
  readonly status:
    | "inactive"
    | "idle"
    | "announcing"
    | "retry_wait"
    | "reannounce_wait";
  readonly announceEvent: "started" | "update" | null;
  readonly totalAttempts: number;
  readonly consecutiveFailures: number;
  readonly lastPeerCount: number | null;
  readonly seeders: number | null;
  readonly leechers: number | null;
  readonly intervalSeconds: number | null;
  readonly nextAction: "announce" | "retry" | "reannounce" | null;
  readonly nextActionInMs: number | null;
  readonly observedAtMs: number;
  readonly lastSuccessAgeMs: number | null;
  readonly lastFailureAgeMs: number | null;
  readonly error: string | null;
}

export interface TrackerSet {
  readonly state: "available" | "torrent_missing";
  readonly order: readonly string[];
  readonly rows: Readonly<Record<string, TrackerRow>>;
}

export interface DiskPipeline {
  readonly pressure: "idle" | "normal" | "backpressured" | "draining" | "error";
  readonly checkpointStage: "idle" | "syncing" | "committing" | "error";
  readonly intakeBackpressured: boolean;
  readonly sampleMillis: number;
  readonly residentLimitBytes: number;
  readonly residentHighWatermarkBytes: number;
  readonly residentLowWatermarkBytes: number;
  readonly requestedBytes: number;
  readonly residentBytes: number;
  readonly queuedWriteBytes: number;
  readonly writingBytes: number;
  readonly hashingBytes: number;
  readonly checkpointDirtyPieces: number;
  readonly checkpointDirtyBytes: number;
  readonly checkpointDirtyPieceHighWater: number;
  readonly checkpointDirtyByteHighWater: number;
  readonly checkpointOldestDirtyMillis: number;
  readonly checkpointBatchesStarted: number;
  readonly checkpointBatchesCompleted: number;
  readonly checkpointPiecesCompleted: number;
  readonly checkpointSyncOperationsCompleted: number;
  readonly checkpointSyncServiceMicros: number;
  readonly checkpointSyncServiceMaxMicros: number;
  readonly checkpointCommitServiceMicros: number;
  readonly checkpointCommitServiceMaxMicros: number;
  readonly checkpointActiveMicros: number | null;
  readonly storageJobsPending: number;
  readonly receivedBytesTotal: number;
  readonly storedBytesTotal: number;
  readonly verifiedBytesTotal: number;
  readonly receiveRateBytes: number;
  readonly writeRateBytes: number;
  readonly hashRateBytes: number;
  readonly writeOperationsStarted: number;
  readonly writeOperationsCompleted: number;
  readonly hashOperationsStarted: number;
  readonly hashOperationsCompleted: number;
  readonly writeQueueWaitMicros: number;
  readonly writeQueueWaitMaxMicros: number;
  readonly writeServiceMicros: number;
  readonly writeServiceMaxMicros: number;
  readonly hashQueueWaitMicros: number;
  readonly hashQueueWaitMaxMicros: number;
  readonly hashServiceMicros: number;
  readonly hashServiceMaxMicros: number;
  readonly pressureTransitionCount: number;
  readonly backpressuredMillisTotal: number;
  readonly lastError: string | null;
}

export interface DiskPieceRow {
  readonly id: string;
  readonly torrentId: string;
  readonly torrentName: string;
  readonly pieceIndex: number;
  readonly pieceLength: number;
  readonly attempt: number;
  readonly stage:
    | "receiving"
    | "queued"
    | "writing"
    | "stored"
    | "hashing"
    | "checkpoint_dirty"
    | "checkpoint_syncing"
    | "checkpoint_committing"
    | "failed";
  readonly requestedBytes: number;
  readonly receivedBytes: number;
  readonly storedBytes: number;
  readonly ageMillis: number;
  readonly stageAgeMillis: number;
  readonly error: string | null;
}

export interface DiskSet {
  readonly pipeline: DiskPipeline;
  readonly order: readonly string[];
  readonly rows: Readonly<Record<string, DiskPieceRow>>;
}

export type PieceLifecycleStage =
  | "requested"
  | "received"
  | "stored"
  | "hashing"
  | "checkpoint_dirty"
  | "checkpoint_syncing"
  | "checkpoint_committing"
  | "failed";

export interface ActivePieceSummary {
  readonly id: string;
  readonly pieceIndex: number;
  readonly attempt: number;
  readonly pieceLength: number;
  readonly stage: PieceLifecycleStage;
  readonly requestedBytes: number;
  readonly receivedBytes: number;
  readonly storedBytes: number;
  readonly ageMillis: number;
  readonly error: string | null;
}

export interface PieceMapSet {
  readonly torrentId: string;
  readonly pieceCount: number;
  readonly verified: Uint8Array;
  readonly active: readonly ActivePieceSummary[];
  readonly revision: number;
}

export interface InspectionSnapshot {
  readonly revision: number;
  readonly session: SessionSummary;
  readonly demo: DemoState | null;
  readonly storage: DownloadStorageSettings;
  readonly torrentOrder: readonly string[];
  readonly torrents: Readonly<Record<string, TorrentRow>>;
  readonly peersByTorrent: Readonly<Record<string, PeerSet>>;
  readonly filesByTorrent: Readonly<Record<string, FileSet>>;
  readonly trackersByTorrent: Readonly<Record<string, TrackerSet>>;
  readonly piecesByTorrent: Readonly<Record<string, PieceMapSet>>;
  readonly disk: DiskSet;
  readonly logs: readonly LogRow[];
  readonly logLoss: LogLoss;
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
      readonly storage?: DownloadStorageSettings;
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
      readonly trackers?: readonly (KeyedPatch<TrackerRow> & {
        readonly torrentId: string;
        readonly state?: TrackerSet["state"];
        readonly order?: readonly string[];
      })[];
      readonly pieces?: Readonly<Record<string, PieceMapSet>>;
      readonly disk?: DiskSet;
      readonly logs?: {
        readonly append: readonly LogRow[];
        readonly sourceEvictedCount: number;
        readonly retainedFromSequence: string;
        readonly deliveryResetCount: number;
        readonly lastDeliveryResetReason: string | null;
      };
    };

export type InspectionCommand =
  | {
      readonly type: "add_magnet";
      readonly magnet: string;
      readonly storageRoot: string;
    }
  | { readonly type: "choose_download_root"; readonly repairRoot?: string }
  | { readonly type: "set_default_download_root"; readonly rootId: string }
  | { readonly type: "set_show_add_options"; readonly show: boolean }
  | { readonly type: "remove_download_root"; readonly rootId: string }
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
  | "piece-retry"
  | "large-swarm"
  | "file-progress"
  | "disk-error"
  | "slow-disk-pressure"
  | "diagnostic-console"
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
  readonly storageRoot?: DownloadRoot | null;
}
