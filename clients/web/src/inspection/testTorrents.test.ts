import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { validateTorrentInput } from "./torrentInput";
import { WEBTORRENT_TEST_TORRENTS } from "./testTorrents";

interface SourceCatalog {
  readonly schema_version: number;
  readonly torrents: readonly {
    readonly slug: string;
    readonly name: string;
    readonly info_hash: string;
    readonly magnet: string | null;
  }[];
}

describe("WebTorrent test torrent shortcuts", () => {
  it("match the machine-readable live catalog exactly", () => {
    const source = JSON.parse(
      readFileSync(
        new URL("../../../../tests/live/torrents.json", import.meta.url),
        "utf8",
      ),
    ) as SourceCatalog;

    expect(source.schema_version).toBe(2);
    expect(
      WEBTORRENT_TEST_TORRENTS.map((torrent) => ({
        slug: torrent.id,
        name: torrent.name,
        info_hash: torrent.infoHash,
        magnet: torrent.magnet,
      })),
    ).toEqual(
      source.torrents.filter((torrent) => torrent.magnet !== null).map((torrent) => ({
        slug: torrent.slug,
        name: torrent.name,
        info_hash: torrent.info_hash,
        magnet: torrent.magnet,
      })),
    );
  });

  it("contains unique bounded magnets accepted by toolbar validation", () => {
    expect(new Set(WEBTORRENT_TEST_TORRENTS.map((torrent) => torrent.id)).size).toBe(5);
    expect(
      WEBTORRENT_TEST_TORRENTS.every(
        (torrent) => validateTorrentInput(torrent.magnet).accepted,
      ),
    ).toBe(true);
  });
});
