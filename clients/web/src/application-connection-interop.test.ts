import { afterAll, describe, expect, it } from "vitest";
import { WebSocket as NodeWebSocket } from "ws";

import type { IndexRange, RequestEnvelope, ViewSpec } from "./api";
import { ViewController } from "./view-controller";
import type { ViewSetState } from "./view-set-reducer";
import type { ApplicationWebSocket } from "./websocket-view-client";
import { WebSocketApplicationViewClient } from "./websocket-view-client";

interface ProcessEnvironment {
  process: { env: Record<string, string | undefined> };
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
let client: WebSocketApplicationViewClient | undefined;
let controller: ViewController | undefined;

afterAll(async () => {
  await controller?.close();
  await client?.close();
});

liveDescribe("multiplexed application connection interop", () => {
  it(
    "reduces leased list and piece views through exact completion",
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
      client = new WebSocketApplicationViewClient(
        gatewayUrl,
        gatewayToken,
        (url) =>
          new NodeWebSocket(url, {
            origin: gatewayOrigin,
          }) as unknown as ApplicationWebSocket,
        "00000000000000000000000000000001",
      );
      let state: ViewSetState | undefined;
      let updates = 0;
      let requested = 0;
      let received = 0;
      let stored = 0;
      let controllerError: Error | undefined;
      const library: ViewSpec = {
        type: "torrent_list",
        view_id: "library",
        delivery: { min_interval_millis: 0 },
      };
      controller = await ViewController.open(
        client,
        [library],
        (next) => {
          state = next;
          updates += 1;
          const pieces = next.views.pieces;
          if (pieces?.type === "piece_activity") {
            requested = Math.max(
              requested,
              activeRangeBytes(pieces.active, "requested"),
            );
            received = Math.max(
              received,
              activeRangeBytes(pieces.active, "received"),
            );
            stored = Math.max(
              stored,
              activeRangeBytes(pieces.active, "stored"),
            );
          }
        },
        (error) => {
          controllerError = error;
        },
      );

      const request: RequestEnvelope = {
        version: 1,
        request_id: "web-connection-interop-add",
        command: {
          type: "add_magnet",
          magnet,
          storage_root: "downloads",
          skip_files: [],
        },
      };
      const response = await controller.dispatch(request);
      expect(response.status).toBe("success");
      await waitUntil(
        () => libraryTorrent(state, torrentId) !== undefined,
        () => controllerError,
      );
      await controller.setViews([
        library,
        {
          type: "piece_activity",
          view_id: "pieces",
          torrent_id: torrentId,
          delivery: { min_interval_millis: 0 },
        },
      ]);
      await waitUntil(
        () => libraryTorrent(state, torrentId)?.state === "complete",
        () => controllerError,
      );

      const finalPieces = state?.views.pieces;
      expect(finalPieces?.type).toBe("piece_activity");
      expect(
        finalPieces?.type === "piece_activity"
          ? rangeBytes(finalPieces.verified)
          : 0,
      ).toBe(3);
      expect(libraryTorrent(state, torrentId)?.verified_piece_count).toBe(3);
      console.log(
        `application_connection_interop info_hash=${torrentId} ` +
          `updates=${updates} requested=${requested} received=${received} ` +
          `stored=${stored}`,
      );
    },
    45_000,
  );
});

function libraryTorrent(state: ViewSetState | undefined, id: string) {
  const library = state?.views.library;
  return library?.type === "torrent_list"
    ? library.torrents.find((torrent) => torrent.torrent_id === id)
    : undefined;
}

async function waitUntil(
  predicate: () => boolean,
  failure: () => Error | undefined,
): Promise<void> {
  const deadline = Date.now() + 30_000;
  while (!predicate()) {
    const error = failure();
    if (error !== undefined) throw error;
    const remaining = deadline - Date.now();
    if (remaining <= 0) throw new Error("interop view did not converge");
    await new Promise((resolve) => globalThis.setTimeout(resolve, 25));
  }
}

function rangeBytes(ranges: ReadonlyArray<IndexRange>): number {
  return ranges.reduce(
    (total, range) => total + range.end_exclusive - range.start,
    0,
  );
}

function activeRangeBytes(
  active: ReadonlyArray<{
    requested: ReadonlyArray<IndexRange>;
    received: ReadonlyArray<IndexRange>;
    stored: ReadonlyArray<IndexRange>;
  }>,
  field: "requested" | "received" | "stored",
): number {
  return Math.max(0, ...active.map((piece) => rangeBytes(piece[field])));
}
