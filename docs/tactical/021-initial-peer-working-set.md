# Tactical 021: Initial Peer Working Set

Status: Active

Topics: `tracker-discovery`, `peer-lifecycle`,
`performance-and-live-evidence`, `oracle-driven-engine-campaign`

## Motivation And Outcome

Tactical `020` made a capable peer competitive at sustained transfer, but its
clean three-run 50% screen completed only once. The two misses retained just
four or nine content candidates and two connections; no unused candidate was
eligible. Paired libtorrent samples reached the milestone with 16--22 peers.

Build a useful initial peer working set without weakening endpoint validation,
tracker response bounds, peer history, or task cleanup. The tracker owner must
allow bounded startup discovery across not-yet-known tracker records, feed
every accepted response through the existing peer registry, and let the
content owner keep a bounded set of useful or still-evaluable connections.
The outcome is reliable metadata, first-piece, and 50% progress under the
common tracker profile; ordinary request scheduling remains owned by
Tactical `020`.

## Source Dossier

Pinned libtorrent `2.0.13` at `7d7fc38fac61177fa5e02148f791b2f65250b09d`
is the behavioral completeness oracle. No source or fixture is copied.

- `src/magnet_uri.cpp::parse_magnet_uri` assigns successive magnet `tr`
  parameters distinct tiers.
- `src/torrent.cpp::announce_with_tracker` starts an endpoint's selected tier
  at unknown. With the default `announce_to_all_trackers=false` and
  `announce_to_all_tiers=false`, it still queues not-yet-working tiers during
  the initial pass; a known working tracker constrains later passes.
- `torrent.cpp::tracker_response` independently records each response, adds
  every bounded compact peer through the peer-list owner, updates future
  announce state, and invokes `do_connect_boost`.
- `torrent.cpp::do_connect_boost` immediately attempts up to the remaining
  startup quota; the pinned default `torrent_connect_boost` is 30 and the
  session connection limit is 200.
- `src/peer_list.cpp::find_connect_candidates` examines a bounded rotating
  subset, prefers better candidates, observes reconnect time and fail count,
  and does not let one failed address block other records.
- Pinned defaults request 200 peers per tracker, use a 15-second peer-connect
  timeout, allow three ordinary peer failures, and use a 60-second reconnect
  base. RSTorrent already matches those broad bounds except for its smaller
  deliberate per-torrent connection set.

RSTorrent initially shuffled all magnet UDP trackers into one synthetic tier,
ran one UDP operation at a time, and ended the entire round on the first valid
response. That response scheduled at least five minutes of sleep, even when it
contained only a few candidates. After tracker fan-out landed, its live-peer
limit still counted half-open dials against eight established slots. This
tactical preserves the documented synthetic-tier compatibility choice while
correcting both startup operation breadth and the resulting content working
set.

## Ownership And Design

`TrackerSchedule` remains runtime-independent and owns record eligibility,
failure, success, promotion, and future announce times. The async tracker
manager owns a bounded set of in-flight UDP operations. It asks the schedule
for startup work until the operation ceiling or current round boundary is
reached, then serializes every completion back through the schedule.

At most eight tracker operations may be in flight for one torrent. This is
enough to cover the five retained Big Buck Bunny UDP trackers without adopting
libtorrent's session-wide limits, and remains bounded below the parser's
32-tracker input ceiling. Each operation temporarily owns the token cache for
its tracker record and returns it to the manager, preserving 60-second BEP 15
connection-token reuse without shared mutable socket state.

Every operation that began before a success may finish and contribute peers.
A success constrains new scheduling according to the existing promoted
working-tracker policy; it does not cancel already-valid operations or discard
their later results. Failure of one operation advances unattempted startup
records while no success has established the round wait. Cancellation aborts
and joins every operation before the manager terminates.

The peer registry remains the sole peer-record owner. The content supervisor
may own at most eight half-open dials in addition to 30 established
connections, replacing only peers already classified replaceable by its
deterministic state. Pending handshakes and live connections are distinct
resource pools: a pending attempt must not consume an established slot, while
a late successful attempt that finds the live set full is closed unless a
deterministic replacement exists. The 30-peer bound matches the pinned
startup-connect quota instead of adopting libtorrent's unlimited per-torrent
default or its 200-connection session budget. A tracker result never proves
reachability or usefulness.

## Invariants And Bounds

- At most eight UDP tracker operations, four queued tracker result batches,
  200 compact peers per response, 1,000 peer records, eight pending dials, and
  30 established peers exist under current defaults.
- Pending dials never reserve request payload. Established peers share the
  existing torrent-wide payload allowance, bounded command/event queues, and
  per-connection request limit; increasing the peer set does not multiply the
  payload ceiling.
- Each tracker record has at most one operation in flight and one returned
  token cache; stale task results cannot mutate another record.
- Source, transaction, action, stride, endpoint, address-family, and network
  policy validation remains unchanged for every operation.
- A valid zero-peer response is still protocol success, but does not erase
  peers from other concurrent valid responses.
- Failure, cancellation, receiver closure, pause, and shutdown join every
  manager-owned operation and release every UDP socket.
- Tracker events and public diagnostics retain aggregate counts only; peer
  endpoints do not enter retained benchmark reports.

## Adversarial Validation

- A barrier-held set of scripted trackers proves multiple connect exchanges
  begin before any tracker is allowed to answer; sequential execution must
  fail this test.
- Concurrent valid responses with disjoint and duplicate peers merge through
  the registry exactly once and all response batches remain observable.
- One success, zero-peer success, malformed correlation, timeout, and tracker
  error cannot suppress already-started healthy operations.
- More trackers than the operation ceiling never exceed it and continue from
  failures without duplicate record operations.
- Cancellation at connect, announce, bounded result-send, and scheduled-wait
  phases joins all operation tasks and permits rebinding their client sockets.
- Existing retransmission, token reuse/expiry, fallback, interval, metadata,
  late-discovery content, mixed-peer, and controlled libtorrent gates remain
  green.

## Live And Comparator Gates

Record tracker response-batch count, total reported peers, peer-dial attempts,
the current content-registry classification, and a bounded endpoint-free row
for each established peer in the headless probe. Mirror the useful libtorrent
peer-table fields: choke and wanted-piece state, queue length and bytes,
target, window phase, useful bytes, current sampled rate, connection and
payload ages, adaptive timeout, and oldest request age. Do not retain tracker
or peer addresses.

After deterministic gates:

1. run three RSTorrent-only common-profile Big Buck Bunny attempts to 50%;
2. require at least 2/3 success before an alternating paired screen;
3. run an independent paired cohort only after that screen passes; and
4. retain correct identity, hashes, cleanup, candidate breadth, connection
   counts, and milestone timing rather than interpreting speed alone.

The comparable claim still requires two independent 10-run cohorts with at
least 8/10 RSTorrent successes and median paired latency no worse than 2.0x
libtorrent. A changing public swarm cannot be a deterministic CI gate.

## Implementation Evidence

The first checkpoint adds explicit `updating` state to each pure tracker
record. Selecting an announce enters that state; success and failure leave it;
and the schedule returns `Pending` instead of wrapping a round while an
operation is unresolved. This makes concurrent runtime ownership visible to
the deterministic schedule without introducing Tokio or task handles there.

The async manager now owns a `JoinSet` of at most eight operations. It fills
the startup window from schedule actions, serializes completions back through
the pure state owner, and permits every already-started response to contribute
peers after another tracker succeeds. Each task temporarily owns only its
record's BEP 15 token cache and returns it with the result, so different
trackers share no socket state and reannounce token reuse remains intact.
Cancellation, result-receiver closure, and a task join failure abort and join
the remaining set before the manager returns.

Adversarial loopback tests prove three trackers begin before a response barrier
opens, three disjoint response batches merge through the peer registry, eight
silent operations hold the exact ceiling, a ninth begins only after a failed
operation frees capacity, and cancellation of three silent operations permits
immediate rebinding of every client socket. A pure test proves an unresolved
round cannot reselect the same tracker. Existing tracker schedule,
retransmission, token, metadata, content-discovery, and mixed-peer tests remain
green.

The public probe now reports saturating endpoint-free totals for successful
tracker response batches, peers reported by those batches, and peer dial
attempts. A unit test proves accumulation without retaining the event's
tracker or peer strings. The independent controlled UDP tracker fixture was
also corrected to assert the already-accepted provisional port `6881` instead
of the obsolete port-zero behavior; its three complete metadata/content runs
then passed.

At this checkpoint, workspace formatting and warning-denying clippy pass. The
workspace has 225 passing tests, three changing public-network tests ignored,
and no failures; the engine library contributes 114 of those passes. The
three-run UDP tracker interop, mixed-peer liveness scenario, controlled paired
publication, and all seven comparator unit tests also pass.

The first clean live 50% screen after fan-out completed 0/3 within 180 seconds.
It nevertheless proved the tracker change effective: every run received two
response batches, retained 14--15 candidates, attempted 17--19 dials, and
ended with five or six established peers rather than the preceding two. The
terminal registries still had two to five eligible candidates, while the
established plus pending counts exactly equaled the old eight-slot limit. All
three runs were receiving 3.2--4.0 MiB/s near termination and had verified 36,
135, or 186 of 1,055 pieces. This classifies admission, not tracker breadth or
healthy-peer request rate, as the next owner.

Pinned source confirms the mismatch: libtorrent defaults a torrent to
unlimited connections under a 200-connection session cap, starts up to 30
connection attempts immediately after a tracker response, and otherwise
permits 30 attempts per second. RSTorrent now keeps an explicit smaller bound
of 30 established peers plus eight half-open attempts. A deterministic truth
table proves an in-flight handshake cannot consume a live slot, the pending
ceiling is exact, and replacement probing at a full live set remains limited
to one attempt.

The clean post-admission screen at commit `5bc4719` still completed 0/3 at 50%
within 180 seconds, with exact cleanup. It verified 25, 55, and 96 pieces.
Every run again received two tracker batches and retained 14--15 content
candidates, but the artificial combined ceiling was gone: five or six peers
were established, six to eight were dialing, zero remained eligible, and up
to three were backed off. Four or five established peers were unchoked.
Terminal request targets totaled 592--715, with one peer reaching 360 or 500;
7.6--26.2 MiB had been useful while 479--710 requests remained outstanding.

This disproves larger admission as a sufficient fix and exhausts the current
tracker candidate population. The aggregate 3.0--4.1 MiB/s sampled rate is not
consistent with the cohort's 40--140 KiB/s wall-clock average, so it cannot
identify whether one recently fast peer hoards the queue, useful peers arrive
late, or multiple slow-start rows dominate. Pinned libtorrent exposes queue
length, target queue length, current payload rate, queue time, snubbed state,
and request timeout per peer. RSTorrent now captures the endpoint-free bounded
equivalent for at most 30 established peers. A pure test proves wanted-piece,
choke, queue, target, payload, phase, age, and timeout accounting for both a
useful and irrelevant connection. The controlled paired publication remains
`both_reached`; its completed peer row reports exact 79,000 useful bytes, no
pending queue, and no retained endpoint. A single classified live sample is
next; another policy change is not justified from the aggregate alone.

## Non-Goals

- HTTP, HTTPS, WebSocket trackers, BEP 12 metainfo tiers, announce-all user
  settings, tracker persistence, proxying, or session-wide budgets
- DHT, PEX, LSD, incoming connections, NAT traversal, uTP, upload, or seeding
- a session-wide connection allocator beyond the source-derived bounded
  per-torrent working set
- request-window, piece-picker, endgame, cancel, hash-failure, storage, UI,
  desktop-launch, or visible-device work

## Validation And Stopping Condition

Run focused tracker schedule/manager and peer-registry tests, controlled UDP
tracker and libtorrent interoperability, mixed-peer liveness, formatting,
warning-denying workspace clippy, and workspace tests. All live work remains
headless and cleans temporary payloads and reports after aggregate evidence is
recorded.

This tactical is complete when bounded initial tracker breadth and exact task
cleanup pass the adversarial gates, the 2/3 live screen passes or retains a new
classified boundary not owned here, living topics record actual evidence, and
the next narrow owner is decision-complete. No human decision is currently
required.
