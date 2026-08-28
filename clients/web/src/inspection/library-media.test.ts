import { describe, expect, it } from "vitest";

import type { MediaRow } from "./model";
import { episodeLabel, sortMediaRows } from "./library-media";

describe("Library media ordering", () => {
  it("orders typed episodes numerically before unclassified videos", () => {
    const rows = [
      media("unclassified", 4, { type: "unclassified_video" }),
      media("episode-10", 3, episode(1, 10)),
      media("episode-2", 2, episode(1, 2)),
      media("episode-1", 1, episode(1, 1)),
    ];
    expect(sortMediaRows(rows).map((row) => row.id)).toEqual([
      "episode-1",
      "episode-2",
      "episode-10",
      "unclassified",
    ]);
  });

  it("retains multi-episode endings in the compact label", () => {
    const row = media("episode", 1, {
      ...episode(2, 7),
      endingEpisodeNumber: 8,
    });
    expect(episodeLabel(row)).toBe("S02 · E07–08");
  });

  it("uses case-insensitive paths before file index as the stable tie", () => {
    const laterPath = {
      ...media("later-path", 1, episode(1, 2)),
      path: ["Sample Show", "Zeta", "episode.mkv"],
    };
    const earlierPath = {
      ...media("earlier-path", 9, episode(1, 2)),
      path: ["sample show", "alpha", "episode.mkv"],
    };
    const sameFoldedPath = {
      ...media("file-index-tie", 0, episode(1, 2)),
      path: ["SAMPLE SHOW", "ALPHA", "EPISODE.MKV"],
    };

    expect(
      sortMediaRows([laterPath, earlierPath, sameFoldedPath]).map(
        (row) => row.id,
      ),
    ).toEqual(["file-index-tie", "earlier-path", "later-path"]);
  });
});

function episode(seasonNumber: number, episodeNumber: number) {
  return {
    type: "episode" as const,
    seriesTitleHint: "Sample Show",
    seasonNumber,
    episodeNumber,
    endingEpisodeNumber: null,
  };
}

function media(
  id: string,
  fileIndex: number,
  role: MediaRow["role"],
): MediaRow {
  return {
    id,
    torrentId: "torrent",
    fileIndex,
    path: [`${id}.mkv`],
    name: `${id}.mkv`,
    folder: "",
    extension: "mkv",
    lengthBytes: "1024",
    selection: "normal",
    doneBytes: "0",
    verifiedBytes: "0",
    mediaAvailability: "unverified",
    role,
  };
}
