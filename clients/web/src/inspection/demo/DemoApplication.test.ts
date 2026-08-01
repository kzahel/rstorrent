import { afterEach, describe, expect, it, vi } from "vitest";

import { InspectionController } from "../controller";
import { DemoApplication } from "./DemoApplication";

afterEach(() => {
  vi.useRealTimers();
});

describe("DemoApplication", () => {
  it("advances a deterministic healthy download and joins its clock", async () => {
    vi.useFakeTimers();
    const application = new DemoApplication({
      scenarioId: "healthy-download",
      running: true,
      tickMs: 1_000,
    });
    const controller = new InspectionController(application);
    controller.start();

    expect(controller.store.getState().torrentOrder).toHaveLength(3);
    await vi.advanceTimersByTimeAsync(8_000);
    const torrent = controller.store.getState().torrents[
      controller.store.getState().torrentOrder[0]!
    ];
    expect(torrent?.status).toBe("downloading");
    expect(torrent?.progress).toBeGreaterThan(0);

    await controller.close();
    const revision = controller.store.getState().revision;
    await vi.advanceTimersByTimeAsync(5_000);
    expect(controller.store.getState().revision).toBe(revision);
  });

  it("recovers a tracker cohort and represents reconnect as a new identity", async () => {
    const application = new DemoApplication({
      scenarioId: "tracker-recovery",
      running: false,
    });
    const controller = new InspectionController(application);
    controller.start();
    const torrentId = controller.store.getState().torrentOrder[0]!;
    expect(controller.store.getState().peersByTorrent[torrentId]?.order).toHaveLength(0);

    await controller.dispatch({
      type: "advance_demo_clock",
      milliseconds: 24_000,
    });
    const originalConnection = controller.store.getState().peersByTorrent[
      torrentId
    ]?.order[0];
    expect(controller.store.getState().peersByTorrent[torrentId]?.order).toHaveLength(14);
    expect(controller.store.getState().torrents[torrentId]?.peersKnown).toBe(42);
    await controller.dispatch({
      type: "advance_demo_clock",
      milliseconds: 22_000,
    });
    const nextPeerSet = controller.store.getState().peersByTorrent[torrentId];
    expect(nextPeerSet?.rows[originalConnection!]).toBeUndefined();
    expect(nextPeerSet?.order[0]).toContain("reconnect");
    await controller.close();
  });

  it("applies pause, archive, add, and reset commands", async () => {
    const application = new DemoApplication({
      scenarioId: "healthy-download",
      elapsedMs: 42_000,
      running: false,
    });
    const controller = new InspectionController(application);
    controller.start();
    const torrentId = controller.store.getState().torrentOrder[0]!;

    await controller.dispatch({ type: "pause", torrentId });
    expect(controller.store.getState().torrents[torrentId]?.status).toBe("paused");
    await controller.dispatch({ type: "archive", torrentId });
    expect(controller.store.getState().torrents[torrentId]?.archived).toBe(true);
    await controller.dispatch({ type: "add_demo_torrent" });
    expect(controller.store.getState().torrentOrder).toHaveLength(4);
    await expect(
      controller.dispatch({
        type: "add_magnet",
        magnet:
          "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213",
      }),
    ).rejects.toThrow("unavailable in demo scenarios");
    await controller.dispatch({ type: "reset_demo" });
    expect(controller.store.getState().torrentOrder).toHaveLength(3);
    expect(controller.store.getState().demo?.elapsedMs).toBe(0);
    await controller.close();
  });

  it("materializes the bounded scale fixture without starting a timer", async () => {
    vi.useFakeTimers();
    const application = new DemoApplication({
      scenarioId: "large-swarm",
      running: false,
    });
    const controller = new InspectionController(application);
    controller.start();
    const state = controller.store.getState();
    const torrentId = state.torrentOrder[0]!;
    expect(state.torrentOrder).toHaveLength(2_000);
    expect(state.peersByTorrent[torrentId]?.order).toHaveLength(10_000);
    const revision = state.revision;
    await vi.advanceTimersByTimeAsync(60_000);
    expect(controller.store.getState().revision).toBe(revision);
    await controller.close();
  });
});
