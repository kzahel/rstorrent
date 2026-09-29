# Tactical 234: Desktop User Intent And Background Lifecycle

Status: **Active, 2026-09-29.** Maintainer-authorized implementation and
macOS/Windows/Linux qualification follow Tactical 232's installed checkpoint.

Parent: [232](232-desktop-extension-control.md), campaign
[231](231-jstorrent-migration-working-campaign.md).
Topics: `client-surfaces`, `runtime-configurations-and-headless-deployment`,
`application-connection-architecture`, `web-ui-design`, `capability-readiness`,
`desktop-jstorrent-replacement`, `product-surfaces-and-migration`.

## Outcome And Accepted Rules

The desktop runtime owns one library and its torrent engine. The extension's
packaged React tab and native window are independent views of that owner.
Closing or losing the browser UI does not stop the engine. The runtime must
retain its tray/status-bar icon throughout its lifetime, including idle,
paused and seeding states; no hide-icon preference or completion shutdown is
introduced. Notification permission and browser badges are not lifetime gates.
OS-controlled tray placement/overflow is distinct from the application hiding
its icon. Verify actual indicator availability on supported guest desktops.

| Trigger | Runtime intent | Presentation |
| --- | --- | --- |
| Actual extension toolbar action | Start or attach, including after Quit | Open/focus extension UI in the invoking browser context |
| Explicit Start in extension UI | Start or attach, including after Quit | Reconnect that UI |
| Browser/page restore, reload, retry, worker wake/start | Attach only | Existing extension UI; stopped runtime offers Start |
| Extension tab/browser closes | Preserve runtime | Tray remains; native window is independent |
| Tray Show/Open or ordinary desktop launch | Start/retain sole owner as applicable | Native desktop window |
| OS-delivered magnet or `.torrent` activation | Start/retain sole owner | Native intake UI |
| Explicit Quit | Joined shutdown; future explicit intent can launch again | Remove tray; connected browser shows Start |

No preferred UI, last-used surface routing, browser-profile discovery or native
attempt to launch a particular browser profile. Both views can coexist. The
existing default-on **Run in Background** setting continues to govern native
window close only; disabling it does not make browser disconnection an exit.

## Scope, Non-goals And Stopping Condition

Implement and test the intent boundary, same-owner recovery, persistent tray
and native routing on macOS, Windows and Linux. Stop after deterministic gates
and installed controlled-guest matrices pass, with exact artifacts, cleanup,
resource observations and remaining limitations recorded. Never infer installed
platform behavior from a shared unit test or another platform's success.

No importer, personal migration, profile selector, browser network interception,
new browser permissions, auto-shutdown policy, notification campaign, offscreen
engine, runtime architecture replacement, updater publication, production
identity or release claim. OS association dispatch is in scope; intercepting
all website magnet links inside the extension is not. Mixed-version refusal
and stale attachment remain tested; full installer/update compatibility and
production rollout remain separate campaign gates.

## Source Review And Design

Existing desktop tray construction is unconditional, including background
startup. Its Show action and admitted OS inputs already restore the native
window. Preserve those routes and qualify them without browser-profile logic.
The extension popup currently calls Launch on document initialization; a
restored/direct popup tab must not count as a toolbar click. Distinguish the
actual popup view before automatic launch; explicit buttons remain available.
Companion connection setup/retry remains attach-only and page ownership must
clean up across pagehide, restoration and disconnect during asynchronous mount.

Official inputs: Chrome [extension views](https://developer.chrome.com/docs/extensions/reference/api/extension#method-getViews),
[action popup](https://developer.chrome.com/docs/extensions/reference/api/action),
[worker lifecycle](https://developer.chrome.com/docs/extensions/develop/concepts/service-workers/lifecycle),
and Tauri [system tray](https://v2.tauri.app/learn/system-tray/).
`chrome.extension.getViews({type: "popup"})` returns actual popup window
objects; matching this document's window avoids treating a tab with the same
URL as a user action. No added permission or persisted launch-intent flag.
Tactical 162 owns prior tray/source review; Tactical 232 owns bounded bootstrap,
picker and native AppKit decisions. Engine/protocol behavior is unchanged, so
this slice adds no libtorrent algorithm/reference requirement.

## Ownership, Cancellation And Bounds

- Desktop shell owns the tray, native window, bootstrap, application service
  and joined shutdown. Browser connections cannot remove the tray or own exits.
- Extension worker owns bounded bootstrap/open requests and a remembered tab
  ID, never credentials or an engine. Worker restart does not replay Start.
- Each browser page owns one attachment attempt/connection, one mounted React
  view and one retry timer. Departure cancels that owner; late asynchronous
  completions must close/unmount, and restoration may attach but never start.
- Existing four-client, HTTP, frame, upload and single-picker limits from 232
  remain unchanged. No unbounded retry/task list or background observer added.
- Dependencies remain shell/extension adapters -> semantic application service;
  no engine dependency on tray/browser state. Android/Crostini keep their
  existing platform choices; desktop-only intent changes must not auto-launch
  either ChromeOS backend or alter mobile lifetime policy.

## Ordered Work And Evidence

1. Record these rules in living topics and the campaign; inspect source/tests
   and platform references before edits. Preserve clean handoff 7ade2df9.
2. Add regressions for real popup versus restored tab, explicit Start after
   Quit, attach-only automatic attempts, delayed mount/departure and restoration.
   Implement only observed ownership/intent gaps and tray failures.
3. Run builder extension/web gates and relevant native baseline. Package
   incrementally; verify architecture, deployment target/ABI and dependencies
   before installing. Preserve exact hashes/source snapshots.
4. Through claimed Machine Control guests, prepare fresh controlled roots and
   a private transfer. Prove cold/warm toolbar launches, native tray Open,
   browser closure with continued bytes, tab/worker/browser restart and current
   library recovery, Quit with no automatic resurrection, each explicit
   relaunch route, native close policy and OS magnet/file routing. Verify the
   tray while active and idle/paused, and its removal after Quit.
5. Repeat bounded lifecycle cycles and sample processes, handles/FDs, memory
   and connection cleanup. Check stale/incompatible admission and avoid guessed
   command success or automatic duplicate command replay after disconnection.
6. Restore inherited state, remove owned artifacts/processes, park guests and
   release claims. Reconcile evidence and remaining campaign gates; commit
   completed bounded changes without pushing.

## Restart Checkpoint

Both local repositories are clean at start. Machine Control c1bb89d includes
8480463's macOS session-probe descriptor fix and a 92-minute validation; the
previous resident issue is no longer an assumed blocker. Local macOS is
suspended and resumable. Local UTM Linux/Windows are unavailable; the existing
remote Linux controller has a clean RSTorrent checkout at 07a23248 and a
powered-off native Linux guest. Recheck remote Windows doctor/credentials.
Concrete routes and claims stay outside public documentation.

### First builder checkpoint

Actual popup identity now gates automatic toolbar launch. A direct/restored
popup tab may check setup but needs its Launch button; uncertain platform
identification never auto-launches desktop. The companion now gives a cached
page a fresh attach-only owner after its prior asynchronous mount/cleanup
finishes. Departure releases the ownership loop even if a frozen page never
receives the WebSocket close event. No launch state or credential is persisted.

After `source ~/.profile`: `npm test --prefix clients/extension` passes 42
cases plus package validation; `npm run typecheck --prefix clients/web` passes;
`npm test --prefix clients/web` passes 413 cases with two skipped. Eight focused
companion cases cover automatic retries, explicit Start/double click,
authentication refusal, mount failure, disconnect cleanup and cached-page
restoration including delayed mount and absent socket-close delivery.
`npm run package --prefix clients/extension` passes. These are deterministic
builder results; installed cross-platform evidence remains open.

### Installed macOS activation finding

An OS magnet delivered to a background owner with no native window deadlocked
its main thread before creating that window. A guest `sample` shows
`tauri-plugin-deep-link::on_event -> emit -> on_open_url ->
restore_main_window -> prepare_pending_webview -> plugins.lock`. Tauri 2.11.5
holds its plugin-store lock during plugin event delivery; webview creation
needs that same lock. `run_on_main_thread` executes inline on the main thread,
so wrapping the callback with it would retain the deadlock.

Handle macOS file and magnet URLs in the application's `RunEvent::Opened`
callback, after Tauri finishes plugin delivery and releases that lock. Keep
the deep-link callback on Windows/Linux, where the single-instance plugin
forwards URL events outside this macOS plugin dispatch. This adds no task,
queue, browser-routing state or retry. Installed regression must start without
a native window, deliver the OS magnet, then prove native presentation,
responsive commands, same owner and joined Quit; repeat cold file/magnet intake.

The macOS correction passes `cargo test -p rstorrent-desktop --lib` (53),
`cargo clippy -p rstorrent-desktop --all-targets -- -D warnings`, and formatting.
The rebuilt unsigned debug desktop SHA-256 is
`4c57171f2be1ff89e130612e8cca5ecb06525149536ea0e7333603b5b258b409`.
Installed reproduction now passes: cold extension toolbar -> absent native
window -> OS `open 'magnet:?xt=urn:btih:<fixture-hash>'` -> native window and
Already in your session, with the same runtime/library and responsive snapshot.
Native Cmd-Q then cold OS `open <fixture.torrent>` starts a new owner and native
window, restoring the same verified 512-piece fixture. The hung pre-fix test
process required targeted termination; it is not counted as Quit evidence.

### Desktop connection resource contract

Rapid installed Linux reloads exhausted the global 32 view-set limit: each
new page creates a fresh client identity, and departed sets remain leased for
five minutes. This is bounded retention, not an unbounded memory leak, but it
makes repeated desktop UI reopening unavailable. The companion deliberately
rebuilds its view on reconnect and cannot resume a departed page's view set.

Desktop-control disconnect therefore retires that authenticated connection
owner's view sets after joining calls and attachment pumps, before releasing
its registry generation. Same-client takeover waits for this cleanup. Other
owners (including the native UI) remain intact. Ordinary headless/remote and
ChromeOS connections retain their existing resumable leases; no wire change,
engine change or Android policy change is needed. Resource limits stay 32
sets globally/eight per owner and four desktop connections. Validate more than
32 create/drop cycles, an unattached set, same-client takeover, unaffected
second-owner state and waiter termination before repeating installed cycles.

The focused owner-retirement and 64-client desktop WebSocket regressions pass
on the macOS builder (`cargo test -p rstorrent-session retiring_one_owner`
and `cargo test -p rstorrent-gateway departed_pages_and_takeover`). Full
session/gateway suites and final installed rebuilds remain in progress.
