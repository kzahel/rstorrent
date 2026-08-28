# Tactical 186: Current Rates And Incremental Speed History

Status: **Active.** Explicit user direction on 2026-08-28 temporarily yields
Tactical `176` to this measured application-view optimization. Tactical `176`
retains only its unchanged macOS-hosted iOS simulator/archive compile gate.

Topics:
[`client-view-delivery-policy`](../topics/client-view-delivery-policy.md),
[`application-view-api`](../topics/application-view-api.md),
[`performance-and-live-evidence`](../topics/performance-and-live-evidence.md),
[`web-ui-design`](../topics/web-ui-design.md),
[`client-surfaces`](../topics/client-surfaces.md), and
[`capability-readiness`](../topics/capability-readiness.md).

## Motivation And Desired Outcome

The clean Tactical `185` run leaves an approximately 5 KiB/s floor in every
ordinary React surface. The always-present `session-rates` view requests a
complete 300-bucket ten-minute `SessionSpeed` history for payload received and
uploaded once per second, but its only consumers read the two current rates for
the session summary and document title. The actual Speed panel already owns a
separate interest-selected history view.

Separate latest current rates from ordered graph history. Current-rate clients
receive one tiny complete latest-value record at their selected cadence.
Visible speed graphs receive one complete bounded snapshot followed by every
newly completed bucket since their last semantic history position. Slower
delivery batches points without dropping resolution. The established view-set
cursor, acknowledgement, queue, lease, reset, and reconnect behavior remains
the sole delivery protocol.

At the stopping condition idle Transfers no longer sends graph history, a
visible graph reconstructs byte-identical fixed windows under ordinary,
coalesced, slow, replayed, and reset delivery, every first-party boundary
passes, and the retained production-browser measurement proves the result.

## Stable Scenarios

1. **SRH-001, current-only state:** a `SessionCurrentRates` view returns only
   the distinct requested metrics and complete latest values. A later update
   completely replaces that tiny state; pending updates keep only the newest
   value.
2. **SRH-002, interest:** the ordinary UI retains current received/uploaded
   rates on every surface but requests `SessionSpeedHistory` only while the
   Speed panel is visible. Entering the panel starts with a complete window;
   leaving it removes the history view while recording continues server-side.
3. **SRH-003, exact append:** after a history snapshot, a patch carries every
   newly completed bucket for every selected metric, including explicit null
   gaps, with no repeated retained prefix and no open/incomplete bucket.
4. **SRH-004, slow cadence:** when multiple buckets complete between
   deliveries, coalescing concatenates the exact contiguous values in order.
   Applying the merged patch equals applying each original patch.
5. **SRH-005, continuity:** history epoch, range, bucket geometry, base
   complete-through position, series membership, and equal append lengths are
   validated before mutation. A mismatch rejects the patch and uses the
   existing resynchronization path.
6. **SRH-006, bounded lag:** no client acknowledgement retains bounded queued
   delivery. Appends never exceed the selected fixed window; overflow or loss
   of reconstructible continuity rotates the existing view-set epoch and
   supplies a fresh complete snapshot rather than growing memory.
7. **SRH-007, replay and reconnect:** acknowledged cursors replay outstanding
   batches while the lease survives. Cursor mismatch, lease expiry, view-spec
   replacement, history-epoch replacement, and range/metric changes establish
   a new complete history snapshot.
8. **SRH-008, client parity:** Rust, generated TypeScript/schema, web, Android,
   and iOS reducers reconstruct the same complete current-rate and history
   models. Presentation consumes no partial series and invents no samples.
9. **SRH-009, measured result:** the exact Tactical `183` production fixture
   lowers the approximately 5 KiB/s always-on history floor with exact browser/
   gateway agreement, retained progress, zero resets, and clean shutdown.

## Semantic Contract

Add a distinct `SessionCurrentRates` view selector, snapshot, and patch. The
selector carries `1..=19` distinct available metrics and the existing delivery
policy. The value carries a capture time and exactly those metric/value pairs.
It is intentionally a complete replacement: latest state wins, no rate-specific
cursor exists, and a null value means the rolling rate is not yet covered.

Rename the ordered projection to `SessionSpeedHistory`. Its selector retains
the current fixed `SpeedRange`, `1..=8` distinct selected metrics, and delivery
policy. The complete snapshot contains history epoch, capture time, exact
range/bucket/start/complete-through geometry, live and persistence facts,
selected series, and the bounded metric catalog. Current rates do not appear
in the history DTO or in each series.

An ordinary history patch contains:

- `history_epoch`;
- `base_complete_through_millis` and new `complete_through_millis`;
- `captured_millis`;
- one ordered append per selected metric with identical nonzero value count;
  and
- an optional changed persistence fact.

The first appended bucket is exactly one bucket after the base position;
timestamps are derived from the declared fixed bucket size rather than
repeated per point. Applying `N` appended values drops the oldest `N` values,
advances `start_millis`, appends every supplied nullable value, and advances
the complete-through position atomically. No patch is emitted when no bucket
or metadata fact changed.

The view batch cursor and post-reduction acknowledgement remain responsible
for reliable transport, replay, queue release, and backpressure. The history
epoch plus complete-through positions are semantic continuity anchors, not a
second transport cursor or acknowledgement. New connections do not request an
arbitrary historical cursor in this slice; a bounded fresh snapshot is the
recovery result when established view-set replay is unavailable.

## Ownership, Tasks, And Bounds

`SessionRateHistory` remains the sole mutable fixed-ring owner and continues
recording independently of client interest. The existing joined
`SpeedHistoryRuntime` remains the only cadence/persistence task. `ViewHub`
projects complete current rates and exact completed-bucket ranges for each
requested selector. Subscription/view-set state retains only the latest
published semantic history position needed to form the next append.

`ViewSetInner` continues to own pending delivery and byte/count bounds. Its
history coalescer concatenates contiguous appends, replaces current-rate state,
and rejects incompatible bases or shapes. Transport adapters remain ignorant
of rate semantics. Client reducers own complete local windows and acknowledge
only after atomic validation and application.

Existing bounds remain authoritative:

- 19 available current metrics and at most 8 selected history series;
- the seven fixed range tiers and their existing 240--2,880 point windows;
- the existing view-count, snapshot-byte, queued-byte, batch-count, lease, and
  delivery-interval ceilings; and
- one patch append no larger than the selected complete window. A larger
  catch-up requires a snapshot/reset.

No engine, socket, persistence, or transport task is added. Runtime-independent
diff, validation, application, and coalescing helpers own the important state
transitions.

## Encoding Independence

JSON remains the only wire codec. Current-rate keyed values and contiguous
history arrays define semantic meaning before serialization. A future
MessagePack or other negotiated binary codec may assign explicit stable numeric
IDs to view kinds, metrics, and fields while preserving the same snapshots,
appends, cursor acknowledgements, and reset behavior. Rust enum order is not a
binary registry.

This tactical adds no binary dependency, numeric registry, codec negotiation,
compression, or mixed-version lane.

## Validation

- pure snapshot/append/apply/merge tests across every range geometry, multiple
  cadence intervals, null gaps, multiple metrics, maximum catch-up, and all
  hostile continuity failures;
- view-hub and view-set snapshot, interest, replay, acknowledgement, slow
  consumer, coalescing, overflow/reset, lease-expiry, and reconnect tests;
- generated Rust schema/TypeScript/UniFFI drift checks and exhaustive web,
  Android, iOS, desktop, and headless consumers;
- web reducer/component/controller tests, typecheck, production/CSP build,
  Android dual-ABI generated-boundary/build/unit tests, and Linux-available iOS
  generated/source inspection;
- Rust formatting, workspace Clippy, and workspace tests; and
- the identical opt-in production WebSocket bandwidth fixture, with report
  hash recorded and temporary profiles, reports, generated inspection trees,
  and payloads removed.

The updated iOS source and generated Swift boundary remain subject to Tactical
`176`'s existing macOS-only simulator/archive compile gate. This Linux host
must not claim that compile.

## Non-Goals

- No arbitrary time-range query, panning/page cursor, long-lived resume token,
  client wall-clock position, or second acknowledgement protocol.
- No server-side downsampling, interpolation, sample omission, or new speed
  persistence tier.
- No raw user cadence control, named cellular/background profile, visibility
  lifecycle policy, Library/Summary overlap removal, viewport paging, or log
  optimization.
- No binary format, WebSocket fallback, relay, TLS/carrier accounting, or
  public compatibility promise.
- No engine bandwidth accounting, rate definition, scheduling, persistence,
  or product graph redesign.

## Escalation And Stopping Condition

Stop for direction if exact history requires a second transport acknowledgement,
unbounded per-client state, a new persistence owner, a transport-specific
semantic API, a third-party dependency, or a changed speed/rate product
definition. Ordinary generated-contract replacement, client reducer work,
bounded state retention, and measured validation remain authorized.

This tactical is complete only when both old overloaded `SessionSpeed` uses are
removed, all stable scenarios pass on every available first-party boundary,
the clean retained run proves the expected causal reduction without lost
points/reset/progress, owning topics record exact evidence and deferrals,
Tactical `176` is restored as the sole **Now**, commits are coherent, and all
temporary artifacts are removed.
