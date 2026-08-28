# Tactical 184: View-Aware Current-State Coalescing

Status: **Complete (2026-08-28).** Compatible pending patches now coalesce
across unrelated view IDs without crossing a same-view barrier. The retained
production-browser A/B reduces active detail traffic by 70--86% with no reset,
lost progress, duplicate-view batch, or public-contract change. Tactical
[`185`](185-typed-sparse-hot-view-patches.md) owns the measured sparse-row
follow-up. Tactical `176` retains only its unchanged macOS-hosted iOS
simulator/archive compile gate.

Topics:
[`client-view-delivery-policy`](../topics/client-view-delivery-policy.md),
[`application-view-api`](../topics/application-view-api.md),
[`application-connection-architecture`](../topics/application-connection-architecture.md),
[`performance-and-live-evidence`](../topics/performance-and-live-evidence.md),
and [`capability-readiness`](../topics/capability-readiness.md).

## Motivation And Desired Outcome

Tactical `183` proves that navigation interest already omits inactive detail
projections and unchanged torrent rows, but ordinary wide Workbench traffic is
still 101.86--147.93 KiB/s during one 256 KiB/s download. General delivers 390
complete Library patches and 390 complete Summary replacements during one
eight-second steady window even though each view asks for a 100 ms minimum
interval.

The view-set accumulator only attempts patch coalescing against the tail of
one shared pending queue. Hub publication interleaves Library, Summary, and
selected-detail IDs, so compatible later patches for a view cannot reach that
view's earlier pending patch. Cadence delays those updates but does not reduce
them to one current representation.

Make coalescing view-aware without separating logical views, adding a socket,
or weakening one atomic view-set cursor. At the stopping condition, compatible
pending patches for one view coalesce across unrelated interleaved view IDs;
same-view barriers and ordered Diagnostics remain exact; queue accounting and
reset behavior remain bounded; every adapter sees the same batches; and the
retained Tactical `183` baseline demonstrates the causal reduction before any
sparse-field contract is introduced.

## Stable Scenario Subset

1. **VC-001, interleaved latest values:** Library A1, Summary B1, Library A2,
   and Summary B2 leave one pending compatible patch per view with the newest
   complete values.
2. **VC-002, collection composition:** keyed upsert/removal, range, peer,
   file, tracker, disk, DHT, speed, and singleton coalescing retain their
   existing projection-specific semantics when other view IDs intervene.
3. **VC-003, ordered Diagnostics:** diagnostic events retain sequence order.
   Compatible append segments may combine across other views only within the
   existing 128-event/128-KiB patch bounds. An incompatible or full segment is
   a same-view barrier and later events never merge past it.
4. **VC-004, replacement barriers:** a same-view snapshot or removal discards
   superseded earlier pending state as today. A later patch follows that
   replacement and never reaches an older patch across it.
5. **VC-005, cadence and batches:** a compatible current-state view appears at
   most once in an emitted batch, and `min_interval_millis` continues to space
   deliveries rather than manufacturing updates.
6. **VC-006, exact bounds:** every successful replacement recomputes the
   encoded update size and exact pending-byte total. Queue high water remains
   monotonic; overflow still produces one explicit reset and coherent
   snapshots.
7. **VC-007, transport equivalence:** HTTP diagnostic pull, Tauri Channel, and
   the browser WebSocket consume the unchanged `UpdateBatch` contract and
   acknowledge the same cursor. The production run still uses one WebSocket,
   no semantic HTTP, no binary frame, and no reset.
8. **VC-008, measured A/B:** rerun Tactical `183` from a clean revision with
   its exact 12-row, 64-MiB, 256-KiB/s, eight-second configuration. Record
   per-view updates, duplicate-view high water per batch, bytes, batches,
   gateway cross-check, progress, and cleanup against the retained pre-change
   result.

## Contract, Ownership, And Algorithm

No public DTO, generated binding, view ID, delivery interval, cursor, API
version, or task owner changes in this tactical. `ViewSetInner` remains the
one lock-protected owner of pending items, exact bytes, one in-flight batch,
cadence timestamps, reset state, and notification.

For a new patch:

1. scan backward through the byte-bounded pending queue for the most recent
   item with the same `view_id`;
2. if that item is a compatible patch, apply the existing pure
   `coalesce_patch`, update its encoded size and readiness, and enforce the
   unchanged aggregate bound;
3. if the newest same-view item is a snapshot, removal, or incompatible patch,
   append after it and do not search farther; and
4. if no same-view item exists, append normally.

This is intentionally a bounded linear search rather than a second mutable
index whose offsets must be repaired on drains, replacements, and removals.
The queue is already capped at 512 KiB and snapshots at 16 MiB. Record any
evidence that makes lookup cost material; do not add an abstraction in
anticipation of it.

Logical views remain separate. Coalescing never combines Library with Summary
or Files with Pieces. They continue to share one atomic batch/cursor and may be
serialized together after each view has independently accumulated its pending
meaning.

## Encoding Independence And Follow-Up Boundary

Coalescing occurs on typed semantic `ViewPatch` values before transport
serialization. Do not inspect JSON field names, serialized byte substrings, or
wire frame shape to decide compatibility. A later negotiated binary codec must
be able to encode exactly the same `ViewSnapshot`, `ViewPatch`, `UpdateBatch`,
cursor, reset, and acknowledgement semantics.

The second user-directed repair is typed sparse current-state rows. It is not
implemented here because this A/B run must identify the residual fields and
projections. Its design must use closed typed fields or typed field groups,
preserve absent versus explicit nullable values, merge deterministically, and
remain independent from JSON object paths. Fresh and reset snapshots remain
complete authoritative values. Open the follow-up tactical from the retained
post-coalescing evidence rather than guessing its contract in this slice.

Binary negotiation, compression, schema dictionary design, field-number
assignment, and codec benchmarking are much later work. This tactical must not
make them harder by coupling semantic deltas to JSON.

## Validation

- deterministic `rstorrent-session` accumulator, cadence, replacement,
  diagnostic-order, byte-accounting, overflow, replay, and reset tests;
- existing application view-set and adapter suites;
- Rust formatting, workspace Clippy, and workspace tests;
- web generated-contract drift, unit tests, typecheck, and production/CSP
  build, even though the public contract should remain byte-identical;
- the opt-in production WebSocket baseline plus its browser/gateway exact-byte
  cross-check; and
- documentation reconciliation and exact temporary cleanup.

Android and iOS generated/build gates are inapplicable because this slice
changes no shared DTO or client reducer. The subsequent sparse-patch tactical
will require both generated native boundaries and first-party reducers in the
same change.

## Non-Goals

- No field-level patch, projection overlap removal, rate-history delta,
  viewport/page policy, cadence profile, hidden-client policy, or user setting.
- No binary codec, compression, polling fallback, relay, TLS/carrier-byte
  measurement, or public network run.
- No queue-limit increase, reset suppression, cursor weakening, reordered or
  conflated Diagnostics, second socket, per-view task, or compatibility alias.
- No product UI or engine behavior change.

## Escalation And Stopping Condition

Stop for direction if correct coalescing requires a public contract change,
another task/lock owner, relaxed Diagnostics semantics, larger queue/snapshot
bounds, or a transport-specific branch. Ordinary accumulator repair, focused
test-harness attribution, and bounded live-run repair remain in scope.

This tactical is complete only when all stable scenarios pass, the unchanged
wire contract is proven, the clean Tactical `183` run records a causal A/B
reduction with no lost state or reset, the owning topics record the result,
and a separate sparse-row tactical is selected from the remaining measured
traffic.

## Result And Evidence

`ViewSetInner` now finds the newest pending item for the same view rather than
only inspecting the shared queue tail. Deterministic tests prove interleaved
Library/Summary collapse, snapshot barriers, bounded Diagnostics segment
ordering, and exact replacement byte accounting. The measurement harness also
records duplicate view IDs and maximum updates for one view in each batch.

The clean retained run at commit `4151c837` used the same 11 stopped torrents,
one 64 MiB active torrent, 256 KiB/s source, and eight-second windows as
Tactical `183`. It carried 1,239,166 server bytes instead of 5,268,042
(-76.48%), with zero resets, zero duplicate-view batches, one update per view
per batch maximum, exact browser/gateway agreement, and progress from 1% to
20%. Steady active detail rates changed as follows:

| View | Before KiB/s | After KiB/s | Reduction |
| --- | ---: | ---: | ---: |
| Peers | 114.05 | 33.83 | 70.34% |
| General | 101.86 | 14.69 | 85.58% |
| Files | 113.31 | 16.02 | 85.87% |
| Pieces | 147.93 | 27.81 | 81.20% |
| Normal Logs | 103.23 | 15.96 | 84.54% |

Idle remained 5.28 KiB/s and active Transfers measured 13.14 KiB/s, within run
variance of the 12.84 KiB/s baseline. Batch cadence was nearly unchanged
(520 versus 515 gateway view batches), proving the reduction comes from
semantic coalescing rather than a slower producer. The report SHA-256 is
`64172265b2c1eafc6565f4fd742b067f6a34fc744e2c82c938dd17bcf18838dc`.

Residual attribution selects typed sparse Torrent, Peer, File, and active-piece
rows for Tactical `185`. Complete session-rate history replacement remains a
separate projection-specific follow-up.
