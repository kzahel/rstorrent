# Tactical 157: Beta Release Foundation

Status: **Complete (2026-08-22).** The authoritative beta ledger, Android
module graduation, provisional desktop/iOS artwork and local bundle metadata,
entry-point/status reconciliation, and proportional local build evidence now
pass. Tactical `158` is the next selected release slice; no product identity,
credential, production route, release, or publication changed.

Topics: `beta-release-readiness`, `capability-readiness`, `client-surfaces`,
`product-surfaces-and-migration`, `product-state-and-feedback`,
`client-persistence`, `code-organization-and-refactoring`

Dependencies: completed product Tacticals `117`, `147`--`149`, `152`, and
`154`; completed hybrid Tactical `156`; the maintained desktop, Android, and
iOS clients; and the accepted external `desktop-update-v1` contract in
`kzahel/desktop-release-kit`.

## Decision And Desired Outcome

Turn the repository from an implementation campaign whose release gaps are
scattered across topics into a release campaign with one durable, honest beta
ledger. Complete the repository-identity cleanup that is safe before public
identifiers are frozen: graduate the whole Android application from its
historical experiment path, make provisional app artwork usable by platform
builds, correct stale entry-point/status documentation, and explicitly order
signed updating and CI as the next bounded slices.

This tactical does not claim release readiness. Its outcome is a truthful,
executable starting point from which the remaining gaps cannot be mistaken for
finished distribution infrastructure.

## Scope And Stopping Condition

This tactical owns:

1. a living `beta-release-readiness` topic with stable checklist IDs,
   platform lanes, beta blockers, deliberate feature deferrals, updater
   contract, CI matrix, and ordered next slices;
2. selection of this tactical as the single authoritative **Now**, moving
   decision-complete wired-LAN Tactical `153` to Later without invalidating it;
3. moving the complete maintained Android Gradle module from
   `experiments/android-engine-bootstrap` to `clients/android`, including its
   product UI, platform adapter, generated-boundary build, retained diagnostic
   harness, Gradle wrapper, tests, and device runner;
4. updating active commands, scripts, tests, topics, and indexes to the new
   Android path while retaining accurate historical class/package evidence;
5. renaming the Gradle project and module documentation from an engine
   bootstrap to the Android client without changing the unreleased Android
   application ID in advance of the product-identity decision;
6. generating a complete provisional desktop Tauri icon set from the existing
   repository-owned SVG, enabling ordinary local bundle configuration, and
   supplying a provisional iOS AppIcon from the same source;
7. correcting README, DEVELOPMENT, platform-topic, refactoring-topic, and
   tactical-index statements that still describe iOS as eventual, Android as
   an experiment, Tactical `156` as planned, or packaging gaps imprecisely;
   and
8. validation of path/reference cleanup, Rust formatting, web checks affected
   by configuration, Android two-ABI/Gradle tests, iOS generation/archive, and
   at least one local desktop bundle build in proportion to the edits.

The tactical stops when the beta ledger and queue are authoritative, no active
build/test command points at the old Android directory, the moved Android
client builds from its durable path, provisional icon assets are consumed by
desktop/iOS configuration, entry-point status is truthful, and recorded local
validation passes.

## Non-Goals

- choosing RSTorrent versus JSTorrent public branding or taking over an
  existing store identity;
- changing Android's unreleased `org.rstorrent.bootstrap` application ID or
  iOS/desktop bundle identifiers before the release identity decision;
- deleting the retained Tactical 004/005 diagnostic service and runner,
  rewriting historical tactical prose, or changing Android runtime ownership;
- provisioning updater/signing/store credentials or a production update
  route;
- adding updater code, release workflows, general presubmit CI, store
  listings, tags, releases, or publication;
- implementing UI or BitTorrent feature gaps; or
- executing physical-device, public-swarm, or production-service mutations.

## Invariants And Compatibility

- The Android move keeps the full module together. Compose, SAF, foreground
  service, UniFFI generation, ABI packaging, tests, and diagnostic evidence do
  not split across `clients/` and `experiments/`.
- Existing unreleased development state may be cleared, but this tactical does
  not create an accidental public identity or migration promise.
- Historical tactical titles such as `004-android-engine-bootstrap` and class
  names that identify its diagnostic contract remain truthful. Executable
  paths are updated to the current module location.
- Provisional artwork is repository-owned and explicitly non-final. It must be
  technically valid at every configured size without implying a final brand.
- Enabling local Tauri bundling does not imply signing, notarization, updater,
  clean-machine installation, or release evidence.
- The shared update contract is adopted by reference, not copied into a
  divergent RSTorrent protocol. Product-specific route, key, UI, release tags,
  and lifecycle remain owned here.
- The repository remains free of signing keys, passphrases, developer teams,
  machine inventory, or other private operational values.

## Owner And Data-Flow Map

This slice adds no runtime tasks or data paths.

```text
beta-release-readiness topic
  -> capability queue and bounded tacticals
  -> platform release evidence records

repository-owned icon.svg
  -> generated Tauri desktop icons
  -> provisional iOS AppIcon PNG

clients/android
  -> Gradle/Compose/SAF platform owner
  -> generated Kotlin + two Rust ABI libraries
  -> retained explicit-target diagnostic/product runners
```

## Validation Matrix

Run and record:

- `git diff --check` and a tracked-text search proving no active executable
  command/default points to `experiments/android-engine-bootstrap`;
- `cargo fmt --all -- --check`;
- `npm run typecheck --prefix clients/web` and
  `npm run test --prefix clients/web` when desktop/web configuration or assets
  affect the shared package;
- `clients/android/build.sh`, followed by Gradle `lintDebug`,
  `testDebugUnitTest`, and `assembleDebugAndroidTest` without starting a
  device;
- iOS project generation and an unsigned archive to a temporary path;
- a local Tauri production build with the narrowest native bundle sufficient
  to prove the configured icon/package path; and
- cleanup of generated evidence outside ignored build directories.

If a platform toolchain is unavailable, record the exact missing gate and keep
the tactical in progress. No physical device or public service is required.

## Escalation And Next Slice

Ordinary path updates, internal variable names, generated icon filenames,
documentation reconciliation, and build-only fixes at the moved-module or
bundle boundary are authorized. Stop for a choice that would freeze public
branding/identifiers, take over existing JSTorrent application state, add a
dependency with a material license/maintenance cost, provision credentials,
change a production route, or publish an artifact.

## Completed Evidence

- Added [`../topics/beta-release-readiness.md`](../topics/beta-release-readiness.md)
  with stable shared/platform checklist IDs, independent release lanes, MVP
  boundaries, updater and CI contracts, and ordered tacticals.
- Moved the complete tracked Android module to `clients/android`, renamed the
  Gradle project `rstorrent-android`, updated active scripts/tests/commands and
  historical executable paths, and retained the unreleased
  `org.rstorrent.bootstrap` identity plus explicit diagnostic harness for a
  later identity/isolation decision.
- Generated the standard Tauri desktop PNG/ICNS/ICO/Windows tile set from the
  repository SVG, enabled local bundle metadata with per-user NSIS policy, and
  added an opaque 1024x1024 iOS AppIcon from the same provisional art. The art
  remains explicitly non-final.
- Corrected README iOS/product truth, DEVELOPMENT priority and Android paths,
  capability **Now**, Tactical `153`/`156` index status, client/release gaps,
  persistence baseline, updater installation identity, product migration
  relationship, and Android refactoring status.
- The old Android path remains only in historical prose in this execution
  record and the new module's move explanation; no executable command or
  default points there.

Validation on 2026-08-22:

- `git diff --check` and the tracked path audit pass.
- `cargo fmt --all -- --check` passes.
- `npm run typecheck --prefix clients/web` passes; web unit tests pass 248 with
  2 skips. The production web build/CSP check also passes as part of the Tauri
  build, with the pre-existing 1.39 MB chunk-size warning.
- `clients/android/build.sh` cross-builds and packages `x86_64` and
  `arm64-v8a`, generates both UniFFI packages, assembles the debug APK, and
  passes Kotlin unit tests. Gradle `lintDebug`, `testDebugUnitTest`, and
  `assembleDebugAndroidTest` pass without starting a device. Existing Android
  deprecated-API warnings remain release cleanup, not a move regression.
- iOS project generation and a 35 MB temporary unsigned arm64 archive pass
  under Xcode 26.6/iOS 26.5 SDK. Existing target-specific Rust dead-code
  warnings remain; the archive consumes the new AppIcon.
- `tauri build --bundles app --no-sign --ci` produces the production
  `RSTorrent.app` with the configured icon after compiling the Rust desktop
  binary and shared React bundle. Signing, notarization, installers, and
  update artifacts remain Tactical `158` gates.
- Python syntax checks pass for the moved runner and three Android interop
  entry points; JSON validation passes for Tauri and iOS asset metadata.

Temporary iOS archive evidence was moved to Trash after validation. Generated
project/build directories remain under existing ignores. No device, public
swarm, credential, production update service, tag, release, or publication was
used.

## Next Slice

Tactical [`158`](158-desktop-signed-packaging-and-updater.md) owns desktop
signed packaging and `desktop-update-v1` adoption. It separately defines
per-app key/route provisioning, client updater states and privacy, package
ownership, release artifact validation, tagged-workflow failure boundaries,
and exact older-to-newer testbed evidence. Cross-platform presubmit CI follows
as its own bounded slice.
