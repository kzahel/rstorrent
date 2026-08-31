# Tactical 201: Durable Seeding Goals And Seed Admission

Status: **Active.** User direction on 2026-08-31 selected the exact pinned
libtorrent implementation semantics for the seed queue, durable counters,
timers, and goal ranking. End-to-end implementation was authorized on the same
date. The task-free policy, fresh schema-23, runtime-accounting, and combined
admission gates are implemented; generated clients, presentation, platform
validation, and final end-to-end evidence remain open.

Topics: `incoming-reachability-and-seeding`, `client-persistence`,
`application-control`, `client-surfaces`, `android-jstorrent-replacement`,
`runtime-configurations-and-headless-deployment`, `capability-readiness`,
`oracle-driven-engine-campaign`

Dependencies: completed long-lived peer-runtime Tactical
[`086`](086-long-lived-torrent-peer-runtime.md), concurrent admission Tactical
[`114`](114-session-wide-concurrent-torrent-admission.md), duplex upload
Tactical [`124`](124-duplex-verified-piece-upload.md), hierarchical transfer
limits Tactical [`134`](134-hierarchical-transfer-rate-enforcement.md),
retained peer-transfer totals Tactical
[`175`](175-retained-swarm-peer-transfer-totals.md), disposable-state Tactical
[`179`](179-disposable-incubation-state-epoch.md), direct-storage Tactical
[`191`](191-direct-filesystem-storage.md), and Android lifetime Tactical
[`200`](200-android-product-background-lifecycle.md).

## Implementation Progress

The first task-free gate now provides independently authored Rust values for
the exact defaults, any-one-threshold goal predicate, rank flags and demand,
tracker/live fallback, full/selected-finished scale, strict applicable-rate
classification, and delayed inactive/active transitions. Fourteen focused
tests pass. This gate creates no task, socket, database field, setting, client
contract, or behavior claim; the module's temporary dead-code allowance is
removed when the application owner consumes it in the next gates.

Validation at this checkpoint:

- `cargo fmt --all -- --check`;
- `cargo test -p rstorrent-session seed_policy`; and
- `cargo clippy -p rstorrent-session --lib -- -D warnings`.

The fresh durable gate now installs schema 23 and recognizes every schema
`1..=22` through the bounded disposable reset. The singleton round-trips the
typed active-seed limit and three exact goal thresholds. Each torrent stores
only monotonic payload totals/timers and explicit-unknown tracker counts; a
fixed transaction accepts at most 500 unique rows and rejects regression,
overflow, or malformed timer ordering. Fresh/reopen, hostile-state, exact
setting-boundary, batch-bound, and schema-22 payload-sentinel tests pass. The
batch writer and effective seed-limit helper are intentionally task-free and
temporarily allowed unused until the immediately following accounting and
admission gates consume them. `cargo test -p rstorrent-session` passes 311
tests with two opt-in cases ignored; the settings-bearing test subscription
budget was raised from 4 KiB to 8 KiB without changing the production 256 KiB
default or queue-count limits.

The accounting gate now installs one task-free session accumulator beneath the
existing joined application-maintenance owner. Exact peer-I/O metric sinks
observe successfully received or written payload for initiated downloads and
active or completed incoming uploads; protocol bytes and failed writes remain
excluded. Generation fencing covers late observations and download-to-seed
handover. One monotonic tick accrues the nested active/finished/full-seeding
timers, retains latest-known tracker counts, saturates every durable scalar at
SQLite's signed maximum with an observable event count, wakes at 1 MiB, flushes
timer-only dirtiness after five seconds, writes at most 500 rows, and forces a
synchronized checkpoint after joined network shutdown. Removed owners discard
their accumulator only after durable row finalization. Focused pure-v2 evidence
proves exact pre-completion download and upload accounting across the active to
complete handover; completed-seed evidence proves a seven-byte checkpoint and
restart upload continuity.

The combined-admission gate now classifies downloads and full completed seeds
in one runtime-independent transition. Downloads retain durable queue order;
seeds sort by descending exact rank with canonical identity ties; inactive
active torrents consume only the hard slot; downloads receive the fixed
500-torrent hard capacity first; and zero, five, Unlimited, shrink, and growth
limits are exact. The existing joined maintenance owner samples inactivity and
reconciles the pinned 30-second interval plus ordinary wakes. Seed demotion
joins peer, route, discovery, and read ownership before promotion without
rewriting durable run intent or replacing unaffected runtime generations.
Stale route tokens are detected and recovered rather than accepted as active.
An end-to-end test proves a successful one-byte upload crossing the exact ratio
goal, peer closure, demand-ranked replacement, live `0`/Unlimited/`1` setting
changes, durable intent and generation retention, and the same winner after
restart. The 500-torrent scale case admits 497 seeds beside three preferred
downloads under the fixed hard limit while retaining the established peer,
slot, bandwidth, and storage bounds.

Validation at this checkpoint:

- `cargo fmt --all`;
- `cargo test -p rstorrent-session` (322 passed, two opt-in cases ignored,
  plus all package binary tests); and
- `cargo clippy -p rstorrent-engine -p rstorrent-session --lib --bins -- -D
  warnings`.

## Decision And Desired Outcome

Add the missing seeding queue and durable accounting policy without inventing
a hard stop-on-ratio product contract.

RSTorrent adopts the pinned libtorrent behavior:

- all ordinary desired-running torrents remain automatically managed;
- downloading and seeding have separate active limits beneath one fixed hard
  active-torrent ceiling;
- the seed queue is ordered by a libtorrent-shaped seed rank rather than the
  durable download queue position;
- per-torrent payload totals and active/finished/seeding durations survive
  restart;
- share ratio, finished-time ratio, and absolute finished-time thresholds are
  session settings with the pinned defaults;
- a seed has unmet goals only while **all three** threshold comparisons remain
  below target, so reaching any one threshold makes the goal met;
- goal-unmet seeds rank ahead of goal-met seeds, but goal completion does not
  mutate durable run intent and does not permanently stop a torrent; and
- a goal-met torrent continues seeding whenever it still wins an active seed
  slot or capacity is otherwise available.

The product must not label these values **Stop at ratio** or promise that a
threshold closes a torrent. Settings and status copy use **seeding priority**,
**goal met**, and **active seeds**. Automatic queuing remains runtime state;
Pause remains the user's durable stop intent.

This tactical also makes the current automatic download manager and the new
seed manager one runtime-independent admission policy under the pinned fixed
hard ceiling. It does not add another engine, daemon, polling service, or
payload path.

## Stable Scenarios

1. **DSG-001: ordinary completion enters the seed queue.** A desired-running
   torrent completes verified content, releases its download slot, and either
   becomes an active seed or remains automatically queued under the configured
   seed limit. Queueing does not change `desired_running`.
2. **DSG-002: any goal threshold is sufficient.** Equal-to-target share
   ratio, finished/download time ratio, or absolute finished time independently
   clears the goal-unmet rank bit. Just-below every threshold retains it.
3. **DSG-003: goal met is not hard stop.** A lone goal-met seed stays active;
   when capacity is scarce, an otherwise eligible goal-unmet seed outranks it
   and the demoted torrent joins its peer/discovery owners without becoming
   user-paused.
4. **DSG-004: demand orders equal goal classes.** Known tracker counts are
   preferred over live-peer fallback. No-known-other-seed takes strict
   priority; otherwise more downloaders per seed rank higher.
5. **DSG-005: restart preserves policy.** Durable payload totals, timers, and
   cached tracker counts reopen without double counting. A reopened torrent
   obtains the same goal classification and rank from the same inputs.
6. **DSG-006: abrupt death is conservative.** A crash may lose only the
   explicitly bounded uncommitted accounting tail. It never invents uploaded
   bytes or elapsed time, so a torrent may seed slightly longer but cannot be
   declared goal-met from uncommitted work.
7. **DSG-007: live settings converge.** Active-seed and threshold changes use
   the established typed settings patch/revision path, trigger one admission
   reconciliation, and preserve unaffected peer/runtime generations when the
   winning set is unchanged.
8. **DSG-008: inactive exemption fills unused capacity.** The pinned default
   inactive-rate and startup-delay behavior allows a slow seed to stop
   consuming a seed-type slot while it still consumes the hard active ceiling.
   A later useful transfer returns it to counted status without exceeding the
   declared limits.
9. **DSG-009: Android lifetime composes.** Compose and the ChromeOS companion
   see the same configured settings and seed-queue truth. Tactical `200`'s
   default-off background policy remains authoritative; time does not accrue
   while the Android application owner is stopped.
10. **DSG-010: scale remains bounded.** Five hundred completed torrents can be
    ranked, reconciled, checkpointed, reopened, and shut down without one task,
    timer, database transaction, or open payload handle per queued torrent.

## Source-First Record

### Normative Protocol Boundary

Seeding goals and automatic queue priority are application policy, not a new
BitTorrent extension. BEP 3 remains relevant only for the meanings of tracker
`uploaded`, `downloaded`, `left`, complete/incomplete counts, and stopped/
completed announcement events. No peer-wire value, reserved bit, extension
message, tracker parameter, DHT value, or support claim changes.

### Pinned libtorrent 2.0.13

Planning inspected exact commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/settings_pack.hpp` documents `active_downloads`,
  `active_seeds`, `active_limit`, `seed_time_limit`, `share_ratio_limit`,
  `seed_time_ratio_limit`, `auto_manage_prefer_seeds`, and
  `dont_count_slow_torrents`;
- `src/settings_pack.cpp` sets the adopted defaults: five active seeds, a
  500-torrent hard active limit, 30-second automatic reconciliation, 86,400
  finished seconds, 200% share ratio, 700% finished/download time ratio,
  download preference, inactive exemption enabled, 2,048-byte/s inactive
  thresholds, and a 60-second startup/inactivity delay;
- `src/torrent.cpp::seed_rank` owns the exact bit-ranked predicate, full-seed
  versus selected-finished scale, no-other-seed priority, recent-started bit,
  tracker-count/live-peer fallback, and demand ratio;
- `src/torrent.cpp::{second_tick,active_time,finished_time,seeding_time,
  do_pause,write_resume_data}` owns payload accumulation, monotonic active
  durations, pause exclusion, and resume serialization;
- `src/session_impl.cpp::{recalculate_auto_managed_torrents,
  auto_manage_torrents}` sorts seeds descending by rank, applies download/
  seed/hard limits, preserves the inactive exemption, gracefully pauses
  losers, disables their announces, and resumes winners;
- `src/{read_resume_data,write_resume_data}.cpp` and
  `include/libtorrent/add_torrent_params.hpp` persist total uploaded/downloaded,
  active/finished/seeding seconds, and cached complete/incomplete counts; and
- `docs/manual.rst` describes the three queues, persistence dependency, demand
  ranking, anti-flap intent, action limits, and inactive-torrent behavior.

Relevant tests inspected:

- `simulation/test_auto_manage.cpp::{dont_count_slow_torrents,
  count_slow_torrents,seed_limit,pause_completed_torrents,
  checking_announce,force_started}`;
- `simulation/test_torrent_status.cpp::active_timer_no_seed`;
- `simulation/test_swarm.cpp::{active_seeds,active_seeds_negative}`; and
- `test/test_resume.cpp::{generate_resume_data,default_tests}` for durable
  totals and active/finished/seeding durations.

The pin has no direct boundary table for `torrent::seed_rank`. RSTorrent must
therefore add independently authored exact-value tests for every comparison
and rank flag rather than inferring coverage from the broader auto-management
simulations.

The implementation, rather than the manual's looser prose, settles an
important ambiguity: `seed_ratio_not_met` is set only while all three clauses
are true. Equal-to or above **any** threshold means goal met. The implementation
also compares the setting called `seed_time_limit` against `finished_time`,
not `seeding_time`, and derives download time as `active_time -
finished_time`. RSTorrent follows those facts exactly.

RSTorrent independently authors the Rust policy and tests. No libtorrent
source, fixture, resume encoding, or test vector is copied.

### JSTorrent Product History

Planning inspected the local JSTorrent checkout at exact commit
`25e4b701433fd815398ba89526546f5e4f072e3f`:

- `packages/engine/src/core/torrent-queue-manager.ts` has separate download and
  seed queues, an internal 500-torrent hard cap, graceful demotion, a five-
  minute seed anti-oscillation window, and least-recently-activated rotation;
- `packages/engine/test/core/torrent-queue-manager.test.ts` covers active-seed
  limits, immediate anti-flap, later rotation, completion state, user stop,
  and force-start bypass;
- `packages/engine/src/core/{torrent.ts,session-persistence.ts}` accumulates
  and restores torrent upload/download totals;
- `packages/engine/src/config/config-schema.ts` exposes `activeSeeds`; and
- the maintained Android settings and status surfaces expose **Max active
  seeds**, **Keep seeding**, and share ratio.

JSTorrent confirms that active-seed capacity and truthful queued/done
presentation matter to the product. Its round-robin seed policy, wall-clock
activation map, split persistence timing, force-active flag, JavaScript
engine, and service topology are not adopted because user direction selected
the pinned libtorrent rank semantics and RSTorrent already owns a different
durable intent/runtime boundary.

### Current RSTorrent Pressure Points

Implementation must revalidate the then-current owners before editing. The
planning inspection found:

- `crates/rstorrent-session/src/auto_manager.rs` admits only incomplete
  downloads in durable queue order and has no seed rank;
- `application.rs::reconcile_admission` owns the one serialized start/retain/
  join loop and currently excludes `TorrentState::Complete`;
- `incoming_seeding.rs::eligibility_reason` and
  `TorrentRuntimeHandle::{reconcile_seed,unregister_seed}` own completed-seed
  registration and exact joined removal;
- `torrent_runtime.rs::TorrentPeerViewSink` already derives generation-fenced
  per-connection upload/download deltas for volatile tracker counters, but no
  durable per-torrent sink exists;
- `settings/contract.rs::{ClientSettings,ClientSettingsPatch,
  ClientSettingsRuntimeView}` owns typed durable configured/effective settings;
- schema 22 has no lifetime payload totals or active/finished/seeding timers;
  and
- current complete torrents may all retain incoming registration, discovery,
  peer state, and storage-read eligibility, so seed admission must govern the
  whole completed runtime rather than only hide a UI row.

The concrete refactor is to generalize the runtime-independent automatic
manager to classify checking, downloading, and full-seed candidates and emit
one coherent set of start/retain/demote/idle decisions. SQLite, Tokio,
channels, sockets, platform handles, and application views remain outside the
pure rank and admission values.

## Exact Policy Contract

### Configured settings and fixed defaults

Add these client settings through the existing closed typed patch:

| Setting | Type and bound | Default | Exact meaning |
| --- | --- | --- | --- |
| Active seeds | `Unlimited` or `Limited { torrents: 0..=500 }` | `Limited { torrents: 5 }` | Counted auto-managed seed-type slots. Zero starts no counted seed; Unlimited removes only the type limit, not the fixed hard limit. |
| Share-ratio limit | integer percent `0..=2_147_483_647` | `200` | `total_uploaded * 100 / max(total_downloaded, total_size)` threshold. |
| Finished/download time-ratio limit | integer percent `0..=2_147_483_647` | `700` | `finished_seconds * 100 / download_seconds` threshold when download time exceeds one second. |
| Finished-time limit | seconds `0..=2_147_483_647` | `86_400` | Absolute active finished-state duration threshold. |

Zero is not an alias for disabled: it makes the corresponding threshold met
immediately, matching libtorrent. There is no per-torrent goal override,
`UntilStopped` enum, or stop-on-goal flag in this slice. Users who want every
eligible seed active choose Unlimited active seeds; resource budgets still
apply.

The following pinned defaults remain internal constants in this tactical:

- hard active-torrent limit: 500;
- automatic reconciliation interval: 30 seconds, plus the pinned ordinary
  state, settings, and inactivity-triggered recalculations;
- downloads preferred when the hard limit cannot satisfy both type limits;
- inactive-torrent exemption: enabled;
- inactive upload/download threshold: strictly below 2,048 payload bytes/s;
- inactive transition delay: 60 continuous seconds; and
- recently-started priority: cumulative active time strictly below 1,800
  seconds.

Do not expose the hard limit, auto-management interval, inactive thresholds,
preference, or action-specific DHT/tracker limits in this slice.

### Exact goal predicate

The runtime-independent calculation uses nonnegative durable values and wide
intermediates:

```text
downloaded_base = max(total_downloaded, full_metainfo_total_size)
download_seconds = active_seconds - finished_seconds

goal_unmet =
  finished_seconds < finished_time_limit_seconds
  && download_seconds > 1
  && finished_seconds * 100
       < download_seconds * finished_download_ratio_limit_percent
  && downloaded_base > 0
  && total_uploaded * 100
       < downloaded_base * share_ratio_limit_percent
```

The multiplication form is algebraically equivalent to libtorrent's integer
division for nonnegative inputs while `u128` intermediates prevent overflow.
Threshold equality means met. A zero-size torrent, or a torrent with no more
than one computed download second, is goal-met exactly because libtorrent's
conjunction is false in those cases.

`full_metainfo_total_size` is the exact libtorrent-shaped torrent content
size, including padding geometry represented by the metainfo. Lifetime totals
include ordinary payload transfer before and after completion. Metadata,
protocol framing, tracker/DHT bytes, retransmission overhead, HTTP media,
remote-file streaming, storage reads, and hash work do not count.

### Exact seed rank

Use the pinned bit layout and descending comparison:

```text
0x40000000  goal_unmet
0x20000000  no_known_other_seed
0x10000000  cumulative_active_seconds < 1800 and currently unpaused
0x0fffffff  bounded demand score
```

The pure function returns zero for a torrent that is not selected-finished.
The demand score follows `torrent::seed_rank`:

- use the maximum known tracker complete and incomplete values across current
  tracker records;
- when either value is unknown, derive that side from the current bounded peer
  registry;
- subtract this torrent from the known seed count only while it is an active
  full seed;
- if the resulting seed count is zero, set `no_known_other_seed` and use the
  downloader count in the low bits; otherwise use
  `(1 + downloaders) * scale / seeds`;
- `scale` is 1,000 for a full seed and 500 for selected-finished partial
  content; and
- mask the bounded result into the low 28 bits.

RSTorrent uses canonical `TorrentId` as a deterministic final tie-break when
numeric ranks are equal. The pin makes no ordering promise for comparator-equal
seeds; this explicit implementation refinement makes tests and restart
ordering stable without changing any ranked libtorrent outcome.

The pure rank shape retains the pinned full-seed/selected-finished distinction,
but this tactical admits only torrents already eligible for RSTorrent's
existing full completed-seed runtime. Extending post-selection-completion
partial upload ownership is a separate capability and is not smuggled into
this policy slice.

### Admission and lifecycle

- Checking retains its existing one-owner admission path.
- Incomplete desired-running torrents retain durable download queue order and
  the configured/effective active-download limit.
- Full completed desired-running torrents enter seed-rank order and the active
  seed limit. Download queue position does not order seeds.
- Fixed hard-limit accounting considers counted downloads, counted seeds, and
  inactive-exempt active torrents exactly once. Downloads receive capacity
  before seeds when the hard ceiling binds.
- A torrent continuously below the pinned applicable rate for 60 seconds is
  inactive. It stops consuming its type slot but still consumes the hard
  limit. A useful rate transition is likewise delayed and then re-enters
  counted status. Pending transitions are generation-fenced and cancellable.
- A winner owns its ordinary peer runtime, completed-seed registration,
  discovery advertisement, and bounded read eligibility. A loser gracefully
  closes or fences peer writes, unregisters every v1/v2 route, publishes
  inactive peer state, emits truthful stopped discovery behavior, and releases
  payload handles before another seed is admitted.
- Demotion and promotion never rewrite `desired_running`, queue position,
  selection, have state, storage root, priority, or error state.
- Pause, archive, removal, checking, storage loss, repair, and shutdown remain
  stronger ineligibility transitions than rank.
- Goal crossing is observed by the next ordinary 30-second or already-triggered
  reconciliation. It has no dedicated stop or immediate wake path, and it does
  not by itself close a lone seed.

There is one application-generation seed-admission owner. Do not create a task
per queued torrent. One joined timer/wake loop may drive the 30-second and
60-second policy edges; ordinary torrent runtimes remain the owners of active
peer/discovery/storage children.

## Durable Accounting Contract

Advance the current disposable catalog to schema 23. If another authorized
tactical advances the schema before implementation, use the next fresh schema
and update this document before code lands.

Schema 23 stores, per torrent:

- lifetime peer payload uploaded and downloaded;
- cumulative active, finished, and full-seeding whole seconds;
- the latest known tracker complete and incomplete counts, with explicit
  unknown values; and
- no derived ratio, rank, goal bit, current-rate sample, wall-clock deadline,
  queued/active result, task identifier, peer identifier, or open-handle fact.

The client-settings singleton stores the four configured settings above. Every
recognized schema `1..=22` follows the existing bounded application-private
reset contract. Reset never traverses or deletes selected roots or external
payload, and old totals cannot establish verified content authority. No
`0.1.x` migration reader or compatibility alias is added.

One session accounting owner consumes absolute, generation-fenced peer
payload observations and computes monotonic deltas. Count bytes only after the
peer I/O owner classifies a received or successfully written peer payload
span; never count a requested span, queued read, cancelled response, protocol
header, or failed write. New connection generations start from zero without
reusing an old absolute counter. Saturation is explicit and diagnostic; wrap
is forbidden.

Timers use the runtime's monotonic clock:

- `active_seconds` accrues whenever the torrent is admitted and not paused,
  including active checking, exactly like libtorrent's unpaused active timer;
- `finished_seconds` accrues during admitted selected-finished state;
- `seeding_seconds` accrues during admitted full-seed state;
- queued, stopped-service, closed Android generation, archived,
  storage-unavailable, and application-offline time does not accrue;
- paused checking or repair does not accrue, while an admitted running checker
  accrues only `active_seconds`; and
- no persisted wall clock is subtracted after restart.

Dirty accounting is coalesced by one joined checkpoint owner. Flush at most
once per five seconds under timer-only dirtiness and promptly when aggregate
uncommitted payload reaches 1 MiB; commit at most 500 dirty torrent rows in one
transaction and force a final synchronized flush on clean shutdown. The
implementation records the actual maximum uncommitted byte/time tail. Abrupt
death may undercount only that bounded tail; it cannot overcount or skip a
goal through speculative data.

## Application And Presentation Contract

Extend the generated application model with compact, encoding-neutral facts:

- decimal-string lifetime uploaded and downloaded payload;
- active, finished, and seeding seconds;
- a nullable display share ratio derived from the exact durable values;
- goal `unmet` or `met`, plus which thresholds are met;
- seed admission `active`, `queued`, `inactive_exempt`, or `ineligible`; and
- configured/effective active-seed limit and current counted/exempt seed
  counts in the client-settings runtime view.

Do not expose the raw numeric seed-rank bitfield as the product contract.
Structured diagnostics may report the bounded rank components, input source,
promotion/demotion reason, accounting flush, and high-water values.

The shared React Settings surface adds the four global values under
**Connection & seeding** or the established queue category. Torrent Summary/
General shows lifetime totals, share ratio when defined, goal status, elapsed
times, and truthful active/queued state. Copy states that goals set priority
and that a goal-met torrent may continue seeding.

Compose receives the same generated types, editable global settings, torrent
status, and draft-revision convergence. Android presentation may remain more
compact than Workbench, but it cannot label a queued seed as active or a goal
as a hard stop. Generated Swift must compile and round-trip the new complete
settings shape; an iOS settings editor is not required unless the existing
SwiftUI settings architecture already exposes the same category without a
new product decision.

Tactical `200` remains the Android process-lifetime authority. **Keep seeding
in background** permits the service owner to run the seed manager; it does not
force every candidate active, bypass a zero seed limit, or accrue time while
the process is stopped. An admitted or promotable desired-running seed is the
background reason; goal status alone is not.

## Owner, Task, Cancellation, And Dependency Map

```text
peer I/O owners (TCP/uTP, initiated/accepted, plaintext/MSE)
  -> absolute generation-fenced payload observations
  -> one session durable-accounting accumulator/checkpoint owner
  -> schema-23 monotonic totals and durations
                         |
tracker + peer snapshots | settings + durable torrent facts + monotonic time
                         v
runtime-independent seed rank and combined admission transition
                         |
             one application admission owner
               | winner             | loser
               v                    v
       torrent peer/runtime    joined unregister/stop
       discovery + reads       no durable-intent rewrite
                         |
                  generated app views
             React / Compose / Swift boundary
```

Dependency direction remains platform/presentation -> application/session ->
engine -> protocol/domain values. The pure rank and admission modules contain
no Tokio, SQLite, socket, filesystem, channel, task-handle, wall-clock, or
platform type.

Every new background owner has one application-generation cancellation token,
a bounded queue or coalesced wake, explicit terminal result, and joined
shutdown. Queued torrents own no per-torrent timer, task, socket, payload
handle, or pending storage request.

## Initial Resource And Security Bounds

- At most 500 auto-managed active torrent generations under the fixed hard
  limit; checking retains its existing separate bound.
- Active-seed settings cannot exceed 500 except the explicit Unlimited value,
  which remains hard-capped at 500 combined active torrents.
- Rank input is one fixed-size record per seed candidate; select only the
  fixed-hard-limit prefix with `O(n log min(n, 500))` work or an equivalent
  bounded selection, and retain no historical rank samples.
- One pending inactivity transition per active torrent is represented as
  bounded state beneath the single timer owner, not 500 detached sleeps.
- One accounting batch contains at most 500 unique torrent rows and at most
  the fixed scalar fields above.
- Aggregate uncommitted payload target is 1 MiB plus one bounded observation
  batch; timer-only dirty age target is five seconds. Actual high waters must
  be measured and recorded before completion.
- Existing session peer, request, queued-payload, upload-slot, bandwidth,
  storage-read, 40-handle, writer, DHT/tracker, and diagnostic bounds remain
  authoritative and do not multiply by configured seed slots.
- Peer-controlled counts and totals use checked/saturating bounded conversion;
  hostile tracker counts cannot allocate work or overflow rank arithmetic.
- No payload bytes, paths, credentials, peer strings, or unbounded tracker
  values enter diagnostics or settings.

## Shape-Changing Edge Cases

- Exact just-below/equal/above comparisons for every threshold, including
  zero thresholds, zero-size torrents, `download_seconds` zero/one/two, and
  maximum configured values.
- `active_seconds < finished_seconds` is malformed durable state and repairs
  or resets torrent-local accounting; it never underflows into a high rank.
- Imported/adopted complete payload with zero lifetime download uses full
  metainfo size as the ratio denominator, exactly like libtorrent.
- Pre-completion uploads and downloads contribute to lifetime totals; toggling
  selection, checking, or full-seed status never resets them.
- Duplicate/late peer observations, reconnect, crossed connections, peer-ID
  replacement, uTP/TCP fallback, MSE retry, cancellation during write, and a
  writer failure cannot double count.
- Share-ratio crossing during an in-flight upload changes rank only after the
  successful bytes are observed and an ordinary reconciliation runs; already
  admitted response ownership remains race-safe during graceful demotion.
- Goal crossing with free capacity retains the same active generation. Goal
  crossing under saturation promotes the highest-ranked loser only after the
  demoted owner's registrations and reads are fenced.
- A setting reduction, increase, zero, or Unlimited transition applies live.
  Failed persistence changes nothing; replay/no-op resubmits authoritative
  intent through the established revision path.
- Known tracker seed count includes this client only when active and full;
  self subtraction clamps at zero. Partial/unknown tracker responses fall back
  only for the missing side.
- Equal numeric ranks use canonical identity and remain stable across input
  ordering and restart.
- Root loss, recheck, hash failure, archive, removal, Pause, service timeout,
  network-policy closure, and shutdown override inactive/goal state and drain
  every owner exactly.
- Android activity loss with background seeding disabled closes the application
  without changing durable run intent or adding offline time. Reopen resumes
  from committed totals and recomputes rank.
- A crash before/after accounting commit and before/during/after seed
  promotion cannot leave both queued and active authority or advertise a
  stopped torrent.

## Implementation Order And Intermediate Gates

1. **Contract and pure policy.** Add settings/rank/admission value shapes and
   exhaustive task-free tests before runtime or persistence changes. Gate on
   exact pinned defaults, predicate boundaries, rank flags, type/hard limits,
   inactivity transitions, and deterministic ordering.
2. **Fresh durable epoch.** Add schema 23, recognized pre-23 reset, settings,
   per-torrent counters/timers/counts, round trips, hostile-state validation,
   and fixed-size batch transactions. No runtime behavior changes in this
   gate.
3. **Accounting owner.** Feed exact initiated/accepted TCP/uTP plaintext/MSE
   payload observations into one accumulator, measure dirty high waters, and
   prove clean/crash/reconnect behavior while the existing all-seeds runtime
   policy remains unchanged.
4. **Combined admission owner.** Generalize the current download auto manager,
   place completed full seeds under rank and active/hard limits, compose
   inactivity, and join registration/discovery/read ownership on demotion.
   Keep generated/UI behavior behind truthful views.
5. **Application and first-party surfaces.** Extend complete/sparse views,
   typed settings patches, runtime counts, React and Compose editing/status,
   generated TypeScript/Kotlin/Swift boundaries, and Android lifetime facts.
6. **Controlled interoperability and platform gate.** Run multi-seed
   RSTorrent/libtorrent threshold/rotation/restart cases, Android dual-ABI and
   connected AVD lifecycle, iOS simulator/archive compilation, web/browser,
   and repository baselines. Record exact high waters and reconcile every
   owning topic before completion.

Each gate must leave one diagnosable state. Do not land UI controls before the
enforcing owner or activate seed admission before counters can reopen.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure policy | Exact defaults; all threshold boundaries; any-one goal completion; zero/maximum inputs; wide-arithmetic equivalence; goal/no-seed/recent/demand bits; tracker/live fallback; self subtraction; seed/finished scale; inactive delay; 0/1/5/Unlimited seed slots; 500 hard cap; downloads-first ordering; stable ties |
| Persistence | Fresh/reopen schema 23; representative schemas 21/22 reset with payload sentinel unchanged; configured setting round trips; monotonic totals/timers/counts; malformed/future/busy/symlink failure; one-shot reset report; 500-row batch; no derived runtime state stored |
| Accounting runtime | Initiated and accepted TCP/uTP plus forced-MSE upload/download; pre/post-completion totals; reconnect/crossed/late observation; cancel/write failure; overflow; threshold observation at ordinary reconciliation; bounded coalescing; clean final flush; forced death at pre/post commit with conservative reopen |
| Admission runtime | Download-to-seed transition; active limit reduction/increase/zero/Unlimited; goal crossing with/without contention; no-other-seed and demand promotion; inactive exemption/recount; exact unregister/advertisement stop/read fence; Pause/recheck/root loss/archive/removal/network close/shutdown; terminal zero owners |
| Controlled interop | At least six complete deterministic torrents, active-seed limit one and two, pinned libtorrent leechers generating known exact payload, threshold-induced reorder, exact hashes, tracker events/counts, restart, and no residue; no public swarm required |
| Application/web | Generated schema drift, typed patch receipt/replay/stale/rollback, configured/effective runtime counts, sparse reducer continuity, React settings/status at wide and phone sizes, accessibility, truthful goal copy, browser/Tauri/headless shared behavior |
| Android/Apple | Both Android Rust ABIs, UniFFI/Kotlin generation, JVM unit tests, APK; connected API 35 AVD visible/background-disabled/background-enabled queue and settings matrix with exact cleanup; generated Swift validation, maintained iOS simulator tests, and unsigned archive compile |
| Scale/resources | 500 complete torrents, fixed rank/accounting memory, no per-queued-torrent task/timer/handle, batch and uncommitted-tail high waters, shared peer/read/handle/bandwidth ceilings, joined shutdown |
| Repository | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, `npm run generate --prefix clients/web`, `npm run typecheck --prefix clients/web`, `npm run test --prefix clients/web`, applicable Playwright/package gates, `git diff --check` |

No public swarm, physical device, firewall, gateway, relay, signed package,
deployment, or external mutation is required. Headless and automated AVD/
simulator evidence is proportional because the slice changes shared engine,
persistence, generated contracts, and Android lifetime semantics but adds no
new OS integration primitive.

## Non-Goals And Intentional Deferrals

- Hard stop-on-ratio/time, automatic durable Pause, removal/close on goal, or
  copy implying those behaviors.
- Per-torrent goal overrides, per-torrent active-seed exemption, force start,
  manual seed priority, seed schedules, idle-time goals, low-battery shutdown,
  or bandwidth schedules.
- Extending selected-finished partial torrents into a new post-download upload
  runtime; this tactical ranks that source shape but admits only existing full
  completed-seed candidates.
- Super-seeding, share mode, BEP 21 `upload_only`, predictive announces,
  choking changes, peer classes, web seeds, hole punching, LSD, tracker
  mutation, or a new protocol support claim.
- User-configurable hard active limit, auto-manage interval, inactivity rates/
  delay, prefer-seeds policy, DHT/tracker action limits, or automatic scrape
  requests.
- Global historical traffic analytics, peer-history persistence, product
  telemetry, cloud sync, backup/restore, import from JSTorrent/libtorrent, or
  retention of disposable schema-22 state.
- Changing upload-slot, peer, file-handle, storage, memory, bandwidth,
  background-notification, or Android `dataSync` ceilings.
- Native Android playback, stable sharing, direct remote-file qualification,
  desktop signing/updater work, first-supported-version declaration, or
  production JSTorrent migration.

## Escalation And Autonomous Implementation Authority

Once implementation is explicitly authorized, ordinary authority includes
the schema-23 disposable reset, generated contract changes, focused extraction
of pure rank/combined-admission/accounting modules, one joined timer/checkpoint
owner, internal tracker/peer observation plumbing, deterministic clocks,
scripted peers, loopback pinned-libtorrent processes, browser automation,
connected AVD and simulator/archive builds, and same-boundary bug fixes needed
to satisfy the declared scenarios and limits.

Stop for direction before:

- changing the exact pinned defaults, predicate, rank flags/order, inactive
  behavior, or goal-met-not-stop decision;
- adding per-torrent settings, force start, partial-finished upload ownership,
  a protocol behavior, external dependency, or a second persistence/runtime
  authority;
- weakening payload verification, storage/read fences, durable intent,
  application revision semantics, or the fixed shared resource ceilings;
- preserving schema-22 application state instead of the authorized reset or
  deleting/scanning external payload during reset;
- using a physical device, public swarm, firewall/gateway change, signed
  package, deployment, account, relay, or other external mutation; or
- making a stable public API, compatibility, release, or JSTorrent migration
  promise.

Ordinary refactoring, conservative tightening inside the stated bounds,
additional adversarial tests implied by the invariants, and internal module
names do not require escalation.

## Stopping Condition And Next Boundary

Tactical `201` is complete only when the exact pinned seed-rank predicate and
defaults are implemented through durable schema-23 accounting, one combined
automatic admission owner, truthful React/Compose/generated-client behavior,
controlled pinned-libtorrent threshold/reorder/restart evidence, Android and
Apple build/runtime gates, measured 500-torrent/resource high waters, and
joined zero-owner cleanup. The tactical and every owning topic must record the
actual implementation, commands, evidence, and deliberate gaps.

After completion, active-seed capacity and global goal priority are supported.
Hard stop policy, per-torrent overrides, partial-finished upload continuity,
and broader seeding breadth remain separate tacticals chosen from product
evidence; none is implied by this stopping condition.
