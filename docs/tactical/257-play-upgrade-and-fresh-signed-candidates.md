# Tactical 257: Play Upgrade And Fresh Signed Candidates

Status: **Active, 2026-10-04.**

Topics: `android-jstorrent-replacement`, `beta-release-readiness`,
`product-surfaces-and-migration`.

## Objective And Authorization

The maintainer requests continuing after the CI fix with actual Play-delivered
Android upgrade qualification, fresh signed candidates containing current
recovery fixes, and the remaining cutover checklist. This bounded slice records
an available physical old-to-new Play cohort and current-source non-publishing
desktop/Android CI candidates. Source commits/pushes and manual signed candidate
builds are part of that qualification. Stop with exact evidence and the next
unqualified checklist rows; no production release, feed promotion, store
production rollout or extension publication is implied.

## Dependencies And Invariants

Read 255, 250/251, the cutover checklist, Android release runbook, owning topics
and Machine Control's platform guide. 256 owns the current CI repair. Wait for
its exact-source presubmit before dispatching the fresh signed candidates.
Retain existing package IDs, launcher, original signing/updater roots, API 28
minimum, both Android ABIs and the independently staggered extension contract.

Use the common Machine Control facade for the physical testbed. Read-only
doctor passes on both physical devices. Their native adapters report target
claims unsupported; before mutations acquire an exclusive matching target-side
marker and release it in finally cleanup, as in 253. Keep target aliases,
accounts, paths, grants, private library details and captures out of public
evidence. Never clear/uninstall production app state or guess credentials.

One device already reports Play-installed 1.0.25; this observation establishes
current package state, not who updated it or preservation of a prior cohort.
The other reports Play-installed 1.0.23 and supplies the selected before/after
cohort if its existing state and store enrollment can be safely preserved.

## Ordered Work

1. Inspect old package identity, Play provenance and current library. Preserve
   inherited state; add only bounded controlled fixtures where safe and record
   independently checked content/settings/folder intent before updating.
2. Use the actual Play update route, qualify signer/launcher and retained
   library/storage/restart. Distinguish unavailable tester enrollment and human
   account steps from application failures. Do not substitute an APK sideload.
3. After repaired exact-source CI passes, dispatch the existing production
   desktop candidate and Android signed-build workflows on that exact commit.
   Both dispatch routes retain Actions artifacts and do not publish releases.
   Artifacts in this public repository are downloadable; no keys are included.
4. Inspect complete package/collector gates and independently verify retained
   public-root signatures and artifact hashes. Reconcile the checklist with
   exact passing subsets and remaining cohorts. Clean owned fixture state,
   listeners/captures and matching marker while preserving inherited state.

## Validation And Completion

Play-generated package signer is checked separately from the upload key. Require
package-manager Play provenance, version progression without uninstall/data
clear, retained native launcher and independently observed state/content effects.
Folders are OS capabilities; URI strings alone cannot prove retained permission.
Do not claim a Web Store in-place update from an unpacked extension. Broader
architectures, supported persistence declaration, sleep/reboot/network-loss and
production shipment remain open unless separately evidenced here.

Fresh CI must bind every artifact to one exact source, expected product identity,
both Android ABIs and the complete five-target desktop matrix. Require original
desktop updater root, Developer ID/notarization/stapling/Gatekeeper and Windows
publisher checks; retain actual skipped or failing lanes explicitly.

## Evidence And Restart Checkpoint

The actual Play 1.0.23-to-1.0.25 update preserves package UID and first-install
time. Independently inspected APKs share the Google-managed app-signing root.
The ordinary old standalone app creates one completed 256-KiB torrent and one
paused partial 4-MiB torrent in an owned picker-granted tree. Both rows, paused
intent and exact pre-update payload hashes survive. The unmetered-only setting
also survives. With the independent source offline, the completed file reaches
verified seeding state without changing its hash. The partial file conservatively
starts at zero verified progress while preserving its bytes; after the source
returns it completes with the independently expected full 4-MiB hash. Both rows
and full hashes survive force-stop/relaunch. This proves bounded standalone
read/write grant continuity, not reboot, a second tree or actual upload serving.
The [sanitized Play receipt](../evidence/android-play-upgrade-257.json) records
base-APK hashes separately from full split-install identity and lists every
unqualified cohort. Remove/keep-data preserves both full hashes. Removing the
owned rows and force-stop/relaunch leaves an empty library without resurrecting
the legacy source. The original network setting is restored; owned payloads,
metainfo, seed/tracker and UI dump are removed/stopped. The sole current folder
has no UI forget action, so its empty picker-granted tree remains explicitly.
The matching exclusive marker is released and final common doctor passes.

The installed successor exposes an overlooked release resource override:
`app/src/release/res/values/strings.xml` still names the app RSTorrent Canary,
overriding the restored main resources. This is a production candidate branding
defect, not a Play signer or migration failure. Correct the release resource
and require final resolved APK/AAB labels to be JSTorrent before staging fresh
signed candidates. Add negative artifact-label fixtures and qualify actual
processed release resources. Prepare 1.0.26/code 26 above the already-published
25 without changing identities, keys, minimum API or ABI scope.
The already-published internal artifact remains immutable and its label limit
stays recorded. No new Play upload is part of this repair.

The original-key CI inputs remain the signing authority. The controller's
existing local signing store still has the historical incubation certificate;
the unchanged signature validator refuses those locally rebuilt packages.
Do not substitute its certificate, generate a new key or weaken that gate.
Local 1.0.26 APK/AAB metadata and resolved resources independently pass the new
branding gate; all 24 Android release-tool tests, 116 release JVM tests and
release lint pass. These checks are local preparation, not qualified signing.
Fresh hosted candidates must supply original-key artifact evidence.

## Exact-Source Hosted Build Checkpoint

Source 46c19b3abf77e0d9d5ab2fca467a3bc48cf533b4 passes all ten executed
presubmit jobs and Website; manual-only verification/dependency jobs are skipped.
The [source receipt](../evidence/release-source-ci-257.json) binds their exact runs.
Fresh non-publishing desktop [37235683007](https://github.com/kzahel/rstorrent/actions/runs/37235683007)
and Android [37235699078](https://github.com/kzahel/rstorrent/actions/runs/37235699078)
are dispatched on that source. Desktop updater-input identity proof passes,
but its source gate exposes a separate asynchronous external-intake unit-test
race; packaging is correctly refused. Tactical 256 owns that repair and the
next exact-source retry. Android remains in progress. Neither route tags or
publishes a release.
