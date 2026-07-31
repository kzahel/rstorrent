# Peer Discovery And Lifecycle

Topic: `peer-lifecycle`

Status: Tactical `010` completed the first bounded peer registry, selection,
dial, failure, and live-connection lifecycle. Tactical `011` now feeds it
bounded one-shot UDP tracker observations. Tactical `013` applies explicit
destination policy and per-operation deadlines to the runtime owner. Multiple
simultaneous peers remain unimplemented.

## Scope

This topic owns the torrent-engine vocabulary and invariants for peer
observations, accumulated peer records, dial eligibility and selection,
connection attempts, live peer connections, failure history, and bounded
peer-record retention.

It does not own tracker, DHT, PEX, or local-discovery wire protocols; choking,
piece selection, and upload policy; product presentation; or durable
application persistence. Those capabilities produce or consume the peer
lifecycle described here.

## Vocabulary

RSTorrent uses these terms:

- A **peer endpoint** is one validated IP address and listening port that may
  be dialed.
- A **peer observation** is bounded, untrusted evidence from one discovery
  source about an endpoint. An observation is not a live peer.
- A **peer record** is the torrent-scoped accumulation of observations,
  reachability, learned facts, dial state, and history for one endpoint.
- The **peer registry** is the bounded owner of peer records.
- **Dial eligibility** is a derived answer for one record under current time,
  torrent state, and policy. It is not stored as an independently synchronized
  candidate collection.
- A **dial candidate** is a temporary selector result identifying an eligible
  record and endpoint.
- A **dial attempt** is one uniquely identified transition from an eligible
  record into in-progress connection work.
- A **peer connection** is a live socket plus peer-wire state associated with
  exactly one successful dial attempt.
- The **swarm** is the eventual complete per-torrent peer subsystem: registry,
  selector, attempts, and live connections. A peer-record map alone is not the
  whole swarm.

The protocol peer ID learned during the handshake is peer identity evidence,
not the primary key for a discovered endpoint. Multiple endpoints may later
report the same peer ID, and endpoint duplicate policy remains distinct from
identity grouping.

## Accepted Direction

Discovery protocols translate their results into `PeerObservation` values.
Only the registry merges those observations into engine state. Trackers,
magnet hints, DHT, PEX, incoming connections, cache restore, and manual input
must not grow independent address lists or dial sockets directly.

Repeated observations accumulate a set of sources rather than preserving only
the first source. A later observation may strengthen reachability, such as
when an endpoint first seen on an incoming connection is subsequently
advertised with a listening port. Source-specific trust changes require
explicit policy; an untrusted hint does not become a confirmed fact merely
because it was repeated.

Dial eligibility is derived at selection time. At minimum, a record is not
eligible while non-connectable, dialing, connected, banned, within reconnect
backoff, or at its configured failure ceiling. Selection is deterministic
when policy inputs are equal so controlled tests can explain why one endpoint
was chosen.

Every dial attempt carries a generation identity. Success, failure, and close
transitions must match that identity so a stale asynchronous completion cannot
mutate a newer connection lifecycle. Tactical `010` exercises one attempt at
a time, but the state model must not require replacing these identities when
parallel dialing arrives.

The registry has a per-torrent capacity from its introduction. It never evicts
a dialing or connected record merely to accept hostile discovery input.
Pruning policy prefers unusable and repeatedly failing idle records before
healthy records. Bans must not be silently forgotten by ordinary capacity
pressure.

Live socket, decoder, request, availability, and transfer state belongs to
`PeerConnection`. Connection-independent history remains in `PeerRecord` when
that socket closes. Dynamic peer records are reconstructible engine state and
are not part of the initial SQLite authority; a later bounded good-peer cache
may persist selected endpoint observations explicitly.

Destination permission is runtime infrastructure rather than peer-record
truth. Observations enter the registry only when allowed by the configured
policy, and the runtime checks again immediately before dialing. `Online`
permits otherwise valid routed unicast endpoints, `LoopbackOnly` isolates
controlled tools, and `Offline` prevents network work. Changing a future
session policy must close active network resources without pretending their
peer records failed.

## Reference Direction

Rasterbar libtorrent `v2.0.13` supplies the mature behavioral reference:
`torrent_peer` is a connection-independent endpoint record, `peer_list` owns
bounded retention and derived connect-candidate policy, discovery sources
accumulate as flags, incoming ephemeral ports remain non-connectable, and
hinted seed state remains separate from confirmed connection facts.

Current JSTorrent provides clearer decomposition vocabulary through
`PeerAddress`, `SwarmPeer`, `Swarm`, `PeerSelector`, `ConnectionManager`, and
`PeerConnection`, but its current first-source-wins and unbounded record-map
behavior are not RSTorrent requirements.

RSTorrent independently implements the public behavior and does not copy
reference source, layouts, raw-pointer ownership, bit packing, per-IP
deduplication defaults, or candidate-cache optimizations.

## Current Evidence And Gaps

Tactical `006` proved direct `x.pe` resolution, BEP 9 metadata transfer, and
same-socket content handoff. Tactical `010` replaced the remaining ad hoc
address loops with one runtime-independent registry and one diagnostic
runtime owner. Both manual explicit peers and resolved magnet hints now enter
the same observation, selection, attempt, and connection lifecycle.

Deterministic state evidence covers source accumulation, strengthened
reachability, capacity and pruning, active and banned retention, eligibility
and reconnect backoff, successful and failed transitions, stale callbacks,
and checked counter exhaustion. Scripted engine evidence proves failover from
both an unreachable endpoint and an extension-incapable connected endpoint
before the next peer completes verified metadata and content. The controlled
libtorrent `2.0.13.0` metadata/content exchange remains green.

Tactical `011` proved the intended discovery boundary: compact results from a
bounded one-shot UDP tracker announce enter as tracker observations, merge and
deduplicate in the registry, and follow the existing dial and failure
lifecycle. A tracker rejection advances to another tracker; an unreachable
tracker peer advances to the next record; and a successful explicit hint
avoids tracker traffic.

Tactical `013` removed the implicit loopback preference and restriction.
Desktop and Android product owners explicitly use `Online`; diagnostic and
controlled runtimes use `LoopbackOnly`. TCP connect, handshake read/write,
complete-message reads across fragmentation, and complete-frame writes now
have bounded deadlines owned by the peer connection. Timely messages can
continue indefinitely because no deadline bounds the torrent's lifetime.

The runtime still deliberately permits only one live connection. It does not
fail over during content transfer, persist peer records, resolve duplicate
peer IDs, perform simultaneous dialing, or own multi-peer request selection.

The next recommended peer slice is a small bounded live-peer set with explicit
request ownership and content-transfer failover. Reannounce scheduling,
incoming advertised-port updates, mature performance selection, peer-ID
duplicate resolution, integrity reputation, PEX, DHT, and persisted peer
caches remain later work.
