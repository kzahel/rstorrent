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

RSTorrent currently shuffles all magnet UDP trackers into one synthetic tier,
runs one UDP operation at a time, and ends the entire round on the first valid
response. That response schedules at least five minutes of sleep, even when it
contains only a few candidates. This tactical preserves the documented
synthetic-tier compatibility choice while correcting startup operation
breadth.

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
continues to dial at most three pending peers into at most eight established
connections, replacing only peers already classified replaceable by its
deterministic state. A tracker result never proves reachability or usefulness.

## Invariants And Bounds

- At most eight UDP tracker operations, four queued tracker result batches,
  200 compact peers per response, 1,000 peer records, three pending dials, and
  eight established peers exist under current defaults.
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
and the current content-registry classification in the headless probe. Do not
retain tracker or peer addresses.

After deterministic gates:

1. run three RSTorrent-only common-profile Big Buck Bunny attempts to 50%;
2. require at least 2/3 success before an alternating paired screen;
3. run an independent paired cohort only after that screen passes; and
4. retain correct identity, hashes, cleanup, candidate breadth, connection
   counts, and milestone timing rather than interpreting speed alone.

The comparable claim still requires two independent 10-run cohorts with at
least 8/10 RSTorrent successes and median paired latency no worse than 2.0x
libtorrent. A changing public swarm cannot be a deterministic CI gate.

## Non-Goals

- HTTP, HTTPS, WebSocket trackers, BEP 12 metainfo tiers, announce-all user
  settings, tracker persistence, proxying, or session-wide budgets
- DHT, PEX, LSD, incoming connections, NAT traversal, uTP, upload, or seeding
- raising the eight-peer content limit without evidence that eligible useful
  peers are rejected at that boundary
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
