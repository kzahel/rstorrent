# Tactical 252: Opt-In JSTorrent Installed Upgrade Rehearsal

Status: **Active, end-to-end implementation and owned installed validation
authorized 2026-10-01; test-only routing and deployment remain bounded below.**

Owners: `desktop-jstorrent-replacement`, `product-surfaces-and-migration`,
`beta-release-readiness`. Generic routing belongs to `simple-app-update-server`;
private deployment configuration belongs to dotfiles.

## Scope And Stopping Condition

Plan one bounded end-to-end desktop checkpoint: an exact released JSTorrent
0.2.1 installation receives a pinned, original-key-signed CI successor through
its ordinary HTTPS updater, installs/relaunches and migrates generated legacy
state. Start with Linux x64 AppImage, then independently qualify Windows NSIS
and macOS after Apple notarization is available. One passing platform does not
close the complete cutover checklist.

Implement and locally test an opt-in server cohort before reviewing its live
deployment. The existing release already sends `X-CFU-Id`; explicitly registered
test installation IDs can select a pinned candidate response on the existing
URL. Ordinary/missing/unknown IDs keep the existing production release selection.
There is no required old-client rebuild, intermediate old-app update or new
client-visible channel. The maintainer accepts this validation plan; a concrete
external deployment still follows implementation, testing and review.

Stop the first implementation slice with local cohort-routing negative tests,
reviewable pinned artifacts/deployment/disable configuration, and the Linux
installed happy path plus wrong-signature and interrupted-download/retry evidence,
or exact prerequisites recorded. Complete work needed for review before seeking
authority for an external deployment. Public default-feed promotion is separate.

Non-goals: shipping two products, automatic rollback/downgrade, personal profiles,
production extension/store updates, Android migration, public swarms, every old
release, or selecting candidates automatically from mutable GitHub Latest.

## Source Survey And Dependencies

- JSTorrent `tauri-app-v0.2.1`, commit
  `73427b7d3aef2eaf1c4ac1409922fbb52dff751d`: desktop
  `tauri-app/src-tauri/tauri.conf.json` retains the ordinary production endpoint
  and root; `src/lib.rs` sets `X-CFU-Id` for GUI checks and exposes
  `--check-update` / `--auto-update`; `src/headless_updater.rs` sets the same
  header and invokes actual download/install/restart; `desktop/common/src/lib.rs`
  persists the generated ID in `jstorrent-native/cfu-id`.
- `simple-app-update-server/src/{server,channels,github}.ts`: current channel
  selection uses query parameters, the installation ID is currently analytics
  input only, Tauri responses use `no-store`, and release selection comes from
  per-product/channel caches. The opt-in route is now implemented and locally tested in server commit
  `f45885c`; deployment/installed evidence remain separate below.
- Successor `clients/desktop/src-tauri/src/{updater,update_channels}.rs`: legacy
  ID adoption is bounded, header emission follows metrics preference, and update
  responses must confirm a discovered client channel. An internal cohort must
  preserve Stable/Latest semantics rather than invent a Beta channel. Disabling
  statistics must not break normal updating; do not force identifier emission.
- Tactical 251 supplies exact original-key Windows/Linux artifacts from CI
  run 36831643488. All eight available payload signatures and 16 receipt hashes
  independently pass. A failed-only retry now passes both macOS app/DMG
  notarization checks. Fresh complete run 36845370571 passes all five lanes,
  exact release collection and all ten original-root updater payload signatures
  on source `19eb3a88703088a2dfebb803342394938d305cb8`. Use this complete
  same-attempt candidate for the trial; installed migration remains unqualified.
- Reuse 242/243's independently checked released writers, multi-profile fixture
  generation, closed catalog inspection, fencing and source/payload oracles.

## Ownership, Bounds And Invariants

The generic service owns a pure, explicit cohort selector and validated static
candidate input, using existing request/server shutdown ownership. No new engine
task, DTO, client beta toggle or dynamic release-publishing system is needed.
Read the server's own repository instructions before implementation there.

Keep test IDs in private deployment configuration and use generated guest IDs.
The existing analytics ID is a cohort selector, not authentication or a secret.
Bound the first cohort to 32 canonical UUIDs, five supported updater targets and
a 64-KiB candidate manifest. Pin candidate source SHA, version, payload URLs,
sizes/hashes and original-key signatures. Candidate assets need ordinary HTTPS
URLs usable by the old updater; GitHub Actions downloads alone are not that
delivery surface. A public GitHub release is not required for owned test assets.

Validate exact-match selection, no ID/malformed/unknown ID, unsupported target,
equal/newer installed version, unavailable candidate, failed configuration and
cross-cohort requests at the same URL. Candidate requests must not populate
ordinary caches. Keep responses uncached at the proxy too. Trial configuration
defaults disabled; disabling it restores ordinary selection without downgrades.

Use Machine Control's exact claimed guest and explicit desktop grant, with
restoration of inherited state and joined cleanup. Back up/snapshot both product
state and generated payload; an OS snapshot does not cover external folders.
Do not weaken TLS/signature/OS-signing checks. Read-only checks are followed by a
real install inside the guest; do not call a version-response check migration.

## Installed Validation Sequence

1. Install checksum-pinned old release, generate paused legacy records through
   its ordinary writers across two profiles, and record settings/root bindings,
   catalog/source and independent payload hashes. Start with 242/243's valid,
   corrupt, pending-magnet and missing-root cohort; no public network activity.
2. Opt in that guest ID. Run old `--check-update`, then actual `--auto-update`.
   Verify offered version, signature refusal negatives, installed executable
   bytes/OS identity, relaunch and exactly one successor owner. Repeat using the
   old UI's ordinary Check for Updates / Install & Restart interaction.
3. Inspect imported/already-present/skipped outcomes, supported settings, union
   and simple duplicates, paused intent, ordinary full checking, actionable
   unavailable storage, preserved source and unrelated payload. Restart and prove
   one import marker and stable identities. Repair corruption only through normal
   controlled transfer/recheck behavior with expected resulting hashes.
4. Interrupt download, retry and reject a corrupted/wrongly signed payload
   without replacing the old app or damaging state. Exercise running old hosts,
   denied registration and restart around migration's existing transaction
   checkpoints. Keep a recovery baseline with all torrent writers stopped.
5. Add legacy/successor extension pairs and file/magnet/toolbar/tray journeys.
   Qualify Windows and macOS independently; then address production Android/Play
   signing, emulator/physical SAF and staggered ChromeOS stores separately.

## Shipment Boundary

This rehearsal increases confidence in actual delivered behavior; it does not
authorize a rollout. The full checklist retains platform coverage, historical
cohort dispositions, physical/store tests, interrupted installation and recovery,
and final stop/rollback responsibilities. Restore normal test routing and remove
owned guest files/services/claims when each run ends.

## Execution Checkpoint

The maintainer directs end-to-end execution and commits as each slice lands.
Implement and test the server route first, retain exact complete CI candidate
identity, then perform owned installed Linux/Windows/macOS and extension trials.
Any test deployment keeps ordinary user selection and store/default-feed
promotion outside scope. Target identity and consent remain explicit gates.

### Server And Authenticated Candidate Preparation

`simple-app-update-server` commit `f45885c` adds one private 64-KiB trial
manifest with at most 32 canonical installation IDs and five pinned platform
payloads. Exact product/channel/ID selection precedes ordinary cache lookup;
missing/malformed/unknown IDs preserve ordinary selection, selected equal/newer
versions and unsupported targets return 204. Invalid trial configuration leaves
ordinary startup available. Discovery, downloads and other products are unchanged.

Optional local assets use exact configured HTTPS hostname/path pairs, startup
size/hash validation, 64-KiB streaming buffers and disconnect cancellation. There
is no directory listing or upload route. Each asset is bounded to 512 MiB; the
existing five-target limit bounds the total startup scan. Files remain immutable
while enabled. Disabling the manifest and restarting removes selection/delivery.
The installation ID is a selector, not an authentication credential.

Validation: `npm run check` passes lint, TypeScript and all 86 tests, including
ordinary-cache isolation, channel/product/discovery/installer continuity, malformed
IDs/configuration, equal/newer versions, unsupported targets, exact delivery and
HEAD, wrong-host/query/traversal refusal, changed/missing/symlinked assets.
`npm run build` and `git diff --check` pass. No dependency is added.

Complete run 36845370571's downloaded 23 asset receipts and all ten updater
signatures independently pass. `scripts/prepare-jstorrent-upgrade-trial.mjs`
checks the complete receipts again, verifies the five actual delivery payloads
against the original root, and emits a private manifest plus
[candidate receipts](../evidence/jstorrent-upgrade-trial-252-candidate.json).
The generated test IDs stay outside the repository. URLs are prepared for owned
HTTPS delivery; no trial is enabled on the live server at this checkpoint.

Linux and Windows exact x64 test appliances are available through their Linux
controller; the local macOS test appliance is separately claimed. Claim identity,
credentials and concrete machine selectors remain private. Restore initially
powered-off guests to off, and initially suspended macOS to suspended.

### Signed Manual Replacement Checkpoint

Before live deployment, the exact signed 0.3.0 CI packages pass the extended
242/243 installed rehearsal on Linux x64 AppImage, Windows x64 NSIS and macOS
arm64. This uses manual package replacement, not the old updater. The sanitized
[three-platform evidence](../evidence/jstorrent-upgrade-trial-252-manual-migration.json)
records old/new package hashes, candidate source/run and each finished outcome.

Ordinary released writers create four stopped records across two profiles.
Closed imported catalogs, before and after native Quit/restart, preserve DHT/PEX
false, peer limit 73, upload slots 5 and download limit 65,536 bytes/s. Four records
import with zero skipped/already-present; intent remains paused. Intact content
rechecks successfully, corrupt/missing content has zero verified pieces, and the
pending magnet trusts no metadata. Missing roots stay absent. One completion
marker and stable IDs survive restart. Source discovery/KV/main/nonempty WAL and
independent payload hashes remain unchanged. This does not prove repair transfer.

A pre-handshake old host and denied helper registration each stop startup before
catalog creation. Retry after repairing registration succeeds. Eight Windows,
six Linux and nine macOS executable registration routes return the exact legacy
extension refusal. These probes do not qualify real mixed-version extension UI.
The Windows view shows four rows, no Active rows, three Paused and one Needs
attention, with the unavailable root showing Storage unavailable. Native tray
text still says RSTorrent in this candidate: the separate Tauri catalog correction
is committed in `34889e3e`, after the candidate's pinned source. Its focused Rust
test, all four localization checks and formatting pass; a fresh signed candidate
must qualify that correction. The reused Windows guest shortcut shows an old
incubation icon. Independent 7z extraction of the pinned NSIS executable and
Windows `ExtractAssociatedIcon` proves its embedded icon is the JSTorrent blue
box; fresh-install shell-cache behavior remains a presentation qualification.

Initial operator-timeout attempts restore their owned files rather than claiming
success. The final three manual runs pass. Mac phase control uses the common
resident AX route with freshly discovered product Quit/OK controls; Linux Quit
uses the freshly introspected exported status-notifier menu, and Windows uses
fresh tray/UIA discovery. No outer VM input, privacy acknowledgement, network
allowance or public swarm is used. Windows Cancel creates four task-specific
firewall Block rules across the runs; exact-program/baseline-guarded cleanup
removes those four and the original rule-name set matches. File/registry scopes
restore, macOS inherited backup directories are empty and product owners join.

### Prepared Real-Updater Driver And Remaining Gate

The installed drivers accept `--trial-installation-id PATH`, optional
`--trial-gui`, and `--trial-negative wrong-signature|interrupted`. Stage the shared
`legacy_desktop_signed_update.py` with the three existing fixture/rehearsal
helpers. Trial mode performs the startup fences, restores the exact old package,
and parks at `ready-for-signed-update` for the controller's owned `allow-update`
file. It writes the generated guest ID to the isolated old configuration, removes
stale check output and invokes the unchanged released `--check-update` followed
by `--auto-update`, or the ordinary old GUI. Real installation requires the
pinned AppImage/desktop executable bytes, joined updater-spawned owners, the same
catalog/settings/source/payload assertions and native restart. Negative mode
parks at `negative-complete` until owned `allow-retry` after valid routing returns.
These real-updater branches remain unexecuted; their existence is not evidence.

Linux keeps AppImage's supported extract-and-run environment across Tauri restart;
FUSE launch remains a separate lane. Windows preserves both expected per-user
install locations, its production torrent class and both registry views. Cleanup
joins exact installation-owned processes, including updater-spawned children,
before restoring files. A real macOS process smoke proves exact ownership stops
the task child while an adjacent application remains running. Python compilation,
`--help`, malformed-ID/tampered-receipt refusal before manifest output and
`git diff --check` pass. No Rust DTO/schema/engine behavior changes in this slice.

Live deployment remains disabled and awaits explicit push/test-deployment
authorization requested under the repository's no-push rule. Server commit
`f45885c` and the private three-ID manifest are reviewable; existing ordinary
product/channel configuration is retained. Deploy only that tested server commit,
build before restart, configure immutable authenticated assets plus the private
manifest, and verify ordinary/malformed/unknown-ID responses before opening any
`allow-update` gate. After test completion, remove the trial environment/config,
restart and verify ordinary selection. Do not publish a GitHub release, promote
the default feed or modify a store item.

Actual HTTPS old-client installation and GUI restart, signature/interruption
negatives, controlled repair and real extension/file/magnet/toolbar journeys
remain open. These depend on the trial and further owned validation; the
three-platform manual result does not close P-01 or full cutover acceptance.
