import type {
  DemoScenarioId,
  DemoScenarioSummary,
  DiskPieceRow,
  DiskSet,
  FileRow,
  FileSet,
  InspectionSnapshot,
  LogRow,
  PeerRow,
  PieceMapSet,
  TorrentRow,
  TrackerRow,
  TrackerSet,
} from "../model";
import { emptyDiskSet } from "../state";

const BASE_TIME_MS = Date.UTC(2026, 7, 1, 8, 0, 0);
const BUNNY_ID = "a962f460b83861cfb5faa1d7ad7da9c3f3cc2fc4";
const SINTEL_ID = "08ada5a7a6183aae1e09d831df6748d566095a10";
const ARCH_ID = "e2d1f50a5d72bfc9c4d6c3f9913b1dfb2cf4f210";

export const DEMO_SCENARIOS: readonly DemoScenarioSummary[] = [
  {
    id: "healthy-download",
    title: "Healthy download",
    description: "Metadata, useful peers, sustained transfer, and completion.",
    durationMs: 110_000,
    autoplay: true,
  },
  {
    id: "stalled-metadata",
    title: "Stalled metadata",
    description: "Candidates exist, but no metadata request is making progress.",
    durationMs: 120_000,
    autoplay: true,
  },
  {
    id: "tracker-recovery",
    title: "Tracker recovery",
    description: "A UDP timeout backs off, retries, and discovers peers.",
    durationMs: 80_000,
    autoplay: true,
  },
  {
    id: "endgame",
    title: "Endgame",
    description: "The final blocks duplicate safely and converge on completion.",
    durationMs: 35_000,
    autoplay: true,
  },
  {
    id: "piece-retry",
    title: "Piece hash retry",
    description: "A corrupt attempt fails, clears, and retries from clean state.",
    durationMs: 30_000,
    autoplay: true,
  },
  {
    id: "large-swarm",
    title: "Large swarm",
    description: "2,000 torrents and 10,000 peers for rendering pressure.",
    durationMs: 60_000,
    autoplay: false,
  },
  {
    id: "file-progress",
    title: "File progress",
    description: "A 4,096-row file tree with stored and verified progress boundaries.",
    durationMs: 90_000,
    autoplay: true,
  },
  {
    id: "slow-disk-pressure",
    title: "Slow disk pressure",
    description: "Receive buffers fill, intake pauses, drains, and resumes.",
    durationMs: 70_000,
    autoplay: true,
  },
  {
    id: "disk-error",
    title: "Disk error",
    description: "A storage write fails and leaves an actionable stopped state.",
    durationMs: 60_000,
    autoplay: false,
  },
  {
    id: "diagnostic-console",
    title: "Diagnostic console",
    description: "Mixed structured records, scopes, severities, and a busy ordered feed.",
    durationMs: 45_000,
    autoplay: true,
  },
  {
    id: "empty-library",
    title: "Empty library",
    description: "A clean first-run library with no active torrents.",
    durationMs: 60_000,
    autoplay: false,
  },
];

export function isDemoScenarioId(value: string | null): value is DemoScenarioId {
  return DEMO_SCENARIOS.some((scenario) => scenario.id === value);
}

export function demoScenario(id: DemoScenarioId): DemoScenarioSummary {
  const result = DEMO_SCENARIOS.find((scenario) => scenario.id === id);
  if (result === undefined) throw new Error(`unknown demo scenario: ${id}`);
  return result;
}

export function buildScenarioSnapshot(
  scenarioId: DemoScenarioId,
  elapsedMs: number,
  running: boolean,
  revision: number,
): InspectionSnapshot {
  const scenario = demoScenario(scenarioId);
  const elapsed = clamp(elapsedMs, 0, scenario.durationMs);
  const content = buildScenarioContent(scenarioId, elapsed);
  const torrents = Object.fromEntries(content.torrents.map((row) => [row.id, row]));
  const peersByTorrent = Object.fromEntries(
    Object.entries(content.peers).map(([torrentId, rows]) => [
      torrentId,
      {
        order: rows.map((row) => row.connectionId),
        rows: Object.fromEntries(rows.map((row) => [row.connectionId, row])),
      },
    ]),
  );
  const filesByTorrent = content.files ?? {};
  const trackersByTorrent = content.trackers ?? {};
  const piecesByTorrent = content.pieces ?? {};
  const disk = content.disk ?? emptyDiskSet();
  const active = content.torrents.filter(
    (torrent) => torrent.status === "downloading" || torrent.status === "metadata",
  );
  return {
    revision,
    session: {
      connection: "demo",
      downloadRate: sum(active.map((torrent) => torrent.downloadRate)),
      uploadRate: sum(active.map((torrent) => torrent.uploadRate ?? 0)),
      dhtNodes: scenarioId === "empty-library" ? 0 : 638,
      knownPeers: sum(content.torrents.map((torrent) => torrent.peersKnown ?? 0)),
    },
    demo: {
      scenarioId,
      elapsedMs: elapsed,
      running,
      durationMs: scenario.durationMs,
    },
    storage: { roots: [], defaultRoot: null, showAddOptions: true },
    torrentOrder: content.torrents.map((torrent) => torrent.id),
    torrents,
    peersByTorrent,
    filesByTorrent,
    trackersByTorrent,
    piecesByTorrent,
    disk,
    logs: content.logs.slice(-2_048),
    logLoss: {
      sourceEvictedCount: 0,
      retainedFromSequence: content.logs.at(-2_048)?.id ?? "1",
      localEvictedCount: Math.max(0, content.logs.length - 2_048),
      deliveryResetCount: 0,
      lastDeliveryResetReason: null,
    },
    viewStatus: {
      library: { status: "ready" },
      torrentSummary: { status: "ready" },
      peers: { status: "ready" },
      files: { status: "ready" },
      trackers: { status: "ready" },
      pieces: { status: "ready" },
      disk: { status: "ready" },
      logs: { status: "ready" },
    },
  };
}

interface ScenarioContent {
  readonly torrents: readonly TorrentRow[];
  readonly peers: Readonly<Record<string, readonly PeerRow[]>>;
  readonly files?: Readonly<Record<string, FileSet>>;
  readonly trackers?: Readonly<Record<string, TrackerSet>>;
  readonly pieces?: Readonly<Record<string, PieceMapSet>>;
  readonly disk?: DiskSet;
  readonly logs: readonly LogRow[];
}

function buildScenarioContent(
  scenarioId: DemoScenarioId,
  elapsedMs: number,
): ScenarioContent {
  const content = (() => {
    switch (scenarioId) {
    case "healthy-download":
      return healthyDownload(elapsedMs);
    case "stalled-metadata":
      return stalledMetadata(elapsedMs);
    case "tracker-recovery":
      return trackerRecovery(elapsedMs);
    case "endgame":
      return endgame(elapsedMs);
    case "piece-retry":
      return pieceRetry(elapsedMs);
    case "large-swarm":
      return largeSwarm();
    case "file-progress":
      return fileProgress(elapsedMs);
    case "slow-disk-pressure":
      return slowDiskPressure(elapsedMs);
    case "disk-error":
      return diskError(elapsedMs);
    case "diagnostic-console":
      return diagnosticConsole(elapsedMs);
    case "empty-library":
      return { torrents: [], peers: {}, logs: [] };
    }
  })();
  return {
    ...content,
    pieces: buildPieceMaps(scenarioId, elapsedMs, content.torrents),
  };
}

function healthyDownload(elapsedMs: number): ScenarioContent {
  const seconds = elapsedMs / 1000;
  const metadata = seconds < 7;
  const progress = metadata ? null : clamp((seconds - 7) / 90, 0, 1);
  const complete = progress === 1;
  const rate = metadata || complete ? 0 : 11_400_000 + wave(seconds, 2_600_000);
  const peers = metadata ? buildPeers(BUNNY_ID, 8, seconds, 0.28) : buildPeers(BUNNY_ID, 36, seconds, progress ?? 0);
  const primary = torrent({
    id: BUNNY_ID,
    name: "Big Buck Bunny 1080p surround",
    status: complete ? "complete" : metadata ? "metadata" : "downloading",
    sizeBytes: metadata ? null : 276_445_467,
    progress,
    downloadRate: rate,
    uploadRate: complete ? 310_000 : 82_000,
    peersConnected: peers.length,
    peersKnown: 143,
    etaSeconds: progress === null || complete ? null : Math.ceil((276_445_467 * (1 - progress)) / Math.max(1, rate)),
    progressReason: complete ? "All pieces verified" : metadata ? "Requesting metadata from 8 peers" : "Receiving useful blocks from multiple peers",
  });
  const completed = torrent({
    id: ARCH_ID,
    name: "Arch Linux 2026.08.01 x86_64",
    status: "complete",
    sizeBytes: 1_265_348_608,
    progress: 1,
    uploadRate: 1_820_000,
    uploadedBytes: 3_812_881_408,
    peersConnected: 6,
    peersKnown: 47,
    progressReason: "Seeding to 6 peers",
    addedAtMs: BASE_TIME_MS - 86_400_000,
  });
  const paused = torrent({
    id: SINTEL_ID,
    name: "Sintel 4K open movie",
    status: "paused",
    sizeBytes: 4_294_967_296,
    progress: 0.642,
    downloadedBytes: 2_757_797_003,
    peersConnected: 0,
    peersKnown: 91,
    progressReason: "Paused by user",
    addedAtMs: BASE_TIME_MS - 7_200_000,
  });
  return {
    torrents: [primary, completed, paused],
    peers: { [BUNNY_ID]: peers },
    files: { [BUNNY_ID]: demoFileSet(BUNNY_ID, progress ?? 0, 36) },
    logs: timelineLogs(BUNNY_ID, [
      [0, "info", "lifecycle", "Torrent added from magnet"],
      [1, "info", "tracker", "Announce started for 3 UDP trackers"],
      [2, "info", "dht", "DHT lookup returned 54 peer candidates"],
      [4, "debug", "peer", "Eight metadata connections established"],
      [7, "info", "metadata", "Metadata verified: 1,055 pieces, 3 files"],
      [8, "info", "scheduler", "Content request window opened across 16 peers"],
      [25, "debug", "storage", "Write queue healthy: 18 jobs, 288 KiB retained"],
      [62, "info", "piece", "Verified 50% of wanted payload"],
      [94, "debug", "scheduler", "Entering bounded endgame"],
      [98, "info", "integrity", "All pieces verified"],
      [99, "info", "storage", "Published 3 files"],
    ], elapsedMs),
  };
}

function diagnosticConsole(elapsedMs: number): ScenarioContent {
  const base = healthyDownload(45_000);
  const categories = [
    "lifecycle.session",
    "tracker.announce",
    "discovery.dht",
    "peer.connection",
    "metadata.exchange",
    "scheduler.request",
    "piece.block",
    "storage.io",
    "integrity.hash",
    "performance.backpressure",
  ] as const;
  const severities = ["info", "debug", "trace", "info", "warning"] as const;
  const messages = [
    "Application profile opened",
    "Tracker announce completed with fresh peers",
    "DHT lookup returned candidate endpoints",
    "Peer extension handshake completed",
    "Piece request window advanced",
    "Storage queue crossed its high watermark",
    "Piece hash verification succeeded",
  ] as const;
  const count = Math.min(10_000, 100 + Math.floor(elapsedMs / 4.5));
  const logs: LogRow[] = Array.from({ length: count }, (_, index) => {
    const category = categories[index % categories.length] ?? "lifecycle.session";
    const torrentId =
      index % 11 === 0 ? null : index % 5 === 0 ? SINTEL_ID : BUNNY_ID;
    const pieceIndex = (index * 17) % 1_055;
    return {
      id: String(index + 1),
      timestampMs: BASE_TIME_MS + index * 210,
      severity: severities[index % severities.length] ?? "info",
      category,
      code: `${category.replaceAll(".", "_")}_${index % 4}`,
      message: messages[index % messages.length] ?? "Diagnostic activity observed",
      torrentId,
      subjects:
        torrentId === null
          ? []
          : category === "piece.block" || category === "integrity.hash"
            ? [{ type: "piece", piece_index: pieceIndex, attempt: 1 + (index % 3) }]
            : category === "tracker.announce"
              ? [{ type: "tracker", tracker_id: "udp://tracker.example:6969/announce" }]
              : [],
      fields: [
        { key: "event_index", value: { type: "count", value: String(index) } },
        ...(category === "storage.io"
          ? [
              {
                key: "queued_bytes",
                value: { type: "bytes" as const, value: String(524_288 + index * 16_384) },
              },
            ]
          : []),
      ],
    };
  });
  return { ...base, logs };
}

function fileProgress(elapsedMs: number): ScenarioContent {
  const progress = clamp(0.18 + elapsedMs / 160_000, 0, 0.92);
  const ordinaryDoneProgress = clamp(0.18 + elapsedMs / 120_000, 0, 0.94);
  const hashFailureActive = elapsedMs >= 36_000 && elapsedMs < 48_000;
  const doneProgress = hashFailureActive
    ? Math.min(ordinaryDoneProgress, progress + 0.005)
    : ordinaryDoneProgress;
  const rate = 18_600_000 + wave(elapsedMs / 1_000, 3_200_000);
  const files = demoFileSet(BUNNY_ID, doneProgress, 4_096, progress);
  const total = Object.values(files.rows).reduce(
    (sum, file) => sum + Number(file.lengthBytes),
    0,
  );
  return {
    torrents: [
      torrent({
        id: BUNNY_ID,
        name: "Open Movies production archive",
        status: "downloading",
        sizeBytes: total,
        progress,
        downloadRate: rate,
        uploadRate: 84_000,
        peersConnected: 22,
        peersKnown: 138,
        etaSeconds: Math.ceil((total * (1 - progress)) / rate),
        progressReason: "Stored blocks lead verified pieces in the active file",
      }),
    ],
    peers: { [BUNNY_ID]: buildPeers(BUNNY_ID, 22, elapsedMs / 1_000, progress) },
    files: { [BUNNY_ID]: files },
    logs: timelineLogs(
      BUNNY_ID,
      [
        [0, "info", "metadata", "Metadata verified: 4,096 files"],
        [2, "info", "storage", "Managed file layout prepared"],
        [8, "debug", "piece", "Stored blocks crossed a file boundary"],
        [18, "debug", "integrity", "Verified piece advanced two file rows"],
        [36, "warning", "piece", "Hash failure regressed unverified Done bytes"],
      ],
      elapsedMs,
    ),
  };
}

function demoFileSet(
  torrentId: string,
  doneProgress: number,
  count: number,
  verifiedProgress = doneProgress,
): FileSet {
  const rows: FileRow[] = [];
  let offset = 0n;
  for (let index = 0; index < count; index += 1) {
    const padding = index === count - 3;
    const length = padding
      ? 262_144n
      : BigInt(1_200_000 + ((index * 7919) % 9_000_000));
    const fileDoneProgress = clamp(doneProgress * count - index, 0, 1);
    const fileVerifiedProgress = Math.min(
      fileDoneProgress,
      clamp(verifiedProgress * count - index, 0, 1),
    );
    const done = padding
      ? length
      : BigInt(Math.floor(Number(length) * fileDoneProgress));
    const verified = padding
      ? length
      : BigInt(Math.floor(Number(length) * fileVerifiedProgress));
    const folder = padding
      ? ".pad"
      : index % 5 === 0
        ? "featurettes/behind-the-scenes"
        : index % 3 === 0
          ? "audio/lossless"
          : "video/chapter-reels";
    const name = padding
      ? length.toString()
      : `asset-${String(index + 1).padStart(3, "0")}.${index % 5 === 0 ? "mkv" : index % 3 === 0 ? "flac" : "mp4"}`;
    const path = [folder, name];
    const id = index.toString();
    rows.push({
      id,
      torrentId,
      index,
      path,
      name,
      folder,
      extension: padding ? "" : name.split(".").at(-1) ?? "",
      lengthBytes: length.toString(),
      torrentOffsetBytes: offset.toString(),
      firstPiece: Math.floor(Number(offset / 262_144n)),
      lastPiece: Math.floor(Number((offset + length - 1n) / 262_144n)),
      selection: padding ? null : index % 29 === 0 ? "skipped" : "wanted",
      padding,
      doneBytes: done.toString(),
      verifiedBytes: verified.toString(),
      storagePath: `/Users/demo/Downloads/${torrentId}/${path.join("/")}`,
    });
    offset += length;
  }
  return {
    state: "available",
    filesystemContentBase: `/Users/demo/Downloads/${torrentId}`,
    order: rows.map((row) => row.id),
    rows: Object.fromEntries(rows.map((row) => [row.id, row])),
  };
}

function stalledMetadata(elapsedMs: number): ScenarioContent {
  const seconds = elapsedMs / 1000;
  const peers = buildPeers(BUNNY_ID, 18, seconds, 0).map((peer, index) => ({
    ...peer,
    state: index < 6 ? ("connected" as const) : peer.state,
    downloadRate: 0,
    requestsPending: 0,
    useful: false,
    flags: index < 6 ? (["download_choked"] as const) : [],
  }));
  return {
    torrents: [
      torrent({
        id: BUNNY_ID,
        name: "Big Buck Bunny (magnet metadata)",
        status: "metadata",
        sizeBytes: null,
        progress: null,
        peersConnected: peers.length,
        peersKnown: 127,
        progressReason: "Candidates available; no metadata request is active",
      }),
    ],
    peers: { [BUNNY_ID]: peers },
    logs: timelineLogs(BUNNY_ID, [
      [0, "info", "lifecycle", "Torrent added from magnet"],
      [2, "info", "dht", "DHT lookup returned 127 candidates"],
      [5, "debug", "peer", "Six extension handshakes completed"],
      [12, "warning", "metadata", "No metadata request has been active for 7 seconds"],
      [30, "warning", "scheduler", "Metadata peers remain connected but unproductive"],
      [60, "warning", "metadata", "Stall persists; candidate replacement is available"],
    ], elapsedMs),
  };
}

function trackerRecovery(elapsedMs: number): ScenarioContent {
  const seconds = elapsedMs / 1000;
  const recovered = seconds >= 22;
  const metadata = seconds < 29;
  const progress = !recovered || metadata ? null : clamp((seconds - 29) / 42, 0, 1);
  const peers = recovered ? buildPeers(BUNNY_ID, 14, seconds, progress ?? 0) : [];
  if (seconds >= 45 && peers[0] !== undefined) {
    peers[0] = {
      ...peers[0],
      connectionId: `${BUNNY_ID.slice(0, 8)}-reconnect-00001`,
      state: "connected",
      useful: true,
    };
  }
  const rate = progress === null || progress === 1 ? 0 : 8_800_000 + wave(seconds, 1_200_000);
  const observedAtMs = Date.now();
  const primaryStatus: TrackerRow["status"] =
    seconds < 3
      ? "announcing"
      : seconds < 22
        ? "retry_wait"
        : "reannounce_wait";
  const primary: TrackerRow = {
    id: "udp://tracker.openbittorrent.com:80",
    torrentId: BUNNY_ID,
    url: "udp://tracker.openbittorrent.com:80",
    transport: "udp",
    source: "magnet",
    tier: 0,
    status: primaryStatus,
    announceEvent: seconds < 3 ? "started" : null,
    totalAttempts: seconds < 22 ? 1 : 2,
    consecutiveFailures: seconds >= 3 && seconds < 22 ? 1 : 0,
    lastPeerCount: seconds < 22 ? null : 42,
    seeders: seconds < 22 ? null : 31,
    leechers: seconds < 22 ? null : 11,
    intervalSeconds: seconds < 22 ? null : 600,
    nextAction:
      seconds < 3 ? null : seconds < 22 ? "retry" : "reannounce",
    nextActionInMs:
      seconds < 3
        ? null
        : seconds < 22
          ? Math.max(0, (22 - seconds) * 1_000)
          : Math.max(0, (622 - seconds) * 1_000),
    observedAtMs,
    lastSuccessAgeMs: seconds < 22 ? null : (seconds - 22) * 1_000,
    lastFailureAgeMs: seconds < 3 ? null : (seconds - 3) * 1_000,
    error: seconds >= 3 && seconds < 22 ? "UDP announce timed out" : null,
  };
  const fallback: TrackerRow = {
    id: "udp://tracker.opentrackr.org:1337",
    torrentId: BUNNY_ID,
    url: "udp://tracker.opentrackr.org:1337",
    transport: "udp",
    source: "magnet",
    tier: 0,
    status: seconds < 22 ? "idle" : "reannounce_wait",
    announceEvent: null,
    totalAttempts: seconds < 22 ? 0 : 1,
    consecutiveFailures: 0,
    lastPeerCount: seconds < 22 ? null : 18,
    seeders: seconds < 22 ? null : 14,
    leechers: seconds < 22 ? null : 4,
    intervalSeconds: seconds < 22 ? null : 900,
    nextAction: seconds < 22 ? "announce" : "reannounce",
    nextActionInMs:
      seconds < 22 ? 0 : Math.max(0, (922 - seconds) * 1_000),
    observedAtMs,
    lastSuccessAgeMs: seconds < 22 ? null : (seconds - 22) * 1_000,
    lastFailureAgeMs: null,
    error: null,
  };
  return {
    torrents: [
      torrent({
        id: BUNNY_ID,
        name: "Big Buck Bunny via tracker retry",
        status: progress === 1 ? "complete" : metadata ? "metadata" : "downloading",
        sizeBytes: metadata ? null : 276_445_467,
        progress,
        downloadRate: rate,
        peersConnected: peers.length,
        peersKnown: recovered ? 42 : 0,
        configuredTrackerCount: 2,
        etaSeconds: progress === null || progress === 1 ? null : Math.ceil((276_445_467 * (1 - progress)) / Math.max(1, rate)),
        progressReason: recovered ? (metadata ? "Tracker recovered; negotiating metadata" : "Downloading from recovered tracker cohort") : "UDP tracker retry scheduled in 22 seconds",
      }),
    ],
    peers: { [BUNNY_ID]: peers },
    trackers: {
      [BUNNY_ID]: {
        state: "available",
        order: [primary.id, fallback.id],
        rows: { [primary.id]: primary, [fallback.id]: fallback },
      },
    },
    logs: timelineLogs(BUNNY_ID, [
      [0, "info", "tracker", "UDP announce started"],
      [3, "warning", "tracker", "UDP announce timed out"],
      [3, "info", "tracker", "Retry scheduled with bounded backoff"],
      [22, "info", "tracker", "Retry succeeded with 42 candidates"],
      [24, "debug", "peer", "Fourteen connection attempts admitted"],
      [29, "info", "metadata", "Metadata verified after tracker recovery"],
    ], elapsedMs),
  };
}

function endgame(elapsedMs: number): ScenarioContent {
  const seconds = elapsedMs / 1000;
  const progress = clamp(0.987 + seconds / 2200, 0, 1);
  const complete = progress === 1;
  const peers = buildPeers(BUNNY_ID, 24, seconds, progress).map((peer, index) => ({
    ...peer,
    requestsPending: complete ? 0 : index < 10 ? 2 + (index % 3) : peer.requestsPending,
    oldestRequestMs: complete ? null : 280 + index * 19,
  }));
  return {
    torrents: [
      torrent({
        id: BUNNY_ID,
        name: "Big Buck Bunny — endgame inspection",
        status: complete ? "complete" : "downloading",
        sizeBytes: 276_445_467,
        progress,
        downloadRate: complete ? 0 : 3_600_000 + wave(seconds, 900_000),
        peersConnected: 24,
        peersKnown: 186,
        etaSeconds: complete ? null : Math.max(1, Math.ceil(31 - seconds)),
        progressReason: complete ? "All duplicate attempts canceled and pieces verified" : "Strict endgame: 18 blocks have bounded duplicate owners",
      }),
    ],
    peers: { [BUNNY_ID]: peers },
    logs: timelineLogs(BUNNY_ID, [
      [0, "info", "scheduler", "Entering strict endgame with 61 blocks remaining"],
      [3, "debug", "peer", "Duplicate request admitted for block 1048:12"],
      [7, "debug", "protocol", "Cancel sent to losing request owner"],
      [14, "debug", "piece", "Late duplicate payload ignored safely"],
      [29, "info", "integrity", "All 1,055 pieces verified"],
      [30, "info", "storage", "Publication complete; request owners drained"],
    ], elapsedMs),
  };
}

function pieceRetry(elapsedMs: number): ScenarioContent {
  const seconds = elapsedMs / 1_000;
  const retrying = seconds >= 12;
  const recovered = seconds >= 24;
  const progress = recovered ? 0.427 : 0.426;
  return {
    torrents: [
      torrent({
        id: BUNNY_ID,
        name: "Big Buck Bunny — hash retry inspection",
        status: "downloading",
        sizeBytes: 276_445_467,
        progress,
        downloadRate: seconds >= 9 && seconds < 12 ? 0 : 4_800_000,
        peersConnected: 18,
        peersKnown: 121,
        etaSeconds: 36,
        progressReason: recovered
          ? "Piece 450 verified on its clean retry"
          : retrying
            ? "Piece 450 is being fetched again after a hash failure"
            : "Piece 450 attempt 1 is moving through storage and hashing",
      }),
    ],
    peers: { [BUNNY_ID]: buildPeers(BUNNY_ID, 18, seconds, progress) },
    logs: timelineLogs(BUNNY_ID, [
      [0, "debug", "piece", "Piece 450 attempt 1 requested"],
      [7, "debug", "storage", "Piece 450 attempt 1 entered hashing"],
      [9, "warning", "integrity", "Piece 450 attempt 1 failed its SHA-1 check"],
      [12, "info", "scheduler", "Piece 450 attempt 2 admitted from clean state"],
      [20, "debug", "storage", "Piece 450 attempt 2 entered hashing"],
      [24, "info", "integrity", "Piece 450 attempt 2 verified"],
    ], elapsedMs),
  };
}

function largeSwarm(): ScenarioContent {
  const torrents: TorrentRow[] = [];
  for (let index = 0; index < 2_000; index += 1) {
    const progress = ((index * 37) % 1000) / 1000;
    const id = fixedId(index + 10_000);
    torrents.push(
      torrent({
        id,
        name: `Scale fixture ${String(index + 1).padStart(4, "0")} — ${index % 3 === 0 ? "long multi-file archive and sample media" : "open dataset"}`,
        status: index % 19 === 0 ? "paused" : index % 23 === 0 ? "complete" : "downloading",
        sizeBytes: 250_000_000 + index * 1_048_576,
        progress: index % 23 === 0 ? 1 : progress,
        downloadRate: index % 19 === 0 ? 0 : 80_000 + ((index * 7919) % 18_000_000),
        peersConnected: index % 31,
        peersKnown: 30 + (index % 260),
        etaSeconds: 60 + (index * 17) % 86_400,
        progressReason: "Synthetic scale row",
        addedAtMs: BASE_TIME_MS - index * 60_000,
      }),
    );
  }
  const selectedId = torrents[0]?.id ?? fixedId(10_000);
  if (torrents[0] !== undefined) {
    torrents[0] = {
      ...torrents[0],
      status: "downloading",
      progress: 0.54,
      peersConnected: 10_000,
    };
  }
  return {
    torrents,
    peers: { [selectedId]: buildPeers(selectedId, 10_000, 17, 0.54) },
    logs: timelineLogs(selectedId, [
      [0, "info", "performance", "Loaded 2,000 torrents and 10,000 peers"],
    ], 60_000),
  };
}

function buildPieceMaps(
  scenarioId: DemoScenarioId,
  elapsedMs: number,
  torrents: readonly TorrentRow[],
): Readonly<Record<string, PieceMapSet>> {
  const torrent = torrents[0];
  if (torrent === undefined || torrent.progress === null) return {};

  const pieceCount = scenarioId === "large-swarm" ? 250_000 : 1_055;
  const verified = new Uint8Array(pieceCount);
  let verifiedCount = Math.min(
    pieceCount,
    Math.max(0, Math.floor(torrent.progress * pieceCount)),
  );
  if (scenarioId === "piece-retry") verifiedCount = 450;
  verified.fill(1, 0, verifiedCount);

  const active = [];
  if (scenarioId === "piece-retry") {
    const seconds = elapsedMs / 1_000;
    if (seconds < 9) {
      active.push(pieceSummary(450, 1, seconds < 4 ? "received" : "hashing", seconds));
    } else if (seconds < 12) {
      active.push(pieceSummary(450, 1, "failed", seconds, "SHA-1 mismatch"));
    } else if (seconds < 24) {
      active.push(
        pieceSummary(
          450,
          2,
          seconds < 16 ? "requested" : seconds < 20 ? "received" : "hashing",
          seconds - 12,
        ),
      );
    } else {
      verified[450] = 1;
    }
  } else if (verifiedCount < pieceCount && scenarioId !== "stalled-metadata") {
    const activeCount = scenarioId === "large-swarm" ? 6 : scenarioId === "endgame" ? 8 : 4;
    const stages = ["requested", "received", "stored", "hashing"] as const;
    for (let index = 0; index < activeCount; index += 1) {
      const pieceIndex = Math.min(pieceCount - 1, verifiedCount + index * 3);
      active.push(
        pieceSummary(
          pieceIndex,
          1,
          stages[index % stages.length] ?? "requested",
          elapsedMs / 1_000 + index,
        ),
      );
    }
  }

  return {
    [torrent.id]: {
      torrentId: torrent.id,
      pieceCount,
      verified,
      active,
      revision: Math.floor(elapsedMs / 250),
    },
  };
}

function pieceSummary(
  pieceIndex: number,
  attempt: number,
  stage: "requested" | "received" | "stored" | "hashing" | "failed",
  ageSeconds: number,
  error: string | null = null,
) {
  const pieceLength = 256 * 1_024;
  const receivedBytes =
    stage === "requested" ? 48 * 1_024 : stage === "received" ? 192 * 1_024 : pieceLength;
  return {
    id: `${pieceIndex}:${attempt}`,
    pieceIndex,
    attempt,
    pieceLength,
    stage,
    requestedBytes: pieceLength,
    receivedBytes,
    storedBytes: stage === "stored" || stage === "hashing" || stage === "failed" ? pieceLength : 0,
    ageMillis: Math.max(0, Math.floor(ageSeconds * 1_000)),
    error,
  } as const;
}

function diskError(elapsedMs: number): ScenarioContent {
  const failed = elapsedMs >= 7_000;
  return {
    torrents: [
      torrent({
        id: BUNNY_ID,
        name: "Big Buck Bunny — storage failure",
        status: failed ? "error" : "downloading",
        sizeBytes: 276_445_467,
        progress: 0.341,
        downloadRate: failed ? 0 : 6_200_000,
        peersConnected: failed ? 0 : 12,
        peersKnown: 88,
        etaSeconds: failed ? null : 29,
        error: failed ? "Write failed: destination has no free space" : null,
        progressReason: failed ? "Storage requires user action before transfer can resume" : "Downloading normally before the injected failure",
      }),
    ],
    peers: { [BUNNY_ID]: failed ? [] : buildPeers(BUNNY_ID, 12, elapsedMs / 1000, 0.341) },
    disk: demoDiskSet({
      elapsedMs,
      pressure: failed ? "error" : "normal",
      residentBytes: failed ? 0 : 8 * 1024 * 1024,
      queuedWriteBytes: failed ? 0 : 6 * 1024 * 1024,
      writingBytes: failed ? 0 : 256 * 1024,
      pieceCount: failed ? 1 : 18,
      error: failed ? "Write failed: destination has no free space" : null,
    }),
    logs: timelineLogs(BUNNY_ID, [
      [0, "info", "storage", "Opened staging files"],
      [5, "warning", "storage", "Write latency exceeded 2 seconds"],
      [7, "error", "storage", "Write failed: no free space on destination"],
      [7, "info", "lifecycle", "Content peers stopped; torrent intent retained"],
    ], elapsedMs),
  };
}

function slowDiskPressure(elapsedMs: number): ScenarioContent {
  const seconds = elapsedMs / 1_000;
  const pressure =
    seconds < 10
      ? "normal"
      : seconds < 36
        ? "backpressured"
        : seconds < 52
          ? "draining"
          : "normal";
  const residentBytes =
    pressure === "backpressured"
      ? 29 * 1024 * 1024
      : pressure === "draining"
        ? Math.max(4 * 1024 * 1024, (29 - (seconds - 36) * 1.4) * 1024 * 1024)
        : seconds >= 52
          ? 3 * 1024 * 1024
          : (4 + seconds * 2.1) * 1024 * 1024;
  const pieceCount = pressure === "backpressured" ? 64 : pressure === "draining" ? 30 : 12;
  const progress = clamp(0.21 + elapsedMs / 180_000, 0, 0.61);
  return {
    torrents: [
      torrent({
        id: BUNNY_ID,
        name: "Big Buck Bunny — slow storage",
        status: "downloading",
        sizeBytes: 276_445_467,
        progress,
        downloadRate: pressure === "backpressured" ? 0 : 12_400_000,
        peersConnected: 22,
        peersKnown: 109,
        progressReason:
          pressure === "backpressured"
            ? "Disk high watermark paused new payload assignment"
            : pressure === "draining"
              ? "Disk queue draining below the low watermark"
              : "Storage intake and peer requests are flowing",
      }),
    ],
    peers: { [BUNNY_ID]: buildPeers(BUNNY_ID, 22, seconds, progress) },
    disk: demoDiskSet({
      elapsedMs,
      pressure,
      residentBytes,
      queuedWriteBytes:
        pressure === "backpressured" ? 23 * 1024 * 1024 : residentBytes * 0.65,
      writingBytes: 256 * 1024,
      pieceCount,
      error: null,
    }),
    logs: timelineLogs(BUNNY_ID, [
      [0, "info", "storage", "Storage pipeline opened with a 32 MiB resident limit"],
      [10, "warning", "storage", "Resident payload crossed the high watermark; intake paused"],
      [36, "info", "storage", "Resident payload fell below the low watermark; intake resumed"],
      [52, "info", "storage", "Storage pipeline returned to normal pressure"],
    ], elapsedMs),
  };
}

function demoDiskSet(input: {
  readonly elapsedMs: number;
  readonly pressure: DiskSet["pipeline"]["pressure"];
  readonly residentBytes: number;
  readonly queuedWriteBytes: number;
  readonly writingBytes: number;
  readonly pieceCount: number;
  readonly error: string | null;
}): DiskSet {
  const pieces: DiskPieceRow[] = Array.from({ length: input.pieceCount }, (_, index) => {
    const stage: DiskPieceRow["stage"] =
      input.error !== null
        ? "failed"
        : index % 9 === 0
          ? "hashing"
          : index % 3 === 0
            ? "writing"
            : index % 3 === 1
              ? "queued"
              : "receiving";
    const length = 256 * 1024;
    const stored = stage === "hashing" ? length : Math.min(length, (index % 8) * 32 * 1024);
    return {
      id: `${BUNNY_ID}:${220 + index}:1`,
      torrentId: BUNNY_ID,
      torrentName: "Big Buck Bunny — slow storage",
      pieceIndex: 220 + index,
      pieceLength: length,
      attempt: 1,
      stage,
      requestedBytes: length,
      receivedBytes: Math.min(length, stored + 64 * 1024),
      storedBytes: stored,
      ageMillis: 1_200 + index * 37,
      stageAgeMillis: 90 + index * 19,
      error: input.error,
    };
  });
  const received = 58 * 1024 * 1024 + input.elapsedMs * 8_000;
  const stored = 52 * 1024 * 1024 + input.elapsedMs * 4_500;
  return {
    pipeline: {
      pressure: input.pressure,
      checkpointStage: input.pressure === "error" ? "error" : "syncing",
      intakeBackpressured: input.pressure === "backpressured",
      sampleMillis: 1_000,
      residentLimitBytes: 32 * 1024 * 1024,
      residentHighWatermarkBytes: 24 * 1024 * 1024,
      residentLowWatermarkBytes: 16 * 1024 * 1024,
      requestedBytes:
        input.pressure === "backpressured" ? 18 * 1024 * 1024 : 42 * 1024 * 1024,
      residentBytes: Math.round(input.residentBytes),
      queuedWriteBytes: Math.round(input.queuedWriteBytes),
      writingBytes: input.writingBytes,
      hashingBytes: 256 * 1024,
      checkpointDirtyPieces: 18,
      checkpointDirtyBytes: 18 * 256 * 1024,
      checkpointDirtyPieceHighWater: 64,
      checkpointDirtyByteHighWater: 64 * 256 * 1024,
      checkpointOldestDirtyMillis: 1_240,
      checkpointBatchesStarted: 7,
      checkpointBatchesCompleted: 6,
      checkpointPiecesCompleted: 146,
      checkpointSyncOperationsCompleted: 12,
      checkpointSyncServiceMicros: 820_000,
      checkpointSyncServiceMaxMicros: 190_000,
      checkpointCommitServiceMicros: 115_000,
      checkpointCommitServiceMaxMicros: 26_000,
      checkpointActiveMicros: 42_000,
      storageJobsPending: input.pieceCount * 6,
      receivedBytesTotal: received,
      storedBytesTotal: stored,
      verifiedBytesTotal: Math.max(0, stored - 2 * 1024 * 1024),
      receiveRateBytes: input.pressure === "backpressured" ? 0 : 12_400_000,
      writeRateBytes: 4_500_000,
      hashRateBytes: 4_200_000,
      writeOperationsStarted: 820 + Math.floor(input.elapsedMs / 90),
      writeOperationsCompleted: 819 + Math.floor(input.elapsedMs / 90),
      hashOperationsStarted: 220 + Math.floor(input.elapsedMs / 700),
      hashOperationsCompleted: 219 + Math.floor(input.elapsedMs / 700),
      writeQueueWaitMicros: 9_800_000,
      writeQueueWaitMaxMicros:
        input.pressure === "backpressured" ? 2_800_000 : 180_000,
      writeServiceMicros: 25_600_000,
      writeServiceMaxMicros: 1_900_000,
      hashQueueWaitMicros: 1_100_000,
      hashQueueWaitMaxMicros: 90_000,
      hashServiceMicros: 7_400_000,
      hashServiceMaxMicros: 240_000,
      pressureTransitionCount: input.pressure === "normal" ? 2 : 1,
      backpressuredMillisTotal: Math.max(
        0,
        Math.min(input.elapsedMs - 10_000, 26_000),
      ),
      lastError: input.error,
    },
    order: pieces.map((piece) => piece.id),
    rows: Object.fromEntries(pieces.map((piece) => [piece.id, piece])),
  };
}

function torrent(input: Partial<TorrentRow> & Pick<TorrentRow, "id" | "name" | "status">): TorrentRow {
  const progress = input.progress ?? null;
  const size = input.sizeBytes ?? null;
  return {
    id: input.id,
    name: input.name,
    status: input.status,
    sizeBytes: size,
    progress,
    downloadRate: input.downloadRate ?? 0,
    uploadRate: input.uploadRate ?? 0,
    downloadedBytes: input.downloadedBytes ?? (size === null || progress === null ? 0 : Math.floor(size * progress)),
    uploadedBytes: input.uploadedBytes ?? 0,
    peersConnected: input.peersConnected ?? 0,
    peersKnown: input.peersKnown ?? 0,
    configuredTrackerCount: input.configuredTrackerCount ?? 0,
    etaSeconds: input.etaSeconds ?? null,
    addedAtMs: input.addedAtMs ?? BASE_TIME_MS - 3_600_000,
    archived: input.archived ?? false,
    removalState: input.removalState ?? null,
    deleteManagedDataSupported: input.deleteManagedDataSupported ?? true,
    infoHash: input.infoHash ?? input.id,
    error: input.error ?? null,
    progressReason: input.progressReason ?? "Waiting for activity",
  };
}

function buildPeers(
  torrentId: string,
  count: number,
  seconds: number,
  torrentProgress: number,
): PeerRow[] {
  const clients = ["libtorrent 2.0.13", "qBittorrent 5.1", "Transmission 4.1", "Deluge 2.2", "WebTorrent 2.8", "RSTorrent dev"];
  const sources = ["tracker", "dht", "tracker", "dht", "pex", "manual"] as const;
  const rows: PeerRow[] = [];
  for (let index = 0; index < count; index += 1) {
    const connected = index % 11 !== 0;
    const stalled = connected && index % 17 === 0;
    const choked = connected && !stalled && index % 7 === 0;
    const useful = connected && !stalled && !choked && index % 5 !== 0;
    const rate = useful ? 110_000 + ((index * 7919 + Math.floor(seconds) * 997) % 2_300_000) : 0;
    const connectionId = `${torrentId.slice(0, 8)}-connection-${String(index + 1).padStart(5, "0")}`;
    rows.push({
      connectionId,
      torrentId,
      state: !connected ? "connecting" : stalled ? "stalled" : choked ? "choked" : "connected",
      endpoint: index % 9 === 0 ? `[2001:db8::${(index % 240) + 1}]:${51413 + (index % 7)}` : `198.51.100.${(index % 250) + 1}:${49152 + (index % 12000)}`,
      client: clients[index % clients.length] ?? "Unknown",
      source: sources[index % sources.length] ?? "tracker",
      progress: clamp(torrentProgress + ((index * 13) % 40) / 100, 0, 1),
      downloadRate: rate,
      uploadRate: connected && index % 4 === 0 ? 18_000 + (index * 187) % 130_000 : 0,
      downloadedBytes: Math.floor(rate * Math.max(1, seconds * (0.3 + (index % 8) / 10))),
      uploadedBytes: connected ? (index * 65_537) % 80_000_000 : 0,
      requestsPending: useful ? 1 + (index % 12) : 0,
      oldestRequestMs: useful ? 45 + ((index * 83) % 2_900) : null,
      flags: demoPeerFlags(index, connected, choked, useful),
      useful,
    });
  }
  return rows;
}

function demoPeerFlags(
  index: number,
  connected: boolean,
  choked: boolean,
  useful: boolean,
): PeerRow["flags"] {
  if (!connected) return [];
  return [
    ...(index % 9 === 0 ? (["incoming"] as const) : []),
    choked ? "download_choked" : useful ? "download_allowed" : "download_choked",
    "upload_choked",
    ...(index % 3 !== 0 ? (["extension_protocol"] as const) : []),
    ...(index % 5 === 0 ? (["utp"] as const) : []),
  ];
}

type TimelineEntry = readonly [
  second: number,
  severity: LogRow["severity"],
  category: string,
  message: string,
];

function timelineLogs(
  torrentId: string,
  entries: readonly TimelineEntry[],
  elapsedMs: number,
): LogRow[] {
  return entries
    .filter(([second]) => second * 1000 <= elapsedMs)
    .map(([second, severity, category, message], index) => ({
      id: String(second * 100 + index + 1),
      timestampMs: BASE_TIME_MS + second * 1000,
      severity,
      category,
      code: `${category.replaceAll(".", "_")}_event`,
      message,
      torrentId,
      subjects: [],
      fields: [],
    }));
}

function fixedId(index: number): string {
  return index.toString(16).padStart(40, "0").slice(-40);
}

function wave(seconds: number, amplitude: number): number {
  return Math.round(Math.sin(seconds / 3.7) * amplitude);
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value));
}

function sum(values: readonly number[]): number {
  return values.reduce((total, value) => total + value, 0);
}

export const DEMO_PRIMARY_TORRENT_ID = BUNNY_ID;
export const DEMO_BASE_TIME_MS = BASE_TIME_MS;
