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
        "diagnostics",
      ],
      limits: {
        max_view_sets_per_owner: 8,
        max_views_per_set: 16,
        max_view_id_bytes: 64,
        min_queue_bytes: 16_384,
        default_queue_bytes: 262_144,
        max_queue_bytes: 524_288,
        max_wait_millis: 20_000,
        lease_millis: "300000",
      },
    };
  }

  async dispatch(request: RequestEnvelope): Promise<ResponseEnvelope> {
    return {
      version: 1,
      request_id: request.request_id,
      revision: "4",
      status: "success",
      snapshot: { profile_id: "live", revision: "4", torrents: [] },
    };
  }

  async openViewSet(request: OpenViewSetRequest): Promise<OpenViewSetResponse> {
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
  it("maps active peer state and evicts obsolete responsive views", async () => {
    const client = new FakeLiveClient();
    const application = await LiveApplication.open(client, {
      initialViews: { library: true, torrentId: TORRENT_ID, detail: "peers" },
    });
    const snapshots: InspectionSnapshot[] = [];
    application.subscribe((update) => {
      if (update.type === "snapshot") snapshots.push(update.snapshot);
    });

    const initial = snapshots.at(-1)!;
    const peer = initial.peersByTorrent[TORRENT_ID]?.rows["connection-1"];
    expect(peer?.state).toBe("handshaking");
    expect(peer?.client).toBeNull();
    expect(peer?.downloadRate).toBeNull();
    expect(initial.viewStatus.peers.status).toBe("ready");

    await application.setViews({
      library: false,
      torrentId: TORRENT_ID,
      detail: "logs",
    });
    expect(client.updates.at(-1)?.views.map((view) => view.type)).toEqual([
      "torrent_summary",
      "diagnostics",
    ]);
    const transition = snapshots.at(-1)!;
    expect(transition.torrentOrder).toEqual([]);
    expect(transition.peersByTorrent).toEqual({});
    expect(transition.viewStatus.peers.status).toBe("not_requested");
    expect(transition.viewStatus.logs.status).toBe("loading");
    await application.close();
  });

  it("marks stale state then atomically installs a fresh view-set epoch", async () => {
    const client = new FakeLiveClient();
    const application = await LiveApplication.open(client, {
      initialViews: { library: true, torrentId: TORRENT_ID, detail: "peers" },
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
    await application.close();
  });
});

function snapshotFor(view: ViewSpec, generation: number): ViewSetUpdate {
  switch (view.type) {
    case "torrent_list":
      return {
        type: "snapshot",
        view_id: view.view_id,
        snapshot: { type: "torrent_list", torrents: [torrent()] },
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
    case "diagnostics":
      return {
        type: "snapshot",
        view_id: view.view_id,
        snapshot: { type: "diagnostics", events: [], dropped_count: "0" },
      };
    case "piece_activity":
      throw new Error("piece view is not used by live inspection");
  }
}

function torrent(): TorrentView {
  return {
    torrent_id: TORRENT_ID,
    state: "downloading",
    storage_state: "prepared",
    metadata_available: true,
    piece_count: 8,
    verified_piece_count: 2,
    requested_bytes: "32768",
    received_bytes: "16384",
    stored_bytes: "16384",
    active_peer_connections: 1,
    payload_download_rate_bytes: "4096",
    progress: {
      disposition: "active",
      phase: "transfer",
      reason: "transferring_pieces",
      actions: [],
    },
  };
}

function peer(generation: number): PeerView {
  return {
    connection_id: `connection-${generation}`,
    torrent_id: TORRENT_ID,
    peer_record_id: "peer-1",
    direction: "outgoing",
    transport: "tcp",
    lifecycle: generation === 1 ? "protocol_handshaking" : "connected",
    role: "content",
    lifecycle_age_millis: "12",
    remote_endpoint: "127.0.0.1:6881",
    local_endpoint: null,
    sources: ["manual"],
    peer_id: null,
    client_name: null,
    supports_extensions: null,
    supports_ut_metadata: null,
    local_interested: null,
    remote_interested: null,
    remote_choking: null,
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
      client_name: "unavailable",
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
