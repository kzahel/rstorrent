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

Candidate mechanics committed at `3b589dfda62daf4998d5ea516ff8c63cff97719b`.
Production feed and existing secrets remain unchanged. Manual desktop run
[36822295713](https://github.com/kzahel/rstorrent/actions/runs/36822295713) and
Android run
[36822298091](https://github.com/kzahel/rstorrent/actions/runs/36822298091)
use that exact source; package builds are still in progress.

The fresh CI nonce declares signing key `788A785131367096`. Independent minisign
verification succeeds against the incubation root and fails against the retained
JSTorrent root `415D3DF4B3D0CFB8`. Existing secret names do **not** establish key
equality. The original signing inputs must replace them before an old JSTorrent
installation can authenticate the successor. No private material was reported.
The diagnostic probe now fails visibly on a wrong root; package jobs do not
depend on that probe and still produce best-effort diagnostic evidence.

The real generic update server, using the successor descriptor, passed 33 HTTP
checks against owned GitHub metadata fixtures: five platform mappings, exact
signature/URL passthrough, default Stable/explicit Latest, equal/newer installed
versions (204), draft exclusion, unknown host/platform and invalid channel.
This proves routing and version selection, not artifact authentication or TLS.
No server code or live descriptor/symlink was changed.

In a claimed macOS arm64 guest (macOS 26.6.2), the exact released JSTorrent
0.2.1 app checked its unmodified production endpoint via `--check-update`,
returned exit 0 and `{ "available": false }`, with unchanged executable bytes.
Archive SHA-256:
`4bc5e979635fe9283d9ba60e43f86bfadcf619adf546cdbe4b68b27d424343f1`;
old executable SHA-256:
`f008e2d00e7e414e16096d1ea319d870807ddeddb25d459740b01a0727b019e7`.
An initial task path under macOS's `/tmp` symlink was correctly refused by the
released updater's starting-binary check; the canonical path passed. The test
restored inherited app data, removed owned files, returned the guest to its
prior suspended state and released its claim. This is baseline updater reachability;
no successor download/install/relaunch has been qualified.

### Linux Packaging Repair

Ordinary CI run `36822289035` failed its ARM64 unsigned bundle because the
upstream continuous AppImage output-plugin assets were replaced by a monthly
rebuild at 04:30 UTC on 2026-10-01. The SHA-256 gate refused the moved asset;
it was not bypassed. Review of the pinned source (`src/main.cpp`,
`.github/workflows/main.yml`) and upstream
[build 36815339264](https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/actions/runs/36815339264)
confirms unchanged plugin revision `536b068787179ea901964bd7dabc7bf61e4941c3`,
plugin API 0 and AppDir/output forwarding. Compared with September build
`33467741325`, linuxdeploy remains revision `07333c6` and embedded appimagetool
remains `8c8c91f` / build 295. Both newly downloaded assets match the GitHub
release API digests. Update the two reviewed byte pins; maintain download/hash
bounds, pre-signing attribution and final packaged-notice gates. Native hosted
bundle retry remains required; this is not source-offer/license graduation.
Local validation passes: 18 distribution-review tests, both real asset hashes,
and a wrong-byte download refusal that preserves the prior cache and cleans
temporary download state. No Rust/application source changes in this repair.
Before retry, correct the missing build-job `PACKAGE_PRODUCT` environment value
so package activation/staging checks receive the selected identity consistently.
The original manual release's source checks have passed on all native/web/tool
checks in that job; current signing inputs still block authenticated delivery.

### Android First Attempt

Run `36822298091` built both signed APK/AAB outputs, then failed release lint:
the launcher-removal activity overlay must explicitly declare `android:exported`
even though the merged main activity already inherits `true`. Repeat that
existing value in the overlay; the merged release behavior is unchanged.
Retained outputs remain unqualified. The current-input public certificate
SHA-256 is
`4420779ef395a490176986a058f72ba5b4a163f6eb9156e92a6e99896ef36849`,
different from the original GitHub APK root
`ccb5af8e44d626e9aefb1f0fbd8496dbf23ad27da9347248e71fb3ce70044915`.
Original Android signing inputs are also required; no certificate gate is relaxed.

The final one-product workflow reuses `ANDROID_UPLOAD_*` secret names and removes
the temporary rehearsal selector; replace their contents rather than maintaining
a second production signing tuple. No remote secret was changed. Local release
lint and merged-manifest generation pass with an owned temporary signing fixture
that is deleted afterward; no signed production qualification is inferred.

Independent checks of the retained CI outputs pass their actual signatures,
production package/version/launcher, identical Rust libraries in both ABIs,
native notices, bundletool validation and 16 KiB ELF/APK/AAB alignment. Their
signer remains the wrong canary root. APK SHA-256:
`7b5a4782ceef138c2442fb122210b27158e4eac69bd8efc330435d120c310fa8`;
AAB SHA-256:
`40ae747e3b75ecf9635ba284b29f36e2a81b6fa5e8f1d8548f48d3b3ffb08bc2`.

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
