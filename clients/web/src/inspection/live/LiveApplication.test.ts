import { describe, expect, it } from "vitest";

import type {
  ApiHello,
  OpenViewSetRequest,
  OpenViewSetResponse,
  PeerView,
  RequestEnvelope,
  ResponseEnvelope,
  TorrentView,
  UpdateBatch,
  UpdateViewSetRequest,
  ViewSetUpdate,
  ViewSpec,
} from "../../api";
import type { ApplicationViewClient } from "../../api/client";
import { HttpApiError } from "../../api/client";
import type { InspectionSnapshot } from "../model";
import { LiveApplication } from "./LiveApplication";

const TORRENT_ID = "000102030405060708090a0b0c0d0e0f10111213";

class FakeLiveClient implements ApplicationViewClient {
  readonly updates: UpdateViewSetRequest[] = [];
  readonly opens: OpenViewSetRequest[] = [];
  readonly requests: RequestEnvelope[] = [];
  openCount = 0;
  private rejectPoll: ((error: Error) => void) | null = null;

  async hello(): Promise<ApiHello> {
    return {
      api: { current: 1, minimum: 1 },
      encodings: ["json"],
      deliveries: ["poll", "long_poll"],
      capabilities: [
        "torrent_list",
        "torrent_summary",
        "torrent_peers",
        "torrent_files",
        "torrent_trackers",
        "diagnostics",
      ],
      limits: {
        max_view_sets_per_owner: 8,
        max_views_per_set: 16,
        max_view_id_bytes: 64,
        min_queue_bytes: 16_384,
        default_queue_bytes: 262_144,
        max_queue_bytes: 524_288,
        max_snapshot_bytes: 16_777_216,
        max_wait_millis: 20_000,
        lease_millis: "300000",
      },
    };
  }

  async dispatch(request: RequestEnvelope): Promise<ResponseEnvelope> {
    this.requests.push(request);
    return {
      version: 1,
      request_id: request.request_id,
      revision: "4",
      status: "success",
      snapshot: {
        profile_id: "live",
        revision: "4",
        storage: { roots: [], show_add_options: true },
        torrents: [],
      },
    };
  }

  async chooseDownloadRoot(): Promise<null> {
    return null;
  }

  async openViewSet(request: OpenViewSetRequest): Promise<OpenViewSetResponse> {
    this.opens.push(request);
    this.openCount += 1;
    const viewSetId = `vs_${String(this.openCount).padStart(32, "0")}`;
    const initial: UpdateBatch = {
      api_version: 1,
      view_set_id: viewSetId,
      epoch: String(this.openCount),
      base_cursor: "0",
      cursor: "1",
      durable_revision: "4",
      updates: request.views.map((view) => snapshotFor(view, this.openCount)),
    };
    return {
      view_set_id: viewSetId,
      lease_millis: "300000",
      effective_queue_bytes: 262_144,
      effective_views: request.views,
      initial,
    };
  }

  async updateViewSet(
    _viewSetId: string,
    request: UpdateViewSetRequest,
  ): Promise<void> {
    this.updates.push(request);
  }

  async nextUpdates(
    _viewSetId: string,
    _after: string,
    _waitMillis: number,
    signal?: AbortSignal,
  ): Promise<UpdateBatch> {
    return new Promise((_resolve, reject) => {
      this.rejectPoll = reject;
      signal?.addEventListener("abort", () => reject(new Error("aborted")), {
        once: true,
      });
    });
  }

  async closeViewSet(): Promise<void> {}
  async close(): Promise<void> {}

  expireViewSet(): void {
    this.rejectPoll?.(
      new HttpApiError(404, "unknown_view_set", "view-set lease expired"),
    );
    this.rejectPoll = null;
  }
}

describe("LiveApplication", () => {
  it("maps semantic magnet intake to the bounded application command", async () => {
    const client = new FakeLiveClient();
    const application = await LiveApplication.open(client);
    const magnet =
      "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213";

    await expect(
      application.dispatch({
        type: "add_magnet",
        magnet,
        storageRoot: "root_a",
        startContent: false,
      }),
    ).resolves.toEqual({ accepted: true, message: "Torrent added" });
    expect(client.requests).toHaveLength(1);
    expect(client.requests[0]?.command).toEqual({
      type: "add_magnet",
      magnet,
      storage_root: "root_a",
      start_content: false,
      skip_files: [],
    });
    await application.close();
  });

  it("maps sorted live file priority changes to the application command", async () => {
    const client = new FakeLiveClient();
    const application = await LiveApplication.open(client);

    await expect(
      application.dispatch({
        type: "set_file_priority",
        torrentId: TORRENT_ID,
        fileIndices: [9, 2, 4],
        priority: "skip",
      }),
    ).resolves.toEqual({ accepted: true, message: "Selected files skipped" });
    expect(client.requests[0]?.command).toEqual({
      type: "set_file_priority",
      torrent_id: TORRENT_ID,
      file_indices: [2, 4, 9],
      priority: "skip",
    });
    await application.close();
  });

  it("uses a unique bounded request namespace for each application instance", async () => {
    const firstClient = new FakeLiveClient();
    const secondClient = new FakeLiveClient();
    const first = await LiveApplication.open(firstClient);
    const second = await LiveApplication.open(secondClient);

    await first.dispatch({ type: "archive", torrentId: TORRENT_ID });
    await first.dispatch({ type: "unarchive", torrentId: TORRENT_ID });
    await second.dispatch({ type: "archive", torrentId: TORRENT_ID });

    const firstIds = firstClient.requests.map((request) => request.request_id);
    const secondId = secondClient.requests[0]?.request_id;
    expect(firstIds[0]).toMatch(/^web-[0-9a-f]{32}-1$/);
    expect(firstIds[1]).toMatch(/^web-[0-9a-f]{32}-2$/);
    expect(secondId).toMatch(/^web-[0-9a-f]{32}-1$/);
    expect(secondId).not.toBe(firstIds[0]);
    expect(firstIds.every((requestId) => requestId.length <= 128)).toBe(true);

    await first.close();
    await second.close();
  });

  it("maps archive and explicit retention removal commands", async () => {
    const client = new FakeLiveClient();
    const application = await LiveApplication.open(client);

    await application.dispatch({ type: "archive", torrentId: TORRENT_ID });
    await application.dispatch({
      type: "remove",
      torrentId: TORRENT_ID,
      deleteData: true,
    });

    expect(client.requests.map((request) => request.command)).toEqual([
      { type: "archive", torrent_id: TORRENT_ID },
      {
        type: "remove_torrent",
        torrent_id: TORRENT_ID,
        data: "delete_managed",
      },
    ]);
    await application.close();
  });

  it("maps active peer state and evicts obsolete responsive views", async () => {
    const client = new FakeLiveClient();
    const application = await LiveApplication.open(client, {
      initialViews: {
        library: true,
        torrentId: TORRENT_ID,
        detail: "peers",
        logCapture: null,
      },
    });
    const snapshots: InspectionSnapshot[] = [];
    application.subscribe((update) => {
      if (update.type === "snapshot") snapshots.push(update.snapshot);
    });

    const initial = snapshots.at(-1)!;
    expect(initial.torrents[TORRENT_ID]?.name).toBe("movie.mkv");
    expect(initial.torrents[TORRENT_ID]?.configuredTrackerCount).toBe(2);
    const peer = initial.peersByTorrent[TORRENT_ID]?.rows["connection-1"];
    expect(peer?.state).toBe("handshaking");
    expect(peer?.client).toBeNull();
    expect(peer?.downloadRate).toBeNull();
    expect(peer?.flags).toEqual([
      "incoming",
      "download_choked",
      "extension_protocol",
      "metadata_extension",
      "utp",
    ]);
    expect(initial.viewStatus.peers.status).toBe("ready");

    await application.setViews({
      library: false,
      torrentId: TORRENT_ID,
      detail: "logs",
      logCapture: { profile: "normal", torrentId: null },
    });
    expect(client.updates.at(-1)?.views.map((view) => view.type)).toEqual([
      "torrent_summary",
      "diagnostics",
    ]);
    expect(client.updates.at(-1)?.views.at(-1)).toMatchObject({
      type: "diagnostics",
      torrent_id: null,
      filter: { profile: "normal", minimum_severity: "info" },
    });
    await application.setViews({
      library: false,
      torrentId: TORRENT_ID,
      detail: "logs",
      logCapture: { profile: "trace", torrentId: TORRENT_ID },
    });
    expect(client.updates.at(-1)?.views.at(-1)).toMatchObject({
      type: "diagnostics",
      torrent_id: TORRENT_ID,
      filter: { profile: "trace", minimum_severity: "trace" },
    });
    const transition = snapshots.at(-1)!;
    expect(transition.torrentOrder).toEqual([]);
    expect(transition.peersByTorrent).toEqual({});
    expect(transition.viewStatus.peers.status).toBe("not_requested");
    expect(transition.viewStatus.logs.status).toBe("loading");
    await application.close();
  });

  it("subscribes to files only while requested and maps exact byte strings", async () => {
    const client = new FakeLiveClient();
    const application = await LiveApplication.open(client, {
      initialViews: {
        library: false,
        torrentId: TORRENT_ID,
        detail: "files",
        logCapture: null,
      },
    });
    const snapshots: InspectionSnapshot[] = [];
    application.subscribe((update) => {
      if (update.type === "snapshot") snapshots.push(update.snapshot);
    });
    const file = snapshots.at(-1)?.filesByTorrent[TORRENT_ID]?.rows["0"];
    expect(file).toMatchObject({
      name: "movie.mkv",
      lengthBytes: "9007199254740993",
      doneBytes: "16384",
      verifiedBytes: "0",
      storagePath: "/tmp/content/video/movie.mkv",
    });
    expect(
      client.opens[0]?.views.find((view) => view.type === "torrent_files")
        ?.delivery.min_interval_millis,
    ).toBe(250);
    await application.setViews({
      library: true,
      torrentId: TORRENT_ID,
      detail: "general",
      logCapture: null,
    });
    expect(client.updates.at(-1)?.views.map((view) => view.type)).toEqual([
      "torrent_list",
      "torrent_summary",
    ]);
    expect(snapshots.at(-1)?.filesByTorrent).toEqual({});
    await application.close();
  });

  it("subscribes to trackers only while requested and maps retained state", async () => {
    const client = new FakeLiveClient();
    const application = await LiveApplication.open(client, {
      initialViews: {
        library: false,
        torrentId: TORRENT_ID,
        detail: "trackers",
        logCapture: null,
      },
    });
    const snapshots: InspectionSnapshot[] = [];
    application.subscribe((update) => {
      if (update.type === "snapshot") snapshots.push(update.snapshot);
    });
    const tracker =
      snapshots.at(-1)?.trackersByTorrent[TORRENT_ID]?.rows[
        "udp://tracker.example:6969"
      ];
    expect(tracker).toMatchObject({
      torrentId: TORRENT_ID,
      status: "retry_wait",
      lastPeerCount: 12,
      seeders: 7,
      leechers: 5,
      nextAction: "retry",
      nextActionInMs: 17_000,
      error: "temporary timeout",
    });
    expect(
      client.opens[0]?.views.find((view) => view.type === "torrent_trackers")
        ?.delivery.min_interval_millis,
    ).toBe(250);
    await application.setViews({
      library: true,
      torrentId: TORRENT_ID,
      detail: "general",
      logCapture: null,
    });
    expect(snapshots.at(-1)?.trackersByTorrent).toEqual({});
    expect(snapshots.at(-1)?.viewStatus.trackers.status).toBe(
      "not_requested",
    );
    await application.close();
  });

  it("marks stale state then atomically installs a fresh view-set epoch", async () => {
    const client = new FakeLiveClient();
    const application = await LiveApplication.open(client, {
      initialViews: {
        library: true,
        torrentId: TORRENT_ID,
        detail: "peers",
        logCapture: null,
      },
      retryBaseMillis: 1,
      retryMaximumMillis: 2,
    });
    const snapshots: InspectionSnapshot[] = [];
    application.subscribe((update) => {
      if (update.type === "snapshot") snapshots.push(update.snapshot);
    });

    client.expireViewSet();
    await waitUntil(() => client.openCount === 2);
    await waitUntil(() => snapshots.at(-1)?.session.connection === "connected");

    expect(
      snapshots.some(
        (snapshot) =>
          snapshot.session.connection === "reconnecting" &&
          snapshot.viewStatus.peers.status === "stale",
      ),
    ).toBe(true);
    const recovered = snapshots.at(-1)!;
    expect(recovered.peersByTorrent[TORRENT_ID]?.order).toEqual([
      "connection-2",
    ]);
    expect(
      recovered.peersByTorrent[TORRENT_ID]?.rows["connection-1"],
    ).toBeUndefined();
    expect(
      recovered.peersByTorrent[TORRENT_ID]?.rows["connection-2"]?.flags,
    ).toEqual([
      "incoming",
      "download_allowed",
      "extension_protocol",
      "utp",
    ]);
    expect(
      recovered.peersByTorrent[TORRENT_ID]?.rows["connection-2"]?.client,
    ).toBe("µTorrent 3.5.5");
    await application.close();
  });
});

function snapshotFor(view: ViewSpec, generation: number): ViewSetUpdate {
  switch (view.type) {
    case "torrent_list":
      return {
        type: "snapshot",
        view_id: view.view_id,
        snapshot: {
          type: "torrent_list",
          torrents: [torrent()],
          storage: { roots: [], show_add_options: true },
        },
      };
    case "torrent_summary":
      return {
        type: "snapshot",
        view_id: view.view_id,
        snapshot: { type: "torrent", torrent: torrent() },
      };
    case "torrent_peers":
      return {
        type: "snapshot",
        view_id: view.view_id,
        snapshot: {
          type: "peers",
          torrent_id: TORRENT_ID,
          peers: [peer(generation)],
        },
      };
    case "torrent_files":
      return {
        type: "snapshot",
        view_id: view.view_id,
        snapshot: {
          type: "files",
          torrent_id: TORRENT_ID,
          state: "available",
          filesystem_content_base: "/tmp/content",
          files: [
            {
              file_id: "0",
              file_index: 0,
              path: ["video", "movie.mkv"],
              length_bytes: "9007199254740993",
              torrent_offset_bytes: "0",
              first_piece: 0,
              last_piece: 7,
              selection: "wanted",
              padding: false,
              done_bytes: "16384",
              verified_bytes: "0",
            },
          ],
        },
      };
    case "torrent_trackers":
      return {
        type: "snapshot",
        view_id: view.view_id,
        snapshot: {
          type: "trackers",
          torrent_id: TORRENT_ID,
          state: "available",
          trackers: [
            {
              tracker_id: "udp://tracker.example:6969",
              url: "udp://tracker.example:6969",
              transport: "udp",
              source: "magnet",
              tier: 0,
              status: "retry_wait",
              announce_event: null,
              total_attempts: 2,
              consecutive_failures: 1,
              last_peer_count: 12,
              seeders: 7,
              leechers: 5,
              interval_seconds: 600,
              next_action: "retry",
              next_action_in_millis: "17000",
              last_success_age_millis: "4000",
              last_failure_age_millis: "500",
              last_error: "temporary timeout",
            },
          ],
        },
      };
    case "session_disk":
      return {
        type: "snapshot",
        view_id: view.view_id,
        snapshot: {
          type: "session_disk",
          pipeline: {
            pressure: "idle",
            checkpoint_stage: "idle",
            intake_backpressured: false,
            sample_millis: "0",
            resident_limit_bytes: "0",
            resident_high_watermark_bytes: "0",
            resident_low_watermark_bytes: "0",
            requested_bytes: "0",
            resident_bytes: "0",
            queued_write_bytes: "0",
            writing_bytes: "0",
            hashing_bytes: "0",
            checkpoint_dirty_pieces: "0",
            checkpoint_dirty_bytes: "0",
            checkpoint_dirty_piece_high_water: "0",
            checkpoint_dirty_byte_high_water: "0",
            checkpoint_oldest_dirty_millis: "0",
            checkpoint_batches_started: "0",
            checkpoint_batches_completed: "0",
            checkpoint_pieces_completed: "0",
            checkpoint_sync_operations_completed: "0",
            checkpoint_sync_service_micros: "0",
            checkpoint_sync_service_max_micros: "0",
            checkpoint_commit_service_micros: "0",
            checkpoint_commit_service_max_micros: "0",
            storage_jobs_pending: "0",
            received_bytes_total: "0",
            stored_bytes_total: "0",
            verified_bytes_total: "0",
            receive_rate_bytes: "0",
            write_rate_bytes: "0",
            hash_rate_bytes: "0",
            write_operations_started: "0",
            write_operations_completed: "0",
            hash_operations_started: "0",
            hash_operations_completed: "0",
            write_queue_wait_micros: "0",
            write_queue_wait_max_micros: "0",
            write_service_micros: "0",
            write_service_max_micros: "0",
            hash_queue_wait_micros: "0",
            hash_queue_wait_max_micros: "0",
            hash_service_micros: "0",
            hash_service_max_micros: "0",
            pressure_transition_count: "0",
            backpressured_millis_total: "0",
          },
          pieces: [],
        },
      };
    case "diagnostics":
      return {
        type: "snapshot",
        view_id: view.view_id,
        snapshot: {
          type: "diagnostics",
          events: [],
          retention: {
            source_evicted_count: "0",
            retained_from_sequence: "1",
          },
        },
      };
    case "piece_activity":
      throw new Error("piece view is not used by live inspection");
  }
}

function torrent(): TorrentView {
  return {
    torrent_id: TORRENT_ID,
    display_name: "movie.mkv",
    state: "downloading",
    storage_state: "prepared",
    metadata_available: true,
    piece_count: 8,
    verified_piece_count: 2,
    requested_bytes: "32768",
    received_bytes: "16384",
    stored_bytes: "16384",
    active_peer_connections: 1,
    configured_tracker_count: 2,
    payload_download_rate_bytes: "4096",
    progress: {
      disposition: "active",
      phase: "transfer",
      reason: "transferring_pieces",
      actions: [],
    },
    archived: false,
    delete_managed_data_supported: true,
  };
}

function peer(generation: number): PeerView {
  return {
    connection_id: `connection-${generation}`,
    torrent_id: TORRENT_ID,
    peer_record_id: "peer-1",
    direction: "incoming",
    transport: "utp",
    lifecycle: generation === 1 ? "protocol_handshaking" : "connected",
    role: "content",
    ...(generation === 1
      ? {}
      : {
          peer_flags: [
            "incoming",
            "download_allowed",
            "extension_protocol",
            "utp",
          ] satisfies NonNullable<PeerView["peer_flags"]>,
        }),
    lifecycle_age_millis: "12",
    remote_endpoint: "127.0.0.1:6881",
    local_endpoint: null,
    sources: ["manual"],
    peer_id: null,
    client_name: generation === 1 ? null : "µTorrent 3.5.5",
    supports_extensions: true,
    supports_ut_metadata: true,
    local_interested: true,
    remote_interested: null,
    remote_choking: true,
    local_choking: null,
    available_piece_count: null,
    wanted_piece_count: null,
    payload_download_rate_bytes: null,
    payload_downloaded_bytes: null,
    protocol_download_rate_bytes: null,
    protocol_downloaded_bytes: null,
    payload_upload_rate_bytes: null,
    payload_uploaded_bytes: null,
    pending_requests: null,
    target_requests: null,
    queued_payload_bytes: null,
    oldest_request_age_millis: null,
    request_timeout_millis: null,
    request_phase: null,
    connected_age_millis: null,
    last_useful_age_millis: null,
    last_payload_age_millis: null,
    disconnect_reason: null,
    capabilities: {
      local_endpoint: "unavailable",
      client_name: generation === 1 ? "unavailable" : "available",
      ut_metadata: "unavailable",
      interest_directions: "unavailable",
      local_choke: "unavailable",
      piece_availability: "unavailable",
      protocol_rates: "unavailable",
      upload: "unsupported",
      metadata_stage: "unavailable",
    },
  };
}

async function waitUntil(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (predicate()) return;
    await new Promise((resolve) => globalThis.setTimeout(resolve, 1));
  }
  throw new Error("condition was not reached");
}
