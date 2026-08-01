export interface TestTorrentShortcut {
  readonly id: string;
  readonly name: string;
  readonly menuLabel: string;
  readonly infoHash: string;
  readonly magnet: string;
}

const COMMON_WEBTORRENT_PARAMETERS = [
  "tr=udp%3A%2F%2Fexplodie.org%3A6969",
  "tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969",
  "tr=udp%3A%2F%2Ftracker.empire-js.us%3A1337",
  "tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969",
  "tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337",
  "tr=wss%3A%2F%2Ftracker.btorrent.xyz",
  "tr=wss%3A%2F%2Ftracker.fastcast.nz",
  "tr=wss%3A%2F%2Ftracker.openwebtorrent.com",
  "ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F",
].join("&");

const SOURCES = [
  {
    id: "big-buck-bunny",
    name: "Big Buck Bunny",
    menuLabel: "Big Buck Bunny",
    infoHash: "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
    encodedName: "Big+Buck+Bunny",
  },
  {
    id: "cosmos-laundromat",
    name: "Cosmos Laundromat",
    menuLabel: "Cosmos Laundromat",
    infoHash: "c9e15763f722f23e98a29decdfae341b98d53056",
    encodedName: "Cosmos+Laundromat",
  },
  {
    id: "sintel",
    name: "Sintel",
    menuLabel: "Sintel",
    infoHash: "08ada5a7a6183aae1e09d831df6748d566095a10",
    encodedName: "Sintel",
  },
  {
    id: "tears-of-steel",
    name: "Tears of Steel",
    menuLabel: "Tears of Steel",
    infoHash: "209c8226b299b308beaf2b9cd3fb49212dbd13ec",
    encodedName: "Tears+of+Steel",
  },
  {
    id: "wired-cd",
    name: "The WIRED CD — Rip. Sample. Mash. Share",
    menuLabel: "WIRED CD",
    infoHash: "a88fda5954e89178c372716a6a78b8180ed4dad3",
    encodedName: "The+WIRED+CD+-+Rip.+Sample.+Mash.+Share",
  },
] as const;

export const WEBTORRENT_TEST_TORRENTS: readonly TestTorrentShortcut[] =
  SOURCES.map((source) => ({
    id: source.id,
    name: source.name,
    menuLabel: source.menuLabel,
    infoHash: source.infoHash,
    magnet:
      `magnet:?xt=urn:btih:${source.infoHash}` +
      `&dn=${source.encodedName}` +
      `&${COMMON_WEBTORRENT_PARAMETERS}` +
      `&xs=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F${source.id}.torrent`,
  }));
