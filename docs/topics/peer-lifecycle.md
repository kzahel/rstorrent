# Peer Discovery And Lifecycle

Topic: `peer-lifecycle`

Status: Tactical `017` completed bounded simultaneous dialing, metadata
acquisition, live content connections, torrent-owned requests, expiry,
replacement, and failover. Tactical `020` completed bounded per-connection
useful-payload feedback and sampled inactivity. Tactical `021` installed
bounded tracker fan-out plus a source-derived 30-peer live set. Tactical `022`
removed the classified duplex command/event backpressure deadlock and passed
3/3 owner-only plus 3/3 paired 50% screens. Tactical `023` now owns strict
endgame duplicate-attempt lifecycle and cancellation. Tracker and DHT
observations remain live while content runs. Integrity reputation, measured
picker policy, incoming connections, and persistent peer records remain later
work.

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

Tactical `023` now uses the request-attempt shape to retain multiple bounded
owners for a block during strict endgame. Ordinary scheduling still permits
only one active attempt until every missing block is covered. First response
wins, loser cancels are typed, and every attempt remains charged to the global
payload allowance. Do not introduce generic picker, transport, reputation, or
policy traits until a concrete second implementation or measured ownership
problem requires them.

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
policy remain separate tacticals. Incoming listening, NAT-PMP/UPnP, and
seeding are deliberately lower priority than correct outbound downloading;
the provisional tracker announce port does not change that ordering.

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
reannounces. Tactical `017` now admits those later observations into bounded
parallel dials while existing content connections remain installed.

Tactical `016` adds a session-owned DHT participant and feeds its results
through `PeerSource::Dht`. Controlled trackerless metadata/content completion
passes. Tactical `018` made every registry and metadata-attempt disposition
inspectable, then completed two public trackerless Big Buck Bunny metadata
runs from 93 and 100 candidates after 9 and 12 attempts. Failures were ordinary
connect, handshake, and extension-phase failures; one peer advertised
`ut_metadata`, supplied both blocks, and won while remaining dials were
canceled. The registry remains the authority rather than a diagnostic address
loop.

A subsequent ten-run tracker-only and ten-run trackerless-DHT cohort completed
8/10 and 7/10 respectively. The two tracker timeouts each received two full
16 KiB metadata blocks across four requests but no verified dictionary. The
three DHT timeouts discovered 29–83 candidates and attempted 23–38 peers but
sent zero metadata requests. Tactical `019` responds by replacing independent
per-peer dictionaries with one torrent-owned block table. It retains accepted
sources and request history, reassigns stalled or disconnected blocks, and can
complete from disjoint peer contributions. Public cohorts still need to
separate assembly ownership from candidate and negotiation quality.

Tactical `013` removed the implicit loopback preference and restriction.
Desktop and Android product owners explicitly use `Online`; diagnostic and
controlled runtimes use `LoopbackOnly`. TCP connect, handshake read/write,
complete-message reads across fragmentation, and complete-frame writes now
have bounded deadlines owned by the peer connection. Timely messages can
continue indefinitely because no deadline bounds the torrent's lifetime.

Tactical `017` installs up to eight live connections, three pending dials,
four requests per peer, four active pieces, bounded terminal history, and one
torrent supervisor that owns storage and task joins. Controlled scenarios
prove split availability, disconnect/choke reassignment, request expiry,
harmless late payload, late discovery, handshake silence, full-slot
replacement, no-alternative waiting, and cancellation. Pinned libtorrent
participates in a verified 16-piece completion while a scripted connected
peer remains choked.

Metadata acquisition now keeps up to eight dial/negotiation work items in
flight, continues tracker or DHT discovery, and gives them one bounded
torrent-owned BEP 9 coordinator. Each peer receives at most two requests;
the first is immediate and the second follows a response or a one-second ramp.
Ordinary assignments are unique for three seconds, after which untried peers
are preferred. Disjoint peers can complete one dictionary, disconnect/reject
releases work, and a hash-invalid generation resets with contributor
attribution. The worker delivering the final hash-valid block is preserved for
content while losing work is cooperatively canceled and joined.

Tactical `019` met the public metadata gates. Two independent Big Buck Bunny
tracker cohorts each completed 9/10 for RSTorrent versus 10/10 for libtorrent;
successful RSTorrent medians were 5.72 and 4.12 seconds versus 20.52 and 20.33
seconds, with paired p90 ratios below 1.6x. Ten isolated fresh-DHT RSTorrent
runs completed 10/10; the locked reference produced no torrent candidates in
three contemporaneous attempts despite a populated DHT routing table, so
paired DHT latency remains externally blocked.

The first four-torrent breadth matrix exposed peers that ignored an immediately
pipelined second metadata request. Endpoint-free attempt diagnostics and
`ut_metadata.cpp::maybe_send_request` identified libtorrent's one-request-per-
event/tick cadence. After adding the deterministic request ramp, Cosmos,
Sintel, Tears of Steel, and WIRED CD all completed 3/3 for both owners. Each
RSTorrent run used two requests for two accepted blocks without a hash or
cleanup failure.

`DownloadControl::diagnostic_snapshot` now exposes a bounded read-only peer
registry table and active/recent metadata attempts. Initial BEP 10 handshakes
that omit `ut_metadata` release their slot, metadata rejection is counted, and
unrelated messages cannot extend the independent metadata-progress deadline.
The snapshot is engine diagnostics; product UI projection remains separate.

Tactical `020` installed adaptive per-connection request windows and measured
stall deadlines. A capable public peer now reaches the 50% reference range,
but a clean screen completed only 1/3 while the misses retained four or nine
current candidates and two connections. Tactical `021` therefore owns the
preceding working-set boundary: bounded initial tracker-operation breadth,
multi-response registry intake, and prompt bounded dialing. The registry and
content supervisor remain the owners of candidate history, connection limits,
replacement, and cleanup; tracker success does not prove peer usefulness.

Tracker fan-out then expanded the clean public runs to 14--15 candidates and
five or six live connections, but completed 0/3 at 50%. Each terminal state
had established plus half-open counts equal to the old eight-slot ceiling and
still retained two to five eligible candidates. Pinned libtorrent defaults an
individual torrent to unlimited connections beneath a 200-session cap and
uses a 30-attempt startup boost. RSTorrent now separately bounds eight
half-open attempts and 30 established peers; a pending handshake no longer
occupies a live slot, and late successes at the live ceiling still require a
classified replacement.

The next clean screen remained 0/3 and exhausted every current candidate, so
the larger cap is not sufficient. Four or five of the five or six established
peers were unchoked, yet one peer target reached 360 or 500 and hundreds of
requests remained outstanding. The aggregate sampled rate was incompatible
with wall-clock useful bytes. Following libtorrent's `peer_info` diagnostic
shape, the engine snapshot now retains one endpoint-free row per bounded live
connection with choke, availability, request queue, target, payload, rate,
phase, age, and timeout facts. This observability is separate from policy and
will select the next deterministic owner.

The first clean row-bearing sample did so: the table stopped refreshing after
a one-to-two-second-old state in which one peer had delivered 6.24 MiB and
held 383 requests. The supervisor awaits all commands through a 16-entry
channel before consuming the 64-entry event channel, while the peer task
awaits event delivery before draining more commands. Tactical `022` owns this
duplex cycle; request-window policy is unchanged until transport progress is
restored.

Tactical `022` removed the duplex cycle. Tactical `023` has installed strict
endgame attempt ownership and core cancellation through deterministic and
controlled gates; its public publication gate remains pending. Incoming
listener ownership and advertised-port updates, measured performance
selection, peer-ID duplicate resolution, integrity reputation, PEX, and
persisted peer caches remain later work.
