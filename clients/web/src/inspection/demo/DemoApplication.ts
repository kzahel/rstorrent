import type { InspectionApplication } from "../application";
import type {
  CommandResult,
  DesiredInspectionViews,
  DemoScenarioId,
  InspectionCommand,
  InspectionSnapshot,
  InspectionUpdate,
  KeyedPatch,
  FileRow,
  LogRow,
  PeerRow,
  TorrentRow,
  TrackerRow,
} from "../model";
import {
  buildScenarioSnapshot,
  DEMO_BASE_TIME_MS,
  DEMO_SCENARIOS,
  demoScenario,
} from "./catalog";

export interface DemoApplicationOptions {
  readonly scenarioId: DemoScenarioId;
  readonly elapsedMs?: number;
  readonly running?: boolean;
  readonly tickMs?: number;
}

export class DemoApplication implements InspectionApplication {
  readonly kind = "demo" as const;
  readonly scenarios = DEMO_SCENARIOS;

  private scenarioId: DemoScenarioId;
  private elapsedMs: number;
  private running: boolean;
  private readonly tickMs: number;
  private revision = 1;
  private readonly listeners = new Set<(update: InspectionUpdate) => void>();
  private timer: ReturnType<typeof setInterval> | null = null;
  private closed = false;
  private paused = new Set<string>();
  private archived = new Set<string>();
  private removed = new Set<string>();
  private extraTorrentCount = 0;
  private commandLogs: LogRow[] = [];
  private snapshot: InspectionSnapshot;
  private desiredViews: DesiredInspectionViews = {
    library: true,
    torrentId: null,
    detail: null,
  };

  constructor(options: DemoApplicationOptions) {
    this.scenarioId = options.scenarioId;
    const scenario = demoScenario(this.scenarioId);
    this.elapsedMs = clamp(options.elapsedMs ?? 0, 0, scenario.durationMs);
    this.running = options.running ?? scenario.autoplay;
    this.tickMs = clamp(options.tickMs ?? 1_000, 250, 60_000);
    this.snapshot = this.buildSnapshot();
  }

  subscribe(listener: (update: InspectionUpdate) => void): () => void {
    this.ensureOpen();
    this.listeners.add(listener);
    listener({ type: "snapshot", snapshot: this.snapshot });
    this.reconcileTimer();
    return () => {
      this.listeners.delete(listener);
      this.reconcileTimer();
    };
  }

  async setViews(views: DesiredInspectionViews): Promise<void> {
    this.ensureOpen();
    if (sameDesiredViews(this.desiredViews, views)) return;
    this.desiredViews = { ...views };
    this.replaceSnapshot();
  }

  async dispatch(command: InspectionCommand): Promise<CommandResult> {
    this.ensureOpen();
    switch (command.type) {
      case "set_demo_scenario":
        this.scenarioId = command.scenarioId;
        this.elapsedMs = 0;
        this.running = demoScenario(command.scenarioId).autoplay;
        this.resetOverlays();
        this.replaceSnapshot();
        return accepted(`Loaded ${demoScenario(command.scenarioId).title}`);
      case "set_demo_running":
        this.running = command.running;
        this.advance(0);
        this.reconcileTimer();
        return accepted(command.running ? "Demo clock running" : "Demo clock paused");
      case "advance_demo_clock":
        if (!Number.isFinite(command.milliseconds) || command.milliseconds < 0 || command.milliseconds > 600_000) {
          return rejected("Demo clock advances are limited to 10 minutes");
        }
        this.advance(command.milliseconds);
        return accepted(`Advanced ${Math.round(command.milliseconds / 1000)} seconds`);
      case "reset_demo":
        this.elapsedMs = 0;
        this.running = false;
        this.resetOverlays();
        this.replaceSnapshot();
        this.reconcileTimer();
        return accepted("Demo scenario reset");
      case "pause":
        if (this.snapshot.torrents[command.torrentId] === undefined) return rejected("Torrent is not present");
        this.paused.add(command.torrentId);
        this.addCommandLog("lifecycle", "Torrent paused in demo mode", command.torrentId);
        this.advance(0);
        return accepted("Torrent paused");
      case "resume":
        if (this.snapshot.torrents[command.torrentId] === undefined) return rejected("Torrent is not present");
        this.paused.delete(command.torrentId);
        this.addCommandLog("lifecycle", "Torrent resumed in demo mode", command.torrentId);
        this.advance(0);
        return accepted("Torrent resumed");
      case "archive":
        this.archived.add(command.torrentId);
        this.addCommandLog("lifecycle", "Torrent archived in demo mode", command.torrentId);
        this.advance(0);
        return accepted("Torrent archived");
      case "unarchive":
        this.archived.delete(command.torrentId);
        this.addCommandLog("lifecycle", "Torrent restored from archive", command.torrentId);
        this.advance(0);
        return accepted("Torrent restored");
      case "remove":
        if (this.snapshot.torrents[command.torrentId] === undefined) {
          return rejected("Torrent is not present");
        }
        this.removed.add(command.torrentId);
        this.addCommandLog(
          "lifecycle",
          command.deleteData
            ? "Torrent and managed data removed in demo mode"
            : "Torrent removed while retaining data in demo mode",
          command.torrentId,
        );
        this.advance(0);
        return accepted(command.deleteData ? "Torrent and data removed" : "Torrent removed");
      case "add_demo_torrent":
        this.extraTorrentCount = Math.min(8, this.extraTorrentCount + 1);
        this.addCommandLog("lifecycle", "Generated demo transfer added", null);
        this.advance(0);
        return accepted("Generated demo transfer added");
      case "add_magnet":
        return rejected("Live magnet add is unavailable in demo scenarios");
    }
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.stopTimer();
    this.listeners.clear();
  }

  private advance(milliseconds: number): void {
    const scenario = demoScenario(this.scenarioId);
    const previous = this.snapshot;
    this.elapsedMs = clamp(this.elapsedMs + milliseconds, 0, scenario.durationMs);
    if (this.elapsedMs >= scenario.durationMs) this.running = false;
    this.revision += 1;
    const next = this.buildSnapshot();
    const update = diffSnapshots(previous, next);
    this.snapshot = next;
    if (update !== null) this.emit(update);
    this.reconcileTimer();
  }

  private replaceSnapshot(): void {
    this.revision += 1;
    this.snapshot = this.buildSnapshot();
    this.emit({ type: "snapshot", snapshot: this.snapshot });
  }

  private buildSnapshot(): InspectionSnapshot {
    return materializeDemoViews(
      applyOverlays(
        buildScenarioSnapshot(
          this.scenarioId,
          this.elapsedMs,
          this.running,
          this.revision,
        ),
        this.paused,
        this.archived,
        this.removed,
        this.extraTorrentCount,
        this.commandLogs,
      ),
      this.desiredViews,
    );
  }

  private addCommandLog(
    category: string,
    summary: string,
    torrentId: string | null,
  ): void {
    this.commandLogs.push({
      id: `demo-command-${this.revision}-${this.commandLogs.length}`,
      timestampMs: DEMO_BASE_TIME_MS + this.elapsedMs,
      severity: "info",
      category,
      summary,
      torrentId,
    });
  }

  private resetOverlays(): void {
    this.paused = new Set();
    this.archived = new Set();
    this.removed = new Set();
    this.extraTorrentCount = 0;
    this.commandLogs = [];
  }

  private reconcileTimer(): void {
    const shouldRun = !this.closed && this.running && this.listeners.size > 0;
    if (shouldRun && this.timer === null) {
      this.timer = setInterval(() => this.advance(this.tickMs), this.tickMs);
    } else if (!shouldRun) {
      this.stopTimer();
    }
  }

  private stopTimer(): void {
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
  }

  private emit(update: InspectionUpdate): void {
    for (const listener of this.listeners) listener(update);
  }

  private ensureOpen(): void {
    if (this.closed) throw new Error("demo application is closed");
  }
}

function sameDesiredViews(
  left: DesiredInspectionViews,
  right: DesiredInspectionViews,
): boolean {
  return (
    left.library === right.library &&
    left.torrentId === right.torrentId &&
    left.detail === right.detail
  );
}

function materializeDemoViews(
  source: InspectionSnapshot,
  desired: DesiredInspectionViews,
): InspectionSnapshot {
  const selected =
    desired.torrentId === null ? undefined : source.torrents[desired.torrentId];
  const torrents = desired.library
    ? source.torrents
    : selected === undefined
      ? {}
      : { [selected.id]: selected };
  const peersByTorrent =
    desired.detail === "peers" && desired.torrentId !== null
      ? {
          [desired.torrentId]: source.peersByTorrent[desired.torrentId] ?? {
            order: [],
            rows: {},
          },
        }
      : {};
  const filesByTorrent =
    desired.detail === "files" && desired.torrentId !== null
      ? {
          [desired.torrentId]: source.filesByTorrent[desired.torrentId] ?? {
            state: "metadata_pending" as const,
            filesystemContentBase: null,
            order: [],
            rows: {},
          },
        }
      : {};
  const trackersByTorrent =
    desired.detail === "trackers" && desired.torrentId !== null
      ? {
          [desired.torrentId]: source.trackersByTorrent[desired.torrentId] ?? {
            state: "available" as const,
            order: [],
            rows: {},
          },
        }
      : {};
  return {
    ...source,
    torrentOrder: desired.library ? source.torrentOrder : [],
    torrents,
    peersByTorrent,
    filesByTorrent,
    trackersByTorrent,
    logs: desired.detail === "logs" ? source.logs : [],
    droppedLogs: desired.detail === "logs" ? source.droppedLogs : 0,
    viewStatus: {
      library: desired.library
        ? { status: "ready" }
        : { status: "not_requested" },
      torrentSummary:
        desired.torrentId === null
          ? { status: "not_requested" }
          : selected === undefined
            ? { status: "unavailable", reason: "Torrent is no longer present" }
            : { status: "ready" },
      peers:
        desired.detail === "peers"
          ? { status: "ready" }
          : { status: "not_requested" },
      files:
        desired.detail === "files"
          ? { status: "ready" }
          : { status: "not_requested" },
      trackers:
        desired.detail === "trackers"
          ? { status: "ready" }
          : { status: "not_requested" },
      logs:
        desired.detail === "logs"
          ? { status: "ready" }
          : { status: "not_requested" },
    },
  };
}

function applyOverlays(
  source: InspectionSnapshot,
  paused: ReadonlySet<string>,
  archived: ReadonlySet<string>,
  removed: ReadonlySet<string>,
  extraTorrentCount: number,
  commandLogs: readonly LogRow[],
): InspectionSnapshot {
  const torrents: Record<string, TorrentRow> = {};
  for (const id of source.torrentOrder) {
    if (removed.has(id)) continue;
    const row = source.torrents[id];
    if (row === undefined) continue;
    torrents[id] = {
      ...row,
      status: paused.has(id) ? "paused" : row.status,
      archived: archived.has(id),
      downloadRate: paused.has(id) ? 0 : row.downloadRate,
      uploadRate: paused.has(id) ? 0 : row.uploadRate,
      etaSeconds: paused.has(id) ? null : row.etaSeconds,
      progressReason: paused.has(id) ? "Paused in demo mode" : row.progressReason,
    };
  }

  const torrentOrder = source.torrentOrder.filter((id) => !removed.has(id));
  for (let index = 0; index < extraTorrentCount; index += 1) {
    const id = `f${String(index + 1).padStart(39, "0")}`;
    torrents[id] = {
      id,
      name: `Generated demo transfer ${index + 1}`,
      status: "downloading",
      sizeBytes: 734_003_200 + index * 104_857_600,
      progress: 0.08 + index * 0.03,
      downloadRate: 1_200_000 + index * 240_000,
      uploadRate: 0,
      downloadedBytes: 58_720_256 + index * 3_145_728,
      uploadedBytes: 0,
      peersConnected: 4 + index,
      peersKnown: 18 + index * 3,
      configuredTrackerCount: 0,
      etaSeconds: 540 - index * 22,
      addedAtMs: DEMO_BASE_TIME_MS + source.demo!.elapsedMs,
      archived: false,
      removalState: null,
      deleteManagedDataSupported: true,
      infoHash: id,
      error: null,
      progressReason: "Generated by the demo adapter",
    };
    torrentOrder.push(id);
  }

  const active = Object.values(torrents).filter(
    (row) => row.status === "downloading" || row.status === "metadata",
  );
  const logs = [...source.logs, ...commandLogs]
    .sort((left, right) => left.timestampMs - right.timestampMs)
    .slice(-256);
  return {
    ...source,
    session: {
      ...source.session,
      downloadRate: active.reduce((sum, row) => sum + row.downloadRate, 0),
      uploadRate: active.reduce((sum, row) => sum + (row.uploadRate ?? 0), 0),
      knownPeers: Object.values(torrents).reduce(
        (sum, row) => sum + (row.peersKnown ?? 0),
        0,
      ),
    },
    torrentOrder,
    torrents,
    filesByTorrent: Object.fromEntries(
      Object.entries(source.filesByTorrent).filter(([torrentId]) => !removed.has(torrentId)),
    ),
    trackersByTorrent: Object.fromEntries(
      Object.entries(source.trackersByTorrent).filter(
        ([torrentId]) => !removed.has(torrentId),
      ),
    ),
    logs,
    droppedLogs: source.droppedLogs + Math.max(0, source.logs.length + commandLogs.length - 256),
  };
}

function diffSnapshots(
  previous: InspectionSnapshot,
  next: InspectionSnapshot,
): InspectionUpdate | null {
  const torrentUpserts = next.torrentOrder
    .map((id) => next.torrents[id])
    .filter((row): row is TorrentRow => row !== undefined)
    .filter((row) => !shallowEqual(previous.torrents[row.id], row));
  const torrentRemoved = previous.torrentOrder.filter(
    (id) => next.torrents[id] === undefined,
  );
  const peerPatches: Array<
    KeyedPatch<PeerRow> & { readonly torrentId: string; readonly order: readonly string[] }
  > = [];
  const filePatches: Array<
    KeyedPatch<FileRow> & {
      readonly torrentId: string;
      readonly state: "metadata_pending" | "available" | "torrent_missing";
      readonly filesystemContentBase: string | null;
      readonly order: readonly string[];
    }
  > = [];
  const trackerPatches: Array<
    KeyedPatch<TrackerRow> & {
      readonly torrentId: string;
      readonly state: "available" | "torrent_missing";
      readonly order: readonly string[];
    }
  > = [];
  for (const [torrentId, nextSet] of Object.entries(next.peersByTorrent)) {
    const previousSet = previous.peersByTorrent[torrentId];
    const upsert = nextSet.order
      .map((id) => nextSet.rows[id])
      .filter((row): row is PeerRow => row !== undefined)
      .filter((row) => !shallowEqual(previousSet?.rows[row.connectionId], row));
    const removed =
      previousSet?.order.filter((id) => nextSet.rows[id] === undefined) ?? [];
    if (
      upsert.length > 0 ||
      removed.length > 0 ||
      !arraysEqual(previousSet?.order ?? [], nextSet.order)
    ) {
      peerPatches.push({ torrentId, upsert, removed, order: nextSet.order });
    }
  }
  for (const [torrentId, previousSet] of Object.entries(previous.peersByTorrent)) {
    if (next.peersByTorrent[torrentId] === undefined) {
      peerPatches.push({ torrentId, upsert: [], removed: previousSet.order, order: [] });
    }
  }
  for (const [torrentId, nextSet] of Object.entries(next.filesByTorrent)) {
    const previousSet = previous.filesByTorrent[torrentId];
    const upsert = nextSet.order
      .map((id) => nextSet.rows[id])
      .filter((row): row is FileRow => row !== undefined)
      .filter((row) => !shallowEqual(previousSet?.rows[row.id], row));
    const removed = previousSet?.order.filter((id) => nextSet.rows[id] === undefined) ?? [];
    if (
      upsert.length > 0 ||
      removed.length > 0 ||
      previousSet?.state !== nextSet.state ||
      previousSet?.filesystemContentBase !== nextSet.filesystemContentBase ||
      !arraysEqual(previousSet?.order ?? [], nextSet.order)
    ) {
      filePatches.push({
        torrentId,
        state: nextSet.state,
        filesystemContentBase: nextSet.filesystemContentBase,
        upsert,
        removed,
        order: nextSet.order,
      });
    }
  }
  for (const [torrentId, previousSet] of Object.entries(previous.filesByTorrent)) {
    if (next.filesByTorrent[torrentId] === undefined) {
      filePatches.push({
        torrentId,
        state: "torrent_missing",
        filesystemContentBase: null,
        upsert: [],
        removed: previousSet.order,
        order: [],
      });
    }
  }
  for (const [torrentId, nextSet] of Object.entries(next.trackersByTorrent)) {
    const previousSet = previous.trackersByTorrent[torrentId];
    const upsert = nextSet.order
      .map((id) => nextSet.rows[id])
      .filter((row): row is TrackerRow => row !== undefined)
      .filter((row) => !shallowEqual(previousSet?.rows[row.id], row));
    const removed =
      previousSet?.order.filter((id) => nextSet.rows[id] === undefined) ?? [];
    if (
      upsert.length > 0 ||
      removed.length > 0 ||
      previousSet?.state !== nextSet.state ||
      !arraysEqual(previousSet?.order ?? [], nextSet.order)
    ) {
      trackerPatches.push({
        torrentId,
        state: nextSet.state,
        upsert,
        removed,
        order: nextSet.order,
      });
    }
  }
  for (const [torrentId, previousSet] of Object.entries(
    previous.trackersByTorrent,
  )) {
    if (next.trackersByTorrent[torrentId] === undefined) {
      trackerPatches.push({
        torrentId,
        state: "torrent_missing",
        upsert: [],
        removed: previousSet.order,
        order: [],
      });
    }
  }

  const previousLogIds = new Set(previous.logs.map((row) => row.id));
  const appendedLogs = next.logs.filter((row) => !previousLogIds.has(row.id));
  return {
    type: "patch",
    revision: next.revision,
    session: next.session,
    ...(next.demo === null ? {} : { demo: next.demo }),
    ...(torrentUpserts.length === 0 &&
    torrentRemoved.length === 0 &&
    arraysEqual(previous.torrentOrder, next.torrentOrder)
      ? {}
      : {
          torrents: {
            upsert: torrentUpserts,
            removed: torrentRemoved,
            order: next.torrentOrder,
          },
        }),
    ...(peerPatches.length === 0 ? {} : { peers: peerPatches }),
    ...(filePatches.length === 0 ? {} : { files: filePatches }),
    ...(trackerPatches.length === 0 ? {} : { trackers: trackerPatches }),
    ...(appendedLogs.length === 0
      ? {}
      : {
          logs: {
            append: appendedLogs,
            dropped: Math.max(0, next.droppedLogs - previous.droppedLogs),
          },
        }),
  };
}

function shallowEqual<T extends object>(left: T | undefined, right: T): boolean {
  if (left === undefined) return false;
  const keys = Object.keys(right);
  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  return (
    keys.length === Object.keys(left).length &&
    keys.every((key) => leftRecord[key] === rightRecord[key])
  );
}

function arraysEqual(
  left: readonly string[],
  right: readonly string[],
): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function accepted(message: string): CommandResult {
  return { accepted: true, message };
}

function rejected(message: string): CommandResult {
  return { accepted: false, message };
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value));
}
