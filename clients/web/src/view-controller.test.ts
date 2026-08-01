import { describe, expect, it } from "vitest";

import type {
  ApiHello,
  OpenViewSetRequest,
  OpenViewSetResponse,
  RequestEnvelope,
  ResponseEnvelope,
  UpdateBatch,
  UpdateViewSetRequest,
  ViewSpec,
} from "./api";
import type { ApplicationViewClient } from "./api/client";
import { HttpApiError } from "./api/client";
import { ViewController } from "./view-controller";
import { ViewSetContinuityError } from "./view-set-reducer";

const viewSetId = "vs_000102030405060708090a0b0c0d0e0f";
const listView: ViewSpec = {
  type: "torrent_list",
  view_id: "library",
  delivery: { min_interval_millis: 0 },
};

class FakeClient implements ApplicationViewClient {
  public active = 0;
  public maximumActive = 0;
  public closedViewSet = false;
  public openCount = 0;
  public readonly after: string[] = [];
  public batches: Array<UpdateBatch | Error> = [];

  public async hello(): Promise<ApiHello> {
    throw new Error("unused");
  }

  public async dispatch(request: RequestEnvelope): Promise<ResponseEnvelope> {
    return {
      version: 1,
      request_id: request.request_id,
      revision: "0",
      status: "success",
      snapshot: { profile_id: "test", revision: "0", torrents: [] },
    };
  }

  public async openViewSet(
    _request: OpenViewSetRequest,
  ): Promise<OpenViewSetResponse> {
    this.openCount += 1;
    const openedId =
      this.openCount === 1
        ? viewSetId
        : "vs_111102030405060708090a0b0c0d0e0f";
    const initial = batch("0", "1", [
      {
        type: "snapshot",
        view_id: "library",
        snapshot: { type: "torrent_list", torrents: [] },
      },
    ]);
    initial.view_set_id = openedId;
    initial.epoch = String(6 + this.openCount);
    return {
      view_set_id: openedId,
      lease_millis: "300000",
      effective_queue_bytes: 262_144,
      effective_views: [listView],
      initial,
    };
  }

  public async updateViewSet(
    _viewSetId: string,
    _request: UpdateViewSetRequest,
  ): Promise<void> {}

  public async nextUpdates(
    _viewSetId: string,
    after: string,
    _waitMillis: number,
    signal?: AbortSignal,
  ): Promise<UpdateBatch> {
    this.after.push(after);
    this.active += 1;
    this.maximumActive = Math.max(this.maximumActive, this.active);
    try {
      const ready = this.batches.shift();
      if (ready instanceof Error) throw ready;
      if (ready !== undefined) return ready;
      await new Promise<void>((_resolve, reject) => {
        signal?.addEventListener(
          "abort",
          () => reject(new Error("poll aborted")),
          { once: true },
        );
      });
      throw new Error("unreachable");
    } finally {
      this.active -= 1;
    }
  }

  public async closeViewSet(): Promise<void> {
    this.closedViewSet = true;
  }

  public async close(): Promise<void> {}
}

describe("view controller", () => {
  it("keeps one poll in flight and acknowledges only reduced state", async () => {
    const client = new FakeClient();
    client.batches.push(
      batch("1", "2", [
        {
          type: "patch",
          view_id: "library",
          patch: { type: "torrent_list", upsert: [], removed: [] },
        },
      ]),
    );
    const reached = promiseWithResolvers<void>();
    const controller = await ViewController.open(client, [listView], (state) => {
      if (state.cursor === "2") reached.resolve();
    });
    await reached.promise;
    await waitUntil(() => client.active === 1);

    expect(client.after.slice(0, 2)).toEqual(["1", "2"]);
    expect(client.maximumActive).toBe(1);
    await controller.close();
    expect(client.closedViewSet).toBe(true);
    expect(client.active).toBe(0);
  });

  it("does not acknowledge a batch that fails continuity reduction", async () => {
    const client = new FakeClient();
    client.batches.push(batch("9", "10", []));
    const failed = promiseWithResolvers<Error>();
    const controller = await ViewController.open(
      client,
      [listView],
      () => {},
      (error) => failed.resolve(error),
    );
    expect(await failed.promise).toBeInstanceOf(ViewSetContinuityError);
    await Promise.resolve();
    expect(client.after).toEqual(["1"]);
    expect(controller.current().cursor).toBe("1");
    await controller.close();
  });

  it("reopens an expired lease with the desired views", async () => {
    const client = new FakeClient();
    client.batches.push(
      new HttpApiError(404, "unknown_view_set", "view set is unavailable"),
    );
    const recovered = promiseWithResolvers<void>();
    const controller = await ViewController.open(client, [listView], (state) => {
      if (state.viewSetId.startsWith("vs_111")) recovered.resolve();
    });
    await recovered.promise;
    expect(client.openCount).toBe(2);
    expect(controller.current().viewSetId).toBe(
      "vs_111102030405060708090a0b0c0d0e0f",
    );
    await controller.close();
  });
});

function batch(
  baseCursor: string,
  cursor: string,
  updates: UpdateBatch["updates"],
): UpdateBatch {
  return {
    api_version: 1,
    view_set_id: viewSetId,
    epoch: "7",
    base_cursor: baseCursor,
    cursor,
    durable_revision: cursor,
    updates,
  };
}

function promiseWithResolvers<T>(): {
  promise: Promise<T>;
  resolve(value: T): void;
} {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolved) => {
    resolve = resolved;
  });
  return { promise, resolve };
}

async function waitUntil(predicate: () => boolean): Promise<void> {
  for (let attempts = 0; attempts < 100; attempts += 1) {
    if (predicate()) return;
    await Promise.resolve();
  }
  throw new Error("condition was not reached");
}
