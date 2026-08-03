# Tactical 057: Hardware Performance Baselines

Status: Complete.

Topics: `performance-and-live-evidence`, `application-view-api`,
`capability-readiness`, `oracle-driven-engine-campaign`

## Motivation And Outcome

Tactical `054` established a reproducible 1 GiB/10 GiB single-file loopback
matrix and found several geometry-dependent hot-path defects. Its executable
gate still has only one global throughput floor, however. It cannot protect a
fast geometry from a large regression while a slower row remains above that
floor, it does not reject an incompatible machine, and it does not exercise
the application while a UI consumes live views.

Make performance policy explicit and reviewable through named hardware
profiles. A profile validates the environment, selects workloads and
repetitions, records observed calibration separately from required floors, and
applies row-specific absolute and libtorrent-relative gates. Add a controlled
application observation profile that measures the idle application, each real
UI view combination, every view simultaneously, and a deliberately slow
all-view consumer. The result must identify which view has the largest paired
throughput and resource cost rather than reporting only a combined UI penalty.

The stopping condition is an executable MacBook profile, a deliberately
generous GitHub-hosted runner profile, a retained per-view/adversarial
application harness, deterministic policy tests, and a CI workflow that
produces machine-readable evidence and fails only on its declared floors.

## Stable Scenarios

- Selecting a profile is explicit. A hostname may be recorded, but never
  silently selects performance policy.
- A selected profile checks operating system, architecture, CPU/memory and CI
  identity requirements before allocating a fixture. A mismatch is
  `not_applicable`, not a performance failure or a silent skip.
- Every throughput row has an exact size, piece size and storage-execution
  point. Missing, duplicated and unexpected rows are configuration failures.
- Calibration observations and required gates are different fields. A slower
  run never rewrites an accepted floor automatically.
- Exact payload bytes, whole-file SHA-1, publication and cleanup remain hard
  gates independently from speed policy.
- Application modes use the production view types and delivery intervals. The
  common wide Workbench shape is Library plus selected Summary; one additional
  detail view is varied at a time.
- The adversarial mode opens Library, Summary, Peers, Files, Trackers, Pieces,
  Disk and trace Diagnostics together. The slow-adversarial mode consumes the
  same set slowly enough to exercise coalescing, queue high water and explicit
  reset recovery.
- Every observed application mode is compared with an idle application cohort
  from the same fixture and run sequence. Hardware-normalized idle ratios are
  first-class gates.
- Every background seed, application and view-consumer owner is bounded,
  canceled, joined and cleaned. Ordinary reports retain no payload, profile,
  peer endpoint or machine path.

## Profile And Evidence Contract

Profiles are committed as one TOML file per named performance environment.
TOML uses Python 3.12's standard `tomllib`, matches the repository's existing
configuration posture, and adds no parser dependency.

```text
tests/perf/baselines/
  kmacbook-m4pro.toml
  github-ubuntu-24.04-x64.toml
```

The command selects `--baseline-profile NAME` and `--profile-tier smoke|full`.
CI supplies the profile explicitly. Requirements are a fail-closed guard over
the selected logical environment; ephemeral GitHub hostnames are never profile
keys. Reports additionally fingerprint the actual hostname, CPU, logical CPU
count, memory, temporary storage, runner labels/image metadata, toolchain,
commit, dirty state, binary and cache policy.

Each case separates:

```text
observed = calibration median and provenance
required = accepted minimum throughput, reference ratio, idle ratio,
           and bounded reset/resource policy
```

Calibration emits candidate JSON. Changing committed observed values or
required floors is an ordinary reviewed source change; no benchmark command
updates policy in place.

The MacBook full tier retains the 1 GiB/10 GiB matrix at 256 KiB, 1 MiB,
4 MiB and 16 MiB with three rotating repetitions. Its first required values
use material headroom below Tactical `054`'s repeated medians while protecting
each geometry independently. The GitHub profile begins uncalibrated, uses one
1 GiB repetition and broad absolute/reference catastrophe floors. Hosted
runner hardware and contention may drift, so tighter gates require a retained
multi-run calibration or a dedicated runner.

## Application Observation Matrix

The controlled application workload uses one deterministic loopback fixture,
one pinned libtorrent seeder, path-backed SQLite application storage and the
selected `4/4` storage point. Modes are:

| Mode | Requested production views | Purpose |
| --- | --- | --- |
| `idle` | none | Application, SQLite and view-source control |
| `library` | Library | Ordinary Library/Transfers collection |
| `general` | Library + Summary | Wide Workbench common control |
| `peers` | common + Peers | Active connection cost |
| `files` | common + Files | File catalog/progress cost |
| `trackers` | common + Trackers | Tracker lifecycle cost |
| `pieces` | common + Pieces | Piece bitmap/activity cost |
| `disk` | common + Disk | Session storage pipeline cost |
| `logs-normal` | common + normal Diagnostics | Ordinary Logs capture |
| `logs-detailed` | common + detailed Diagnostics | Higher diagnostic interest |
| `logs-trace` | common + trace Diagnostics | Per-block diagnostic pressure |
| `all` | every named view, trace Diagnostics | Maximum active observer |
| `slow-all` | same, delayed consumption | Queue/coalescing/reset adversary |

The runner records transfer time and throughput, process CPU and sampled peak
RSS when the host supports them, delivered batches/updates/JSON bytes,
per-view update counts, queue high water and explicit resets. It emits medians,
the ratio to the idle median, and a worst-view ranking. A missing host resource
metric is `null`, never zero.

This tactical measures the in-process Rust application and view delivery. It
does not claim browser decode/reducer/render cost; the established synthetic
browser scale fixtures remain that owner. A later browser-attached throughput
row may join the same profile only after its process and paint boundaries can
be measured without conflating them with this producer comparison.

## Normative And Reference Dossier

No reference source, fixture, test data or benchmark implementation is copied.

- Pinned libtorrent `2.0.13` at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d` is the primary performance
  oracle. `tools/run_benchmark.py` creates a 10,000-piece/15-file controlled
  workload, varies peers and cache modes, samples process resources and emits
  session-counter artifacts. Legacy `examples/run_benchmarks.py` uses a
  100 GiB source, 50--1,000 peers, 100-second steady intervals, cache purging
  and disk-thread sweeps rather than completion-only timing.
- `tools/benchmark_checking.py` sweeps one through 64 hashing threads.
  `tools/checking_benchmark.cpp` exercises roughly 7 GiB v1, v2 and hybrid
  checking through mmap and positional backends.
  `tools/disk_io_stress_test.cpp` shuffles bounded exact writes and reads while
  varying queue size, threads, file-pool size, allocation, random readback,
  file release and piece clearing.
- `docs/tuning.rst::{profiling,hashing,scalability}` recommends periodic
  `post_session_stats`, asynchronous `post_torrent_updates`, changed-torrent
  subscriptions and batched status calls because synchronous return-value API
  calls block on the network thread. This supports measuring active observers
  separately and keeping one authoritative application projection path.
- `include/libtorrent/performance_counters.hpp` includes block/write/read/hash,
  read-back, file-pool, disk-service and payload counters. The exact
  `test/test_session.cpp::session_stats` and
  `test/test_alert_types.cpp::session_stats_alert` cases prove complete metric
  indexing and typed counter delivery; `test/setup_transfer.cpp::get_counters`
  demonstrates the asynchronous sampling path used by transfer tests.
- RSTorrent's own production view oracle is
  `clients/web/src/inspection/live/LiveApplication.ts::viewSpecs`: Library,
  Summary, Peers, Pieces, Disk and Diagnostics request 100 ms delivery, while
  Files and Trackers request 250 ms. `inspection/controller.ts::desiredViewsFor`
  proves only the selected detail is ordinarily retained alongside the common
  Workbench views.
- The JSTorrent sibling at observed HEAD
  `9895410beeed6aff554053769bd006a3fbd373ef` uses direct heap access plus event
  subscriptions in `packages/client/src/hooks/useEngineState.ts` and records a
  per-torrent subscription migration in `extension/CHANGELOG.md`. Its topology
  differs, so those are product-history cautions against broad polling rather
  than a profile or API template.

Intentional differences from libtorrent are RSTorrent's row-specific TOML
policy, same-fixture paired idle ratios, exact production view combinations and
recoverable slow-consumer adversary. This slice does not copy libtorrent's
counter registry, benchmark build system, cache-purge privilege or long-running
multi-peer seeding topology.

## Owner, Task And Dependency Shape

| Owner | Mutable state | Work and termination |
| --- | --- | --- |
| Profile loader | Parsed requirements, cases and gates | Task-free Python; rejects bounds and duplicate cases before fixture creation |
| Python observation runner | Fixture, libtorrent seed, case order, child/resource samples and report | Sequential bounded cases; always stops the seed, kills failed children and removes its exact temporary root |
| Application profile process | One `ApplicationService`, SQLite profile and path storage | One bounded diagnostic process; joins engine, checkpoint, DHT and view-reaper work during shutdown |
| View consumer task | Cursor and delivered-update counters for one view set | One task per observed process; cancellation races the long poll, then the task and view set are joined/closed |
| GitHub workflow | Selected profile, toolchain/cache setup and JSON artifacts | One bounded job with a workflow timeout; no public swarm or visible product surface |

The profile parser and Python orchestration remain in `tests/interop` and
`tests/perf`; they do not enter Rust runtime crates. The Rust diagnostic binary
depends on the existing public application/view contracts and does not expose a
new product command, remote transport or persisted schema.

## Bounds And Failure Policy

- Profiles contain at most 64 throughput cases and 32 application modes.
- Payload is at most the retained comparator's 10 GiB ceiling; piece sizes and
  storage points retain its existing validated bounds.
- One application process owns at most one view set and eight requested views.
  Queue limits remain the production server limits.
- Repetitions are at most five; CI begins at one and local full calibration at
  three.
- Process, case, job and workflow timeouts are explicit. Disk preflight occurs
  before source creation.
- Integrity, publication, cleanup, profile applicability and malformed-policy
  failures use distinct report/status text. A noisy speed miss cannot mask an
  integrity failure.
- The CI profile is deliberately generous and marked uncalibrated. A first
  retained CI cohort may tighten it, but no unknown hosted-runner number is
  presented as measured evidence.

## Implementation And Validation Sequence

1. Add the standard-library TOML loader, environment fingerprint and exact
   requirement/case validation with deterministic tests.
2. Teach `local_throughput_compare.py` to select profile workloads and apply
   row-specific floors while preserving all direct command-line options and
   schema-compatible raw integrity evidence.
3. Add the application profile binary and Python runner. Prove task cleanup,
   exact content, cursor continuity, per-view counts, all-view completion and
   slow-consumer reset recovery.
4. Commit MacBook and GitHub-hosted profiles. Add a fast CI tier and a
   scheduled/manual full observation tier, both headless and artifact-safe.
5. Run focused Python and Rust tests, the retained comparator smoke, and the
   application mode matrix on the local machine. Record observed calibration
   only for cohorts actually executed.
6. Update the living performance/API/capability records and close this
   tactical when the profiled commands and CI workflow are reproducible.

## Non-Goals

- Optimizing a view, changing the semantic application API or selecting a
  binary wire codec before the new matrix identifies a measured offender.
- Treating GitHub-hosted hardware as stable enough for tight production-speed
  claims before repeated evidence exists.
- Privileged cold-cache control, filesystem-wide cache purging or destructive
  disk preparation.
- Public-swarm performance gates, visible Tauri/browser automation, Android or
  ChromeOS hardware baselines.
- Multi-torrent fairness, upload/seeding throughput, storage read-through or
  another storage-execution architecture change.

## Escalation Boundary

Ordinary parser design, diagnostic-only metrics, workflow syntax, generous
uncalibrated CI floors and measurement-driven corrections inside this matrix
do not require maintainer feedback.

Stop if evidence requires a new dependency, privileged/destructive cache
control, paid or self-hosted runner selection, a product API/schema change,
visible application or physical-device operation, or relaxation of integrity,
bounded queue, checkpoint or cleanup invariants.

## Completed Implementation And Evidence

The implementation adds:

- `tests/interop/performance_profiles.py`, a dependency-free, bounded TOML
  loader with explicit environment matching, dotted profile IDs, workload
  selection and observed/required separation;
- hardware profiles for the calibrated local Apple M4 Pro and uncalibrated
  GitHub Ubuntu 24.04 x64 logical environments;
- row-specific profile mode in `local_throughput_compare.py` while preserving
  its direct workload and gate options;
- `rstorrent-application-throughput-profile`, an in-process application
  diagnostic using the production view specifications and joined consumer;
- `application_view_throughput.py`, which measures one exact mode per process,
  CPU/RSS where supported, update/reset/queue evidence, paired idle ratios,
  exact publication and cleanup; and
- `.github/workflows/performance-baseline.yml`, with affected-PR smoke,
  scheduled full, manual tier selection and JSON artifact retention.

The local engine smoke ran 1 GiB/256 KiB at `4/4`. RSTorrent completed at
615.6 MiB/s and libtorrent 2.0.13 at 485.1 MiB/s, a 1.269 ratio. Both clients
published the exact 1,073,741,824 bytes and SHA-1, reported zero failed and
redundant payload and cleaned the case root. The calibrated row gates passed.

The application full tier completed 39 exact 1 GiB transfers in 449.327
seconds of total harness time. Idle median throughput was 177.9 MiB/s. Library
alone retained 93.5% with zero resets; general retained 90.0% but incurred up
to 898 resets. Trace Diagnostics was the worst individual specialization at
98.4 MiB/s, 55.3% of idle and 1.081 GB median serialized delivery. Pieces was
next at 123.6 MiB/s. Every view together measured 74.0 MiB/s, 41.6% of idle,
1.742 GB serialized delivery and up to 1,737 resets. The one-second all-view
consumer measured 122.3 MiB/s with nine resets and a 16,777,725-byte queue
high water. All 39 cases verified 4,096 pieces, exact whole payload and clean
removal.

This establishes both the regression guard and the next measured target: the
common Summary view already forces reset snapshots, trace Diagnostics is the
largest individual throughput/serialization cost, and all views compound the
problem. Optimizing those paths remains a subsequent tactical; this slice does
not change semantic API or delivery behavior.

Validation completed:

```text
uv run --locked python -m unittest test_performance_profiles.py
  5 passed
uv run --project tests/interop --locked python -m py_compile ...
  passed
cargo fmt --all -- --check
  passed
cargo clippy -p rstorrent-session \
  --bin rstorrent-application-throughput-profile -- -D warnings
  passed
cargo test -p rstorrent-session \
  --bin rstorrent-application-throughput-profile
  2 passed
local_throughput_compare.py --baseline-profile kmacbook-m4pro \
  --profile-tier smoke
  passed
application_view_throughput.py --baseline-profile kmacbook-m4pro \
  --profile-tier full
  passed, 39/39 exact transfers
Ruby YAML parse of .github/workflows/performance-baseline.yml
  passed
```

The GitHub-hosted profile and workflow are deliberately not labeled measured:
no remote workflow was run from this local implementation turn. Their first
retained cohort is the calibration input for any tighter CI floor.
