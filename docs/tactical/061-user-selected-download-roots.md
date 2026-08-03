# Tactical 061: User-Selected Download Roots

Status: Implemented for the macOS code paths on 2026-08-03. Store,
application, transport, and shared-UI evidence passes. The native chooser was
launched through Computer Use, but its transient macOS system panel was not
addressable by the available automation; one manual validation pass covering
choose, cancel, and restart in both interactive products still closes the
macOS stopping condition. Linux, Windows, and Android parity remain bounded
follow-up slices.

Topics: `download-roots`, `application-control`, `client-persistence`,
`web-ui-design`, `product-surfaces-and-migration`,
`storage-throughput-architecture`

## Motivation And Outcome

Before this tactical, the desktop product and `./scripts/webui` configured a
path-backed root named `downloads` beneath application-private state. The
React add path then submitted that root and all files without presenting
storage choice. That useful bring-up behavior was not an acceptable product
destination.

Implement the first JSTorrent-like root-selection slice on macOS. A fresh
interactive profile has no payload root. The first torrent add opens a shared
add-options dialog, requires a folder chosen through the local platform
adapter, registers that folder under an opaque durable root ID, makes the
first root the default, and starts the torrent only after confirmation. Later
adds show the current default by default and permit a per-torrent alternate
without silently changing it. The shared Settings surface manages established
roots and the add-options preference.

The stopping condition is a fresh macOS Tauri profile and a fresh macOS
`./scripts/webui` profile that both require and successfully use a native
folder selection on first add, persist the resulting root/default across
restart, support add/default/repair/bounded removal in Settings, preserve the
selected root on each torrent, and no longer create an implicit app-data
payload root. Deterministic store, application, gateway, Tauri-adapter, React,
and headless-browser evidence plus one Computer Use native-picker smoke must
pass. Linux and Windows native picker/build evidence remain explicitly open.

## Accepted Scenario Subset

This tactical implements these scenarios from
[`download-roots.md`](../topics/download-roots.md):

1. A fresh interactive profile opens with zero roots and no default.
2. A first add displays options, preserves the magnet while choosing a folder,
   and creates no torrent when the picker or dialog is cancelled.
3. A chosen folder is registered through a local platform operation; the
   presentation never supplies its path in `add_magnet` or another portable
   command.
4. The first usable root becomes the default. A later root becomes default
   only through an explicit action.
5. Each accepted torrent durably retains its selected root. Changing the
   default affects future adds only.
6. **Show options when adding torrents** defaults on. **Don't show again**
   disables it after a confirmed add. A missing or unavailable default still
   forces the dialog.
7. Settings shows bounded root identity, label, local display path,
   availability, and actions to add, default, repair/re-select, and remove.
8. An unreferenced root may be removed. A referenced root is rejected without
   changing torrent state. Removing the default clears it unless another root
   was explicitly selected first.
9. Restart reopens roots conservatively. A missing or unreadable path is
   projected as unavailable and is never recreated as proof of access.
10. An explicit developer/headless startup root remains supported, but is
    visibly preconfigured rather than silently inferred from application data.

This tactical continues to select every file. It does not stage magnet
metadata for file selection or claim the complete add-torrent workflow.

## Pre-Implementation Evidence And References

### Current RSTorrent boundary

- `crates/rstorrent-session/src/store.rs` schema version `4` already has a
  `storage_roots(root_id, locator)` table and every torrent has a foreign-keyed
  `storage_root`. Startup configuration currently upserts all roots, but the
  durable snapshot does not project them or a default.
- `crates/rstorrent-session/src/application.rs::ApplicationService` owns one
  immutable configured root map and creates configured path roots on open.
  Runtime registration, repair, availability, and default mutation are absent.
- `crates/rstorrent-session/src/control.rs::Command::AddMagnet` already accepts
  an opaque root ID and validates it. That portable command remains the
  torrent-binding operation; it will not accept a path.
- `clients/desktop/src-tauri/src/lib.rs::desktop_application_config` currently
  installs `<app-data>/downloads`.
- `crates/rstorrent-gateway/src/main.rs` requires
  `RSTORRENT_STORAGE_ROOT`, and `scripts/webui` always supplies
  `.local/webui/downloads`.
- `clients/web/src/inspection/live/LiveApplication.ts` hard-codes
  `storage_root: "downloads"`, while Settings currently owns only local
  appearance preferences.

### JSTorrent product oracle

The sibling JSTorrent checkout was inspected at
`9895410beeed6aff554053769bd006a3fbd373ef` on 2026-08-03. Its checkout has
untracked maintainer files that are unrelated and must not be modified.

- `packages/engine/src/config/config-schema.ts::showFileSelection` defaults
  add options on and stores `defaultRootKey` separately from the root list.
- `packages/client/src/App.tsx` makes the first accessible root the effective
  default and performs explicit later default changes.
- `packages/ui/src/components/FileSelectionModal.tsx` combines location,
  files, confirmation, cancellation, and **Don't show again**. RSTorrent uses
  the location-only subset until staged metadata exists.
- `packages/client/src/engine-manager/daemon-engine-manager.ts` reconciles
  persisted roots/default with host capabilities and treats folder acquisition
  as a host operation.
- `packages/engine/src/storage/storage-root-manager.ts` resolves a
  torrent-specific root before the default and rejects missing roots.
- `desktop/host/src/folder_picker.rs` starts at a recent usable root or Home on
  macOS, deliberately avoids probing Downloads, deduplicates selected paths,
  and registers the root in the native host.

RSTorrent adopts the behavior and failure lessons, not JSTorrent's native-host
topology, path-derived root identity, mutable engine configuration, or source.

### Pinned libtorrent storage oracle

The pinned libtorrent checkout is
`7d7fc38fac61177fa5e02148f791b2f65250b09d`.

- `include/libtorrent/add_torrent_params.hpp` records `save_path` per torrent.
- `src/session_handle.cpp` rejects an empty save path before adding a torrent
  (`errors::invalid_save_path`), and `test/test_session.cpp` covers the
  asynchronous empty-save-path failure.
- `src/write_resume_data.cpp` and `src/read_resume_data.cpp` persist and restore
  the chosen save path.
- `src/storage_utils.cpp::move_storage` and its tests demonstrate that changing
  storage for an existing torrent is a fenced operation with collision,
  partial-move, and recheck consequences. This tactical therefore changes only
  future defaults and root locators for unavailable roots; torrent relocation
  remains out of scope.

There is no protocol specification for a user's download-directory choice.
The normative inputs are the accepted product topic and operating-system
capability semantics; BitTorrent metainfo/path safety continues to be governed
by the existing engine tacticals.

## Owners, Tasks, Cancellation, And Data Flow

```text
shared React add/settings UI
  | established root IDs only
  +-- Tauri local-folder operation --------\
  +-- loopback WebUI local-folder endpoint -+--> native macOS picker
                                             |    cancel => no mutation
                                             v
                                     platform adapter validates path
                                             |
                                             v
application service
  root registry/default/settings + torrent binding + reactive projection
         |                         |
         v                         v
  SQLite profile authority     engine receives resolved root path
```

- `ApplicationService` owns the in-memory root registry, default, add-options
  preference, store mutation, and reactive application view. It remains the
  single writer behind its existing owner mutex.
- `SessionStore` owns schema migration, stable root records, settings, root
  references, and request receipts for portable semantic mutations.
- Tauri and the local gateway own picker invocation. They obtain a suggested
  starting directory without holding the application mutex across the visible
  dialog, then return the selected path directly to an in-process
  installation method.
- The picker child process is the only new background operation. Cancellation
  is the normal OS-dialog Cancel result. The macOS process is kill-on-drop so
  an abandoned request does not intentionally leave an ownerless picker.
- Existing torrent metadata/content owners remain unchanged. A root cannot be
  removed while referenced, and repair of an active torrent's root is rejected
  until that torrent is inactive so an already-running task cannot retain a
  stale locator snapshot.
- Checkpoint tasks receive an immutable root-map snapshot, as today. Root
  mutation updates future tasks and reactive durable views; it does not alter
  an active engine task behind its back.

Dependency direction remains presentation/platform adapter → application
service → engine. OS dialog or path types do not enter protocol or engine
modules.

## Durable Model And Bounds

Schema version `5` adds enough typed data to make the existing root table the
profile authority:

- at most 32 retained roots per profile;
- opaque root IDs of at most the existing 128-byte bound, generated as a
  fixed-format random identifier for locally selected roots;
- UTF-8 display labels of 1..=256 bytes;
- UTF-8 desktop locators of 1..=4096 bytes in this path-backed slice;
- one nullable default root ID;
- one `show_add_options` boolean defaulting true; and
- availability derived conservatively at open/repair rather than accepted as
  durable proof.

Configured developer/test roots are merged idempotently at open. Persisted
roots not present in startup configuration remain available for restart.
Duplicate canonical desktop locators resolve to the existing root instead of
creating aliases. A first installed usable root sets the default in the same
transaction. Repair retains the root ID and updates its label/locator only
after the replacement directory passes validation and does not belong to
another root.

Schema migration retains existing root and torrent references but does not
automatically make a pre-v5 root the new default. This prevents an old implicit
app-data root from silently governing future adds. Existing torrents may keep
using that retained root so migration does not strand or delete payload.

The application snapshot and torrent-list view project a bounded complete root
list plus default and add-options preference. Root/settings changes use the
existing acknowledged, resettable view delivery rather than browser-local
state or a new polling loop.

## Implementation Sequence And Gates

### 1 — establish durable root semantics

Migrate the store, load persisted roots, add default/add-options mutations,
add runtime install/repair/removal operations, project conservative
availability, and replace the service's startup-only root authority.

Gate: store and application tests prove fresh, configured, first-root,
deduplication, explicit-default, torrent binding, referenced-removal,
unavailable restart, repair, receipt replay, and v4 migration behavior.

### 2 — carry roots through the application view

Extend the bounded torrent-list snapshot/patch with complete storage settings,
update generated TypeScript/Kotlin contracts, validators, reducers, and the
live inspection model.

Gate: Rust view-set and TypeScript reducer/validation tests prove initial
snapshot, reactive root/default/preference patches, patch coalescing, reset,
and unknown-input rejection.

### 3 — install trusted macOS picker adapters

Remove the Tauri implicit root. Add a Tauri local-folder command and an exact
loopback/Origin-authenticated gateway endpoint. On macOS, invoke the system
folder chooser starting at the most recent usable root or Home. The browser
never transmits a path. `RSTORRENT_STORAGE_ROOT` becomes an optional explicit
developer/headless injection; ordinary `scripts/webui` omits it.

Gate: adapter tests prove cancellation, start-directory selection, explicit
injection, endpoint authentication, and path-free portable commands. Both
desktop and gateway compile on macOS. Non-macOS builds retain an honest
unsupported stub until their native slices.

### 4 — implement shared add and Settings UX

Add the location-only add-options dialog, root chooser/default selector,
**Don't show again**, and Downloads/Storage settings. Retain magnet input on
cancel/failure. Bypass the dialog only when options are off and the default is
usable. Keep every-file selection explicit in copy.

Gate: component tests cover keyboard/focus behavior, first add, cancel,
per-torrent choice, default invariance, forced repair, preference toggling,
add/default/remove/repair settings, and useful errors. Existing wide/phone
headless suites remain green.

### 5 — prove the macOS product paths

Use isolated profiles and temporary chosen payload directories. Run one fresh
Tauri picker smoke and one fresh `scripts/webui` picker smoke through Computer
Use. Confirm root/default persistence after restart and exact selected-root
binding without relying on a public swarm completing.

Gate: capture the actual commands, profile isolation, UI observations, and any
permission behavior below. Remove temporary downloads and captures before
completion.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure/store | Schema v4→v5, zero-root fresh profile, 32-root bound, ID/label/locator bounds, first-default transaction, duplicate path, explicit default, referenced removal, repair identity, unavailable reopen, request replay/conflict. |
| Application | Selected root reaches torrent record and resolved engine config; changing default does not move prior torrents; unavailable default forces choice; active-root mutation is fenced; view refresh is coherent. |
| View/transport | Snapshot, patch, coalescing, reset, generated schemas, Tauri invoke and gateway endpoint limits/authentication; no path in `add_magnet`. |
| React | Add-options default-on, first-add requirement, cancellation/input retention, alternate root, Don't show again, settings actions, unavailable state, wide and phone accessibility. |
| macOS native | Tauri and WebUI system chooser selection/cancel, Home/recent start behavior where observable, chosen path registration, restart persistence, and no implicit app-data payload directory. |
| Baseline | `cargo fmt --all -- --check`; `cargo clippy --workspace -- -D warnings`; `cargo test --workspace`; web typecheck/tests/E2E in proportion to changed surfaces. |
| Future Linux | Native portal/dialog selection, Wayland UI automation, build dependencies, unavailable/permission behavior. Not required for completion here. |
| Future Windows | Native folder selection in an interactive session, WinApp automation, Windows path/restart behavior, ARM64 VM and preferably native x64 build evidence. Not required for completion here. |
| Future Android | Existing SAF root/restart regressions and later multi-root presentation alignment. No device action in this tactical. |

No public-swarm completion, performance run, VM startup, physical-device use,
or network fixture download is required. Native visible macOS picker actions
are authorized by the originating request. Any new OS permission prompt is
handled normally; this work never edits TCC storage.

## Non-Goals And Next Boundaries

- staged magnet metadata and file selection;
- `.torrent` file intake;
- changing file priorities after add;
- torrent relocation, content import, or automatic move from app data;
- user-visible publication naming beyond the existing info-hash layout;
- collision/adoption policy for recognizable final names;
- macOS security-scoped bookmarks or sandbox entitlement design;
- free-space monitoring, hot-plug watchers, or a root polling task;
- browser File System Access API storage;
- a remote/relay ability to open a backend machine's picker;
- full Linux, Windows, or Android parity in this session; and
- implementing YepAnywhere federated session jump or cross-host delegation.

The recommended immediate follow-up is a user-visible publication-layout
tactical, because selected roots still contain hash-named torrent directories.
The later staged magnet/file-selection tactical should then replace the
location-only add dialog without changing root identity or platform authority.

## Escalation Contract

Implementation may choose internal names, refactor the root map/store boundary,
update generated contracts, add same-boundary adversarial tests, and adjust
copy/layout without further approval. Stop for direction if evidence requires
paths in the portable command, a background native host, a new external
dependency with meaningful platform tradeoffs, automatic relocation, schema
semantics that discard existing torrent access, or a product behavior that
contradicts `download-roots.md`.

## Completion Evidence

### Landed checkpoints

- `10fee9d` defines the accepted behavior, JSTorrent oracle, bounded macOS
  slice, and platform follow-ups.
- `f64b419` adds schema version 5, the durable root registry/default and add
  preference, runtime install/repair/removal, availability projection, and
  generated client contracts.
- `2db2422` removes implicit app-data payload roots and adds the macOS picker,
  Tauri operation, authenticated loopback operation, and optional explicit
  developer root injection.
- `457df14` adds the shared location-only add dialog and root-management
  Settings experience, including first-root selection, per-add overrides,
  **Don't show again**, repair, default, and removal actions.
- The final checkpoint makes native picker execution asynchronous and
  kill-on-drop, records the evidence below, and preserves honest non-macOS
  unsupported behavior.

### Automated evidence

Completed on macOS on 2026-08-03:

- `cargo test -p rstorrent-session --no-fail-fast`: 81 session library tests
  plus the crate's additional test targets passed.
- `cargo test -p rstorrent-platform -p rstorrent-gateway -p rstorrent-desktop`:
  2 platform, 15 gateway, and 3 desktop tests passed. These include picker
  starting-path/error behavior, exact Origin/authentication enforcement,
  installed-root response, and repair-ID forwarding.
- `npm run typecheck --prefix clients/web` passed.
- `npm test --prefix clients/web` passed: 20 test files, 115 tests passed and
  2 unrelated tests skipped.
- `npm run build --prefix clients/web` passed with the existing Vite chunk-size
  warning.
- With an isolated Vite server on port 4178,
  `RSTORRENT_PLAYWRIGHT_BASE_URL=http://127.0.0.1:4178 npm run test:e2e`
  passed 15 wide/phone tests; 4 live-fixture tests were intentionally skipped.
- `cargo fmt --all -- --check`,
  `cargo clippy --workspace -- -D warnings`, and `cargo test --workspace` all
  passed on the final tree. The workspace test run included 174 engine tests
  passed with 3 opt-in live tests ignored, 81 session tests, 70 protocol tests,
  and every platform, gateway, desktop, Android, binary, architecture, and
  doc-test target without a failure.

### Computer Use macOS evidence and remaining manual gate

An isolated `./scripts/webui --no-open` profile was run on port 4179 with no
configured root. Computer Use submitted a retained test magnet and observed:

- the first-add **Choose download options** dialog;
- the required-folder explanation, **Choose folder...**, explicit all-files
  copy and later-file-selection deferral, and disabled confirmation;
- launch of `/usr/bin/osascript` and macOS's transient Open and Save Panel
  Service, starting from Home; and
- after the test-owned picker process was terminated, an actionable picker
  error, re-enabled choose action, disabled add action, and the original magnet
  still present. No torrent or implicit payload root was created.

The available Computer Use runtime could enumerate Chrome and ordinary
applications but could not attach to
`com.apple.appkit.xpc.openAndSavePanelService`; addressing that process or its
XPC path timed out. The chooser therefore could not be selected or cancelled
through automation. The isolated test browser tab was closed, the server was
stopped, and its temporary profile and destination were moved to Trash.

Still required before changing this tactical to completed:

1. manually choose and cancel the macOS system panel once through both a fresh
   Tauri profile and a fresh `scripts/webui` profile;
2. restart each profile and observe the same selected root/default and exact
   torrent binding; and
3. execute the future native Linux and Windows picker/build slices in
   interactive sessions. Those platform slices do not block the scoped macOS
   implementation, but they remain prerequisites for a cross-platform claim.
