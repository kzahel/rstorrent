# Tracker Discovery

Topic: `tracker-discovery`

Status: Tactical `011` completed the first bounded tracker operation: one
loopback-only BEP 15 UDP connect and started announce whose compact peers
enter the existing peer registry. Scheduling, tiers, other tracker transports,
and public-network product support remain unimplemented.

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
- A future **tracker record** will retain URL, tier, failure history,
  connection-token cache, and next eligible announce time independently from
  any one in-flight operation.
- A future **tracker manager** will own tracker records, tier selection,
  scheduling, concurrency, cancellation, and stopped/completed events for one
  torrent.

Tactical `011` deliberately needs only an ordered bounded list of UDP tracker
URLs and one runtime operation at a time. It must not introduce a permanent
manager with guessed scheduling policy before a second announce exists.

## Accepted Direction

Tracker protocol values, binary codecs, and response validation remain
independent from Tokio, DNS, sockets, clocks, and random-number generation.
The runtime supplies transaction IDs and the announce key, resolves URLs,
owns one socket, enforces a deadline, and translates accepted compact
endpoints into `PeerObservation` values with `PeerSource::Tracker`.

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

The first magnet path remains loopback-only. It lazily tries retained tracker
URLs when the selector has no eligible peer, so explicit hints can work
without tracker traffic while a failed hint can still fall through to
tracker discovery. One successful response may add several observations, but
the existing diagnostic still connects to only one peer at a time.

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

## First-Slice Limits And Next Work

Tactical `011` does not cache the connection ID, retransmit UDP requests,
schedule reannounces, emit stopped or completed events, announce a real
listening port, honor tiers, or support HTTP, HTTPS, WebSocket, authentication,
proxying, or BEP 41 URL-data extensions. It asks for a bounded peer set,
reports 16 KiB left when magnet metadata is unknown as libtorrent does, and
owns no incoming peer listener.

The next peer-focused work should first add bounded multi-peer transfer so the
additional observations already returned by a tracker can improve reliability
and eventually throughput. The next tracker-focused tactical should introduce
the real per-torrent tracker record and scheduled lifecycle when reannounce
behavior becomes material. A live public test-torrent run remains useful
manual evidence but must not replace controlled protocol and libtorrent tests.
