import { describe, expect, it } from "vitest";

import type {
  ApiHello,
  ApplicationClientFrame,
  ApplicationServerFrame,
  OpenViewSetResponse,
  RequestEnvelope,
  ResponseEnvelope,
  UpdateBatch,
  ViewSpec,
} from "./api";
import type {
  ApplicationWebSocket,
  ApplicationWebSocketFactory,
} from "./websocket-view-client";
import { WebSocketApplicationViewClient } from "./websocket-view-client";

const viewSetId = "vs_000102030405060708090a0b0c0d0e0f";
const clientInstanceId = "00000000000000000000000000000001";
const listView: ViewSpec = {
  type: "torrent_list",
  view_id: "library",
  delivery: { min_interval_millis: 0 },
};

class FakeWebSocket implements ApplicationWebSocket {
  public readyState = 0;
  public binaryType: BinaryType = "blob";
  public onopen: ((event: Event) => void) | null = null;
  public onmessage: ((event: MessageEvent<unknown>) => void) | null = null;
  public onerror: ((event: Event) => void) | null = null;
  public onclose: ((event: CloseEvent) => void) | null = null;
  public readonly sent: ApplicationClientFrame[] = [];

  public constructor(
    public readonly url: string,
    private readonly respond: (
      frame: ApplicationClientFrame,
      socket: FakeWebSocket,
    ) => void,
  ) {}

  public open(): void {
    this.readyState = 1;
    this.onopen?.({} as Event);
  }

  public send(data: string): void {
    const frame = JSON.parse(data) as ApplicationClientFrame;
    this.sent.push(frame);
    this.respond(frame, this);
  }

  public server(frame: ApplicationServerFrame): void {
    this.onmessage?.({ data: JSON.stringify(frame) } as MessageEvent<string>);
  }

  public serverRaw(data: string): void {
    this.onmessage?.({ data } as MessageEvent<string>);
  }

  public close(code = 1000, reason = ""): void {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.onclose?.({ code, reason } as CloseEvent);
  }
}

describe("multiplexed application WebSocket adapter", () => {
  it("uses one socket for calls and acknowledges a batch only on the next pull", async () => {
    const sockets: FakeWebSocket[] = [];
    const acknowledgements: string[] = [];
    const factory: ApplicationWebSocketFactory = (url) => {
      const socket = new FakeWebSocket(url, (frame, active) => {
        queueMicrotask(() => {
          switch (frame.type) {
            case "connect":
              active.server(connected());
              break;
            case "call":
              if (frame.operation.type === "open_view_set") {
                active.server({
                  type: "result",
                  call_id: frame.call_id,
                  result: { type: "view_set_opened", response: opened() },
                });
              } else if (frame.operation.type === "dispatch") {
                active.server({
                  type: "result",
                  call_id: frame.call_id,
                  result: {
                    type: "command_response",
                    response: commandResponse(
                      frame.operation.request.request_id,
                    ),
                  },
                });
              }
              break;
            case "attach":
              active.server({
                type: "attached",
                call_id: frame.call_id,
                stream_id: frame.stream_id,
                view_set_id: frame.view_set_id,
              });
              break;
            case "ack":
              acknowledgements.push(frame.cursor);
              active.server({
                type: "view_batch",
                stream_id: frame.stream_id,
                batch: batch(frame.cursor, "3"),
              });
              break;
            case "detach":
              active.server({
                type: "detached",
                call_id: frame.call_id,
                stream_id: frame.stream_id,
              });
              break;
          }
        });
      });
      sockets.push(socket);
      return socket;
    };
    const client = new WebSocketApplicationViewClient(
      "http://127.0.0.1:3030/",
      "token",
      factory,
      clientInstanceId,
    );
    const opening = client.openViewSet({ views: [listView], options: {} });
    expect(sockets).toHaveLength(1);
    sockets[0]?.open();
    await expect(opening).resolves.toMatchObject({ view_set_id: viewSetId });

    const stream = await client.streamUpdates(viewSetId, "1");
    sockets[0]?.server({
      type: "view_batch",
      stream_id: "view-1",
      batch: batch("1", "2"),
    });
    const iterator = stream[Symbol.asyncIterator]();
    await expect(iterator.next()).resolves.toMatchObject({
      done: false,
      value: { base_cursor: "1", cursor: "2" },
    });
    expect(acknowledgements).toEqual([]);

    const command = client.dispatch(snapshotRequest("snapshot"));
    await expect(command).resolves.toMatchObject({ request_id: "snapshot" });
    expect(sockets).toHaveLength(1);
    await expect(iterator.next()).resolves.toMatchObject({
      done: false,
      value: { base_cursor: "2", cursor: "3" },
    });
    expect(acknowledgements).toEqual(["2"]);

    await stream.close();
    await client.close();
    expect(sockets[0]?.sent[0]).toMatchObject({
      type: "connect",
      client_instance_id: clientInstanceId,
      token: "token",
    });
  });

  it("rejects malformed batches without acknowledging them", async () => {
    const sockets: FakeWebSocket[] = [];
    const factory: ApplicationWebSocketFactory = (url) => {
      const socket = new FakeWebSocket(url, (frame, active) => {
        queueMicrotask(() => {
          if (frame.type === "connect") active.server(connected());
          if (frame.type === "attach") {
            active.server({
              type: "attached",
              call_id: frame.call_id,
              stream_id: frame.stream_id,
              view_set_id: frame.view_set_id,
            });
          }
        });
      });
      sockets.push(socket);
      return socket;
    };
    const client = new WebSocketApplicationViewClient(
      "http://127.0.0.1:3030/",
      null,
      factory,
      clientInstanceId,
    );
    const hello = client.hello();
    sockets[0]?.open();
    await hello;
    const stream = await client.streamUpdates(viewSetId, "1");
    const iterator = stream[Symbol.asyncIterator]();
    const next = iterator.next();
    sockets[0]?.serverRaw(
      JSON.stringify({
        type: "view_batch",
        stream_id: "view-1",
        batch: { ...batch("1", "2"), epoch: "not-decimal" },
      }),
    );

    await expect(next).rejects.toThrow();
    expect(sockets[0]?.sent.some((frame) => frame.type === "ack")).toBe(false);
    await stream.close();
    await client.close();
  });

  it("does not replay a pending command and reuses client identity on reconnect", async () => {
    const sockets: FakeWebSocket[] = [];
    const factory: ApplicationWebSocketFactory = (url) => {
      const socket = new FakeWebSocket(url, (frame, active) => {
        queueMicrotask(() => {
          if (frame.type === "connect") active.server(connected());
          if (
            frame.type === "call" &&
            frame.operation.type === "open_view_set"
          ) {
            active.server({
              type: "result",
              call_id: frame.call_id,
              result: { type: "view_set_opened", response: opened() },
            });
          }
        });
      });
      sockets.push(socket);
      return socket;
    };
    const client = new WebSocketApplicationViewClient(
      "http://127.0.0.1:3030/",
      null,
      factory,
      clientInstanceId,
    );
    const hello = client.hello();
    sockets[0]?.open();
    await hello;
    const pending = client.dispatch(snapshotRequest("not-replayed"));
    await Promise.resolve();
    sockets[0]?.close(1006, "abnormal");
    await expect(pending).rejects.toMatchObject({ code: "connection_closed" });

    const reopening = client.openViewSet({ views: [listView], options: {} });
    await new Promise((resolve) => setTimeout(resolve, 275));
    expect(sockets).toHaveLength(2);
    sockets[1]?.open();
    await reopening;
    const secondFrames = sockets[1]?.sent ?? [];
    expect(secondFrames[0]).toMatchObject({
      type: "connect",
      client_instance_id: clientInstanceId,
    });
    expect(
      secondFrames.some(
        (frame) =>
          frame.type === "call" && frame.operation.type === "dispatch",
      ),
    ).toBe(false);
    await client.close();
  });
});

function connected(): ApplicationServerFrame {
  return {
    type: "connected",
    api_version: 1,
    encoding: "json",
    hello: hello(),
    connection_limits: {
      max_attachments: 8,
      max_pending_calls: 16,
      max_client_message_bytes: 65_536,
      max_application_payload_bytes: 16_777_216,
      heartbeat_idle_millis: 15_000,
      heartbeat_timeout_millis: 10_000,
    },
  };
}

function hello(): ApiHello {
  return {
    api: { current: 1, minimum: 1 },
    encodings: ["json"],
    deliveries: ["poll", "long_poll", "stream"],
    capabilities: ["torrent_list"],
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

function opened(): OpenViewSetResponse {
  return {
    view_set_id: viewSetId,
    lease_millis: "300000",
    effective_queue_bytes: 262_144,
    effective_views: [listView],
    initial: {
      api_version: 1,
      view_set_id: viewSetId,
      epoch: "1",
      base_cursor: "0",
      cursor: "1",
      durable_revision: "0",
      updates: [
        {
          type: "snapshot",
          view_id: "library",
          snapshot: { type: "torrent_list", torrents: [] },
        },
      ],
    },
  };
}

function batch(baseCursor: string, cursor: string): UpdateBatch {
  return {
    api_version: 1,
    view_set_id: viewSetId,
    epoch: "1",
    base_cursor: baseCursor,
    cursor,
    durable_revision: cursor,
    updates: [
      {
        type: "patch",
        view_id: "library",
        patch: { type: "torrent_list", upsert: [], removed: [] },
      },
    ],
  };
}

function snapshotRequest(requestId: string): RequestEnvelope {
  return {
    version: 1,
    request_id: requestId,
    expected_revision: null,
    command: { type: "snapshot" },
  };
}

function commandResponse(requestId: string): ResponseEnvelope {
  return {
    version: 1,
    request_id: requestId,
    revision: "0",
    status: "success",
    snapshot: { profile_id: "test", revision: "0", torrents: [] },
  };
}
