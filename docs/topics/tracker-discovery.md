# Tracker Discovery

Topic: `tracker-discovery`

Status: Tactical `014` replaced the first one-shot operation with a supervised
scheduled UDP tracker lifecycle. Tactical `021` added bounded concurrent
startup operations and classified the remaining live failure at content-peer
admission rather than tracker intake. Tactical `043` makes the deterministic
schedule's retained lifecycle the authoritative inspectable state and proves
it through the live browser surface. Other transports and metainfo tracker
tiers remain unimplemented. Planned Tactical `081` owns the first persisted
BEP 12 metainfo tiers and source attribution while retaining HTTP/HTTPS
trackers as truthfully unsupported configuration rather than implementing
those transports.

## Scope

This topic owns tracker URL values, announce inputs and results, tracker
operation lifecycle, retry and scheduling direction, tier policy, tracker
failure history, and the boundary from tracker results into peer
observations.

It does not own peer selection or connections, DHT, PEX, local discovery,
content scheduling, or application presentation. Trackers discover endpoints;
the peer registry remains the only owner of accumulated peer records.

## Vocabulary And Ownership

- A **tracker URL** is a bounded validated description of one tracker
  transport endpoint. It is not a peer endpoint.
- A **tracker announce** is one identified operation carrying torrent
  identity, client identity, transfer counters, event, listening port, key,
  and requested peer count.
- A **tracker response** is untrusted interval, swarm-count, and compact-peer
  data correlated to one announce transaction.
- A **tracker record** retains URL, synthetic tier, source, failure history,
  announce state, interval, and next eligible monotonic time independently
  from any one in-flight operation.
- A **tracker manager** owns tracker records, selection, one in-flight UDP
  operation, connection-token caching, retry timers, cancellation, and a
  bounded result channel for one active torrent.

## Accepted Direction

Tracker protocol values, binary codecs, and response validation remain
independent from Tokio, DNS, sockets, clocks, and random-number generation.
The runtime supplies transaction IDs and the announce key, resolves URLs,
owns one socket, enforces a deadline, and translates accepted compact
endpoints into `PeerObservation` values with `PeerSource::Tracker`.

Tracker failure and tracker exhaustion are mechanism outcomes, not necessarily
torrent errors. Application progress assessment must combine tracker status
with peer hints, scheduled retries, and other installed discovery mechanisms.
It may report externally blocked discovery only when none can still act
automatically. Bounded typed tracker events explain attempts and outcomes
without making formatted tracker log text application state.

Peer endpoints from a tracker are untrusted hints. Invalid endpoints are
discarded, duplicates merge through the registry, and a tracker does not
confirm reachability, peer identity, seed status, or integrity merely by
reporting an address.

UDP response correlation requires the expected remote endpoint, transaction
ID, action, minimum length, address-family stride, and peer-count bound.
Unrelated or stale transaction IDs are ignored within the operation deadline;
a malformed packet correlated to the active transaction fails that tracker
operation. Bounded tracker error text may be diagnostic context but never
application state or an allocation authority.

The magnet path starts its tracker manager after bounded peer-hint resolution
and before peer selection or dialing, then keeps it alive while metadata or
content work is active. Runtime policy is checked before DNS when offline,
after tracker resolution, on every compact peer observation, and again before
peer dialing. One successful response may add several observations; the
torrent supervisor can dial them while other content peers remain active
under its connection and pending-work bounds. The parent explicitly cancels
and joins the manager on completion, failure, pause, or shutdown.

Magnet `tr` parameters do not encode BEP 12 tier structure, so retained UDP
trackers form one initially shuffled synthetic tier. Failure falls through to
another eligible record in the same round. After all records fail, the
manager waits for the earliest retry; each record remains eligible
indefinitely under the libtorrent-style quadratic delay
`5 + 12.5 * failures²` seconds, capped at 60 minutes. A valid response,
including a zero-peer response, ends the round, resets that record's failure
count, promotes it, and schedules an ordinary announce from its interval
clamped to five minutes through 24 hours.

Each UDP connect or announce exchange sends immediately, retransmits once
after 15 seconds of silence, and completes after an aggregate 30-second
deadline. Valid connection IDs are cached per remote endpoint for 60 seconds
in a bounded cache. A started event is repeated until acknowledged; later
successful announces use the ordinary event.

## Reference Direction

BEP 15 is normative for the connect and announce packet shapes, network byte
order, transaction correlation, compact IPv4/IPv6 response formats,
connection-token lifetime, and retransmission guidance.

Rasterbar libtorrent `v2.0.13` is the mature behavioral reference. It keeps
URL resolution and sockets outside the codec, rejects unexpected source,
transaction, action, and response stride, tries alternate resolved tracker
addresses, caches connection IDs for 60 seconds, and emits tracker results
through the ordinary peer-list path.

Current JSTorrent provides useful `TrackerManager`, `UdpTracker`, announce
statistics, and `peersDiscovered` vocabulary plus a practical local UDP
tracker fixture. Its current single transaction field, IPv4-only compact
parser, short-packet handling, and missing source check are not RSTorrent
requirements.

No reference source or fixture is copied. RSTorrent independently implements
the public wire behavior and constructs its own deterministic and controlled
interoperability evidence.

## Current Evidence

Tactical `011` established bounded tracker URL retention in both parsed and
SQLite-canonicalized magnets, pure connect/announce codecs, lazy runtime
operation ownership, and the tracker-observation boundary. Deterministic tests
cover URL and packet limits, two-tracker protocol failover, stale and
undersized datagrams, invalid and duplicate compact endpoints, peer dial
failover, explicit-hint precedence, and socket release on timeout and
cancellation.

Three controlled libtorrent `2.0.13.0` runs acquired a 26,686-byte,
two-block info dictionary and every hash-verified content piece from a
tracker-only magnet. The independent Python tracker observed exactly one
connect and one announce per run and all processes and artifacts terminated
cleanly. Android API-28 cross-builds passed for x86_64 and arm64-v8a; this
tactical did not claim a public tracker run or on-device networking evidence.

Tactical `013` retained the controlled loopback exchange while making the
policy choice explicit. Desktop and Android product adapters select `Online`;
engine/session diagnostics and the authenticated browser gateway select
`LoopbackOnly`. An opt-in Big Buck Bunny metadata probe reached a public UDP
tracker operation under online policy, then timed out waiting for that
tracker's connect response. This is evidence that policy no longer rejects
the public route, not evidence of a reachable public swarm.

Tactical `014` adds deterministic schedule tests for fallback, promotion,
quadratic and saturated retry delays, bounded success intervals, and correct
earliest-retry selection. Scripted UDP tests cover dropped connect and
announce requests, retransmission, token reuse and expiry, started-to-ordinary
events, zero-peer success, and cancellation with socket release. Three
controlled libtorrent `2.0.13.0` runs still acquired verified metadata and all
content from a tracker-only magnet with exactly one connect and one announce
per run.

The application now emits typed tracker attempt, retransmit, failure,
fallback, retry, reannounce, success, unusable-response, and peer-dial
diagnostics. Failure retry and successful reannounce are distinct facts. A
retained automatic action with no eligible peer projects as
`waiting/discovery/waiting_for_discovery`, not blocked. Headless Chrome over
the loopback gateway and an owned API 34 arm64 no-window AVD rendered the same
assessment and tracker-filtered timeline. The Android run also passed Activity
recreation/backgrounding and joined foreground shutdown.

A Tactical `018` tracker-only Big Buck Bunny rerun retained the complete
90-second timeline. It discovered no peers: two trackers timed out during UDP
connect, one hostname no longer resolved, and two trackers rejected the
announce because RSTorrent reported port zero. No dial, peer connection, or
BEP 9 request occurred. A follow-up made port `6881` an explicit provisional
announce input. The same headless smoke then received six tracker candidates
within 0.36 seconds and acquired hash-verified metadata in 11.41 seconds.
Ten immediate tracker-only repetitions completed 8/10 within the 90-second
bound. Successful acquisition had a 32.77-second median and 38.41-second mean,
with a 1.71–75.51-second range. Candidate counts ranged from 6 to 131 and did
not predict completion latency. Both timeouts had discovered and attempted six
peers, so they were not tracker-silence failures.

Pinned libtorrent `2.0.13.0` then completed the same metadata-only tracker
scenario 10/10 with a 20.94-second median and a narrow 20.81–21.49-second
range. Its alert timeline spent about 10 seconds apiece timing out against the
first two listed trackers, received 71 peers from the third, and verified
metadata 0.11 seconds later. This reference kept libtorrent's ordinary peer
concurrency while disabling DHT, LSD, PEX, incoming peers, uTP, and NAT
mapping; it is not yet an alternated paired result.

Port `6881` is a compatibility placeholder, not a reachability claim. Tactical
`078` later added one independently configured IPv4 loopback listener, but
tracker announces do not consume its actual port and no NAT mapping is
requested. DHT therefore continues to omit `announce_peer` until advertised
port ownership and reachability policy are implemented.

Tactical `020` then showed that a capable peer can reach Big Buck Bunny's 50%
milestone in 24--28 seconds, but the clean post-stall screen completed only
1/3. Its two misses retained only four or nine current tracker candidates and
two connections. The same paired libtorrent profile reached 50% with 16--22
peers. A renewed pinned-source audit found an omitted startup behavior:
libtorrent assigns magnet trackers distinct tiers, queues every
not-yet-working tier in the initial announce pass even with both announce-all
settings disabled, accepts every already-started reply, and invokes a bounded
30-peer connection boost. RSTorrent instead runs one operation and sleeps for
at least five minutes after the first valid response. Tactical `021` owns a
bounded initial fan-out while preserving RSTorrent's documented synthetic
tier and later promoted-tracker policy.

The first Tactical `021` checkpoint installs that bounded fan-out. Pure
tracker records now explicitly distinguish an in-flight update, and one
manager owns up to eight operations with per-record token caches and joined
cancellation. Scripted barriers prove true concurrency, the exact ceiling,
failure-driven admission beyond the ceiling, multi-response peer intake, and
socket release. Endpoint-free probe totals expose response batches, reported
peers, and dial attempts. The first clean live screen received two response
batches and retained 14--15 candidates in every run, versus four to nine
before fan-out. Its 0/3 50% result stopped at the downstream combined
live-plus-pending connection ceiling; tracker startup is no longer the
classified owner.

Tactical `043` extends those same pure tracker records instead of creating a
UI-side tracker authority. Immutable snapshots expose announcing, retry,
reannounce, and inactive lifecycle plus attempts, consecutive failures,
accepted interval and swarm counts, monotonic outcome ages, the next scheduled
action, and bounded error context. The runtime publishes typed snapshots after
every schedule transition and only publishes terminal inactive state after
its UDP operations have been aborted and joined. Diagnostics remain an
independent ordered observation stream.

The leased application view retains at most the schedule's existing 32
tracker records. A durable magnet can reconstruct inactive configured rows
after restart, but volatile response and deadline history is deliberately not
persisted. A controlled delayed loopback announce let the live browser observe
the pre-response `announcing` state and then exact response values of one peer,
37 seeds, 11 leeches, and a 30-minute reannounce interval while libtorrent
seeded hash-verified content. This is tracker state and interoperability
evidence, not a claim that a response peer count is a cumulative unique-peer
count or that any returned endpoint is reachable.

## Current Limits And Next Work

The manager has volatile state and an eight-operation per-torrent ceiling. It
does not parse `.torrent` `announce-list` tiers, support HTTP, HTTPS, WebSocket,
authentication, proxying, or BEP 41 URL-data, emit completed/stopped events,
announce real transfer counters or an actually bound listening port, scrape,
or share a session-wide tracker-operation budget. It reports 16 KiB left
while magnet metadata is unknown and does not consume the application-owned
loopback listener's actual port. Until truthful listener advertisement exists,
scheduled tracker announces explicitly carry the conventional port `6881` so
trackers that reject port zero can still return endpoints for outbound
dialing. The peer ID now matches the application lifetime's peer-handshake
identity, but that identity consistency does not make the port reachable.

The DHT owner is a separately owned source using the same peer-observation
boundary and session network policy. Tactical `017` now lets later tracker and
DHT observations improve active-transfer reliability. Later tracker work
should focus on transfer accounting, metainfo tiers, persistence, and
session-wide resource policy. Multi-peer upload, actual-port advertisement,
and NAT traversal are separate later work. The headless public-torrent
comparator adds useful live evidence but cannot replace controlled protocol
and libtorrent tests.

Tactical `081` is the accepted bounded exception to the first gap: it parses
and persists up to 32 valid unique `announce-list`/`announce` URLs, preserves
compact tier grouping and metainfo source, feeds only UDP rows into the
existing manager, and projects retained HTTP/HTTPS rows as unsupported. It
does not implement their wire protocols or broaden tracker authentication.
