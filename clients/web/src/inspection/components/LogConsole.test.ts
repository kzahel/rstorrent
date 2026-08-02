import { describe, expect, it } from "vitest";

import type { LogRow } from "../model";
import { filterLogRows, layoutLogRows } from "./LogConsole";

describe("diagnostic console projection", () => {
  it("filters locally without changing chronological order", () => {
    const rows = [
      row("1", "info", "lifecycle.session", null, "opened"),
      row("2", "debug", "peer.connection", "selected", "connected"),
      row("3", "warning", "tracker.announce", "other", "timed out"),
      {
        ...row("4", "error", "storage.io", "selected", "write failed"),
        fields: [
          { key: "error", value: { type: "error_code", value: "disk_full" } },
        ],
      },
    ] satisfies readonly LogRow[];
    const filtered = filterLogRows(rows, {
      minimumSeverity: "debug",
      categoryPrefix: "",
      search: "",
      displayTorrentId: "selected",
      clearThroughSequence: null,
      torrents: { selected: { name: "Selected torrent" } },
    });
    expect(filtered.map((item) => item.id)).toEqual(["1", "2", "4"]);
    expect(
      filterLogRows(rows, {
        minimumSeverity: "trace",
        categoryPrefix: "storage",
        search: "disk_full",
        displayTorrentId: null,
        clearThroughSequence: "2",
        torrents: {},
      }).map((item) => item.id),
    ).toEqual(["4"]);
  });

  it("includes session records in selected scope and bounds row layout", () => {
    const rows = Array.from({ length: 2_048 }, (_, index) =>
      row(String(index + 1), "info", "test.event", null, "bounded"),
    );
    const selected = filterLogRows(rows.slice(0, 2), {
      minimumSeverity: "info",
      categoryPrefix: "",
      search: "",
      displayTorrentId: "selected",
      clearThroughSequence: null,
      torrents: {},
    });
    expect(selected).toHaveLength(2);
    const layout = layoutLogRows(rows, new Set(["10"]), true);
    expect(layout.rows).toHaveLength(2_048);
    expect(layout.rows[10]!.top - layout.rows[9]!.top).toBeGreaterThan(34);
    expect(layout.totalHeight).toBeGreaterThan(2_048 * 34);
  });
});

function row(
  id: string,
  severity: LogRow["severity"],
  category: string,
  torrentId: string | null,
  message: string,
): LogRow {
  return {
    id,
    timestampMs: Number(id),
    severity,
    category,
    code: `${category.replaceAll(".", "_")}_event`,
    message,
    torrentId,
    subjects: [],
    fields: [],
  };
}
