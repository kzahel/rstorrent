import { describe, expect, it } from "vitest";

import {
  ContractError,
  decodeGatewayServerMessage,
  decodeUpdateBatch,
} from "./validation";

describe("gateway validation", () => {
  it("rejects unknown variants and non-canonical ranges", () => {
    expect(() =>
      decodeGatewayServerMessage(JSON.stringify({ type: "invented" })),
    ).toThrow(ContractError);
    expect(() =>
      decodeGatewayServerMessage(
        JSON.stringify({
          type: "update",
          update: {
            contract_version: 2,
            stream_id: "1",
            epoch: "1",
            sequence: "1",
            base_revision: "0",
            revision: "0",
            type: "snapshot",
            snapshot: {
              type: "piece_activity",
              torrent_id: "0".repeat(40),
              piece_count: 2,
              verified: [{ start: 1, end_exclusive: 3 }],
              active: [],
            },
          },
        }),
      ),
    ).toThrow(ContractError);
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
    batch.updates[0]!.snapshot.torrents[0]!.configured_tracker_count = 33;
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
          trackers: [
            {
              tracker_id: "udp://tracker.example:6969",
              url: "udp://tracker.example:6969",
              transport: "udp",
              source: "magnet",
              tier: 0,
              status: "retry_wait",
              announce_event: null,
              total_attempts: 1,
              consecutive_failures: 1,
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
