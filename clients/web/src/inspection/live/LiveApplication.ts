import type {
  ApiHello,
  DiagnosticEvent,
  PeerSourceView,
  PeerView,
  RequestEnvelope,
  TorrentState,
  TorrentView,
  ViewSnapshot,
  ViewSpec,
} from "../../api";
import type { ApplicationViewClient } from "../../api/client";
import { ViewController, type ViewControllerOptions } from "../../view-controller";
import type { ViewSetState } from "../../view-set-reducer";
import type { InspectionApplication } from "../application";
import type {
  CommandResult,
  DesiredInspectionViews,
  InspectionCommand,
  InspectionSnapshot,
  InspectionUpdate,
  LogRow,
  PeerRow,
  PeerSet,
  TorrentRow,
  ViewMaterialization,
} from "../model";

const LIBRARY_VIEW_ID = "library";
const SUMMARY_VIEW_ID = "torrent-summary";
const PEERS_VIEW_ID = "torrent-peers";
const LOGS_VIEW_ID = "logs";

export interface LiveApplicationOptions extends ViewControllerOptions {
  readonly initialViews?: DesiredInspectionViews;
}

export class LiveApplication implements InspectionApplication {
  readonly kind = "live" as const;
  readonly scenarios = [];

  private readonly listeners = new Set<(update: InspectionUpdate) => void>();
  private controller: ViewController | null = null;
  private desired: DesiredInspectionViews;
  private snapshot: InspectionSnapshot;
  private hello: ApiHello | null = null;
  private closed = false;
  private readonly requestInstanceId = generateRequestInstanceId();
  private requestSequence = 1;
  private removeWakeHints: (() => void) | null = null;

  private constructor(
    private readonly client: ApplicationViewClient,
    initialViews: DesiredInspectionViews,
  ) {
    this.desired = initialViews;
    this.snapshot = emptyLiveSnapshot(initialViews, "offline");
  }

  static async open(
    client: ApplicationViewClient,
    options: LiveApplicationOptions = {},
  ): Promise<LiveApplication> {
    const desired = options.initialViews ?? {
      library: true,
      torrentId: null,
      detail: null,
    };
    const application = new LiveApplication(client, desired);
    application.hello = await client.hello();
    const specs = application.viewSpecs(desired);
    application.controller = await ViewController.open(
      client,
      specs,
      (state) => application.acceptState(state),
      (error) => application.markReconnecting(error),
      options,
    );
    return application;
  }

  subscribe(listener: (update: InspectionUpdate) => void): () => void {
    this.ensureOpen();
    this.listeners.add(listener);
    listener({ type: "snapshot", snapshot: this.snapshot });
    return () => this.listeners.delete(listener);
  }

  async setViews(views: DesiredInspectionViews): Promise<void> {
    this.ensureOpen();
    if (sameViews(this.desired, views)) return;
    this.desired = { ...views };
    if (this.controller === null) return;
    this.snapshot = transitionSnapshot(
      this.snapshot,
      this.desired,
      this.capabilities(),
    );
    this.emit({ type: "snapshot", snapshot: this.snapshot });
    try {
      await this.controller.setViews(this.viewSpecs(views));
    } catch (error) {
      this.markReconnecting(asError(error));
      throw error;
    }
  }

  async dispatch(command: InspectionCommand): Promise<CommandResult> {
    this.ensureOpen();
    if (
      command.type !== "add_magnet" &&
      command.type !== "pause" &&
      command.type !== "resume" &&
      command.type !== "archive" &&
      command.type !== "unarchive" &&
      command.type !== "remove"
    ) {
      return {
        accepted: false,
        message: "This command is available only in named demo scenarios",
      };
    }
    const request: RequestEnvelope = {
      version: 1,
      request_id: `web-${this.requestInstanceId}-${this.requestSequence++}`,
      command:
        command.type === "add_magnet"
          ? {
              type: "add_magnet",
              magnet: command.magnet,
              storage_root: "downloads",
              skip_files: [],
            }
          : command.type === "remove"
            ? {
                type: "remove_torrent",
                torrent_id: command.torrentId,
                data: command.deleteData ? "delete_managed" : "keep",
              }
            : {
              type: command.type === "unarchive" ? "restore_archive" : command.type,
              torrent_id: command.torrentId,
            },
    };
    const response = await this.controller?.dispatch(request);
    if (response === undefined) {
      return { accepted: false, message: "Live controller is unavailable" };
    }
    if (response.status === "error") {
      return { accepted: false, message: response.error.message };
    }
    return {
      accepted: true,
      message:
        command.type === "add_magnet"
          ? "Torrent added"
          : command.type === "pause"
            ? "Torrent paused"
            : command.type === "resume"
              ? "Torrent resumed"
              : command.type === "archive"
                ? "Torrent archived"
                : command.type === "unarchive"
                  ? "Torrent restored"
                  : "Torrent removal started",
    };
  }

  requestImmediatePoll(): void {
    if (this.closed) return;
    this.controller?.requestImmediatePoll();
  }

  installBrowserWakeHints(targetWindow: Window, targetDocument: Document): void {
    this.ensureOpen();
    this.removeWakeHints?.();
    const wake = () => this.requestImmediatePoll();
    const visibility = () => {
      if (targetDocument.visibilityState === "visible") wake();
    };
    targetWindow.addEventListener("online", wake);
    targetWindow.addEventListener("pageshow", wake);
    targetDocument.addEventListener("visibilitychange", visibility);
    this.removeWakeHints = () => {
      targetWindow.removeEventListener("online", wake);
      targetWindow.removeEventListener("pageshow", wake);
      targetDocument.removeEventListener("visibilitychange", visibility);
    };
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.removeWakeHints?.();
    this.removeWakeHints = null;
    this.listeners.clear();
    await this.controller?.close();
    await this.client.close();
  }

  private acceptState(state: ViewSetState): void {
    if (this.closed) return;
    this.snapshot = mapViewState(
      state,
      this.desired,
      this.capabilities(),
      "connected",
    );
    this.emit({ type: "snapshot", snapshot: this.snapshot });
  }

  private markReconnecting(error: Error): void {
    if (this.closed) return;
    this.snapshot = {
      ...this.snapshot,
      session: { ...this.snapshot.session, connection: "reconnecting" },
      viewStatus: {
        library: staleIfMaterialized(this.snapshot.viewStatus.library, error),
        torrentSummary: staleIfMaterialized(
          this.snapshot.viewStatus.torrentSummary,
          error,
        ),
        peers: staleIfMaterialized(this.snapshot.viewStatus.peers, error),
        logs: staleIfMaterialized(this.snapshot.viewStatus.logs, error),
      },
    };
    this.emit({ type: "snapshot", snapshot: this.snapshot });
  }

  private capabilities(): ReadonlySet<string> {
    return new Set(this.hello?.capabilities ?? []);
  }

  private viewSpecs(views: DesiredInspectionViews): ViewSpec[] {
    const capabilities = this.capabilities();
    const specs: ViewSpec[] = [];
    if (views.library && capabilities.has("torrent_list")) {
      specs.push({
        type: "torrent_list",
        view_id: LIBRARY_VIEW_ID,
        delivery: { min_interval_millis: 100 },
      });
    }
    if (views.torrentId !== null && capabilities.has("torrent_summary")) {
      specs.push({
        type: "torrent_summary",
        view_id: SUMMARY_VIEW_ID,
        torrent_id: views.torrentId,
        delivery: { min_interval_millis: 100 },
      });
    }
    if (
      views.detail === "peers" &&
      views.torrentId !== null &&
      capabilities.has("torrent_peers")
    ) {
      specs.push({
        type: "torrent_peers",
        view_id: PEERS_VIEW_ID,
        torrent_id: views.torrentId,
        delivery: { min_interval_millis: 100 },
      });
    }
    if (views.detail === "logs" && capabilities.has("diagnostics")) {
      specs.push({
        type: "diagnostics",
        view_id: LOGS_VIEW_ID,
        torrent_id: views.torrentId,
        filter: {
          profile: "detailed",
          minimum_severity: "debug",
          categories: [],
        },
        delivery: { min_interval_millis: 100 },
      });
    }
    // Rust view sets intentionally require at least one view. A detail-only
    // unsupported state keeps the selected summary as navigation context.
    if (specs.length === 0 && capabilities.has("torrent_list")) {
      specs.push({
        type: "torrent_list",
        view_id: LIBRARY_VIEW_ID,
        delivery: { min_interval_millis: 100 },
      });
    }
    return specs;
  }

  private emit(update: InspectionUpdate): void {
    for (const listener of this.listeners) listener(update);
  }

  private ensureOpen(): void {
    if (this.closed) throw new Error("live inspection application is closed");
  }
}

function mapViewState(
  state: ViewSetState,
  desired: DesiredInspectionViews,
  capabilities: ReadonlySet<string>,
  connection: "connected" | "reconnecting" | "offline",
): InspectionSnapshot {
  const library = projection(state, LIBRARY_VIEW_ID, "torrent_list");
  const summary = projection(state, SUMMARY_VIEW_ID, "torrent");
  const peers = projection(state, PEERS_VIEW_ID, "peers");
  const diagnostics = projection(state, LOGS_VIEW_ID, "diagnostics");
  const torrentRows = new Map<string, TorrentRow>();
  if (library !== null) {
    for (const torrent of library.torrents) {
      torrentRows.set(torrent.torrent_id, mapTorrent(torrent));
    }
  }
  if (summary?.torrent !== null && summary?.torrent !== undefined) {
    torrentRows.set(summary.torrent.torrent_id, mapTorrent(summary.torrent));
  }
  const peerSet = peers === null ? null : mapPeers(peers.peers);
  const logs = diagnostics?.events.map(mapLog) ?? [];
  const torrents = Object.fromEntries(torrentRows);
  const torrentOrder = library?.torrents.map((torrent) => torrent.torrent_id) ?? [];
  const peersByTorrent =
    peerSet === null || desired.torrentId === null || peers?.torrent_id !== desired.torrentId
      ? {}
      : { [desired.torrentId]: peerSet };
  const rows = [...torrentRows.values()];
  return {
    revision: safeNumber(state.durableRevision),
    session: {
      connection,
      downloadRate: rows.reduce((total, row) => total + row.downloadRate, 0),
      uploadRate: null,
      dhtNodes: null,
      knownPeers: null,
    },
    demo: null,
    torrentOrder,
    torrents,
    peersByTorrent,
    logs,
    droppedLogs: diagnostics === null ? 0 : safeNumber(diagnostics.dropped_count),
    viewStatus: {
      library: materialization(
        desired.library,
        capabilities.has("torrent_list"),
        library !== null,
        "Torrent library is unavailable",
      ),
      torrentSummary: materialization(
        desired.torrentId !== null,
        capabilities.has("torrent_summary"),
        summary !== null,
        "Torrent summary is unavailable",
      ),
      peers: materialization(
        desired.detail === "peers",
        capabilities.has("torrent_peers"),
        peers?.torrent_id === desired.torrentId,
        "Peer inspection is unavailable",
      ),
      logs: materialization(
        desired.detail === "logs",
        capabilities.has("diagnostics"),
        diagnostics !== null,
        "Diagnostic logs are unavailable",
      ),
    },
  };
}

function projection<T extends ViewSnapshot["type"]>(
  state: ViewSetState,
  viewId: string,
  type: T,
): Extract<ViewSnapshot, { type: T }> | null {
  const view = state.views[viewId];
  return view?.type === type
    ? (view as Extract<ViewSnapshot, { type: T }>)
    : null;
}

function materialization(
  requested: boolean,
  supported: boolean,
  present: boolean,
  unavailableReason: string,
): ViewMaterialization {
  if (!requested) return { status: "not_requested" };
  if (!supported) {
    return { status: "unsupported", reason: unavailableReason };
  }
  if (present) return { status: "ready" };
  return { status: "loading" };
}

function transitionSnapshot(
  current: InspectionSnapshot,
  desired: DesiredInspectionViews,
  capabilities: ReadonlySet<string>,
): InspectionSnapshot {
  const selected =
    desired.torrentId === null ? undefined : current.torrents[desired.torrentId];
  const libraryRows = desired.library
    ? current.torrentOrder
        .map((id) => current.torrents[id])
        .filter((row): row is TorrentRow => row !== undefined)
    : [];
  const rows = new Map(libraryRows.map((row) => [row.id, row]));
  if (selected !== undefined) rows.set(selected.id, selected);
  return {
    ...current,
    torrentOrder: desired.library ? libraryRows.map((row) => row.id) : [],
    torrents: Object.fromEntries(rows),
    peersByTorrent: {},
    logs: [],
    droppedLogs: 0,
    viewStatus: {
      library: transitionStatus(
        desired.library,
        capabilities.has("torrent_list"),
        current.viewStatus.library,
      ),
      torrentSummary: transitionStatus(
        desired.torrentId !== null,
        capabilities.has("torrent_summary"),
      ),
      peers: transitionStatus(
        desired.detail === "peers",
        capabilities.has("torrent_peers"),
      ),
      logs: transitionStatus(
        desired.detail === "logs",
        capabilities.has("diagnostics"),
      ),
    },
  };
}

function transitionStatus(
  requested: boolean,
  supported: boolean,
  retained?: ViewMaterialization,
): ViewMaterialization {
  if (!requested) return { status: "not_requested" };
  if (!supported) return { status: "unsupported", reason: "View is unsupported" };
  if (retained?.status === "ready" || retained?.status === "stale") return retained;
  return { status: "loading" };
}

function mapTorrent(torrent: TorrentView): TorrentRow {
  const pieceCount = torrent.piece_count;
  return {
    id: torrent.torrent_id,
    name: `Torrent ${torrent.torrent_id.slice(0, 12)}`,
    status: mapTorrentState(torrent.state),
    sizeBytes: null,
    progress:
      pieceCount === 0 ? null : torrent.verified_piece_count / pieceCount,
    downloadRate: safeNumber(torrent.payload_download_rate_bytes),
    uploadRate: null,
    downloadedBytes: safeNumber(torrent.received_bytes),
    uploadedBytes: null,
    peersConnected: torrent.active_peer_connections,
    peersKnown: null,
    etaSeconds: null,
    addedAtMs: null,
    archived: torrent.archived,
    removalState: torrent.removal_state ?? null,
    deleteManagedDataSupported: torrent.delete_managed_data_supported,
    infoHash: torrent.torrent_id,
    error: torrent.error ?? null,
    progressReason: torrent.progress.reason.replaceAll("_", " "),
  };
}

function mapTorrentState(state: TorrentState): TorrentRow["status"] {
  switch (state) {
    case "awaiting_metadata":
      return "metadata";
    case "checking":
      return "checking";
    case "complete":
      return "complete";
    case "paused":
      return "paused";
    case "needs_repair":
    case "error":
      return "error";
    case "awaiting_storage":
    case "downloading":
    case "awaiting_publication":
      return "downloading";
  }
}

function generateRequestInstanceId(): string {
  const bytes = new Uint8Array(16);
  globalThis.crypto.getRandomValues(bytes);
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function mapPeers(peers: readonly PeerView[]): PeerSet {
  const rows = peers.map(mapPeer);
  return {
    order: rows.map((peer) => peer.connectionId),
    rows: Object.fromEntries(rows.map((peer) => [peer.connectionId, peer])),
  };
}

function mapPeer(peer: PeerView): PeerRow {
  const state: PeerRow["state"] =
    peer.lifecycle === "transport_connecting"
      ? "connecting"
      : peer.lifecycle === "protocol_handshaking"
        ? "handshaking"
        : peer.lifecycle === "disconnecting"
          ? "disconnecting"
          : peer.request_phase === "stalled"
            ? "stalled"
            : peer.remote_choking === true
              ? "choked"
              : "connected";
  return {
    connectionId: peer.connection_id,
    torrentId: peer.torrent_id,
    state,
    endpoint: peer.remote_endpoint,
    client: peer.client_name,
    source: mapPeerSource(peer.sources),
    progress: null,
    downloadRate: safeNullableNumber(peer.payload_download_rate_bytes),
    uploadRate: safeNullableNumber(peer.payload_upload_rate_bytes),
    downloadedBytes: safeNullableNumber(peer.payload_downloaded_bytes),
    uploadedBytes: safeNullableNumber(peer.payload_uploaded_bytes),
    requestsPending: peer.pending_requests,
    oldestRequestMs: safeNullableNumber(peer.oldest_request_age_millis),
    flags: [
      peer.supports_extensions === true ? "E" : "",
      peer.local_interested === true ? "I" : "",
      peer.remote_choking === true ? "C" : "",
    ].join(""),
    useful:
      safeNullableNumber(peer.payload_downloaded_bytes) !== null &&
      safeNullableNumber(peer.payload_downloaded_bytes)! > 0,
  };
}

function mapPeerSource(sources: readonly PeerSourceView[]): PeerRow["source"] {
  if (sources.includes("tracker")) return "tracker";
  if (sources.includes("dht")) return "dht";
  if (sources.includes("peer_exchange")) return "pex";
  if (sources.includes("manual") || sources.includes("magnet_hint")) return "manual";
  if (sources.includes("incoming")) return "incoming";
  if (sources.includes("cache")) return "cache";
  return "unknown";
}

function mapLog(event: DiagnosticEvent): LogRow {
  return {
    id: event.sequence,
    timestampMs: safeNumber(event.timestamp_millis),
    severity: event.severity === "trace" ? "debug" : event.severity,
    category: event.category,
    summary: event.summary,
    torrentId: event.torrent_id ?? null,
  };
}

function staleIfMaterialized(
  status: ViewMaterialization,
  error: Error,
): ViewMaterialization {
  return status.status === "ready" || status.status === "stale"
    ? { status: "stale", reason: error.message.slice(0, 240) }
    : status;
}

function safeNullableNumber(value: string | null): number | null {
  return value === null ? null : safeNumber(value);
}

function safeNumber(value: string): number {
  try {
    const parsed = BigInt(value);
    if (parsed < 0n) return 0;
    return Number(parsed > BigInt(Number.MAX_SAFE_INTEGER) ? Number.MAX_SAFE_INTEGER : parsed);
  } catch {
    return 0;
  }
}

function emptyLiveSnapshot(
  desired: DesiredInspectionViews,
  connection: "offline" | "reconnecting",
): InspectionSnapshot {
  return {
    revision: 0,
    session: {
      connection,
      downloadRate: 0,
      uploadRate: null,
      dhtNodes: null,
      knownPeers: null,
    },
    demo: null,
    torrentOrder: [],
    torrents: {},
    peersByTorrent: {},
    logs: [],
    droppedLogs: 0,
    viewStatus: {
      library: desired.library ? { status: "loading" } : { status: "not_requested" },
      torrentSummary:
        desired.torrentId === null ? { status: "not_requested" } : { status: "loading" },
      peers: desired.detail === "peers" ? { status: "loading" } : { status: "not_requested" },
      logs: desired.detail === "logs" ? { status: "loading" } : { status: "not_requested" },
    },
  };
}

function sameViews(
  left: DesiredInspectionViews,
  right: DesiredInspectionViews,
): boolean {
  return (
    left.library === right.library &&
    left.torrentId === right.torrentId &&
    left.detail === right.detail
  );
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
