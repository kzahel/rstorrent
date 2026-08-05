# Tactical 091: Availability-Ranked Piece Activation

Status: Planned. This document records a decision-complete candidate slice;
it does not authorize implementation or displace the current truthful tracker
and DHT advertisement work. It is the first planned download-liveness slice
after that **Now** item.

Topics: `download-correctness`, `peer-lifecycle`, `capability-readiness`

Dependencies: completed Tacticals
[`017`](017-adversarial-multi-peer-liveness.md),
[`020`](020-sustained-transfer-parity.md),
[`023`](023-strict-endgame-ownership.md), and
[`039`](039-generous-download-resource-pipelines.md) establish partial-piece
preference, per-connection availability, bounded request windows, strict
endgame, and active-piece memory limits. Planned Tactical
[`090`](090-peer-id-duplicate-connection-resolution.md) is not a code
dependency and follows this slice in the default post-**Now** order.

## Decision And Motivation

Retain the existing partial-piece-first and unique-piece connection-retention
policies, then make `RarestFirst` the default policy for activating new
background pieces. Retain a simple `InOrder` policy as an internal baseline
and deterministic diagnostic mode. Use a small closed policy enum and plain
state/functions, not a generic picker trait or plugin framework.

RSTorrent already knows which pieces each connection advertises, schedules
connections fairly, finishes requestable active work first, and protects a
connection that is the sole holder of a wanted piece. It does not maintain an
aggregate availability rank. When a connection needs to activate new work,
the picker currently walks the ordered incomplete-piece set and selects the
first requestable index. This can make many clients converge on low indices
and does not deliberately acquire scarce pieces while their sources remain
live.

The obvious rarest-first implementation is unsafe at supported scale. Scanning
or sorting every incomplete piece while filling every request slot turns a
2,097,152-piece torrent into a CPU denial of service even though the code is
conceptually pure. The first-party JSTorrent implementation also has product
history of severe picker cost when rarity work was performed through broad
candidate scans and repeated ordered insertion. This is a design constraint,
not merely a later optimization opportunity.

RSTorrent currently has 128-MiB and 256-MiB active-piece byte budgets but no
independent active-piece count ceiling. Tiny pieces can therefore admit far
more active entries than a large-piece torrent under the same byte budget.
This tactical adds a count ceiling and partial-pressure rule together with the
rarity index so active-first traversal itself stays bounded.

This tactical owns stable correctness scenario DL-C30 in
[`download-correctness`](../topics/download-correctness.md).

## Stopping Condition

This tactical is complete when all of the following hold:

1. requestable active background pieces remain ahead of new background
   activation and retain exact request ownership and active-byte bounds;
2. active work also has an explicit count ceiling and new activation stops
   under peer-ratio or 32-MiB partial-pressure limits;
3. among eligible inactive pieces, availability one is selected before higher
   availability and lower positive availability is selected before higher
   availability;
4. equal-rarity candidates use a stable per-torrent dispersed order rather
   than global ascending index, without runtime randomness inside pure state;
5. availability counts remain exact through bitfield, duplicate `Have`,
   have-all/have-none once supported, connection replacement, and disconnect;
6. block assignment from already-active work performs no global inactive-piece
   scan, sort, rank rebuild, or candidate allocation;
7. the optimized selector agrees with a deliberately naive test oracle across
   randomized and adversarial state-transition traces;
8. maximum-geometry operation counters, comparative CPU measurements, retained
   memory, temporary memory, and multi-torrent amplification all pass the
   explicit graduation gates below;
9. skipped/completed pieces, late payload, hash reset, strict endgame,
   unique-piece retention, and request fairness retain their existing
   behavior; and
10. deterministic, scripted split-availability, controlled libtorrent, and
    paired representative performance evidence pass.

## Scope

- Add torrent-owned live availability accounting derived only from admitted
  connection generations and validated availability messages.
- Represent full seeds separately from per-piece nonseed counts so have-all
  and seed disconnect do not require rewriting every piece counter.
- Maintain a production rarity index or equally bounded incremental structure.
  A single `Have` updates only the affected rank entry; dense bitfields and
  disconnect may use one bounded bulk update and one lazy rebuild.
- Add a closed `PieceActivationPolicy` with `InOrder` and `RarestFirst`.
  `RarestFirst` becomes the production default when this tactical graduates;
  `InOrder` remains available to deterministic tests, benchmarks, and bounded
  internal diagnostics without becoming a persisted user setting.
- Add `NaiveRarestFirst` only in test support. It scans every eligible piece
  using the same pure comparison key and serves as the readable correctness
  oracle; it is not compiled into or selectable by production clients.
- Rank new activation by positive availability ascending. Availability one is
  the explicit scarce-piece case but does not bypass file selection, completed
  state, active limits, or connection availability.
- Seed the equal-rarity order from stable torrent/session input supplied to the
  pure state, such as info hash plus the application peer ID. Two clients in
  the same swarm should not all choose the same lowest index solely because
  their torrent metadata matches.
- Preserve the existing round-robin connection pass and activate work only
  through a connection that advertises the selected piece. If connection-local
  eligibility makes an indexed query require a full piece scan, move the
  activation decision to a bounded piece-first scheduler boundary instead of
  adding a per-connection full-size index or shipping the scan.
- Add a hard default ceiling of 2,048 active pieces, independently of the byte
  ceiling. Platforms may configure a lower ceiling; raising it requires the
  same maximum-geometry CPU and memory evidence.
- Before activating another inactive piece, suppress activation if the
  resulting requestable partial work would exceed either 1.5 times the
  established content peer count or 2,048 standard blocks (32 MiB). At least
  one piece remains activatable so large pieces and one-peer torrents cannot
  deadlock. Falling peer count does not cancel already-owned blocks; it only
  prevents further activation until pressure falls.
- Add scheduler snapshot or structured diagnostic facts for the selected
  piece's availability, policy, selection tier, and active/pressure limits
  when needed to explain behavior.

## Non-Goals

- Implementing sequential-download UI, streaming priority, high/low file
  priority, deadlines, super-seeding, or user-facing picker controls.
- A generic strategy trait, dynamic dispatch, third-party picker plugins, or a
  persisted policy schema. The two production policies share one explicit
  state boundary; the naive policy is test-only.
- Request-window adaptation, endgame duplicate policy, storage queues, or
  hash-generation ownership.
- Persistent availability history, tracker scrape estimates, PEX hints, or
  counting disconnected peers as live availability.
- Peer scoring by client name, IP, throughput, latency, trust, or discovery
  source.
- Full snub behavior, common-piece probes for degraded peers, parole
  isolation, BEP 6 suggestions, or reverse-rarest selection.

Streaming remains a future overlay, but this slice must not close its seam.
The background activation policy stays below an outer urgency-band boundary
so a future active byte-range request can put its bounded near-playback window
ahead of unrelated background partials. Within that future urgent band,
distance from the requested offset may outrank rarity; outside it, the same
background rarest-first index remains the fallback. A seek must be able to
replace only the bounded urgent window without reordering or rebuilding all
pieces. This tactical adds no unused range-request or playback machinery.

## Normative And Reference Dossier

Rarest-first is implementation policy rather than a standalone BEP. BEP 3
defines `Bitfield` and `Have` as peer availability facts; it does not prescribe
the picker architecture.

Pinned libtorrent revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected:

- `reference/libtorrent/src/piece_picker.cpp::pick_pieces` keeps a shuffled
  priority-bucket piece list and updates individual availability changes in
  place, while dense bitfield changes mark the list dirty for one bulk rebuild;
- the same function forces partial-first behavior when partial pieces exceed
  1.5 times the peer count or 2,048 blocks, explicitly documented there as
  32 MiB;
- `piece_picker.cpp::partial_compare_rarest_first` sorts only the separately
  bounded partial list by availability and completion state;
- `reference/libtorrent/include/libtorrent/piece_picker.hpp` owns compact
  availability, priority-position, and download-state data;
- `reference/libtorrent/src/peer_connection.cpp` selects rarest-first after
  its initial-picker policy and reverses rarity for snubbed peers; and
- `reference/libtorrent/test/test_piece_picker.cpp`, especially
  `partial_piece_order_rarest_first` and the prioritize-partials cases around
  the partial-ratio behavior, covers partial rarity, most-complete ties,
  requested blocks, priority filtering, and parole mode.

Adopt the bounded partial-pressure lesson, incremental/single-change versus
bulk-rebuild split, and the separation between availability fact and picker
policy. Do not copy libtorrent's buckets, random threshold, class boundaries,
priority encoding, suggestion policy, or snub/parole coupling without
RSTorrent evidence. The 2,048-piece hard ceiling is an RSTorrent backstop;
libtorrent's cited limit is a partial-block pressure condition, not that same
data model.

The first-party JSTorrent checkout was inspected:

- `packages/engine/src/core/piece-availability.ts` owns a `Uint16Array` of
  aggregate live counts and keeps seeds separate;
- `packages/engine/src/core/active-piece-manager.ts` has independent active
  count and buffered-byte caps;
- `packages/engine/src/core/piece-requester.ts` finishes partial work before
  new activation, contains a bounded streaming overlay, and its
  `findNewPieceCandidates` history shows the broad scans, duplicate checks,
  and ordered candidate insertion that must not be repeated here; and
- `packages/engine/test/core/active-piece-manager.test.ts` and
  `piece-requester.test.ts` cover rarity, pending work, and filtered pieces.

Useful lessons are incremental counts, explicit partial/new phases, separate
count and byte bounds, and a narrow urgent-range overlay. Maintainer history
records that the original broad rarest-first picker performed abysmally; the
adversarial gates below turn that history into a regression condition.
RSTorrent does not inherit the TypeScript object model, policy constants,
streaming implementation, randomness, source, or fixtures.

## Owner And Data-Flow Map

```text
validated bitfield / have / connection removal
                    |
                    v
pure torrent swarm state
  connection availability
  seed count + bounded per-piece counts
  active/incomplete/verified piece state
  deterministic rank semantics + stateful bounded index
                    |
                    v
outer urgency seam -> active background -> ranked inactive background
                    |
                    v
existing request-attempt assignment and storage pipeline
```

- Pure swarm state owns counts, rank semantics, rank-index transitions, and
  activation. Socket tasks emit validated facts but do not compute rarity or
  select pieces.
- Purity applies to the comparison key, policy result, and state transitions;
  it does not require the production query to reconstruct or sort candidates
  from immutable snapshots on every call.
- The pure eligibility input may include a connection's advertised pieces,
  choke state, remaining request capacity, and generation together with the
  torrent's active and completion state. These are bounded values/state
  references, not sockets, tasks, channels, or a clone of the whole runtime.
- The torrent supervisor continues to own connection tasks, storage work,
  cancellation, and joins.
- Storage verification remains the only path to durable have state.
- Availability accounting contains no Tokio, clock reads, filesystem values,
  channels, or platform adapters.

## Ranking And Accounting Invariants

- One admitted nonseed connection contributes either zero or one to a piece.
  Duplicate `Have` messages do not increment twice.
- A seed contributes through the separate seed count and not through every
  per-piece counter. Transitions between bitfield and full-seed state reconcile
  exactly once.
- Disconnect and replacement remove precisely that connection generation's
  contribution. Stale events cannot decrement a newer generation.
- Completed, skipped, invalid, unavailable, and already-active pieces are not
  candidates in the inactive rank index.
- Requestable active background work remains first while every piece is in the
  background urgency band. The boundary permits a future bounded urgent band
  to precede it without changing background rarity semantics.
- Within inactive eligible background work, the comparison key is availability
  first and dispersed tie order second. Piece index is only a final total-order
  fallback.
- `InOrder` and optimized `RarestFirst` differ only in inactive background
  activation order. They share eligibility, active-first, ownership, limits,
  and connection-capacity rules.
- The test-only naive oracle uses the exact optimized policy key and eligibility
  predicate. A mismatch is a correctness failure, not acceptable heuristic
  drift.
- The tie seed is explicit input to pure state and deterministic in tests.
- Ranking never changes block ownership, reservation accounting, write
  eligibility, hash generations, or endgame loser handling.

## Complexity And Resource Bounds

Let `P` be piece count, `H` the set bits in one peer's availability, `C` the
established connection count, and `A` the active-piece count.

| Operation or resource | Initial bound |
| --- | --- |
| Piece geometry | `P <= 2,097,152` |
| Active pieces | `A <= 2,048` and existing platform byte ceiling; lower pressure limits may stop activation earlier |
| Availability counter | Covers the configured per-torrent live-peer ceiling; no wider than `u16` initially, four MiB at maximum geometry |
| Retained rarity state | Counts plus rank index at most 12 bytes per piece, 24 MiB at maximum geometry, plus `O(C)` connection state |
| Temporary rebuild state | At most 8 bytes per piece, 16 MiB at maximum geometry, one torrent rebuild at a time |
| Per-connection rank state | No additional `P`-sized candidate, position, or availability copy; existing peer bitfields remain separately bounded |
| Active block assignment | At most `O(A)` active-state inspection and zero inactive rank scans, sorts, rebuilds, or candidate allocations |
| Duplicate `Have` | `O(1)` accounting check and no rank mutation |
| New single `Have` | One counter mutation and `O(log P)` or bounded `O(1)` rank maintenance; never an `O(P)` rebuild |
| Dense bitfield admission/removal | Linear in its payload/set entries plus at most one `O(P)` lazy rebuild before the next activation |
| New-piece activation | `O(log P)` or bounded amortized equivalent after index readiness; never a full `P` scan or a `P * C` search |
| Completion/skip/reset | Bounded affected-piece rank maintenance; no unrelated global rebuild |
| Future urgent seek | `O(window)` replacement of a bounded urgent range; no `O(P)` background reorder |

The following are prohibited in production: sorting all incomplete pieces per
request or activation; recomputing counts by scanning peers times pieces;
allocating a candidate vector while issuing each block; rebuilding the global
rank for a single `Have`; and hiding a full scan behind an iterator, pure helper,
or seed-peer fallback. A representation that cannot meet these bounds must
change the activation boundary or stop for design revision.

The memory ceiling applies per maximum-geometry torrent. Validation also runs
four such pure torrent states concurrently and records the aggregate high-water
mark so a locally bounded representation does not hide unacceptable
multi-torrent amplification. A session-wide download-memory budget remains a
separate concern.

## Implementation Gates

1. Add test instrumentation for piece visits, rank comparisons, rank mutations,
   full rebuilds, temporary candidate allocations, and active visits before
   changing policy.
2. Add the test-only naive oracle and make current `InOrder` plus the new pure
   rarity key pass deterministic eligibility and tie-order tests.
3. Add availability-accounting transitions and differential traces through
   bitfield, `Have`, disconnect, completion, skip, reset, and stale generation.
4. Add the explicit active count and partial-pressure guards while preserving
   byte ownership and at-least-one-piece progress.
5. Prototype the smallest production index that satisfies the operation and
   memory counters. Buckets, an indexed heap, or a hybrid dirty rebuild are
   representation candidates, not pre-authorized architecture.
6. Switch the production default only after optimized and naive results agree,
   the maximum-geometry adversarial suite passes, and the comparative CPU gates
   pass.
7. Obtain controlled libtorrent completion with deliberately split and skewed
   availability, then update the owning readiness and correctness records.

## Adversarial Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure differential | Thousands of seeded randomized traces compare optimized and naive choices after bitfield add/replace, duplicate and new `Have`, seed transition, choke/capacity changes, disconnect/replacement, completion, skip/unskip, hash reset, and tie-cursor wrap |
| Maximum geometry | A synthetic 2,097,152-piece torrent exercises dense, sparse, alternating, and disjoint peer bitfields; rare pieces occur at the final indices and immediately behind the tie cursor; all pieces begin incomplete |
| Active hot path | With `A` at its hard ceiling, at least 100,000 block-assignment attempts visit only bounded active state and report zero inactive-piece visits, global sorts, rank rebuilds, and temporary candidate allocations |
| Rarity mutation | Long runs of duplicate `Have`, single new `Have`, dense bitfield arrival, seed conversion, disconnect, and connection churn assert the per-operation counter bounds and exact counts |
| Activation pressure | One-block tiny pieces, maximum-size pieces, one peer, maximum peers, fully requested active pieces, and falling peer count prove the count, byte, peer-ratio, 2,048-block, and at-least-one-progress rules |
| Scripted runtime | Scarce peer arriving/leaving, final unique piece, churn during activation, all seeds, all partial peers, no currently requestable rare holder, hash reset, and strict endgame unchanged |
| Controlled interoperability | RSTorrent downloads a skewed multi-piece fixture from multiple pinned libtorrent peers and acquires the deliberately scarce piece before common inactive pieces |
| Multi-torrent resources | Four maximum-geometry pure states build, update, query, and drop their indices while retained, temporary, and terminal memory return are measured |

The naive oracle intentionally does not run the full maximum-geometry hot
trace. It proves semantics on smaller exhaustive/randomized states; deterministic
operation counters prove the optimized algorithm at hostile geometry without
making the test suite reproduce the algorithm it is meant to prevent.

## CPU And Memory Graduation Gates

All timing comparisons run in the same process and build profile after warmup,
with at least five independent samples. Operation-count assertions are the
portable hard proof; wall-clock ratios catch constant-factor regressions that
asymptotic counters miss.

- On the maximum-geometry active hot-path trace, optimized `RarestFirst`
  scheduler CPU has median no greater than `1.10x` and p95 no greater than
  `1.20x` the `InOrder` baseline. Both use identical active state and request
  results.
- On a representative mixed trace containing availability updates, active
  assignment, piece activation, completion, and churn, optimized
  `RarestFirst` median scheduler CPU is no greater than `1.25x` `InOrder`.
- After initialization, block assignment performs zero heap allocations.
  Single-`Have` updates perform no `P`-sized allocation or rebuild.
- Bulk build/rebuild work remains linear by deterministic piece-visit counters.
  Measurements at 131,072, 524,288, and 2,097,152 pieces record normalized
  time per piece; a superlinear trend or an unexplained full-geometry cliff
  fails graduation.
- Retained and temporary picker memory stay within the table ceilings, include
  allocator overhead in the recorded high-water evidence, and return on
  torrent removal. Exceeding either ceiling requires an explicit tactical
  amendment rather than an undocumented optimization deferral.
- The report records request assignments per second as well as CPU time so a
  lower CPU figure caused by doing less scheduler work cannot pass.

No visible client, schema migration, new dependency, or physical device is
required. Public-swarm comparison remains opt-in under the existing evidence
policy.

## Escalation And Next Boundary

Ordinary representation choices within the recorded operation/memory bounds,
a small pure module extraction, additional accounting cases, and conservative
bound tightening are authorized only when this tactical is explicitly selected
for implementation. Stop for direction if evidence requires user-visible
sequential/streaming policy, a higher piece/peer/active ceiling, new persistent
state, a generic picker abstraction, or weakening a CPU or memory graduation
gate.

The next planned download-liveness slice is
[`090`](090-peer-id-duplicate-connection-resolution.md), followed by the BEP 6
request lifecycle in [`093`](093-bep6-fast-request-lifecycle.md). Snubbed-peer
reverse rarity and parole isolation remain separate, evidence-gated work.
