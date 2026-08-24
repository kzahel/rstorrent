# Tactical 162: Desktop Single-Instance And Tray Lifecycle

Status: **Implementation in progress and selected as Now (2026-08-24).**
Desktop release/updater Tactical
[`158`](158-desktop-signed-packaging-and-updater.md) is paused behind this
bounded installed-lifecycle gate and resumes when this tactical completes.

Topics: `beta-release-readiness`, `client-surfaces`,
`product-state-and-feedback`, `client-persistence`

Dependencies: completed cross-platform presubmit Tactical
[`159`](159-cross-platform-presubmit-ci.md); completed Windows listener and
folder-picker Tacticals [`160`](160-windows-local-network-address-selection.md)
and [`161`](161-packaged-desktop-folder-picker.md); the maintained Tauri/React
product; and the existing joined `ApplicationService` shutdown boundary.

## Decision And Desired Outcome

Make the packaged desktop application own one predictable product lifetime.
One OS user may run only one RSTorrent application instance. Closing the main
window normally keeps that exact instance and its in-process application
service alive behind a tray icon. The tray restores the window, starts a
visible manual update check, changes the persisted close policy, or requests
one joined application shutdown. No ordinary desktop action may leave an
unreachable process or bypass application-service cleanup.

Fresh installs default **Run in Background** to enabled. This preserves active
transfers when a user closes the window and matches the mature JSTorrent
product convention. The setting is shell-owned installation state rather than
a torrent/profile setting: it applies before or across profile services and
does not enter the generated application contract or profile database.

This tactical deliberately does not add file associations, magnet/deep-link
handoff, start-at-login, crash restart, window-position persistence, transfer
statistics in the tray, or JSTorrent identity migration. A second ordinary
launch restores the existing window; later input-routing work may consume its
bounded argument handoff without changing the single-instance owner.

## Scope And Stopping Condition

This tactical owns:

1. early single-instance registration on every packaged desktop platform and
   restoration/focus of the existing main window on an ordinary second launch;
2. a tray on macOS, Windows, and Linux with **Show RSTorrent**, **Check for
   Updates**, checked **Run in Background**, and **Quit RSTorrent**;
3. a versioned, bounded, atomically replaced shell settings file whose sole
   initial value is `run_in_background`, defaulting to true;
4. close-to-hide while background operation is enabled and joined shutdown
   when it is disabled;
5. one idempotent shutdown owner used by window close, tray Quit, application
   exit requests, and the existing shutdown command;
6. main-window restoration after hiding, minimization, macOS Dock reopen, and
   a second ordinary launch;
7. a tray update action that restores the main window and enters the existing
   visible manual-update state without adding an updater implementation;
8. focused deterministic lifecycle/settings tests and unchanged local Rust,
   React, browser E2E, and hosted platform package floors;
9. opt-in retained Windows NSIS and Linux AppImage packages for installed
   machine-control campaigns; and
10. installed Windows x86_64 and Linux arm64 evidence for close, tray,
    restore, second launch, setting persistence, update check, joined Quit,
    and visible application/taskbar icon behavior. Linux x86_64 remains a
    native compile/package gate because the available Linux installed testbed
    is arm64.

The tactical stops when local and hosted gates pass and both installed
campaigns prove that close-to-background remains reachable, a second launch
does not create a second application-service process, the same window can be
restored, disabling background makes close terminate completely, the setting
survives relaunch, and tray Quit terminates completely. macOS must still build
and retain its existing Dock-reopen behavior; installed Intel macOS is not
required.

## Product Contracts And Invariants

- Register single-instance ownership before plugins or setup work that can
  create a profile, bind listeners, start a media server, or show a window.
- A second ordinary launch never creates a second engine, profile service,
  listener set, media server, updater timer, tray, or main window. It asks the
  established owner to show, unminimize, and focus its main window.
- Unrecognized second-launch arguments are bounded and ignored in this slice.
  They are not logged verbatim, persisted, or interpreted as paths or URLs.
- Hiding is not destruction. Close-to-background prevents the close and keeps
  the same webview, subscriptions, view-resource leases, profile, and engine
  alive.
- **Show RSTorrent** is idempotent. It shows, unminimizes, and focuses the
  existing main window; if a platform destroys it despite policy, the shell
  recreates it from the configured `main` window definition.
- **Check for Updates** first restores the main window, then emits one
  Tauri-only product-lifetime request. The existing updater controller
  deduplicates, times out, and renders the manual result. Browser/demo clients
  gain no Tauri dependency.
- Turning **Run in Background** off commits the setting before changing the
  checkmark. It does not immediately stop transfers. The next ordinary close
  requests joined shutdown.
- Turning the setting on or off is atomic from the process's perspective. A
  failed durable write leaves both in-memory policy and checkmark unchanged
  and emits a bounded diagnostic.
- The settings file has a closed versioned schema and fixed maximum size.
  Missing state defaults to enabled. Malformed, unknown-version, oversized,
  or non-file state is replaced with defaults without blocking startup; a
  bounded diagnostic explains the reset without exposing paths.
- Exactly one owner may start shutdown. Concurrent close, Quit, exit, and
  shutdown-command requests observe that owner rather than start another.
  Initial exit requests are prevented; only successful completion sets the
  final-exit gate and calls `app.exit(0)`.
- Joined shutdown stops subscriptions and view-resource registrations, awaits
  `ApplicationService::shutdown`, awaits the media server, and only then exits.
  Failure is visible and fails closed rather than forcing process termination.
- Window destruction retains the existing generation-fenced subscription and
  view-resource cleanup. Shutdown and restore cannot attach new resources to
  an obsolete generation.
- The tray and taskbar/window icon use committed package artwork. Absence or a
  generic platform icon is a release defect; this slice adds the narrowest
  safe platform fix supported by installed evidence.
- Release Windows packages use the GUI subsystem and must not create a console
  or terminal window as a side effect of ordinary launch.
- No updater key/route, package identifier, profile schema, torrent record,
  storage locator, engine behavior, or generated application contract changes.

## Owner, Task, Cancellation, And Dependency Map

```text
OS application lifetime
  -> single-instance plugin (registered first)
     -> established AppHandle
        -> show/unminimize/focus main window

DesktopLifecycle state
  -> immutable app-config settings path
  -> synchronized DesktopShellSettings
  -> shutdown state: running | stopping | final-exit
  -> checkable tray menu item

window close / tray Quit / application exit / shutdown command
  -> one compare-and-start shutdown request
  -> stop presentation registrations
  -> await ApplicationService shutdown
  -> await media-server shutdown
  -> final-exit gate
  -> app.exit(0)

tray Check for Updates
  -> restore main window
  -> Tauri event adapter
  -> existing DesktopUpdater manual check
  -> existing About & updates presentation
```

The app lifetime owns settings and shutdown state. Each tray/menu callback is
synchronous and starts at most one bounded persistence operation or one
shutdown task. The shutdown task has one explicit owner and completes before
the process exit gate opens. No detached engine, storage, or media-server task
is introduced. Dependency direction remains Tauri shell -> application
service; no protocol, session, or engine layer depends on Tauri or tray types.

## Source-First Dossier

The product-behavior reference is the local JSTorrent checkout at commit
`9598770baecb1164a00ba5d41f7e7c11bfb78828` (MIT):

- `desktop/tauri-app/src-tauri/src/lib.rs` registers
  `tauri-plugin-single-instance` before other plugins, restores the main
  window for unhandled second launches, implements default-on persisted
  `run_in_background`, hides on close, builds the tray and menu actions, and
  preserves macOS reopen;
- the same file contains explicit Windows and Linux window-icon repair because
  package/default icon propagation was insufficient in that product; and
- `desktop/tauri-app/src-tauri/Cargo.toml` plus `desktop/Cargo.lock` resolve
  `tauri-plugin-single-instance` `2.4.3`.

RSTorrent adopts the single owner, default-on background policy, tray action
vocabulary, and show/unminimize/focus behavior. It intentionally does not copy
JSTorrent's native-host/extension routing, raw immediate Quit, autostart,
window-state plugin, live tray statistics, macOS tray-visibility preference,
or unsafe Win32 icon implementation. RSTorrent already owns the engine and
application service in-process, so its Quit must await those owners.

The platform implementation references are exact Tauri `2.11.5` tray/menu,
window-event, run-event, image, and application APIs and official
`tauri-plugin-single-instance` `2.4.3` (MIT/Apache-2.0). Tauri's tray source
states that Linux does not emit tray icon click events, so Linux acceptance
uses menu **Show RSTorrent** and does not claim left-click restoration.

There is no BitTorrent protocol specification or libtorrent behavior involved.
No reference source, fixture, artwork, or persisted format is imported.

## Edge And Failure Cases

- Close while background is enabled hides once and never triggers shutdown.
- Close while background is disabled prevents destruction, starts shutdown
  once, and exits only after joined completion.
- Repeated close, Quit, exit, and shutdown-command requests during stopping do
  not race, panic, or create duplicate shutdown tasks.
- A second launch while hidden, minimized, visible, or stopping does not start
  another product lifetime. During stopping, restoration is a bounded no-op.
- A missing window is recreated once; creation, show, focus, or unminimize
  failure remains a bounded diagnostic without starting another service.
- Settings directory creation, temporary write, flush, replace, permission,
  and serialization failures leave the prior live setting unchanged.
- Missing settings use defaults. Empty, oversized, malformed, wrong-version,
  and directory-at-file-path inputs recover conservatively without panic.
- Tray construction failure fails startup visibly rather than silently
  allowing a background policy with no way to restore or Quit.
- An installed Windows launch exposes exactly the product window and native
  shell surfaces; an accompanying console or terminal is a release defect.
- A manual update request before the React listener is attached is retained as
  one bounded pending request or prevented by initialization ordering.
- Updater unavailable/error/up-to-date/available outcomes remain visible in
  the existing update presentation. Tray interaction never installs or
  relaunches automatically.
- Destroyed-window cleanup remains safe when no subscription or view-resource
  registration exists and when shutdown already drained them.

## Implementation And Validation Stages

1. Record this contract, select Tactical `162` as the sole **Now**, and pause
   Tactical `158` without changing its signed-release outcome.
2. Add exact Tauri tray/image and single-instance dependencies; factor pure
   shell settings, close-decision, and shutdown-admission logic with focused
   tests.
3. Register the single-instance owner first, build the tray, implement window
   restoration and close interception, and route every explicit quit through
   joined shutdown.
4. Connect tray **Check for Updates** to the existing Tauri updater controller
   and visible About & updates state with listener cleanup and browser/demo
   isolation.
5. Add opt-in Windows x86_64 plus Linux x86_64/arm64 package retention and
   deterministic assertions where platform behavior can be checked without a
   headed session.
6. Run the complete local Rust/web floor and hosted credential-free matrix.
7. Install the retained Windows x86_64 and Linux arm64 packages in isolated
   machine-control campaigns; exercise the full stopping-condition matrix,
   inspect icons, and remove exact test installs/profiles/artifacts.
8. Reconcile exact evidence and remaining deferrals, complete this tactical,
   return `158` to **Now**, commit, push, and require hosted CI green.

## Deliberate Deferrals

- `.torrent` file associations, magnet/deep-link registration, and payload
  handoff from second-launch arguments;
- start-at-login, crash restart, launch-on-download, window geometry/state,
  multi-window presentation, and profile-selection arguments;
- dynamic tray status, speeds, notifications, pause/resume controls, and
  macOS menu-bar visibility policy;
- migration from released JSTorrent identity, settings, updater key, or
  application state; and
- force-kill recovery semantics beyond the existing profile/session contracts.
