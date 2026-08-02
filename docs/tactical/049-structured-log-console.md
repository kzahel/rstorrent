# Structured Log Console

Status: Planned.

Topics: `desktop-inspection-surface`, `web-ui-design`,
`application-view-api`, `application-control`, `client-surfaces`,
`performance-and-live-evidence`

## Motivation

RSTorrent already emits bounded typed diagnostics and can deliver a filtered
diagnostics projection through leased view sets. The legacy direct-DOM client
exposes profiles, severity, category, scope, search, autoscroll, dropped
counts, and copy. The new React inspection application instead reduces those
events to a shallow `LogRow`, discards stable event codes and structured
context, maps trace into debug, implicitly requests only the selected
torrent's records, and presents the result as a sortable data table.

That scaffold conflicts with the accepted product direction. Logs form an
ordered explanatory timeline, not another collection whose rows become more
meaningful when sorted by a column. Maintainers need a Chrome DevTools-like
console that preserves sequence, makes structured context inspectable, can
select expensive capture deliberately, and remains useful at high event rates
without turning arbitrary Rust internals or formatted `tracing` lines into an
application contract.

Build the first production Logs experience around the existing diagnostic
authority and Tactical `048`'s shared HTTP/Tauri view delivery. This slice
normalizes the record and capture-interest contract where evidence requires
it, separates producer capture from client-side display filtering, implements
one virtualized ordered console, and proves explicit loss and recovery. It
does not attempt comprehensive instrumentation of every engine subsystem.

## Outcome

When complete:

- Logs is a session-scoped global tab that works with or without a selected
  torrent;
- records remain in authoritative sequence order and cannot be column-sorted;
- every row exposes severity, hierarchical category, optional torrent and
  other stable subjects, a concise human message, and deliberately projected
  structured fields;
- Normal is the low-volume default, while Detailed and Trace capture are
  explicit and visibly more expensive;
- text, severity, category, and torrent display filters never reorder records
  or cause subscription churn;
- capture profile and capture scope alter producer interest through the
  existing desired-view contract rather than merely hiding transported data;
- scroll-follow, new-record indication, local clear, expansion, and copying
  behave like an operational console;
- history and queues remain byte- and count-bounded, and every unrecoverable
  sequence gap is visible in its true timeline position;
- HTTP polling and acknowledged Tauri streaming consume identical log batches
  through the same reducer and Zustand transaction; and
- no log history is persisted across application restart.

## Existing Foundation And Corrections

Tactical `012` established `DiagnosticEvent`, severity, flat categories,
Normal/Detailed/Trace profiles, a typed fallback summary and context pairs,
pre-transport filtering, bounded subscriber queues, and explicit dropped
counts. Tactical `033` included diagnostics in the leased view-set contract.
Tactical `048` made HTTP pull and acknowledged Tauri stream delivery
interchangeable behind one controller and reducer.

The current implementation has useful foundations to retain:

- `ViewHub` is the application diagnostic authority and assigns monotonic
  session-local sequences and wall-clock timestamps;
- event text and context are bounded and sanitized before retention;
- the source ring is capped at 512 events and 192 KiB;
- subscriptions and view sets filter before their transport queues;
- diagnostics use ordered append patches rather than current-state keyed
  conflation;
- a view-set delivery interval already coalesces publication into bounded
  batches; and
- generated Rust, TypeScript, JSON Schema, Kotlin, legacy adapter, HTTP, and
  Tauri boundaries already understand diagnostics.

This slice must correct these concrete problems:

- `DiagnosticCategory` is a closed flat enum, preventing additive category
  hierarchy without regenerating every client;
- `DiagnosticField` makes every value an untyped string and does not
  distinguish stable subjects from expandable facts;
- cumulative source-ring eviction is presented as if the active client lost
  records, while local retention and transport/reset loss are also conflated;
- every event is formatted before the hub knows whether any active capture
  interest needs expensive detail;
- the React mapper drops `code` and `context` and converts trace to debug;
- the inspection store retains only 256 mapped rows while the view reducer
  retains another independently bounded copy;
- Logs is requested only when a torrent is selected, despite being declared a
  session-scoped tab; and
- the generic `VirtualTable` enables sorting and column concepts that violate
  ordered-feed semantics.

Do not keep a behavior merely because the legacy client implemented it. Do
not replace the existing authority with a second tracing subscriber, frontend
logger, or platform log bridge.

## Reference Dossier

### RSTorrent

- [`012-bounded-diagnostics-progress.md`](012-bounded-diagnostics-progress.md)
  owns the first typed diagnostic contract and cross-surface evidence.
- [`033-headless-view-set-foundation.md`](033-headless-view-set-foundation.md)
  owns the leased view-set, reducer, and generated application API.
- [`048-unified-view-delivery-and-tauri-migration.md`](048-unified-view-delivery-and-tauri-migration.md)
  owns interchangeable pull/stream delivery and post-application Tauri
  acknowledgement.
- `crates/rstorrent-session/src/views.rs` currently owns diagnostic types,
  filtering, retention, publication, and older reactive subscribers.
- `crates/rstorrent-session/src/view_sets.rs` adapts diagnostics into leased
  desired views and bounded snapshot/append delivery.
- `clients/web/src/legacy-main.ts` is the existing product-behavior reference
  for capture profile, scope, severity, categories, search, follow, and copy.
- `clients/web/src/inspection/components/DetailPane.tsx` contains the sortable
  Logs scaffold to replace.
- `clients/web/src/inspection/live/LiveApplication.ts`,
  `view-set-reducer.ts`, and `inspection/state.ts` reveal the current mapping,
  duplicate retention, and selected-torrent assumptions.

### JSTorrent

The sibling checkout was inspected at
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/ui/src/tables/LogTable.solid.tsx` provides a compact virtualized
  chronological feed, filter bar, bottom-follow behavior, and bounded-DOM
  product reference;
- `packages/ui/src/tables/LogTableWrapper.tsx` demonstrates the React/Solid
  coordination layer that RSTorrent deliberately does not inherit; and
- `packages/engine/src/logging/logger.ts` and
  `docs/archive/engine/designs/archive/DESIGN-logger.md` demonstrate component,
  engine, torrent, and peer scoping plus producer-side filtering, but also
  retain arbitrary variadic arguments and formatted prefixes that are not an
  acceptable RSTorrent application contract.

Useful behavior to adopt independently: chronological virtualization,
bottom-follow only when already at the bottom, capture filtering before
expensive output, stable component/instance identity, and compact density.
Deliberate differences: pure React/TypeScript presentation, no mixed runtime,
no arbitrary argument serialization, no internal class-name authority, and a
recoverable leased semantic view rather than direct access to a JavaScript
engine store.

### Libtorrent completeness oracle

The pinned libtorrent revision is
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/alert.hpp` defines independently selectable alert
  categories including status/error, peer/connect, tracker, storage, DHT,
  progress, and high-volume logging families;
- `include/libtorrent/alert_types.hpp` keeps typed alert identity and fields
  distinct from human `message()` text and defines explicit dropped-alert
  reporting;
- `src/alert_manager.cpp` enforces bounded pending alerts and posts the
  dropped-alert meta event;
- `src/peer_connection.cpp` and `src/request_blocks.cpp` check
  `should_post<...>()` before constructing optional peer and picker
  diagnostics; and
- `test/test_alert_manager.cpp`, `test/test_alert_types.cpp`, and
  `test/settings.cpp` cover masks, category assignments, finite queue
  overflow, dropped reporting, and enabled/disabled publication.

RSTorrent adopts typed identity, pre-format interest checks, bounded queues,
explicit loss, and separate coarse/high-volume categories. It does not copy
libtorrent's bitmask, inheritance hierarchy, category names, alert IDs, or
application architecture. No source or fixture is imported.

## Accepted UX

### Ordered console, not a sortable table

Implement a dedicated `LogConsole`, not a configuration of `VirtualTable`.
Records always appear by ascending diagnostic sequence with the newest at the
bottom. The console has no sortable headers, column-resize affordance,
live-resort preference, or row order derived from timestamps. Sequence is the
ordering authority; timestamps are display context and may repeat or move
with wall-clock adjustment.

One compact collapsed entry contains:

```text
06:42:18.371  WARN  tracker.announce  Big Buck Bunny  UDP response timed out
```

The visual structure is flexible rather than a rigid data grid:

- timestamp with millisecond precision;
- severity icon, text, and color that remain understandable without color;
- hierarchical category badge;
- zero or more stable context chips, beginning with torrent identity;
- concise human message; and
- a disclosure affordance when structured subjects or fields exist.

Wide layout may align these elements like console columns. Compact and phone
layouts may wrap category/context above the message while retaining sequence
and readable targets. User text selection is allowed.

Expanded content uses an accessible definition list of deliberately projected
subjects and typed fields. It never renders `Debug` output, arbitrary JSON,
module paths, source locations, backtraces, task handles, channel values,
engine structs, or payload bytes. Copy Message and Copy Structured Record are
per-entry actions. A bulk export or persistent diagnostic capture is deferred.

The local product does not apply enterprise-style authorization redaction to
legitimate diagnostic values. A deliberately projected endpoint, tracker, or
storage path is shown completely within its declared bound. Secrets,
credentials, full magnets, payloads, and ambient platform capabilities are
excluded at the producer boundary rather than captured and cosmetically
masked. A future remote product owns any separate exposure policy.

### Capture controls versus display filters

Make the difference visible:

- **Capture profile** (`Normal`, `Detailed`, `Trace`) changes producer
  interest and the leased diagnostics view. Trace is labeled high-volume and
  never enabled by default.
- **Capture scope** is `All torrents` or one explicitly selected torrent. It
  changes producer interest. A selected-torrent choice is pinned by torrent ID
  until the user changes it; merely selecting another library row must not
  silently retarget an active expensive capture. Torrent-scoped Detailed or
  Trace interest enables expensive records only for that torrent. The
  intrinsic global Normal baseline remains active.
- **Display filters** for text, minimum or selected severities, category
  prefixes, and torrent identity operate only over the bounded local history.
  They do not replace the view set or make uncaptured history appear to exist.

Selected-torrent display scope includes that torrent's records plus
session-global records, because network policy, storage pressure, and
application lifecycle may explain the torrent. It does not include records
attributed only to other torrents.

Normal is the startup default, captures session and all-torrent operational
records, and includes every warning/error even when its category is otherwise
outside the coarse profile. Detailed adds ordinary debug records for metadata,
protocol, scheduling, performance, and peer lifecycle. Trace adds explicitly
high-rate wire, block, picker, and packet diagnostics. Exact category-prefix
membership is a tested Rust policy table rather than UI knowledge.

If a display filter asks for a severity or category not present in the active
capture profile, the empty state says that those records are not being
captured. It must not imply that the engine produced none.

### Follow, clear, expansion, and accessibility

- Opening Logs initially positions the viewport at the newest retained entry.
- New records keep the viewport at the bottom only when it was already within
  a small tested bottom threshold.
- Scrolling away stops follow without stopping capture or ingestion. A
  persistent `N new` control returns to the bottom and reenables follow.
- Do not add a misleading Pause action that stops transport or retains an
  unbounded client backlog. Follow state is the presentation control.
- Clear advances a UI-local sequence watermark. It neither clears the Rust
  ring nor sends an application command. A later snapshot/reconnect cannot
  resurrect entries at or below that watermark during the same UI lifetime.
- Filtering and expansion retain the source order. No initial duplicate
  collapsing, grouping, or frequency summarization may hide retry cadence.
- Serious warning/error status and the `N new` count use restrained status
  announcements. The continuously changing virtualized feed is not a noisy
  `aria-live` region.
- Every disclosure, filter, chip, clear, copy, and follow action is keyboard
  accessible with visible focus. Virtualization cannot make focus disappear
  merely because new records arrive.

Log records, filters, expansion, clear watermark, and follow state are
session-only. This slice does not store them in the profile database or
browser persistence. Responsive layout changes retain them only in the live
application store.

## Semantic Record Contract

Evolve the internal v1 diagnostic record while no public compatibility is
claimed. Preserve these stable concepts:

```text
DiagnosticRecord
  sequence
  timestamp_millis
  severity
  category
  code
  torrent_id?
  message
  subjects[]
  fields[]
```

- `sequence` is a monotonic session-local decimal and the only ordering key.
- `timestamp_millis` is bounded wall-clock display context.
- `severity` remains `trace | debug | info | warning | error`.
- `category` becomes a bounded hierarchical identifier rather than a closed
  generated enum. It uses lowercase ASCII segments separated by `.`, at most
  64 bytes total, at most four segments, and no empty segment. Existing roots
  remain recognizable, for example `lifecycle.torrent`, `tracker.announce`,
  `peer.connection`, `peer.protocol`, `metadata.exchange`,
  `scheduler.request`, `piece.block`, `storage.write`, `integrity.hash`,
  `platform.android`, and `performance.backpressure`. Unknown valid categories
  render and filter normally in an older frontend.
- `code` is a stable bounded event identifier within the category. Clients may
  copy and filter it but never parse it to determine product state.
- `torrent_id` remains the common optional application identity and maps to a
  current display name in the client without repeating mutable names in every
  event.
- `message` replaces presentation terminology implying that arbitrary source
  data was summarized. It is a deliberately authored bounded human sentence.
- `subjects` is a small tagged list of stable identities appropriate for chips
  and filtering, initially limited to peer connection, tracker, piece, file,
  and owned-task identities. It contains no open handle or arbitrary object.
- `fields` is a bounded ordered list whose value is a tagged scalar such as
  text, boolean, decimal count, decimal bytes, duration milliseconds,
  endpoint, or error code. Presentation formatting follows the value kind.

The exact Rust enum/record layout may be simplified during implementation if
it preserves these semantics and generated validation. Limits are:

- at most four subjects;
- at most eight fields;
- keys/codes at most 48 ASCII bytes;
- messages at most 320 Unicode scalar values;
- individual bounded text values at most 240 Unicode scalar values; and
- an encoded record no larger than 4 KiB.

Although the wire value is forward-compatible text, Rust emitters use one
validated category newtype and centrally defined constants/prefix policy.
They must not scatter unchecked category literals or derive categories from
module/type names.

Reject invalid producer definitions in tests and sanitize bounded untrusted
values at the deliberate emission boundary. Do not silently stringify values
of an unsupported kind.

## Capture Interest And Ownership

Extract diagnostic record/filter/ring policy from the already large
`views.rs` into a focused session/application diagnostics module. The
application service remains the owner; `ViewHub` may expose a small facade so
existing emitters and view-set publication do not learn transport details.
Do not introduce a general logging framework or make protocol/engine domain
modules depend outward on view-set, Tauri, HTTP, React, or tracing types.

The owner maintains an aggregate `DiagnosticInterest` derived from all live
legacy subscribers and leased diagnostics views. Interest contains capture
profile, hierarchical category prefixes, severity, and optional torrent
scope. View replacement, explicit close, lease expiry, Tauri window
destruction, and shutdown update the aggregate promptly. An abandoned client
cannot leave Trace capture enabled after its lease expires.

The application itself contributes one intrinsic all-torrent Normal interest
so a newly opened console has a small useful operational tail even when no UI
was attached previously. Client interests may broaden that baseline to
Detailed or Trace and narrow their own delivered scope, but closing the last
client returns producer work to the Normal baseline rather than disabling
warnings and lifecycle history entirely.

Provide a cheap producer query or lazy emission operation so optional detailed
and trace records avoid message/context formatting when no active interest
could accept them. Always-on Normal facts may retain the direct emission path.
The interest check is an optimization only: bounds and filtering remain
authoritative if interest changes concurrently between check and publication.

This slice audits existing `record_diagnostic` call sites, maps them into the
hierarchy, and gives representative tracker, DHT/discovery, peer lifecycle,
metadata, piece/integrity, storage, performance, and application lifecycle
events useful structured fields. It does not authorize spraying a log call
through every function or adding all peer-wire, block, picker, DHT packet, and
filesystem details. New high-rate instrumentation requires its own evidence
or a small follow-up under this owner.

Rust `tracing`, stderr, platform logcat, browser console messages, panics, and
Tauri host logs may mirror deliberate diagnostics for developer convenience.
They are not ingested wholesale and do not become application truth.

## Retention, Batching, Loss, And Recovery

Use separate, explicit meanings:

- **source eviction** means older session history left the bounded application
  ring; an initial snapshot may mark the retained-history boundary;
- **delivery loss** means a client cannot reconstruct one or more sequences
  because its bounded queue/reset path overflowed; insert one ordered gap
  marker before the first surviving record;
- **local eviction** means the React console discarded its own oldest already
  observed records; report `showing latest N`, not a false transport drop; and
- **local clear** is the presentation watermark described above.

A single cumulative `dropped_count` must no longer ambiguously represent all
four conditions. Snapshot/patch values carry enough sequence and retention
metadata for the pure reducer to construct honest boundaries after initial
attach, replay, overflow reset, and lease replacement. A repeated batch or
lost response remains idempotent by diagnostic sequence.

Adopt these initial bounds:

- application recent history: at most 2,048 records and 2 MiB encoded;
- individual record: at most 4 KiB encoded;
- client materialization: at most 2,048 records plus explicit boundary/gap
  markers;
- diagnostic delivery cadence: 100 ms by default;
- one diagnostic patch: at most 128 records and 128 KiB; and
- existing whole-view-set 512 KiB steady queue, 16 MiB coherent snapshot, one
  Tauri unacknowledged batch, and lease bounds remain authoritative.

If measurement shows that these values violate the existing whole-set bounds,
lower the diagnostic limits rather than silently raising an unrelated global
limit. Record actual encoded and resident high-water marks in this tactical.

Appending one delivered batch may perform one bounded immutable store update.
Do not map, stringify, or rebuild expandable context for every retained record
on every paint. Display-filter evaluation over the 2,048-record cap is
acceptable initially and must be measured before introducing indexing. The
virtualized DOM contains only visible entries plus bounded overscan.

## Application And Frontend Boundaries

Extend `DesiredInspectionViews` with explicit log capture interest rather than
reading controls inside `LiveApplication`. Logs remains desired when the
session tab is active even if there is no selected torrent. Capture changes
replace the semantic diagnostics view through the existing controller;
display filters remain presentation state and do not alter desired views.

The generated Rust/TypeScript/schema contract remains the semantic source.
`ViewController`, `reduceUpdateBatch`, and Zustand remain shared across HTTP
and Tauri. Neither adapter adds a log-specific channel, polling loop, queue,
cursor, acknowledgement, or retry policy.

Use one canonical frontend diagnostic record shape through reduction and
presentation. Do not silently map trace to debug or discard code/context in an
intermediate `LogRow`. The console owns only derived visible indices,
expansion, follow, clear watermark, and display filters.

The demo application gains a permanent `diagnostic-console` scenario with
mixed severities, hierarchical categories, global and multi-torrent records,
structured expansion, a retained-history boundary, a delivery gap, hostile
bounded text, and enough entries to prove virtualization. Scenario updates
append in sequence and exercise new-record follow behavior.

## Owners, Tasks, And Cancellation

| Owner | State and work | Termination |
| --- | --- | --- |
| Application diagnostic owner | Sequence, bounded recent ring, category/profile policy, aggregate live interest | Joined application-service shutdown clears non-durable history |
| View-set diagnostics materialization | Capture interest, bounded snapshot/append accumulator, cursor continuity and loss metadata | Explicit close, lease expiry, owner cleanup, window destruction, or application shutdown |
| Pull/stream controller | Existing one-consumer delivery, validation, reducer application, retry and lease reopen | Existing Tactical `048` cancellation and joined close |
| Zustand inspection store | At most 2,048 records, boundary markers, capture/display controls, expansion, clear watermark and follow status | One React application lifetime; never persisted |
| `LogConsole` | Virtualized visible range, scroll anchoring, focus and entry actions | React unmount; owns no subscription or producer interest directly |

No component owns a network request, Tauri Channel, Rust task, or application
diagnostic ring. No diagnostic task or view-set handle enters Zustand.

## Shape-Changing And Adversarial Cases

- a global Logs tab opens with no torrent selected;
- the selected library torrent changes while capture is pinned to another
  torrent;
- Normal, Detailed, or Trace interest changes while events are being emitted;
- two view sets request disjoint categories/scopes, one expires silently, and
  aggregate interest drops only its contribution;
- an expensive lazy record is disabled, enabled, then disabled without
  formatting while disabled;
- source retention evicts records already delivered to one client but unseen
  by a newly attached client;
- delivery overflow resets after an unacknowledged Tauri batch or suspended
  HTTP client and produces exactly one honest gap;
- a replayed batch, repeated sequence, wall-clock reversal, or equal timestamp
  cannot reorder or duplicate entries;
- clear followed by reconnect or fresh snapshot does not resurrect entries
  below the local watermark;
- filters hide every row, then reveal the same original ordering without
  changing capture;
- scrolling one pixel outside the tested bottom threshold stops follow while
  ingestion remains bounded;
- expanded/focused rows survive append when retained and recover focus
  predictably when evicted;
- a 4 KiB maximum record, hostile bidi/control text, invalid category, too many
  fields, and unsupported typed value cannot escape bounds;
- 10,000 high-rate attempted records keep source, view-set, reducer, Zustand,
  and DOM high-water marks within their declared limits;
- close, view replacement, window destruction, lease expiry, and application
  shutdown race active capture without leaking interest or tasks; and
- HTTP polling and Tauri streaming reduce the same trace to byte-equivalent
  semantic console state before presentation-only controls.

## Implementation Order

1. Record this tactical, the accepted console UX, existing authority, exact
   references, limits, and non-goals.
2. Extract the diagnostic domain owner from `views.rs`; define hierarchical
   category validation, structured subjects/values, profiles, aggregate
   interest, lazy emission, distinct loss metadata, and bounded tests.
3. Adapt existing subscribers and leased view sets without adding another
   transport; prove per-interest filtering, aggregation, batching, replay,
   overflow, expiry, and joined cleanup.
4. Regenerate TypeScript/schema and any affected compatibility artifacts;
   update validators and pure reducer fixtures for record and gap semantics.
5. Carry capture interest through `DesiredInspectionViews` and keep the global
   Logs view active without torrent selection through demo, HTTP, and Tauri
   application adapters.
6. Replace the sortable scaffold with the virtualized `LogConsole`, structured
   expansion, capture controls, local filters, follow/new count, clear
   watermark, copying, responsive layout, and restrained accessibility.
7. Add the permanent diagnostic scenario and deterministic component/E2E
   cases, including bounded DOM and high-rate update measurements.
8. Extend the controlled live browser proof with representative structured
   tracker/metadata/peer/storage/integrity records, filter transitions, lease
   recovery, and exact payload verification; compile Tauri without launching
   it and test its adapter directly.
9. Run full repository gates, record resource high-water marks and evidence,
   update the owning topics/readiness queue, and commit logical slices.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Diagnostic domain | category/value validation, profile tables, aggregate interest, lazy non-formatting, count/byte eviction and distinct loss metadata |
| View-set state | initial retained boundary, append order, 128-record/128 KiB patch bounds, replay idempotence, explicit overflow gap, replacement, lease expiry and cleanup |
| Generated client | deterministic Rust-derived declarations/schema, hostile validation, exact pull/stream reducer convergence and no trace/context loss |
| Zustand/presentation | 2,048-record cap, display filters preserve order, pinned capture scope, clear watermark, expansion, copy, follow threshold and new count |
| Responsive browser | wide, compact and phone console geometry; keyboard/focus, contrast and restrained announcements; 10,000 attempted updates with bounded DOM |
| Controlled live | real application service and libtorrent seed emit useful structured lifecycle through verified completion; filter/profile change and lease recovery remain honest |
| Tauri | direct command/Channel tests plus production `tauri build --no-bundle`; no window launched |
| Repository | generated drift, formatting, Clippy with warnings denied, workspace tests, frontend typecheck/tests/build/E2E and temporary cleanup |

Automated validation uses named fixtures and the loopback headless browser
gateway. It must not launch or focus Tauri, a normal browser, an emulator, or
a physical device. Android UI parity is not part of this desktop/web slice,
though compatibility artifacts touched by shared semantic types must compile.

## Non-goals

- persistence of logs, capture profiles, filters, expansion, follow state, or
  clear history across application restart;
- JSONL/bulk export, diagnostic support bundles, upload, telemetry, crash
  reporting, or remote log collection;
- ingestion of generic Rust `tracing`, stderr, browser console, Android logcat,
  Tauri host output, panics, or backtraces into the semantic feed;
- arbitrary maps, JSON values, `Debug` strings, module/source locations,
  secrets, payload bytes, full magnets, credentials, platform capabilities,
  or unbounded paths in diagnostic context;
- comprehensive new peer-wire, block, picker, DHT packet, storage syscall, or
  performance instrumentation;
- sorting, column resizing, configurable columns, duplicate collapse,
  grouping, frequency aggregation, or navigation from a log to another tab;
- browser WebSocket view streaming, binary encoding, a local desktop server,
  or changes to Tactical `048` cursor/acknowledgement semantics;
- deletion of legacy subscriptions or Android diagnostic presentation;
- Android Logs UI parity, emulator, or physical-device evidence; or
- using diagnostics as command success, torrent state, progress assessment,
  correctness, or payload-verification authority.

## Stopping Condition

This slice is complete when the new React Logs tab is a global, responsive,
virtualized, strictly ordered console; stable structured records retain their
severity, hierarchy, code, identities, message, and expandable typed fields;
capture interest prevents disabled expensive formatting; Normal is the
default and Trace is explicit; display filtering does not replace capture or
reorder records; source eviction, delivery loss, local eviction, and clear are
truthfully distinct; pull and Tauri stream delivery converge through the same
controller/reducer/store; all owners and interests terminate on replacement,
expiry, window close, and shutdown; controlled live evidence remains payload-
verified; resource high-water marks satisfy the declared bounds; no history
is persisted; no visible client is launched by automation; owning topics and
evidence are current; logical commits are complete; and the worktree is clean.

## Escalation Contract

The diagnostic-domain extraction, internal semantic record evolution,
generated artifact updates, bounded interest aggregation, representative
existing-emitter cleanup, leased-view changes, console/store implementation,
demo/live headless evidence, compatibility compilation, topic updates, and
logical commits are authorized once implementation is requested. Stop for
direction if evidence requires persisting or exporting logs, ingesting generic
host/platform logs, adding a dependency, raising global view-set limits,
changing command or product-state semantics, implementing comprehensive new
high-volume engine instrumentation, changing Android presentation, selecting
a binary/remote transport, launching a visible/physical client, or expanding
beyond this console slice.

## Implementation And Evidence

Pending.
