# Tactical 063: Live File Selection

Status: Active

Topics: `application-control`, `client-persistence`, `download-correctness`,
`download-roots`, `storage-throughput-architecture`, `web-ui-design`

## Motivation

The Files inspection surface already projects durable per-file selection, and
the engine already honors an immutable initial skip list. The product cannot
yet change that selection, however. The add dialog also conflates choosing
files with the simpler question of whether content should start after magnet
metadata arrives.

This slice makes binary file selection a durable live control for multi-file
torrents. The Files tab exposes only `Normal` and `Skip`. The add dialog gains
one independent start-content checkbox. A metadata-only add must fetch and
verify metadata without creating payload, staging, or part-file artifacts.

The storage behavior follows the pinned libtorrent semantics: a piece is
wanted when any overlapping non-padding file is wanted; full boundary pieces
are requested and verified; lowering a file does not migrate an already
created destination into the part file; raising a file exports available
verified spans from the part file; and the part file is created only when a
write actually needs it.

## Stopping Condition

This tactical is complete when a path-backed multi-file magnet can be added in
metadata-only mode, resumed explicitly, and changed between `Normal` and
`Skip` from the shared Files tab with durable restart behavior; the active
engine generation is safely joined before the new selection takes effect;
selection-dependent payload placement, conservative recheck, materialization,
publication, and empty-part cleanup are exact; and the Rust workspace plus
focused web component and authenticated browser evidence pass.

## Scope

- Add a bounded semantic application command for setting one or more file
  indices to `Normal` or `Skip`.
- Persist sparse skipped-file overrides transactionally and make receipt replay
  idempotent.
- Apply a live change by safely cancelling and joining the matching active
  engine generation, then reopening, conservatively rechecking, replanning,
  and restarting it when user intent still says to run.
- Extend add-magnet intent with a start-content flag. Metadata acquisition
  continues while content intent is paused, but no content storage is prepared
  until explicit resume.
- Replace the add dialog's file-choice affordance with one checked-by-default
  start-content checkbox.
- Make the Files table range and batch actions issue `Normal` or `Skip` and
  project the resulting durable state.
- Make path-backed selective storage retain existing destinations on a
  wanted-to-skipped change, create the part file lazily on the first physical
  skipped-byte write, export verified part spans on skipped-to-wanted, and
  remove the path part file when its final slot is released.
- Preserve the existing immutable positional write/hash plans and broad
  torrent-generation fence for this uncommon control operation.

## Non-Goals

- Higher, lower, sequential, streaming, deadline, or piece-level priorities.
- A file-selection modal or tree in the add-torrent dialog.
- Deleting or compacting a file when it is changed to `Skip`.
- Moving existing file bytes into the part file.
- Changing selection while an Android SAF descriptor manifest is active.
  Dynamic selection is path-backed in this slice; the portable durable command
  fails closed for platform-capability storage until its descriptor reacquire
  and provider lifecycle are designed.
- Simultaneous multi-torrent execution, per-file hot-path fences, relocation,
  seeding/upload reads, or a public libtorrent-compatible priority scale.

## Reference Dossier

### Normative behavior

- `reference/bittorrent.org/beps/bep_0003.rst` defines multi-file content as
  one concatenated torrent byte space and hashes complete piece ranges. A
  selected boundary therefore still requires bytes belonging to an adjacent
  skipped file.
- `reference/bittorrent.org/beps/bep_0047.rst` makes padding bytes synthetic
  zeroes that need not be requested or stored.

Neither BEP defines a product file-priority API or part-file format. Those are
client policy and storage implementation details.

### Pinned libtorrent oracle

The required oracle is libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` from
`reference/pins.toml`.

- `include/libtorrent/torrent_handle.hpp` documents `file_priority()` and
  `prioritize_files()` as asynchronous disk operations, with completion
  reported by `file_prio_alert`. It explicitly says lowering an existing file
  to priority zero does not move it into the part file.
- `src/torrent.cpp::{fix_priorities,on_file_priority,prioritize_files,
  set_file_priority,update_piece_priorities}` validates/coalesces file changes
  and gives each piece the maximum priority of its overlapping files.
- `src/mmap_storage.cpp::{need_partfile,set_file_priority}` creates the part
  owner on demand, exports part data when a file becomes wanted, and routes a
  newly skipped file through the part file only when its destination does not
  already exist.
- `src/part_file.cpp` owns lazy slot allocation, positional payload access,
  export, release, and removal after the final allocation is freed.
- `test/test_priority.cpp::{no_metadata_prioritize_files,
  no_metadata_file_prio,file_priority_multiple_calls,
  export_file_while_seed,file_priority_stress_test}` covers pre-metadata
  intent, last-update behavior, asynchronous export, and repeated changes.
- `test/test_torrent.cpp::test_running_torrent` covers changing selection while
  a torrent runs, including rapid select/unselect updates.
- `test/test_storage.cpp` covers initially skipped destinations remaining
  absent and creation after promotion.
- `test/test_part_file.cpp::{part_file,posix_part_file}` proves that an empty
  constructed/flushed part file is absent, the first write creates it, reopen
  and export retain exact bytes, and releasing the last slot removes it.
- `test/test_checking.cpp` and `test/test_resume.cpp` cover priority changes
  during checking and durable priority restoration.

RSTorrent adopts the observable semantics, not libtorrent's C++ object model,
disk-job API, priority numbering, resume format, or memory-mapped backend.

### JSTorrent product history

The local JSTorrent sibling was inspected at
`9895410beeed6aff554053769bd006a3fbd373ef`.

- `packages/engine/src/core/file-priority-manager.ts` implements binary normal
  and skipped files, derives wanted/boundary/blacklisted pieces, recomputes
  piece priority after a change, and stops active work for newly skip-only
  pieces.
- `packages/engine/src/core/torrent.ts::{initPartsFile,setFilePriority,
  setFilePriorityAsync,materializePiece,exportPieceWantedSpans,
  syncPartsPiecesToCurrentPriorities}` connects selection to storage and makes
  the asynchronous form await materialization.
- `packages/engine/src/core/torrent-content-storage.ts` routes writes by the
  current file selection.
- `packages/engine/src/core/parts-file.ts` does not create storage merely when
  its object is initialized; its flush path creates storage only for allocated
  slots and deletes it when empty.
- `test/core/session-fileprio.test.ts` covers persisted selection restoration.
- `test/core/materialize.test.ts` covers boundary-piece placement, the exact
  skip/unskip round trip, asynchronous materialization, restart sync, partial
  export, and final part cleanup.

JSTorrent confirms the product behavior and useful cases. RSTorrent does not
adopt its in-memory complete-piece cache, JavaScript/FFI topology, numeric
priority scale, or restriction against skipping an already completed file.

## Existing RSTorrent Boundary

- `rstorrent-protocol::storage_layout::FileSelection` is already the pure
  binary wanted/skip authority. Its piece selection takes the maximum
  overlapping file selection and its request ranges include all real bytes of
  a wanted boundary piece.
- `run_selective_download` currently derives one immutable piece plan at
  generation startup. There is no safe mutable picker API.
- `rstorrent-session` already stores sparse skipped rows in `file_selection`,
  and the Files view already projects them.
- `SelectiveStorage` currently creates a physical `PartFile` eagerly and
  assumes it is always present for resume, sync, hashing, and checkpoint
  handle capture. It also assumes every currently skipped destination routes
  through that part file.

The concrete boundary improvement is to make selection an application-owned
durable intent and keep the engine generation immutable. Storage separately
records physical routing so scheduling priority is not confused with where
already-created bytes live.

## State, Owner, And Cancellation Map

| State or work | Owner | Transition / termination |
| --- | --- | --- |
| Sparse skipped-file intent | `SessionStore` transaction | Set command validates metadata indices, updates rows and revision atomically; receipt replay returns the retained result. |
| Start-content intent | Durable torrent desired state | Add starts as running or paused; metadata acquisition is allowed in either state; explicit resume changes paused intent to running. |
| Active piece plan | One engine generation | Immutable after startup. A selection command requests safe cancellation, awaits the exact supervisor/storage/checkpoint join, and only then starts a replacement generation. |
| Per-file physical route | `SelectiveStorage` | A skipped existing file retains its handle; a skipped missing file routes to the part file; promotion creates/opens its destination and exports verified spans before routing changes. |
| Part-file object and handle | `SelectiveStorage` | Absent initially; created before the first planned part write; shared with checkpoint durability through one bounded generation-local late handle; dropped and unlinked after the last slot is released. |
| Metadata-only worker | Existing application active task | Uses the ordinary cancellation/join path. Verified metadata is committed, but no content pipeline or storage artifact is opened. |

No new detached task, daemon, or background owner is introduced. The
application's existing single-active-torrent rule remains in force.

## Required Invariants And Limits

- A set-selection command contains at least one and at most the metainfo file
  count, never more than the parser's existing `4,096`-file bound. Indices are
  sorted, unique, in range, and may not identify padding entries.
- `Normal` and `Skip` are semantic values; neither the Rust nor generated
  client contract exposes an ordinal priority scale.
- The store commits selection before an active engine is cancelled. Replaying
  a successful request is idempotent. A failed validation changes neither
  selection nor engine state.
- The old engine and every storage/checkpoint child are joined before a new
  generation opens the same artifacts. No positional plan or completion can
  cross that fence.
- Piece selection is the maximum overlapping file selection. A skip-only piece
  is not requested. A boundary piece is requested and verified in full, with
  non-padding skipped bytes written to their established route.
- Adding with start-content disabled may perform metadata networking and
  bounded parsing only. It creates no staging tree, wanted file, or part file.
- Constructing or resuming selective storage does not create a path part file.
  The first write whose physical route requires a part slot creates it. An
  empty path part file is removed.
- Lowering an existing destination retains and continues routing bytes to that
  file. It never copies bytes into the part file and never deletes user-visible
  data.
- Raising a missing destination copies every available verified overlapping
  span from part slots before those slots can be released. A slot is released
  only after no remaining part-routed skipped file needs bytes from that
  piece.
- A persisted have claim whose current storage sources are absent is cleared
  before hashing and downloaded again. Missing or corrupt sources never become
  verified by selection alone.
- All-skipped selection stops content work without treating absent content as
  complete. Promoting a file starts checking/download automatically only when
  durable desired intent remains running.
- Publication remains all wanted verified plus exact publication completion.
  Skipped retained files do not make missing wanted content complete.
- Late checkpoint-handle registration is bounded to the one optional part
  file per active engine generation. It cannot create an unbounded handle
  registry.
- Paths, magnets, peer addresses, and payload bytes remain outside diagnostic
  command or error text.

## Implementation Sequence

1. Add deterministic selective-storage routing and lazy-part behavior with
   create/resume/materialize/hash/sync/release tests, including retained files
   and missing-source conservative recheck.
2. Add the bounded durable command and start-content intent, metadata-only
   application lifecycle, receipt replay, active-generation join/restart, and
   store/application tests.
3. Regenerate the client contracts and wire the shared add dialog and Files
   table actions without adding high/low priority UI.
4. Run focused engine/session/web tests, authenticated headless browser
   evidence, the Rust workspace baseline, and generated-contract checks.
5. Update this tactical and the owning topics/readiness matrix with exact
   evidence before the implementation commit.

## Validation Matrix

### Deterministic and storage

- all-normal, all-skipped, exact-boundary, cross-file, padding, and final-short
  piece selection;
- fresh selective construction leaves the part path absent;
- first skipped boundary write creates it and last release removes it;
- wanted-to-skipped retains an existing destination and never imports it;
- skipped-to-wanted exports exact verified bytes and preserves boundary bytes
  still needed by another skipped file;
- resume with no part, an empty legacy part, retained skipped destinations,
  missing sources, corrupt sources, and partial verified state;
- lazy checkpoint durability includes a part handle created after pipeline
  startup; and
- cancellation joins every old routing generation before reopen.

### Store and application

- command shape, bounds, metadata/index/padding validation, no-op behavior,
  receipts, revision conflict, sparse persistence, and restart projection;
- metadata-only add fetches and verifies metadata while leaving storage absent
  and state paused;
- explicit resume prepares content; live Skip/Normal safely restarts a running
  torrent; paused changes stay paused; all-skipped becomes idle; and a later
  promotion resumes when desired intent is running;
- boundary-piece progress and complete-state regression after promoting a
  previously skipped file; and
- platform-capability dynamic selection fails closed without altering durable
  selection.

### Product surface and repository

- add-dialog checkbox defaults on, submits both values, and remembers only the
  existing dialog preference;
- Files current/range/batch selection invokes only `Normal` or `Skip`, exposes
  pending/error state, and refreshes from the authoritative view;
- authenticated loopback browser add-metadata-only, resume, skip, and normal
  flow against a controlled multi-file fixture;
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, and
  `cargo test --workspace`;
- generated TypeScript/Kotlin contract checks, web typecheck, component tests,
  production build, and focused browser tests.

## Accepted Design Differences

Libtorrent applies a priority update through an asynchronous disk fence inside
one long-lived torrent. This first RSTorrent slice uses a coarser but existing
ownership boundary: transactionally retain intent, safely join the entire
active engine generation, then reopen and recheck under a new immutable plan.
It costs peer reconnection for an uncommon user control, but makes the routing
fence explicit and leaves no mutable picker or stale positional plan. A later
per-file fence requires evidence that this cost is material.

Dynamic platform-capability selection remains deliberately unavailable. Its
correct implementation must reacquire or create provider documents and prove
publication behavior; silently pretending a fixed descriptor manifest can
change would violate the capability and restart contracts.

## Evidence

Pending implementation.
