# Tactical 066: Smooth Session Speed History

Status: Planned; direction accepted on 2026-08-03. Implementation has not
begun.

Topics: `application-view-api`, `web-ui-design`,
`desktop-inspection-surface`, `disk-and-piece-inspection`,
`performance-and-live-evidence`, `capability-readiness`

## Motivation

RSTorrent exposes truthful current receive and storage rates, but the Speed tab
is an empty scaffold and no history survives while it is closed. JSTorrent's
uPlot implementation demonstrates the usefulness of a live speed chart, but
its update path and visual behavior are not the product direction here. A
general chart dependency would add weight while still requiring custom timing,
staleness, interpolation, accessibility, and theme behavior.

This slice adds a small session-owned round-robin history and a hand-rolled
high-DPI Canvas renderer. Exact byte observations remain discrete and bounded.
The visible chart moves on `requestAnimationFrame`, one sample behind the
latest complete point, so it can interpolate between known anchors and pan
smoothly without claiming RAF-rate measurements. When delivery stops it freezes
and becomes stale; it never invents a glide to zero.

## Desired Outcome And Stopping Condition

The session-scoped Speed tab opens with recent history even when it was closed
during the transfer. It offers 30-second, 2-minute, and 10-minute windows for
payload download and completed disk writes, shows upload as unavailable rather
than zero, and renders a smooth nonnegative curve through exact sample anchors.
RAF motion is limited to presentation, pauses when hidden, honors reduced
motion, and does not drive React renders. Hover and keyboard inspection snap to
the nearest exact sample.

The tactical stops when the pure multi-tier history owner, application lifetime
and shutdown path, named session view, generated contracts, hand-rolled chart,
deterministic scenarios, resource evidence, and controlled loopback transfer
all pass. No chart dependency is added.

## Dependencies And Sequence

- Tacticals `033`, `044`, `045`, `048`, `052`--`054`, and `060` provide view
  delivery, exact receive/store observations, the Canvas precedent, bounded
  storage throughput, and shared transport behavior.
- Tactical `064` centralizes tab scope and Tactical `065` proves the following
  session tab through it. This tactical is third in the accepted sequence.
- The implementation must read
  [`../topics/performance-and-live-evidence.md`](../topics/performance-and-live-evidence.md)
  before collecting any performance evidence. Public live evidence remains
  out of scope.

## Scope

- Add a runtime-independent `SessionRateHistory` projection that records exact
  byte counts from accepted `BlockReceived` and completed `BlockStored` activity.
- Retain three fixed monotonic-time tiers per available series:
  `100 ms x 300`, `500 ms x 240`, and `2 s x 300`.
- Add one application-owned 100 ms clock task that closes elapsed buckets,
  including known zero-byte intervals, while the application runs whether or
  not the Speed tab is leased.
- Add capability `session_speed`, `ViewSpec::SessionSpeed`, bounded snapshots
  and append/upsert patches, generated TypeScript/schema/UniFFI/Kotlin
  contracts, strict browser decoding, and reducer coverage.
- Render the default 30-second window and selectable 2-minute/10-minute windows
  with a purpose-built Canvas plot, current/average/peak summaries, exact-sample
  inspection, responsive legends, and explicit stale behavior.
- Add permanent deterministic scenarios plus accessibility, responsive,
  reduced-motion, theme, stale/reset, scale, and controlled live evidence.

## Non-Goals

- uPlot, D3, another general chart package, or a reusable chart framework.
- Pan, zoom, arbitrary time ranges, annotations, export, persisted history,
  cross-session history, database writes, or a generic metrics system.
- Upload/seeding implementation. Payload upload is explicitly unavailable and
  has no zero-valued samples.
- Hash-rate history, DHT traffic, protocol overhead, per-torrent charting,
  per-peer charting, disk latency plotting, or client-lifetime byte accounting.
- Changing the peer receive, storage, scheduling, or backpressure hot paths
  beyond a bounded task-free observation addition.
- Background animation while hidden, animation under reduced-motion, public-
  swarm throughput comparison, visible desktop launch, or native Android UI.

## Reference Dossier

### Metric semantics

BEP 3 does not define client rate sampling or chart behavior. The two initial
series are RSTorrent product observations:

- **Download payload** counts accepted peer block payload emitted exactly once
  as `BlockReceived`; and
- **Disk write** counts block payload emitted exactly once after successful
  storage as `BlockStored`.

They exclude protocol framing, retransmission not accepted by the block owner,
hash reads, checkpoint metadata, and DHT traffic. Presentation and contract
labels must retain these meanings.

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
accepted payload and `BlockStored { length }` after storage completion. The
application activity sink maps both through the view hub, so exact byte deltas
can be accumulated without sampling peer rows, subtracting reset-prone totals,
or adding payload to the application boundary. Current Disk rates remain
useful instantaneous diagnostics but are not retained history.

The concrete boundary improvement is one pure bounded session projection fed
by typed byte events. It has no Tokio, filesystem, socket, JSON, React, or
Canvas dependency. A tiny application-owned clock closes its buckets; the
engine does not depend outward on session history.

## Owner, Task, Cancellation, And Data Flow

```text
content/storage owners
  | BlockReceived(length) / BlockStored(length), no payload bytes
  v
existing synchronous application activity sink
  |
  v
ViewHub-owned SessionRateHistory (pure fixed rings)
  ^
  | advance monotonic clock every 100 ms
ApplicationService-owned rate clock task
  |
  v
session_speed snapshot / bounded bucket patches
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
adds the event length to the current bucket in each relevant tier. This avoids
a stale timestamp racing a bucket close.

`ApplicationService` owns exactly one interval task, cancellation token, and
join handle. The task advances history every 100 ms using a monotonic clock and
notifies interested views only when at least one bucket closes. It uses skipped
missed-tick behavior and advances all elapsed fixed boundaries in one bounded
operation; it never queues overdue ticks. Application shutdown cancels and
joins it before the view hub is dropped. Opening or closing Speed changes view
interest only and does not start, reset, or stop history.

## History Semantics And Bounds

The initial tiers are fixed and not configurable:

| Window | Bucket width | Capacity | Nominal retained history |
| --- | ---: | ---: | ---: |
| Recent | 100 ms | 300 | 30 seconds |
| Short | 500 ms | 240 | 2 minutes |
| Session detail | 2,000 ms | 300 | 10 minutes |

All tiers align to one application-session monotonic epoch. A byte event is
added independently to the one current bucket in each tier, so each tier is an
exact alternative aggregation: every observed byte appears exactly once per
tier. Bucket intervals are half-open `[start, end)`; an event at a boundary
belongs to the new bucket. Only completed buckets cross the view boundary.
Bucket placement uses the application's synchronous observation time, not an
invented wire-arrival or filesystem-completion timestamp; that distinction is
part of the series definition.

An elapsed bucket with no matching event is an exact known zero because the
activity sink is synchronous with accepted/stored byte ownership. If the clock
task is delayed, `advance` closes every elapsed boundary up to each ring's
capacity and fast-forwards older empty history without allocating per missed
tick. Event ordering through the serialized owner prevents a late byte from
being written into a completed bucket.

Initial available series are exactly two: download payload and disk write.
Each retains `300 + 240 + 300 = 840` buckets, for 1,680 maximum stored points.
The upload capability is represented as unavailable with no ring allocation or
samples. Adding any series, tier, capacity, or persistence later requires
rechecking the 512 KiB steady view-set budget and is outside this tactical.

Counters use saturating arithmetic. Bucket bytes and series totals use the
existing decimal-string JSON convention. Time is session-elapsed monotonic
milliseconds, never a manufactured wall-clock timestamp.

## View Contract

Add capability `session_speed` and `ViewSpec::SessionSpeed { view_id,
delivery }`. Conceptually:

```text
SpeedSeriesKind = payload_download | disk_write | payload_upload
SpeedSeriesAvailability = available | unavailable
SpeedTierKind = recent | short | session_detail

SpeedBucketView {
  bucket_start_millis,
  duration_millis,
  bytes,
}

SpeedTierView {
  tier,
  bucket_millis,
  capacity,
  buckets[],
}

SpeedSeriesView {
  kind,
  availability,
  unavailable_reason?,
  total_bytes,
  tiers[],
}

SessionSpeedSnapshot {
  lifecycle,
  captured_millis,
  history_epoch,
  series[],
}

SessionSpeedPatch {
  captured_millis,
  history_epoch,
  upsert_buckets[],
}
```

The exact patch bucket key is `(series, tier, bucket_start_millis)`. In an
ordinary 100 ms close it appends two recent points; on aligned 500 ms and 2 s
boundaries it may also append those tier points, for at most six normal entries.
If a client falls behind, existing whole-view coalescing may send a larger
bounded replacement or reset; it never creates an unbounded backlog.

`history_epoch` changes only when the application session/history owner is
recreated and forces client replacement. Unsupported, unavailable,
disconnected, stale, reset, overflow, and valid zero history are distinct.
Payload upload is `unavailable` with a short typed reason and empty tiers, not
an available series containing zeros.

Delivery for an interested Speed view is requested at 100 ms. The service may
coalesce, reset, or arrive late; `captured_millis` and exact bucket timestamps
remain authoritative. Closing the lease evicts the browser replica but not the
session rings.

## Canvas Rendering Contract

The chart is purpose-built code local to Speed, with pure geometry helpers and
one imperative renderer. React owns layout, controls, labels, and semantic
state; it does not own animation frames or a copied array on every frame.

### Time and RAF behavior

- The chart intentionally displays one selected-tier sample period behind the
  newest complete anchor: 100 ms, 500 ms, or 2 s. This permits interpolation
  only between two received exact values.
- On receipt, the renderer anchors session elapsed time to
  `performance.now()`. While visible, RAF updates the x transform so time pans
  continuously and advances interpolation through already received anchors.
- RAF never fabricates a future y value, mutates history, asks React to render,
  or changes the exact crosshair samples. If no bracketing anchor exists, the
  line holds the last exact anchor.
- If no fresh view capture arrives for one second or ten selected-tier sample
  periods, whichever is greater, the plot freezes at its last authoritative
  time and shows **Stale**. It does not decay toward zero.
- `document.visibilityState !== "visible"`, unmount, transport disconnect, or
  tab switch cancels the frame. Resume uses the next authoritative capture and
  never tries to replay missed animation frames.
- Under `prefers-reduced-motion: reduce`, there is no continuous RAF. The chart
  draws only on data, resize, theme, range, series, or inspection changes and
  presents exact stepped/connected samples without animated panning.

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
- Download payload and disk write use distinct theme tokens plus dash/pattern
  distinction. Color is not the only series cue.
- The default range is 30 seconds. A compact segmented control selects
  30 seconds, 2 minutes, or 10 minutes; checkboxes toggle available series.
  There is no free pan or zoom.
- Pointer movement and keyboard left/right inspection snap to the nearest exact
  bucket, never the interpolated pixel value. The crosshair/tooltip names the
  series, exact rate, exact bytes, interval, and relative time.
- Adjacent semantic summaries expose current, selected-window average, peak,
  total observed bytes, availability, and stale state. Current is the newest
  completed selected-tier bucket, average is visible bytes divided by visible
  completed duration, and peak is the maximum exact bucket rate in that window.
  They are not updated as an assertive live region on every frame.
- Empty/idle history, unavailable series, disconnected transport, stale data,
  reset, and overflow use the shared inspection-state vocabulary.
- Compact layouts stack summaries above a full-width plot and move the legend
  below it. Labels do not overlap or require horizontal page scrolling.

## Stable Scenarios And Shape-Changing Cases

Permanent scenarios include `speed-steady`, `speed-bursty`, `speed-idle`,
`speed-unavailable-upload`, `speed-stale`, and `speed-reset`. Their clock and
RAF driver are deterministic; screenshots do not depend on wall time.

Implementation and tests must cover:

1. exact boundary assignment at 100/500/2,000 ms and wraparound at every
   capacity;
2. multiple block events in one bucket, idle zero buckets, a long delayed tick,
   and saturating counter behavior;
3. history continues while no Speed lease exists and reopens with the bounded
   recent window;
4. payload received before disk completion produces an honest temporary gap
   between the two series;
5. cancellation, completion, removal, and replacement generations add no
   duplicate bytes and settle to exact zero buckets;
6. patch coalescing, reset, epoch replacement, disconnect, and lease recovery;
7. cubic output passes through anchors, remains nonnegative/in-range, and
   breaks at gaps/resets;
8. RAF starts/stops exactly with visibility/mount/staleness and performs no
   React state update per frame; and
9. reduced motion, keyboard inspection, high DPR, resize, and 1,680-point
   rendering remain exact and bounded.

These cases establish counting, retention, epoch, animation, and truthfulness
and must land with the common path.

## Staged Implementation And Gates

1. **Reference and pure history.** Reconfirm the dossier, implement the three
   fixed rings and exact event assignment, and prove rollover, fast-forward,
   zero, precision, epoch, and aggregation behavior without Tokio.
2. **Owner lifecycle.** Connect received/stored events, add the one clock task,
   and prove task cancellation/join plus history retention without view
   interest. Measure task wakeups and retained high-water memory.
3. **Application contract.** Add capability/view/snapshot/patch, generated
   artifacts, strict bounds, reducer, reset, reconnect, and lease recovery.
   Rust and contract tests gate rendering work.
4. **Renderer.** Implement pure scales/geometry/interpolation, imperative
   Canvas/RAF ownership, exact inspection, stale handling, reduced motion, and
   deterministic scenarios.
5. **Controlled live proof.** Run one loopback transfer with a controlled
   storage delay, verify series byte totals against engine/storage truth, close
   and reopen Speed mid-transfer, verify payload integrity, and join all owners.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Boundary assignment, all tier rollovers, exact bytes, idle/fast-forward, precision, epoch, and cap tests. |
| Contract | Snapshot/patch/type/schema generation, unsupported upload, invalid/oversized rejection, reset/coalescing/lease reducer tests. |
| Scripted runtime | No-interest retention, tick skipping, lifecycle zero, cancellation/replacement exactness, task shutdown, and memory/wakeup high-water marks. |
| Renderer | Curve-anchor/no-overshoot properties, time anchoring, scale half-life, stale freeze, visibility, reduced motion, DPR/resize, and no React render per RAF. |
| Web UI | Component, accessibility, keyboard, light/dark, narrow/standard/wide, idle/stale/reset, and 1,680-point deterministic visual tests. |
| Controlled interoperability | Headless loopback transfer with delayed storage, exact payload/write totals, reopen continuity, SHA-1 equality, and joined cleanup. |
| Platform | Rust workspace baseline, production web build, and proportional Tauri/Android generated-contract compilation. |
| Public live evidence | Not authorized or required. |

Performance evidence records retained bytes, maximum serialized snapshot and
patch size, ordinary/max Canvas draw time, RAF count while visible/hidden, React
render count, rate-task wakeups, and application memory high-water. These are
observations, not new throughput gates unless a later tactical authorizes one.

## Escalation Contract

When activated, ordinary internal refactoring, the one bounded clock task,
generated contracts, local Canvas code, deterministic clocks, controlled
storage delay, and conservative limit tightening are authorized. Stop for
direction before adding a dependency, persistence or telemetry export,
changing metric meaning, adding series/ranges/interactions, changing engine
scheduling or storage policy, launching a visible client, using public network
traffic, or establishing a new performance gate.

## Next Boundary

Upload becomes a real series only with an upload/seeding tactical. Hash, DHT,
protocol-overhead, per-torrent, per-peer, disk-latency, export, arbitrary
navigation, and persisted history each require separate product and resource
decisions. The accepted missing-detail-tab sequence ends here.
