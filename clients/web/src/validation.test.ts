import { describe, expect, it } from "vitest";

import {
  ContractError,
  decodeApplicationServerFrame,
  decodeChooseDownloadRootResponse,
  decodeResponseEnvelope,
  decodeUpdateBatch,
} from "./validation";
import { clientSettingsRuntimeFixture } from "./test-support/client-settings";

describe("download folder response validation", () => {
  it("accepts selection or cancellation and rejects an oversized label", () => {
    const selected = {
      root: {
        root_id: "root_a",
        label: "Downloads",
        display_path: "/Users/test/Downloads",
        availability: "available",
      },
    };
    expect(
      decodeChooseDownloadRootResponse(JSON.stringify(selected)).root,
    ).toMatchObject({ root_id: "root_a" });
    expect(
      decodeChooseDownloadRootResponse('{"root":null}').root,
    ).toBeNull();
    selected.root.label = "x".repeat(257);
    expect(() =>
      decodeChooseDownloadRootResponse(JSON.stringify(selected)),
    ).toThrow(/label exceeds 256 bytes/);
  });
});

describe("application connection validation", () => {
  it("rejects unknown variants and non-canonical ranges", () => {
    expect(() =>
      decodeApplicationServerFrame(JSON.stringify({ type: "invented" })),
    ).toThrow(ContractError);
    expect(() =>
      decodeApplicationServerFrame(
        JSON.stringify({
          type: "view_batch",
          stream_id: "view-1",
          batch: {
            ...peerBatch("0".repeat(40)),
            epoch: "not-decimal",
          },
        }),
      ),
    ).toThrow(ContractError);
  });
});

describe("client settings validation", () => {
  it("applies Rust-owned defaults to older service and view snapshots", () => {
    const response = decodeResponseEnvelope(
      JSON.stringify({
        version: 1,
        request_id: "request-1",
        revision: "0",
        status: "success",
        snapshot: {
          profile_id: "default",
          revision: "0",
          storage: { roots: [], show_add_options: true },
          torrents: [],
        },
      }),
    );
    expect(response.status).toBe("success");
    if (response.status === "success") {
      expect(response.snapshot.client_settings).toEqual({
        listener: { type: "disabled" },
        preferred_listen_port: 6_881,
        port_mapping: "disabled",
        peer_connection_limit: 200,
        upload_slots: 8,
      });
    }

    const batch = torrentBatch("Defaulted");
    const snapshot = batch.updates[0]!.snapshot as unknown as Record<
      string,
      unknown
    >;
    delete snapshot.client_settings;
    const decoded = decodeUpdateBatch(JSON.stringify(batch));
    const update = decoded.updates[0]!;
    expect(update.type).toBe("snapshot");
    if (update.type === "snapshot" && update.snapshot.type === "torrent_list") {
      expect(update.snapshot.client_settings).toEqual(
        clientSettingsRuntimeFixture(),
      );
    }
  });

  it("rejects additional, malformed, and inconsistent runtime settings", () => {
    const additional = torrentBatch("Additional");
    const additionalSettings = additional.updates[0]!.snapshot
      .client_settings.configured as unknown as Record<string, unknown>;
    additionalSettings.invented = true;
    expect(() => decodeUpdateBatch(JSON.stringify(additional))).toThrow(
      ContractError,
    );

    const fractional = torrentBatch("Fractional");
    fractional.updates[0]!.snapshot.client_settings.configured.upload_slots = 1.5;
    expect(() => decodeUpdateBatch(JSON.stringify(fractional))).toThrow(
      ContractError,
    );

    const inconsistent = torrentBatch("Inconsistent");
    inconsistent.updates[0]!.snapshot.client_settings.listener_status = {
      type: "listening",
      address: "127.0.0.1",
      port: 6_881,
    };
    expect(() => decodeUpdateBatch(JSON.stringify(inconsistent))).toThrow(
      /disabled listener reports a listening status/,
    );

    const mismatchedUdp = torrentBatch("Mismatched UDP");
    mismatchedUdp.updates[0]!.snapshot.client_settings = {
      ...clientSettingsRuntimeFixture(),
      effective_listener: {
        listener: { type: "automatic_loopback" },
        preferred_listen_port: 6_881,
      },
      configured: {
        ...clientSettingsRuntimeFixture().configured,
        listener: { type: "automatic_loopback" },
      },
      listener_status: {
        type: "listening",
        address: "127.0.0.1",
        port: 6_881,
      },
      session_udp_status: {
        type: "bound",
        address: "127.0.0.1",
        port: 6_882,
        coordinated_with_tcp: true,
      },
    };
    expect(() => decodeUpdateBatch(JSON.stringify(mismatchedUdp))).toThrow(
      /coordinated session UDP endpoint differs/,
    );
  });
});

describe("peer view validation", () => {
  it("accepts bounded active peers and rejects cross-torrent rows", () => {
    const batch = peerBatch("0".repeat(40));
    expect(decodeUpdateBatch(JSON.stringify(batch)).updates).toHaveLength(1);
    batch.updates[0]!.snapshot.peers[0]!.torrent_id = "1".repeat(40);
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(
      /another torrent/,
    );
  });

  it("accepts typed flags and rejects duplicate flag state", () => {
    const batch = peerBatch("0".repeat(40));
    batch.updates[0]!.snapshot.peers[0]!.peer_flags = [
      "incoming",
      "extension_protocol",
      "utp",
    ];
    expect(decodeUpdateBatch(JSON.stringify(batch)).updates).toHaveLength(1);
    batch.updates[0]!.snapshot.peers[0]!.peer_flags = ["incoming", "incoming"];
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(
      /flags contain duplicates/,
    );
    batch.updates[0]!.snapshot.peers[0]!.peer_flags = Array(17).fill(
      "incoming",
    );
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(
      /flags exceed their bound/,
    );
    batch.updates[0]!.snapshot.peers[0]!.peer_flags = ["invented"];
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(
      ContractError,
    );
  });
});

describe("swarm view validation", () => {
  it("accepts coherent bounded registry state and rejects oversized or inconsistent counts", () => {
    const batch = swarmBatch();
    expect(decodeUpdateBatch(JSON.stringify(batch)).updates).toHaveLength(1);
    batch.updates[0]!.snapshot.maximum_records = 1_001;
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(
      /record maximum must be an integer in range/,
    );
    const inconsistent = swarmBatch();
    inconsistent.updates[0]!.snapshot.counts.eligible = 1;
    expect(() => decodeUpdateBatch(JSON.stringify(inconsistent))).toThrow(
      /counts are inconsistent/,
    );
  });
});

describe("torrent display-name validation", () => {
  it("accepts a bounded verified name and rejects oversized input", () => {
    const batch = torrentBatch("Verified torrent");
    expect(decodeUpdateBatch(JSON.stringify(batch)).updates).toHaveLength(1);
    batch.updates[0]!.snapshot.torrents[0]!.display_name = "x".repeat(256);
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(
      /display name exceeds 255 bytes/,
    );
  });
});

describe("torrent tracker-count validation", () => {
  it("accepts the bounded summary count and rejects an oversized catalog", () => {
    const batch = torrentBatch("Verified torrent");
    expect(decodeUpdateBatch(JSON.stringify(batch)).updates).toHaveLength(1);
    batch.updates[0]!.snapshot.torrents[0]!.configured_tracker_count = 999_995;
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(
      /configured tracker count must be an integer in range/,
    );
  });
});

describe("tracker view validation", () => {
  it("accepts bounded state and rejects an oversized retained error", () => {
    const batch = trackerBatch();
    expect(decodeUpdateBatch(JSON.stringify(batch)).updates).toHaveLength(1);
    batch.updates[0]!.snapshot.trackers[0]!.last_error = "x".repeat(257);
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(
      /last error exceeds 256 bytes/,
    );
  });
});

describe("disk view validation", () => {
  it("accepts bounded pipeline state and rejects impossible piece accounting", () => {
    const batch = diskBatch();
    expect(decodeUpdateBatch(JSON.stringify(batch)).updates).toHaveLength(1);
    batch.updates[0]!.snapshot.pieces[0]!.stored_bytes = "262145";
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(
      /stored_bytes exceeds the piece length/,
    );
  });
});

describe("DHT view validation", () => {
  it("accepts exact bounded state and rejects reordered or inconsistent buckets", () => {
    const batch = dhtBatch();
    expect(decodeUpdateBatch(JSON.stringify(batch)).updates).toHaveLength(1);
    batch.updates[0]!.snapshot.inspection.buckets_v4[159]!.bucket_index = 158;
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(
      /exact engine index order/,
    );

    const inconsistent = dhtBatch();
    inconsistent.updates[0]!.snapshot.inspection.routing_nodes_v4 = 2;
    expect(() => decodeUpdateBatch(JSON.stringify(inconsistent))).toThrow(
      /aggregates do not match/,
    );
  });
});

describe("piece activity validation", () => {
  it("accepts keyed attempts and rejects overlapping lifecycle ranges", () => {
    const batch = pieceBatch();
    expect(decodeUpdateBatch(JSON.stringify(batch)).updates).toHaveLength(1);
    batch.updates[0]!.snapshot.active[0]!.received = [
      { start: 8_192, end_exclusive: 24_576 },
    ];
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(
      /lifecycle ranges overlap/,
    );
  });
});

describe("diagnostic validation", () => {
  it("accepts structured records and rejects invalid hierarchy and typed values", () => {
    const batch = diagnosticBatch();
    expect(decodeUpdateBatch(JSON.stringify(batch)).updates).toHaveLength(1);
    batch.updates[0]!.snapshot.events[0]!.category = "Peer.Connection";
    expect(() => decodeUpdateBatch(JSON.stringify(batch))).toThrow(ContractError);

    const invalidValue = diagnosticBatch();
    invalidValue.updates[0]!.snapshot.events[0]!.fields[0]!.value.value = "12ms";
    expect(() => decodeUpdateBatch(JSON.stringify(invalidValue))).toThrow(
      ContractError,
    );
  });
});

function diagnosticBatch() {
  return {
    api_version: 1,
    view_set_id: "vs_000102030405060708090a0b0c0d0e0f",
    epoch: "1",
    base_cursor: "0",
    cursor: "1",
    durable_revision: "1",
    updates: [
      {
        type: "snapshot" as const,
        view_id: "logs",
        snapshot: {
          type: "diagnostics" as const,
          events: [
            {
              sequence: "1",
              timestamp_millis: "1",
              severity: "debug",
              category: "peer.connection",
              code: "handshake_completed",
              torrent_id: "0".repeat(40),
              message: "Peer extension handshake completed",
              subjects: [
                { type: "peer_connection", connection_id: "connection-1" },
              ],
              fields: [
                {
                  key: "elapsed",
                  value: { type: "duration_millis", value: "12" },
                },
              ],
            },
          ],
          retention: {
            source_evicted_count: "0",
            retained_from_sequence: "1",
          },
        },
      },
    ],
  };
}

function dhtBatch() {
  const buckets = Array.from({ length: 160 }, (_, bucket_index) => ({
    bucket_index,
    good_nodes: 0,
    questionable_nodes: 0,
    replacement_candidates: 0,
    oldest_live_response_age_millis: null as string | null,
  }));
  buckets[159] = {
    bucket_index: 159,
    good_nodes: 1,
    questionable_nodes: 0,
    replacement_candidates: 2,
    oldest_live_response_age_millis: "12000",
  };
  return {
    api_version: 1,
    view_set_id: "vs_000102030405060708090a0b0c0d0e0f",
    epoch: "1",
    base_cursor: "0",
    cursor: "1",
    durable_revision: "1",
    updates: [{
      type: "snapshot" as const,
      view_id: "dht",
      snapshot: {
        type: "session_dht" as const,
        inspection: {
          lifecycle: "participating" as const,
          network_policy: "loopback_only" as const,
          local_node_id: "0".repeat(40),
          captured_millis: "1000",
          routing_nodes_v4: 1,
          occupied_buckets_v4: 1,
          deepest_shared_prefix_bits_v4: 0,
          active_transactions: 1,
          active_lookups: 1,
          queries_sent: "1",
          responses_received: "1",
          queries_received: "0",
          malformed_received: "0",
          rate_limited: "0",
          discovered_peers: "0",
          bootstrap_attempts: "1",
          routing_refreshes: "0",
          datagram_bytes_sent: "42",
          datagram_bytes_received: "84",
          buckets_v4: buckets,
          lookups: [{
            lookup_id: "1",
            target_id: "1".repeat(40),
            age_millis: "500",
            deadline_in_millis: "29500",
            unqueried_candidates: 8,
            in_flight_candidates: 3,
            responded_candidates: 2,
            failed_candidates: 1,
            discovered_peers: 0,
            closest_responded_prefix_bits: 7,
            last_convergence_improvement_age_millis: "100",
          }],
        },
      },
    }],
  };
}

function pieceBatch() {
  return {
    api_version: 1,
    view_set_id: "vs_000102030405060708090a0b0c0d0e0f",
    epoch: "1",
    base_cursor: "0",
    cursor: "1",
    durable_revision: "1",
    updates: [
      {
        type: "snapshot" as const,
        view_id: "pieces",
        snapshot: {
          type: "piece_activity" as const,
          torrent_id: "0".repeat(40),
          piece_count: 4,
          verified: [{ start: 2, end_exclusive: 3 }],
          active: [
            {
              piece_id: "0:1",
              piece_index: 0,
              attempt: 1,
              piece_length: 262_144,
              stage: "requested",
              requested: [{ start: 0, end_exclusive: 16_384 }],
              received: [] as Array<{ start: number; end_exclusive: number }>,
              stored: [],
              age_millis: "10",
            },
          ],
        },
      },
    ],
  };
}

function diskBatch() {
  const zeroFields = {
    sample_millis: "0",
    resident_limit_bytes: "1048576",
    resident_high_watermark_bytes: "786432",
    resident_low_watermark_bytes: "524288",
    requested_bytes: "16384",
    resident_bytes: "16384",
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
    received_bytes_total: "16384",
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
  };
  return {
    api_version: 1,
    view_set_id: "vs_000102030405060708090a0b0c0d0e0f",
    epoch: "1",
    base_cursor: "0",
    cursor: "1",
    durable_revision: "1",
    updates: [
      {
        type: "snapshot" as const,
        view_id: "disk",
        snapshot: {
          type: "session_disk" as const,
          pipeline: {
            pressure: "normal",
            checkpoint_stage: "idle",
            intake_backpressured: false,
            ...zeroFields,
          },
          pieces: [
            {
              row_id: `${"0".repeat(40)}:0:1`,
              torrent_id: "0".repeat(40),
              torrent_name: "Test torrent",
              piece_index: 0,
              piece_length: 262144,
              attempt: 1,
              stage: "receiving",
              requested_bytes: "16384",
              received_bytes: "16384",
              stored_bytes: "0",
              stage_age_millis: "1",
              age_millis: "1",
            },
          ],
        },
      },
    ],
  };
}

function trackerBatch() {
  return {
    api_version: 1,
    view_set_id: "vs_000102030405060708090a0b0c0d0e0f",
    epoch: "1",
    base_cursor: "0",
    cursor: "1",
    durable_revision: "1",
    updates: [
      {
        type: "snapshot" as const,
        view_id: "torrent-trackers",
        snapshot: {
          type: "trackers" as const,
          torrent_id: "0".repeat(40),
          state: "available",
          page: { offset: 0, limit: 1024, total: 1, next_offset: null },
          trackers: [
            {
              tracker_id: "000000:000000",
              url: "udp://tracker.example:6969",
              transport: "udp",
              security: "unencrypted",
              source: "magnet",
              tier: 0,
              status: "retry_wait",
              announce_event: null,
              total_attempts: 1,
              consecutive_failures: 1,
              last_connection_family: null,
              last_peer_count: null,
              seeders: null,
              leechers: null,
              interval_seconds: null,
              next_action: "retry",
              next_action_in_millis: "17000",
              last_success_age_millis: null,
              last_failure_age_millis: "100",
              last_error: "timeout",
            },
          ],
        },
      },
    ],
  };
}

function torrentBatch(displayName: string) {
  return {
    api_version: 1,
    view_set_id: "vs_000102030405060708090a0b0c0d0e0f",
    epoch: "1",
    base_cursor: "0",
    cursor: "1",
    durable_revision: "1",
    updates: [
      {
        type: "snapshot" as const,
        view_id: "library",
        snapshot: {
          type: "torrent_list" as const,
          storage: { roots: [], show_add_options: true },
          client_settings: clientSettingsRuntimeFixture(),
          torrents: [
            {
              torrent_id: "0".repeat(40),
              display_name: displayName,
              state: "downloading",
              storage_state: "staging",
              metadata_available: true,
              piece_count: 1,
              verified_piece_count: 0,
              requested_bytes: "0",
              received_bytes: "0",
              stored_bytes: "0",
              active_peer_connections: 0,
              configured_tracker_count: 2,
              payload_download_rate_bytes: "0",
              progress: {
                disposition: "active",
                phase: "transfer",
                reason: "transferring_pieces",
                actions: [],
              },
              archived: false,
              delete_managed_data_supported: true,
              force_recheck_available: true,
            },
          ],
        },
      },
    ],
  };
}

function peerBatch(torrentId: string) {
  return {
    api_version: 1,
    view_set_id: "vs_000102030405060708090a0b0c0d0e0f",
    epoch: "1",
    base_cursor: "0",
    cursor: "1",
    durable_revision: "1",
    updates: [
      {
        type: "snapshot" as const,
        view_id: "torrent-peers",
        snapshot: {
          type: "peers" as const,
          torrent_id: torrentId,
          peers: [
            {
              connection_id: "1",
              torrent_id: torrentId,
              peer_record_id: "2",
              direction: "outgoing",
              transport: "tcp",
              lifecycle: "protocol_handshaking",
              role: "metadata",
              peer_flags: undefined as string[] | undefined,
              lifecycle_age_millis: "5",
              remote_endpoint: "127.0.0.1:6881",
              local_endpoint: null,
              sources: ["magnet_hint"],
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
                local_endpoint: "unsupported",
                client_name: "unsupported",
                ut_metadata: "unavailable",
                interest_directions: "unavailable",
                local_choke: "unsupported",
                piece_availability: "unavailable",
                protocol_rates: "unsupported",
                upload: "unsupported",
                metadata_stage: "unavailable",
              },
            },
          ],
        },
      },
    ],
  };
}

function swarmBatch() {
  return {
    api_version: 1,
    view_set_id: "vs_000102030405060708090a0b0c0d0e0f",
    epoch: "1",
    base_cursor: "0",
    cursor: "1",
    durable_revision: "1",
    updates: [
      {
        type: "snapshot" as const,
        view_id: "torrent-swarm",
        snapshot: {
          type: "swarm" as const,
          torrent_id: "0".repeat(40),
          state: "active",
          captured_millis: "1000",
          maximum_records: 1000,
          counts: {
            total: 0,
            eligible: 0,
            not_connectable: 0,
            dialing: 0,
            connected: 0,
            backed_off: 0,
            failure_limited: 0,
            banned: 0,
          },
          peers: [],
        },
      },
    ],
  };
}
