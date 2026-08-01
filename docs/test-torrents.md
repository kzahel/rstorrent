# Public Test Torrents

This catalog records public torrents that are useful for manual and
interoperability testing. Swarm health, tracker availability, WebSeed
behavior, and remote content can change independently of this repository.
Treat successful live retrieval as current evidence, not a permanent
guarantee.

RSTorrent currently understands the retained UDP tracker URLs but not the
WebSocket trackers, WebSeed, or exact-source URL in these magnets. Tactical
`013` added an ignored, metadata-only Big Buck Bunny probe under explicit
online policy. Tactical `018` made its peer and metadata state inspectable. A
later ten-run RSTorrent cohort completed metadata 8/10 through UDP trackers
and 7/10 through DHT; pinned libtorrent `2.0.13.0` completed the corresponding
reference-only cohorts 10/10. These runs stop at the verified 21,307-byte info
dictionary. They do not establish full completion of the 276,445,467-byte
payload.

## Comparative Smoke Policy

The planned headless comparator uses this catalog as variable live evidence,
not as a deterministic test fixture. It runs RSTorrent and the pinned
libtorrent reference sequentially in isolated temporary profiles and classifies
the paired outcome before comparing speed:

- both complete: compare timing, completion, and resource measurements;
- libtorrent completes and RSTorrent does not: actionable RSTorrent gap;
- both fail or time out: inconclusive public-swarm attempt; and
- RSTorrent completes and libtorrent does not: record the success, but treat
  the reference comparison as inconclusive.

Common-denominator mode disables libtorrent capabilities that RSTorrent does
not claim for the scenario. Full-reference mode retains them and reports the
user-visible capability gap. DHT campaign runs remove tracker parameters and
compare cold and warm session starts using the same stable info hash.

Every run has explicit duration, payload, connection, bandwidth, disk, and
artifact-retention bounds. Correct verified completion is a hard gate; public
speed ratios are initially observations rather than CI thresholds. Detailed
measurement and cleanup requirements live in
[`topics/performance-and-live-evidence.md`](topics/performance-and-live-evidence.md).

## WebTorrent Free Torrents

Source: [WebTorrent Free Torrents](https://webtorrent.io/free-torrents)

Retrieved: 2026-07-30

WebTorrent describes these as public-domain or Creative Commons torrents
useful for testing. The source page currently publishes five magnet links.
The exact links below retain its UDP and WebSocket trackers, HTTP WebSeed base,
and exact `.torrent` source.

The live web inspection toolbar exposes these exact entries beneath More > Add
test torrent. Its checked-in TypeScript projection has a byte-for-byte drift
test against `tests/live/torrents.json`; the shortcut does not change the live
evidence policy or imply that a changing public swarm will complete.

### Big Buck Bunny

Info hash: `dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c`

```text
magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny&tr=udp%3A%2F%2Fexplodie.org%3A6969&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969&tr=udp%3A%2F%2Ftracker.empire-js.us%3A1337&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=wss%3A%2F%2Ftracker.btorrent.xyz&tr=wss%3A%2F%2Ftracker.fastcast.nz&tr=wss%3A%2F%2Ftracker.openwebtorrent.com&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F&xs=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2Fbig-buck-bunny.torrent
```

### Cosmos Laundromat

Info hash: `c9e15763f722f23e98a29decdfae341b98d53056`

```text
magnet:?xt=urn:btih:c9e15763f722f23e98a29decdfae341b98d53056&dn=Cosmos+Laundromat&tr=udp%3A%2F%2Fexplodie.org%3A6969&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969&tr=udp%3A%2F%2Ftracker.empire-js.us%3A1337&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=wss%3A%2F%2Ftracker.btorrent.xyz&tr=wss%3A%2F%2Ftracker.fastcast.nz&tr=wss%3A%2F%2Ftracker.openwebtorrent.com&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F&xs=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2Fcosmos-laundromat.torrent
```

### Sintel

Info hash: `08ada5a7a6183aae1e09d831df6748d566095a10`

```text
magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel&tr=udp%3A%2F%2Fexplodie.org%3A6969&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969&tr=udp%3A%2F%2Ftracker.empire-js.us%3A1337&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=wss%3A%2F%2Ftracker.btorrent.xyz&tr=wss%3A%2F%2Ftracker.fastcast.nz&tr=wss%3A%2F%2Ftracker.openwebtorrent.com&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F&xs=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2Fsintel.torrent
```

### Tears of Steel

Info hash: `209c8226b299b308beaf2b9cd3fb49212dbd13ec`

```text
magnet:?xt=urn:btih:209c8226b299b308beaf2b9cd3fb49212dbd13ec&dn=Tears+of+Steel&tr=udp%3A%2F%2Fexplodie.org%3A6969&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969&tr=udp%3A%2F%2Ftracker.empire-js.us%3A1337&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=wss%3A%2F%2Ftracker.btorrent.xyz&tr=wss%3A%2F%2Ftracker.fastcast.nz&tr=wss%3A%2F%2Ftracker.openwebtorrent.com&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F&xs=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2Ftears-of-steel.torrent
```

### The WIRED CD — Rip. Sample. Mash. Share

Info hash: `a88fda5954e89178c372716a6a78b8180ed4dad3`

```text
magnet:?xt=urn:btih:a88fda5954e89178c372716a6a78b8180ed4dad3&dn=The+WIRED+CD+-+Rip.+Sample.+Mash.+Share&tr=udp%3A%2F%2Fexplodie.org%3A6969&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969&tr=udp%3A%2F%2Ftracker.empire-js.us%3A1337&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=wss%3A%2F%2Ftracker.btorrent.xyz&tr=wss%3A%2F%2Ftracker.fastcast.nz&tr=wss%3A%2F%2Ftracker.openwebtorrent.com&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F&xs=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2Fwired-cd.torrent
```
