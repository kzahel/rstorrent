# Tactical 079: Engine Driver Source Shape

Status: In progress from maintainer direction on 2026-08-04. The clean
baseline, test-layout gate, and download-control extraction are complete; the
storage-owner extraction remains. This tactical is a behavior-preserving
architecture slice and does not change the authoritative capability queue
unless the maintainer explicitly schedules it.

Topics: `product-direction`, `download-correctness`, `peer-lifecycle`,
`storage-throughput-architecture`

## Execution Record

The reconciled baseline had 16,605 lines in `driver.rs`: 8,803 lines through
the production facade and 7,801 lines in its inline test module including the
module wrapper. `cargo test -p rstorrent-engine --lib -- --list` reported 196
tests and zero benchmarks; the focused baseline passed 193 tests with the
three public-network probes ignored.

The test-layout gate first moved the existing test body unchanged to
`driver/tests/mod.rs`, then divided its 80 driver scenarios among `control`,
`storage_pipeline`, `content`, `discovery_metadata`, and
`recheck_publication` child modules. Shared private fixtures remain in the
2,200-line parent test module so production visibility did not widen. The
facade is 8,805 lines including the private test-module declaration, and the
categorized test tree is 7,766 lines after formatting. All 196 focused engine
library tests retain the same result; only their private module qualification
changed.

The control-owner gate moved 2,490 lines of cancellation, accounting,
diagnostic snapshots, activity sinks, platform-storage installation, and
bounded test instrumentation to `driver/control.rs`. The facade is 6,381
lines at this gate and retains the same root public re-exports. Direct facade
access to the control's cancellation field became private control methods;
task and token ownership did not change. `cargo check -p rstorrent-engine
--tests`, the focused 196-test engine suite, and warning-denying focused
Clippy pass.

## Decision And Motivation

Refactor the engine download driver around two already-established ownership
boundaries before more protocol breadth accumulates there:

- `DownloadControl` owns cancellation, resource accounting, diagnostics,
  activity emission, platform-storage installation, and test instrumentation;
  and
- the content-storage pipeline owns bounded write, hash, checkpoint,
  completion, cancellation, and join execution.

Keep top-level download orchestration, metadata and discovery coordination,
content-swarm supervision, recheck, and publication sequencing in the driver
for this slice. Move the driver's large private test body into categorized
child modules without widening production visibility.

The current crate graph remains appropriate:

```text
rstorrent-protocol
        -> rstorrent-engine
                -> rstorrent-session
                        -> gateway / desktop / Android adapters
```

`rstorrent-platform` remains a reused operating-system capability boundary,
and `rstorrent-gateway` remains a distinct transport and security boundary.
File size alone does not justify a new `core`, `domain`, `storage`, or `views`
crate.

The review that selected this slice found:

| File | Production lines | Test lines | Relevant pressure |
| --- | ---: | ---: | --- |
| `rstorrent-engine/src/driver.rs` | about 8,803 | about 7,801 | Download control, tracker and DHT intake, metadata, storage execution, content supervision, recheck, publication, and many fixture families meet in one file. |
| `rstorrent-engine/src/selective_storage.rs` | about 3,616 | about 1,859 | One legitimate state owner also carries several backing, planning, descriptor, publication, and path concerns. |
| `rstorrent-session/src/views.rs` | about 4,468 | about 1,144 | Contract values, projection models, hub state, subscription queues, patching, and range algorithms coexist and import view-set internals bidirectionally. |
| `rstorrent-session/src/application.rs` | about 3,144 | about 2,379 | Service lifecycle, cleanup, checkpoint callbacks, activity projection, and durable view construction coexist. |
| `rstorrent-session/src/store.rs` | about 3,197 | about 1,309 | One correct SQLite owner also carries schema, migration, reads, mutations, and validation. |

Across the 200 commits inspected during the review, `driver.rs` was touched 52
times with roughly 17,000 changed lines. `application.rs`, `views.rs`, and the
web live adapter were also frequent convergence points. These measurements
are selection evidence, not hard source-size budgets.

`driver.rs` is the first target because it combines the largest production
and test concentration with the widest set of independent engine owners. The
two selected extractions follow behavior and lifecycle boundaries that have
already been exercised through the storage and observability campaigns. This
is not a cosmetic one-file-per-type rewrite.

## Desired Outcome And Stopping Condition

The tactical stops when all of the following are true:

- `driver.rs` is the engine download facade and top-level orchestration owner,
  not the implementation home of download-control observation or the content
  storage/checkpoint task;
- a private driver child module owns `DownloadControl`, its inner state,
  cancellation and safe-cancel behavior, resource counters, disk and metadata
  observation, activity sinks, platform-storage installation, and bounded
  test instrumentation;
- a separate private driver child module owns content-storage commands,
  write/hash jobs, checkpoint admission and execution, queueing,
  backpressure, completion, cancellation, and exact task shutdown;
- engine-root public names and cross-crate call sites retain their existing
  paths and semantics unless an internal name can change without becoming a
  compatibility event;
- no implementation item becomes more visible merely so a moved test can
  reach it;
- the former monolithic driver test body is divided into private child test
  modules by owner, with shared helpers remaining test-only and bounded;
- existing test names and assertions are preserved except for mechanical
  qualification changes or a recorded correction to a test that was already
  invalid;
- the extraction adds no task, channel, queue, allocation, copy, lock,
  timeout, retry, or runtime branch to the production path;
- source counts before and after are recorded as navigation evidence, while
  ownership and dependency direction remain the acceptance criteria; and
- formatting, warning-denying lint, workspace tests, controlled storage and
  download interoperability, Android cross-compilation, and diff checks pass.

The target layout may use local naming conventions, but its intended shape is
approximately:

```text
rstorrent-engine/src/
  driver.rs                    facade and top-level download orchestration
  driver/
    control.rs                 cancellation, bounds, observation, sinks
    storage_pipeline.rs        write/hash/checkpoint task ownership
    tests/
      mod.rs
      control.rs
      discovery_metadata.rs
      content.rs
      storage_pipeline.rs
      recheck_publication.rs
      support.rs
```

Exact test grouping may change when fixture dependencies are mapped. Do not
create empty or one-test categories merely to match this sketch.

## Source-Organization Contract

This tactical applies the following guidance to the selected engine boundary:

- A module primarily owns state, invariants, or a lifecycle. It is not a
  bucket for similarly named types.
- `lib.rs` and subsystem facade files normally document the boundary, declare
  modules, and expose deliberate APIs. Substantial implementation may remain
  only when the crate or subsystem is genuinely small and cohesive.
- Around 1,000 non-test lines is a prompt to inspect cohesion. Around 2,000 is
  a strong prompt to record why the owner should remain together or extract a
  demonstrated seam. Neither value is a CI failure or automatic split.
- Independent tactical churn, unrelated mutable owners, bidirectional module
  knowledge, and several unrelated fixture families are stronger split
  signals than physical size.
- A new crate requires a concrete dependency, reuse, platform, security,
  feature-isolation, lifecycle, or testing boundary with a useful acyclic
  API. Moving a large private module into a crate only to shorten a file is
  not sufficient.
- Initial extraction preserves behavior and uses `pub(super)` or `pub(crate)`
  rather than widening the public engine API.
- Generated code, recorded catalogs, fixture data, and naturally tabular
  declarations are judged by their owning workflow rather than ordinary
  hand-authored line thresholds.

This guidance is recorded here as the contract for the slice. Promotion into
`docs/engineering-principles.md` is a later documentation decision after the
first extraction tests whether the guidance is useful in practice.

## Ownership, Tasks, And Dependency Direction

The production ownership must remain:

```text
top-level download orchestration (driver.rs)
  -> DownloadControl (driver/control.rs)
       -> cancellation and safe-cancel critical section
       -> resource and queue accounting
       -> bounded diagnostics and activity sinks
       -> platform storage capability installation
  -> content swarm supervisor (driver.rs in this slice)
       -> ContentStoragePipeline (driver/storage_pipeline.rs)
            -> bounded command queue
            -> independent write/hash JoinSets
            -> checkpoint task and sync/commit work
            -> typed completions and exact shutdown
            -> SelectiveStorage execution plans
```

The driver remains the caller of protocol, peer, discovery, and storage
components. `control.rs` may depend on engine snapshots and platform-storage
capabilities but must not learn about session, SQLite, gateway, Android, or
application commands. `storage_pipeline.rs` may depend on the existing
checkpoint and selective-storage owners but must not acquire application or
transport policy.

No owner or task changes hands in this tactical. Cancellation tokens,
semaphores, channels, `JoinSet`s, checkpoint callbacks, and activity sinks are
moved with the state that already owns them. Their construction and shutdown
order must remain observable at the same call sites.

## Test Placement Contract

Rust test placement follows the cheapest boundary that proves behavior:

- Compact value and transition tests may remain inline in small modules.
- Large tests that require private driver state live in
  `driver/tests/*.rs`, reached through `#[cfg(test)] mod tests;`. Descendant
  access to private implementation is preferred over production visibility
  changes.
- Reusable scripted peers, metainfo builders, temporary-path helpers, and
  timing helpers remain under `#[cfg(test)]` and are divided only when more
  than one test family genuinely shares them.
- Crate-level `tests/` is reserved for behavior expressible through the public
  crate API. Existing `tests/interop/` harnesses continue to own cross-process
  and independent-client evidence.
- Ignored public-network probes remain opt-in and must not become the proof of
  a behavior-preserving refactor. If their existing public APIs make them
  natural external harnesses, moving them is allowed only after equivalent
  opt-in invocation and cleanup are preserved.

Splitting test files is a navigation and ownership improvement. It is not a
reason to weaken assertions, merge distinct scenarios, or replace exact
cleanup checks with broad success checks.

## Stable Scenarios And Invariants

The refactor must preserve at least these established behaviors:

- download-control snapshots retain exact requested, received, stored,
  buffered, outstanding, queue, write, hash, checkpoint, peer, tracker, DHT,
  metadata, and disk meanings;
- safe cancellation cannot cross storage creation or publication critical
  boundaries, while ordinary cancellation remains prompt and joined;
- received payload remains independently byte-bounded until storage accepts
  or releases it;
- storage write and hash concurrency remain independently bounded and may
  overlap without exceeding their permits;
- logical block completion remains ordered correctly across physical write
  batching, hash verification, checkpoint sync, SQLite callback, and durable
  piece publication;
- a failed write, hash, sync, checkpoint callback, task join, or cancellation
  produces the same typed terminal path and releases every reservation;
- storage saturation cannot starve discovery intake, dial refill, peer
  events, endgame cancellation, or shutdown;
- single-file, one-entry `files`, cross-file, skipped, padding, recheck,
  publication-recovery, and descriptor/platform storage paths retain their
  established behavior; and
- diagnostics and test delay/fault controls retain their current bounds and
  remain unavailable through production authenticated configuration where
  existing policy excludes them.

## Scope

- Record the exact clean-baseline source counts, test list, and relevant
  public engine exports before moving code.
- Move the existing driver test module into categorized child files without
  changing production behavior.
- Extract the download-control owner and its closely coupled snapshot,
  accounting, observation, sink, storage-installation, and test-control state.
- Extract the content storage/checkpoint task, its private commands, jobs,
  completions, queue helpers, write batching, hash execution, and shutdown.
- Keep the driver facade responsible for constructing these owners and
  coordinating them with metadata, discovery, swarm, recheck, and
  publication.
- Preserve engine-root re-exports and audit visibility after extraction.
- Add concise module documentation naming each extracted owner's invariant,
  dependencies, and shutdown contract.
- Update this tactical with actual layout, before/after counts, validation,
  any rejected extraction, and the next recommended boundary.

Small same-boundary cleanup is permitted when extraction exposes duplicated
imports, obsolete qualification, test-only helper leakage, or a private type
that clearly belongs to the moved owner. It must not become a semantic
redesign.

## Non-Goals

- New BitTorrent behavior, incoming listening, uploading, seeding, PEX, uTP,
  IPv6, tracker support, scheduling policy, or storage capability.
- Tactical `075` ephemeral application state or Tactical `078` local TCP
  seeding. This slice may land before Tactical `078` so that seeding does not
  add pressure to the old driver shape, but it does not implement or redesign
  `078`'s direction-neutral `peer_io`, incoming, upload, or seed-content
  owners.
- Wholesale extraction of metadata acquisition, tracker execution, DHT
  intake, content supervision, recheck, or publication from `driver.rs`.
- Splitting `SelectiveStorage`, `ApplicationService`, `SessionStore`, session
  views, the web client, or Android product layout.
- A new crate, dependency, framework, service layer, trait hierarchy, actor
  system, generic repository, or dependency-injection mechanism.
- Public API cleanup, persisted-schema changes, generated-contract changes,
  resource-default changes, performance tuning, or error-text redesign.
- A hard repository-wide line limit or CI source-size gate.
- Renaming or moving the Android application out of
  `experiments/android-engine-bootstrap`; that graduation remains a separate
  repository-structure tactical.

## Implementation Sequence And Intermediate Gates

1. **Baseline and map.** Start from a reconciled working tree, record source
   and test counts, list the engine public exports used by session and
   Android, run the focused engine test list, and map test helpers to their
   owning scenario families.
2. **Test-layout gate.** Move only the driver test body into child files.
   Preserve test names, ignored status, helper bounds, and exact test-list
   count. Run the engine library tests before moving production code.
3. **Control-owner gate.** Extract `DownloadControl` and its owned state,
   retain root exports and call-site behavior, and run cancellation,
   diagnostic, metadata, disk-pressure, queue-accounting, and Android compile
   checks.
4. **Storage-owner gate.** Extract the write/hash/checkpoint pipeline with its
   commands, completions, jobs, permits, and joins. Run storage saturation,
   fault, cancellation, checkpoint, endgame, recheck, and publication tests.
5. **Boundary audit.** Remove superseded imports and qualifications, narrow
   visibility, add module contracts, confirm that the facade still owns
   orchestration, and reject any dependency direction that points toward
   session or platform clients.
6. **Closing evidence.** Run the complete validation matrix, record the final
   shape and counts, update this execution record and affected living topics
   only if their architectural truth changed, and commit the bounded slice.

Each gate must leave the workspace compiling and its relevant tests passing.
Do not combine a failing mechanical move with a speculative redesign to make
the failure disappear.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Mechanical shape | Before/after production and test line counts, unchanged or explicitly reconciled engine test list, no test-only visibility widening, and module dependency review. |
| Focused engine | Engine library tests for control snapshots, cancellation, metadata, discovery, content swarm, storage execution, checkpoints, recheck, publication, and exact cleanup. |
| Workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and `git diff --check`. |
| Controlled download | One ordinary controlled first-piece/full-publication run through the existing loopback libtorrent harness with exact payload and process cleanup. |
| Storage execution | One representative selective-storage profile plus the existing checkpoint fault/crash matrix, retaining exact content, durable state, and joined owner evidence. |
| Platform compile | Both established Android Rust targets compile the engine and Android adapter; no emulator or physical device is required. |

The implementation records exact commands and results. No public swarm,
visible browser, Tauri launch, web build, AVD, or physical-device run is
required because this tactical changes no corresponding product boundary.

## Deferred Refactoring Boundaries

The architecture review identified useful later slices but does not authorize
them here:

- split selective-storage backing/acquisition, immutable I/O planning,
  descriptor/platform validation, publication, and path helpers while keeping
  `SelectiveStorage` as the state owner;
- reorganize session views into contract, hub/model, subscription, view-set,
  and diff modules so `views.rs` and `view_sets.rs` no longer know each
  other's implementation details bidirectionally;
- retain `ApplicationService` and `SessionStore` as owners while extracting
  checkpoint/activity sinks, managed-artifact cleanup, durable view
  construction, and SQLite schema/connection policy;
- graduate the Android product from the bootstrap experiment and separate the
  current application boundary from the legacy diagnostic `EngineSession`;
- after Tactical `077`, separate pure view mapping from `LiveApplication`,
  divide semantic validation by contract domain, and extract VirtualTable's
  selection/configuration algorithms without adopting a data-grid framework;
  and
- prune entry-point documentation that accumulates stale tactical checkpoints
  instead of linking the authoritative current queue.

Each later slice requires its own bounded tactical when selected. Completion
of this driver slice does not imply that every large file must be split.

## Escalation And Next Boundary

Implementation may choose exact private module and test filenames, move
closely coupled private types with their owner, use `pub(super)` or
`pub(crate)` as needed, and make mechanical same-boundary cleanup without
direction.

Stop for maintainer direction if the extraction requires a new crate or
dependency, changes a public engine path consumed outside the workspace,
changes runtime behavior or a resource default, changes task/cancellation
ownership, alters a protocol or persistence contract, weakens a test to make
the move pass, conflicts materially with Tactical `075` or `078`, or expands
into one of the deferred boundaries above.

After completion, the next refactoring boundary should be selected from
observed implementation pressure rather than a fixed file-size queue. The
leading candidates are selective-storage internals and the session
view/view-set subsystem.
