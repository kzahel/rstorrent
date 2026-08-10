# Public Torrent Testing

Topic: `public-torrent-testing`

Status: The nine-entry schema-v2 catalog pins exact metainfo identity and
geometry for five WebTorrent works plus current Debian, Ubuntu, Arch Linux,
and Linux Mint images from their official project pages. Tactical `122` also
records a bounded paired Big Buck Bunny completion and Ubuntu discovery
failure classification. Public runs remain opt-in, headless, bounded, and
supporting evidence only.

## Scope

This topic owns the small set of well-known, legally distributable public
torrents used to exercise changing-network interoperability. It records why a
project is useful, the protocol shape observed at a dated release, and the
safety rules for refreshing or running it. It does not own protocol support
claims, performance thresholds, or a promise that an external swarm remains
available.

[`tracker-discovery.md`](tracker-discovery.md) owns tracker behavior,
[`dht-discovery.md`](dht-discovery.md) owns DHT behavior, and
[`performance-and-live-evidence.md`](performance-and-live-evidence.md) owns
live-run safety and evidence classification. Tactical
[`096`](../tactical/096-metadata-tracker-activation-and-family-observability.md)
records the completed metadata-only and connection-family repair.

## Catalog And Test Roles

The release names, sizes, URLs, trackers, and DNS capabilities below combine
the 2026-08-06 application evidence with Tactical `122`'s exact 2026-08-10
performance catalog. Refresh source aliases before every run and reject any
metainfo identity or geometry drift rather than treating the table as an
immutable fixture.

| Project | Observed official artifact | Observed discovery shape | Intended role |
| --- | --- | --- | --- |
| WebTorrent | Big Buck Bunny plus four breadth works from the [official free-torrents page](https://webtorrent.io/free-torrents); Big Buck Bunny is 276,445,467 bytes. | The retained metainfo mixes UDP and WebSocket trackers; matched native-engine profiles retain the UDP rows and disable WebSocket trackers. | Small primary completion and legal media breadth. Exact source SHA-256, v1 identity, geometry, trackers, and web seeds are pinned for all five works. |
| Ubuntu | Ubuntu 26.04 live-server amd64, 2,918,598,656 bytes, from the [official server download page](https://ubuntu.com/download/server). The earlier application evidence used Ubuntu 24.04.4. | Tier 0 `https://torrent.ubuntu.com/announce`; tier 1 `https://ipv6.torrent.ubuntu.com/announce`. The exact 26.04 metainfo has no alternate discovery source. | Primary HTTPS application smoke and direct-driver discovery boundary. It does not by itself prove an IPv6 route. |
| Debian | Debian 13.6.0 amd64 netinst, 791,674,880 bytes, from the [official BitTorrent directory](https://cdimage.debian.org/debian-cd/current/amd64/bt-cd/) | One `http://bttracker.debian.org:6969/announce` row; the hostname was dual-stack. | Best next plaintext HTTP and non-default-port tracker smoke. With a genuinely routed IPv6 host it can also verify the observed connection-family field. |
| Arch Linux | Arch Linux 2026.08.01 x86_64, 1,597,014,016 bytes, from the [official download page](https://archlinux.org/download/) | The downloaded metainfo contained no tracker; the page also supplied a magnet. | Trackerless DHT-only metadata and payload discovery. This guards against accidental dependence on trackers and is not an HTTP test. |
| Linux Mint | Linux Mint 22.3 Cinnamon 64-bit, 3,091,660,800 bytes, from the [official edition page](https://www.linuxmint.com/edition.php?id=326) | One public `udp://tracker.opentrackr.org:1337/announce` row. | Cross-distribution UDP interoperability against independently operated tracker infrastructure. It adds little HTTP coverage. |
| Fedora | The [official torrent landing page](https://fedoraproject.org/torrents/) redirected to a missing release page during inspection. | No current metainfo was accepted into the catalog. | Monitor and re-evaluate after the official page is healthy; do not pin a guessed mirror or stale release. |
| openSUSE | The [current Tumbleweed ISO directory](https://download.opensuse.org/download/tumbleweed/iso/) exposed direct files, checksums, signatures, and mirror metadata but no current torrent during inspection. | No current torrent. | Excluded until the project again publishes an official torrent. Metalink and mirror behavior belong to a different feature. |

Internet Archive items and Blender open movies remain later breadth candidates
for multifile, older-metainfo, and web-seed-adjacent behavior. Before adding a
specific item, verify its license, official download page, current metainfo,
payload size, tracker/web-seed composition, and whether it adds a protocol
shape not already covered above.

## Selection Policy

- Prefer an official project page and its exact `.torrent` bytes. Do not use a
  search-result magnet, third-party index, repack, or unverified mirror.
- Record the inspection date, project release, payload size, v1 info hash,
  tracker tiers, web seeds, and DHT/private flags in the run artifact.
- Choose entries for distinct protocol roles. Popularity alone does not
  justify another long-lived fixture.
- Release aliases such as `current` may change between discovery and download.
  Fetch once into the run's temporary root and derive every fact from those
  retained bytes.
- Do not commit third-party metainfo unless its origin, license, redistribution
  permission, and attribution are explicitly reviewed. The normal live
  catalog stores official source URLs and observations, not copied fixtures.

Tactical `122`'s schema-v2 catalog separates stable magnet-only entries from
exact metainfo inputs. An entry records source organization/page/date/license,
roles, allowed input modes, expected geometry, and per-owner limits. Metainfo
mode additionally requires an HTTPS official-host allowlist, outer SHA-256,
v1 info hash, full geometry, tracker tiers, and web seeds before payload work.
All nine entries now pin exact official metainfo source bytes. The five
WebTorrent entries additionally retain stable magnet mode for discovery-only
comparisons.

## Live Run Contract

- Public runs require explicit opt-in and `NetworkPolicy::Online`.
- Use the headless application or diagnostic CLI; do not launch visible
  clients merely to exercise the engine.
- Run one public swarm at a time with explicit metadata, wall-clock, peer,
  request, and payload bounds. Default to metadata-only and stop immediately
  after verification unless payload transfer was separately authorized.
- Use a fresh temporary profile and payload root. Pause or shut down through
  the application owner, wait for joined termination, and delete all temporary
  metainfo, databases, logs, and content afterward.
- Summaries may retain project, release, info hash, tracker host, transport,
  connection family, timings, counts, and terminal state. They must not retain
  raw peer IP addresses, tracker DNS answers, source addresses, credentials,
  or unrelated network captures.
- A dual-stack hostname is not IPv6 evidence. Claim a family only from the
  transport's observed successful connection family. If the host lacks routed
  IPv6, record the row as unavailable and rely on controlled AAAA-only tests.
- One successful or failed run is a dated observation, never a general public-
  tracker reliability or protocol-support claim.

## Current Evidence And Next Work

The 2026-08-06 Ubuntu running-content smoke reached both official HTTPS rows
once and hash-verified metadata in 34.334 seconds before pause. Its preceding
metadata-only run exposed the inactive-tracker defect repaired by Tactical
`096`.

The post-fix metadata-only repeat hash-verified the same official release in
150.736 seconds. Both HTTPS rows completed started and stopped announces,
ended inactive with two attempts, and retained IPv4 as their last successful
connection family. No payload file was created. This verifies the repaired
application lifecycle and honest family projection, but it is not an IPv6 or
public-reliability claim because both Ubuntu names were dual-stack.

An IPv6-only connectivity probe reached Debian's dual-stack tracker host on
port 6969, but the deliberately incomplete request received no HTTP response.
That proves only a route and TCP connection, not an accepted tracker announce.

Tactical `122`'s 2026-08-10 quick comparison then completed Big Buck Bunny for
both owners and independently verified all 276,445,467 bytes. Its Ubuntu 26.04
pair classified `reference_only`: libtorrent verified 293,289,984 bytes at the
10% milestone in 4.731 seconds, while RSTorrent received no tracker response
or peer candidate before its 120-second boundary. The public probe uses the
focused direct engine manager, whose tracker task currently rejects HTTP(S)
endpoints; the long-lived application owner already supports them. The result
therefore identifies a direct-driver integration gap rather than contradicting
the earlier application HTTPS evidence or measuring Ubuntu payload throughput.

The next source-first candidate is that direct-manager HTTP(S) integration.
Separate later breadth remains a bounded application-level Debian run on a
host with native routed IPv6 and an Arch DHT-only cohort. Do not add a
product-wide force-family setting or hard-code a dated DNS address merely to
turn either green. Linux Mint remains optional UDP diversity, while Fedora and
openSUSE remain monitor-only.
