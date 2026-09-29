# Tactical 235: Linux Quit / Launch Handoff

Status: **Active, 2026-09-29.** Parent campaign [231](231-jstorrent-migration-working-campaign.md);
follow-up to [234](234-desktop-user-intent-and-background-lifecycle.md).
Topics: `client-surfaces`, `runtime-configurations-and-headless-deployment`.

## Scope And Stopping Condition

Explain and repair the Linux launch immediately after Quit that returned
launcher success without a runtime/window in Tactical 234. Qualify ordinary
launch, OS magnet/file delivery and explicit background launch across the
shutdown boundary. Stop after deterministic/scripted builder tests and an
installed isolated Linux guest pass, with exact artifacts and cleanup recorded.
Update compatibility is the next separate bounded tactical; no importer,
personal migration, production identities/routes, publication or release claim.

## Invariants And Ownership

- One desktop application service/library owner. A waiting launcher must not
  construct an engine while another instance still owns admission.
- A launch admitted before Quit may be superseded by that Quit. Fresh explicit
  intent arriving during shutdown must be retained by its launcher until the
  old owner exits, or fail visibly after a bounded wait.
- No passive reconnect, startup observer or page restoration creates launch
  intent. No restart flag, durable pending launch, background relaunch helper
  or browser-profile routing is introduced.
- Preserve background versus native-window intent and the existing eight-input
  intake bound; never log magnet contents, torrent paths or credentials.
- Keep process arbitration in the Linux desktop adapter, outside application,
  protocol and torrent state. macOS/Windows and Android ownership are unchanged.

## Source Review And Plan

Pinned `tauri-plugin-single-instance` 2.4.3
`src/platform_impl/linux.rs::ExecuteCallback` returns no admission result.
Its secondary process discards the D-Bus call result and exits zero even if the
old primary disappears. `lib.rs::handle_external_activation_values` queues
inputs during Stopping, while `restore_main_window` deliberately does nothing.
This combination can lose post-Quit intent. Verify in the installed baseline
before selecting the final repair. No upstream source is copied.

Official [Tauri single-instance documentation](https://v2.tauri.app/plugin/single-instance/)
describes secondary launch notification and process exit. Review the exact
pinned source for behavior beyond that public description. Existing joined
shutdown owns picker, desktop control, view subscriptions, application service
and media termination; do not release singleton admission before those join.

1. Reproduce with the cached Tactical 234 DEB in a claimed, initially powered-off
   Linux guest, using a new task-owned HOME and real tray Quit.
2. Select bounded acknowledgement/retry behavior based on that evidence.
   Record timeout, admission limits, cancellation and failure ownership before
   adding runtime work.
3. Test startup, warm delivery, shutdown retry, unavailable bus, timeouts,
   concurrent launches and no-intent termination on the Linux builder first.
4. Install the changed package and repeat native/background/magnet/file cases,
   counting actual process/window effects rather than launcher exit alone.
5. Restore guest installation/profile state, power and claims. Commit the
   bounded result and carry update compatibility into its own tactical.

No engine, application contract, React or mobile behavior is changed by this
Linux process-admission investigation, so engine oracle and Android runtime
parity tests are inapplicable unless implementation scope changes.

## Execution Evidence

Initial repository: clean `2ddb0e27`, with Linux handoff `07a23248` present.
Linux builder: clean `bbe24600`; cached DEB SHA-256
`88b72903d8c8cda83e88a073ff83c0b875787484dcf14f41837dcc8b9608af85`.
Common Machine Control discovery/doctor finds the remote Linux target powered
off; exclusive claim and `target ensure-ready` produce a ready unlocked guest
through target-native administration/semantic routes. No inherited RSTorrent
package/process exists. Guest x86_64, Ubuntu glibc 2.39 matches the cached DEB.

Baseline evidence: twenty immediate full-process ordinary launches after real
tray Quit all survived as a replacement. Twelve tighter trials sent the exact
`org.SingleInstance.DBus.ExecuteCallback(as, s)` message after tray Quit:
all twelve observed the old singleton name still owned, a successful empty
reply, then old-process exit with no tray. These are installed IPC-boundary
trials, not twelve full OS association failures; they isolate the lost-admission
window that the slower launcher trials missed.

### Selected Repair And Resource Contract

Use a small independently written Linux desktop admission adapter over the
already-present zbus dependency. Retain the exact Tauri D-Bus name, object path,
interface, method and successful empty reply for old/new warm interoperability.
Return a named retry error while setup is incomplete or shutdown is stopping;
reject failed shutdown. New launchers retry name acquisition or delivery for
at most eight seconds, sleeping 25 ms between retry responses, with one-second
IPC call deadlines. Timeout/unknown/malformed replies fail closed, without
starting an engine or reporting successful delivery. Bus-owner disappearance
may retry acquisition; no forced replacement or queued name ownership.

The process owns one connection; export admission before claiming its name,
keep the name through joined service shutdown, and release on the terminal
Exit event. Waiting secondary processes own their original arguments and no
engine, tray, service, profile or helper. Terminating the launcher cancels its
wait. There is no persisted restart request. Bound each call to 64 arguments,
512 KiB aggregate argument bytes and 64 KiB cwd; existing intake limits still
apply. IPC errors never print caller contents. These are same-user session-bus
activation semantics, not extension authentication or a cross-user boundary.

Admission checks the atomic shutdown phase before queuing main-thread delivery;
a request accepted before Quit is allowed to be superseded by that Quit.
Requests observed after Stopping retry. Native creation stays on the main
thread. Linux warm inputs are classified directly once, without the upstream
plugin's implicit deep-link pre-dispatch; cold input remains the existing path.
macOS/Windows keep their pinned plugin unchanged. Old binaries retain their
old shutdown race; this repair cannot retrofit acknowledgement into them.

Required builder tests use an owned private D-Bus daemon and real message
exchange, covering warm delivery, startup/stop retry, owner disappearance,
concurrent arbitration, argument bounds, deadline and unavailable bus. No
installed product UI or host browser is used for those tests.
