import { afterAll, describe, expect, it } from "vitest";
import { WebSocket as NodeWebSocket } from "ws";

import type {
  IndexRange,
  RequestEnvelope,
} from "./api";
import {
  emptyApplicationViewState,
  reduceViewUpdate,
} from "./reducer";
import type {
  ApplicationSubscription,
} from "./application-client";
import {
  WebSocketApplicationClient,
} from "./websocket-client";

interface ProcessEnvironment {
  process: {
    env: Record<string, string | undefined>;
  };
}

const environment = (globalThis as unknown as ProcessEnvironment).process.env;
const gatewayUrl = environment.RSTORRENT_INTEROP_GATEWAY_URL;
const gatewayOrigin = environment.RSTORRENT_INTEROP_GATEWAY_ORIGIN;
const gatewayToken = environment.RSTORRENT_INTEROP_GATEWAY_TOKEN;
const magnet = environment.RSTORRENT_INTEROP_MAGNET;
const torrentId = environment.RSTORRENT_INTEROP_TORRENT_ID;
const enabled =
  gatewayUrl !== undefined &&
  gatewayOrigin !== undefined &&
  gatewayToken !== undefined &&
  magnet !== undefined &&
  torrentId !== undefined;

const liveDescribe = enabled ? describe : describe.skip;
let client: WebSocketApplicationClient | undefined;

afterAll(async () => {
  await client?.close();
});

liveDescribe("authenticated gateway interop", () => {
  it(
    "reduces summary and lossless piece streams through completion",
    async () => {
      if (
        gatewayUrl === undefined ||
        gatewayOrigin === undefined ||
        gatewayToken === undefined ||
        magnet === undefined ||
        torrentId === undefined
      ) {
        throw new Error("interop environment is incomplete");
      }
      client = await WebSocketApplicationClient.connect(
        gatewayUrl,
        gatewayToken,
        (url) =>
          new NodeWebSocket(url, { origin: gatewayOrigin }) as unknown as WebSocket,
      );
      const list = await client.subscribe({
        selector: { type: "torrent_list" },
        projection: "summary",
        delivery: {
          min_interval_millis: 0,
          max_queue_bytes: 256 * 1024,
        },
      });
      let state = emptyApplicationViewState();
      let updates = 0;
      let requested = 0;
      let received = 0;
      let stored = 0;
      let wake: (() => void) | undefined;
      const changed = (): void => {
        wake?.();
        wake = undefined;
      };
      const consume = async (
        subscription: ApplicationSubscription,
      ): Promise<void> => {
        for await (const update of subscription) {
          state = reduceViewUpdate(state, update);
          updates += 1;
          const active = state.pieces[torrentId]?.active;
          requested = Math.max(requested, activeRangeBytes(active, "requested"));
          received = Math.max(received, activeRangeBytes(active, "received"));
          stored = Math.max(stored, activeRangeBytes(active, "stored"));
          changed();
        }
      };
      const listTask = consume(list);
      await waitUntil(
        () => state.streams[list.streamId] !== undefined,
        () => new Promise<void>((resolve) => {
          wake = resolve;
        }),
      );

      const request: RequestEnvelope = {
        version: 1,
        request_id: "web-interop-add",
        command: {
          type: "add_magnet",
          magnet,
          storage_root: "downloads",
          skip_files: [],
        },
      };
      const response = await client.dispatch(request);
      expect(response.status).toBe("success");
      await waitUntil(
        () => state.torrents[torrentId] !== undefined,
        () => new Promise<void>((resolve) => {
          wake = resolve;
        }),
      );

      const pieces = await client.subscribe({
        selector: { type: "torrent", torrent_id: torrentId },
        projection: "piece_activity",
        delivery: {
          min_interval_millis: 0,
          max_queue_bytes: 256 * 1024,
        },
      });
      const pieceTask = consume(pieces);
      await waitUntil(
        () => state.torrents[torrentId]?.state === "complete",
        () => new Promise<void>((resolve) => {
          wake = resolve;
        }),
      );

      expect(requested).toBeGreaterThan(0);
      expect(received).toBeGreaterThan(0);
      expect(stored).toBeGreaterThan(0);
      expect(state.torrents[torrentId]?.verified_piece_count).toBe(3);
      await pieces.close();
      await list.close();
      await Promise.all([listTask, pieceTask]);
      console.log(
        `gateway_interop info_hash=${torrentId} updates=${updates} ` +
          `requested=${requested} received=${received} stored=${stored}`,
      );
    },
    45_000,
  );
});

async function waitUntil(
  predicate: () => boolean,
  nextChange: () => Promise<void>,
): Promise<void> {
  const deadline = Date.now() + 30_000;
  while (!predicate()) {
    const remaining = deadline - Date.now();
    if (remaining <= 0) throw new Error("interop view did not converge");
    await withTimeout(nextChange(), remaining);
  }
}

async function withTimeout(
  operation: Promise<void>,
  timeoutMillis: number,
): Promise<void> {
  let timer: ReturnType<typeof globalThis.setTimeout> | undefined;
  try {
    await Promise.race([
      operation,
      new Promise<never>((_, reject) => {
        timer = globalThis.setTimeout(
          () => reject(new Error("interop view did not converge")),
          timeoutMillis,
        );
      }),
    ]);
  } finally {
    if (timer !== undefined) globalThis.clearTimeout(timer);
  }
}

function rangeBytes(ranges: ReadonlyArray<IndexRange> | undefined): number {
  return (
    ranges?.reduce(
      (total, range) => total + range.end_exclusive - range.start,
      0,
    ) ?? 0
  );
}

function activeRangeBytes(
  active:
    | ReadonlyArray<{
        requested: ReadonlyArray<IndexRange>;
        received: ReadonlyArray<IndexRange>;
        stored: ReadonlyArray<IndexRange>;
      }>
    | undefined,
  field: "requested" | "received" | "stored",
): number {
  return Math.max(0, ...(active ?? []).map((piece) => rangeBytes(piece[field])));
}
