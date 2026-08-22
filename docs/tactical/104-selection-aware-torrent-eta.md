# Selection-Aware Torrent ETA

Status: Complete (2026-08-07).

Topics: `application-view-api`, `capability-readiness`,
`desktop-inspection-surface`, `download-correctness`, `web-ui-design`

## Motivation And Outcome

Workbench has an ETA column and deterministic demos populate it, but live
torrent rows always project `null`. The current torrent view also exposes only
cumulative received bytes, verified piece count, and a short-window peer-sum
download rate. Those values cannot produce a truthful selective-download ETA:

- a wanted file makes every overlapping piece necessary, including real bytes
  belonging to an adjacent skipped file;
- BEP 47 padding in such a piece is synthesized locally and is not network
  work;
- verified-piece count is too coarse for accepted blocks in an unfinished
  piece; and
- cumulative received bytes include historical retries and therefore do not
  describe work retained by the current generation.

This slice adds one application-owned, selection-aware torrent ETA. The Rust
application view derives exact required and remaining peer-payload bytes,
maintains a small smoothed payload-rate estimator, and publishes a typed ETA.
The shared React product only validates, sorts, formats, and explains that
state in Transfers and Workbench.

Complete this tactical when live torrent rows expose exact selection-aware
work, a deterministic constant-space rate estimate, and typed warming,
estimated, stalled, and unavailable states; hash failure, restart, pause,
selection replacement, padding, and shared-boundary semantics are exact; the
per-event and periodic paths do not scan pieces, files, or blocks; and the
deterministic, maximum-geometry, generated-contract, web, controlled transfer,
and workspace gates pass.

## Scope

- Add a pure storage-layout calculation for the real peer payload covered by
  all pieces required by the current binary file selection.
- Reconstruct exact remaining work from durable verified state when metadata,
  selection, or the active torrent generation changes.
- Count uniquely accepted current-generation payload immediately, before
  verification, and restore failed-piece payload when its hash fails.
- Add one application-owned, monotonic, one-second ETA cadence and a
  constant-space integer exponential moving average (EMA).
- Project exact required bytes, exact remaining bytes, the smoothed rate used
  by ETA, and a tagged ETA state through every generated first-party contract.
- Replace the live frontend's `etaSeconds: null` placeholder with the typed
  projection and keep arbitrary-precision decimal strings intact.
- Display the same ETA in the clean Transfers queue and the existing
  Workbench ETA column with raw semantic sorting and accessible state text.
- Give the deterministic demo explicit examples of all ETA states without
  making React derive ETA from demo progress, size, or instantaneous rate.
- Record measured maximum-geometry construction cost and retained-state size
  before completing the tactical.

## Non-Goals

- Per-file ETA, aggregate queue ETA, or session completion time.
- High, medium, low, sequential, streaming, deadline, playback, or piece-level
  priorities. The existing `Normal`/`Skip` selection remains unchanged.
- Repairing the existing live Size or Progress columns. The exact work
  calculation is a foundation for that separate product slice, not permission
  to expand this one.
- Predicting metadata discovery, checking, publication, allocation, hashing,
  repair, seeding goals, or paused time.
- Including BitTorrent protocol overhead, TCP/IP overhead, duplicate or
  unsolicited payload, disk throughput, or future bytes that may be lost to a
  hash failure in the estimate.
- Confidence ranges, completion deadlines, historical charts, adaptive
  smoothing controls, user preferences, or persisted estimator history.
- Changing the existing instantaneous `payload_download_rate_bytes` field or
  the Down column that consumes it.
- A new networking-engine ETA API, React-side semantic calculation, Android
  Compose presentation, public-swarm evidence, visible desktop launch, or
  physical-device validation.

## Terminology And User Contract

- A **required piece** overlaps at least one wanted non-padding file byte.
- **Required payload bytes** are all real, non-padding torrent bytes in every
  required piece. This includes real skipped-file bytes in a shared boundary
  piece because the complete piece must be fetched and hashed.
- **Accepted payload bytes** are unique requested block bytes retained by the
  current engine generation. Redundant, unsolicited, rejected, and merely
  requested bytes are not accepted work.
- **Remaining payload bytes** are required payload bytes minus bytes in
  required verified pieces minus accepted bytes retained by unfinished
  required pieces in the current generation.
- The **ETA payload rate** is a smoothed rate of accepted required payload
  bytes. It is deliberately separate from the current peer-observation sum
  used by the Down column.

The generated torrent contract adds these required fields:

```rust
pub struct TorrentView {
    // existing fields ...
    pub required_payload_bytes: Option<String>,
    pub remaining_payload_bytes: Option<String>,
    pub eta_payload_download_rate_bytes: String,
    pub eta: TorrentEtaView,
}

#[serde(tag = "state", rename_all = "snake_case")]
pub enum TorrentEtaView {
    Estimate { seconds: String },
    WarmingUp,
    Stalled,
    Unavailable,
}
```

`required_payload_bytes` and `remaining_payload_bytes` are `null` only before
verified metadata supplies a safe layout. All-skipped selection is the known
value `"0"`, not `null`. The ETA rate is a decimal bytes-per-second string and
is `"0"` when the estimator is reset, warming, stalled, or inapplicable.
Estimated seconds use integer ceiling division and remain an exact decimal
string; no Rust or TypeScript floating-point conversion enters the contract.
The four fields serialize on every produced `TorrentView` and are required by
the regenerated schema. This first-party additive change remains API v1
because no stable public remote compatibility is claimed; Rust, schema,
TypeScript, fixtures, UniFFI, and Kotlin update atomically rather than adding
an omitted-field fallback or version fork.

The state rules are:

| State | Exact condition | Presentation |
| --- | --- | --- |
| `estimate` | A live content generation has remaining work, at least one completed nonzero sample, its activation or last accepted payload is newer than the stall threshold, and the smoothed rate is positive. | Compact duration such as `4m 12s`. |
| `warming_up` | A live content generation has remaining work but has not yet produced a usable completed sample and has not reached the stall threshold. | `—`, accessible text `Calculating ETA`. |
| `stalled` | A live content generation has remaining work and ten seconds have passed since activation or the last accepted payload. | `∞`, accessible text `Transfer stalled`. |
| `unavailable` | Metadata is absent; no content generation is active; selection is all skipped; the torrent is paused, checking, repairing, failed, publishing, removing, or complete; or remaining work is zero. | `—`, accessible text `ETA unavailable`. |

The existing typed progress assessment remains the explanation for why work is
not currently active. ETA does not duplicate its reason string.

## Required-Payload Geometry

`rstorrent-protocol::storage_layout` owns the pure geometry because it already
owns torrent offsets, file/padding segments, piece lengths, and the binary
`FileSelection`. Add a plain value/function that consumes a validated layout,
selection, and bounded have-state view and returns at least:

```text
required_payload_bytes
verified_required_payload_bytes
```

The calculation follows these rules:

1. Treat each wanted, non-padding, non-empty file as the half-open torrent
   interval `[start, end)`. Its required inclusive piece range is
   `start / piece_length ..= (end - 1) / piece_length`; coalesce adjacent and
   overlapping ranges in file order.
2. Walk those piece ranges and layout segments in ascending order. For each
   required piece, count its intersection with real file segments and exclude
   its intersection with padding segments.
3. Add the same non-padding piece contribution to the verified total only when
   the supplied have state marks that complete piece verified.
4. Reject inconsistent lengths, piece indices, have lengths, or arithmetic
   overflow rather than clamp a malformed geometry into a plausible ETA.

This is piece work, not selected logical file size. A selected one-byte file
may require almost two full real pieces; a large padding file inside those
pieces contributes zero peer payload. A final short piece contributes only its
real non-padding extent.

The implementation must not call `request_ranges()` for every piece, allocate
block plans, or retain a second piece bitmap. It may make one ascending pass
over the existing have slice and layout segments at construction. Coalesced
selection ranges are bounded by the existing 4,096-file metainfo limit and are
temporary or retained only if they replace equivalent existing selection
geometry rather than duplicating it.

## Runtime Accounting And Lifecycle

The application-view model retains only scalar ETA work after construction:

```text
required payload bytes
remaining payload bytes
bytes accepted since the last rate tick
smoothed rate
activation, last tick, and optional last-accepted monotonic instants
whether a usable sample exists
active torrent generation identity
```

Runtime transitions are constant-time:

- `BlockReceived { length }` is the engine's unique accepted-content event.
  For the matching active generation, subtract `length` from remaining work,
  add it to the current rate bucket, and update the last-accepted instant.
- `BlockStored` and `PieceVerified` do not subtract again. Verification changes
  durable truth but not the amount of retained current-generation work.
- Carry `failed_bytes` through the application activity mapping for
  `PieceHashFailed`. Add that exact planned non-padding payload back to
  remaining work. The engine already derives it from the failed piece's
  received request blocks.
- A stale event from a joined or replaced generation cannot mutate the new
  estimator. Generation mismatch fails closed and is covered by a race test.
- Pause, recheck, selection change, repair, terminal failure, or any content
  generation replacement first uses the existing cancellation/join fence.
  After the join, rebuild from verified have state and reset transient accepted
  bytes and the rate estimator before a successor becomes active.
- Restart reconstructs from durable verified pieces only. It never infers
  retained work from cumulative `received_bytes`, part-file contents, or stale
  view activity.
- A durable snapshot refresh that preserves ordinary counters and peer rows
  must not accidentally preserve ETA state across a torrent-generation or
  selection identity change. Preserve it only when that identity is exactly
  unchanged; otherwise reconstruct it explicitly.

Maintain and test `0 <= remaining <= required`. Underflow, overflow, a block
length beyond remaining, or `remaining + failed_bytes > required` is an
internal invariant failure with a bounded diagnostic, not a saturating
arithmetic policy. The engine's `failed_bytes` value is the authoritative
exact accepted payload for that failed piece; ETA adds no second per-piece
ledger merely to re-prove it. The user-visible view becomes unavailable until
the next authoritative reconstruction rather than publishing a false number.

### Concrete generation seam

Use an ETA-specific content-generation token; the existing long-lived
`TorrentRuntime::generation()` is too broad because file-selection replacement
reuses that runtime. `ViewHub` reserves a monotonically increasing nonzero
token before a `DownloadControl` receives its activity sink. The sink captures
that token, `ActiveDownload` retains it, and installation activates that exact
token. A failed installation deactivates the reservation. Pause, reap,
shutdown, and replacement deactivate the retained token after requesting
cancellation and no later than the task join.

Every ETA-mutating activity call supplies the captured token. `ViewHub`
accepts it only when it equals the model's active token. Tracker/discovery
sinks have no ETA token and cannot mutate ETA accounting.

`durable_view_state()` computes required/verified geometry before acquiring
the view-hub mutex and carries the two scalar totals in
`DurableTorrentViewState`. `ViewHub::replace_durable()` follows this exact
policy:

- first verified metadata initializes previously unknown geometry without
  invalidating an already active metadata/content token;
- an unchanged file selection preserves current-generation remaining/rate
  state across ordinary checkpoint refreshes, even as durable have advances;
- a changed selection, transition out of running metadata/content work,
  recheck/repair/removal, or inconsistent geometry invalidates the token,
  reconstructs remaining as `required - verified`, resets rate state, and
  publishes `unavailable`; and
- a successor reserves and activates a fresh token only after the existing
  cancellation/join path permits a new `ActiveDownload`.

Selection equality uses the existing `FileProgressModel` catalog/selection
state; ETA does not retain another file vector or probabilistic fingerprint.
This ordering handles the current command path, which commits and refreshes a
new selection before it joins the old engine generation: the refresh fences
the old token before any new geometry becomes mutable.

## Rate And ETA Derivation

Introduce one `TorrentEtaRuntime` (name may follow the local module vocabulary)
owned by the application-service generation. It has one cancellation token,
one task, and an awaited shutdown. One shared timer ticks all materialized
torrent ETA models; there is never one timer or task per torrent.

The deterministic estimator uses:

- a nominal one-second cadence with `MissedTickBehavior::Skip`;
- monotonic elapsed time supplied to a pure tick transition;
- an interval sample of `accepted_bytes * 1000 / elapsed_millis`, using checked
  `u128` intermediate arithmetic and actual elapsed time rather than assuming
  a perfect timer;
- the pinned-libtorrent integer EMA, `new = old * 4 / 5 + sample / 5`, after
  seeding the first usable nonzero sample directly and using `u128`
  intermediates before the checked `u64` result;
- zero samples in the EMA so a short traffic gap decays rather than making the
  ETA flap immediately; and
- an explicit ten-second interval since the last accepted byte that sets the
  public rate to zero and the ETA to `stalled`.

A late or skipped tick creates one elapsed-time-normalized sample; it does not
replay artificial one-second zero buckets. An elapsed interval of zero is
ignored. Rate state resets at every authoritative generation reconstruction
and is never persisted.

For a positive rate and positive remaining work:

```text
seconds = remaining / rate + (remaining % rate != 0)
```

This form avoids the overflow risk in `remaining + rate - 1`. The result is at
least one second. ETA and its smoothed rate publish at most once per tick;
accepted-block events update scalar source state but do not cause a separate
ETA-only patch for every block. Existing activity projections may still emit
their already-authorized changes.

Place the pure scalar state machine and joined cadence owner in
`crates/rstorrent-session/src/views/eta.rs`. `ViewHub` exposes narrowly scoped
reserve/activate/deactivate, activity, and tick methods; application startup
starts the single runtime after constructing `ViewHub`, and shutdown cancels
and awaits it after active downloads join but before view sets close.

The cadence must not use the common `hub.torrents.clone()` diff pattern because
that clones file, peer, swarm, and active-piece state. Under one hub lock,
capture only each changed torrent's small previous/new `TorrentView`, advance
the scalar models, then publish those torrent-view changes through a targeted
helper. Geometry construction likewise never runs while the hub lock is held.

## Owner, Task, And Cancellation Map

| State or work | Owner | Lifetime and termination |
| --- | --- | --- |
| File selection and verified have state | Existing durable application/store owners | Transactional selection and conservative verified-piece rules remain unchanged. |
| Required-payload geometry | Pure `rstorrent-protocol::storage_layout` value/function | Constructed from verified metadata and selection; no task or I/O. |
| Remaining-work and rate scalars | Per-torrent application-view model | Reconstructed at metadata/selection/generation boundaries; removed with the torrent row. |
| Accepted/failure deltas | Existing generation-fenced application activity sink | Accepted only for its torrent generation; ends before generation join returns. |
| ETA cadence | One application-service `TorrentEtaRuntime` | Starts after the view hub, cancels and is awaited during the existing service shutdown sequence. |
| Typed DTO and diff | Existing `ViewHub` and view sets | Published through ordinary snapshots, coalescing, reset, and delivery policy. |
| Formatting and table state | Pure shared React model/components | No timer, transport, rate calculation, or background owner. |

No task, mutable callback, clock, Tokio type, engine handle, or application DTO
enters `rstorrent-protocol`.

## Cost And Memory Contract

These are completion requirements, not optimization aspirations:

- Geometry construction is `O(P + F)` worst case for `P` supplied have bits
  and `F` layout segments/files, and occurs only when authoritative metadata,
  selection, or verified generation state is reconstructed.
- A `BlockReceived`, `PieceHashFailed`, state transition, and ETA derivation are
  `O(1)` and allocate nothing after model construction.
- A one-second tick is `O(T)` for `T` materialized torrents. It does not inspect
  their piece ranges, have bits, file rows, peer rows, or active block maps.
- New retained ETA state is `O(1)` per torrent: scalar integers, instants, flags,
  and one generation identity. No new retained per-piece, per-file, per-peer,
  per-block, or sample-window collection is allowed.
- Geometry may temporarily retain at most one coalesced piece-range entry per
  metainfo file (`4,096`) and must not allocate a second maximum-piece bitmap.
- There is one application-wide cadence task and no per-torrent cadence task.
- ETA publication is at most 1 Hz per changed torrent and continues to obey
  existing view-set coalescing and queue bounds.

Add a release-mode maximum-geometry benchmark or calibrated test using the
accepted 2,097,152-piece and 4,096-file limits, alternating verified state,
padding boundaries, and fragmented selection. Record wall time, peak or total
new allocation, and retained `size_of` evidence in this tactical. The evidence
must confirm the structural bounds above. If profiling finds a scan or
allocation on the event/tick path, stop and repair it before product wiring;
do not waive it as an acceptable first version.

## Reference Dossier

Reference status was checked on 2026-08-07. The BEP, rqbit, and libtorrent
checkouts match `reference/pins.toml` and are clean. The JSTorrent sibling is
at the exact revision below. Its relevant
tracked files match that revision; unrelated local files elsewhere in the
sibling are not part of this inspection and must not be touched.

### Normative behavior

- `reference/bittorrent.org/beps/bep_0003.rst` defines multi-file torrents as
  one concatenated byte space and hashes complete pieces. It therefore makes
  real skipped-file bytes in a wanted boundary piece necessary network work.
- `reference/bittorrent.org/beps/bep_0047.rst` defines padding files as zero
  bytes clients may know in advance. RSTorrent already synthesizes them, so
  they participate in full-piece hashing but not in peer-payload ETA.

Neither BEP defines ETA, sampling, smoothing, or presentation states. Those
remain application/product policy.

### Pinned libtorrent oracle

The required oracle is libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`.

- `include/libtorrent/torrent_status.hpp` defines `total` as bytes actually
  requested from peers, excluding pad files, and distinguishes
  `total_wanted`, `total_wanted_done`, and payload download rate.
- `src/torrent.cpp::bytes_done` derives wanted bytes from the picker and file
  layout. With `query_accurate_download_counters`, it includes finished and
  writing blocks in unfinished pieces; its own comment identifies that walk as
  too expensive to perform unconditionally.
- `include/libtorrent/torrent_handle.hpp::query_accurate_download_counters`
  documents that explicit accuracy choice.
- `test/test_torrent.cpp::total_wanted` covers selective wanted totals, zero
  wanted bytes, and last-priority-update behavior.
- `include/libtorrent/stat.hpp::stat_channel` and
  `src/stat.cpp::stat_channel::second_tick` keep current/total counters and an
  integer five-sample low-pass rate using `average * 4 / 5 + sample / 5`.

RSTorrent adopts the payload/wanted distinction, inclusion of retained partial
blocks, pad exclusion, and bounded integer smoothing. It deliberately derives
exact partial work incrementally instead of requesting an expensive status
walk, adds explicit typed ETA states, and does not adopt libtorrent's picker,
status flags, object model, or resume format.

### Pinned rqbit comparison

Pinned rqbit is
`4e5f94cbcf1d57ec500885c77cf1e24d70232d89`.

- `crates/librqbit_core/src/speed_estimator.rs::SpeedEstimator` uses monotonic
  progress snapshots, a bounded five-snapshot `VecDeque`, a sliding-window
  byte rate, and remaining/rate time.
- `crates/librqbit/src/torrent_state/live/mod.rs` updates that estimator from a
  per-torrent 100 ms task.

RSTorrent adopts monotonic, bounded deterministic sampling. It deliberately
uses constant scalar EMA state and one application-wide one-second cadence,
not a queue, floating point, zero sentinel, or per-torrent task.

### JSTorrent product history

The local sibling was inspected at
`9895410beeed6aff554053769bd006a3fbd373ef`.

- `packages/engine/src/core/torrent.ts::eta` divides full-torrent geometry
  minus verified-piece bytes by the current peer-summed rate. It is not
  selection-aware and does not include retained unverified blocks.
- `packages/ui/src/utils/format.ts::{computeEtaSeconds,formatEta}` separately
  recomputes ETA from progress, total size, and rate, returning infinity for
  complete and stalled inputs.
- `packages/ui/src/tables/TorrentTable.tsx` uses that second calculation,
  creating two semantic owners with different unavailable behavior.
- `packages/engine/src/utils/rrd-history.ts::RrdHistory` and
  `packages/engine/src/core/bandwidth-tracker.ts` provide a three-second
  current-rate window for general bandwidth history.

RSTorrent retains the familiar compact duration, infinity for a real running
stall, and sortable ETA column. It avoids verified-only coarseness,
full-torrent selective errors, UI recomputation, and ambiguous null/infinity
sentinels.

### Existing RSTorrent boundary

- `rstorrent-protocol::storage_layout::{TorrentLayout, FileSelection}` and
  `request_ranges()` already define wanted pieces and skip
  `SegmentTarget::Padding` while retaining real skipped-file boundary bytes.
- `rstorrent-session::views::model::TorrentModel` already keeps compact
  verified ranges and bounded active-piece received/stored ranges for
  inspection, but ETA must not add another such collection.
- `ViewActivitySink` already receives unique `BlockReceived` and exact
  `PieceHashFailed { failed_bytes }` events. The latter currently loses
  `failed_bytes` when mapped into `TorrentActivity` and must retain it.
- Durable torrent loading already has verified metadata, `FileSelection`, and
  the bounded have slice together. This is the preferred one-time geometry
  reconstruction seam.
- `ViewHub::replace_durable` currently preserves transient counters, active
  pieces, and peer-rate state. ETA requires an explicit generation/selection
  identity decision there rather than accidental preservation.
- `payload_download_rate_bytes` is currently a sum of peers' observed payload
  rates. It remains the Down-column value and is not silently redefined.
- `clients/web/src/inspection/components/TorrentTable.tsx` already has an ETA
  column, `TorrentRow` has `etaSeconds`, and the live adapter always sets it to
  `null`. `VirtualTable` already supports exact decimal-string sorting.

The concrete boundary improvement is one reusable pure network-work geometry
in the layout owner and one generation-fenced scalar estimate in the
application-view owner. Neither the engine hot path nor React acquires
selection or ETA policy.

## Implementation Sequence

1. Add pure required/verified payload geometry with differential and
   maximum-limit cost tests. Do not wire views until the structural cost
   contract passes.
2. Add generation-fenced scalar remaining-work transitions, carry exact hash
   failure bytes, and prove restart/pause/selection reconstruction.
3. Add the pure integer estimator and one joined application-wide cadence.
4. Extend `TorrentView`, JSON Schema, TypeScript, UniFFI, and Kotlin-generated
   contracts; add snapshot, patch, validation, recovery, and stale-generation
   coverage.
5. Replace the frontend placeholder with the typed row model and
   arbitrary-precision formatter; wire default-visible responsive ETA columns
   in Transfers and Workbench and explicit demo states.
6. Run deterministic, cost, workspace, web, browser, and controlled transfer
   gates. Update this tactical and all owning topics with exact evidence before
   the implementation commit.

## Validation Matrix

### Geometry and work accounting

- single-file, one-entry `files`, and ordinary multi-file layouts;
- selected file wholly inside a piece, exactly on a piece boundary, and
  spanning several pieces;
- selected/skipped boundary in both file orders;
- several selected files sharing and not sharing pieces;
- internal padding, padding at a piece edge, a padding-only piece, and final
  short piece;
- empty files, all wanted, all skipped, and malformed/out-of-range have state;
- verified pieces inside and outside required ranges and alternating maximum
  have state;
- accepted then stored then verified without double subtraction;
- exact hash-failure restoration followed by retry;
- duplicate/redundant/unsolicited payload producing no accepted-work event;
- pause, restart, recheck, selection replacement, and stale old-generation
  event after successor activation; and
- checked arithmetic at every accepted metainfo limit.

Use a deliberately simple test-only per-piece/per-segment oracle on bounded
random layouts and selections to differentially verify the optimized geometry.

### Estimator and state machine

- first activation and first partial interval remain `warming_up`;
- first complete nonzero sample seeds the rate and produces ceiling seconds;
- exact integer EMA sequence including zero samples;
- irregular/late tick normalization and a skipped tick;
- after a prior usable sample, nine seconds without payload remains a decaying
  estimate and ten seconds is `stalled` with public rate zero;
- with no payload since activation, the first nine seconds remain
  `warming_up` and ten seconds becomes `stalled`;
- resumed accepted payload leaves stalled state on the next usable tick;
- zero remaining, all skipped, metadata absent, pause, checking, failure,
  publication, completion, and removal are `unavailable`;
- generation reset drops the prior rate and accepted bucket;
- very large remaining/rate values avoid overflow and retain exact decimal
  seconds; and
- one shared cadence starts, cancels, and joins with no pending task.

All time-dependent tests inject monotonic instants into pure transitions. They
do not sleep for semantic assertions.

### Contract and shared React product

- Rust JSON round trips and schema snapshots cover every tagged state and
  nullable pre-metadata exact fields;
- view snapshots, patches, coalescing, resets, and live-controller recovery
  retain the exact decimal strings and state tag;
- generated TypeScript validation rejects malformed tags and estimate variants
  without seconds; the live mapper treats negative or noncanonical decimal
  strings as unavailable rather than converting them to imprecise numbers;
- `formatEta` consumes the typed state, uses `BigInt`, and covers seconds,
  minutes, hours, days, stalled, warming, and unavailable values;
- Workbench keeps its existing default-visible 84-pixel ETA column at its
  current 700-pixel minimum viewport;
- Transfers adds a default-visible 84-pixel ETA column after Rate with a
  640-pixel minimum viewport, without displacing Name, Status, or Progress;
- only `estimate.seconds` is a decimal sort value; `stalled`, `warming_up`, and
  `unavailable` sort after estimates in stable row order in either direction;
- live-sort remains opt-in, so one-second ETA changes do not move rows unless
  the user requested it;
- persisted table configuration accepts the new Transfers column without
  losing previous widths, visibility, sort, or live-sort settings; and
- wide, compact, and phone evidence covers all four states, hover/accessibility
  explanations, virtualization bounds, and no serious or critical axe finding.

### Controlled end-to-end evidence

Use a bounded scripted peer to pace accepted payload through warming,
estimate, ten-second stall, recovery, one hash failure, and completion. The
fixture includes a wanted/skipped shared boundary and BEP 47 padding, asserts
the exact required/remaining/rate/seconds snapshots, and verifies final
payload. Repeat the ordinary completion path against pinned libtorrent to
confirm the selected payload and complete-piece behavior. No public network is
required.

## Validation Commands

Run in proportion to the landed implementation, sourcing the configured
profile first:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Also run the generated-contract drift command used by the repository, focused
Rust maximum-geometry/release evidence, focused and complete web tests,
TypeScript typecheck, production Vite build, CSP scan, the production-hosted
headless browser scenario, controlled scripted-peer evidence, controlled
pinned-libtorrent evidence, and `git diff --check`. Record exact commands,
counts, timing, allocation/high-water results, skipped opt-in cases, and any
environmental limitation in Completion Evidence.

## Documentation And Evidence Closure

Before marking this tactical complete:

- update `application-view-api` with the accepted exact fields, typed ETA,
  scalar estimator owner, and generation reset rule;
- update `download-correctness` with the distinction between full-piece hash
  bytes and non-padding peer-payload ETA work plus the deterministic cases;
- update `web-ui-design` and `desktop-inspection-surface` with the two table
  presentations and accessibility behavior;
- update `capability-readiness` with the completed evidence and next queue
  decision; and
- fill in exact code paths, benchmark results, controlled observations, test
  commands, and deliberate deferrals below.

## Escalation Conditions

Continue without routine approval for the pure layout helper, scalar view
state, one shared joined cadence, generated additive contract fields, shared
React table wiring, deterministic demos, focused refactors, tests, and topic
updates described above.

Stop for direction before changing persisted schema, selection semantics,
file-priority vocabulary, storage routing, engine scheduling, cumulative
transfer counters, existing Down-rate semantics, public API versioning policy,
adding a dependency, adding per-torrent tasks or retained per-piece ETA state,
or expanding into file/streaming ETA, Size/Progress repair, Android UI,
physical devices, or public-network evidence.

## Completion Evidence

Completed on 2026-08-07 in bounded tactical, geometry, runtime, contract,
presentation, proof, and closure commits. The implementation lives at these
ownership seams:

- `TorrentLayout::required_payload_geometry` in
  `rstorrent-protocol::storage_layout` performs the
  pure selection-aware geometry pass and retains only two `u64` totals;
- `rstorrent-session::views::eta` owns the generation-fenced scalar state
  machine and the one joined application-wide cadence;
- `views::{hub,model}` and `application` carry exact accepted/failure events,
  reconstruct outside the hub lock, and publish targeted torrent-row changes;
- `views::contract` plus the generated TypeScript/schema and UniFFI/Kotlin
  surfaces carry the four exact fields and closed tagged state; and
- the shared React live adapter, validators, formatter, demos, Transfers, and
  Workbench consume that state without deriving network work or rate.

The optimized geometry agrees with a deliberately simple `request_ranges`
oracle for every selection of the fixed boundary/padding fixture and every
selection across 96 deterministic generated layouts. The ignored release
maximum case uses 2,097,152 pieces, 4,096 files, alternating have state,
padding every eighth file, and fragmented selection. A cached
`/usr/bin/time -l cargo test --release` run completed the whole test process in
0.11 seconds with 96,092,160 bytes maximum RSS and a 71,582,320-byte peak
memory footprint. The ETA-specific temporary coalesced-range upper bound was
49,152 bytes and retained `RequiredPayloadGeometry` is 16 bytes. The complete
per-torrent `TorrentEtaModel`, including Rust `Instant` representation and
generation state, is 184 bytes and has no collection.

Pure-clock and view-hub tests cover warming, first estimate, irregular and
zero-duration ticks, EMA decay, the exact ten-second stall, recovery, exact
hash-failure restoration, retry to zero work, pause/deactivation, all skipped,
completion, selection reconstruction, stale generation fencing, and `u64`
overflow boundaries. The controlled activity lifecycle traverses
warming -> estimate -> stalled -> recovered estimate -> hash failure -> clean
network completion without sleeping. A real loopback HTTP and HTTPS
tracker/peer application transfer additionally hash-verifies and publishes
content, then exposes exact required bytes, zero remaining work, zero ETA rate,
and `unavailable` through the ordinary subscribed summary.

The pinned libtorrent checkout remained clean at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`; its Python binding and native
library both reported `2.0.13.0`. One controlled seven-file selective run
requested exactly 97,232 peer bytes in seven blocks across four of five
pieces, omitted the fully skipped piece, excluded 3,304 padding bytes, wrote
73,000 selected logical bytes plus 24,232 required boundary bytes, independently
hash-verified every selected file, and cleaned its fixture. This corroborates
the geometry semantics without making libtorrent an ETA owner.

Contract and product evidence is exact:

- `npm run generate` regenerated the TypeScript, JSON Schema, validators, and
  fixtures with no drift; Rust JSON round trips cover every ETA tag and the
  schema/validator tests cover required nullable pre-metadata work and malformed
  estimates;
- Vitest passed 214 tests in 34 files with two files/two tests deliberately
  skipped, including exact `BigInt` seconds/minutes/hours/days formatting,
  semantic sorting, all four demo states, both tables, and live mapping;
- the production Vite build and CSP scan passed. A production-preview headless
  Chrome case covered estimate, warming, unavailable, stalled, Compact mode,
  the phone-width visibility rule, titles/accessibility names, and zero serious
  or critical axe findings;
- regenerated UniFFI Kotlin compiled in the Android bootstrap and all 12 debug
  unit tests passed. No Android Compose ETA presentation was added; and
- the Rust workspace passed formatting, clippy with warnings denied, and 690
  tests with 11 deliberate opt-in/maximum cases ignored.

The exact retained validation commands were:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
/usr/bin/time -l cargo test --release -p rstorrent-protocol \
  storage_layout::tests::maximum_required_payload_geometry_stays_structurally_bounded \
  -- --ignored --exact --nocapture
tests/interop/.venv/bin/python tests/interop/first_verified_piece.py \
  --selective-files --runs 1

cd clients/web
npm run generate
npm test
npm run typecheck
npm run build
npx vite preview --host 127.0.0.1 --port 4178 --strictPort
RSTORRENT_PLAYWRIGHT_BASE_URL=http://127.0.0.1:4178 \
  npx playwright test tests/inspection-demo.spec.ts \
  --grep "typed torrent ETA"

cd ../../clients/android
./gradlew -p . assembleDebug testDebugUnitTest
```

Before the Gradle gate, the host `rstorrent-android` library was rebuilt and
`rstorrent-uniffi-bindgen generate --crate rstorrent_session` regenerated the
Kotlin source from that library with
`crates/rstorrent-session/uniffi.toml`. The generated source remains an ignored
build artifact; the tracked Android reducer fixture is the compile-time
consumer updated by this tactical.

No public swarm, visible desktop launch, or physical-device run was used.
Per-file/queue ETA, richer priority, live Size/Progress repair, estimator
persistence, and streaming policy remain the deliberate non-goals above.
