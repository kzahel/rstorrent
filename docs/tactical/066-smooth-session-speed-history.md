# Tactical 066: Smooth Session Speed History

Status: Planned; direction accepted on 2026-08-03. Implementation has not
begun.

Topics: `application-view-api`, `web-ui-design`,
`desktop-inspection-surface`, `disk-and-piece-inspection`,
`performance-and-live-evidence`, `capability-readiness`,
`product-state-and-feedback`

## Motivation

RSTorrent exposes truthful current receive and storage rates, but the Speed tab
is an empty scaffold and no history survives while it is closed. JSTorrent's
uPlot implementation demonstrates the usefulness of a live speed chart, but
its update path and visual behavior are not the product direction here. A
general chart dependency would add weight while still requiring custom timing,
staleness, interpolation, accessibility, and theme behavior.

This slice adds a bounded session-owned round-robin history, durable coarse
rollups, and a hand-rolled high-DPI Canvas renderer. Exact byte observations
remain discrete and explicitly classified. The visible chart moves on
`requestAnimationFrame`, one sample behind the latest complete point, so it can
interpolate between known anchors and pan smoothly without claiming RAF-rate
measurements. When delivery stops it freezes and becomes stale; it never
invents a glide to zero.

## Desired Outcome And Stopping Condition

The session-scoped Speed tab opens with recent history even when it was closed
during the transfer and can request progressively coarser 1-hour, 24-hour,
30-day, and 2-year history without downloading every retained bucket. Its
primary chart distinguishes accepted payload, completed logical staging writes,
and hash-verified payload. It shows upload as unavailable rather than zero and
renders a smooth nonnegative curve through exact sample anchors. RAF motion is
limited to presentation, pauses when hidden, honors reduced motion, and does
not drive React renders. Hover and keyboard inspection snap to the nearest
exact sample.

The tactical stops when the pure multi-tier history owner, exact metric
instrumentation, separate bounded `metrics.db`, application lifetime and
shutdown path, range-selective named session view, generated contracts,
hand-rolled chart, deterministic scenarios, resource evidence, and controlled
loopback transfer all pass. No chart dependency is added.

## Dependencies And Sequence

- Tacticals `033`, `044`, `045`, `048`, `052`--`054`, and `060` provide view
  delivery, exact receive/store observations, the Canvas precedent, bounded
  storage throughput, and shared transport behavior.
- Tactical `007` and [`../topics/client-persistence.md`](../topics/client-persistence.md)
  provide the profile directory, SQLite precedent, and correctness-critical
  `session.db` boundary that the disposable metrics store must not join.
- Tactical `064` centralizes tab scope and Tactical `065` proves the following
  session tab through it. This tactical is third in the accepted sequence.
- The implementation must read
  [`../topics/performance-and-live-evidence.md`](../topics/performance-and-live-evidence.md)
  before collecting any performance evidence. Public live evidence remains
  out of scope.

## Scope

- Add a runtime-independent `SessionRateHistory` that records exact accepted
  payload, completed logical staging writes, and hash-verified payload, plus a
  bounded closed taxonomy for network/discovery traffic and wasted work.
- Retain fixed 100 ms, 500 ms, 2 s, 10 s, 1 minute, 15 minute, and 1 day tiers
  covering 30 seconds through 2 years. Roll completed child byte sums into
  their aligned parent; never average averages.
- Persist only the 1-minute-and-coarser closed tiers in a separate bounded
  profile-local `metrics.db`. Keep high-resolution tiers in memory.
- Add one application-owned rate-clock task that runs at 100 ms only while a
  Speed view is interested. Recording and snapshot creation advance elapsed
  buckets synchronously, so history remains truthful while the task is parked.
- Add capability `session_speed`, a range-selective `ViewSpec::SessionSpeed`,
  bounded snapshots and append/upsert patches, generated TypeScript/schema and
  UniFFI bridge contracts, strict browser decoding, and reducer coverage.
- Render 30-second, 2-minute, 10-minute, 1-hour, 24-hour, 30-day, and 2-year
  ranges with a purpose-built Canvas plot, exact trailing-one-second current
  rates, selected-window average/peak summaries, exact-sample inspection,
  responsive legends, and explicit stale behavior.
- Add a compact selectable traffic breakdown for exact peer protocol, DHT, and
  tracker byte categories without putting tiny discovery traffic on the
  primary payload/storage scale by default.
- Add permanent deterministic scenarios plus accessibility, responsive,
  reduced-motion, theme, stale/reset, scale, and controlled live evidence.

## Non-Goals

- uPlot, D3, another general chart package, or a reusable chart framework.
- Pan, zoom, arbitrary time ranges, annotations, telemetry export, an unbounded
  metric catalog, or a generic analytics/query system.
- Upload/seeding implementation. Payload upload is explicitly unavailable and
  has no zero-valued samples.
- Per-torrent or per-peer charting, disk latency plotting, physical block-device
  IO claims, kernel-derived IP/TCP overhead, or packet capture.
- Encoding startup, shutdown, crash, application-version change, migration, or
  other discrete operational events as numeric RRD buckets. Their accepted
  installation-wide ownership and separate future tactical live in
  [`../topics/product-state-and-feedback.md`](../topics/product-state-and-feedback.md).
- Changing the peer receive, storage, scheduling, or backpressure hot paths
  beyond a bounded task-free observation addition.
- Background animation while hidden, animation under reduced-motion, public-
  swarm throughput comparison, visible desktop launch, or native Android UI.

## Reference Dossier

### Metric semantics

BEP 3 does not define client rate sampling or chart behavior. The primary
series are RSTorrent product observations:

- **Payload received** counts a requested content block accepted by the block
  owner as `BlockReceived`. Redundant and unsolicited blocks are excluded.
- **Staged write** counts the logical block length after its current-generation
  staging write completes successfully as `BlockStored`. It is not physical
  device IO, hash rereads, filesystem metadata, or checkpoint synchronization.
- **Payload verified** counts a complete piece only after its v1 SHA-1 succeeds.
  It precedes any required durable have-state checkpoint and is not publication.

Accepted and staged bytes remain counted if their piece later fails its hash;
the retry is counted again when it is received and written again. Verified
does not advance for the failed generation. Hash-failed and redundant payload
therefore remain exact separately labeled waste totals and optional breakdown
series, not corrections subtracted from historical receive/write buckets.

The instrumentation catalog additionally owns peer-wire payload/protocol,
metadata payload, DHT datagram, and tracker datagram bytes in each applicable
direction, plus logical hash-read bytes. Measurements occur at the actual
codec, socket, or storage command boundary. RSTorrent must not label
application-visible peer-wire bytes as kernel IP/TCP overhead. The default
chart remains payload received, staged write, and payload verified; smaller
traffic categories appear in an explicit breakdown.

### Pinned libtorrent oracle

Re-inspect libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `src/session_stats.cpp` for current versus cumulative counters and metric
  units;
- `include/libtorrent/performance_counters.hpp` for stable counter identity and
  aggregation distinctions; and
- `examples/session_view.cpp` for a session-oriented operational speed view.

RSTorrent adopts explicit metric identity and honest units, not libtorrent's
counter inventory, alert cadence, history format, or terminal UI.

### JSTorrent product history

Inspect local JSTorrent revision
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/ui/src/components/SpeedTab.tsx` for the existing product affordance
  and uPlot update behavior;
- `packages/engine/src/core/bandwidth-tracker.ts` for observation semantics; and
- the round-robin history implementation and tests reached from those files.

RSTorrent retains the useful session history and multiple time windows but
authors an independent Rust history and Canvas renderer. No source, fixture,
uPlot option structure, or dependency is copied.

## Existing Boundary And Concrete Improvement

The engine already emits `DownloadActivityEvent::BlockReceived { length }` for
accepted payload and `BlockStored { length }` after current-generation storage
completion. Its Disk runtime also owns verified-byte, hash-failure, logical
hash-work, and redundant-payload facts, but the activity boundary needs a
length-bearing verified observation and exact traffic byte observations before
they can feed history without subtracting reset-prone totals. Current Disk
rates remain useful instantaneous diagnostics but are not retained history.

DHT currently retains query/response counts rather than datagram bytes,
tracker observation is lifecycle-oriented, and peer protocol byte fields are
explicitly unavailable. This tactical adds byte counters at those owners; it
does not infer them from message counts or serialized diagnostic text.

The concrete boundary improvement is one pure bounded session projection fed
by typed byte events. It has no Tokio, filesystem, socket, SQLite, JSON, React,
or Canvas dependency. A small persistence adapter owns durable coarse buckets,
and an application-owned clock closes live buckets only while observation
requires cadence. The engine does not depend outward on history, persistence,
or presentation.

## Owner, Task, Cancellation, And Data Flow

```text
peer/DHT/tracker/storage/integrity owners
  | typed byte observations, no payload buffers
  v
existing synchronous application activity sink
  |
  v
ViewHub-owned SessionRateHistory (pure fixed rollup rings)
  ^
  | advance monotonic clock while Speed has interest
ApplicationService-owned parkable rate clock task
  |
  +--> metrics persistence owner --> profile-local metrics.db
  |
  +--> selected-range snapshot / bounded bucket patches
  |
  v
leased transport -> strict browser replica
  |
  +--> textual current / average / peak
  +--> Canvas renderer with its own RAF-only display state
```

`SessionRateHistory` is the sole mutable history owner, serialized by the
existing view-hub synchronization boundary. Event recording obtains its
monotonic timestamp while holding that owner, advances elapsed buckets, and
adds the event length to its 100 ms base accumulator. Completed buckets cascade
upward as exact byte sums. This avoids a stale timestamp racing a bucket close,
avoids averaging averages, and keeps event-path work independent of the number
of retained presentation tiers.

`ApplicationService` owns exactly one interval task, cancellation token, and
join handle. While at least one Speed view is interested, the task advances
history every 100 ms using a monotonic clock and notifies views only when a
bucket closes. It uses skipped missed-tick behavior and advances all elapsed
fixed boundaries in one bounded operation; it never queues overdue ticks.
Without Speed interest it parks on a notification rather than waking ten times
per second. Typed byte events and snapshot creation still advance history, so
closing Speed never resets or stops measurement. Application shutdown first
joins every byte-producing engine owner, then advances/finalizes history,
cancels and joins the clock, flushes and joins its bounded persistence owner,
and finally drops the view hub.

## History Semantics And Bounds

The tiers are fixed and not configurable:

| Window | Bucket width | Capacity | Nominal retained history |
| --- | ---: | ---: | ---: |
| Recent | 100 ms | 300 | 30 seconds |
| Short | 500 ms | 240 | 2 minutes |
| Session detail | 2,000 ms | 300 | 10 minutes |
| Hour | 10 seconds | 360 | 1 hour |
| Day | 1 minute | 1,440 | 24 hours |
| Month | 15 minutes | 2,880 | 30 days |
| Long term | 1 day | 730 | 2 years |

The four high-resolution tiers are driven by one application-session monotonic
clock. Their boundary phase is mapped from the captured UTC anchor so the
10-second child aligns with fixed UTC minute boundaries. The three durable
tiers therefore remain addressable across process epochs without using wall
time to order live events. Every parent width is an exact multiple of its
child: `5 x 100 ms`, `4 x 500 ms`, `5 x 2 s`, `6 x 10 s`, `15 x 1 min`, and
`96 x 15 min`. A parent sums completed child bytes and coverage; it never
averages child rates. Bucket intervals are half-open `[start, end)` and an event
at a boundary belongs to the new bucket. Only completed buckets cross the view
boundary.

Bucket placement uses the application's synchronous observation time, not an
invented wire-arrival or filesystem-completion timestamp. Monotonic time owns
live ordering. A captured UTC anchor identifies durable boundaries; a backward
wall-clock jump, an unsupported forward discontinuity, or missing persisted
state starts a new history epoch and an explicit gap rather than overwriting a
bucket or manufacturing zeros.

An elapsed bucket while the application owner was alive with no matching event
is an exact known zero because the activity sink is synchronous with byte
ownership. After all byte-producing engine owners join, a clean shutdown
persists the active durable accumulator; startup can complete intervening zero
time. An unclean shutdown may lose at most the active persistence batch and
records that interval as a gap, not zero. If a task is delayed, `advance`
closes every elapsed boundary up to each ring's capacity and fast-forwards
older empty history without allocating per missed tick. Event ordering through
the serialized owner prevents a late byte from entering a completed bucket.

Each fully retained series has `300 + 240 + 300 + 360 + 1,440 + 2,880 + 730 =
6,250` buckets. The closed initial catalog has 18 available retained series,
for a hard maximum of 112,500 native buckets and 90,900 durable bucket rows.
Payload upload is the nineteenth kind but remains `unavailable` with no ring
allocation until upload exists. An ordinary minute persists at most 18 closed
rows; an aligned 15-minute/day boundary persists at most 36/54 in the same
transaction, plus bounded retention deletes. Only one requested tier and at
most eight requested series cross one view boundary. Adding a metric kind or
retention tier requires rechecking native memory, on-disk size, write
amplification, and the selected-range view budget.

Counters use saturating arithmetic. Bucket bytes and selected-range totals use
the existing decimal-string JSON convention. Live animation uses
session-elapsed monotonic time; persisted bucket identity uses UTC epoch
milliseconds with an explicit epoch/coverage marker and never silently
conflates a clock gap with an idle zero.

## Metric Catalog And Accounting Invariants

The first catalog is closed and bounded. A new kind requires an explicit
semantic definition and resource review rather than accepting an arbitrary
string name.

| Metric kind | Meaning | Initial presentation |
| --- | --- | --- |
| `payload_received` | Requested content block body accepted by the block owner | Primary line |
| `staged_write` | Current-generation logical block bytes after successful staging write | Primary dashed line |
| `payload_verified` | Complete piece bytes after successful v1 hash | Primary line/summary |
| `peer_wire_received` / `peer_wire_sent` | TCP bytes at the application socket boundary, excluding kernel IP/TCP headers | Breakdown |
| `peer_protocol_received` / `peer_protocol_sent` | Peer-wire framing, handshakes, control messages, and other non-payload bytes | Breakdown |
| `metadata_payload_received` / `metadata_payload_sent` | BEP 9 metadata body bytes, separate from content payload | Breakdown |
| `peer_unclassified_received` / `peer_unclassified_sent` | Residual socket bytes not yet attributable to a closed payload/protocol category | Breakdown/degraded state |
| `dht_received` / `dht_sent` | Complete UDP DHT datagram lengths at `recv_from` / successful `send_to` | Breakdown |
| `tracker_received` / `tracker_sent` | Complete tracker transport bytes at receive / successful send | Breakdown |
| `logical_hash_read` | Piece bytes submitted to verification reads, including failed hashes | Breakdown |
| `payload_redundant` | Late duplicate block bodies for a request attempt already satisfied | Waste summary/breakdown |
| `payload_hash_failed` | Piece-generation bytes reported when its hash fails | Waste summary/breakdown |
| `payload_upload` | Unavailable until upload/seeding exists | Unavailable summary |

Component series may overlap their enclosing wire total and therefore are not
blindly summed. For peer traffic, classified content bodies, metadata bodies,
and protocol bytes must never exceed the corresponding wire total over one
coherent interval. Any residual unclassified wire bytes remain named
`unclassified`, not silently assigned to protocol overhead. Invalid or
unsolicited payload may increment wire/unclassified accounting but never
`payload_received`.

`payload_hash_failed` is attributed to the moment verification reports the
failure. It does not retroactively rewrite the earlier receive/write buckets;
the three primary histories remain records of when that work actually
occurred.

All byte observations are task-free bounded values. No packet body, endpoint,
torrent identity, peer identity, URL, or error text enters metric history.
Session aggregation continues across torrent removal. Counters saturate rather
than wrap, and a saturated series becomes visibly degraded instead of
publishing a plausible lower value.

## Durable Metrics Store

Long-lived coarse history belongs in `metrics.db` beside `session.db` in the
profile directory. It is a separate SQLite database and connection, not a
separate process or daemon. The separation is deliberate:

- `session.db` is correctness-critical control, resume, storage-root, removal,
  and request-replay state with `synchronous=FULL` semantics;
- metrics are derived, bounded, independently resettable data whose write
  cadence must not contend with have-state checkpoint commits;
- metrics schema migration, retention, corruption, backup, and export policy
  must not expand the failure domain of torrent recovery; and
- a metrics persistence failure degrades durable history but never pauses or
  fails a download.

`MetricsStore` owns one connection and one bounded application task. It uses
WAL with `synchronous=NORMAL`, batches all series whose one-minute-or-coarser
buckets closed into at most one transaction per minute, and joins after a final
bounded flush on clean shutdown. The receive/storage hot path never performs
SQLite work or waits for this task. Queue saturation or a write failure records
an explicit history gap and diagnostic, retains live in-memory history, and
does not retry without a bound.

The conceptual schema is one versioned table keyed by metric kind, tier, and
UTC bucket start, storing exact bytes, duration, and coverage. It stores no
rates, averages, peaks, display labels, or JSON. Retention deletes or replaces
rows outside each fixed ring capacity in the same transaction. A corrupt or
unsupported metrics database is preserved for diagnosis and replaced by a new
history epoch without opening or mutating `session.db`.

Only the 1-minute, 15-minute, and 1-day tiers are durable. Clean shutdown
persists the active coarse accumulator needed to resume exact rollup. An
unclean stop may lose at most the documented active batch; restart marks that
span as unknown coverage rather than manufacturing zero. A new browser client
within the same application epoch sees in-memory high-resolution history; a
new application process reconstructs only durable coarse tiers.

Profile deletion removes both databases under the existing profile ownership
policy. Backup/export does not silently begin including metrics merely because
the file shares a directory; that remains an explicit future product choice.

## View Contract

Add capability `session_speed` and `ViewSpec::SessionSpeed { view_id, range,
series, delivery }`. `range` selects exactly one appropriate tier; `series` is
a duplicate-free set of at most eight kinds from the closed catalog. The
default requests the three primary series for 30 seconds. Conceptually:

```text
SpeedRange =
  recent_30s | short_2m | detail_10m | hour_1h |
  day_24h | month_30d | long_term_2y

SpeedSeriesKind =
  payload_received | staged_write | payload_verified |
  peer_wire_received | peer_wire_sent |
  peer_protocol_received | peer_protocol_sent |
  metadata_payload_received | metadata_payload_sent |
  peer_unclassified_received | peer_unclassified_sent |
  dht_received | dht_sent | tracker_received | tracker_sent |
  logical_hash_read | payload_redundant | payload_hash_failed |
  payload_upload

SpeedSeriesAvailability = available | unavailable

SpeedSeriesView {
  kind,
  availability,
  unavailable_reason?,
  range_bytes,
  trailing_one_second_bytes?,
  first_bucket_start_utc_millis,
  bucket_millis,
  capacity,
  bucket_bytes[], // decimal string = complete, including "0"; null = gap
}

SpeedBucketUpdate {
  kind,
  bucket_start_utc_millis,
  bytes?, // present = complete, including "0"; absent = gap
}

SessionSpeedSnapshot {
  lifecycle,
  captured_utc_millis,
  captured_monotonic_millis,
  history_epoch,
  range,
  series[],
}

SessionSpeedPatch {
  captured_utc_millis,
  captured_monotonic_millis,
  history_epoch,
  range,
  series_summaries[],
  upsert_buckets[],
}
```

`range_bytes` is the exact sum of complete covered buckets in the selected
window, excluding gaps. It is not an all-time counter. The optional
`trailing_one_second_bytes` is computed from the completed 100 ms tier and is
present only when that full second has known coverage; it remains independent
of the selected historical tier. Patches carry the corresponding bounded
per-series summaries so a client does not infer coverage from arrival cadence.

The exact patch bucket key is `(series, range, bucket_start_utc_millis)`. A
normal interested 30-second view appends at most one completed 100 ms bucket
per requested available series. Coarser child and durable parent closures do
not cross that view unless its requested range selects them. If a client falls
behind, existing whole-view coalescing may send a larger bounded replacement or
reset; it never creates an unbounded backlog.

`history_epoch` changes when the history owner is recreated, durable state is
replaced, or a clock discontinuity prevents safe continuation, and it forces
client replacement. Unsupported, unavailable, disconnected, stale, reset,
overflow, explicit gap, and valid zero history are distinct. Payload upload is
`unavailable` with a short typed reason and no buckets, not an available series
containing zeros.

Delivery for an interested 30-second Speed view is requested at 100 ms. Other
ranges publish bucket data no faster than their bucket width, while a compact
trailing-one-second summary heartbeat is capped at 1 Hz. The service may
coalesce, reset, or arrive late; capture and exact bucket timestamps remain
authoritative. Snapshots use one aligned columnar byte array per series rather
than repeating timestamps/durations in thousands of JSON objects: a decimal
string, including `"0"`, is complete coverage and `null` is a gap. Patch
updates preserve the same distinction. Switching range replaces the leased
projection with that one bounded tier rather than downloading every retained
tier. Closing the lease evicts the browser replica but not native rings or
durable coarse history.

## Canvas Rendering Contract

The chart is purpose-built code local to Speed, with pure geometry helpers and
one imperative renderer. React owns layout, controls, labels, and semantic
state; it does not own animation frames or a copied array on every frame.

### Time and RAF behavior

- For 30-second, 2-minute, 10-minute, and 1-hour live ranges, the chart
  intentionally displays one selected-tier sample period behind the newest
  complete anchor: 100 ms, 500 ms, 2 s, or 10 s. This permits interpolation
  only between two received exact values.
- On receipt, the renderer anchors session elapsed time to
  `performance.now()`. While visible, RAF updates the x transform so time pans
  continuously and advances interpolation through already received anchors.
- RAF never fabricates a future y value, mutates history, asks React to render,
  or changes the exact crosshair samples. If no bracketing anchor exists, the
  line holds the last exact anchor.
- If a 30-second view receives no fresh capture for one second, or another
  range misses three expected 1 Hz summary heartbeats, the live presentation
  freezes at its last authoritative time and shows **Stale**. It does not decay
  toward zero. An idle connected application continues to publish exact zero;
  idle and stale never share presentation.
- `document.visibilityState !== "visible"`, unmount, transport disconnect, or
  tab switch cancels the frame. Resume uses the next authoritative capture and
  never tries to replay missed animation frames.
- Under `prefers-reduced-motion: reduce`, there is no continuous RAF. The chart
  draws only on data, resize, theme, range, series, or inspection changes and
  presents exact stepped/connected samples without animated panning.
- The 24-hour, 30-day, and 2-year historical ranges draw only when their data,
  size, theme, or inspection changes. They do not run a continuous RAF merely
  to move a subpixel fraction of a coarse bucket.

### Curve and scale behavior

- Each bucket becomes a bytes-per-second anchor at its end timestamp. The curve
  must pass through every displayed anchor.
- Use a shape-preserving piecewise cubic Hermite interpolation with monotone
  slope limiting. Each segment is clamped to the minimum/maximum of its two
  anchors and to zero, so the renderer cannot create negative rates or an
  overshoot that was never measured.
- A gap, reset, or stale boundary breaks the path; interpolation never crosses
  epochs or missing authoritative data.
- The y-axis starts at zero. It grows immediately to at least 110% of a newly
  visible maximum and decays toward 110% of the current window maximum with a
  two-second exponential half-life. The nonzero plotting floor is 1 KiB/s so an
  all-zero window has stable geometry. Scale animation follows the same
  visibility and reduced-motion rules.
- Grid and label selection are pure deterministic functions of dimensions and
  the current scale. Units use binary byte-rate labels consistently with the
  rest of the application.

### Canvas, interaction, and accessibility

- Canvas backing size follows measured CSS size and device pixel ratio capped
  at `3`. `ResizeObserver` and theme changes schedule one coalesced draw.
- Payload received is the dominant solid line with a restrained fill; staged
  write is a thinner dashed line without fill; verified payload is a distinct
  solid/patterned line without fill. A shaded band between receive and write is
  forbidden because it could be mistaken for queued-byte volume. Color is not
  the only series cue.
- The default range is 30 seconds. A compact segmented control keeps 30
  seconds, 2 minutes, and 10 minutes immediate; a History control selects 1
  hour, 24 hours, 30 days, or 2 years. Available primary series can be toggled,
  and range/visibility are local presentation preferences. There is no free pan
  or zoom.
- Pointer movement and keyboard left/right inspection snap to the nearest exact
  bucket, never the interpolated pixel value. The crosshair/tooltip names the
  series, exact rate, exact bytes, interval, and relative time.
- Adjacent semantic summaries expose current, selected-window average, peak,
  total observed bytes, availability, and stale state. Current is the exact
  trailing one-second byte sum from completed 100 ms buckets, independent of
  the selected historical range. Average is visible bytes divided by visible
  covered duration, excluding explicit gaps; peak is the maximum exact bucket
  rate in that window. They are not updated as an assertive live region on
  every frame.
- The pane is labeled **Session · All torrents** so selection of a torrent
  cannot appear to filter it. Upload is an explicit unavailable summary, not an
  empty line. Protocol, DHT, tracker, metadata, waste, and logical hash-read
  categories use a separate selectable breakdown rather than a competing
  primary y-axis.
- Empty/idle history, unavailable series, disconnected transport, stale data,
  reset, and overflow use the shared inspection-state vocabulary.
- Compact layouts stack summaries above a full-width plot and move the legend
  below it. Labels do not overlap or require horizontal page scrolling.

## Stable Scenarios And Shape-Changing Cases

Permanent scenarios include `speed-steady`, `speed-bursty`, `speed-idle`,
`speed-hash-retry`, `speed-traffic-breakdown`, `speed-history`,
`speed-unavailable-upload`, `speed-stale`, and `speed-reset`. Their monotonic
clock, UTC anchor, and RAF driver are deterministic; screenshots do not depend
on wall time.

Implementation and tests must cover:

1. exact boundary assignment at every tier, exact child-to-parent sums, and
   wraparound at every capacity without averaging averages;
2. multiple block events in one bucket, idle zero buckets, a long delayed tick,
   and saturating counter behavior;
3. history continues while no Speed lease exists and reopens with the bounded
   recent window while its 100 ms task has remained parked;
4. received payload precedes staging, staging precedes verification, and a hash
   failure advances received/write/waste but not verified before a retry counts
   its real work again;
5. peer wire/protocol/metadata classification is bounded and coherent, while
   DHT and tracker byte totals match exact controlled datagrams;
6. cancellation, completion, removal, and replacement generations add no
   duplicate accepted bytes and settle to exact zero buckets;
7. `metrics.db` clean restart continuity, retention pruning, bounded batched
   writes, unclean active-batch gap, corruption isolation, and `session.db`
   noninterference;
8. range-selective snapshots never send unrequested tiers; range changes,
   patch coalescing, reset, epoch replacement, disconnect, and lease recovery
   reconstruct exact state;
9. cubic output passes through anchors, remains nonnegative/in-range, and
   breaks at gaps/resets;
10. RAF starts/stops exactly with visibility, mount, interest, historical
    range, and staleness and performs no React state update per frame; and
11. reduced motion, keyboard inspection, high DPR, resize, largest single-tier
    rendering, and traffic-breakdown selection remain exact and bounded.

These cases establish counting, retention, epoch, animation, and truthfulness
and must land with the common path.

## Staged Implementation And Gates

1. **Reference and accounting taxonomy.** Reconfirm the libtorrent and
   JSTorrent dossiers, instrument exact peer, DHT, tracker, storage, integrity,
   redundancy, and verification byte boundaries, and record deliberate
   differences. Prove category and no-double-count invariants first.
2. **Pure history.** Implement fixed live/durable rings, exact child rollup,
   UTC/monotonic epoch mapping, gap coverage, rollover, fast-forward, zero,
   precision, and saturation behavior without Tokio or SQLite.
3. **Owner lifecycle and persistence.** Connect typed byte events, add the
   parkable clock and bounded metrics writer tasks, implement `metrics.db`, and
   prove cancellation/join, clean/unclean restart, corruption isolation,
   retention without view interest, and no `session.db` contention. Measure
   task wakeups, database write rate, retained bytes, and memory high water.
4. **Application contract.** Add capability/range-selected view,
   snapshot/patch, generated artifacts, strict bounds, reducer, reset,
   reconnect, and lease recovery. Rust and contract tests gate rendering work.
5. **Renderer.** Implement pure scales/geometry/interpolation, imperative
   Canvas/RAF ownership, exact inspection, stale handling, reduced motion, and
   deterministic scenarios.
6. **Controlled live proof.** Run loopback transfers with controlled storage
   delay and one hash-failure retry, verify primary, waste, peer-protocol, DHT,
   and tracker totals against owner truth, close/reopen and change range
   mid-transfer, verify payload integrity, restart coarse history, and join all
   owners.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Every boundary and rollover, child sums, exact bytes, idle/fast-forward, precision, UTC/monotonic epochs, gaps, and caps. |
| Accounting | Peer wire/protocol/metadata invariants, exact DHT/tracker datagrams, logical write/hash semantics, redundant bytes, and hash-failure retry. |
| Persistence | Separate-schema migration, clean/unclean restart, bounded write cadence/rows, pruning, corruption isolation, and main-store noninterference. |
| Contract | Range-selective snapshot/patch/type/schema generation, unsupported upload, invalid/oversized rejection, reset/coalescing/lease reducer tests. |
| Scripted runtime | No-interest retention, parked tick, lifecycle zero, cancellation/replacement exactness, both task shutdown paths, and memory/wakeup/write high-water marks. |
| Renderer | Curve-anchor/no-overshoot properties, time anchoring, scale half-life, stale freeze, historical static draw, visibility, reduced motion, DPR/resize, and no React render per RAF. |
| Web UI | Component, accessibility, keyboard, light/dark, narrow/standard/wide, all ranges, idle/stale/gap/reset, and maximum selected-tier visual tests. |
| Controlled interoperability | Headless loopback with delayed storage and hash retry; exact traffic/write/verified/waste totals, reopen/range continuity, restart, SHA-1 equality, and joined cleanup. |
| Platform | Rust workspace baseline, production web build, and proportional Tauri/Android generated-contract compilation. |
| Public live evidence | Not authorized or required. |

Performance evidence records native and on-disk retained bytes, row count,
SQLite transaction/WAL bytes, persistence queue high water, maximum serialized
selected-range snapshot and patch size, ordinary/max Canvas draw time, RAF
count while visible/hidden/historical, React render count, rate-task wakeups,
and application memory high water. These are observations, not new throughput
gates unless a later tactical authorizes one.

## Escalation Contract

When activated, ordinary internal refactoring, the closed metric catalog,
true-boundary byte instrumentation, the bounded clock and metrics-writer tasks,
the separate `metrics.db`, generated contracts, local Canvas code,
deterministic clocks, controlled storage/hash failures, and conservative limit
tightening are authorized. Stop for direction before adding a dependency,
putting metrics in `session.db`, recording endpoints or identities, adding an
arbitrary metric/event API, exporting telemetry, changing a metric meaning or
retention tier, changing engine scheduling or storage policy, launching a
visible client, using public network traffic, or establishing a new
performance gate.

## Next Boundary

Upload becomes a real series only with an upload/seeding tactical. Per-torrent,
per-peer, physical-device IO, disk-latency, telemetry export, and arbitrary
history navigation remain separate product and resource decisions.

Startup, clean-shutdown intent and completion, detection of a previous unclean
stop, application-version change, schema-migration start/outcome, and similar
facts are discrete operational events rather than byte-rate buckets. They may
eventually justify a bounded typed operational history in the installation-
wide `product.db` accepted by
[`../topics/product-state-and-feedback.md`](../topics/product-state-and-feedback.md).
That topic and a future tactical decide latest-value versus bounded-history
shape, retention, privacy, crash semantics, and backup/export policy. This
tactical must not smuggle those events into metric series. The accepted
missing-detail-tab sequence ends here.
