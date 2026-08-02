# Global Disk Inspection

Status: Complete (2026-08-02).

Topics: `disk-and-piece-inspection`, `download-correctness`,
`application-view-api`, `web-ui-design`, `desktop-inspection-surface`,
`performance-and-live-evidence`

## Motivation

RSTorrent can download and verify content, but the detailed client cannot show
whether a stall is caused by peer supply, resident payload, queued writes,
active I/O, verification, or a failed storage operation. Existing
`DownloadProgress` counters are torrent-local diagnostics with incomplete
current gauges; they are not a coherent application view. This slice makes the
entire receive-to-verification pipeline observable and hardens the slow-storage
flow-control boundary that the view is intended to diagnose.

## Scope

- Define immutable engine storage-runtime and piece-attempt snapshots with
  explicit gauges, limits, counters, rates/durations, pressure, and bounds.
- Add high/low watermark pressure state and prove bounded intake stop/recovery
  without peer-timeout misclassification.
- Retain and aggregate all active download observations in one session-owned
  application Disk projection without inventing a session scheduler.
- Add `session_disk` snapshot/keyed-patch support to view sets, generated
  TypeScript/schema/UniFFI/Kotlin contracts, and strict decoders/reducers.
- Add semantic global Disk interest to live/demo Zustand state.
- Split the tab hierarchy into torrent-specific and session groups and render a
  responsive statistics-first Disk panel with a piece-level active-work table.
- Add a permanent `slow-disk-pressure` named scenario and headless screenshot,
  accessibility, responsive, scale, and recovery evidence.
- Prove the live view with controlled local storage delay and exact verified
  completion; update owning topics and capability status.

## Non-goals

- A session-wide multi-torrent disk scheduler, parallel positional-I/O
  execution, OS cache redesign, mmap, direct I/O, disk cache, or storage
  priority system.
- One UI row or application event per 16 KiB block, payload bytes on the
  application boundary, or a complete history of disk jobs.
- A torrent-specific Disk authority. Rows may be locally filtered by torrent
  identity later.
- File or piece priority controls, disk-space management, relocation, full
  filesystem error taxonomy, or benchmark claims.
- A long-lived Speed history backend or a new chart dependency.
- An Android Disk screen or visible desktop launch. Shared contracts and
  proportional Android compilation remain required.
- Public-swarm traffic or libtorrent speed comparison.

## Reference Dossier

Pinned libtorrent `2.0.13` revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/disk_interface.hpp` defines async read/write/hash
  ownership and observer-based backpressure.
- `src/disk_buffer_pool.cpp::{allocate_buffer,free_buffer}` applies a configured
  queued-byte high watermark and wakes observers below the low watermark.
- `src/peer_connection.cpp` paths using `disk_observer` suspend socket reads
  under `bw_disk` and resume from the observer callback.
- `src/mmap_disk_io.cpp::{async_write,async_hash,add_job,submit_jobs}` owns job
  admission, generic/hash pools, per-storage fences, and incremental 16 KiB
  hashing without a contiguous piece allocation.
- `src/session_stats.cpp` defines queued/running/blocked disk gauges, queued
  bytes, operation counts, cache behavior, and duration metrics.
- `examples/session_view.cpp` shows aggregate disk metrics as the primary
  inspection surface.
- `test/test_fence.cpp`, `test/test_storage.cpp`, `test/test_read_piece.cpp`,
  and `simulation/disk_io.cpp` cover fence ordering, storage mappings, complete
  piece reads, delayed operations, cancellation, and watermark wakeups.

Adopted behavior: bounded queued bytes, high/low pressure recovery, explicit
current versus cumulative metrics, storage fencing, incremental hashing, and a
session-oriented inspection vocabulary. Intentional differences: the first
RSTorrent implementation remains one serialized torrent-local storage owner;
it does not imitate libtorrent's mmap cache or thread pools.

Local JSTorrent revision
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/engine/src/core/disk-queue.ts` and its tests show bounded pending
  and running work, worker count, and product-visible queue statistics.
- `packages/engine/src/adapters/native/native-batching-disk-queue.ts` owns a
  different platform-global shape.
- `packages/ui/src/tables/DiskTable.tsx` renders every pending/running job.

RSTorrent adopts pipeline visibility but deliberately does not expose the raw
queue or make the UI mirror one backend. No source or fixture is copied.

## Invariants And Resource Limits

- Mutable storage state remains owned by the content-storage pipeline.
- Snapshots are read-only and never used as commands or scheduling inputs.
- `requested >= resident` is not assumed; requested bytes may arrive or be
  cancelled independently. No gauge is derived by subtracting unrelated
  cumulative counters.
- Resident payload and storage command/completion queues retain explicit hard
  bounds. Pressure enters at the configured high watermark and clears only at
  a lower watermark.
- Once pressure is active, new piece/block assignment stops. Already-promised
  peer payload remains bounded by the independent outstanding-request and
  resident-payload ceilings.
- Storage gating freezes or reclassifies peer request aging; it cannot evict a
  useful peer solely because the disk owner is slow.
- Accepted block accounting is exact once across receive, store, verify,
  cancellation, duplicate, late response, and hash-failure paths.
- At most one Disk row exists per active piece attempt. No row contains block
  payload or per-block objects. Recent terminal retention, if implemented, is
  time- and count-bounded.
- Counter fields are monotonic within one engine task. Application replacement
  is explicit and does not merge unrelated task epochs.
- All integers crossing JSON that may exceed JavaScript's exact range use the
  existing decimal-string convention.

Initial profile limits remain those owned by `DownloadResourceLimits`:
desktop resident payload is at most 32 MiB and outstanding requests at most
256 MiB; Android resident payload is at most 16 MiB and outstanding requests at
most 128 MiB. This tactical may introduce derived high/low watermarks but does
not silently expand those caps.

## Owner, Task, Cancellation, And Data Flow

```text
content supervisor
  | accepts peer payload only within limits
  v
ContentStoragePipeline --> one joined storage task --> filesystem/hash
  | immutable snapshot            |
  +---------- DownloadControl -----+
                    |
       ApplicationService active-download set
                    |
       ViewHub session Disk projection
                    |
     leased session_disk view / polling
                    |
       strict TS adapter / Zustand
                    |
        Disk summary + piece table
```

The storage task remains cancelled and joined by the existing supervisor. The
application samples active controls when a Disk view is opened or polled and
publishes a coherent replacement only when semantic values change. View polling
does not create an engine timer task. Removing, pausing, completing, or failing
an active task publishes a terminal/empty session state before its owner is
dropped. A future active-download map can aggregate the same per-torrent
snapshot without changing the view vocabulary.

## View Contract

Add capability `session_disk` and `ViewSpec::SessionDisk`. Conceptually:

```text
DiskPressure = idle | normal | elevated | backpressured | draining | error

DiskPipelineView {
  pressure,
  intake_backpressured,
  sample_millis,
  resident_limit_bytes,
  resident_low_watermark_bytes,
  requested_bytes,
  resident_bytes,
  queued_write_bytes,
  writing_bytes,
  hashing_bytes,
  received_bytes_total,
  stored_bytes_total,
  verified_bytes_total,
  receive_rate_bytes,
  write_rate_bytes,
  hash_rate_bytes,
  write_operations_started/completed,
  hash_operations_started/completed,
  write_queue_wait/service totals and maxima,
  hash_queue_wait/service totals and maxima,
  pressure_transition_count,
  backpressured_millis_total,
  last_error,
}

DiskPieceView {
  row_id,
  torrent_id,
  torrent_name,
  piece_index,
  piece_length,
  state,
  requested_bytes,
  received_bytes,
  stored_bytes,
  attempt,
  age_millis,
  queue_age_millis,
  operation_age_millis,
  error,
}

ViewSnapshot::SessionDisk { pipeline, pieces }
ViewPatch::SessionDisk { pipeline, upsert, removed }
```

Unsupported or unavailable data is `null`, not zero. An empty piece collection
with an idle pipeline is a valid ready state. Patches replace the pipeline
summary and key piece rows. View-set reset and lease recovery replace both from
one coherent snapshot.

The selected global Disk tab requests this view even when no torrent is
selected. Switching away evicts it. Requested delivery is no faster than
100 ms; actual polling remains client controlled and rate calculations carry
their exact observation interval.

## Presentation Contract

- Left/session pipeline cards show requested, resident/capacity, queued write,
  writing, hashing, stored, verified, and current pressure.
- Rates and duration cards label their sampling/cumulative meaning.
- A compact CSS pipeline bar makes the limiting stage visible without relying
  on color alone.
- The table title is **Active storage pieces**. Default columns are Torrent,
  Piece, State, Requested, Received, Stored, Queue age, Operation age, and
  Error. Existing table column visibility, width, stable typed sort, and live
  re-sort behavior apply.
- There is no row click or navigation to Pieces.
- Compact/phone layouts stack cards, preserve the pipeline order, and allow the
  virtual table to scroll horizontally without clipping the page.
- `slow-disk-pressure` visibly moves from normal to backpressured/draining and
  recovers to normal/idle while keeping row identity stable.

## Adversarial Validation

1. A deterministic pressure state machine proves high-water entry, no chatter
   between high/low marks, low-water recovery, counter monotonicity, and reset.
2. Delayed writes saturate storage while a peer continues sending already
   promised payload. Resident/queue high-water marks remain bounded.
3. No new requests are assigned during pressure; after draining below the low
   watermark, scheduling resumes and completes.
4. Peer request deadlines do not fire solely during storage gating.
5. Cancellation while queued/writing/hashing joins the owner and publishes no
   ghost active row.
6. Write error and hash failure report different terminal states and preserve
   verified-content integrity.
7. Application view snapshot/patch, active-task replacement, torrent removal,
   queue overflow/reset, lease expiry, and browser-suspension recovery are
   deterministic.
8. Headless UI covers named scenario time, keyboard/tab semantics,
   accessibility, 250 active rows, phone/compact/wide geometry, high-DPI canvas
   independence, and exact state labels.
9. A controlled loopback seed plus injected storage delay reaches
   backpressured state, resumes, verifies exact output, and leaves empty active
   rows after joined completion.

## Implementation Order

1. Land this tactical and the shared topic.
2. Add engine snapshot vocabulary, exact current gauges, pressure hysteresis,
   piece-attempt retention, and adversarial storage tests.
3. Add the application session projection, view spec/capability,
   snapshot/patch/recovery behavior, and generated contracts.
4. Add strict live/demo frontend state plus the global tab split and Disk UI.
5. Add the deterministic scenario, headless product tests, and controlled
   delayed-storage interoperability proof.
6. Run repository gates, record evidence in topics, mark complete, and commit a
   clean slice before Tactical `045` begins.

## Required Gates

- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- generated contract check and Android shared-contract compilation
- web lint/typecheck/unit tests/build
- deterministic Playwright scenario/accessibility/responsive tests
- isolated controlled delayed-storage browser proof

No visible product client or public network is required.

## Stopping Condition

This slice is complete when the global Disk view truthfully shows every stage
of a controlled slow-storage pipeline, explicit high/low pressure stops and
resumes intake within all hard bounds, storage delay cannot falsely punish the
peer, exact content reaches verified completion, application and browser lease
recovery produce coherent fresh state, all proportional gates pass, evidence
is recorded, and the working tree is clean.

## Escalation Contract

Pure snapshot types, focused storage/view module extraction, derived
watermarks, deterministic test-only storage delay, generated-contract changes,
global tab grouping, CSS-only charts, fixtures, and isolated harness changes
are authorized. Stop for direction if evidence requires a new storage backend,
parallel disk executor, session scheduler, persistence migration, public
authentication policy, new chart/table dependency, public-swarm traffic,
visible app launch, destructive data action, or architecture beyond the shared
topic.

## Implementation And Evidence

The engine now owns an immutable bounded storage-runtime snapshot alongside
its existing scheduler and storage facts. Resident payload uses distinct 75%
high and 50% low watermarks. Entering pressure stops new request assignment;
recovery shifts request and replacement deadlines by the gated duration so a
slow storage owner does not manufacture a peer stall. Piece-attempt rows retain
only canonical byte ranges and aggregate at piece granularity. Integrity
failure remains a piece-attempt failure, while filesystem failure alone sets
the session storage-error state.

`ViewHub` aggregates active torrent samples into one `session_disk` projection
with decimal-string counters, keyed piece patches, exact reset behavior, and
at most the existing view-set delivery cadence. Task completion and
cancellation explicitly remove their sample, producing an idle pipeline and
empty active set instead of retaining a ghost terminal owner. The gateway's
bounded `RSTORRENT_TEST_*` delay and resident-limit controls are accepted only
by unauthenticated loopback development mode and exist solely for controlled
evidence.

The React surface requests Disk independently of torrent selection, separates
session tabs from torrent tabs, and renders a statistics-first pressure panel
plus a virtual piece-attempt table. The permanent `slow-disk-pressure`
scenario passed wide, compact, and phone geometry, keyed 64-row scale, and an
axe serious/critical scan with no findings.

The controlled production-web proof used libtorrent `2.0.13.0` as a loopback
seed for a 4 MiB payload plus a 7,000-byte prefix in 17 pieces. With a 128 KiB
resident limit and 150 ms injected write delay, the browser observed
`Backpressured`, 96 KiB resident at the 96 KiB high watermark, queued
piece-level work, and paused intake. It then observed exact verified
completion, idle pressure, zero active rows, joined gateway/browser/seed
owners, and external SHA-1 equality. No public network or visible client was
used.

Validation passed:

- all 155 non-ignored engine unit tests and all 67 session unit tests;
- workspace compilation including the Rust Android adapter;
- generated TypeScript/schema regeneration with no hand-edited contract;
- 59 web unit tests, TypeScript checking, and the production Vite build;
- the focused deterministic Playwright accessibility/responsive scenario; and
- the isolated controlled slow-storage browser proof above.

Full workspace clippy and tests are recorded with the closing commit. The
deliberate deferrals remain a session-wide storage scheduler, parallel I/O,
long-window Speed history, native Android presentation, and broader storage
failure policy. Tactical `045` owns the next selected-torrent Pieces canvas.
