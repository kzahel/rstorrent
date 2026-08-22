# Tactical 114: Session-Wide Concurrent Torrent Admission

Status: **Complete** on 2026-08-09. Schema 17, the durable automatic download
queue, session-wide resource authority, concurrent application admission,
shared product controls, controlled performance evidence, and physical Pixel
7a evidence are implemented. It follows graduated Tactical `112` and
evidence-limited closed Tactical `113`; `capability-readiness.md` owns the
current queue after this graduation.

Topics: `capability-readiness`, `application-control`, `application-view-api`,
`client-persistence`, `download-correctness`, `peer-lifecycle`,
`storage-throughput-architecture`, `incoming-reachability-and-seeding`,
`performance-and-live-evidence`, `oracle-driven-engine-campaign`

Dependencies: completed Tactical
[`039`](039-generous-download-resource-pipelines.md) established the bounded
download working sets whose authority this slice moves to the session;
Tacticals [`054`](054-bounded-independent-storage-execution.md) and
[`067`](067-dynamic-platform-file-acquisition.md) established bounded storage
execution and the session file pool; Tactical
[`082`](082-bounded-multi-peer-upload-ownership.md) established the shared
connection budget and fair session upload scheduler; Tactical
[`084`](084-persisted-client-connection-and-seeding-settings.md) established
the persisted client-settings surface; Tacticals
[`086`](086-long-lived-torrent-peer-runtime.md) and
[`108`](108-serialized-torrent-control-and-observable-checking.md) established
the long-lived peer runtime and serialized per-torrent control owner; Tactical
[`097`](097-live-client-settings-and-replaceable-session-generations.md)
established live setting reconciliation and generation-fenced replacement;
and Tactical [`110`](110-atomic-download-now.md) established the semantic
`Download now` intent that this slice admits through a queue instead of
rejecting as busy.

## Decision And Motivation

Replace the application's one-active-torrent slot with a session auto-manager
that can retain hundreds of runnable torrents while admitting a small,
configurable number of simultaneous downloads under shared session resource
ceilings.

The important change is not replacing an `Option` with a map. Today's download
memory and disk limits belong to one content runtime. Starting several copies
unchanged would multiply the advertised bounds by the number of active
torrents, let one torrent monopolize disk or connection opportunities, and
make mobile behavior accidental. This slice therefore moves resource
authority above the torrent before it permits concurrent payload owners.

The user-visible policy follows mature libtorrent behavior where the concepts
match: durable queue order, separate download and checking admission, automatic
promotion when capacity becomes available, and work-conserving limits. The
implementation does **not** copy libtorrent's classes, source, fixtures, or
task graph. A small runtime-independent Rust state machine owns policy, while
existing RSTorrent torrent controllers and engine tasks retain their local
invariants.

This is deliberately the downloading half of mature multi-torrent operation.
Completed torrents continue to use Tactical `082`'s already session-wide fair
upload choker. Durable seed goals, seed-rank activation, finite bandwidth
limits, and adaptive operating-system memory-pressure policy are follow-on
slices because each needs its own persisted facts, failure policy, and
evidence.

## Stopping Condition

This tactical is complete when all of the following hold:

1. A persisted automatic download queue replaces every ordinary
   different-torrent `busy` rejection, and startup restores all eligible run
   intent rather than stopping after the first torrent.
2. The session admits up to the effective active-download limit concurrently,
   defaults to three on desktop and at most two on Android, and promotes the
   next eligible torrent without another client command when a slot opens.
3. A queued torrent owns no content task, peer connection, tracker/DHT announce
   task, storage job, or timer. Retaining 100 or 500 queued torrents therefore
   does not multiply runtime work by catalog size.
4. Outstanding request bytes, received-but-uncommitted payload bytes, active
   piece bytes and count, storage write/hash execution, open files, and peer
   connections have session-wide ceilings. Starting more torrents cannot
   multiply any former per-torrent ceiling.
5. Download memory, disk, and outbound connection admission are
   work-conserving and fair across runnable torrents: one or two torrents can
   use otherwise idle capacity, while a continuously ready peer cannot starve
   another continuously ready torrent.
6. Checking remains independently limited to one torrent, shares the same
   storage/hash authority, and cannot silently multiply disk work beside
   downloading.
7. Pause, Resume, `Download now`, queue movement, completion, error, removal,
   live limit changes, restart, and shutdown have deterministic durable and
   runtime outcomes with exact cancellation and joined termination.
8. Application views and the shared web client distinguish queued, active,
   checking, seeding, paused, and error states; expose stable queue position
   and configured/effective download limits; and never infer scheduler state
   locally.
9. Controlled one-, two-, and three-torrent evidence meets the performance and
   fairness gates below, a 100-runnable-torrent scenario meets every resource
   high-water bound, and a 500-complete-torrent scenario preserves Tactical
   `082`'s global upload-slot and fairness behavior.
10. The tactical, owning topics, readiness matrix, generated application
    contract, and recorded validation all describe the landed behavior and
    its deliberate deferrals exactly.

## Product And Durable-State Contract

### Intent, queue order, and operational state

Three facts remain distinct:

- `desired_state` is the user's durable running or paused intent;
- `download_queue_position` is durable ordering among incomplete torrents;
  and
- active, queued, checking, stopping, or seeding is derived runtime state.

The store does not persist `active = true`. On restart, one scheduler generation
loads durable intent and current torrent facts, chooses the admitted set, and
converges runtime ownership. A crash can therefore lose transient work but
cannot turn a derived slot decision into durable user intent.

Incomplete torrents receive a queue position when first made runnable. Pause
retains the position so Resume normally returns to the same place. Completion
removes the download queue position and hands the torrent to existing complete
seeding eligibility. If a later recheck or selection change makes it incomplete
again, running intent appends it to the queue unless the same transaction
specifies a different placement. Archive and removal exclude the torrent.

Queue order is stored as a signed 64-bit sortable integer. Append, move to top,
move to bottom, removal, and bounded renumbering are single SQLite
transactions. The product derives contiguous one-based display positions from
the sort order; it does not require every row to be rewritten merely to display
an ordinal. Near integer exhaustion triggers a deterministic dense renumber in
the same transaction before the requested mutation commits. Stable torrent ID
breaks impossible legacy ties during migration and repair.

### Commands

Ordinary Resume expresses automatic management. It succeeds durably even when
all slots are occupied and reports queued state rather than `busy`. Pause
removes an active torrent from admission, cancels and joins its exact runtime
generation, and immediately permits promotion. Pausing an already queued
torrent creates no runtime task.

`Download now` keeps Tactical `110`'s atomic wanted-plus-running semantics and
also moves the torrent to the head of the download queue in that same durable
transaction. It does not bypass session resource ceilings or evict a healthy
active torrent. If a slot is available it starts; otherwise it is first to be
promoted. Exact request replay returns the recorded result but reconciles only
current durable state, so replay cannot undo a later Pause or queue move.

Add bounded semantic `move_download_to_top` and `move_download_to_bottom`
commands. They accept only an incomplete retained torrent with a queue
position, use the existing request-receipt and expected-revision rules, and
change no run intent. Arbitrary numeric position assignment, drag-and-drop,
and user-authored torrent priorities are not part of this slice.

Metadata acquisition counts against `active_downloads`. A magnet waiting for
metadata owns peer, connection, metadata-buffer, tracker, and DHT work and must
not provide a task-creation escape hatch for hundreds of queued magnets.
Checking does not count as downloading but has its own fixed limit of one and
shares disk/hash resources.

### Client setting and platform cap

Add persisted `active_downloads` to `ClientSettings`:

| Property | Contract |
| --- | --- |
| Meaning | Maximum automatically admitted incomplete content runtimes |
| Fresh default | `3`, matching pinned libtorrent |
| Accepted configured range | `1..=20` |
| Desktop effective value | Configured value |
| Android effective value in this slice | `min(configured, 2)` |
| Apply behavior | Live, through scheduler reconciliation |

The application exposes both configured and effective values and the reason
for a platform clamp. The shared web Settings surface edits the configured
value. Android consumes the same contract but this tactical does not add a
Compose settings control.

Increasing the limit immediately admits the next eligible torrents. Decreasing
it stops admitting new work and gracefully demotes the lowest-priority active
torrents until the effective limit is met. Demotion uses the ordinary
cancel/join/storage-fence path and retains durable running intent and queue
position. It is not an error or a durable Pause.

The schema version used by implementation is the next available version when
this tactical begins. Under the currently planned order, Tactical `111` uses
schema `15`, Tactical `112` uses schema `16`, and Tactical `113` adds no store
migration, making this migration `17`; implementation must reconcile that
assumption against repository state rather than overwriting a concurrent
migration.

## Admission Policy

### Pure scheduler

A runtime-independent `TorrentAutoManager` receives bounded snapshots of
durable intent, queue order, completion/checking facts, active generations,
the effective limit, and terminal outcomes. It returns explicit start, retain,
stop, and no-op decisions. It owns no task, socket, filesystem object, SQLite
connection, channel, or async clock.

Downloads are considered in durable queue order. Already active eligible
torrents retain their slots unless they become ineligible, the effective limit
shrinks, or an explicit future preemption policy says otherwise. Newly queued
work never churns healthy active downloads. When demotion is required, the
least-preferred active torrent is the one latest in current queue order; stable
torrent ID breaks ties.

The manager is event driven, with a 30-second maintenance reconciliation
matching pinned libtorrent's default interval as a lost-wakeup safety net.
Durable commands, runtime completion/failure, metadata/completion transitions,
checking transitions, and settings changes coalesce a wake immediately. The
timer does not poll or spawn work per torrent.

This first slice deliberately counts a slow but progressing active download
against the limit. Pinned libtorrent's `dont_count_slow_torrents = true`
default depends on mature inactivity thresholds and separate tracker, DHT,
LSD, and hard-active limits. Importing only the exemption would allow hundreds
of low-rate runtimes to bypass RSTorrent's memory and task bounds. Activity-
aware soft limits may follow only with session resource admission and explicit
hard caps intact.

### Work-conserving behavior

The active-torrent limit controls runtime ownership, not bandwidth shares. If
only one torrent has useful work, it may consume the full session memory,
connection, disk, and unlimited-network capacity. With two torrents, idle
capacity is not reserved for a dormant member. Fair admission constrains
contention only when multiple torrents are continuously ready.

No finite-rate token bucket is inserted on the unlimited path. Future
session/per-torrent bandwidth channels must be attachable at the common
network admission boundary, but a disabled limiter remains a direct fast path
with no timer tick, quota fragmentation, or forced equal split.

## Session Resource Authority

The ownership change is:

```text
ApplicationService generation
  -> TorrentAdmissionOwner (one task, durable queue + active handle map)
       -> per-torrent controller/runtime, only for admitted torrents
       -> SessionDownloadResources
            -> request/payload/active-piece byte permits
            -> fair outbound connection admission + existing PeerBudget
            -> root-aware fair storage admission
       -> existing UploadScheduler and seed runtimes
```

The scheduler chooses *which torrents may run*. Resource owners decide *which
ready operation runs next* under the session ceilings. Neither guesses the
other's state from channel occupancy.

### Download memory

`DownloadResourceLimits` becomes a session profile rather than a per-content-
runtime allowance. Initial totals preserve today's proven platform values:

| Resource | Desktop session total | Android session total |
| --- | ---: | ---: |
| Outstanding block-request bytes | 256 MiB | 128 MiB |
| Received payload awaiting storage ownership | 32 MiB | 16 MiB |
| Active piece working-set bytes | 256 MiB | 128 MiB |
| Active piece descriptors | 2,048 | 2,048 |

Every reservation is charged before a request, payload buffer, or piece
assembly changes ownership and released exactly once after commit, rejection,
generation cancellation, or task failure. Byte permits, not configured active
torrent count, prove the resident and in-flight high-water. Per-torrent queues
also retain small descriptor caps so a torrent cannot consume all bookkeeping
with zero-byte work.

Fair admission uses byte-cost deficit round robin. The base quantum is one
16 KiB request block; a larger bounded operation consumes multiple quanta.
Each continuously ready active torrent gets an opportunity before another
torrent receives a second new quantum at the same priority, while unused
credit cannot manufacture permits beyond the session ceiling. Control,
completion, and cancellation messages use separate small bounded lanes and do
not wait behind payload bytes.

### Storage and hashing

Implement the session-owned authority already specified by
`storage-throughput-architecture.md` before starting a second downloader:

- existing configured write and hash concurrency become session totals rather
  than per-torrent worker counts; production begins with the current `4` write
  and `4` hash maxima, subject to an equal or lower root/backend profile;
- work is grouped by resolved storage root and backend so one blocked SAF or
  filesystem root cannot consume every dispatch opportunity;
- per-root write and read concurrency, global hash concurrency, aggregate
  queued/resident write bytes, and the existing 40-handle file pool remain
  explicit independent bounds;
- ready work is byte-cost deficit-round-robin across torrents within a root,
  and roots are visited work-conservingly rather than through one global FIFO;
- generation fences, piece commit order, verification failure, publication,
  selection change, pause, removal, and shutdown retain the exact per-torrent
  storage invariants already proven; and
- a torrent's slow or failing root diagnoses that torrent/root and does not
  halt unrelated roots.

The storage owner accepts bounded descriptors only after their payload bytes
are charged to `SessionDownloadResources`. No `active_downloads * channel
capacity` calculation is allowed to become an unaccounted memory bound.

### Connections and discovery

Tactical `082`'s effective session `PeerBudget` remains the hard authority:
configured default 200, descriptor-derived clamp where available, and ten
incoming connections of slack. The existing per-download working sets of at
most 30 pending dials and 30 established peers remain soft torrent-local
bounds beneath it.

Add fair outbound admission above the shared budget. Each active torrent may
hold at most one pending request for the next dial opportunity; a
round-robin/deficit owner visits ready torrents and then transfers an acquired
budget generation into the existing connecting/established lifetime. A failed
dial, handshake failure, cancellation, duplicate rejection, panic, and normal
close release once. Incoming peers continue to use the established intake and
slack rules and are not forced through the outbound queue.

Only admitted downloads run tracker, DHT, PEX, metadata, or ordinary peer
discovery. Queued torrents retain their durable source data but do not
announce, scrape on a timer, dial, or keep a dormant content task. The current
per-content-runtime tracker-operation cap becomes one session-wide limit of
eight so concurrency cannot multiply it.

### Existing seeding resources

This slice does not put an `active_seeds` gate in front of complete torrents.
All currently eligible complete seeds may remain registered, bounded by the
existing 1,024 registrations, but peer work continues to share:

- the same session connection budget;
- eight global upload slots with one derived optimistic slot;
- ten global upload-read jobs;
- the 40-handle file pool; and
- Tactical `082`'s fair peer rotation and byte-charged send bounds.

Download changes must not reserve upload slots per torrent, replace the upload
choker, or make hundreds of idle seed registrations expensive. A separate seed
auto-manager may later adopt libtorrent-style seed rank and durable ratio/time
goals after session lifetime counters exist.

## Owner, Task, And Cancellation Map

| Owner | Mutable state | Starts work | Stop and termination path |
| --- | --- | --- | --- |
| `TorrentAutoManager` | Task-free queue/checking decision state | Nothing | Dropped with its caller |
| `TorrentAdmissionOwner` | Effective limits, coalesced wake, active handle map, terminal queue | Exact admitted torrent generations | Stops admission, cancels every active controller, joins every generation, then terminates |
| Per-torrent serialized controller | Latest durable intent, selection/checking generation, one admitted content runtime | Existing peer/check/storage work for one torrent | Existing generation fence, cancellation token, storage barrier, outcome, joined handle |
| `SessionDownloadResources` | Byte/descriptor permits and fair waiter queues | No detached work; grants bounded ownership | Closes admission, wakes waiters with cancellation, proves all permits returned |
| Session storage owner | Root queues, DRR deficit, active write/hash jobs, fences | Bounded blocking jobs under session/root caps | Rejects new work, cancels queued generations, joins active jobs, drains completion/fence paths |
| Existing `PeerBudget` and upload owner | Connection generations and seed peer grants | Existing incoming/outgoing peer and upload jobs | Existing close/cancel/join path, after content owners stop |

There is no task per queued torrent. The admission owner has one coalescing
wake cell/channel and one terminal channel bounded by the maximum active
downloads plus the one checker. Repeated commands set the latest-wake flag
rather than accumulating unbounded scheduler messages.

For each reconciliation, the owner reloads current durable facts, applies the
pure decision, and starts or stops exact generations. A successful command
receipt means durable intent committed; it does not promise immediate slot,
peer, storage, or payload progress. Views expose convergence separately.

Shutdown order is: stop accepting scheduler wakes and new resource
reservations; cancel and join active download/checking controllers; close and
join storage work after their clients terminate; then continue the established
seed, listener, discovery, persistence, and generation shutdown. A panic or
late completion is generation-fenced and cannot release another torrent's
permit or resurrect stale intent.

## Failure, Restart, And Reconciliation Rules

- A recoverable runtime failure affects only its torrent, releases every
  session permit, records the existing diagnosed error, and opens a slot. It
  does not retry in a hot loop or stop another torrent.
- A storage root failure diagnoses torrents using that root without stopping
  progress on a healthy root. Shared-resource invariant failure is a session
  diagnostic and fail-closed condition, not an excuse to exceed a ceiling.
- Completion promotes the next queued torrent only after the completing
  generation has crossed its storage fence and released download ownership.
  Seed registration may overlap only through the already bounded upload path.
- Startup repairs invalid/missing queue positions deterministically, loads all
  desired-running torrents, and admits only the effective limit. It never
  iterates by starting every torrent and then pausing the excess.
- An exact command replay after restart reuses its receipt and wakes current
  reconciliation; it never replays an old scheduler action.
- Live `active_downloads` replacement uses the existing setting-generation
  fence. A superseded application generation cannot start or stop work in the
  replacement generation.
- Queue compaction, migration, and movement are transactional. A crash exposes
  either the old total order or the new total order, never duplicate ownership
  or a partially shifted range.

## Product And Observability Surface

The application snapshot and change stream add authoritative fields sufficient
to render:

- configured and effective active-download limits plus clamp reason;
- session active-download and checking counts;
- per-torrent operational state: queued, starting, downloading, checking,
  stopping, seeding, paused, or error;
- one-based queue position where applicable; and
- resource-pressure diagnostics by category without exposing engine permit or
  Tokio implementation types.

The shared Transfers surface orders queued incomplete torrents by queue
position and offers Move to top/Move to bottom through the semantic commands.
Resume and `Download now` report accepted/queued outcomes in ordinary status
text rather than presenting a different torrent as a blocking error. The UI
waits for authoritative snapshots and performs no optimistic slot arithmetic.

Structured diagnostics include scheduler reason, torrent ID, generation,
configured/effective limit, active/queued counts, transition, and resource
category. High-water metrics cover all session byte permits, active pieces,
queued storage bytes/descriptors, write/hash jobs by root, connections and
fair-waiter depth, active content tasks, and seed registrations. Logs remain
separate from commands, views, and events.

## Stable Scenarios

The following scenario names remain stable even if test function names change:

| ID | Required behavior |
| --- | --- |
| `T114-C01` | Startup with five runnable incomplete torrents and limit three starts exactly the first three and creates no content task for the other two. |
| `T114-C02` | Completion of one active torrent releases its resources and automatically promotes the fourth. |
| `T114-C03` | Pausing an active torrent joins that exact generation and promotes the next; pausing a queued torrent starts nothing. |
| `T114-C04` | Resume durably queues without `busy`, retains an existing position, and appends only when no position exists. |
| `T114-C05` | `Download now` atomically changes wanted/running intent and moves the target to queue head without bypassing a full active limit. |
| `T114-C06` | Move-top/bottom, exact replay, request conflict, stale revision, crash/reopen, and near-overflow renumber preserve one total order. |
| `T114-C07` | Metadata acquisition consumes an active-download slot and a hundred queued magnets own no discovery or peer tasks. |
| `T114-C08` | Increasing the live limit starts eligible work; decreasing it gracefully demotes the latest active queue entries without changing durable run intent. |
| `T114-C09` | One torrent at limit three can consume every idle session memory, disk, and allowed connection resource up to its existing local soft caps. |
| `T114-C10` | Two continuously ready torrents both receive request/payload/active-piece permits; neither can consume more than the session total. |
| `T114-C11` | A small torrent completes beside a large continuously ready torrent without starvation or permanent head-of-line blocking. |
| `T114-C12` | Three active torrents never exceed the desktop or Android session memory high-waters, including cancellation with full queues. |
| `T114-C13` | Under a deliberately small peer budget, ready torrents receive outgoing dial opportunities round-robin and every generation releases once. |
| `T114-C14` | A slow or failed storage root does not stop a healthy root, and aggregate write/hash concurrency remains within its session/root limits. |
| `T114-C15` | One checker at a time shares disk/hash capacity with downloads and preserves pause/resume/checkpoint behavior from Tactical `108`. |
| `T114-C16` | Error, panic, hash failure, selection change, removal, and archive affect only the target torrent and cannot leak permits or resurrect stale work. |
| `T114-C17` | Shutdown with active, stopping, queued, checking, and seeding torrents joins every owner in dependency order with no late state mutation. |
| `T114-C18` | One hundred runnable downloads with effective limit three own exactly three content runtimes and bounded scheduler/resource queues. |
| `T114-C19` | Five hundred complete seed records preserve eight global upload grants, fair peer rotation, and bounded registration/task memory while downloads run. |
| `T114-C20` | Android's configured value above two remains visible but effective admission is two and uses Android session memory totals. |

## Performance And Resource Gates

Measurements use fixed local files, loopback/LAN peers, release builds, the
same storage root, warm-up rules, and repeated trials recorded with hardware,
OS, filesystem, commit, and configuration. Public-swarm evidence is optional
and cannot replace controlled correctness.

Required gates:

1. With only one runnable torrent, changing the configured limit from one to
   three regresses median payload throughput by no more than 5 percent and
   does not add periodic limiter wakeups or reserve resources for idle slots.
2. With two runnable torrents, aggregate payload throughput is at least 90
   percent of the equivalent one-torrent storage/network ceiling, unless a
   recorded component bottleneck and accepted follow-up supersede the gate.
3. With three continuously ready torrents, every torrent makes verified
   progress in every bounded observation window chosen by the harness, and a
   small fixture completes beside two large fixtures.
4. The 1/2/4/8 active-download sweep records aggregate throughput, per-torrent
   progress, CPU, resident memory, request/payload/piece high-waters,
   write/hash utilization, open handles, connections, and cancellation time.
   It informs the default but does not raise the accepted production maximum
   without a separate decision.
5. A slow-root versus fast-root run demonstrates useful fast-root progress and
   the declared per-root and session high-waters.
6. The 100-runnable and 500-complete scenarios prove task, queue, handle,
   memory, and connection bounds; no claim rests only on stable RSS after an
   unobserved queue spike.
7. A headless Android build/run proves two simultaneous downloads stay within
   Android memory totals and converge under Pause, completion, and shutdown.
   Physical low-memory/background-pressure evidence belongs to the deferred
   adaptive-profile tactical.

The tactical records distributions or repeated medians, not one favorable
run. A failed gate is either fixed or returned for an explicit product
decision; it is not silently reworded after measurement.

## Normative And Reference Dossier

There is no new BEP. Queueing and resource allocation are client policy, while
all peer-wire, tracker, DHT, storage-integrity, and application-persistence
contracts remain governed by their existing tacticals and references.

### Pinned libtorrent

Reference commit: `7d7fc38fac61177fa5e02148f791b2f65250b09d`
(`libtorrent-2.0.13`). The implementation inspected and recorded:

- `src/session_impl.cpp::auto_manage_checking_torrents`,
  `auto_manage_torrents`, and `recalculate_auto_managed_torrents` for separate
  checking/downloading/seeding order, hard/session discovery limits, graceful
  pause, and event-triggered recalculation;
- `src/settings_pack.cpp` for defaults `active_downloads = 3`,
  `active_seeds = 5`, `active_checking = 1`, `active_limit = 500`,
  `auto_manage_interval = 30`, `auto_manage_startup = 60`, and
  `dont_count_slow_torrents = true`;
- `src/torrent.cpp::seed_rank` for later seed-goal/rank vocabulary, not for an
  implementation in this slice;
- `simulation/test_auto_manage.cpp`, especially
  `dont_count_slow_torrents`, `count_slow_torrents`, `force_stopped_download`,
  `force_started`, `seed_limit`, `download_limit`, `checking_announce`,
  `paused_checking`, `stop_when_ready`, `resume_reject_when_paused`,
  `no_resume_when_paused`, `no_resume_when_started`, and
  `pause_completed_torrents`;
- `test/test_torrent.cpp` queue and paused-queue cases;
- `test/test_bandwidth_limiter.cpp` cases `equal_connection`,
  `conn_var_rate`, `torrents`, `torrent_var_rate`, `bandwidth_limiter`,
  `max_bandwidth_channels`, `peer_priority`, and `no_starvation`; and
- `simulation/test_file_pool.cpp::file_pool_size`.

Behavior adopted here is the default active-download count, durable automatic
queue ordering, independently bounded checking, work-conserving resource
admission, automatic promotion, and graceful demotion. Intentional differences
are:

- queued downloads have no dormant engine task, rather than a paused torrent
  object carrying engine ownership;
- slow torrents count against the first-slice active limit until RSTorrent has
  the necessary activity and hard-limit policy;
- `Download now` is queue-head intent, not an unbounded force-start bypass;
- metadata acquisition counts as active downloading;
- Android applies an explicit effective cap and shared memory profile; and
- seed auto-management, finite bandwidth channels, and separate DHT/tracker/
  LSD active limits are deferred.

Libtorrent is the completeness and edge-case oracle, not an architectural or
source donor. Tests are independently authored against the behavior above; no
source, fixture, class layout, persistence encoding, or test data is copied.

### JSTorrent product history

The implementation inspected local JSTorrent commit
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/engine/src/core/torrent-queue-manager.ts` for the product semantics
  of automatic download/checking limits, queue position, force activation, and
  seed rotation;
- `packages/engine/src/config/config-schema.ts` for its five-download,
  two-seed, one-checker, 20-peer-per-torrent, and 200-global-peer defaults; and
- `packages/engine/test/core/torrent-queue-manager.test.ts` for queue
  enforcement, top/bottom movement, stop/restart, force behavior, seed
  rotation, and checking-queue scenarios.

RSTorrent adopts the demonstrated need for queue operations and scale tests,
not JSTorrent's mutable manager or fixed five-minute seed rotation. It also
rejects JSTorrent's exemption for metadata/file-selection wait states because
those states can own bounded but material network and metadata work. Existing
Tactical `082` upload scheduling is retained, and later seeding activation
should prefer demand/goal-aware rank over blind time rotation.

### Existing RSTorrent source boundaries

The implementation dossier revisited:

- `crates/rstorrent-session/src/application.rs`, especially
  `ApplicationService::active_torrent`, `install_active_download`, startup
  `restore_running`, and `start_if_possible_with_mode`, which encode the
  current single-slot behavior;
- `crates/rstorrent-session/src/torrent_runtime.rs` and the Tactical `108`
  controller for per-torrent generation ownership;
- `crates/rstorrent-engine/src/driver.rs::DownloadResourceLimits` and
  `crates/rstorrent-engine/src/swarm.rs::DEFAULT_MAX_ACTIVE_PIECES` for limits
  that must become session totals;
- `crates/rstorrent-engine/src/driver/storage_pipeline.rs` and
  `docs/topics/storage-throughput-architecture.md` for the storage-authority
  move;
- the existing peer-budget, tracker-operation, upload-scheduler, upload-read,
  and session file-pool owners; and
- store/settings/view adapters and the generated TypeScript contract for the
  durable queue and observable state.

## Validation Plan

### Deterministic policy and persistence

- Pure scheduler tables cover zero/one/many limits, queue order, active
  retention, demotion, checking independence, completion, errors, ties, and
  stable decisions under identical input.
- Store tests cover migration from every supported schema, queue insertion,
  pause retention, completion removal, re-incompletion, top/bottom movement,
  near-overflow renumber, exact replay, request conflict, stale revision,
  rollback, repair, and reopen.
- Model-based command sequences compare the store/scheduler outcome with a
  small reference model over at least 1,000 torrents and deliberate crash
  boundaries.
- Contract round trips cover Rust, JSON Schema, generated TypeScript,
  validators, snapshots, events, and unknown/new field compatibility.

### Runtime and adversarial evidence

- Execute every `T114-C01`--`T114-C20` scenario with deterministic clocks or
  scripted peers/storage where wall time is not the behavior under test.
- Inject dial, handshake, tracker, DHT, metadata, read, write, hash, publication,
  channel-close, task-panic, and shutdown failures while permits are held.
- Assert exact high-water counters and zero outstanding generations after each
  test; a timeout alone is not proof of clean termination.
- Run concurrent selection, Pause, Resume, `Download now`, completion,
  settings replacement, removal, and shutdown races against current durable
  intent.

### Controlled interoperability and product evidence

- Extend `tests/interop` with one orchestrated multi-torrent scenario using
  independently hash-verified fixtures and the pinned libtorrent peer.
- Exercise RSTorrent downloading three torrents concurrently, including one
  small fixture, while another retained torrent remains visibly queued and is
  promoted on completion.
- Exercise RSTorrent seeding while downloads run, with multiple interested
  peers proving the existing global upload grant and connection behavior.
- Prove Transfers queue actions and settings through authoritative application
  updates in component and headless-browser tests.
- Run the no-window Android scenario named by the performance gates; do not
  launch a visible product client merely for engine evidence.

### Commands

At minimum, run and record:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
git diff --check
```

Also run the focused controlled interoperability, performance/resource,
headless browser, and Android commands added by the implementation. Rerun
contract generation cleanly and prove it produces no diff.

## Completion Record

### Landed ownership and behavior

Schema 17 adds `ClientSettings.active_downloads` and one unique sortable
`download_queue_position` per incomplete torrent. Queue append, head/bottom
movement, pause retention, completion removal, replay, conflict handling,
near-overflow renumbering, and version-16 migration stay inside the existing
SQLite transaction authority. `download_queue.rs` owns the task-free ordering
operations, while `store_schema.rs` owns the current schema and newest bounded
migration; historical one-off migrations deliberately remain with their
existing projection helpers.

`TorrentAutoManager` is a runtime-independent admission transition. The
application generation owns one active-download map, one coalesced wake plus
30-second lost-wakeup reconciliation task, and exact controller generations.
It restores every durable running intent, admits the stable queue head up to
the effective limit, retains healthy active torrents, gracefully joins excess
generations after a limit decrease, and promotes work after terminal outcomes.
Checking remains a separate one-torrent lane. Queued torrents retain catalog
and source facts but own no content generation; discovery registrations remain
task-free and become active only for admitted generations.

`SessionDownloadResources` owns aggregate request, payload, active-piece,
write, hash, tracker-operation, and outbound-turn admission. Each admitted
torrent receives a generation-scoped registration tied to its storage root.
Memory reservations use the existing platform totals, storage execution is
work-conserving and fair across roots and torrents, outbound turns yield
between ready torrents, and the existing 40-handle file pool and session peer
budget remain the final descriptor/connection authorities. Explicit release
after controller join prevents a completed generation retained by a client
handle from delaying promotion or inflating live registration counts.

Browser/Tauri now render authoritative queued/active/checking/seeding/paused/
error state and queue position, expose Move to top/Move to bottom, and edit the
configured active-download count. The application contract exposes configured
and effective limits, clamp reason, and live active/checking counts. Android
uses the same generated contract and session resources, reports a visible
platform clamp, and deliberately adds no Compose settings control in this
slice.

The implementation used the pinned reference behavior listed above as its
completeness oracle. It adopts automatic promotion, durable order, default
three downloads, one checker, work conservation, and graceful limit changes.
The intentional differences remain exactly those accepted in the plan:
queued downloads have no dormant engine task, slow downloads do not bypass a
hard active count, metadata consumes a slot, `Download now` moves queue intent
without force-starting, Android is capped at two, and seed ranking, inactive-
rate exemptions, and finite bandwidth policy remain deferred.

### Scenario evidence

| Scenarios | Executable evidence |
| --- | --- |
| `T114-C01`, `C03`, `C08` | `startup_and_live_limit_changes_admit_only_durable_queue_heads`, the existing joined-pause application tests, and `TorrentAutoManager::{starts_in_durable_order_up_to_the_limit,ineligible_active_torrents_stop_and_open_capacity,shrinking_demotes_the_latest_active_queue_positions}` cover startup, queued/active pause semantics, stable retention, promotion eligibility, and live growth/shrink. |
| `T114-C02`, `C11`, `C12` | `terminal_wake_promotes_the_next_download_without_a_command` and `three_payload_downloads_progress_and_completion_promotes_the_fourth` run one small and two larger admitted payloads beside a queued fourth, verify exact publication, promote without a command, respect every memory ceiling, and finish with zero registrations or bytes. |
| `T114-C04`--`C06` | `download_queue_is_durable_replayable_and_keeps_pause_position`, `download_files_commits_one_replay_safe_wanted_and_running_revision`, `near_overflow_is_renumbered_inside_the_transaction`, and `thousand_entry_queue_matches_model_across_moves_and_rollbacks` cover durable Resume/Download-now intent, head/bottom movement, replay/conflict/stale revision, restart, 1,000 entries, 2,000 model-checked moves, and repeated rollback boundaries. |
| `T114-C07`, `C18` | `checking_and_metadata_acquisition_have_explicit_admission_shapes` and `one_hundred_runnable_torrents_own_only_three_content_generations` prove metadata consumes admission and a 100-row runnable catalog owns three resource generations, three active discovery registrations, and no queued storage work. |
| `T114-C09`, `C10`, `C13`, `C14` | The `session_resources` memory, request-fairness, storage/root-fairness, cancellation, tracker-ceiling, and outbound-turn tests plus `separate_swarms_share_request_and_active_piece_ceilings` prove shared hard totals, idle-capacity use, fair contention, slow-root isolation, and exact release. |
| `T114-C15`--`C17` | The Tactical `108` checker suite now runs through the same registered session storage/hash authority; existing injected storage/hash/failure, selection/removal/archive, generation-cancellation, and application shutdown suites remain green, while final-registration drop and joined-release tests prove terminal resource recovery. |
| `T114-C19` | `five_hundred_complete_seeds_share_upload_slots_with_three_downloads` retains 500 complete registrations while three downloads run and ten interested incoming peers share exactly seven regular plus one optimistic global upload grant, the 200-peer ceiling, and the 40-handle pool; terminal download resources are zero. |
| `T114-C20` | `platform_download_cap_is_visible_without_rewriting_configuration` and the physical `product-concurrent-downloads` profile prove configured three/effective two, two active plus one queued, promotion, Android memory totals, exact output hashes, and terminal cleanup. |

The shared-client evidence includes generated-schema/validator round trips,
React component and live-adapter tests, and the headless Chrome
`inspection-demo.spec.ts` assertions for the active-download setting and
authoritative queue actions. No client computes queue order or admission.

### Controlled performance and resource evidence

The authoritative release run used source commit
`11246b42d734e8299135c80cd2637beb25817668`, pinned libtorrent `2.0.13.0`,
128 MiB per torrent with 1 MiB pieces, one warm-up per case, five recorded
repetitions, and independent libtorrent source sessions so an oracle thread
bottleneck could not masquerade as RSTorrent saturation. The host was an
arm64 Mac16,7 with 14 logical CPUs, macOS 26.5.2, and APFS.

| Active downloads | Median aggregate MiB/s | Median CPU core equivalents | Maximum RSS bytes |
| ---: | ---: | ---: | ---: |
| 1, configured limit 1 | 218.319 | 0.972 | 47,267,840 |
| 1, configured limit 3 | 214.419 | 0.956 | 46,776,320 |
| 2 | 236.122 | 1.081 | 132,562,944 |
| 3 | 179.632 | 1.190 | 135,856,128 |
| 4 | 154.563 | 1.223 | 151,781,376 |
| 8 | 134.915 | 1.221 | 162,856,960 |

The one-torrent limit-three/limit-one ratio was `0.9821`, passing the `0.95`
floor. The two-torrent/one-torrent aggregate ratio was `1.0815`, passing the
`0.90` floor. Every concurrent torrent produced 3--17 measured progress
samples. Across the sweep, maxima were 41,762,816 request bytes, 33,554,432
payload bytes, 193,986,560 active-piece bytes and 185 pieces, four writes,
four hashes, eight registered generations, eight peers, eight open files,
162,856,960 RSS bytes, and 0.011074 seconds shutdown. Every terminal resource
counter was zero. The decline above two is recorded saturation evidence, not
a claim that the default or maximum should increase.

The physical API 37 Pixel 7a run used the no-window Android
`product-concurrent-downloads` profile against independently hash-verified
host fixtures. Configured three produced effective two, two active and one
queued; completion promoted the queued torrent. The registration high-water
was two and terminal registered/request/payload/piece/write/hash counts were
zero. High-waters were 193,304 request bytes, 32,768 payload bytes, 257,768
active-piece bytes, nine pieces, two writes, and two hashes, all within the
Android profile. The shared file-pool limit stayed 40; per-storage owned
high-waters were 12, 12, and 18 with pending high-water two. File descriptors
were 159 before, 186 at high-water, and 185 before successful harness cleanup.
The three published SHA-1 values were
`f2e61e6bd056677c1f9f0921e4b738e2c6453c0b`,
`0a23591285a92566a934aaa7246643741168686d`, and
`aeaf0325f5605ed9be5c320bf845f2e75ac7e234`.

### Logical commits

- `c9f4fbc` persists schema-17 queue and setting state and adds the pure
  scheduler; `5fb7fe4`, `c6d864e`, and `b10086c` establish shared memory,
  storage/root, outbound, tracker, and discovery authority.
- `969348d` exposes authoritative state and controls through generated clients;
  `f8e5cb3` adds controlled payload/resource and performance harnesses;
  `fee5b1c` adds the Android contract and physical profile.
- `4c80796`, `9a4df37`, and `11246b4` stabilize and isolate the performance
  oracle; `42bd27f` closes the 500-seed and headless-browser evidence; and
  `c545e91` model-checks the 1,000-entry queue and rollback boundary.

### Final validation

The final source and documentation checkpoint passed:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
npm run test:e2e --prefix clients/web -- \
  inspection-demo.spec.ts -g 'typed torrent ETA|torrent and file rows'
python3 -m py_compile tests/interop/multi_torrent_throughput.py \
  clients/android/run_bootstrap.py
git diff --check
```

Contract generation produced no diff. The web suite passed 239 tests with two
intentional skips; the focused browser run passed both selected cases. Earlier
in the same completed source checkpoint, the full Android build generated both
x86_64 and arm64 native libraries and Kotlin bindings, then passed
`assembleDebug` and `testDebugUnitTest`; the Pixel and performance commands
documented in `DEVELOPMENT.md` produced the retained evidence above.

No native host, daemon, per-torrent resource multiplier, seed auto-manager,
finite bandwidth policy, dynamic memory-pressure profile, or Compose settings
surface was introduced.

## Non-Goals And Follow-Ons

- Finite global or per-torrent upload/download rate limits, bandwidth-token
  channels, alternative traffic classes, or metered-network policy.
- Durable share ratio, seed time, seed-time ratio, seed rank, active-seed
  count, inactive-rate exemption, or automatic stop/archive goals.
- Tit-for-tat upload while incomplete or changes to the existing eight-slot
  complete-content upload scheduler.
- Dynamic reactions to Android `onTrimMemory`, foreground/background state,
  battery saver, thermal state, memory class, desktop memory pressure, or
  automatic profile selection. This slice uses fixed platform ceilings.
- Per-torrent connection, memory, or disk knobs; torrent weights; sequential,
  streaming, or deadline priorities; arbitrary numeric queue positions; or
  drag-and-drop ordering.
- A task or idle engine object for every retained torrent, or support for 500
  simultaneous payload downloads. The scale claim is hundreds retained with a
  small bounded active set.
- New BEPs, uTP, web seeds, remote-daemon APIs, extension IPC, or multi-process
  resource coordination.
- Changing Android/Compose presentation, aside from consuming the generated
  application contract required to remain compatible.
- Claiming a universal best active-torrent default from one development
  machine. Three follows the pinned mature default; Android's cap follows the
  existing smaller resource profile and must remain observable.

Follow-on tacticals should be ordered by measured need: durable seeding goals
and seed activation/rank; finite session and per-torrent bandwidth allocation
with an unlimited fast path; then adaptive platform/memory-pressure profiles.
Each reuses the session ownership established here instead of introducing a
second scheduler.

## Implementation Slices

1. Reconcile the reference dossier and repository schema, then add the pure
   scheduler, durable queue facts/transactions, setting, views, and
   deterministic model tests while retaining an effective active limit of one.
2. Move memory permits, storage/hash admission, and outbound connection
   fairness to session owners, proving current single-torrent behavior and
   high-waters before enabling concurrency.
3. Replace `active_torrent` with the admission owner and active generation map,
   enable desktop limit three and Android effective limit two, and complete
   runtime failure/cancellation/restart evidence.
4. Add shared Transfers/Settings behavior, generated-contract validation,
   controlled multi-torrent interoperability, Android, scale, fairness, and
   performance evidence.
5. Reconcile owning topics and readiness, record exact defaults/deviations/
   high-waters, graduate this tactical only if every stopping condition and
   gate passes, and leave the three named follow-ons explicitly unclaimed.
