# Tactical 161: Packaged Desktop Folder Picker

Status: **Complete (2026-08-24).** The parented native picker, focused
selection/persistence tests, hosted Windows/Linux packages, and installed
Windows choose/cancel/repair/restart campaign pass. Desktop release/updater
Tactical [`158`](158-desktop-signed-packaging-and-updater.md) resumes as
**Now** and owns the first signed package containing this work.

Topics: `beta-release-readiness`, `client-surfaces`, `download-roots`,
`client-persistence`

Dependencies: completed download-root Tactical
[`061`](061-user-selected-download-roots.md); completed cross-platform
presubmit Tactical [`159`](159-cross-platform-presubmit-ci.md); completed
Windows startup repair Tactical
[`160`](160-windows-local-network-address-selection.md); and the maintained
Tauri/React product.

## Decision And Desired Outcome

Replace the packaged Tauri application's helper-process picker with the
official Tauri dialog plugin's native Rust API. Windows must gain a real
folder picker, and packaged Linux must no longer depend on Zenity or KDialog
being installed. Keep the operation inside the native Tauri command and
parent the folder panel to the product window.

The finished slice lets a fresh Windows or Linux desktop profile choose its
first download directory, registers the selection as a stable root through
the existing application service, makes the first usable root the default,
and restores the same root after application restart. Cancel and failure make
no root or torrent mutation. macOS retains native folder-panel behavior
through the same implementation.

This is a product usability gate rather than a new storage model. The React
application requests a platform operation and receives a root snapshot; it
does not acquire, persist, or submit an ambient filesystem path.

## Scope And Stopping Condition

This tactical owns:

1. exact compatible `tauri-plugin-dialog` integration in the packaged desktop
   shell, using only its Rust API;
2. a folder dialog parented to the invoking webview window, titled for the
   download-root operation, and initialized from the most recent usable root
   or the user's home directory;
3. cancel, native-dialog failure, path-conversion failure, install, and repair
   behavior at the existing Tauri command boundary;
4. focused deterministic tests proving that cancel is mutation-free and a
   chosen path uses the existing first-root/default and stable-ID repair
   semantics;
5. credential-free Windows and Linux package builds in hosted CI;
6. a repeatable way to retain a Windows package from an explicitly requested
   CI run for installed testbed validation, without adding artifacts to every
   presubmit run; and
7. installed Windows evidence for real choose, cancel, first-root/default,
   process restart, and durable restoration, plus cleanup of the isolated test
   profile and selected directory.

The tactical stops when the local Rust and web regression floors pass, the
hosted desktop matrix still packages successfully, and an installed Windows
package passes the real native-folder-panel and restart campaign. Linux's
native package build is required; an installed Linux picker smoke may be
recorded if the available testbed can exercise it without expanding this
slice, but it is not substituted for the required Windows evidence.

## Product Contracts And Invariants

- The generated application command contract and portable React adapter gain
  no path-authority input or new path output. The existing root snapshot may
  still carry its display-only normalized path.
- The Tauri command returns `None` on user cancellation and performs no root,
  default, preference, or torrent mutation.
- A selected path is registered or repaired only through
  `ApplicationService`; its existing directory checks, normalization, stable
  root identity, first-root default, and persistence rules remain the sole
  authority.
- Repair preserves the caller-selected opaque root ID. Adding a duplicate
  usable path retains the application service's current deduplication policy.
- The dialog starts at the suggested existing root when it is usable and then
  at the platform home directory. It never invents an app-data payload root.
- The native panel is parented to the invoking desktop window. Only one
  selection result is consumed per command invocation.
- JavaScript receives no dialog-plugin permission. The plugin is native shell
  infrastructure, not a browser or shared-UI capability.
- The local WebUI gateway retains its current platform adapter, including
  Zenity/KDialog behavior. This slice changes only packaged Tauri ownership.
- No updater identity, signing key, release route, package identifier, storage
  schema, root locator representation, or torrent record changes.

## Owner, Task, Cancellation, And Dependency Map

```text
React choose-folder action
  -> transport-neutral ApplicationClient operation
  -> Tauri choose_download_root command
  -> native dialog manager, parented to WebviewWindow
  -> user chooses or cancels
  -> selected native PathBuf (never sent to React)
  -> ApplicationService install/repair
  -> stable StorageRootSnapshot returned to React

ApplicationService
  -> root validation and normalization
  -> SQLite root/default persistence
  -> restart restoration
```

The Tauri command owns one dialog invocation and one oneshot receiver. The
plugin owns the platform dialog lifetime and completes the callback once.
Dropping a command receiver cannot create a root: registration occurs only
after a selected path is successfully received. No retry loop, background
worker, persistent task, queue, or additional mutable owner is introduced.

Dependency direction remains presentation shell -> platform dialog and
application service -> session/store. The session and store remain independent
of Tauri, native window types, and dialog callbacks.

## Source-First Dossier

The product-behavior reference is the local JSTorrent checkout at commit
`9598770baecb1164a00ba5d41f7e7c11bfb78828`:

- `desktop/tauri-app/src-tauri/src/lib.rs:753` implements its parented Tauri
  folder panel, awaits the callback through a oneshot, and registers the
  result through the native host;
- `desktop/host/src/folder_picker.rs` is its separate host-process fallback
  using `rfd::AsyncFileDialog`; and
- the client uses its latest root path as the starting directory.

RSTorrent adopts the native, parented interaction and recent-root start
behavior. It intentionally does not copy JSTorrent's bridge/native-host split
or expose a path to presentation code because RSTorrent already has an
in-process application service and stable root registry.

The selected dependency is exact `tauri-plugin-dialog` `2.7.2`, from the
official Tauri plugin source already resolved locally under Cargo's registry.
Its MIT/Apache-2.0 desktop implementation provides `DialogExt`,
`FileDialogBuilder::set_parent`, `set_directory`, and callback-based
`pick_folder`; it uses `rfd` for Windows, Linux, and macOS native integration.
No reference source or fixture is imported.

There is no BitTorrent protocol specification involved. Storage-root
semantics remain those surveyed and implemented by Tactical `061`; this slice
does not change protocol, engine, hashing, scheduling, or payload-I/O state.

## Edge And Failure Cases

- A cancel closes the operation with `Ok(None)` and no mutation.
- A callback channel closed without a result is an actionable native-command
  error and no mutation.
- A native file-path value that cannot become a local path is rejected before
  the application service is locked for mutation.
- A deleted, non-directory, or otherwise unusable selected path is rejected
  by the application service and creates no registered root.
- A repair request for an unknown or inapplicable root fails through the
  existing typed application-service path.
- A fresh profile with no usable suggested path uses home only as the dialog
  starting location, never as an automatically accepted root.
- Reopening after a successful first selection restores the same root ID,
  default, label, and availability from SQLite.

## Implementation And Validation Stages

1. Record this decision, select Tactical `161` as the sole **Now**, and pause
   Tactical `158` without changing its release outcome.
2. Add the exact dialog dependency, register the plugin, replace Tauri's
   `rstorrent-platform` call with the parented native callback, and keep the
   selected path private to Rust.
3. Factor only the small selection-to-registration seam required for
   deterministic install/cancel/repair tests.
4. Add an opt-in hosted Windows package artifact for testbed transfer if the
   current CI cannot otherwise provide the built package.
5. Run local formatting, lint, workspace tests, desktop tests, and web tests;
   then run hosted credential-free platform builds.
6. Install the retained Windows package in an isolated machine-control
   campaign, exercise choose and cancel through the OS panel, restart, verify
   durable root/default state, and clean up.
7. Reconcile the tactical and topics with exact evidence, return Tactical
   `158` to **Now**, and leave ordinary close/reopen/tray policy in `DESK-004`.

## Result And Evidence

Commits `5273bd4`, `75a05d1`, and `d071a1a` record the decision, native
implementation, and opt-in package handoff respectively.

The packaged Tauri shell now uses exact `tauri-plugin-dialog` `2.7.2` from
Rust. The invoking `WebviewWindow` parents the panel; Tauri's platform path
resolver supplies a valid Windows home directory; the plugin callback crosses
one oneshot; and only the resulting native `PathBuf` reaches
`ApplicationService`. The old `rstorrent-platform` dependency remains in the
local WebUI gateway but is no longer part of the packaged desktop shell. No
dialog permission is granted to JavaScript and no generated application
contract changed.

Focused desktop evidence passes ten tests, including callback-channel failure,
cancel with no root mutation, first-root defaulting, unavailable-root repair
under the same opaque ID, and durable reopen after repair. The complete local
floor passed:

```text
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
```

The web run passed 262 tests with two deliberate skips. Local `actionlint`
also accepted the opt-in artifact step.

Credential-free CI run
[`32713131288`](https://github.com/kzahel/rstorrent/actions/runs/32713131288)
passed all seven jobs: Rust plus loopback interoperability; web type/unit/build
plus deterministic Playwright E2E; Windows x86_64, Linux x86_64, and macOS
arm64 desktop tests/packages; Android dual-ABI lint/tests; and iOS simulator
tests plus unsigned archive. The Windows leg also repeated the native
local-address regression and retained one three-day NSIS smoke artifact only
because the run was explicitly dispatched. Its exact installer was 10,091,782
bytes with SHA-256
`160eb97ecf554112f42768b4d83583e52e53938bb91e656cb96fbe60a7680675`.

An exclusive machine-control campaign then installed that x86_64 NSIS package
on the accepted Windows 11 appliance with no pre-existing RSTorrent install or
profile. UI Automation observed the native **Choose a download folder** dialog
owned by the RSTorrent window. The campaign proved:

- cancel returned **Folder selection canceled** while **No download folder has
  been chosen yet** remained and no root appeared;
- choosing one prepared directory produced one root and **Default download
  folder**;
- target-native process termination and a new launch restored that same root
  as default;
- removing only the empty prepared directory made the root **Unavailable —
  repair required**;
- **Repair...** opened the same native panel, selected a second prepared
  directory, retained one default root, and reported the repair; and
- a second process termination/new launch restored the repaired root as
  available and default.

First launch of the unsigned listener build also displayed the Windows
Security allow/cancel panel. The campaign selected Cancel, granted no broader
firewall access, and the app plus picker remained usable. This is package and
incoming-listener consent evidence, not a picker failure; Tactical `158` must
characterize the signed candidate and preserve explicit guidance.

Cleanup stopped and uninstalled RSTorrent, removed its seven-file isolated
test profile, the two exact prepared directories, the transferred installer,
and the local temporary artifact. The exclusive claim was released. Existing
JSTorrent state and unrelated applications were not modified. One dialog,
oneshot, selected path, and application-service mutation were live per
operation; no new background task or durable owner was introduced.

Installed Linux picker interaction remains unclaimed. The hosted Linux
x86_64 AppImage test/package proves dependency and compilation coverage, not
portal/desktop behavior. Ordinary close/quit/reopen, tray, and single-instance
policy remain `DESK-004` rather than being inferred from the controlled
process-restart evidence above.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure/session | Existing root install, default, duplicate, repair, and restart tests remain green |
| Desktop native | Focused command-seam cancel/install/repair tests and callback failure behavior |
| Shared UI | Typecheck and unit/component suite; no generated or ambient-path contract drift |
| Local package | Tauri desktop build or package on the host where supported |
| Hosted package | Credential-free Windows x86_64 and Linux x86_64 package legs pass; existing macOS legs remain green |
| Installed Windows | Real parented picker choose/cancel, first default, controlled process restart, persisted root restoration, cleanup |
| Installed Linux | Optional supplemental evidence; absence remains explicit rather than inferred from Windows |

The proportional local floor is:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
```

## Non-Goals And Next Boundary

- browser File System Access, remote path submission, or a generic
  cross-transport dialog API;
- changing the local WebUI gateway's picker or requiring the dialog plugin in
  headless binaries;
- sandbox bookmarks, Linux portal policy selection, removable/cloud-root
  policy, root relocation, payload migration, or general multi-root UX work;
- single-instance behavior, tray/background mode, ordinary close/quit/reopen
  policy, file associations, magnet handoff, or crash-restart policy;
- Intel macOS installed evidence or treating Windows proof as Linux installed
  proof;
- changing signed updater artifacts or publishing a release.

After completion, Tactical `158` resumes as **Now** and owns the first signed
package containing both completed Windows startup repair `160` and this native
picker, followed by exact installed update evidence. General lifecycle work
remains `DESK-004` and installed Linux x86_64 evidence remains in the release
tactical.

## Escalation Contract

Implementation may choose conservative internal helper names, test seams, and
CI artifact retention details; update Cargo resolution; and repair same-owner
defects exposed by the native picker. Stop for direction if evidence requires
exposing paths to React, changing root persistence or identity, adding a
second platform abstraction or remote host, selecting a materially different
licensed dependency, changing public update/release state, or expanding into
desktop lifecycle policy.
