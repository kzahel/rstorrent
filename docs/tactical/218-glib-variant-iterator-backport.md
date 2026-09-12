# Tactical 218: GLib Variant Iterator Backport

Status: **Complete locally and hosted (2026-09-12).** Explicitly approved
narrow backport.

Native source/package qualification below is complete and clears this
dependency blocker with mandatory source proof. Corrected-source run
`34693467376` passes all 12 jobs at `e535a5ffeb6cc9885a5e8f47196890d386960e35`.
The policy-only closure passes strict local review with audit inputs matching
the hosted source evidence. The earlier pending-qualification paragraphs
record intermediate results.

Topics: `beta-release-readiness`, `capability-readiness`

## Scope And Stopping Condition

Vendor the exact published MIT-licensed `glib 0.18.5` crate and apply only the
approved two-line mutable out-pointer repair for RUSTSEC-2024-0429. Preserve
its version and API. Enforce source integrity, honest advisory reporting and
attribution, and qualify optimized execution and native Linux desktop packages
on x86_64 and ARM64 before clearing this specific blocker.

Stop with validated source, negative integrity tests, optimized regression
coverage, both hosted Linux package gates, proportional package/picker/tray
lifecycle evidence, reconciled topics and substantial commits pushed to the
authorized nonpublishing CI branch. A GTK4/Tauri migration, changes to engine
semantics, publication, repaired signed Windows update, public disclosure
release and supported-baseline declaration are outside this slice.

## Source, License And Platform Applicability

The registry archive's checksum must match the existing Cargo.lock. Its
`.cargo_vcs_info.json` identifies gtk-rs-core revision
`42b9caf98e03ded086362d9653ca58fe94dc8658`, directory `glib`.
Inspect `src/variant_iter.rs`, including `impl_get`, all five affected iterator
methods and upstream `test_variant_str_iter_nth`, `_last`, `_count` and
`test_variant_iter_array`. The original LICENSE and every published source
file remain intact except the two reviewed changes. Independently authored
regressions live outside the vendored source.

The normative advisory and upstream correction are
<https://rustsec.org/advisories/RUSTSEC-2024-0429.html> and
<https://github.com/gtk-rs/gtk-rs-core/pull/1343>. C writes the output pointer;
Rust must pass `&mut p`, not a shared reference to immutable storage.

Locked target graphs contain this crate on Linux x86_64/ARM64 desktop only;
Windows, macOS, Android, iOS and the standalone session graph do not use it.
Android runtime/generated boundaries therefore do not change. This is a
platform dependency repair, not an engine/protocol feature; libtorrent and
JSTorrent do not own the faulty binding's invariant. The prior optimized
Linux probe already reproduced SIGSEGV with original source and passed the
candidate repair. It does not prove ordinary application reachability.

## Invariants, Ownership And Bounds

Cargo's workspace patch resolves the existing dependency family to one local
crate, excluded from first-party workspace membership. No new runtime owner,
background task, application command or generated boundary is introduced.
Build/review scripts own bounded synchronous integrity checks. Reject missing,
extra, modified or symlinked source, incorrect patch provenance and a changed
Cargo source selection. Preserve LF/source bytes on every checkout platform.

Audit output must identify source-verified remediation even if cargo-audit
omits a local package. Other warnings and review expiry stay enforced. Package
notices must include this local third-party crate with original license,
archive provenance and patch identity; do not mislabel it first-party or
unmodified. Clear only this advisory after qualification; this is not overall
release approval or closure of GTK3 maintenance warnings.

## Validation And Retirement

Test source/patch/lock/metadata tampering and notice omission. Native optimized
regressions cover all affected iterator entry points, empty/Unicode strings,
empty/singleton arrays, mixed direction, exhaustion and oversized skip counts.
Run proportional workspace fmt/clippy/tests and tooling checks, then hosted
native package builds and full ordinary CI. Package checks exercise the real
Linux environment with private state and bounded owned cleanup.

Retire the override when the supported upstream dependency graph carries the
repair; remove it only after the same regression and package gates pass.
Never change the version to impersonate an upstream fixed release.

## Implementation Checkpoint

The registry archive SHA-256 is
`233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`.
All 121 published files (1,717,592 bytes after patch) are preserved, including
original LICENSE and tests; the only source delta is the approved two lines.
Cargo metadata confirms exactly one local GLib and excludes it from workspace
membership. The Cargo.lock delta changes only this source and the Linux-only
regression dependency. Android and other non-Linux graphs remain unaffected.

Nine independent integrity/audit/notice tests and the 16 existing distribution
checks pass. Linux x86_64 notice generation retains 529 Rust and 28 npm
entries, now identifying the local GLib repair explicitly. Cargo-audit's six
unmaintained warnings remain; disappearance of the path package's advisory
is countered by mandatory source verification and a separate backport record.
The audit collector now restores the original registry identity only in an
in-memory projection. This retains future GLib advisory detection, records
both lock digests and rejects an ordinary path-skipping audit report.
The specific release blocker remains pending native qualification.

Local baseline passes: `cargo fmt --all -- --check`,
`cargo clippy --workspace -- -D warnings`, and `cargo test --workspace`.
Desktop release-tool tests, 22 Android notice/release-tool tests, changed
workflow actionlint, source verification, nine backport tests and 16 existing
distribution tests pass. Both Linux notice generations retain the GLib entry;
macOS generation still excludes that Linux-only dependency. Full native Linux
optimized and packaged qualification is the next execution gate.

### Hosted Checkout Correction

The first implementation is `aa056ff6e4ac1ada4ffe31d1872a2f8699efbe2b`, run
<https://github.com/kzahel/rstorrent/actions/runs/34691933241>. The Windows
source gate correctly rejects a CRLF-converted provenance JSON before build;
vendored source bytes themselves remain intact. Add an explicit LF attribute
for the checksum-bound manifest. A real Git checkout-index fixture with
`core.autocrlf=true` proves that ordinary Cargo.lock uses CRLF while the
manifest and original source retain their exact hashes. All ten backport
checks pass, including future-GLib-advisory rejection and an AppImage target
mislabeling attempt. The initial workflow remains a failure even if its
other platform jobs pass; the corrected source requires another hosted run.

### First Hosted Native Results

Run `34691933241` finishes with ten successful jobs and two failures: the
Windows manifest newline gate above and an unchanged iOS mirrored-layout UI
test waiting five seconds for the Add dialog's `magnet-input`. Its saved
xcresult records 33 passed / one failed; the failure hierarchy remains on the
library screen. GLib is absent from that platform graph. No iOS assertion is
weakened or product change inferred; the corrected full run repeats the test.

Both native Linux jobs pass **44 desktop tests**, the **three independent
iterator cases in both dev and release profiles**, and **346 session tests**
(two ignored). Actual unsigned AppImages pass source, embedded backport notice,
native-host and activation checks. x86_64 inventories 327,422,713 bytes,
174 native components, 113 distro packages and 128 notice files; ARM64
inventories 340,992,541 bytes, 173 components, 112 packages and 127 notices.
The ARM64 AppImage SHA-256 is
`163cca989a62dfdea44be0764f13379773034f98e2bc6abd852aead0c450b538`.
The hosted advisory artifact retains all seven original registry warnings and
separately records the exact source-verified GLib repair. It still reports
release-ready false while the installed-package gate remains pending.

### Native Packaged ARM64 Qualification

The exact unsigned ARM64 AppImage above is independently hashed after
transfer and exercised in a disposable Ubuntu 24.04 ARM64 GNOME 46 Wayland
workspace. System GLib is `2.80.0-6ubuntu3.8`; WebKitGTK is
`2.52.3-0ubuntu0.24.04.1`. The packaged product starts with private HOME and
XDG state without WebKit environment workarounds. The rendered Settings
view, native GTK folder chooser and visible GNOME indicator are observed.

Picker Cancel leaves zero storage roots. Selecting an owned folder reports
that it was added and records exactly one default root with its exact path.
With Run in Background checked, native Alt+F4 removes the showing window
while the application and registered StatusNotifierItem remain. Invoking
the exported native tray menu's Show handler restores a visible, showing
RSTorrent window. Invoking its Quit handler releases the single-instance
bus name and ends the launch unit with `Result=success`, `ExecMainStatus=0`
and `ActiveState=inactive`.

A second launch with the same private profile reaches Settings and again
quits through the native tray handler with status zero. Independent read-only
SQLite inspection after that launch confirms the same sole default root.
The selected folder's payload sentinel is unchanged through both lifecycles,
SHA-256 `86f5de79a0d28549a5aea52b7117202ef7e8ea9ab2c7fee14a470092a676a481`.
The VM is shut down and its disposable overlay discarded through Machine
Control, removing the owned package, state and payload. The baseline remains
off. No host UI input or primary browser is used.

This qualifies the actual current-source ARM64 package's picker, native tray
handlers, background lifecycle and persistence. It does not turn the older
signed x86_64 GNOME indicator gap into evidence for that older package or
claim a repaired signed update. Ordinary VM rendering emits EGL/DRI and
optional canberra-module warnings; the observed product behavior and both
clean exits pass.

The corrected run's Linux package jobs also pass. Their entire extracted
inventories are identical to the first run: 487 ARM64 / 391 x86_64 entries,
including every file hash, notice manifest and native inventory. Thus the
installed ARM64 evidence exercises the same packaged product bytes as the
corrected source build. ARM64 desktop executable SHA-256 is
`d5bd3b7a6c44d59fbef92be21e9d5bdecb186110989e49fb3460b7dd8072d9a2`;
x86_64 is `277d9ae6b627fb97e42b63e6780455522c287a44a773f14998c937ac5ea5cd05`.

### Dependency Policy Closure

Commit `1f506441edfb738da1be472a2280219cdbe1917f` records native qualification
and removes only the GLib entry from `release_blockers`. Fresh Cargo/npm
collection and `review-dependency-audit.py --require-release-ready` pass.
The resulting database revision, all seven warnings, both lockfile hashes
and source-verified backport record exactly match the corrected full run's
hosted advisory artifact; only the qualified blocker and readiness fields
change. Ten backport tests and 16 distribution tests pass again.

An attempted separate `dependency-review.yml` dispatch returns GitHub 404
because that new workflow is not on the default branch. The established
manual CI workflow already invokes it at the tested caller revision. This
policy-only closure is therefore validated locally against matching hosted
inputs; it is not represented as a separate hosted policy run. No default
branch push or publication is used to register the workflow.

### Final Hosted Result

Run <https://github.com/kzahel/rstorrent/actions/runs/34693467376> completes
successfully at 2026-09-12 12:53:29 UTC with all 12 jobs passing. Windows
passes the corrected LF source gate, 43 desktop / 345 session tests (two
ignored), local-address selection, unsigned NSIS installation, activation
registry, native-host and artifact inspection. Rust passes 1,507 workspace
tests with 18 ignored plus deterministic transfer and lifecycle gates.
Android's owned runtime, iOS unit/UI/archive, web, extension, release tools,
extended storage recovery and all four native package lanes pass.

The final policy/evidence commit changes no tested implementation. No
signed update, default-branch push, tag, release or support declaration is
part of this tactical. Native notice/source-delivery review and the other
campaign release gates remain explicit. Owned VM/claim cleanup is verified;
downloaded logs, captures, packages and investigation fixtures are removed.
