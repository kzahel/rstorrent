# Tactical 176: Durable High File Priority

Status: **Implementation complete; iOS host validation pending.** Explicit
user direction on 2026-08-27 temporarily yields desktop release Tactical `158`
to this bounded engine/product slice. All Linux-hosted repository, web, and
Android gates pass; this Linux host has neither Xcode nor Swift, so the updated
iOS SwiftUI presentation has not received its required simulator/archive
compile yet.

Topics: `oracle-driven-engine-campaign`, `capability-readiness`,
`download-correctness`, `client-persistence`, `application-control`,
`application-view-api`, `web-ui-design`, `client-surfaces`,
`http-file-serving-and-streaming`

Dependencies: completed live file-selection Tactical
[`063`](063-live-file-selection.md) supplies serialized Normal/Skip routing;
completed availability-picker Tactical
[`087`](087-availability-ranked-piece-activation.md) supplies ordinary
rarest-first activation; completed incomplete-streaming Tactical
[`139`](139-incomplete-file-streaming-demand.md) supplies the independent
transient current/ahead urgency overlay; schema-19 Tactical
[`143`](143-dual-identity-and-persistence-foundation.md) supplies the first
public-incubation compatibility baseline.

## Motivation And Decision

The application command is named `SetFilePriority`, and iOS retains a High
localization inherited from JSTorrent, but RSTorrent currently implements only
binary Normal/Skip selection. High is absent from the generated contract,
durable store, resume state, engine picker, and all active client actions. This
is not a presentation-only omission.

Add one semantic durable priority model shared by all first-party products:

```text
High > Normal > Skip
```

High and Normal are wanted storage selections. Skip is the existing unwanted
storage selection. Ordinary piece activation uses the strongest priority of
all wanted non-padding files overlapping that piece. High influences new
ordinary work without cancelling already owned requests. The completed
streaming scheduler remains an absolute transient overlay: current and ahead
streaming demand is scheduled before ordinary High or Normal work, is bounded
by Tactical `139`, and disappears with its response lease.

The public application API will not expose libtorrent's raw numeric `0..=7`
scale or a Low setting. The product-established vocabulary in current
JSTorrent is High/Normal/Skip, and pinned libtorrent itself notes that three
download tiers plus filtered is the useful shape. Reserving the richer numeric
scale internally would add persistence and UI states without a present product
need.

## Stable Scenarios

1. **HFP-001 ordinary rank.** With equal availability, a High piece precedes a
   Normal piece. Under rarest-first policy, priority and availability compose
   through the pinned libtorrent weighted key rather than a strict global
   High-before-every-Normal partition.
2. **HFP-002 overlap maximum.** A cross-file piece receives High when any
   overlapping wanted non-padding file is High; Skip never lowers another
   wanted file's boundary piece.
3. **HFP-003 live promotion/demotion.** Normal-to-High and High-to-Normal update
   picker rank in the active generation without stopping storage, cancelling
   requests, invalidating verified data, changing route epoch, or restarting
   the torrent. Skip transitions retain Tactical `063` reconciliation.
4. **HFP-004 restart.** High files persist in a bounded schema-20 table and
   reach resume/start before the picker is constructed. Schema 19 migrates in
   place without discarding torrents, settings, roots, sources, identities, or
   verification state.
5. **HFP-005 command transitions.** Setting High ensures the file is wanted;
   setting Normal ensures wanted and removes High; setting Skip removes High
   and applies the existing skipped route. Repeating the same semantic command
   is revision-idempotent. `Download now` continues to mean Normal plus running
   and clears High only when it actually targets a High file.
6. **HFP-006 streaming composition.** Current and ahead streaming demand still
   wins over durable High. When its lease disappears, the same picker resumes
   weighted High/Normal ordinary order without retaining an urgency boost.
7. **HFP-007 first-party presentation.** Generated Rust/TypeScript/UniFFI
   contracts expose High. Files projections report High/Normal/Skip directly;
   web, Android Compose, and iOS can set and display all three values.
8. **HFP-008 bounds and hostile indices.** Existing verified-metadata file
   validation and the 4,096 selection-entry ceiling also bound High overrides.
   Padding, out-of-range, duplicate, and oversized commands fail atomically.

## Reference Dossier

### Protocol and product semantics

No BitTorrent BEP standardizes local file priority. It is client scheduling
and persistence policy; it changes neither metainfo nor peer-wire messages.

### Pinned libtorrent oracle

The exact pin is rasterbar libtorrent `2.0.13` commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d` from
`reference/pins.toml`. The source survey inspected:

- `include/libtorrent/download_priority.hpp`: filtered `0`, low `1`, default
  `4`, and top `7` on an eight-level scale;
- `include/libtorrent/torrent_handle.hpp`: file/piece priority contracts,
  asynchronous application, resume interaction, and the rule that changing
  file priority derives piece priority;
- `src/torrent.cpp`: `file_to_piece_prio`, `fix_priorities`,
  `on_file_priority`, `prioritize_files`, `set_file_priority`,
  `update_piece_priorities`, and the time-critical deadline owner;
- `include/libtorrent/piece_picker.hpp` and `src/piece_picker.cpp`: the
  availability/priority weighted key, partial-piece state, filtering, and
  priority mutations;
- `test/test_priority.cpp` and `test/test_piece_picker.cpp`: maximum overlap,
  metadata timing, repeated updates, resume, part-file export, filtering,
  partial/open pieces, and randomized priority mutation; and
- `test/test_time_critical.cpp`, `test/test_read_piece.cpp`,
  `test/test_transfer.cpp`, and `test/swarm_suite.cpp`: deadline ordering,
  cancellation, duplicate urgent work, completion, and cleanup.

RSTorrent adopts maximum overlap and the ordinary weighted rarest-first key:

```text
availability * (8 - priority) * 3
Normal = 4, High = 6
```

Unavailable pieces remain ineligible and deterministic tie-breaking remains
RSTorrent-owned. In-order diagnostic policy remains piece-index order. Unlike
libtorrent, RSTorrent does not expose eight durable levels, make file changes
asynchronous disk jobs, or reset explicit per-piece priorities because no
separate public piece-priority API exists.

Libtorrent's time-critical owner confirms that streaming is not merely one
more durable tier: it owns deadlines, request cancellation, peer queue-time
prediction, stalled-request duplication, and cleanup. RSTorrent intentionally
retains Tactical `139`'s independently implemented bounded equivalent rather
than adding another streaming subsystem in this slice.

### JSTorrent product history

The local first-party JSTorrent checkout was inspected at exact revision
`0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`:

- `packages/engine/src/core/file-priority-manager.ts` maps Skip/Normal/High to
  filtered/default/high piece contributions and reserves its strongest value
  for streaming-now;
- `packages/engine/src/core/piece-requester.ts` uses the same weighted
  availability/priority key;
- `packages/engine/src/core/streaming-scheduler.ts` overlays current, next,
  and file-range urgency and suppresses bounded ordinary work;
- `packages/engine/src/core/streaming-request-overlay.ts` bounds urgent peer
  slots and one duplicate outside ordinary endgame; and
- `packages/ui/src/tables/FileTable.tsx` presents High, Normal, and Skip with
  no Low state.

The associated priority and streaming tests cover overlap maximum,
current-before-normal ordering, demand merge/removal, slow-peer limits, and
urgent duplication. RSTorrent adopts the product vocabulary and separation of
durable versus transient priority, not the TypeScript owner graph or source.

## Owner, Task, Cancellation, And Data Flow

```text
semantic command (High / Normal / Skip)
       |
       v
SessionStore transaction
  binary selection + sparse High overrides + revision
       |
       +--> snapshot/file projection --> generated clients
       |
       v
ResumeRecord / FileSelectionUpdate
       |
       v
single active ContentSwarmDownload owner
  layout + selection + sparse file High
       |
       v
per-piece maximum ordinary priority
       |
       v
AvailabilityPicker weighted ordinary rank
       ^
       |
existing StreamingDemandLease overlay schedules first
```

`SessionStore` is the sole durable owner and mutates selection and High rows in
one SQLite transaction. `ContentSwarmDownload` remains the sole active
selection/priority owner. `AvailabilityPicker` owns one compact per-piece byte
beside its availability and heap state. Existing storage, peer, and streaming
tasks are unchanged; there is no new task, channel, timer, cancellation token,
socket, or dependency.

A Normal/High-only update rebuilds picker rank in place and acknowledges its
revision immediately. A Skip boundary change continues through the existing
joined storage stop/reconcile/restart path. Lifecycle cancellation remains
owned by the active torrent generation, while each streaming lease removes
only its transient demand on drop.

Dependency direction remains runtime-independent layout/priority derivation ->
engine picker/swarm owner -> session persistence/projection -> generated
clients. Protocol code does not depend on SQLite, async runtime, serde, or UI.

## Invariants And Resource Bounds

- High overrides are sorted, unique, non-padding verified file indices and
  share the existing maximum of 4,096 sparse selection entries per torrent.
- The persisted table contains only High rows; Normal is implicit for every
  wanted file without an override and Skip remains the binary selection owner.
- High always implies wanted. No store or resume record may contain one file
  in both `skip_files` and `high_priority_files`.
- A piece contribution is the maximum across overlapping wanted non-padding
  files. A wanted boundary piece cannot be filtered by a skipped neighbor.
- The picker retains one `u8` per supported piece: at most 2 MiB at the
  existing 2,097,152-piece ceiling. Its weighted key fits in `u32` under the
  existing `u16` peer-availability ceiling.
- Priority-only mutations rebuild a bounded existing heap once; they allocate
  no per-peer state and do not perform storage I/O or cancel active requests.
- Streaming current/ahead continues before ordinary picker activation and
  retains all Tactical `139` demand, peer, duplicate, rate, and lifecycle
  ceilings.
- Schema 19 to 20 migration is additive and transactional. Older pre-public
  schemas retain Tactical `143`'s explicit reset policy; future/unknown schemas
  still fail closed.

## Implementation And Validation Sequence

1. Add pure priority values, overlap derivation, weighted picker mutation, and
   focused ordering/streaming-composition tests.
2. Carry sparse High indices through startup/resume and live updates, with a
   priority-only fast path distinct from storage selection reconciliation.
3. Add schema-20 additive persistence and schema-19 in-place migration; cover
   atomic transitions, idempotence, restart, invalid state, and retained
   catalog data.
4. Extend Files and torrent snapshots, regenerate TypeScript/schema/UniFFI,
   and update web, Android Compose, and iOS presentation/actions.
5. Run focused deterministic/runtime tests, then formatting, warning-denying
   clippy, the workspace suite, generated-contract verification, web typecheck
   and tests, and both maintained Android ABI builds.
6. Reconcile every owning topic and return Tactical `158` to the sole **Now**
   when the stopping condition is met.

## Implementation And Evidence

The semantic model is implemented end to end. Schema 20 stores only bounded
sparse High overrides and migrates schema 19 transactionally. Resume and live
application updates carry both selection and High state. Layout derives the
maximum overlapping priority, and the ordinary picker plus v2 hash scheduler
use High/Normal order without cancelling accepted work. Files snapshots and
the generated contract expose the three values; React/Tauri, Android Compose,
and iOS SwiftUI display and dispatch them.

The deterministic evidence covers:

- overlap maximum, duplicate/out-of-order and skipped-file priority rejection;
- weighted rarity/priority ordering and live picker mutation;
- v2 hash-need priority and live mutation;
- current/ahead streaming order over an ordinary High piece;
- durable transition/idempotence/restart and schema-19 retention migration;
- live application High and Skip changes without peer-generation replacement;
- truthful paged Files projection and web High/Normal/Skip actions; and
- Android generated binding consumption and Compose compilation.

Validation run on 2026-08-27:

- `cargo fmt --all` and `cargo clippy --workspace -- -D warnings` pass;
- `cargo test --workspace` passes with only the repository's declared ignored
  opt-in/live/maximum tests;
- focused High, streaming-composition, live-application, durable-transition,
  schema-migration, and Files-projection tests pass;
- `npm run generate --prefix clients/web`, `npm run typecheck --prefix
  clients/web`, and `NODE_OPTIONS=--no-webstorage npm run test --prefix
  clients/web` pass; the web suite reports 292 passed and two skipped;
- `clients/android/build.sh` passes both locked Rust ABIs, Kotlin UniFFI
  generation, debug APK assembly, and JVM unit tests; and
- the workspace suite builds and tests `rstorrent-ios` on Linux, but the
  SwiftUI/Xcode simulator/archive gate is unavailable on this host and remains
  the only stopping-condition gap.

## Stopping Condition

This tactical completes when HFP-001 through HFP-008 pass, schema 19 migrates
without data loss, all first-party generated boundaries build, web/Android/iOS
present the truthful three-value model, streaming remains an independently
bounded higher-urgency overlay, declared repository gates pass, and the
readiness/campaign/persistence/client records describe the landed behavior.

## Non-Goals

- A public numeric 0--7 API, Low priority, per-piece user controls, sequential
  mode, automatic media detection, or torrent-wide queue weighting.
- A second streaming scheduler, new deadline API, embedded player, Android
  progressive playback, iOS progressive playback, or changes to HTTP
  capability security and lifecycle.
- Cancelling or preempting ordinary active requests solely because a durable
  High value changed; Tactical `139` remains the only bounded urgency owner
  allowed to preempt ordinary work.
- Libtorrent resume-format compatibility, migration from JSTorrent state,
  public-swarm performance claims, release publication, updater work, or any
  external machine mutation.
