# Tactical 213: Owned Android Runtime CI

Status: **Complete locally (2026-09-12).** Authorized release-readiness campaign,
following Tactical 212's shared application verification gates.

Topics: `beta-release-readiness`, `capability-readiness`, `client-surfaces`,
`android-saf-storage`

## Scope And Stopping Condition

Add a bounded unattended product-runtime gate to the existing Android CI job.
Reuse the real `product-file-selection` SAF campaign: magnet metadata wait,
preconfirmation no-transfer, selective payload hashes, process restart,
force recheck, local torrent cancellation, exact removal and resource bounds.
Run it on a newly created API 35 AVD with the existing dual-ABI debug APK.
Record local ARM64 emulator evidence and retain sanitized seven-day CI JSON;
hosted x86_64 execution remains a separate gate until authorized source push.

Stop after the owned runner, workflow, failure/privacy controls, local runtime
campaign, and owning documentation are validated and committed. Signed store
installation, physical ChromeOS qualification, mobile product policy changes,
new engine behavior, release publication and supported-version declaration
remain separate campaign work.

## Ownership, Bounds And Failure Policy

The wrapper owns one unique `rstorrent-` AVD below a private temporary root,
one process group and at most ten minutes of runtime. It uses the configured
SDK, never an installed user's AVD. The existing product harness owns its
application, fixtures, SAF grants, loopback libtorrent and emulator session.
A wrapper deadline or output overflow terminates and joins the complete child
process group; normal and failed paths remove the owned AVD/files. No physical
device mutation or shared ADB server shutdown is allowed.

Use the installed API 35 Google APIs image for the native host architecture:
ARM64 locally and x86_64 on Ubuntu CI with KVM. Pin the image/API coordinate,
SDK/NDK, Rust, Python/uv and existing action revisions. No additional action or
runtime dependency is needed. SDK image revisions may be updated by Android's
package server; retain the actual image revision in the evidence.

Bound console capture to four MiB and retained JSON to explicit outcome,
version/ABI, APK hash, selection/recheck/removal facts and numeric resource
high-water values. Do not retain AVD/device IDs, grant paths, torrent IDs,
content names, endpoints or raw logs. Failures retain only a stable failure
category; raw diagnostics stay in the transient job console.

## Evidence Basis And Validation

This promotes the existing file-selection tactical's deterministic product
runner rather than adding engine behavior. Its assertions already exercise
the generated Kotlin boundary, Compose input, real SAF broker and first-party
Rust owner. Read `client-surfaces`, `android-saf-storage`, the current Android
CI job, `clients/android/run_bootstrap.py`, and its owned AVD support before
implementation. Tactical 212 owns the independent pinned-libtorrent integrity
and verification changes; this slice preserves those accepted contracts.

Required evidence: missing prerequisite fails before mutation; bounded child
failure/timeout cleanup; sanitized report excludes unexpected identifiers;
actual fresh-AVD product profile passes and leaves no emulator or AVD behind;
workflow lint and existing Android generated/dual-ABI/JVM/lint gates. A local
ARM64 pass does not close hosted x86_64 or physical-device gates.

## Discovered Runtime Failures

The first owned API 35 phone run persisted the SAF grant but sent its next
intent while DocumentsUI was still the resumed activity. Android acknowledged
delivery without the lifecycle diagnostic reaching the product instance.
Fence the actual resumed product activity before sending that intent; retain
launch details on timeout. Do not add fixed sleeps or accept a missing reply.

With that fence, the same campaign reaches selection and exposes a real phone
layout failure: the unweighted 420-dp file list pushes Download/Cancel below
the dialog window. Successful row toggles also incorrectly set the override
limit error, while checkbox limit rejection clears it. Read Tactical 203 and
its focused application/client/Android topics before this bounded repair.
Give the file list only the space left after fixed controls and use one toggle
handler for row and checkbox semantics. Keep the paged list bounded. The
fixture runner may scroll the observed list bounds to find an offscreen file;
it must still click real, visible Download/Cancel controls. No selection,
engine, generated-contract or product-policy change is authorized or needed.

## Completed Evidence

Two complete API 35 ARM64 phone campaigns pass after the observed-activity
fence and bounded Compose repair. The final run uses the bounded-output
wrapper and asserts that two valid row toggles do not show the override-limit
error. Image coordinate is `android-35/google_apis/arm64-v8a`, package revision
9; debug APK SHA-256 is
`bff248627fff9ff79aa2103ca63b51be1794676ba2470748ff8b98465287d42f`.
The existing profile proves metadata wait without payload, selected hashes,
process restart, force recheck, absent skipped files, local torrent
cancellation and exact removal. Storage high water is 3 of 40 handles. Every
attempt, including the pre-repair failures, terminates its emulator; successful
reports explicitly confirm removal of the private AVD root. `adb devices`
returns no remaining emulator.

The 12 Android release/wrapper Python tests pass, including deadline process
reaping, bounded output, failure propagation and identifier stripping. The
full Android build from Tactical 212 supplies both Rust ABIs and generated
Kotlin; after the Compose repair,
`./gradlew assembleDebug testDebugUnitTest lintDebug assembleDebugAndroidTest`
passes. `go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.9`, Python
compilation and `git diff --check` pass. No Rust, application contract, schema,
external route or platform policy changed in this slice.

The ordinary Android job now installs the explicit API 35 x86_64 image,
checks KVM, executes the locked product cohort in a uniquely owned AVD and
retains only the allowlisted seven-day report. Hosted x86_64 runtime execution
remains unclaimed until an authorized push; local ARM64 success is not that
hosted result. Signed AAB evidence already exists under Tactical 210 and is
kept distinct from debug runtime qualification.

Next: commit this substantial Android verification slice, then continue the
claimed native desktop installed campaigns, dependency/artifact review,
diagnostics/privacy presentation and supported-baseline preparation. Source
push, hosted confirmation, public deployment and support declaration remain
explicit final actions, not effects of local validation.
