# Tactical 218: GLib Variant Iterator Backport

Status: **Active (2026-09-12).** Explicitly approved narrow backport.

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
