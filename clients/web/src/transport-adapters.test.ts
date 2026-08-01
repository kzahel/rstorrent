import type { Channel } from "@tauri-apps/api/core";
import { describe, expect, it } from "vitest";

import type { ViewUpdate } from "./api";
import { TauriSubscription } from "./tauri-client";
import { WebSocketSubscription } from "./websocket-client";

describe("frontend transport queues", () => {
  it("preserves the same ordered high-rate trace", async () => {
    const trace = Array.from({ length: 1_000 }, (_, index) =>
      update(index + 1),
    );
    const webSocket = webSocketSubscription(4 * 1024 * 1024);
    const tauri = tauriSubscription(4 * 1024 * 1024);
    for (const item of trace) {
      webSocket.push(item);
      tauri.push(item);
    }

    const webSocketResult = await drain(webSocket, trace.length);
    const tauriResult = await drain(tauri, trace.length);

    expect(webSocketResult).toEqual(trace);
    expect(tauriResult).toEqual(trace);
    expect(tauriResult).toEqual(webSocketResult);
  });

  it("reports the same explicit reset when its local bound overflows", async () => {
    const webSocket = webSocketSubscription(4 * 1024);
    const tauri = tauriSubscription(4 * 1024);
    for (let sequence = 1; sequence <= 40; sequence += 1) {
      const item = update(sequence);
      webSocket.push(item);
      tauri.push(item);
    }

    const webSocketReset = await next(webSocket);
    const tauriReset = await next(tauri);

    expect(webSocketReset.type).toBe("reset_required");
    expect(tauriReset).toEqual(webSocketReset);
  });
});

function webSocketSubscription(maxBytes: number): WebSocketSubscription {
  return new WebSocketSubscription(
    "stream-1",
    maxBytes,
    () => {},
    () => {},
    () => "request-1",
  );
}

function tauriSubscription(maxBytes: number): TauriSubscription {
  const channel = { onmessage: () => {} } as unknown as Channel<ViewUpdate>;
  const subscription = new TauriSubscription(channel, maxBytes, () => {});
  subscription.attach("stream-1");
  return subscription;
}

async function drain(
  subscription: AsyncIterable<ViewUpdate>,
  length: number,
): Promise<ViewUpdate[]> {
  const iterator = subscription[Symbol.asyncIterator]();
  const output: ViewUpdate[] = [];
  for (let index = 0; index < length; index += 1) {
    const result = await iterator.next();
    if (result.done) throw new Error("subscription closed before trace ended");
    output.push(result.value);
  }
  return output;
}

async function next(
  subscription: AsyncIterable<ViewUpdate>,
): Promise<ViewUpdate> {
  const result = await subscription[Symbol.asyncIterator]().next();
  if (result.done) throw new Error("subscription closed without reset");
  return result.value;
}

function update(sequence: number): ViewUpdate {
  return {
    contract_version: 2,
    stream_id: "stream-1",
    epoch: "epoch-1",
    sequence: String(sequence),
    base_revision: String(sequence - 1),
    revision: String(sequence),
    type: "patch",
    patch: {
      type: "torrent_list",
      upsert: [
        {
          torrent_id: "0123456789abcdef0123456789abcdef01234567",
          state: "downloading",
          storage_state: "staging",
          metadata_available: true,
          piece_count: 100_000,
          verified_piece_count: sequence,
          requested_bytes: String(sequence * 16_384),
          received_bytes: String(sequence * 16_384),
          stored_bytes: String(sequence * 16_384),
          progress: {
            disposition: "active",
            phase: "transfer",
            reason: "transferring_pieces",
            actions: [],
          },
        },
      ],
      removed: [],
    },
  };
}
