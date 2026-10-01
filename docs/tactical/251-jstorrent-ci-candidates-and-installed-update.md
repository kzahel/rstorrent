# Tactical 251: JSTorrent CI Candidates And Installed Update

Status: **Active, 2026-10-01.**

Topics: `product-surfaces-and-migration`, `desktop-jstorrent-replacement`,
`android-jstorrent-replacement`, `beta-release-readiness`, `client-surfaces`.

## Scope And Stopping Condition

Run signed production-identity release candidates in CI using the existing
RSTorrent signing inputs first. Verify artifacts against the retained original
JSTorrent public roots rather than assuming secret equality from their names.
Preserve exact source/run/hash identity and retain bounded failed-build artifacts
for diagnosis. Manual candidates cannot create tags/releases or enter a feed.
Use the existing desktop workflow; this is one-product succession, not a second
shipping product. Pause automatic incubation publication during qualification.

Confirm Android upload/app-signing gates in the checklist and attempt its signed
candidate with existing inputs, without Play upload or secret transfer. Prepare
the successor product-owned update-server descriptor. Inspect private operational
routing through dotfiles; do not copy private inventory into this repository.

If original-key signed desktop artifacts are available, install an exact old
JSTorrent release in a claimed, owned guest, generate controlled old state, check
for the successor update through an isolated candidate feed, install/relaunch
and inspect retained library/source/payload. Test-feed routing must be confined
to the guest; production users must not be offered the candidate. Do not weaken
TLS, updater authentication, signing validation or host fencing to obtain a pass.

Stop with the CI attempts, available installed evidence, exact blockers and a
restart checkpoint. Missing original signing material is a delivery blocker;
continue independent source/package/server work and record the actual result.

The user requests CI release builds and an installed update attempt. Commit and
push the required candidate source after verifying maintainer git identity, then
dispatch manual non-publishing builds. No public feed promotion, store rollout,
private-key printing, unrelated installed apps or personal profiles are included.

## Survey, Invariants And Ownership

Read Tactical 250/checklist, existing desktop/Android release workflows and exact
artifact/activation validators before edits. Original JSTorrent updater root:
`415D3DF4B3D0CFB8`; published RSTorrent Latest signatures declare
`788A785131367096`. Re-test the current secret via an actual CI signature.
Android's original GitHub APK certificate is separately pinned; Play upload and
app-signing certificates still need console qualification. Minimum API 28 is
accepted; no API/platform lowering is part of this slice.

The existing workflow owns jobs/temporary credentials and joined cleanup; its
collector owns exact artifact identity and signatures. Package validators select
only explicit known production/incubation identities. Application updater and
engine owners are unchanged; no DTO or runtime task is added. Android desktop-
specific origin behavior was already classified in 250. This release work does
not change peer/protocol/hot-path semantics.

The generic update service currently consumes JSTorrent's old-repo descriptor.
Prepare the successor descriptor locally and exercise product selection with
owned data. Live service routing/promotion remains a final reviewable operation
rather than a prerequisite for testing an isolated feed.

## Validation

- Negative source/input/profile and artifact-mixing cases; production manual
  builds always have publication false. Invalid signatures fail qualification.
- Retained-platform activation, package/notices/OS-signing checks, workflow lint,
  web/extension tests and proportional native checks.
- Current CI signing proof and release attempt; record exact source/run/hash,
  expected/actual signer and incomplete platform lanes honestly.
- Available installed old-to-new updater evidence, or the exact prerequisite
  failure. Restore owned app/profile/feed/guest/claim state after testing.

## Evidence And Restart Checkpoint

Implementation pending. Production descriptor/feed and existing secrets remain
unchanged. No CI candidate has been dispatched yet.

### Pre-Dispatch Checkpoint

Manual desktop production selection now merges 250's overlay, validates exact
production activation metadata and records product identity in all five artifact
receipts. Mixing products or adding publication inputs is rejected. The final
collector must verify all ten updater payload signatures against the retained
production public root. A separate CI nonce proof diagnoses the existing secret
without exposing private material; a failed proof cannot qualify delivery.

The Android manual rehearsal may explicitly use existing canary inputs, but
its original-certificate APK/AAB validator remains mandatory. A public certificate
fingerprint is emitted before building; unqualified outputs are diagnostic only.
Automatic incubation publication now needs an explicit opt-in variable, which
is absent. No production tag/publisher promotion is enabled by this slice.

Local validation: 47 release/input/signature/package tests pass, including real
minisign wrong-root and altered-byte refusal, product mixing/publication negatives
and production activation selection. Changed workflows pass actionlint. Android
Python release tools pass (23). The actual generic update server accepts the
new product-owned descriptor with the original hostname/root and successor repo.
The live symlink/feed is not changed. Runtime Rust/DTO/engine code is unchanged;
250's native baseline remains applicable and CI repeats desktop native checks.
