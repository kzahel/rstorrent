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
   Both dispatch routes retain private artifacts and do not publish.
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

Initial read-only physical package inspection complete. Installed journey and
fresh signed candidates pending the repaired presubmit and store availability.
