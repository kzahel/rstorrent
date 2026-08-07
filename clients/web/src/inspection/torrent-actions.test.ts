import { describe, expect, it } from "vitest";

import type { TorrentRow } from "./model";
import {
  TORRENT_ACTIONS,
  orderedSelectedTorrentRows,
  torrentActionAvailability,
  torrentActionsForPlacement,
} from "./torrent-actions";

describe("torrent selection actions", () => {
  it("keeps direct plus overflow actions equal to the context action set", () => {
    const toolbarUnion = [
      ...torrentActionsForPlacement("direct"),
      ...torrentActionsForPlacement("overflow"),
    ].map((action) => action.id);
    expect(new Set(toolbarUnion)).toEqual(
      new Set(TORRENT_ACTIONS.map((action) => action.id)),
    );
    expect(TORRENT_ACTIONS.map((action) => [action.group, action.id])).toEqual([
      ["transfer", "start"],
      ["transfer", "pause"],
      ["transfer", "force_recheck"],
      ["sharing", "copy_magnet"],
      ["organization", "archive"],
      ["organization", "restore"],
      ["destructive", "remove"],
    ]);
  });

  it("orders complete hidden selections by application order", () => {
    const torrents = Object.fromEntries(
      ["a", "b", "c"].map((id) => [id, row(id)]),
    );
    expect(
      orderedSelectedTorrentRows(
        ["c", "a", "b"],
        torrents,
        new Set(["a", "c"]),
      ).map((target) => target.id),
    ).toEqual(["c", "a"]);
  });

  it("normalizes mixed states and never selects an eligible subset", () => {
    const mixed = [row("paused", { status: "paused" }), row("running")];
    expect(torrentActionAvailability("start", mixed).disabled).toBe(false);
    expect(torrentActionAvailability("pause", mixed).disabled).toBe(false);
    expect(torrentActionAvailability("archive", mixed).disabled).toBe(false);
    expect(torrentActionAvailability("restore", mixed).disabled).toBe(true);

    const recheckMixed = [
      row("ready"),
      row("missing", { forceRecheckAvailable: false }),
    ];
    expect(torrentActionAvailability("force_recheck", recheckMixed)).toEqual({
      disabled: true,
      reason: "1 selected torrent does not have managed content available to recheck.",
    });
  });

  it("keeps copy available but blocks mutations during removal", () => {
    const targets = [row("removing", { removalState: "pending" })];
    expect(torrentActionAvailability("copy_magnet", targets).disabled).toBe(false);
    expect(torrentActionAvailability("pause", targets).disabled).toBe(true);
    expect(torrentActionAvailability("remove", targets).disabled).toBe(true);
  });
});

function row(id: string, overrides: Partial<TorrentRow> = {}): TorrentRow {
  return {
    id,
    name: id,
    status: "downloading",
    sizeBytes: 100,
    progress: 0.5,
    checking: null,
    downloadRate: 1,
    uploadRate: 0,
    downloadedBytes: 50,
    uploadedBytes: 0,
    peersConnected: 1,
    peersKnown: 2,
    configuredTrackerCount: 0,
    requiredPayloadBytes: "100",
    remainingPayloadBytes: "50",
    etaDownloadRateBytes: "1",
    eta: { state: "estimate", seconds: "50" },
    addedAtMs: 1,
    archived: false,
    removalState: null,
    deleteManagedDataSupported: true,
    forceRecheckAvailable: true,
    infoHash: id,
    error: null,
    progressReason: "test",
    ...overrides,
  };
}
