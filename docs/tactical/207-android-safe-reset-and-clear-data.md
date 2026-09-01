# Tactical 207: Android Safe Reset And Clear Data

Status: **Implementation complete; qualification remains active as of
2026-09-01.** Atomic reset, both clear outcomes, exact registered-file
deletion, durable recovery/retry/downgrade, mutation exclusion, joined profile
replacement, and the complete Compose surface have landed. Deterministic,
workspace, dual-ABI Android, localization, Compose, and owned API 28/35 service
evidence passes. The controlled multi-root DeleteData campaign, macOS-hosted
generated Swift compile, and separately authorized physical ChromeOS campaign
remain stopping-condition gates, so this tactical does not yet claim complete
qualification.

Topics: `android-jstorrent-replacement`, `client-surfaces`,
`application-control`, `client-persistence`, `download-roots`,
`android-saf-storage`, `localization`, `capability-readiness`

Dependencies: completed typed-settings Tactical
[`180`](180-typed-settings-patches-and-draft-convergence.md), completed
existing-payload and exact-delete Tactical
[`188`](188-existing-payload-adoption-and-recheck.md), completed direct-storage
Tactical [`191`](191-direct-filesystem-storage.md), completed Android/ChromeOS
owner Tactical [`194`](194-chromeos-android-extension-control.md), completed
Android lifecycle Tactical
[`200`](200-android-product-background-lifecycle.md), completed add-time
selection Tactical
[`203`](203-jstorrent-shaped-add-time-file-selection.md), and completed live
DHT/PEX settings Tactical
[`205`](205-durable-dht-and-pex-controls.md).

## Decision And Product Outcome

Implement the two JSTorrent-shaped Android Advanced Settings actions with
three explicit outcomes:

1. **Reset engine settings** restores every global `ClientSettings` field to
   the application configuration's fresh-profile defaults in one typed,
   atomic, revisioned mutation. It does not remove torrents, roots, payload,
   pairing, metrics, Android lifecycle preferences, or appearance.
2. **Clear all data** with **Also delete downloaded files** unchecked removes
   every torrent and all RSTorrent-owned Android profile state while
   preserving every registered torrent payload and exact part artifact.
3. **Clear all data** with **Also delete downloaded files** checked removes
   every torrent and all RSTorrent-owned Android profile state after deleting
   only exact torrent-registered payload files and exact engine-owned part
   artifacts. It never recursively deletes a selected download root.

The clear checkbox is unchecked every time the dialog opens. The dialog names
the exact effects before confirmation and does not describe the operation as
Android **Clear storage**, uninstall, or reinstall. Clearing is one durable,
joined workflow even though it composes the existing per-torrent removal jobs,
Android SAF work, application shutdown, private-profile reset, persisted-grant
release, preference reset, and fresh application restart.

This tactical intentionally keeps reset and clear as different actions.
Automatic disposable-schema reset, migration fallback, package upgrade, root
repair, and ordinary single/multi-torrent removal can never invoke the clear
workflow or select payload deletion.

## Stable Scenarios And Stopping Condition

### ACD-001: Atomic Engine-Settings Reset

Starting from non-default values in every visible and backing-only global
engine setting, one confirmed reset installs the configured fresh-profile
defaults in one durable revision. The current online Android profile therefore
returns to automatic local-network listening, UPnP, unlimited transfer rates,
and the other values supplied by `ClientSettings::fresh_profile_default()`.
Every existing settings runtime owner converges live; a restart reads the same
values. Per-torrent transfer limits, torrent intent, root selection, Android
preferences, theme, dynamic color, pairing, metrics, and payload are unchanged.

### ACD-002: Clear While Keeping Downloaded Files

With the checkbox unchecked, all torrents finish `RemovalDataPolicy::Keep`,
including active, paused, checking, seeding, archived, metadata-only,
awaiting-file-selection, and unavailable-root rows. No payload, part artifact,
or unrelated root entry is opened, moved, adopted, truncated, or deleted. Only
after all torrent removals join does the workflow shut down the application,
clear its private profile, remove every root registration, release retained
SAF grants, reset Android product preferences, and start one empty fresh
profile.

### ACD-003: Clear And Delete Registered Downloaded Files

With the checkbox checked, all targets finish
`RemovalDataPolicy::DeleteData`. Single-file payloads, metainfo-listed
non-padding files under tree payloads, and exact validated part artifacts are
eligible. Missing registered files are already deleted and count as success.
Metainfo-derived directories are pruned deepest-first only when empty.
Unrelated siblings, unrelated nested files, neighboring torrents, selected
root directories, and provider content outside the exact deletion manifest
remain byte-exact.

### ACD-004: Precise Partial Failure

An unavailable grant, unsafe path or object kind, provider refusal, active
handle that cannot join, deletion error, private-profile reset error, grant
release error, or application restart error prevents a success claim. Already
completed torrent removals are not rolled back. The operation retains a
bounded durable phase, completed/remaining counts, and an exact failed-target
summary. The UI offers **Retry** after repair and, for a failed delete-mode
operation only, a second confirmation to **Finish without deleting remaining
files**. That explicit downgrade changes only the retained failed targets to
Keep; it never restores already deleted files and is never automatic.

### ACD-005: Crash And Lifecycle Convergence

Process death before the first removal changes nothing beyond the durable
clear intent. Death during a torrent removal resumes through the existing
durable removal job. Death after a torrent disappeared treats that target as
complete even if the Android journal had not advanced. Death during joined
shutdown, fixed private-profile cleanup, grant release, preference reset, or
fresh restart resumes idempotently from the durable phase. The operation does
not report complete until the fresh application exposes an empty profile with
fresh settings and no registered roots.

### ACD-006: Mutation Exclusion

From confirmation until terminal completion or a surfaced failure, Android
rejects new add, external intake, torrent, settings, root, playback, and
companion mutations with one localized operation-in-progress result. Existing
media capabilities are revoked as their torrents are removed. The ChromeOS
companion listener is stopped before the first destructive mutation and its
stored pairings are deleted only in the final clear phase. A presentation may
detach; the service remains the operation owner.

### ACD-007: Android Truth And Accessibility

Advanced Settings enables **Reset engine settings** and adds **Clear all
data**. Both confirmations are accessible and explicit. Clear progress
survives navigation and activity recreation, includes completed and total
torrent counts without torrent names in notifications, and presents terminal
success or bounded failure. The destructive checkbox is never sticky, the
destructive confirmation cannot be triggered by row activation alone, and no
success snackbar appears before joined completion.

### ACD-008: Platform And Resource Evidence

Pure state, session/application, path storage, fake SAF provider, Compose,
Android 28/35, dual-ABI, controlled-transfer, process-death, and physical
ChromeOS companion/root evidence pass. The clear journal never exceeds 500
torrent targets, 32 roots, or 512 KiB encoded. At most one payload deletion
job is driven at a time. Descriptor, task, journal, notification, and retained
grant high-water marks are recorded.

This tactical stops only when ACD-001 through ACD-008 pass, the exact outcomes
below are implemented and documented, all generated consumers compile, the
full declared repository gates pass, and no operation or test artifact remains
active after completion.

## Exact Outcome Matrix

| State or capability | Reset engine settings | Clear, keep files | Clear and delete files |
| --- | --- | --- | --- |
| Global `ClientSettings`, including backing-only listen-port and HTTPS-auth fields | Fresh configured defaults, atomically | Fresh profile defaults | Fresh profile defaults |
| Per-torrent settings, desired run state, priorities, verification, sources, and metadata | Preserve | Remove | Remove |
| Registered final payload files | Preserve | Preserve byte-exact | Delete exact registered non-padding files |
| Exact engine-owned part artifacts | Preserve | Preserve byte-exact | Delete after validation |
| Unrelated files/directories in selected roots | Preserve | Preserve | Preserve |
| Empty metainfo-derived directories | Preserve | Preserve | May prune only when empty |
| Root registrations/current root | Preserve | Remove after joined torrent work | Remove after joined torrent work |
| Android persisted SAF grants | Preserve | Release after joined torrent work | Release after successful/downgraded joined torrent work |
| Add-options and add-time file-selection preferences | Preserve | Reset to defaults | Reset to defaults |
| Android background, network, sleep, and notification app preferences | Preserve | Reset to current defaults | Reset to current defaults |
| ChromeOS companion enabled preference and stored pairings | Preserve | Disable, disconnect, and clear | Disable, disconnect, and clear |
| Session speed history and application-private metrics | Preserve | Clear | Clear |
| Theme and dynamic-color choice | Preserve | Preserve | Preserve |
| Locale | System-owned; unchanged | System-owned; unchanged | System-owned; unchanged |
| Android notification permission and system channel choices | Unchanged | Unchanged; posted product notifications are canceled | Unchanged; posted product notifications are canceled |
| Application package/version and OS install record | Unchanged | Unchanged | Unchanged |

The Android preference reset values are the current product defaults:
background continuation off, stop after selected background work completes,
unmetered-only off, active-work sleep inhibition on, completion and attention
notifications on, companion hosting off, and the `dataSync` quota fence
cleared. These are application preferences, not Android permission or channel
mutations.

RSTorrent currently has no app-selected locale or analytics installation ID.
The fresh companion-pairing store creates a new backend instance identity; old
extension credentials cannot authenticate after a clear. If either ownership
fact changes before implementation, update this matrix before code rather
than silently inheriting a new store.

## Payload Ownership And Deletion Contract

The deletion scope is the exact scope already established by Tacticals `188`
and `191`:

- derive paths only from validated retained torrent metainfo and the opaque
  torrent identity used for its part artifact;
- delete a single-file torrent's exact payload file, or a tree torrent's exact
  non-padding file paths;
- delete the exact validated part artifact;
- reject symlinks, traversal, ambiguous provider children, wrong object kinds,
  and unsafe parents rather than broadening or recursively cleaning;
- treat an absent exact file as success;
- attempt metainfo-derived directory removal deepest-first and tolerate a
  nonempty directory as preserved unrelated content;
- never enumerate a root to infer more owned content, delete an entire
  selected directory, or treat a matching name as sufficient ownership; and
- retain a failed torrent record and its root authority until deletion
  succeeds or the user explicitly finishes while keeping the remaining files.

Two registered torrents may name an already-removed exact path. Sequential
deletion makes the first successful owner remove it and treats absence for the
later owner as idempotent success. This does not authorize shared live writers;
the ordinary collision and ownership rules remain unchanged.

## Command, Journal, And State Contract

### Engine-settings command

Add one semantic `ResetClientSettings` application command with no caller-
supplied values. `ApplicationConfig` remains the authority for the fresh
profile setting set appropriate to Online, LoopbackOnly, or Offline policy;
the store receives that configured reset target rather than reconstructing
defaults in Kotlin, TypeScript, or an enum switch.

The command validates the target, replaces all `ClientSettings` columns in one
SQLite transaction, advances at most one revision, preserves replay and
expected-revision behavior, and returns the ordinary receipt/snapshot. An
already-default reset is a semantic no-op at the current revision. The
existing application settings reconciler applies the complete new setting set
to listener, mapping, peer budget, upload slots, admission, rate limits,
encryption, IPv6, DHT, PEX, and tracker HTTPS policy without a second reset-
specific runtime path.

### Clear-data coordinator

Do not add a recursive profile-delete engine command or dispatch a burst of
unobserved UI-owned removals. Add one plain, versioned
`ProductDataResetJournal` owned by `ProductEngineService` outside
`filesDir/product-profile`. It captures:

- an opaque operation ID and version;
- Keep or DeleteData policy, including an explicit later downgrade marker;
- the stable ordered initial torrent IDs, bounded by the existing 500-torrent
  hard ceiling;
- the stable root IDs, labels, and tree URIs needed after profile shutdown,
  bounded by the existing 32-root and registry string limits;
- current phase and completed/remaining target IDs;
- bounded per-target failure code and at most 512 UTF-8 bytes of safe detail;
  and
- whether application shutdown, grant release, preference reset, private-
  profile reset, pairing reset, and fresh restart have completed.

Encoding is strict, checksummed or otherwise corruption-detecting, capped at
512 KiB, synchronously committed before the first destructive command, and
fails closed on malformed/future state. It never stores torrent names,
metainfo, magnets, credentials, or payload paths. Root URIs already live in the
bounded SAF registry and are retained only until grant cleanup completes.

The coordinator dispatches at most one existing `RemoveTorrent` command at a
time and observes the authoritative snapshot until that exact target is gone
or its durable removal state is Failed. For DeleteData on SAF, it continues to
use the application-issued exact removal plan and the Android document adapter
confirmation/failure boundary. Keep mode never asks the platform adapter for a
deletion plan.

After every captured target is absent, the coordinator rechecks that the
profile contains no newly admitted torrent, joins the application service and
all media/companion/storage owners, releases only the captured unregistered
SAF grants, resets the known Android product preferences in idempotent phases,
and resets only the fixed application-private `product-profile` root after
canonical containment, non-symlink, and owner-shutdown checks. It then creates
one fresh online Android application, verifies the empty/default snapshot,
cancels obsolete product notifications, publishes success, and removes the
journal.

The implementation may reuse the proven fixed-file crash marker machinery
from `profile_reset` where its invariants fit. It must not call automatic
schema reset as a proxy for explicit user clear, and it must not turn the
entire Android `filesDir`, shared-preferences directory, cache directory, or a
user-selected root into a recursive deletion target.

## Owner, Task, Cancellation, And Dependency Map

```text
Compose Advanced Settings
  -> exact confirmation and non-sticky delete checkbox
  -> ProductEngineService clear/reset entry point

Reset engine settings
  -> typed ResetClientSettings command
  -> SessionStore atomic configured-default replacement
  -> existing ApplicationService settings convergence
  -> generated snapshot/receipt -> Compose truth

Clear all data
  -> ProductEngineService durable ProductDataResetJournal
  -> stop companion listener; gate Android mutations
  -> serial existing RemoveTorrent commands
       -> ApplicationService cancels and joins torrent owners
       -> path exact-delete job, or
       -> AwaitingPlatform -> Android exact SAF plan -> confirm/fail
  -> joined application/client shutdown
  -> fixed private-profile reset + pairing/metrics removal
  -> SAF registry/grant and Android preference phases
  -> fresh AndroidApplicationClient/ApplicationService
  -> empty/default verification -> terminal UI result -> journal removal
```

`ProductEngineService` owns one clear coroutine. Its existing service
cancellation cancels the current wait/adapter call, joins the coroutine, and
leaves the committed journal plus per-torrent removal job recoverable. No
detached cleanup task, Compose-owned coroutine, second engine, or background
daemon is added. The next service start checks the journal before accepting
ordinary intake or starting the companion.

Dependency direction remains Compose -> Android service/platform adapter ->
UniFFI application service -> session/storage -> engine. Journal values and
state transitions remain plain Kotlin data independent of Compose, Intent,
DocumentsContract, and coroutine task handles. Android URI and permission
types never enter Rust protocol, session persistence, or deletion manifests.

## Source-First Record

### RSTorrent baseline

This tactical was accepted against RSTorrent
`c36f3a7c2e5e1adc64a4e3da3942269d4239b62a`.

- `crates/rstorrent-session/src/control.rs` owns typed
  `RemoveTorrent { data: Keep | DeleteData }` and generated application
  commands.
- `crates/rstorrent-session/src/store.rs::begin_removal` owns durable,
  retryable per-torrent removal jobs and current root-reference constraints.
- `crates/rstorrent-session/src/application.rs::{drive_removal,
  platform_removal_plan,confirm_platform_removal,fail_platform_removal}` owns
  task joining, exact path cleanup, SAF handoff, and terminal catalog removal.
- `direct_payload_manifest`, `delete_path_artifacts`,
  `preflight_direct_payload`, and `remove_empty_payload_directory` already
  enforce metainfo-exact deletion and unrelated-content preservation.
- `ProductEngineService.advanceSaf`, `ProductSafDocuments.deleteData`, and
  `ProductSafRootRegistry` own the Android document deletion, 32-root durable
  registry, retained grants, and release boundary.
- `ClientSettings::fresh_profile_default`, `ClientSettingsPatch`, and the
  existing settings reconciler provide the default and live-application
  behavior reset must reuse.
- `ProductLifecyclePreferenceStore`, `ProductNetworkPreference`,
  `ProductPowerPreference`, `ProductNotificationPreferenceStore`,
  `ProductCompanionPreference`, `ProductDataSyncQuotaFence`, and MainActivity's
  `product_ui` store establish the Android preference ownership matrix above.

The concrete boundary improvement is one service-lifetime durable destructive
operation owner. It replaces fire-and-forget multi-removal loops without
moving exact deletion or profile persistence into Compose.

### Current JSTorrent Android

The product reference was re-opened at clean sibling revision
`0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`:

- `android/app/src/main/java/com/jstorrent/app/viewmodel/SettingsViewModel.kt`
  lines 253-263 reset engine and Android stores; lines 291-312 enumerate the
  current engine torrents, issue non-suspending removals, reset stores, remove
  roots, refresh, and dismiss.
- `android/app/src/main/java/com/jstorrent/app/ui/dialogs/ClearAllDataDialog.kt`
  initializes **Also delete downloaded files** to false and passes the chosen
  value only on confirmation.
- `android/quickjs-engine/src/main/kotlin/com/jstorrent/quickjs/storage/AndroidConfigHub.kt::resetToDefaults`
  preserves the default root, clears config keys, and explicitly reapplies
  only a selected engine-default subset.
- `android/app/src/main/java/com/jstorrent/app/settings/SettingsStore.kt::resetToDefaults`
  preserves locale, theme, and notification-prompt history while clearing the
  remaining Android preferences.
- `android/quickjs-engine/src/main/kotlin/com/jstorrent/quickjs/EngineController.kt`
  exposes both non-suspending `removeTorrent` and awaitable
  `removeTorrentAsync`; the clear loop uses the former.
- `packages/engine/src/adapters/native/controller.ts::__jstorrent_cmd_remove`
  awaits engine removal internally, while
  `packages/engine/src/core/bt-engine.ts::removeTorrentWithData` stops network
  work, closes content storage, deletes known payload and part artifacts,
  accumulates errors, and removes session state.
- `SettingsViewModelTest::clearAllData removes all roots and dismisses dialog`
  asserts roots and dialog state but does not establish joined torrent or
  deletion completion, failure aggregation, or crash recovery.

RSTorrent adopts the two actions, unchecked destructive option, exact known-
payload scope, and preservation of unrelated root content. It intentionally
differs by using the Rust application owner, joining each target, retaining a
durable operation, surfacing partial failure, stopping companion mutation,
clearing pairing/metrics on full clear, releasing grants only after torrent
work, and avoiding the inaccurate reinstall claim.

### Pinned libtorrent completeness oracle

The required storage oracle remains libtorrent `2.0.13` at pinned commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`.

- `src/session_impl.cpp::{remove_torrent,remove_torrent_impl}` removes the
  torrent owner, starts optional file deletion, aborts it, and emits a
  deletion-failure alert when cleanup cannot be started.
- `src/torrent.cpp::delete_files` disconnects peers, stops announcing, and
  submits one asynchronous storage deletion before reporting its callback.
- `src/storage_utils.cpp::delete_files` deletes metainfo-listed files, tries
  derived directories in reverse/deepest-first order, and deletes the part
  file for `delete_files`; it does not recursively remove the save path.
- `test/test_remove_torrent.cpp` covers keep/delete, complete, partial,
  mid-download, part-file, and repeated removal cases. Re-open and represent
  `remove_torrent`, `remove_torrent_and_files`,
  `remove_torrent_files_and_partfile`, partial, mid-download, and twice cases
  during implementation.
- `test/test_storage.cpp` removal cases remain the lower-level listed-file and
  storage-error reference.

RSTorrent adopts stop/join before deletion, exact listed-file scope, part-file
cleanup, idempotent absence, empty-parent pruning, active/partial/repeated
cases, and observable failure. It intentionally retains stricter path/type
validation, durable retry state, platform-capability handoff, unrelated-file
preservation evidence, and a joined multi-torrent product workflow. No
reference source, test, fixture, or asset is copied; JSTorrent is MIT and
libtorrent source is BSD-3-Clause under the repository's existing reference
policy.

No BEP changes this local application/destructive-storage behavior, and no
peer protocol support claim changes.

## Edge And Failure Checklist

- Empty profile reset and clear are valid no-ops with truthful confirmation
  and at most one revision/restart.
- All global settings, including fields not rendered in Compose, reset from
  the configured authority rather than a UI-maintained patch list.
- A stale settings receipt or duplicate request preserves ordinary replay and
  revision semantics.
- Active download, upload/seeding, checker, metadata acquisition, queued,
  archived, and pending-file-selection work cancels and joins before removal.
- Clear captures the complete authoritative profile, not only visible,
  filtered, active, or currently materialized Compose rows.
- A torrent added or root mutation attempted after capture is rejected while
  the gate is active; no silently skipped new target is permitted.
- Keep mode succeeds with unavailable or already-revoked roots because it
  does not inspect payload.
- Delete mode with an unavailable root stops at an exact failed target and
  retains enough registration/grant state for repair or explicit keep.
- Missing files and repeated removal are idempotent; unsafe symlinks, wrong
  kinds, ambiguous SAF children, and provider refusal fail closed.
- Single-file, one-entry tree, nested multi-file, cross-file piece, padding,
  skipped file, partial part artifact, complete, oversized exact file, and
  unrelated sibling/nested sentinel layouts are represented.
- Same-root and multiple-root profiles retain every unrelated entry and
  release each distinct grant at most once.
- A nonempty derived directory is preserved without turning the whole target
  into failure after every exact file was removed.
- A deletion failure after some exact files were removed reports partial
  failure and retries idempotently; it never claims rollback.
- Corrupt/future/oversized journal, mismatched operation ID, changed root URI,
  symlinked private profile, unexpected open owner, and failed synchronous
  journal commit stop before broad cleanup.
- Death is injected before/after journal commit, per-target dispatch,
  AwaitingPlatform, SAF confirm, last target, application shutdown, each grant
  release, private-profile reset, preference reset, fresh-client creation,
  verification, and journal removal.
- Notification denial, blocked channels, Home, task removal, service timeout,
  and activity recreation do not detach the operation or create a false
  background-duration claim.
- Clear stops the companion listener before mutation, refuses reconnect during
  the journal, revokes old credentials at finalization, and starts no listener
  automatically afterward.
- Automatic schema/migration reset never creates a clear journal and never
  selects DeleteData.

## Implementation Sequence

1. Land this accepted tactical and reconcile the owning planning records.
2. Add configured-default `ResetClientSettings` through store/application,
   generated JSON/TypeScript/UniFFI boundaries, runtime convergence, and
   focused deterministic tests.
3. Add the pure bounded journal codec/state machine, operation gate, startup
   recovery, and preference/profile outcome helpers without exposing UI.
4. Implement clear-Keep over serial existing removal jobs, joined shutdown,
   exact fixed private-profile reset, SAF registry/grant cleanup, preference
   reset, fresh application creation, and empty/default verification.
5. Add clear-DeleteData through the existing path/SAF exact deletion plans,
   precise failure aggregation, repair/retry, and the explicit keep-remaining
   downgrade.
6. Enable/add the Advanced Settings rows, exact dialogs, progress/failure
   presentation, lifecycle notification, accessibility semantics, and English
   catalog entries. Regenerate boundary artifacts.
7. Run deterministic, fake-provider, controlled-transfer, process-death,
   Android build/AVD, companion/package, and full repository gates. Record
   high-water marks and remove every fixture/profile/root artifact.
8. With separate authorization for destructive physical-device interaction,
   run the exact disposable-root ChromeOS campaign, restore the testbed, and
   reconcile actual evidence in this tactical and the owning topics.

Each stage keeps the operation diagnosable. Clear-DeleteData does not become
visible before Keep, recovery, exact-deletion failure, and unrelated-sentinel
tests pass.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure Kotlin | strict journal encode/decode; 500 targets/32 roots/512-KiB bounds; corruption/future rejection; every phase transition; retry and explicit downgrade; crash idempotence; mutation gate |
| Session/store | configured-default reset atomicity, no-op/replay/stale revision, every field including hidden values, restart durability, preservation of torrents/per-torrent settings/roots |
| Application/runtime | listener/mapping/limits/encryption/IPv6/DHT/PEX convergence; serial active/checking/seeding/pending/archived removal; joined tasks; empty fresh snapshot; no residual view/media owner |
| Path storage | keep/delete across single/tree/partial/complete/skipped/padding/oversized/repeated cases; exact part file; unrelated sibling/nested and neighboring-torrent preservation; symlink/wrong-kind/failure retry |
| Android SAF unit/fake provider | exact plans, ambiguous child/provider refusal, unavailable/repaired grant, deepest empty-directory cleanup, unrelated sentinels, at-most-once grant release, process death at plan/confirm/finalization |
| Compose/JVM | non-sticky unchecked box, exact copy, confirmation/focus/back/accessibility, progress recreation, partial-failure retry/downgrade, preserved appearance/permission truth, reset/clear outcomes |
| Controlled integration | locally generated multi-torrent fixture across two roots; active download and seed; Keep then re-add/recheck; DeleteData exact absence and hash/sentinel agreement; no public network |
| Android platform | locked x86_64 and arm64 Rust/UniFFI builds, JVM tests, debug APK, API 28 and API 35 connected profiles, process kill/restart, notification denied/granted, lifecycle/quota interruption, exact cleanup |
| ChromeOS | disposable two-root Compose plus extension profile; clear in both modes; listener refusal during/after clear; old pairing rejection; retained unrelated files; fresh repair/re-pair; descriptor/task/grant cleanup |
| Generated clients | `npm run generate --prefix clients/web`, generated diff review, web typecheck/tests, Android boundary compile, and macOS-hosted iOS simulator/archive compile because `Command` changes |
| Repository | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, `git diff --check`, stale unavailable-row search, and clean status |

## Implementation And Current Evidence

Implementation landed incrementally in commits `aa31e5f1`, `08f64509`,
`fb39eb91`, `91aa6db4`, `9b1c4171`, and `05872bdd`:

- `ResetClientSettings` carries no caller values, replaces the complete
  configured setting set in one transaction, and retains ordinary no-op,
  replay, expected-revision, restart, and live-runtime convergence semantics.
- `ProductDataResetJournal` is versioned, checksummed, synchronously committed,
  and fail-closed. The exact 500-torrent/32-root fixture is 27,135 encoded ASCII
  bytes under the 512-KiB ceiling. It retains one ordered cursor, one current-
  torrent retry marker, bounded safe failure detail, and no torrent names,
  magnets, credentials, metainfo, or payload paths.
- `ProductEngineService` checks recovery before ordinary application startup,
  gates all non-owner mutations, stops companion admission, drives one existing
  removal job at a time, joins the application/storage/media/presentation
  owners, resets only `filesDir/product-profile`, releases only captured grants,
  resets the enumerated Android product preferences, and verifies one empty
  fresh application before removing the journal or publishing success.
- DeleteData continues through the existing metainfo-exact path/SAF plan and
  confirmation boundary. Provider or grant failure becomes durable Failed
  state; Retry reissues only a failed target, while the second confirmed
  downgrade retries that target and all remaining targets with Keep. Recovery
  clears a retry marker if the target already disappeared or resumes an
  in-flight removal without duplicating it.
- Advanced Settings now exposes both enabled actions, resets the delete checkbox
  on every dialog open, names the exact keep/delete scope, presents count-only
  service-owned progress through navigation/recreation, and exposes bounded
  failure, Retry, and separately confirmed keep-remaining recovery. Appearance,
  Android permission/channel choices, and unrelated private files remain
  outside the clear owner.

Current evidence on the Linux host:

- focused Rust store/application reset tests pass in the full workspace run
  (the session crate reports 327 passed and 2 ignored); the full workspace
  suite, `cargo fmt --all -- --check`, and warning-denied workspace Clippy pass;
- `clients/android/build.sh` rebuilt locked x86_64 and arm64-v8a Rust/UniFFI
  libraries and the debug client; `testDebugUnitTest`, `lintDebug`, and
  `assembleDebugAndroidTest` pass;
- the pure Android corpus covers exact SAF deletion, unrelated-content
  preservation, provider refusal, wrong-kind preflight, journal corruption,
  future version, hard bounds, retry marker, fixed-profile containment, and
  symlink rejection;
- all 20 `ProductNavigationTest` cases pass on fresh owned API 28 and API 35
  AVDs. Both `ProductDataResetInstrumentationTest` cases also pass on each API:
  reset preserves profile/root/Android/appearance state, clear-Keep produces a
  fresh empty application, and startup resumes a persisted `RESETTING_PROFILE`
  journal while preserving unrelated private and appearance sentinels. Every
  task-owned AVD was deleted after its run;
- `node scripts/check-localization.mjs` passes all 421 Android resources plus
  the existing cross-product catalogs; generated web output is clean, web
  typecheck passes, and Vitest reports 377 passed with 2 skipped; and
- the coordinator structurally owns one reset coroutine, one serial removal,
  the existing single foreground notification, at most 32 captured grants,
  and no detached task or second application owner. Both connected-service
  cases reach observable joined shutdown during cleanup.

Qualification remains open for the controlled two-root active-download/seed
Keep-and-re-add then DeleteData integration, genuine process-kill windows and
notification/lifecycle interruption around destructive work, numeric process
descriptor high water for that workload, the macOS-only generated Swift
simulator/archive compile, and Tactical step 8's physical ChromeOS two-root and
companion campaign. The physical gate still requires separate authorization;
no attached phone, Chromebook, or user root was touched by this implementation
run.

Public swarms, release signing, Play upload, production package identity,
production extension publication, and real user-root deletion are not required
or authorized. Controlled fixture data and testbed grants are removed after
each platform run. The physical ChromeOS run is a stopping-condition gate but
requires a separately authorized device session before execution.

## Non-Goals And Next Boundary

- Do not recursively delete selected download folders, arbitrary siblings,
  browser downloads, media libraries, or any path inferred outside a retained
  torrent's exact deletion manifest.
- Do not add Android `pm clear`, uninstall/reinstall, backup-manager, package-
  data, cache sweeping, or OS permission/channel reset behavior.
- Do not add Reset/Clear presentation to React, Tauri, desktop, headless, or
  iOS in this Android parity slice. The typed engine-settings reset command may
  compile through their generated boundaries without a visible control.
- Do not import JSTorrent state, couple explicit clear to `JAR-004`, or make
  explicit payload deletion available to automatic migration/disposable-state
  reset.
- Do not change ordinary per-torrent removal wording, deletion scope,
  collision ownership, adoption, verification, or root-repair behavior except
  for bugs directly exposed at this joined-operation boundary.
- Do not add a second engine/service, remote daemon, broad filesystem API,
  recursive cleanup helper, dependency, protocol behavior, or public support
  claim.
- Do not choose low-battery, VPN, SOCKS5, search/plugin, translation, manifest,
  or manual-memory dispositions.

After completion, `android-jstorrent-replacement` may mark reset and clear-data
parity implemented. Production identity/migration `JAR-004`, extension rollout
`JAR-005`, the authorized physical unmetered handoff in Tactical `199`, and
the remaining Settings Follow-Up Queue stay independently owned.

## Escalation Contract

Implementation may perform ordinary in-scope refactoring, add the typed
command and generated artifacts, introduce the bounded private journal, update
strict local formats under the incubation compatibility policy, run controlled
local fixtures and AVDs, and fix directly exposed exact-removal/restart bugs.

Stop for direction before changing the outcome matrix, preserving old pairing
or metrics instead of clearing them, resetting appearance, deleting anything
beyond exact registered torrent artifacts and fixed private profile files,
adding a dependency, using clear as migration behavior, publishing/signing,
using public network resources, or touching a physical device or non-disposable
user root. If actual Android ownership makes the selected fixed private-profile
reset unsafe, stop rather than broadening deletion to `filesDir` or Android app
data.
