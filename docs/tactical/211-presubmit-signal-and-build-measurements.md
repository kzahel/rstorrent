# Tactical 211: Presubmit Signal And Build Measurements

Status: **Complete locally and hosted (2026-09-12).**
User-authorized first CI improvement slice. Push and nonpublishing hosted CI
qualification were authorized on September 12.

Topics: `beta-release-readiness`, `capability-readiness`, `client-surfaces`,
`oracle-driven-engine-campaign`, `incoming-reachability-and-seeding`

## Scope And Stopping Condition

Repair the recurring companion-extension packaging rejection and investigate
the intermittent pure-v2 active-upload test failure. Separate extension and
release-tool validation from web contract/type/unit/build/E2E checks. Measure
Rust compilation before recommending a different CI profile or cache policy.

Stop when the reproduced failures have bounded regression evidence, independent
jobs preserve existing coverage and workflow invariants, appropriate local
checks pass, and build measurements plus remaining hosted evidence are recorded.
Hosted qualification was subsequently authorized and is recorded below.
Local validation alone does not establish hosted success. Branch protection,
publication, automatic retry, dependency additions and public-swarm work
remain outside this slice. Job filtering and new integration/emulator suites are later slices.

## Dependencies And Invariants

Read Tactical `159` for the CI contract, `194` for companion permissions and
packaging, and `151` for pure-v2 verified active upload and seed handoff. Keep
the exact ARC endpoints, local executable assets, manifest CSP, generated
application API, payload authority, and joined lifecycle intact. Inspect the
offending bundle strings before accepting any new inert URL. Include negative
URL cases; do not disable the validator.

The application service owns reconciliation; content-generation tasks own
transfer completion and joined teardown; incoming registration belongs to the
session network and the admitted torrent generation. Test polling must drive
the same application maintenance that real service hosts drive, and must not
confuse persisted Complete with joined generation termination. No new runtime
task, owner, protocol state, or dependency direction is planned. If a runtime
defect is found, update this design and its pinned source/test review before
changing production behavior; Android parity then applies in the same slice.

Bounds remain five-second transition deadlines, loopback-only scripted peers,
one two-piece pure-v2 fixture, bounded repeated regression runs, and seven-day
CI diagnostic retention. No larger sleeps or retry-to-green policy.

## Validation And Measurement

- Reproduce companion packaging on the unchanged source; test admitted inert
  URLs and rejected foreign endpoints, then build and validate the real ZIP.
- Reproduce the pure-v2 handoff failure and add controlled synchronization that
  exercises the failed ordering, followed by repeated and concurrent tests.
- Run workspace fmt, warnings-denied clippy, and tests; web contract drift,
  type/unit/production and bundled-Chromium deterministic E2E; moved release
  tool commands; workflow lint and diff checks.
- Record compile versus execution time from hosted job logs. Add bounded Cargo
  timing artifacts and collect local timings without treating unlike cache or
  machine states as a controlled speed comparison. Change no profile solely
  from speculation.
- Generated Android/Swift contracts and runtime behavior are unchanged if the
  Rust repair remains in the test harness; platform rebuilding is then
  inapplicable, not claimed from host tests.

## Execution Checkpoint

### Hosted Windows Recheck Handoff Follow-up

Run `34682523287` exposes a second completion/registration wait in
`durable_complete_torrent_applies_slots_live_and_fences_lifecycle`: Windows
passes 344 session tests but this test observes zero registrations after its
seed lifecycle transitions. Investigate the force-recheck completion boundary
against the existing source dossier and `reap_finished` ownership. A directly
owned application without its maintenance task must drive an application
command after checker termination; persisted Complete alone is insufficient.

Force that ordering in the existing test, demonstrate the old read-only wait
fails, then drive the ordinary command path and retain the existing five-second
bounds, exact payload/accounting, zero-slot choke, generation continuity,
archive/pause/removal and joined shutdown assertions. Stop after focused and
full session validation plus the repaired hosted Windows/package gate pass.
The controlled negative reproduces the exact timeout on macOS too. The
test-only repair passes 20/20 forced-order repetitions across four processes,
all 346 active session tests (two ignored), tests clippy with warnings denied
and workspace formatting. The old Snapshot poll could observe Complete before
task termination; the corrected test forces termination first, proves Complete
with zero registrations, and then uses the same ordinary Snapshot command to
join the checker and restore admission. No production runtime, protocol,
Android contract or resource policy changes. Corrected run `34684328524`
passes all 12 jobs at `8f31f98a48313c57c3e3ba16e5b8907b02d63135`, including
all 345 active Windows session tests and installed NSIS/notice checks.
The Rust gate passes 1,504 workspace tests (18 ignored), both interop smokes
and retained timings. [Exact run evidence](../evidence/release-readiness-ci-2026-09-12.md).

September 12 hosted qualification uses `release-readiness-ci`: pushing `main`
would also deploy the public website, which is outside the CI authorization.
Manual CI calls the same storage-recovery and dependency-review workflows used
by their schedules. This also qualifies newly added workflows before they are
present on the default branch, where GitHub otherwise rejects their individual
dispatches. Ordinary push/PR coverage and all existing time/resource bounds
remain unchanged; the additional jobs run only on manual CI dispatch.

Initial source is `9c6c00b`. Hosted run `34084340378` fails companion packaging
and the pure-v2 test's incoming registration wait. Runs `34037979895`,
`33586149709`, and `33475869664` also fail companion packaging before any web
contract/type/unit/E2E step. Local companion packaging reproduces the same
unexpected-remote-URL rejection. Both repairs and local validation now pass;
the September 12 qualification above now closes the hosted gate with all
12 jobs and retained timing artifacts.

### Diagnosis And Source Review

The rejected domain is `https://formatjs.github.io`. The installed locked
`clients/web/node_modules/@formatjs/intl/index.js` embeds tooling documentation
links in message-formatting errors. The companion bundle contains those inert
diagnostics. Admit that exact HTTPS host alongside the existing reviewed
documentation hosts; compare complete matched hosts instead of prefix or
substring membership, and report unexpected hosts in the failure. The
manifest's unchanged exact ARC-only CSP still owns network admission.
Independently authored fixture tests reject foreign hosts, lookalike suffixes,
ARC substrings, unreviewed WebSocket schemes, and dynamic code.

The Rust failure is test synchronization, not a production lifecycle change.
`ApplicationService::dispatch` reaps finished tasks and reconciles admission;
real hosts additionally run `ensure_maintenance_owner`. The old test stopped
dispatching when the persisted state became Complete, which can precede the
content task's joined termination. Its subsequent read-only incoming snapshot
poll could never create the completed-seed registration. The repair first
observes task termination without reconciliation, checks Complete with zero
registrations, then dispatches a snapshot and proves reaping plus registration.
The payload, active verified upload, accounting, and cleanup assertions remain.

Reconfirmed libtorrent pin `7d7fc38fac61177fa5e02148f791b2f65250b09d` and read
`src/torrent.cpp::{finished,completed}`, `test/test_checking.cpp` pure-v2
checking/corrupt/incomplete/recheck cases, and
`simulation/test_transfer.cpp::is_finished`. These distinguish asynchronous
completion transitions from immediate observations. BEP source pin
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`,
`beps/bep_0052.rst` peer-message/have rules retain verified-piece authority.
RSTorrent intentionally retains its own joined generation and admission
ownership. Tactical `151` owns the existing JSTorrent v1-only product/history
survey; no new product behavior, oracle source, or fixture is imported here.
Android and iOS production code, generated boundaries, and semantics do not
change; this Rust edit is entirely inside `#[cfg(test)]`.

### Regression Evidence

- Unmodified companion packaging reproduced the hosted rejection. After the
  repair, extension tests and the real companion ZIP packaging pass, including
  ten validator tests (eight new bounded cases).
- The unchanged pure-v2 test passed 24/24 runs in four concurrent processes,
  demonstrating why ordinary repetition alone was insufficient. Forcing task
  termination before the old read-only registration wait reproduced the exact
  hosted failure: Complete, zero registrations, 16,384 uploaded bytes, then the
  existing five-second deadline. Adding the application command makes that
  same controlled ordering pass, followed by 40/40 runs in four concurrent
  processes under concurrent workspace compilation and web validation.
- Workflow lint passes with pinned `actionlint` 1.7.9. The web, extension, and
  workflow/release-tool jobs are independent; all existing native jobs and
  coverage remain. No retry, skip, timeout relaxation, or secret was added.

### Build Measurements And Decision

Hosted Linux run
[`34037979895`](https://github.com/kzahel/rstorrent/actions/runs/34037979895)
restored a roughly 1,723 MB Rust cache. The workspace-test step took about
17.2 minutes; Cargo reported **16m 00s** compiling its test profile. The two
earlier feature-specific test builds compiled for **1m 40s** and **3m 00s**.
This identifies compilation and feature-graph rebuilds as the next measurement
target; test-runner parallelism cannot remove most of that critical path.

On the available macOS arm64 host with Rust 1.97.0 and an existing target tree,
the initial session-lib test compilation took **50.24s**, including **30.47s**
for the session test unit and **13.44s** for the engine. Recompiling the repaired
session test unit took **26.70s** wall time (**26.58s** for that unit); its
focused test executed in **2.08s**. These are warm/incremental-workload
observations, not clean-build or cross-machine speed comparisons; Cargo
incremental compilation itself remains disabled by the existing dev profile.

The workflow now separates workspace test-binary compilation (`--no-run`) from
execution and retains timestamped HTML `--timings` reports from workspace and
feature-specific test builds for seven days, including on later failure.
Doctests still compile during `cargo test`; the split does not claim otherwise.
See Cargo's [timing report documentation](https://doc.rust-lang.org/cargo/reference/timings.html).
Keep `opt-level = 2`, cache policy, dependency graph, and runner selection
unchanged until the new hosted unit/concurrency reports support a controlled
profile or cache experiment. No speedup is claimed by this slice.

The full local workspace test-binary compilation subsequently took **231.39s**
wall time (358 compiled units, 480 fresh). The longest individual units were
engine lib tests (**151.85s**), session lib tests (**129.96s**), the desktop
library (**118.24s**), and headless lib tests (**103.06s**). Unit durations
overlap and cannot be added into wall time. This run overlapped the bounded
web/regression validation and uses an existing target tree, so it is diagnostic
evidence rather than a clean, idle-machine performance baseline.
The subsequent workspace test command took **83.66s**, including doctests,
with 1,499 passed and 18 existing ignored tests.

### Local Validation Inventory

Completed on macOS arm64, Rust 1.97.0, Node 25.8.2 (hosted Node remains 22):

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings` (36.27s);
- `cargo test --locked --workspace --no-run --timings` (231.39s);
- `cargo test --locked --workspace`: 1,499 passed, 18 existing ignored
  (83.66s including doctests);
- `cargo test --locked -p rstorrent-session --lib pure_v2_active_generation_uploads_only_verified_piece_before_completion --timings`;
- the bounded 24-run unchanged and 40-run repaired direct test-binary cohorts
  described above, using four processes and isolated temporary roots;
- `npm test --prefix clients/extension` and
  `npm run package --prefix clients/extension`;
- `npm run typecheck --prefix clients/web`;
- `npm run test --prefix clients/web`: 381 passed, 2 existing skips;
- `npm run build --prefix clients/web`, including production CSP validation;
- `npm run generate --prefix clients/web`, followed by
  `git diff --exit-code -- clients/web/src/api/generated clients/web/src/fixtures/reactive-trace.json clients/web/src/fixtures/view-set-trace.json`:
  no drift;
- `CI=1 npm run test:e2e --prefix clients/web`: 39 passed, 14 existing
  live-only skips, using bundled Chromium; browser/server processes terminate;
- `node scripts/validate-desktop-release.mjs`;
- `node --test scripts/validate-desktop-package.test.mjs scripts/validate-desktop-release.test.mjs .github/scripts/validate-desktop-release.test.mjs .github/scripts/write-release-checksums.test.mjs .github/scripts/write-crostini-release-manifest.test.mjs .github/scripts/write-headless-release-manifest.test.mjs`:
  27 passed;
- `bash -n scripts/test-headless-installer.sh scripts/test-crostini-installer.sh`;
- `uv run --project tests/interop --locked python tests/interop/first_verified_piece.py --runs 1`:
  pinned libtorrent 2.0.13 verifies the exact 40,000-byte payload in 0.458s
  scenario time; payload high-water 40,000 bytes under a 262,144-byte limit,
  16,384-byte verification buffer, cleanup passes. The full command took
  24.23s including prerequisite work;
- `go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.9` across all workflows;
  and
- `git diff --check` plus a before/after step inventory confirming no existing
  check step was removed.

At the local checkpoint, the unchanged Crostini/headless shell integrity suites
were syntax-checked on this Mac rather than executed; the subsequent hosted
release-tools job runs and passes them on Linux. Native package/mobile builds
were not repeated during that test-only local repair. Hosted run `34684328524`
now supplies the complete 12-job platform/package/runtime qualification and
seven-day timing/evidence uploads recorded above.

The local and hosted stopping conditions are satisfied. Temporary test roots,
logs, browser output, generated ZIP and downloaded timing/package artifacts
are removed after recording results. Profiles and cache policy remain unchanged;
new tuning requires a bounded comparison using the recorded measurements.
