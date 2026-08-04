import { afterAll, describe, expect, it } from "vitest";

import type { IndexRange, RequestEnvelope, ViewSnapshot } from "./api";
import { HttpApplicationClient } from "./api/client";
import { ViewController } from "./view-controller";
import type { ViewSetState } from "./view-set-reducer";

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
let controller: ViewController | undefined;
let client: HttpApplicationClient | undefined;

afterAll(async () => {
  await controller?.close();
  await client?.close();
});

liveDescribe("authenticated polling gateway interop", () => {
  it(
    "reduces one leased view set through a complete controlled download",
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
      client = new HttpApplicationClient(
        gatewayUrl,
        gatewayToken,
        gatewayOrigin,
      );
      const hello = await client.hello();
      expect(hello.api.current).toBe(1);
      expect(hello.encodings).toContain("json");
      expect(hello.deliveries).toContain("long_poll");

      let state: ViewSetState | undefined;
      let batches = 0;
      let requested = 0;
      let received = 0;
      let stored = 0;
      let controllerError: Error | undefined;
      let wake: (() => void) | undefined;
      const changed = (): void => {
        wake?.();
        wake = undefined;
      };
      controller = await ViewController.open(
        client,
        [
          {
            type: "torrent_list",
            view_id: "library",
            delivery: { min_interval_millis: 0 },
          },
        ],
        (next) => {
          state = next;
          batches += 1;
          const summary = torrentFrom(next.views.library, torrentId);
          if (summary !== undefined) {
            requested = Math.max(requested, Number(summary.requested_bytes));
            received = Math.max(received, Number(summary.received_bytes));
            stored = Math.max(stored, Number(summary.stored_bytes));
          }
          const pieces = next.views.pieces;
          if (pieces?.type === "piece_activity") {
            requested = Math.max(
              requested,
              ...pieces.active.map((piece) => rangeBytes(piece.requested)),
            );
            received = Math.max(
              received,
              ...pieces.active.map((piece) => rangeBytes(piece.received)),
            );
            stored = Math.max(
              stored,
              ...pieces.active.map((piece) => rangeBytes(piece.stored)),
            );
          }
          changed();
        },
        (error) => {
          controllerError = error;
          changed();
        },
      );

      const request: RequestEnvelope = {
        version: 1,
        request_id: "view-set-interop-add",
        command: {
          type: "add_magnet",
          magnet,
          storage_root: "downloads",
          start_content: true,
          skip_files: [],
        },
      };
      const response = await controller.dispatch(request);
      expect(response.status).toBe("success");
      await waitUntil(
        () => {
          if (controllerError !== undefined) throw controllerError;
          return (
            state !== undefined &&
            torrentFrom(state.views.library, torrentId) !== undefined
          );
        },
        () =>
          new Promise<void>((resolve) => {
            wake = resolve;
          }),
      );

      await controller.setViews([
        {
          type: "torrent_list",
          view_id: "library",
          delivery: { min_interval_millis: 0 },
        },
        {
          type: "torrent_summary",
          view_id: "details",
          torrent_id: torrentId,
          delivery: { min_interval_millis: 0 },
        },
        {
          type: "piece_activity",
          view_id: "pieces",
          torrent_id: torrentId,
          delivery: { min_interval_millis: 0 },
        },
      ]);
      await waitUntil(
        () => {
          if (controllerError !== undefined) throw controllerError;
          const torrent =
            state === undefined
              ? undefined
              : torrentFrom(state.views.library, torrentId);
          return (
            torrent?.state === "complete" &&
            torrent.storage_state === "published"
          );
        },
        () =>
          new Promise<void>((resolve) => {
            wake = resolve;
          }),
      );

      const summary = state && torrentFrom(state.views.library, torrentId);
      expect(summary?.verified_piece_count).toBe(3);
      expect(summary?.storage_state).toBe("published");
      expect(state?.views.details?.type).toBe("torrent");
      expect(state?.views.pieces?.type).toBe("piece_activity");
      expect(requested).toBeGreaterThan(0);
      expect(received).toBeGreaterThan(0);
      expect(stored).toBeGreaterThan(0);
      await controller.close();
      controller = undefined;
      console.log(
        `view_set_interop info_hash=${torrentId} batches=${batches} ` +
          `requested=${requested} received=${received} stored=${stored} ` +
          "view_set_close=ok",
      );
    },
    45_000,
  );
});

function torrentFrom(snapshot: ViewSnapshot | undefined, id: string) {
  if (snapshot?.type === "torrent_list") {
    return snapshot.torrents.find((torrent) => torrent.torrent_id === id);
  }
  if (snapshot?.type === "torrent" && snapshot.torrent?.torrent_id === id) {
    return snapshot.torrent;
  }
  return undefined;
}

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
