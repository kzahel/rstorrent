# Tactical 123: iOS On-Device Root Persistence And Recovery

Status: Complete on 2026-08-10 with the accepted app-owned-only outcome.
Maintainer review accepts an on-device-only payload-root policy for the first
eventual iOS implementation. The physical app-owned matrix passes; picker-root
registration is compiled off because the required local/iCloud controls could
not be exercised on the authorized device. This does not authorize or claim a
complete iOS client.

Topics: `product-direction`, `product-surfaces-and-migration`,
`client-surfaces`, `download-roots`, `client-persistence`,
`storage-throughput-architecture`, `capability-readiness`

Dependencies: completed Tactical
[`116`](116-platform-storage-coherence-and-ios-feasibility.md) owns the common
logical-file seam and the first physical-iPhone storage, networking, bookmark,
coordination, and lifecycle evidence. Completed Tacticals
[`061`](061-user-selected-download-roots.md),
[`073`](073-unified-storage-and-complete-recheck.md),
[`105`](105-fact-based-persistence-and-recheck-containment.md), and
[`114`](114-session-wide-concurrent-torrent-admission.md) own stable root
identity, publication/recovery, conservative persisted facts, and session-wide
storage ownership. This slice must preserve those contracts rather than create
an iOS-only torrent runtime.

## Decision And Desired Outcome

The first eventual iOS client will use only payload storage known to remain on
the device. Its unconditional baseline is an app-owned, user-visible Documents
root. A directory selected from **On My iPhone** may join that initial support
set only if this tactical proves that public Apple APIs can positively and
repeatably distinguish the selected local root from iCloud Drive and other
File Provider locations. Ambiguous classification fails closed to the
app-owned root.

iCloud Drive and third-party File Provider locations are not active torrent
roots in this slice. Torrent payload I/O needs sustained seekable positional
reads and writes, truncation, hashing, durability, namespace changes, and
predictable availability. A provider may materialize or evict files, block on
coordination, upload every mutation, impose quotas, or become unavailable.
Those semantics are a poor default for the engine's hot path even when a
single small probe happens to work.

The external-provider case is therefore a negative control, not a support
target. The probe may select one empty iCloud Drive test directory only to
prove rejection before bookmark persistence, root registration, or payload
creation. A later product feature may export or copy completed verified files
to a user-selected cloud destination without making that destination the
active torrent root. Cloud-backed downloading, seeding, relocation, and
source deletion remain separate decisions.

This tactical advances the existing repository-owned feasibility harness. It
settles the root eligibility, access ordering, persistence, repair, and
physical recovery contract needed before a complete iOS product tactical can
be written. It does not create `clients/ios`, select a UI toolkit or binding
generator, or duplicate application-service persistence in Swift.

## Accepted Product Policy

The implementation and evidence must preserve this ordered policy:

1. **App-owned Documents is supported.** Resolve it afresh from
   `FileManager` on each launch. Persist a stable opaque root ID and generation,
   not its container path and not a bookmark that pretends the container is an
   external capability.
2. **A system-selected local directory is conditional.** It is eligible only
   after the same build observes a positively local result on a physical
   device and a distinct unsupported result for the external-provider negative
   control using documented public APIs.
3. **iCloud, third-party providers, and unknown results are unsupported.** The
   picker result is rejected before durable registration or any Rust payload
   operation. A provider display name, visible path component, private API,
   hard-coded container path, or installed-provider allowlist is not acceptable
   evidence of locality.
4. **No silent fallback exists.** A lost bookmark, revoked Files and Folders
   permission, failed coordination, changed classification, or missing root
   leaves that stable root unavailable. It never redirects an existing torrent
   or probe record to app-owned Documents.
5. **Cloud export is a different authority transition.** A later completed-file
   export may copy a verified published artifact through a bounded coordinated
   operation. A true move additionally needs joined torrent ownership,
   cross-provider copy verification, crash recovery, authority replacement,
   rollback, and source deletion only after success.

The first complete iOS client tactical may therefore assume app-owned
Documents and, only if graduated here, a selected **On My iPhone** root. It may
not infer general File Provider support from either case.

## Current Evidence And Gap

Tactical `116` already proves on an iPhone SE (3rd generation) running iOS
26.6 that the real Rust `StorageFilePool`, positional file operations, SHA-1,
namespace operations, direct TCP/UDP, ordinary background expiration, finite
continued processing, and force-close recovery can run in-process. It also
proves non-stale bookmark restoration, balanced security-scope access, and
`NSFileCoordinator` around an app-owned `PickerRoot` fixture.
The probe's `Info.plist` already enables file sharing and opening documents in
place, so its app-owned Documents directory is exposed as user-visible local
storage rather than an invisible cache or temporary directory.

That evidence does not prove a distinct directory selected from **On My
iPhone**. Two attempts to automate the system document picker did not reach
the directory selection, and the completed tactical correctly refused to
treat its app-owned fixture as external-provider evidence. Manual interaction
with the system picker is acceptable for this physical feasibility slice; UI
automation is not the behavior under test.

The current probe also has two ordering gaps that are harmless for its
app-owned fixture but invalid for a picker-returned security-scoped URL:

- `selectedFolder(_:)` validates and creates a bookmark before calling
  `startAccessingSecurityScopedResource()`; and
- bookmark restoration refreshes stale bookmark data before beginning the
  balanced security-scoped access window.

Apple's directory-access contract requires scope acquisition before accessing
the selected URL, coordinated file operations while access is active, and a
minimal bookmark for later restoration. The implementation must correct that
ordering before it records picker evidence.

## Stopping Condition

This tactical is complete only when all of the following hold:

1. A task-free, bounded root-eligibility observation and decision distinguish
   `app_owned`, `selected_on_device`, `unsupported_provider`, and
   `unclassifiable` without persisting a raw URL, path, provider display name,
   File Provider item identifier, or domain identifier in portable evidence.
2. Deterministic tests prove the decision is fail-closed. A true
   `isUbiquitousItem`, false `volumeIsLocal`, missing required value,
   conflicting values, provider lookup failure, timeout, or unknown future
   value can never produce `selected_on_device`. A true `volumeIsLocal` alone
   is not sufficient because a provider's materialized representation may
   reside on local storage.
3. The app-owned Documents root passes initial access, nested positional Rust
   I/O, SHA-1, sync, no-replace rename, observation, cleanup, ordinary
   termination/relaunch, controlled force-close/relaunch, and repeated-run
   cleanup on the physical iPhone.
4. The system picker is exercised manually on the physical iPhone for one
   dedicated empty **On My iPhone** directory. Scope begins before directory
   validation or eligibility observation, the bounded classification is
   recorded, and scope is balanced afterwards. An unclassifiable selection is
   neither bookmarked nor written.
5. If the selected local directory is eligible, it passes bookmark relaunch,
   stale-bookmark handling, permission revoke/restore, explicit repair,
   generation advancement, coordinated Rust storage, force-close recovery,
   and cleanup without a path fallback or root-ID change. Its bookmark uses
   `.minimalBookmark`, and every Rust descriptor lease closes inside the
   coordinated accessor before scope is balanced.
6. The same build selects one dedicated empty iCloud Drive directory as a
   negative control when iCloud Drive is available and separately authorized.
   It records a bounded classification result and performs no bookmark
   persistence, root registration, directory creation, file creation, Rust
   storage call, or provider mutation for that selection.
7. Selected-local support graduates only if the physical local and external
   controls are distinguished by documented public observations and every
   local persistence/recovery case passes. If iCloud is unavailable, public
   signals are ambiguous, or any classifier rule needs a name/path/private-API
   heuristic, the picker-backed root is disabled and the tactical closes with
   app-owned Documents as the sole supported result.
8. Permission loss, bookmark failure, coordination failure, timeout,
   cancellation, background expiration, and process death leave no leaked
   security scope, coordinator work, Rust task, descriptor lease, temporary
   file, optimistic root registration, or false success evidence.
9. The tactical execution record and owning topics state the exact supported
   result, negative evidence, commands, device class/OS, resource high-water
   marks, cleanup, and next client boundary. They do not claim an iOS product
   or general File Provider support.

The app-owned-only outcome is an accepted successful result when the
classification gate cannot safely graduate a picker-backed local root. A
working write into iCloud is not an accepted reason to broaden the policy.

## Scope

### Probe-local root record

Add the smallest versioned probe record needed to exercise persistence and
repair. It deliberately mirrors the durable split the product will later use
without changing the application-service schema:

```text
ProbeRootRecord
  schema_version
  stable_root_id
  kind = app_owned | selected_on_device
  generation
  bounded_display_label
  bookmark_data?          # selected_on_device only
  last_eligibility_class
```

The app-owned locator is recomputed from the current container on launch.
Selected-root bookmark bytes stay in the platform-owned probe store and never
enter JSON evidence or Rust. The stable root ID has no derivation from a path,
bookmark, provider ID, device identifier, or display label. Explicit repair
updates the bookmark behind that ID and advances the generation.

Do not persist a picker selection until locality, directory kind, scope,
bookmark creation, and the initial coordinated no-payload validation have all
succeeded. Do not retain rejected external-provider metadata after the bounded
redacted evidence is written.

This record is feasibility evidence, not permission to create a second Swift
torrent catalog or to move the eventual product's root authority out of
`rstorrent-session`.

### Eligibility observation

Before implementation, inspect the active iPhoneOS SDK headers and current
Apple documentation for the exact availability and meaning of:

- `URLResourceValues.isUbiquitousItem`;
- `volumeIsLocal` and `volumeIsInternal`;
- `NSFileProviderManager.getIdentifierForUserVisibleFile`; and
- any documented public value that can positively identify the system's local
  provider rather than merely identify a File Provider domain.

Encode only the bounded facts needed for the pure decision. Optional resource
values remain optional; absence is not converted to `false`. A File Provider
item/domain identifier establishes identity within the provider system, not
locality by itself. Raw identifiers may be compared ephemerally during the
physical run but must be redacted from retained evidence.

The decision function accepts the app-container case by capability provenance,
not by arbitrary string-prefix comparison. It may accept a selected local root
only when a documented positive signal survives selection, bookmark restore,
and repair and is distinct from the iCloud negative control. If no such signal
exists, remove or disable the selected-root registration action after the
investigation and retain the typed `unclassifiable` result.

### Correct access lifetime

For every picker-returned or restored URL, the lifecycle is:

```text
resolve picker URL or bookmark
  -> start security-scoped access
  -> collect eligibility and directory observations
  -> reject, or create/refresh .minimalBookmark
  -> coordinate the exact read/write operation
  -> use the URL supplied to the coordinator accessor
  -> invoke one synchronous bounded Rust operation
  -> close every Rust lease and join the operation
  -> finish coordination
  -> stop security-scoped access
```

Bookmark resolution itself may precede scope acquisition; reading resource
values, validating the directory, refreshing bookmark data, or invoking Rust
may not. Every successful `startAccessingSecurityScopedResource()` has exactly
one matching stop. A false return is a typed unavailable/permission result and
is never treated as evidence that access was unnecessary for a picker URL.

`NSFileCoordinator` is per operation, not a process-long lock. The Swift owner
must call Rust synchronously inside the accessor because using a descriptor or
coordinated URL after the accessor returns would outlive the proven window.
Cancellation or expiration stops new admission and requests Rust cancellation;
the operation joins and releases its leases before coordination and scope end
when the operating system permits. Force-close is unjoined process death and
is evaluated from persisted facts on relaunch.

### Persistence and recovery matrix

The app-owned root always runs these common cases. A selected local root runs
them only after the eligibility gate passes:

- first launch and exact initial Rust storage/cleanup;
- ordinary termination and relaunch;
- force-close during a controlled phase, followed by relaunch and conservative
  namespace reconciliation;
- repeated open/run/close cycles with stable root ID and no increasing scope,
  coordinator, task, handle, or file count;
- cleanup after success, failure, cancellation, expiration, and repair.

The selected local root additionally runs:

- Files and Folders permission revocation, typed unavailable state, permission
  restoration, and explicit retry;
- stale bookmark refresh under scope, or a typed repair-needed result when the
  platform cannot restore it;
- explicit re-selection/repair behind the same root ID with a new generation;
- selected directory rename or move when safe to perform, yielding either
  continued bookmark identity or typed repair-needed state, never raw-path
  rebinding.

The probe need not manufacture an actually stale bookmark if the platform
cannot do so safely. It must still test the pure stale state and record the
physical move/rename outcome rather than claiming a stale transition that was
not observed.

### External-provider negative control

Use one empty test directory owned by the maintainer in iCloud Drive when that
service is configured and physical interaction is authorized. Selection may
acquire scope only long enough to collect the documented classification facts
and demonstrate rejection. It must not create a bookmark or touch directory
contents.

If obtaining a trustworthy classification would require materializing a file,
writing a marker, enumerating unrelated content, using account-specific data,
or probing a third-party provider, stop and retain the app-owned-only result.
No cloud account setup, provider installation, or provider-data cleanup is
implied by this tactical.

## Persisted And Runtime Facts

The durable split is:

- the probe store owns the versioned root record, opaque root ID, generation,
  kind, bounded label, and selected-local bookmark;
- Swift owns URL resolution, eligibility observation, security-scope and
  coordination lifetime, permission/repair state, and the mapping from the
  probe root ID to the current capability;
- Rust owns logical artifact geometry, file handles, payload buffers,
  positional I/O, hashing, sync, and namespace checks; and
- live URL values, resource observations, File Provider identifiers,
  coordinator instances, scope counts, descriptors, and tasks are runtime
  facts fenced by the root generation.

No raw URL or path is a durable fallback. No bookmark, provider identifier,
domain identifier, device identifier, or directory content appears in retained
JSON evidence. A bounded one-way digest may correlate the same test selection
within one run only if needed; it is discarded during cleanup and is not a
portable root identity.

## Owner, Task, Cancellation, And Dependency Map

```text
probe application generation
  -> probe-local root registry
  -> Swift root capability owner
       -> picker / bookmark resolver
       -> eligibility observer
       -> security-scope lease
       -> per-operation NSFileCoordinator
            -> synchronous Rust probe call
                 -> StorageFilePool
                 -> positional I/O / SHA-1 / namespace operation
  -> bounded evidence writer
```

- Pure eligibility and recovery decisions depend on bounded values, not UIKit,
  Foundation URLs, File Provider types, tasks, descriptors, or Rust handles.
- UIKit and Foundation remain in the outer Swift adapter. The Rust probe never
  sees bookmark bytes, provider identifiers, or a security-scope token.
- Rust storage continues to use the real engine types from Tactical `116`; no
  Swift payload callback, `Data` block transport, second file cache, or provider
  emulation is permitted.
- The application generation owns every asynchronous classification and probe
  task. Dismissal, replacement, expiration, and shutdown cancel admission and
  join or generation-fence late completions.
- Evidence writes occur after a terminal decision and contain no capability.

The dependency direction remains:

```text
pure eligibility and recovery values
                 ^
Swift capability/lifecycle adapter
                 ^
repository-owned iOS probe UI

Rust protocol/domain <- Rust engine storage <- narrow probe FFI
```

The probe composes these two inward boundaries for one operation; neither
boundary imports the other's platform or payload types.

## Resource Bounds

| Resource | Initial bound |
| --- | ---: |
| Persisted probe roots | 2 |
| Selected-local bookmarks | 1 |
| Bookmark bytes | 64 KiB |
| Concurrent eligibility requests | 1 |
| Eligibility request deadline | 5 seconds |
| Concurrent security scopes | 1 |
| Concurrent coordinators / Rust operations | 1 / 1 |
| Rust file leases | 8 |
| Probe-created files | 64 |
| Bounded display label | 256 UTF-8 bytes |
| Error or evidence detail | 1,024 UTF-8 bytes per field |
| Retained provider URLs, item IDs, domain IDs | 0 |

The app-owned record plus one selected-local candidate accounts for the
two-root probe bound. Rejected provider selections do not consume a root slot.
Record scope, coordinator, task, Rust-handle, process-descriptor, file, and
pending-operation high-water marks. Tightening these limits is in scope;
raising them or adding multi-root product behavior is not.

## Shape-Changing Edge Cases

These cases land with the common path:

- picker cancellation, multiple returned URLs despite single-selection policy,
  non-file URL, wrong object kind, symlink/alias ambiguity, and a selected root
  nested inside another retained root;
- scope acquisition failure before validation, bookmark creation failure,
  stale resolution, bookmark resolving to a different eligibility class, and
  permission revoke/restore between observation and coordination;
- optional `URLResourceValues` returning `nil`, mutually inconsistent local
  and ubiquitous values, File Provider lookup failure/timeout, and values that
  change after relaunch;
- coordinator failure or accessor not running, the accessor returning a
  different coordinated URL, cancellation before/inside/after the accessor,
  and a late asynchronous classifier completion from an old generation;
- app background expiration, suspension, memory termination, and force-close
  without an expiration callback;
- process death before/after root-record commit, bookmark refresh, file sync,
  no-replace rename, cleanup, and evidence write;
- selected-root rename/move, root deletion/recreation at a similar display
  path, and explicit repair to a different directory; and
- an external provider that reports locally materialized bytes,
  `volumeIsLocal == true`, or non-ubiquitous state without a documented positive
  system-local
  identity.

No edge case may make an unsupported root available, create payload as a
classification probe, replace a stable root implicitly, or preserve a stale
generation's success.

## Reference Dossier

### Apple platform contract

Reconfirm these primary references immediately before implementation because
the selected SDK may differ from the planning SDK:

- [Providing access to directories](https://developer.apple.com/documentation/uikit/providing-access-to-directories)
  defines system picker access to local, iCloud, and third-party providers;
  security-scope ordering; file coordination; minimal bookmarks; and user
  permission revocation.
- [`UIDocumentPickerViewController`](https://developer.apple.com/documentation/uikit/uidocumentpickerviewcontroller)
  owns user selection and returns the security-scoped directory URL.
- [`NSFileCoordinator`](https://developer.apple.com/documentation/foundation/nsfilecoordinator)
  owns coordinated read/write access and requires use of the URL passed to its
  accessor.
- [`URLResourceValues`](https://developer.apple.com/documentation/foundation/urlresourcevalues)
  defines optional ubiquitous and volume facts; unavailable resource values
  remain `nil`.
- [`NSFileProviderManager`](https://developer.apple.com/documentation/fileprovider/nsfileprovidermanager)
  exposes user-visible item and domain identifiers but does not by itself
  document a generic local-versus-remote classification.
- [Synchronizing the File Provider extension](https://developer.apple.com/documentation/fileprovider/synchronizing-the-file-provider-extension)
  distinguishes dataless and materialized provider items and prevents treating
  one successful local POSIX access as proof of durable local storage.

The planning machine's iPhoneOS 26.5 SDK headers make
`NSURLBookmarkCreationWithSecurityScope` unavailable on iOS while exposing
`NSURLBookmarkCreationMinimalBookmark`; follow the iOS directory-access
guidance rather than copying the macOS bookmark option. The same headers expose
optional `NSURLVolumeIsLocalKey`, `NSURLVolumeIsInternalKey`, and
`NSURLIsUbiquitousItemKey`, plus File Provider item/domain lookup. None is
silently promoted from an observation into proof of an arbitrary provider's
performance or durability.

### RSTorrent starting point

The planning survey used RSTorrent commit
`5213edf44a1de32f4226012e9753495532cef44d`. Implementation refreshed the same
sources at starting commit
`df038d922f946df2298d126ef0ee1b0b70ea3ecd` and used these exact sources:

- `experiments/ios-storage-probe/App/ProbeView.swift` for the real folder
  picker;
- `experiments/ios-storage-probe/App/ProbeModel.swift` for app-owned and
  bookmark/coordinator lifecycle;
- `experiments/ios-storage-probe/src/lib.rs` for the real Rust file pool,
  positional I/O, SHA-1, sync, rename, truncate, and cleanup operation;
- `crates/rstorrent-engine/src/storage_file_pool.rs` for common platform
  storage requests, failures, observations, root failure, and handle bounds;
  and
- Tactical `116` plus `download-roots.md`, `client-persistence.md`, and
  `storage-throughput-architecture.md` for the accepted capability ownership
  and the limits of existing physical evidence.

Rebase the survey on the implementation starting commit and record changed
symbols before editing. Preserve the real Rust seam instead of replacing it
with a Swift-only filesystem demonstration.

### JSTorrent product history

The planning survey used sibling JSTorrent commit
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `ios/JSTorrent/App/SettingsScreen.swift` opens a folder picker with
  `asCopy: false`;
- `ios/JSTorrent/App/AppSettings.swift` stores root keys, labels, paths,
  bookmarks, an internal flag, and a default, then holds restored security
  scopes for the process lifetime;
- `ios/JSTorrentKit/Sources/JSTorrentKit/Bindings/FileBindings.swift` uses a
  bounded descriptor pool and `pread`/`pwrite`; and
- `android/app/src/main/java/com/jstorrent/app/AddRootActivity.kt` explicitly
  accepts only Android's local external-storage and Downloads providers while
  rejecting Drive, Dropbox, OneDrive, and Box because reliable random-access
  writes are required. `RootStoreTest.kt` locks in that allowlist behavior.

JSTorrent supplies product history, not iOS feasibility evidence. Its current
iOS code does not use `NSFileCoordinator`, validates and bookmarks a new
selection before scope acquisition, validates a restored URL before acquiring
scope, retains an optimistic path-backed `ContentRoot` after restore failure,
and has no recorded external-provider physical matrix. Do not copy those
failure behaviors or treat the presence of picker code as a support claim.

The lesson adopted here is the explicit local-only Android policy and bounded
positional hot path. The Android authority allowlist is not portable to iOS,
where public provider classification must be independently proven or rejected.

## Staged Implementation And Intermediate Gates

1. **Freeze and refresh the sources.** Record the RSTorrent and JSTorrent
   commits, active iPhoneOS SDK, exact Apple API availability, existing probe
   tests, and prior physical evidence. Add deterministic fixtures for optional,
   contradictory, external, and unknown eligibility observations before
   changing picker behavior.
2. **Define the pure decision.** Add bounded eligibility and recovery values,
   fail-closed classification, root-generation fencing, record validation, and
   redacted evidence encoding. Prove that no single convenience value such as
   `volumeIsLocal` can admit a provider.
3. **Correct access ordering.** Refactor the probe so a picker or bookmark URL
   begins scope before validation/resource access/bookmark creation, uses
   `.minimalBookmark`, coordinates each exact operation, calls Rust
   synchronously with the accessor URL, closes leases, and balances scope on
   every outcome.
4. **Implement app-owned persistence and recovery.** Add the versioned
   probe-local root record, stable ID/generation behavior, controlled crash
   phases, namespace reconciliation, repeated-run accounting, and cleanup for
   app-owned Documents. Pass host and simulator build gates before using the
   device.
5. **Run the manual physical classification gate.** On the authorized iPhone,
   select one empty **On My iPhone** directory and, when available and
   authorized, one empty iCloud Drive directory. Retain only redacted bounded
   observations. Decide whether selected-local classification graduates or is
   disabled; a negative result is not repaired with a heuristic.
6. **Run the supported-root physical matrix.** Always run the full app-owned
   matrix. If and only if Stage 5 graduates selected local, run bookmark,
   relaunch, permission, repair, rename/move, coordination, Rust I/O,
   force-close, and cleanup cases for it.
7. **Reconcile repository truth.** Record commands, results, failures, limits,
   cleanup, and the final root policy. Update owning topics and readiness
   without adding an iOS product support claim. Remove probe data, temporary
   evidence, build artifacts, installed test app, and controlled cloud test
   directory when safe and authorized.

Stages 1--4 are required even if iCloud is unavailable. Stage 5 cannot
graduate selected-local support without both a positive local observation and
a negative external control. Stage 6 never performs payload I/O against the
external control.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure Swift state | Eligibility truth table, nil/conflicting values, record validation, generation fencing, stale bookmark, repair, late completion, redaction, size bounds |
| Rust host | Existing storage probe tests plus exact handle/file cleanup; no change to payload-buffer ownership |
| Simulator/build | Both required Rust Apple targets, generated Xcode project, host tests, simulator app build and non-picker app-owned smoke |
| Physical app-owned | Initial run, Rust storage/SHA-1/namespace operation, termination/relaunch, controlled force-close/recovery, expiration, repeated cleanup, exact high-water marks |
| Physical selected local | Manual picker, positive eligibility, scope-before-access, minimal bookmark, coordinated accessor URL, relaunch, permission revoke/restore, repair, rename/move, cleanup; required only to graduate selected-local support |
| Physical external negative | Manual empty iCloud selection, unsupported/unclassifiable result, zero bookmark/root/payload/provider mutation; required only to graduate the picker path |
| Repository | Focused tests, proportional Rust baseline, Swift formatting/build checks available in the project, `git diff --check`, topic links, exact execution record |

A simulator does not substitute for either physical classification control.
The external negative proves rejection only; it is not a throughput,
reliability, persistence, recovery, or support test.

## Execution Authority And Escalation

Creating this planned tactical does not itself authorize physical-device or
iCloud interaction. When implementation is explicitly scheduled, ordinary
private refactoring, deterministic tests, Apple cross-builds, simulator work,
and same-boundary defect fixes are in scope. Physical picker interaction,
Files and Folders permission changes, app installation, force-close, and the
empty iCloud negative control require explicit authorization for that run.

Device work is confined to the repository-owned probe application, its
app-owned Documents container, and separately selected empty test directories.
Do not change signing accounts, iCloud account configuration, provider
installation, distribution records, or unrelated device/provider content.

Stop for direction if progress requires:

- accepting a remote or unclassifiable root;
- path/name/private-API/provider-allowlist heuristics;
- payload callbacks or copies through Swift;
- a process-long coordination lock or unbounded security scope;
- a new Swift torrent catalog, product schema, client target, binding
  generator, entitlement with release consequences, or minimum-iOS decision;
- writing to the external-provider negative control;
- destructive provider data handling; or
- a complete iOS client, distribution, background, notification, or migration
  design.

An unavailable picker automation path, ambiguous classification, permission
denial, stale bookmark, coordination error, test failure, or negative
background result is not permission to weaken the gate. Manual picker evidence
and an app-owned-only conclusion are valid outcomes.

## Execution Record

Implementation began from RSTorrent
`df038d922f946df2298d126ef0ee1b0b70ea3ecd` and JSTorrent
`9895410beeed6aff554053769bd006a3fbd373ef`. Xcode 26.6 build
`17F113`, the iPhoneOS 26.5 SDK, Rust 1.97, XcodeGen 2.45.3, and installed
`aarch64-apple-ios` plus `aarch64-apple-ios-sim` targets formed the refreshed
toolchain. The physical target remained an owned iPhone SE (3rd generation)
running iOS 26.6. No stable device identifier is retained.

The active SDK headers reconfirmed the planned public API boundary:

- `Foundation.framework/Headers/NSURL.h` lines 336, 338, and 375 expose
  optional internal-volume, local-volume, and ubiquitous-item values;
- the same header lines 426--428 expose `.minimalBookmark` on iOS and mark
  security-scope bookmark creation unavailable on iOS;
- `FileProvider.framework/Headers/NSFileProviderManager.h` line 138 exposes
  the asynchronous item/domain identifier lookup but no generic
  local-provider classification; and
- the picker and file-coordinator contracts remain the outer Swift adapter's
  authority. No Apple sample source was imported.

### Landed probe boundary

The repository-owned probe now has a task-free eligibility decision and a
versioned platform-local root registry. The registry accepts at most one
app-owned record and one selected-local record, bounds bookmark data to 64
KiB and labels to 256 UTF-8 bytes, uses random opaque IDs, advances generation
on repair, validates corrupt or future state, and fences one pending recovery
operation by root ID and generation. Bookmark bytes and identifiers never
enter Rust or retained JSON evidence.

`ProbeRootAccess` owns one serial root operation, one security scope, one file
coordinator, and one bounded File Provider lookup. Picker and restored URLs
start scope before resource observation; every read or write uses the URL
supplied to a per-operation coordinator; bookmark creation uses
`.minimalBookmark`; and the Rust call completes synchronously before
coordination and scope end. Non-file URLs, wrong kinds, symlinks, overlap with
the app-owned root, ubiquitous items, non-local or external volumes, missing
values, provider identity, lookup failure, and lookup timeout all fail closed.
Error evidence contains typed codes or bounded domain/code pairs rather than
paths or localized descriptions.

The real Rust probe retains the Tactical `116` storage seam and adds a
controlled interrupted workspace. Preparation creates and syncs only
`.rstorrent-ios-storage-probe`, closes the one engine file lease, records a
generation-fenced pending fact, and deliberately leaves that workspace.
Relaunch reruns the ordinary storage operation, which removes the exact owned
partial workspace before rebuilding, verifies the deterministic payload, and
clears the pending fact only after successful cleanup. Stale selected-bookmark
refresh and pending completion have one atomic store transition.

The root-selection implementation is deliberately present but disabled by
`ProbeRootPolicy.selectedRootRegistrationEnabled == false`. A system-picked
folder can only run the bounded, scope-balanced classifier. It cannot create a
bookmark, selected-root record, or Rust payload operation even if the pure
candidate values would say `selected_on_device`. Startup never restores an
older selected record while this gate is off, and no app-owned fallback can
reinterpret it. This is the final product-policy result of this tactical, not
a temporary success claim for picker-backed storage.

### Deterministic, host, and simulator evidence

The following focused gates pass:

```text
cargo fmt --manifest-path experiments/ios-storage-probe/Cargo.toml -- --check
cargo clippy --manifest-path experiments/ios-storage-probe/Cargo.toml -- -D warnings
cargo test --manifest-path experiments/ios-storage-probe/Cargo.toml
cargo build --release --manifest-path experiments/ios-storage-probe/Cargo.toml --target aarch64-apple-ios
cargo build --release --manifest-path experiments/ios-storage-probe/Cargo.toml --target aarch64-apple-ios-sim
cd experiments/ios-storage-probe && xcodegen generate
xcodebuild -project RSTorrentIOSStorageProbe.xcodeproj \
  -scheme RSTorrentIOSStorageProbe \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro,OS=26.2' \
  -only-testing:RSTorrentIOSStorageProbeTests test
```

Four Rust host tests pass. They cover exact fixture geometry, direct TCP/UDP
loopback, ordinary engine-pool I/O and cleanup, and interrupted-workspace
reconciliation. Twelve Swift tests pass on the iOS 26.2 simulator. They cover
the fail-closed truth table, optional and conflicting observations, future
enum rejection, evidence redaction, the disabled registration policy,
registry corruption and bounds, stable IDs, repair generation advancement,
pending-operation fencing, and atomic stale refresh/completion.

A direct simulator install/launch also passed app-owned storage and Rust
loopback networking. A controlled force-close left one pending workspace and
then recovered it on launch with generation 1, pending state `none`, and all
tracked current Swift resources zero. The simulator UI-test runner itself
failed to launch because Xcode reported no debugger version; this did not
replace the direct non-picker smoke or the passing standalone Swift tests.

### Physical app-owned matrix

The final development-signed build installed and launched without changing
signing or account state. Initial storage produced the exact SHA-1
`48b6fdf2fd3b77c14cc54f54891dc6aed1eeec3a`, retained 65,536 initial and
40,960 truncated bytes, rejected the no-replace collision, reopened the
publication, and removed its exact workspace. Direct Rust TCP and UDP each
echoed 30 bytes through the controlled in-process loopback endpoint.

The ordinary operation peaked at one of eight Rust file leases, two
probe-created files, and sampled process descriptors 7/8/7
(baseline/high/final). The interrupted preparation peaked at one file and the
same descriptor and engine-handle counts. Engine cached and owned handles
returned to zero. Swift root-operation and coordinator high water were each
one; security-scope and eligibility-request high water were zero because no
picker operation graduated or ran. Every current Swift resource count was
zero after each recorded terminal state.

The force-close run persisted generation 1 and phase
`prepared_partial_workspace`, synced a 40,960-byte partial file, closed all
leases, and was killed without an expiration callback. Relaunch removed the
partial namespace, repeated the full Rust check, preserved generation 1,
cleared the pending fact, and changed the armed/recovered facts from
`true/false` to `false/true`. Three further ordinary launch/run/terminate
cycles reached launch count 6 with generation 1, exact cleanup, descriptor
7/8/7, and no increasing Swift high water.

One final lifecycle launch registered and submitted the finite iOS 26
continued-processing task, armed one ordinary UIKit background assertion,
and entered the background when another application became active. Within 20
seconds the continued task reported `completed`, the ordinary assertion
reported `expired`, and tracked resources remained at zero. This is finite
lifecycle evidence, not indefinite torrent background authority.

### Picker gate and accepted negative result

The authorized physical UI-test attempt failed before application interaction
because the device timed out while enabling XCTest automation mode, reproducing
the Tactical `116` limitation. Apple iPhone Mirroring was also unavailable for
this Mac/device pairing. Therefore neither a distinct empty **On My iPhone**
directory nor an empty iCloud Drive directory was selected. No cloud directory
was created, read, bookmarked, registered, or mutated, and no provider
identifier was retained.

This absent control cannot graduate selected-local storage. The implementation
responds by disabling picker-root registration entirely, so there is no need
to run bookmark relaunch, permission, repair, rename/move, or selected-root
force-close cases. Those cases are inapplicable until a future tactical first
obtains both physical classification controls and deliberately changes the
compiled policy. App-owned Documents is consequently the sole supported root
result for the first iOS client tactical.

The final signed build was installed fresh after that policy was compiled in.
It reported one app-owned root at generation 1, selected roots disabled with
`fallback=false`, passing Rust storage and networking, descriptors 7/8/7, and
zero terminal Swift resources. The probe app was then uninstalled. Temporary
evidence and build output were removed, the simulator copy was uninstalled,
and no cloud control directory or other provider content required cleanup.

### Physical testbed follow-up

A maintainer-requested follow-up on 2026-08-10 used the dedicated
`ios-device-testbed` provider that should have been used for the original
picker attempt. Its `probe` and `doctor` checks passed for the wired physical
iPhone: the device was connected and unlocked, Developer Mode and Developer
Tools security were enabled, signing was available, and the pinned Agent
Device 0.20.5 runner cache was present.

The first transactional testbed session opened this exact signed probe build
successfully in 4.2 seconds. The follow-up then supplied an invalid unquoted
selector shape to `wait`, so the wait timed out before a snapshot or picker
action and ordinary session cleanup stopped the runner. That operator error is
not evidence of an application or device failure. A fresh semantic `open`
subsequently timed out after 90 seconds, and the testbed's prescribed
`recover` plus `prepare` sequence spent its full 240-second bound repeatedly
failing at `ios_runner_connect`. At the pause point the testbed again reported
the phone connected and unlocked with no active lease. This matches the
testbed's documented possibility of a human-attended Touch ID or XCTest
authentication gate after runner restart, but no prompt was observed and that
cause is not claimed as proven.

While semantic automation was unavailable, the newly installed build repeated
the complete non-picker physical matrix through CoreDevice. App-owned storage
again produced the expected SHA-1 and 65,536/40,960-byte geometry, rejected the
rename collision, returned the file pool to zero cached and owned leases, and
returned process descriptors from a sampled high of eight to a baseline of
seven. Direct Rust TCP and UDP loopback each echoed 30 bytes. A controlled
SIGKILL left the 40,960-byte partial workspace and generation-fenced pending
fact; relaunch removed it, repeated the full storage operation, preserved root
generation 1, and cleared the pending fact. Three more termination/relaunch
cycles retained the same bounds and zero terminal Swift resources. The final
background run completed the finite continued-processing task, expired the
ordinary UIKit assertion after 42 seconds, and retained zero current scopes,
coordinators, root operations, or eligibility requests.

No document-picker selection occurred during this follow-up, so the enabled
policy and support claims do not change. Resume only with the maintainer near
the phone: run the provider `probe`, `doctor`, and `prepare` sequence, complete
any local authentication prompt, then hold one transactional session across a
fresh semantic snapshot and the dedicated empty **On My iPhone** and iCloud
classification controls. Do not enable selected-root registration from the
mere fact that the runner reconnects.

## Deliberate Non-Goals And Next Boundary

This tactical does not implement:

- a complete iOS client, maintained product target, torrent Library or detail
  UI, add flow, settings surface, generated application binding, packaging,
  signing policy, distribution, migration, or release claim;
- an active iCloud Drive, third-party File Provider, network, removable, or
  offloaded payload root;
- cloud-root persistence, recovery, performance, quota, conflict,
  materialization, eviction, or seeding behavior;
- completed-file export, share, copy, move, relocation, source deletion, or
  adoption of existing payload;
- indefinite background downloading or seeding, notification policy, a
  minimum supported iOS version, or an assumption that finite continued
  processing covers a torrent session;
- trusting fast resume, relocation-aware verification, streaming priority,
  v2/hybrid content, or new protocol breadth; or
- a generic filesystem abstraction, native host, companion daemon, REST or
  WebSocket file proxy, or second storage runtime.

After this tactical closes, the next iOS slice may plan the first maintained
in-process client foundation against the exact graduated root set. Its initial
payload policy is app-owned Documents unless this physical matrix also
graduates selected **On My iPhone** storage. It must independently settle the
Swift binding, application-service lifecycle, profile location, foreground
download UI, background/notification policy, and controlled exact torrent
download evidence.

A later, separate product tactical may add **Export completed files...** as a
bounded coordinated copy to a user-selected destination. It should keep the
verified on-device payload authoritative until the copy is independently
validated. Turning that copy into relocation, deleting the source, or seeding
from the provider requires a stronger crash-safe authority-transfer design and
must not be inferred from export success.
