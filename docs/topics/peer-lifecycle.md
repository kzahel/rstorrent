# Peer Discovery And Lifecycle

Topic: `peer-lifecycle`

Status: Tactical `010` completed the first bounded peer registry, selection,
dial, failure, and live-connection lifecycle. Tacticals `011` and `014` feed it
bounded scheduled UDP tracker observations. Tactical `013` applies explicit
destination policy and per-operation deadlines to the runtime owner. Tactical
`016` adds DHT observations through the same registry boundary. Multiple
simultaneous peers, torrent-owned requests, and content failover remain
unimplemented and form the next peer-liveness campaign.

## Scope

This topic owns the torrent-engine vocabulary and invariants for peer
observations, accumulated peer records, dial eligibility and selection,
connection attempts, live peer connections, failure history, and bounded
peer-record retention.

It does not own tracker, DHT, PEX, or local-discovery wire protocols; piece
integrity and durable completion; upload policy; product presentation; or
durable application persistence. It owns connection-scoped choke and
availability facts because those determine slot usefulness and request
eligibility. [`download-correctness.md`](download-correctness.md) owns the
corresponding torrent-level request and completion invariants.

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

Live socket, decoder, peer-wire queues, choke state, and advertised
availability belong to one connection generation. Torrent-level block state,
request assignments, payload reservations, storage acceptance, piece
verification, and completion do not belong to a socket. Connection-independent
history remains in `PeerRecord` when that socket closes. Dynamic peer records
are reconstructible engine state and are not part of the initial SQLite
authority; a later bounded good-peer cache may persist selected endpoint
observations explicitly.

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

For the multi-peer campaign, the pinned libtorrent survey begins with
`torrent.cpp`, `peer_connection.cpp`, `bt_peer_connection.cpp`,
`piece_picker.cpp`, `peer_list.cpp`, their corresponding headers, and focused
peer-list, picker, request-queue, timeout, and disconnect tests. These are a
state-transition and edge-case oracle, not an instruction to reproduce
libtorrent's ownership or class graph. JSTorrent remains useful for product
failure history, scripted-swarm patterns, and simpler integration vocabulary.

## Adversarial Development Model

Multi-peer work proceeds from falsifiable failure scenarios rather than from a
broad peer-manager framework. The campaign's central liveness invariant is:

> A torrent continues making progress when peers arrive late, lack pieces,
> choke, stall, disconnect, or return stale data, without exceeding request,
> payload, connection, queue, or task limits.

The implementation establishes only the narrow architectural waist needed to
make those scenarios independently testable:

- one bounded torrent-owned live-connection set and a separately bounded set
  of pending dial attempts;
- stable peer-record, dial-attempt, and connection-generation identities;
- connection-scoped availability, choke, protocol, and socket state;
- torrent-scoped piece/block state and explicit request attempts containing a
  block, connection generation, issuance time, and terminal disposition;
- one global payload allowance and bounded per-peer request windows;
- typed scheduler decisions for dial, request, close, wait-until, verify, and
  complete behavior; and
- supervised connection tasks using bounded commands and events, explicit
  cancellation, and observable joins.

The request-attempt shape may represent more than one attempt for a block so a
later endgame tactical does not require another ownership rewrite. Ordinary
scheduling in the first slice still permits only one active attempt per block.
Do not introduce generic picker, transport, reputation, or policy traits until
a concrete second implementation or measured ownership problem requires them.

## Adversarial Scenario Families

[`download-correctness.md`](download-correctness.md) owns stable scenario IDs
and required completion results. The peer campaign must cover these families:

### Connection capacity and replacement

- all established slots contain interested peers that never unchoke, then a
  useful candidate arrives;
- all established peers lack the final wanted piece, then a peer advertising
  it arrives;
- pending dial slots contain TCP peers that never complete the handshake;
- peers connect, become briefly useful, and repeatedly churn;
- the registry has many eligible candidates while established and pending
  socket limits remain enforced; and
- no alternative candidate exists, so the client retains plausible peers and
  discovery schedules instead of cycling connections pointlessly.

An established socket does not earn an indefinite slot merely by remaining
connected. Under capacity pressure, a peer that remains choked, cannot serve
wanted pieces, or makes no request progress becomes replaceable after a
bounded grace period. Unique wanted-piece availability and accepted useful
payload are retention evidence. Without an eligible alternative, the same
state normally produces a waiting deadline rather than reconnect churn or a
blocked result.

### Request ownership and stale work

- choke, disconnect, request expiry, pause, and shutdown with requests in
  every disposition;
- a peer sends keepalives or unrelated messages while withholding one block;
- an expired request is reassigned and the old connection later sends data;
- an event from a closed generation arrives after that endpoint reconnects;
- storage remains slow while multiple peers offer payload; and
- repeated expiry cannot starve one wanted block forever.

### Availability and late discovery

- wanted pieces are split across peers, including a final piece available
  from only one peer;
- bitfield and later `have` messages change which work is schedulable;
- every current peer lacks wanted work while tracker or DHT discovery remains
  scheduled; and
- a late tracker or DHT observation becomes a useful content connection while
  other peers are already active.

### Fairness and resource pressure

- one fast peer does not evict a uniquely useful peer;
- one slow peer cannot reserve the entire torrent payload budget;
- churn cannot grow tasks, decoders, queues, diagnostics, or request history
  without bound;
- hostile observation volume cannot evict active or uniquely useful peers;
  and
- each scheduler pass performs bounded work with a full registry and
  connection set.

### Integrity-facing transitions

The first peer tactical retains bounded contributor and generation evidence so
a valid late block is harmless and an unsolicited block cannot consume another
request's reservation. Automatic hash retry, contributor reputation, bounded
endgame duplicates, and cancel messages remain the following correctness
slice unless implementation evidence shows that their state shape must be
settled earlier.

## Test And Evidence Strategy

The scenario matrix uses three evidence layers:

1. Runtime-independent scheduler tests use an explicit or virtual clock and
   cover the large transition matrix without Tokio, sockets, storage, or
   sleeps.
2. Scripted loopback swarms exercise representative split availability,
   permanent choke, message-without-payload stall, disconnect, late discovery,
   stale generation, slow storage, cancellation, and exact task cleanup.
3. Controlled libtorrent runs prove that the bounded ordinary multi-peer wire
   path and verified publication remain interoperable.

Every scheduler transition continuously preserves these assertions:

- every active request has exactly one torrent owner, connection generation,
  issuance time, and payload reservation;
- reserved payload equals authoritative outstanding and writing reservations;
- connection, dial, request, queue, task, and history bounds are never
  exceeded;
- stale generations cannot mutate or release current ownership;
- requests target only live unchoked peers advertising the piece;
- storage acceptance and full-piece verification remain the only path to
  durable have state; and
- when an installed mechanism can advance wanted work, the scheduler emits an
  action or a named future deadline.

The first engine slice remains headless. Typed scheduler facts may extend
application snapshots and generated contracts when needed to classify a
stall, but no desktop, web, or Android presentation work is implied.

## Campaign Slicing

The preferred sequence is:

1. **Bounded multi-peer ownership and failover.** Land the live connection
   set, bounded parallel dialing, torrent-owned request attempts, expiry,
   capacity replacement, late discovery, and content completion under the
   initial adversarial matrix.
2. **Endgame and integrity recovery.** Add core cancel messages, bounded
   duplicate attempts, harmless late responses, hash reset/retry, and bounded
   contributor evidence.
3. **Measured picker and connection policy.** Use controlled and paired public
   evidence to tune availability selection, peer retention, connection
   budgets, CPU, memory, and throughput before adding protocol breadth.

Incoming connections, payload upload, PEX, LSD, uTP, NAT traversal, persistent
peer caches, mature peer-ID duplicate resolution, and dynamic VPN or metered
policy remain separate tacticals.

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

Tactical `014` keeps that same boundary while the tracker manager retries and
reannounces. Later observations continue to enter the registry, but the
one-live-connection content path cannot yet use a newly discovered peer while
another content connection remains installed.

Tactical `016` adds a session-owned DHT participant and feeds its results
through `PeerSource::Dht`. Controlled trackerless metadata/content completion
passes, while a public metadata attempt found many peer values without
completing. That evidence strengthens the need for bounded parallel dialing,
useful-peer retention, and connection turnover; it does not justify bypassing
the registry.

Tactical `013` removed the implicit loopback preference and restriction.
Desktop and Android product owners explicitly use `Online`; diagnostic and
controlled runtimes use `LoopbackOnly`. TCP connect, handshake read/write,
complete-message reads across fragmentation, and complete-frame writes now
have bounded deadlines owned by the peer connection. Timely messages can
continue indefinitely because no deadline bounds the torrent's lifetime.

The runtime still deliberately permits only one live connection. It does not
fail over during content transfer, persist peer records, resolve duplicate
peer IDs, perform simultaneous dialing, or own multi-peer request selection.

The next peer-transfer slice is a small bounded live-peer set with explicit
request ownership, request expiry, capacity-pressure replacement, and
content-transfer failover. Incoming advertised-port updates,
mature performance selection, peer-ID duplicate resolution, integrity
reputation, PEX, and persisted peer caches remain later work.
