# Tactical 164: Desktop Completion And Attention Notifications

Status: **Complete (2026-08-25).** Desktop signed packaging/updater Tactical
[`158`](158-desktop-signed-packaging-and-updater.md) resumes as the sole
**Now** without changing its release outcome.

Topics: `beta-release-readiness`, `client-surfaces`,
`product-state-and-feedback`, `client-persistence`, `application-view-api`,
`web-ui-design`

Dependencies: completed desktop lifecycle Tactical
[`162`](162-desktop-single-instance-and-tray-lifecycle.md), completed external
intake Tactical [`163`](163-desktop-external-torrent-intake.md), the maintained
in-process `ApplicationService`, and the existing Tauri/React product.

## Decision And Desired Outcome

Add basic native desktop notifications for a completed download and a torrent
that enters a fatal or repair-required condition. The Tauri Rust shell owns
notification policy and delivery. React exposes only typed desktop settings;
it neither observes torrent rows to manufacture events nor receives authority
to submit arbitrary operating-system notifications.

Pin and initialize the standard `tauri-plugin-notification` package in the
desktop Rust host. Its standard Rust backend owns macOS and Windows delivery.
Installed Linux evidence found that the package submits through
`notify-rust` and immediately drops its handle; GNOME then retains no visible
notification and exposes no activation callback. Linux therefore uses the
same exact `notify-rust` version directly, retains the handle in a bounded
joined owner, and routes its default action through the existing window
restoration function. This is a recorded platform adapter, not webview
authority or a generic second event system. Android and iOS keep their
independent first-party platform owners and are outside this Tauri-only slice.

Notifications are edge-triggered, best-effort presentation. Startup, webview
creation, subscription reset, application restart, settings enablement, and
restoration of already-terminal torrent state never replay a notification.
The ordinary default is to notify even while the main window is visible or
focused. A desktop preference may instead suppress notifications only while
the main window is focused; a visible window behind another application is
not focused and remains eligible.

There is no initial completion aggregation, notification history, progress
surface, or action vocabulary. One eligible torrent edge produces one native
notification.

## Scope And Stopping Condition

This tactical owns:

1. one exact pinned compatible `tauri-plugin-notification` dependency,
   initialized for Rust-side use without granting its general notification
   commands to the webview, plus the exact Linux `notify-rust` dependency
   required to retain GNOME notifications and their activation handle;
2. a pure bounded notification-policy reducer over authoritative
   application-service torrent-list snapshots and patches;
3. one desktop-lifetime owner for the application view subscription, policy
   state, native delivery, cancellation, and observed termination;
4. completion and fatal/repair-required edge semantics with initial-state,
   restart, recheck, recovery, removal, reset, and duplicate suppression;
5. installation-wide desktop settings for completion, attention, and
   focused-window behavior, plus an explicit version-1-to-version-2
   `desktop-shell.json` migration preserving `run_in_background`;
6. a capability-gated **Notifications** Settings category in the Tauri
   product only, with immediate durable toggles and visible failure feedback;
7. bounded privacy-preserving titles/bodies and diagnostics that never expose
   raw error text, paths, hashes, trackers, peers, endpoints, or source URIs;
8. best-effort activation through the installed platform and existing
   single-instance/window-restoration owner, including the bounded direct
   Linux activation task;
9. deterministic Rust and React coverage, unchanged browser/demo behavior,
   package validators, and the normal credential-free hosted matrix; and
10. installed macOS arm64, Windows x86_64, and Linux arm64 evidence for native
    display, settings, foreground policy, close-to-tray completion, terminal
    attention, non-replay, and actual notification-click behavior. Linux
    x86_64 remains a native package gate, and installed Intel macOS remains
    deliberately omitted.

The tactical stops when an installed package on every required test platform
shows exactly one notification for one controlled incomplete-to-complete
download edge and one newly entered fatal/repair edge, honors every setting,
does not notify for any initial or replayed terminal state, continues while
the webview is hidden, and leaves no RSTorrent-owned notification task after
joined Quit. Actual click activation is recorded on every platform. A
platform adapter outside the standard package is permitted only when its
installed failure is recorded here together with bounded ownership and
cleanup; Linux met that condition, while macOS and Windows retain the
standard package and its measured click limitation.

## Non-Goals

- Android foreground, completion, or error notifications and iOS local or
  remote notifications.
- Progress, percentage, queue, start, pause, seeding, tracker, peer, update,
  duplicate-add, or file-open notifications.
- Aggregation, grouping, rate limiting, notification-center history, badges,
  scheduled notifications, sounds, custom actions, or per-torrent policy.
- Persisting a notification ledger, replaying a notification missed during a
  crash, or treating notification delivery as application correctness.
- A generic application event framework, installation `product.db`, product
  counters, analytics, crash reporting, or prompt campaigns.
- Exposing `notification:default`, the package JavaScript API, arbitrary
  title/body input, or a general notification command to the webview.
- Adding `mac-notification-sys`, WinRT activation code, a forked Tauri plugin,
  or another supplementary platform backend. The bounded Linux direct
  `notify-rust` adapter described above is the sole exception.

## Reference Inspection

### Maintained JSTorrent product reference

The maintained sibling JSTorrent checkout at revision
`9598770baecb1164a00ba5d41f7e7c11bfb78828` was inspected on 2026-08-25:

- `packages/engine/src/config/config-schema.ts` defaults completion and error
  notifications on and keeps extension-only background progress off;
- `packages/client/src/engine-manager/daemon-engine-manager.ts` subscribes to
  `torrent-complete` in the UI-side engine manager;
- `packages/client/src/chrome/notification-bridge.ts` forwards completion,
  error, duplicate, visibility, and throttled progress messages to its host;
- `packages/client/src/host/tauri-channel.ts` checks local settings and invokes
  one native `show_notification` command;
- `desktop/tauri-app/src-tauri/src/lib.rs` uses
  `tauri-plugin-notification` on Windows, `mac-notification-sys` on macOS, and
  direct `notify-rust` on Linux so the latter two can wait for a click and show
  the main window; and
- `desktop/tauri-app/src-tauri/Cargo.toml` plus `desktop/Cargo.lock` resolve
  `tauri-plugin-notification` `2.3.3`, `mac-notification-sys` `0.6.15`, and
  `notify-rust` `4.18.0`.

The current JSTorrent error bridge and native host branch exist, but no call
from its engine manager to `onTorrentError` was found. RSTorrent therefore
does not treat the presence of that branch as error-notification evidence.
It adopts the familiar completion/error defaults and useful window-activation
goal, not JSTorrent's UI-driven event owner, raw error body, per-notification
detached thread, extension/native-host topology, or mixed backend.

### Standard Tauri notification package

Official Tauri v2 notification documentation and exact crate
`tauri-plugin-notification` `2.3.3` were inspected on 2026-08-25. The crate is
MIT OR Apache-2.0 and supports the repository Rust baseline. Its desktop
`src/desktop.rs`:

- constructs a `notify_rust::Notification` behind the public Rust extension;
- associates installed Windows/macOS notifications with the configured
  application identifier;
- submits display work through the Tauri async runtime;
- reports desktop permission as granted; and
- exposes no desktop click callback or desktop action listener through the
  public builder.

The package can therefore own macOS and Windows display but cannot itself
guarantee an explicit `restore_main_window` callback. Installed evidence found
that neither platform routed the notification click to the existing owner.
The package's Linux wrapper additionally dropped the native handle before
GNOME could retain the notification. The direct Linux adapter uses the same
underlying exact package version, retains its handle, and successfully routes
the default action. The tray's existing **Show RSTorrent** remains the
guaranteed restoration fallback on macOS and Windows.

There is no BitTorrent protocol or libtorrent reference requirement for this
platform integration. No JSTorrent or Tauri source, fixture, or asset is
imported.

## Product, Privacy, And Settings Contracts

- Fresh desktop settings default `notify_download_complete`,
  `notify_needs_attention`, and `notify_while_focused` to true.
- **Download completed** controls only an eligible canonical completion edge.
  **Needs attention** controls both fatal-error and storage-repair edges.
- **Notify while RSTorrent is focused** controls only focused-window
  suppression. When false, hidden, minimized, or visible-but-unfocused windows
  remain eligible. When true, window state does not suppress an edge.
- Preference changes are installation-wide shell policy. They do not enter a
  profile database, generated application contract, torrent record, browser
  local storage, or future analytics state.
- Schema version 2 explicitly decodes version 1 and preserves its exact
  `run_in_background` value while adding notification defaults. Wrong future
  versions and malformed/oversized input retain the existing fail-closed
  repair behavior.
- Persist the complete next settings value atomically before updating visible
  UI state. A failed write leaves the prior live policy active and gives
  bounded visible feedback.
- Enabling a notification setting does not scan for or replay an existing
  complete, error, or repair row. Disabling a setting still advances edge
  state, so re-enabling cannot replay an edge suppressed while disabled.
- A completion title is **Download complete** and its body is the bounded
  application display name, falling back to **Torrent**. An attention title is
  **Download needs attention** and its body is the same bounded display name
  plus generic **Open RSTorrent for details** wording.
- Native bodies never contain `TorrentView.error`, a storage path, source URI,
  tracker URL, peer address, hash, installation ID, or diagnostic detail.
- Operating-system notification-center retention and lock-screen presentation
  are platform policy. RSTorrent retains no parallel notification history.
- macOS may initially register RSTorrent with notifications disabled and show
  its own notification-permission notice instead of product content. Enabling
  notifications is an operating-system choice. The edge that caused the
  notice is not replayed; a later eligible edge displays normally.

## Edge Semantics

The native owner consumes the authoritative in-process torrent-list view. It
does not depend on React, a webview event, document visibility, polling, or a
second application service. A pure reducer holds one bounded entry per current
torrent ID and accepts complete snapshots, upsert patches, removals, and reset
boundaries.

### Baseline and reset

- The first complete snapshot establishes state without output. Existing
  Complete, Error, and NeedsRepair rows do not notify.
- A snapshot or reset received after lag/recovery also establishes a new
  baseline without output. No attempt is made to reconstruct missed edges.
- An incomplete Downloading row in a baseline may become eligible for a later
  completion, but display requires newly observed payload/verification
  progress after that baseline. An incomplete torrent restored after process
  restart therefore notifies only if it subsequently completes through
  ordinary download work observed in the new process lifetime.
- A removed torrent immediately drops its policy entry. A bounded FIFO of the
  256 most recently removed torrent IDs prevents a coalesced remove/re-add
  from replaying terminal state; later ordinary progress may establish a new
  edge normally.

### Completion

- The reducer retains bounded previous payload counters and verified-piece
  count. Eligibility arms when an ordinary non-checking generation observes a
  payload-byte or verified-piece increase associated with Downloading work.
  It remains armed through pause/resume and AwaitingPublication for that work
  generation. Merely observing the Downloading enum is insufficient.
- Entering Checking clears completion eligibility. A later return to
  Downloading may arm a new ordinary download edge. Force recheck and startup
  verification can therefore reach Complete without fabricating completion.
- Emit only when an armed row first reaches both `TorrentState::Complete` and
  `StorageState::Published`. Intermediate verified, prepared, or publication
  state is not completion.
- Evaluate ordinary progress evidence before terminal state in one upsert, so
  a coalesced final Downloading-to-Complete patch with new verified/payload
  work still emits exactly once.
- Emission consumes the edge. Repeated full rows, patch coalescing, view-set
  replacement, seeding activity, archive/restore, and a Complete-to-Checking-
  to-Complete recheck do not emit again.
- A later genuinely incomplete generation that re-enters Downloading and
  reaches canonical completion may notify again. Recheck alone never rearms
  it.

### Fatal and repair-required attention

- Emit once when a nonterminal row newly enters `TorrentState::Error`,
  `TorrentState::NeedsRepair`, or `StorageState::NeedsRepair`.
- Changes to raw error text while the row remains in the same attention class
  do not emit again.
- Recovery to a nonterminal, non-repair state clears the attention latch. A
  later distinct terminal transition may emit a new notification.
- AwaitingMetadata, AwaitingStorage, Paused, stalled/no-peer progress, tracker
  retry, and ordinary recoverable network failure are not fatal attention
  edges.
- Completion and attention are evaluated independently, but one row update
  cannot emit both.

## Ownership, Task, And Dependency Direction

```text
ApplicationService authoritative TorrentList subscription
  -> desktop NotificationPolicy reducer
     -> zero or one typed DesktopNotification edge
        -> current DesktopShellSettings filter
        -> macOS/Windows: tauri-plugin-notification Rust extension
        -> Linux: retained notify-rust handle + bounded activation task
        -> operating-system notification center

Tauri Settings capability
  -> constrained read/replace desktop-notification commands
  -> versioned atomic desktop-shell.json owner

notification/app activation, where provided by the OS
  -> existing Tauri run event or single-instance owner
  -> existing restore_main_window

joined desktop shutdown
  -> cancel and await notification subscription owner
  -> close other presentation resources
  -> await ApplicationService and media-server shutdown
  -> final process exit
```

The reducer is a plain desktop-shell module independent of Tauri, React,
filesystems, sockets, and async runtime types. The adapter depends inward on
it. The notification owner has one cancellation token and one observed join
handle; its application-view subscription retains the existing bounded queue
and reset behavior. Native display errors are bounded diagnostics and never
block or roll back torrent state.

The Settings surface receives a narrow `DesktopNotifications` controller only
from Tauri bootstrap, following the updater capability-injection pattern.
Browser, demo, authenticated remote web, Android, and iOS builds omit the
category. The webview may read and replace the three typed preferences, but it
cannot provide notification content or call the notification plugin.

## Failure And Race Cases

- Completion/error before the initial snapshot, during process startup, or
  while the owner is resetting is not replayed.
- Completion after a baseline Downloading row is delivered once even when the
  main webview is destroyed or hidden behind the tray.
- Duplicate/coalesced upserts and out-of-date raw error changes do not notify.
- Disabling a category just before an edge suppresses delivery without
  retaining pending output; enabling it afterward does not replay.
- Focus changes race safely with an edge. The setting and one best-effort
  native focus observation are read once for that edge; there is no delayed
  retry when focus changes.
- A missing/destroyed main window does not prevent display. A later OS
  activation uses the existing generation-fenced restoration path.
- Native notification submission failure records only category/platform and a
  bounded error category. It does not retry, block downloads, or surface raw
  backend text to the notification body.
- A lagged or closed view subscription rebaselines without replay. Unexpected
  closure during normal operation is observable and terminates its owner; it
  does not spin or create a replacement loop without a bounded policy.
- Joined Quit cancels and awaits the owner before the application service is
  shut down. No RSTorrent-owned subscriber survives final exit or updater
  restart.

## Implementation And Validation Stages

1. Record this contract, select Tactical `164` as the sole **Now**, and pause
   Tactical `158` without changing its signed-release stopping condition.
2. Pin and license-audit `tauri-plugin-notification`; initialize it only in
   the Tauri host and keep notification plugin permissions out of the main
   webview capability.
3. Implement the pure edge reducer and exhaustive snapshot/patch/reset tests
   before starting a runtime owner.
4. Add the one subscribed desktop owner, bounded diagnostics, cancellation,
   and joined shutdown ordering. Keep edge and preference decisions in pure
   deterministic tests; exercise real native sinks only in installed packages.
5. Migrate desktop shell settings to version 2, expose the narrow Tauri-only
   controller, and add the capability-gated Settings category with component
   and bootstrap-isolation tests.
6. Extend package/release validators and run the proportional local Rust/web,
   browser/demo, Tauri compile/package, and hosted platform matrix.
7. Run installed macOS arm64, Windows x86_64, and Linux arm64 campaigns with a
   controlled torrent and controlled terminal fault while focused, unfocused,
   and tray-hidden. Record native title/body/icon, exact counts, settings
   persistence, non-replay after relaunch, click behavior, joined Quit, and
   cleanup. Drive the shared test targets through
   `~/code/machine-control/bin/machine-control` and its platform guides: run
   inventory and target doctors first, hold one exclusive claim per target,
   preserve each target's inherited power state, and keep private inventory
   values out of repository evidence. All UI input, capture, and window
   inspection must use the common guest-resident `desktop` routes and report
   `hostInterference: "none"`; do not drive a host-side testbed window or call
   Tart directly. Keep Intel macOS and Linux x86_64 installed omissions
   explicit.
8. Reconcile the focused topics and release checklist, mark this tactical
   complete only from recorded evidence, and resume Tactical `158` as the sole
   **Now**.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure reducer | Initial terminal baseline; restarted incomplete baseline; zero-work Downloading-to-Complete suppression; ordinary payload/verified progress through Downloading/AwaitingPublication/Complete+Published; repeated complete; checking/recheck; repair download; fatal and storage repair entry; repeated detail; recovery/re-entry; removal/re-add; reset; category disable/enable. |
| Shell settings | Fresh defaults; exact v1-to-v2 preservation; v2 reopen; malformed, oversized, unknown-version, write-failure, and concurrent-read behavior. |
| Native owner | Pure preference filtering; subscription reset/close and cancellation ownership by inspection; installed focused/hidden policy, native failure behavior, and observed process exit after joined Quit. |
| React | Tauri-only Notifications category; three accessible controls; immediate successful persistence; failed-save rollback/feedback; wide, compact, and phone layouts; zero serious/critical Axe violations; browser/demo omission. |
| Package | Exact dependency/init; no webview plugin permission; product identity/icon; Windows GUI subsystem; macOS/Windows/Linux package build; dependency/license review. |
| Installed desktop | One real completion and one real fatal/repair edge; focused/unfocused/hidden policy; disable/enable; restart non-replay; native title/body/icon; observed click result; tray restoration fallback; joined Quit and cleanup. |

## Stopping Record

Complete on 2026-08-25:

- `tauri-plugin-notification` is pinned exactly at `2.3.3`, initialized in the
  Rust host, and absent from webview capabilities. Linux alone pins direct
  `notify-rust` `4.18.0`; license review found the same MIT/Apache-compatible
  dependency already underlying the standard package.
- The pure reducer has nine focused tests covering initial/reset suppression,
  zero-work and coalesced completion, metadata-to-terminal coalescing,
  checking/repair generations, attention recovery/re-entry, removal/re-add,
  new runtime terminal rows, and bounded private content. Desktop-shell tests
  cover fresh settings, exact version-1 migration, atomic replacement, repair,
  and write failure. React tests cover the Tauri-only controller/category,
  successful persistence, failed-save feedback, and browser omission.
- Installed Windows x86_64 under Windows 11 arm64 x64 emulation displayed
  completion and attention notifications with the product title/body/icon,
  honored focused suppression, persisted settings, did not replay after
  enablement or restart, and exited cleanly. A notification click did not
  restore the window; the tray fallback did.
- Installed Linux arm64 GNOME/Wayland displayed a tray-hidden completion for
  one exact 4,195,035-byte loopback torrent and a fresh attention edge, honored
  focused suppression without replay, migrated and persisted version-2 shell
  settings, and suppressed terminal rows after restart. Retaining the direct
  native handle kept the notification visible; its default action restored the
  existing window. Quit closed live handles, cancelled and joined activation
  tasks, and left no process.
- Installed macOS arm64 displayed focused and tray-hidden completion plus a
  fresh **Download needs attention** notification with the exact generic body
  after the OS permission was enabled. Category/focus settings, re-enable,
  restart non-replay, tray restoration, Quit, and cleanup passed. A standard
  package notification click did not restore the hidden window. The final
  attention rerun used only machine-control's guest-resident desktop routes;
  every UI result reported `hostInterference: "none"`.
- macOS arm64, Windows x86_64, and Linux arm64 installed packages were cleaned
  exactly, target power state was restored, and exclusive machine-control
  claims were released. Linux x86_64 remains a native compile/package gate;
  installed Intel macOS remains deliberately omitted.
