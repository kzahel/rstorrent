import type {
  DemoScenarioId,
  DemoScenarioSummary,
  InspectionSnapshot,
  LogRow,
  PeerRow,
  TorrentRow,
} from "../model";

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
    id: "large-swarm",
    title: "Large swarm",
    description: "2,000 torrents and 10,000 peers for rendering pressure.",
    durationMs: 60_000,
    autoplay: false,
  },
  {
    id: "disk-error",
    title: "Disk error",
    description: "A storage write fails and leaves an actionable stopped state.",
    durationMs: 60_000,
    autoplay: false,
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
  const active = content.torrents.filter(
    (torrent) => torrent.status === "downloading" || torrent.status === "metadata",
  );
  return {
    revision,
    session: {
      connection: "demo",
      downloadRate: sum(active.map((torrent) => torrent.downloadRate)),
      uploadRate: sum(active.map((torrent) => torrent.uploadRate)),
      dhtNodes: scenarioId === "empty-library" ? 0 : 638,
      knownPeers: sum(content.torrents.map((torrent) => torrent.peersKnown)),
    },
    demo: {
      scenarioId,
      elapsedMs: elapsed,
      running,
      durationMs: scenario.durationMs,
    },
    torrentOrder: content.torrents.map((torrent) => torrent.id),
    torrents,
    peersByTorrent,
    logs: content.logs.slice(-256),
    droppedLogs: Math.max(0, content.logs.length - 256),
  };
}

interface ScenarioContent {
  readonly torrents: readonly TorrentRow[];
  readonly peers: Readonly<Record<string, readonly PeerRow[]>>;
  readonly logs: readonly LogRow[];
}

function buildScenarioContent(
  scenarioId: DemoScenarioId,
  elapsedMs: number,
): ScenarioContent {
  switch (scenarioId) {
    case "healthy-download":
      return healthyDownload(elapsedMs);
    case "stalled-metadata":
      return stalledMetadata(elapsedMs);
    case "tracker-recovery":
      return trackerRecovery(elapsedMs);
    case "endgame":
      return endgame(elapsedMs);
    case "large-swarm":
      return largeSwarm();
    case "disk-error":
      return diskError(elapsedMs);
    case "empty-library":
      return { torrents: [], peers: {}, logs: [] };
  }
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
    peersConnected: peers.filter((peer) => peer.state !== "connecting").length,
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

function stalledMetadata(elapsedMs: number): ScenarioContent {
  const seconds = elapsedMs / 1000;
  const peers = buildPeers(BUNNY_ID, 18, seconds, 0).map((peer, index) => ({
    ...peer,
    state: index < 6 ? ("connected" as const) : peer.state,
    downloadRate: 0,
    requestsPending: 0,
    useful: false,
    flags: index < 6 ? "d" : "",
  }));
  return {
    torrents: [
      torrent({
        id: BUNNY_ID,
        name: "Big Buck Bunny (magnet metadata)",
        status: "metadata",
        sizeBytes: null,
        progress: null,
        peersConnected: 6,
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
  return {
    torrents: [
      torrent({
        id: BUNNY_ID,
        name: "Big Buck Bunny via tracker retry",
        status: progress === 1 ? "complete" : metadata ? "metadata" : "downloading",
        sizeBytes: metadata ? null : 276_445_467,
        progress,
        downloadRate: rate,
        peersConnected: peers.filter((peer) => peer.state === "connected").length,
        peersKnown: recovered ? 42 : 0,
        etaSeconds: progress === null || progress === 1 ? null : Math.ceil((276_445_467 * (1 - progress)) / Math.max(1, rate)),
        progressReason: recovered ? (metadata ? "Tracker recovered; negotiating metadata" : "Downloading from recovered tracker cohort") : "UDP tracker retry scheduled in 22 seconds",
      }),
    ],
    peers: { [BUNNY_ID]: peers },
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
  return {
    torrents,
    peers: { [selectedId]: buildPeers(selectedId, 10_000, 17, 0.54) },
    logs: timelineLogs(selectedId, [
      [0, "info", "performance", "Loaded 2,000 torrents and 10,000 peers"],
    ], 60_000),
  };
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
    logs: timelineLogs(BUNNY_ID, [
      [0, "info", "storage", "Opened staging files"],
      [5, "warning", "storage", "Write latency exceeded 2 seconds"],
      [7, "error", "storage", "Write failed: no free space on destination"],
      [7, "info", "lifecycle", "Content peers stopped; torrent intent retained"],
    ], elapsedMs),
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
    etaSeconds: input.etaSeconds ?? null,
    addedAtMs: input.addedAtMs ?? BASE_TIME_MS - 3_600_000,
    archived: input.archived ?? false,
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
      flags: !connected ? "" : choked ? "d" : useful ? "D u" : "d u",
      useful,
    });
  }
  return rows;
}

type TimelineEntry = readonly [
  second: number,
  severity: LogRow["severity"],
  category: string,
  summary: string,
];

function timelineLogs(
  torrentId: string,
  entries: readonly TimelineEntry[],
  elapsedMs: number,
): LogRow[] {
  return entries
    .filter(([second]) => second * 1000 <= elapsedMs)
    .map(([second, severity, category, summary], index) => ({
      id: `${torrentId.slice(0, 8)}-${second}-${index}`,
      timestampMs: BASE_TIME_MS + second * 1000,
      severity,
      category,
      summary,
      torrentId,
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
