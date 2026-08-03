# Client View Delivery Policy

Topic: `client-view-delivery-policy`

Status: The semantic application-view contract already accepts a per-view
`min_interval_millis`, and the live client already requests only the
projections implied by current navigation. The production web/Tauri adapter
currently hardcodes 100 ms for Library, Summary, Peers, Pieces, Disk and
Diagnostics and 250 ms for Files and Trackers. There is no user-selectable
delivery profile, visibility-driven downshift, global bandwidth budget, or
cadence-specific performance gate. Changing only a delivery interval also
causes an unnecessary fresh snapshot today. This topic records the client
policy required before those controls are implemented.

## Purpose And Scope

Application views can support a real-time inspection surface without making
every client, connection and lifecycle state pay that cost continuously. The
client owns which semantic views it currently needs and how fresh each needs
to be. The application service owns bounds, cursor continuity, coalescing and
truthful recovery.

This topic owns:

- client-visible delivery profiles and their mapping to per-view intervals;
- foreground, background and suspended-client delivery behavior;
- the relationship between desired views, delivery cadence and interaction;
- low-bandwidth and low-observer-cost policy;
- persistence and precedence of delivery preferences; and
- the evidence required to claim that a slower policy is less invasive.

[`application-view-api.md`](application-view-api.md) owns `ViewSpec`, view-set
lifecycle, cursor, snapshot, patch, reset and transport-neutral delivery
semantics. [`client-surfaces.md`](client-surfaces.md) owns platform lifecycle
and host adapters. [`web-ui-design.md`](web-ui-design.md) owns settings
presentation and paint scheduling.
[`performance-and-live-evidence.md`](performance-and-live-evidence.md) owns
hardware-specific throughput and observer-cost measurement.

This topic does not select WebSocket framing, a binary codec, compression,
relay authentication, or a public remote protocol. Those mechanisms must
preserve the policy recorded here rather than becoming competing cadence
authorities. Their accepted connection and multiplexing direction is recorded
in [`application-connection-architecture.md`](application-connection-architecture.md).

## Four Separate Controls

Do not collapse these independent controls into one "refresh rate":

1. **View interest** selects which projections the application retains and
   updates for this client. Removing an unneeded view is normally the largest
   resource and bandwidth reduction.
2. **Semantic delivery cadence** bounds how often a changed view may be
   emitted. `min_interval_millis` is minimum spacing between deliveries, not a
   request to manufacture periodic updates.
3. **Transport wait and heartbeat** determine how an idle connection detects
   closure and renews a lease. The current 20-second long-poll/stream wait is
   not the semantic update cadence.
4. **Paint cadence** determines when a client store change is rendered. Frame
   batching or hidden-document paint suppression does not alter cursor or
   delivery semantics.

A 100 ms semantic interval therefore means no more than approximately ten
deliveries per second for that view when it is changing. With no change there
is no substantive update. Multiple ready view changes may share one atomic
`UpdateBatch`.

Queue bytes are a safety bound, not a bandwidth throttle. Requesting a smaller
queue can cause more overflow resets and larger replacement snapshots, so it
must not be presented as a low-bandwidth control.

## Accepted Policy Direction

The client should expose a small named delivery preference rather than raw
milliseconds. Exact labels and calibrated values belong to an implementing
tactical, but the policy must distinguish at least:

- an interactive real-time profile for Workbench inspection;
- a balanced profile for normal observation;
- an explicitly low-bandwidth profile; and
- a background or suspended lifecycle state that can release expensive views
  independently of the user's foreground preference.

The effective policy is derived from all of:

```text
user delivery preference
        +
surface and visible detail
        +
foreground/background lifecycle
        +
projection-specific semantics
        =
desired ViewSpec set and intervals
```

The user's preference is a resource ceiling. Automatic lifecycle policy may
reduce delivery further but must not silently make an explicit low-bandwidth
preference more aggressive. Network cost heuristics such as metered-network
signals may be considered later, but must remain observable and overridable.

Illustrative, uncalibrated bands are:

| Policy | Current-state views | Expensive or ordered views |
| --- | --- | --- |
| Real-time | 100--250 ms | Only while visibly selected |
| Balanced | 250--1,000 ms | Slower and only while selected |
| Low bandwidth | 2--5 seconds | Omitted unless explicitly opened |
| Background | 5--30 seconds or release the view set | Release detail and diagnostics |

These ranges are design inputs, not performance claims or stable API values.
Hardware profiles and controlled transport measurements must calibrate them.

## Interest Before Cadence

The live adapter already maps responsive navigation to semantic desired
views. It may retain the Library, the selected Summary and one selected detail
on a wide Workbench surface; phone and collection-only surfaces retain less.
That behavior remains the first resource-control layer.

A low-bandwidth policy should not merely send every possible projection more
slowly. It should normally:

- retain only the collection or summary required by the visible surface;
- request Peers, Files, Trackers, Pieces or Disk only while that detail is
  visible;
- disable or narrow Diagnostics unless the user deliberately opens capture;
- preserve presentation navigation locally without retaining evicted engine
  projections; and
- close or substantially reduce a background view set when no fresh state is
  required.

Large initial and reset snapshots remain possible even at a slow cadence.
Files and ordered Diagnostics are particularly important: cadence cannot make
a required catalog snapshot small, and ordered records cannot be silently
latest-value coalesced like current-state counters.

## Interaction And Responsiveness

Commands return bounded receipts independently from application views. The
view feed remains authoritative for the resulting state. At a multi-second
delivery interval, the command receipt may therefore arrive before the next
visible projection update.

The current `requestImmediatePoll` only wakes an HTTP pull; it does not bypass
the server's per-view minimum interval and has no separate meaning for the
Tauri stream. A future interaction acceleration must be an explicit semantic
choice, not an accidental transport behavior. Acceptable directions include:

- honor the selected low-bandwidth interval strictly and show pending command
  state from the receipt;
- allow one bounded targeted flush after an accepted command; or
- permit a short interactive burst only in profiles that authorize it.

Do not implement a burst by repeatedly rewriting delivery policies. In the
current implementation any changed `ViewSpec`, including an interval-only
change, enqueues a fresh snapshot. The service should distinguish projection
identity or selector changes from delivery-policy changes so an interval can
be updated in place without resnapshotting or losing cursor continuity.

## Backpressure And Recovery

Cadence does not replace acknowledgement. One view set retains at most one
emitted, unacknowledged batch while compatible pending current-state changes
may coalesce behind it. A slow client therefore applies backpressure rather
than receiving an unbounded message stream.

Pending state and ordered events remain bounded. If the client is too slow or
the accumulated representation exceeds its queue, the server reports an
explicit reset and sends coherent snapshots under the separate snapshot
ceiling. Low-frequency policy must measure reset rate as well as bytes and
batches: a nominally slow configuration that repeatedly replaces large
snapshots is not low bandwidth.

Transport reconnect preserves the last applied cursor when the view set and
retained batch still exist. Lease expiry, cursor mismatch or process restart
recovers from a new epoch and coherent snapshots. Delivery preference and
semantic desired views may be retained as client presentation policy; view
set identifiers, cursors and materialized engine state remain volatile.

## Transport Independence

Pull, long-poll, in-process stream, future browser WebSocket and future relay
delivery must consume identical `UpdateBatch` values and acknowledge the same
applied cursor. A transport may multiplex many view sets and commands on one
connection, but it cannot invent another subscription model, sampling rule or
loss behavior.

Semantic cadence should be enforced before transport encoding so JSON, future
binary encoding and relay delivery observe the same policy. Transport
heartbeats and reconnect backoff are negotiated connection concerns and must
not create empty semantic revisions or change per-view delivery intervals.

## Current Implementation

The current live web/Tauri adapter constructs intervals in
`LiveApplication.viewSpecs`:

- 100 ms: torrent list, selected summary, peers, piece activity, session Disk
  and Diagnostics;
- 250 ms: Files and Trackers.

`ViewController` owns one pull or stream consumer, cursor application,
retry/reopen and cancellation. HTTP uses a 20-second long poll that returns as
soon as a batch is ready. Tauri uses an acknowledged Channel stream. Neither
surface exposes the delivery policy to a user, and visibility wake hints do
not currently downshift a hidden client.

The server accepts zero through 60,000 ms per view, retains a 256 KiB default
steady-state queue capped at 512 KiB, and treats coherent snapshots separately
under a 16 MiB ceiling. These are resource limits, not recommended client
profiles.

Tactical `057` supplies the first producer-throughput matrix for production
view combinations and serialized bytes. It proves that observer selection and
consumer behavior are measurable, but it does not yet compare server-enforced
delivery intervals, hidden-client policy or remote transport byte rates.

## Required Evidence

An implementing tactical should retain, for at least idle, Library, ordinary
selected-detail and adversarial all-view cases:

- produced application throughput relative to no observer;
- semantic batches and nonempty batches per second;
- encoded payload and framing bytes per second;
- queue and snapshot high-water marks;
- reset count and reset payload bytes;
- command receipt latency and authoritative-view convergence latency;
- client validation/reduction time and visible paint latency where relevant;
- foreground-to-background transition and restoration without stale patches;
  and
- reconnect at the last applied cursor plus lease-expiry snapshot recovery.

Run the matrix through HTTP long polling and Tauri streaming initially, then
through any WebSocket or relay adapter before claiming transport equivalence.
Named hardware profiles should carry generous calibrated floors or ratios;
noisy hosted CI may use deterministic byte, batch, continuity and reset gates
instead of a narrow wall-clock throughput threshold.

## Recommended Next Work

Create one bounded tactical that:

1. introduces a typed client delivery preference separate from semantic
   `DesiredInspectionViews`;
2. maps preference, visible projection and lifecycle to effective intervals;
3. updates interval-only `ViewSpec` changes without a replacement snapshot;
4. implements foreground/background transition and persisted user preference;
5. selects and tests an explicit post-command responsiveness rule;
6. adds cadence rows to the Tactical `057` observer harness; and
7. records calibrated product defaults only after measuring bytes, resets,
   throughput and convergence latency.

Do not select a binary codec, add a general remote service, or redesign view
patches merely to implement this policy.
