# Tactical 232: Desktop Extension Control

Status: **Linux and Windows follow-up in progress, 2026-09-29.** The first
Linux checkpoint is complete. Windows protected bootstrap passes native guest
tests; picker implementation and scripted checks are recorded below. Installed
picker/package and complete Rehearsal A gates remain open. macOS verification
is explicitly deferred to a later session.

Parent: [`231-jstorrent-migration-working-campaign.md`](231-jstorrent-migration-working-campaign.md)

Topics: `desktop-jstorrent-replacement`, `product-surfaces-and-migration`,
`application-connection-architecture`, `runtime-configurations-and-headless-deployment`,
`client-surfaces`, `web-ui-design`, `application-view-api`,
`download-root-acquisition`, `product-state-and-feedback`, `capability-readiness`.

## Outcome And Stopping Condition

The beta extension opens the mature shared React UI and controls the installed
RSTorrent desktop app's existing Rust application service. An extension-first
cold launch does not force the native main window open. Opening that window
later attaches to the same library. Neither switching views nor closing the
browser transfers engine ownership.

There is one desktop library. Multiple browser profiles or authorized extension
installations create client connections, not desktop profiles. No profile
create/select/switch or takeover operation belongs to this slice. Internal
backend/profile identity remains useful for stale-connection validation only.
Legacy profiles will be unioned by the separate importer, not represented as
selectable backends in this UI.

Stop after Rehearsal A in Tactical `231` passes on installed macOS, Windows and
Linux packages, with exact beta-extension identity, controlled byte integrity,
two-view convergence, picker behavior, detach/reconnect, admission failures,
resource bounds and joined shutdown. Record partial platform progress honestly.
This slice uses fresh test profiles; legacy import and production publication
remain later campaign gates.

## Accepted Launch And Connection Rules

Maintainer accepted the simplified model during the migration discussion:

- Clicking the extension opens/focuses its browser UI and starts or attaches
  to the sole desktop runtime without forcing its native window open.
- Ordinary desktop launch opens/focuses the native window on that same runtime.
- Both views may coexist. There is no preferred-UI setting, takeover prompt,
  automatic closing of the other view, or browser-owned data authority.
- Desktop connection setup should be automatic through the installed approved
  native-messaging integration, without routine code-entry pairing. The selected
  protected bootstrap and credential contract is recorded below; finding a
  port or checking Origin alone is insufficient.
- Reconnect may attach to a running/restarting owner, but must never start a
  stopped runtime on its own. Only explicit user open/start requests may launch
  it. Explicit Quit leaves the extension disconnected with a Start action.
- Preserve native close/background policy. Desktop shell actions such as
  notifications and manual update checks retain their existing native routing
  initially; adding remembered presentation preference is outside this slice.

## First Implementation Checkpoint

Build one vertical path: **click extension -> ensure one desktop runtime ->
authenticate -> display its library -> pause/resume a test torrent**. Reuse the
existing React application and native service. This is an intermediate gate
within this tactical, not its full cross-platform stopping condition.

Implementation order:

1. Separate explicit desktop-window activation from background start/attach
   intent in the singleton launch path. Race-test simultaneous requests and
   make automatic reconnect attach-only so it cannot undo explicit Quit.
2. Close and implement the protected bootstrap/authenticated connection contract
   with a typed ready response and bounded failure states. Authenticate and
   validate backend/protocol identity before exposing library state.
3. Open/focus the extension's packaged React page on that connection. Use a
   controlled fresh library/root prepared through the existing native UI for
   this first checkpoint; native picker from the extension follows next.
4. Prove cold launch without a forced native window, warm attachment, the same
   library from two views, pause/resume convergence, repeated-click singleton
   behavior, invalid-credential refusal and Quit without automatic resurrection.

Use deterministic/scripted checks first, followed by a claimed installed
rehearsal through Machine Control. The recommended development loop is a native
Linux x86_64 builder and a matching Linux VM, subject to a successful guest
readiness check; the previously ready macOS VM is a fallback and subsequent
platform gate. Then add extension-driven folder selection,
magnet/metainfo intake, full lifecycle/failure coverage and remaining platform
installed gates. Do not infer migration readiness from this first checkpoint.

## Build And Test Placement

Prefer incremental Rust/web builds on a Linux development controller with a
retained build cache, then transfer the exact application bundle/native host
and packaged extension to its claimed Linux VM. Matching architecture avoids
cross-compilation but does not prove library compatibility: verify the guest
ABI/dependencies and package launch. If those differ, use a compatible build
environment or build inside the VM. Later package gates exercise the actual
installer rather than only copying a development executable.

Keep installed-host registration, test browser, profiles, associations and
lifecycle experiments inside the VM. Use deterministic tests on the builder
and target-native Machine Control operations for guest evidence. Source transfer
must preserve unrelated remote work, identify the exact tested snapshot, and
avoid copying build caches between architectures. An agent may coordinate from
another host over SSH; moving its session into the VM is unnecessary.

The 2026-09-28 read-only alternate-controller survey finds native Linux Rust,
Node via NVM, GTK/WebKit development dependencies and an existing build cache.
Linux/Windows VM routes resolve there but both guests are off; no running-guest
readiness is claimed. Concrete controller selection remains private inventory.
Before use, doctor, claim, start through the common CLI and verify resident
readiness; retain inherited power state and release claims after cleanup.
Qualify macOS separately through its Apple-hosted VM and Windows through its
native target; Linux success does not cover those launch/IPC/picker paths.

## Existing Seams To Reuse

Reviewed in the current working tree at RSTorrent HEAD
`7a4d7730920f84ed0c02374506b0cd4af1f26f2d`:

- `clients/extension/src/service-worker.js`: typed native `hello`/`launch`,
  platform-specific open/focus and remembered extension tab lifetimes.
- `crates/rstorrent-native-host/src/lib.rs` and `tests/process.rs`: bounded
  stdio bootstrap and configured platform launcher. It owns no profile.
- `clients/desktop/src-tauri/src/{lib,desktop_lifecycle,native_host_registration}.rs`:
  application owner, single-instance/window/tray/shutdown and host registration.
  Root selection currently requires a `WebviewWindow` parent. A windowless
  launch is a concrete missing platform seam, not just another launch flag.
- `clients/web/src/{companion-main,android-companion-client}.ts` and
  `inspection/companion-bootstrap.tsx`: packaged React mounting, backend identity,
  transport/platform adapter and disconnect recovery. The current mount hardcodes
  `one_current_root`; do not silently impose Android presentation policy on
  desktop while sharing it.
- `crates/rstorrent-gateway/src/chromeos_companion.rs`: bounded identity/pairing,
  platform requests and application-connection composition. Its ARC address and
  Android trust model must remain platform-specific.
- Tacticals `162`, `166`, `194`, and the connection/runtime/root-acquisition
  topics: lifecycle, host bounds, tested Android pattern and native picker gaps.

Reuse the application client/reducers and authenticated connection machinery
where ownership fits. Extract a concrete shared mount or connection component;
do not fork the UI or add a generic backend/plugin framework. Keep new desktop
bootstrap/lifecycle policy out of protocol and engine crates.

## Scope And User Journeys

1. Desktop popup open/focus presents the extension UI. Keep an explicit action
   to open the native window. Show actionable missing-app, incompatible-version,
   stopped, connecting and disconnected states without displaying raw endpoints
   or profile IDs as routine product guidance.
2. Native messaging locates or starts the existing desktop singleton and
   establishes narrowly scoped extension control. Concurrent cold launches
   converge before a second application service or profile is constructed.
3. The extension uses ordinary semantic commands/views for add, file selection,
   pause/resume, queue, recheck, keep-data/remove, settings and inspection.
   Bounded metainfo attachment remains distinct from torrent payload IO.
4. Root choose/repair runs in the desktop OS. The extension receives the root
   snapshot, never ambient filesystem capability. Test no-window parenting,
   cancellation, foreground activation, disconnect and app shutdown.
5. Native and browser views converge after commands from either. Closing or
   suspending a view releases its leases; reconnect obtains authoritative state.
   A lost reply is not permission to replay a destructive command blindly.
   Two authorized browser profiles must also see the same desktop library;
   neither may cause another catalog to be created or selected.
6. Desktop shell owns background policy, native notifications, updater and Quit.
   The extension cannot become necessary to keep the engine alive. Preserve
   existing native-window close semantics or explicitly settle any change.
7. Provide bounded connection/backend diagnostics and an accessible report
   action. Never record bootstrap secrets in URLs, command arguments or logs.

Core torrent control is the first slice. Before claiming full legacy workflow
parity, inventory media/open-file actions, magnet interception, context menus,
search/plugins and per-presentation preferences. Unsupported capabilities must
be hidden or explained accurately and tracked in Tactical `231`; silently
inheriting Android's no-media capability profile is not desktop parity.

## Transport Design Checkpoint

Selected composition (detailed contract and evidence below): a thin native-messaging bootstrap plus
an authenticated loopback semantic connection hosted by the existing desktop
process, reusing the WebSocket application client. It must not instantiate a second gateway
application service or reuse the media-only listener as ambient control access.

Compare with carrying semantic frames over a persistent native-messaging
bridge to local IPC. Evaluate binary metainfo attachments, frame amplification,
backpressure, service-worker lifetime, OS IPC authentication, packaging and
maintainer complexity. The existing host's 64-KiB bootstrap limit and the
application's larger binary attachment path cannot simply be conflated.

Before coding the chosen transport, record exact official Chrome native-
messaging, MV3 lifecycle/CSP/network-permission documentation and locked Tauri
single-instance/window/dialog source and tests. Resolve:

- how the bootstrap proves the installed owner and same-user caller, including
  protected local rendezvous/IPC and malicious port prebinding;
- endpoint selection, binding, exact extension-origin/Host checks, browser
  permission/CSP scope, and proof that another LAN host cannot control it;
- credential delivery, lifetime, storage, revocation and restart rotation;
  Origin validation alone is not authentication;
- backend/profile/protocol identity and behavior on stale state or skew;
- native-host EOF, handshake deadlines, retry/backoff and cancellation;
- per-connection and global limits for views, queues, outstanding calls,
  attachment buffers, concurrent clients and bootstrap work; and
- shutdown ordering and explicit handling of authentication failure versus
  ordinary temporary connection loss.

Retain current semantic frame/attachment limits unless a separately justified
contract change is needed. Set numerical control-specific bounds before the
implementation checkpoint closes; no unbounded queues or pending requests.

## Owner And Cancellation Map

| Owner | Responsibility | Termination |
| --- | --- | --- |
| Extension worker | Open/focus and short bootstrap work | Worker suspension is tolerated; it owns no engine or persistent view subscription |
| Extension React page | One client connection, reducers and view leases | Page close/abort releases connection and pending work; fresh page recovers state |
| Native host | Validated launch/rendezvous or chosen bounded bridge | EOF/error/deadline closes owned resources; desktop survives presentation detach |
| Desktop shell | Singleton, optional window, tray, updater and control owner | Existing joined Quit/restart path stops admission and joins child owners |
| Desktop control owner | Credentials/connections/platform requests around the existing service | Revocation closes admitted clients; shutdown cancels and joins pumps and picker requests |
| Application service | Existing profile, commands, views, engine and storage | Existing supervised shutdown; no browser lifetime dependency |

Dependency direction: presentation and desktop/transport adapters depend on
the common semantic application boundary; protocol/domain state does not depend
on Tauri, browser runtime, sockets or the native host. Avoid growing desktop
`lib.rs` with a second independent connection/authentication subsystem; extract
the new owner at its actual lifetime boundary.

## Lifecycle Decisions To Close

- Implement genuinely windowless cold startup where feasible; a hidden but
  instantiated webview must be reported as such, with its observer cost.
- Determine how extension requests raise a native folder picker without forcing
  the product window and how unavailable desktop sessions fail visibly.
- Specify behavior when background mode is disabled, the last extension page
  closes, or a native window closes while an extension remains attached. Preserve
  ordinary close behavior; do not invent browser-driven engine termination.
- Keep existing native notification/update routing without a preferred-view
  setting. Verify it remains reachable from extension-first startup and retains
  one updater and notification authority without duplicate prompts.
- Distinguish explicit Quit, restart for update and transient disconnect, with
  reconnect limits and truthful recovery messages.

## Implementation And Validation Order

1. Close the design checkpoints with source-backed contracts, exact bounds and
   deterministic state tests planned. Record the capability disposition matrix.
2. Implement native start/attach plus authenticated hello against a fresh
   in-process service. Prove singleton races, invalid credentials, stale owner
   identity and cancellation before connecting the full UI.
3. Connect the shared React application and native folder capability. Prove
   two-view convergence, metainfo attachment, one controlled transfer and exact
   hashes through the headless/scripted harnesses first.
4. Exercise browser suspend/restart, socket loss, slow consumers, revoked access,
   malformed/oversized input, picker interruption and joined shutdown. Measure
   idle detached observer cost and active resource peaks.
5. Run installed Rehearsal A through Machine Control on each desktop platform.
   Test packages/registration, picker focus, cold launch, tray and update restart
   using target-native observations. Use the test browser, not a temporary
   profile under the user's primary browser identity.
6. Run existing extension platform-routing/package tests and Android companion
   tests after shared changes. Rebuild generated/native boundaries if changed;
   require physical ChromeOS evidence for changes to shared pairing, permissions,
   launch, root or service-lifetime behavior.

Planned baseline: `cargo fmt --all -- --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, web
typecheck/tests and applicable browser E2E, extension tests/package, and native
desktop builds. Run the narrower harnesses first; record exact commands and
results as execution occurs; the execution records below supersede the original
plan's untested status.

## Non-Goals And Completion Record

No legacy importer, production-extension replacement, app rename, release,
remote/LAN control, browser-owned engine, raw IO daemon or separate desktop
service. Public remote authentication work is independent. Android keeps its
existing same-device boundary; Crostini remains its separate backend.

Completion evidence: the first Linux control checkpoint is verified below.
Continue installed picker/package and controlled-transfer evidence on Linux
and Windows, keeping macOS and broader campaign gates open.

## First Checkpoint Contract (2026-09-28)

### Next Linux and Windows checkpoint (2026-09-29)

Execution is authorized on the Linux builder and claimed Linux/Windows guests;
macOS acceptance is deferred. Preserve the first checkpoint's single runtime,
library, exact beta origin, attach-only reconnect and ephemeral credentials.
Implement and verify Windows bootstrap before advancing picker ownership,
controlled transfer and real installer/registration acceptance. Legacy fixture
preparation belongs to M-02; this tactical still does not implement an importer.

Windows bootstrap uses one overlapped, byte-mode named pipe per OS user and
native-host directory. Its name is a SHA-256 digest of that directory and the
user SID, not a bearer secret. The server specifies its owner and a protected
DACL granting only that SID access, rejects remote clients, and requests the
first pipe instance with a two-instance ceiling: one exchange and one pending
accept. A replacement is created before the previous handle closes, preserving
name ownership. Replacement retries a busy kernel slot within one second while
IOCP completes destruction of the previous handle. The
client checks the connected pipe's owner SID before reading credentials and
uses identification-only security QoS. Same-user arbitrary code and privileged
administrators remain outside this boundary, as on Linux.

Reuse the 4-KiB ready frame, one-second exchange deadline and explicit startup
deadline. Windows adds a one-byte acknowledgement before server disconnect so
buffered credentials are not discarded before the client reads them. A busy
pipe is retried only inside the exchange deadline; it is never evidence that
the app stopped. Cancellation closes the overlapped pipe and joins its task.
No rendezvous file containing credentials, additional service or daemon.

Official references reviewed: Microsoft [CreateNamedPipe](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createnamedpipea)
(first-instance admission, overlapped operation, remote rejection),
[GetSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo)
(named-pipe owner queries require READ_CONTROL; returned descriptor ownership),
and locked `tokio-1.53.1/src/net/windows/named_pipe.rs`
(`ServerOptions`, security attributes, `ClientOptions` identification default,
disconnect). Native Windows races exposed retained overlapped-read state when
reusing a handle and delayed kernel-instance retirement after closing it. Use
fresh handles, wait for client EOF after the acknowledgement, and bound
replacement retries. Native source tests cover framing, timeout, busy
admission, owner refusal, duplicate bind and joined shutdown. Installed Windows
evidence remains open until the actual guest passes the lifecycle matrix.

Windows native-host evidence (2026-09-29): the claimed Windows 11 x64 guest
passes `cargo test -p rstorrent-native-host` (12 unit, two process tests),
including 12 simultaneous bootstrap callers, duplicate-owner refusal, owner
SID mismatch, busy/timeout handling, oversized frames and joined shutdown.
Native guest Clippy with tests and `-D warnings` passes. Linux native-host
tests (12 unit, two process) and Windows cross-target Clippy also pass.
This qualifies the bootstrap primitive; desktop/browser installation and
lifecycle integration remain the next evidence gate.

Picker contract: use a desktop-only platform frame on the authenticated semantic
connection, containing a call ID and optional opaque repair-root ID. It is not
an engine command and cannot supply a path. Return only the existing root
snapshot or Cancel. Other runtime compositions refuse this frame. No Android
pairing, permission, picker or service behavior changes.

The desktop owns one picker permit across native and extension views. Repeated
or concurrent requests return busy rather than queueing hidden dialogs. The
extension picker uses a short-lived child invocation of the same executable,
before singleton/Tauri/application setup, exclusively to run the pinned native
`rfd 0.16.0` folder dialog. This is a dialog helper, not another runtime, service
or data owner. Its private standard pipes carry a bounded starting-directory
request and selected path; neither reaches JavaScript. Keep the native view's
existing parented dialog while sharing admission with the extension picker.

This process boundary is deliberate: locked `rfd` GTK `GtkDialogFuture` and
Windows `ThreadFuture`/`IDialog::show` do not close their dialog when the caller
drops its future, and Tauri's callback API exposes no cancellation handle.
The helper lets the parent close and reap exactly its dialog on connection
loss, cancellation, a five-minute deadline or Quit, without platform-specific
cross-thread window destruction. No child is detached; the permit is retained
until the child exits and is reaped. Quit closes admission and joins picker
ownership before application shutdown. Input and output are capped at 16 KiB;
the selected UTF-8 path retains the platform's 4-KiB bound. A failed helper is
an error, not a successful Cancel. Linux uses the existing GTK3 backend;
Windows uses the existing Common Item Dialog. macOS helper activation and
focus remain unadvertised until its future acceptance session.

Picker implementation builder evidence (2026-09-29): desktop helper tests
prove disconnect/Quit kill and reap the owned child before readmission, and
reject malformed output, nonzero exit and deadline expiry. Authenticated gateway
tests cover selection, Cancel, healthy-root repair refusal and disconnect
without root installation. Web tests cover platform capability refusal, normal
Cancel retaining the connection and abort closing the requesting connection.
Web typecheck and suite pass (409 pass, two skipped); extension suite passes
(34 tests), packaging and localization checks pass. Workspace Clippy passes.
The first full workspace test run hit the existing engine corrupt-generation
two-second peer timeout under concurrent builds; its focused rerun passes.
A limited-concurrency full rerun is pending. An independent application lifecycle
smoke also timed out at repair completion (two of three pieces); it is not
claimed as passing transfer evidence. Installed acceptance follows separately.

Chosen composition: short native messaging bootstrap, protected local Unix
rendezvous, then the existing semantic WebSocket adapter inside the desktop
process. No second ApplicationService. A persistent native bridge was rejected
for this checkpoint: Chrome's 1-MiB native response limit would require a new
fragmentation/backpressure protocol for existing 16-MiB semantic responses and
64-MiB metainfo attachments, and would tie transport to another process.

Official sources inspected:

- https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging
  (exact origins, caller argument, native-endian framing, per-message host,
  1-MiB output/64-MiB input; retain our smaller 64-KiB bootstrap limit).
- https://developer.chrome.com/docs/extensions/develop/concepts/service-workers/lifecycle
  (worker suspension; the page owns the connection, never the worker).
- https://developer.chrome.com/docs/extensions/reference/manifest/content-security-policy
  and https://developer.chrome.com/docs/extensions/develop/concepts/network-requests
  (packaged code, exact loopback connect scope, host permissions).
- Locked registry source: `tauri-plugin-single-instance-2.4.3/src/lib.rs` and
  `src/platform_impl/linux.rs`: plugin setup claims the non-replaceable session
  D-Bus name before user setup; secondary launches forward argv then exit.
  The published crate contains no singleton integration tests. Repository
  `desktop_lifecycle.rs` and native-host `tests/process.rs` provide local seams;
  simultaneous installed launches must supply the missing runtime evidence.
- `tauri-2.11.5/src/app.rs::setup` creates only windows with `create=true`;
  `WebviewWindowBuilder::from_config` supports later explicit construction.
  `tauri-plugin-dialog-2.7.2/src/lib.rs::{set_parent,pick_folder}` and
  `rfd-0.16.0/src/backend/gtk3/file_dialog.rs` retain the native GTK picker.
  Picker changes are explicitly deferred; use the existing native window.
- JSTorrent `docs/contracts/native-host-contract.md` and its conformance cases
  require profile allocation/takeover in the legacy host. Deliberate difference:
  this host never selects/creates a profile or owns torrent state.

Linux bootstrap uses a mode-0700, current-UID directory and mode-0600 Unix
socket in the installed native-host directory. Reject symlink/non-owner/insecure
rendezvous objects; validate peer UID. Only the singleton may remove its stale
socket, and only after a refused connection. Bootstrap returns a typed ready
response after the loopback listener has bound. Native messaging additionally
allowlists the exact beta origin for control (legacy hello/launch retain their
existing registration). Same-user arbitrary code execution is outside this
boundary; neither paths nor Origin alone purport to contain it.

The listener binds `127.0.0.1:0`, accepts the exact bound Host and beta extension
Origin, exposes only the semantic connect route, and requires a fresh random
256-bit runtime bearer in the first application frame. No cookie, query token,
static assets, arbitrary HTTP API or LAN binding. Rendezvous authenticity and
bind-before-advertise prevent port prebinding from supplying bootstrap authority.
A stale endpoint/credential cannot authorize a new runtime; credentials rotate
on every process start and are invalidated by joined shutdown. Credentials stay
in native IPC and page memory, never persistent browser storage, URLs or logs.
Windows protected IPC remains unimplemented and fails explicitly; Unix source
sharing is not macOS installed acceptance.

Bounds: four WebSocket clients, five-second application handshake, existing
64-KiB client text, 16-MiB application response plus 4-KiB envelope, 16 pending
calls, eight attachments per connection, 32 control messages, two reserved data
messages, one global 64-MiB torrent upload with its 120-second deadline. Bootstrap
responses are at most 4 KiB; each socket write/read has a one-second deadline;
explicit startup has a ten-second readiness deadline and native host processes
have a twenty-second lifetime ceiling. One worker open operation is coalesced;
one page connection/reconnect loop uses capped backoff and attach-only requests.
Authentication/protocol failure stops automatic retries and offers an explicit
retry. No automatic path launches a stopped app.

Desktop launch intent is explicit: `--extension-background` suppresses initial
webview creation and secondary-window activation. Ordinary launch, tray and
native actions create/focus the main window. Existing close/background policy
is preserved; extension detachment does not decide engine lifetime. Quit closes
bootstrap admission and control connections before the application service.

Capability disposition: shared React library, semantic settings and torrent
control use existing contracts and desktop multi-root presentation. Native root
acquisition, media/open-file, updater and desktop shell actions stay native for
this checkpoint; extension controls must truthfully explain unavailable actions.
No Android trust, pairing, permission or service-lifetime change is intended.

Validation sequence: native bootstrap negative/process tests and control socket
admission/semantic tests; lifecycle tests and desktop build; web/extension tests
and package; installed claimed Linux cold/warm/race/two-view/Quit matrix. Record
exact commands, artifacts, peaks and cleanup below before closing the checkpoint.

### Native implementation checkpoint

Implemented protected Unix bootstrap, loopback-only semantic desktop adapter,
optional webview startup, and control-before-service joined shutdown. The
existing default library remains the sole owner. Windows control fails closed.

Builder evidence so far: `cargo check -p rstorrent-desktop`;
`cargo test -p rstorrent-native-host` (11 unit and 2 process tests);
`cargo test -p rstorrent-native-host -p rstorrent-gateway desktop_control -- --nocapture`
(one focused gateway integration: wrong Host/Origin/token, exact backend identity,
four-client ceiling, library projection and zero active connections after join);
`cargo test -p rstorrent-desktop --lib` (51 pass); desktop/native-host debug build.
Native-host negative cases include insecure/symlink/non-socket rendezvous,
live-owner refusal, stale-socket recovery and bootstrap cleanup. The existing
vendored GLib emits compiler warnings; no vendor source was changed.

### Shared presentation checkpoint

The packaged companion entry now selects desktop explicitly and retains Android
as its default. Desktop uses portable multi-root presentation, an ephemeral
validated bootstrap, the existing WebSocket client, and shared React mount with
proper unmount/close. Folder/media actions remain native-only and the desktop
page offers Open desktop window. Worker clicks coalesce; foreign sender URLs
cannot request bootstrap; credentials never enter remembered-tab storage.

Builder: `npm run typecheck --prefix clients/web`, `npm run test --prefix
clients/web` (403 pass, two skipped), `npm test --prefix clients/extension`
(32 pass), `npm run package --prefix clients/extension`, and
`node scripts/check-localization.mjs` pass. Packaging retains exact beta identity
and permits only exact loopback plus existing ARC network scopes. Installed
Chrome for Testing 151 in the claimed Linux guest reaches the desktop library;
full launch/lifecycle/command matrix is still in progress.

Final resource review also bounds the HTTP phase before WebSocket admission:
eight pending HTTP/1 connections, 32 headers, 16 KiB parser buffer,
and a five-second total HTTP-connection lifetime before upgrade. A joined task set owns these sockets;
Quit cancels it before waiting for application connections. This uses Hyper and
hyper-util already pinned transitively by Axum, now as explicit dependencies.
Reviewed the official [Hyper HTTP/1 Builder](https://docs.rs/hyper/1.11.0/hyper/server/conn/http1/struct.Builder.html)
and [TowerToHyperService](https://docs.rs/hyper-util/0.1.20/hyper_util/service/struct.TowerToHyperService.html)
documentation and pinned `hyper-1.11.0/src/server/conn/http1.rs` defaults; relying on Axum's default
HTTP listener would leave pre-upgrade connection admission unbounded.

Focused final checks pass: eight idle HTTP slots/ninth refusal, five-second
expiry, oversized header refusal and joined Quit with an idle HTTP peer; four
reconnect ownership tests; complete web suite (407 pass, two skipped), web
typecheck/package, native-host tests, workspace Clippy and desktop build.

Installed tab reuse exposed Chrome's redaction of `tabs.Tab.url` without the
broad `tabs` permission. The opener now uses exact own-document
[`runtime.getContexts`](https://developer.chrome.com/docs/extensions/reference/api/runtime#method-getContexts)
(Chrome 116+) to find a live packaged tab, including when its remembered ID is
lost. No broad browsing permission was added. Tests now model redacted URLs
and refusal to focus a navigated-away tab. All 34 extension tests pass; the
fresh Chrome for Testing profile passes 12 repeated opens with one tab, both
with and without a remembered tab ID. Unpacked artifacts are loaded into a
fresh owned test profile after replacement to avoid stale worker code.

### Linux first-checkpoint evidence (2026-09-28)

Builder: Linux x86_64, Rust/Cargo 1.97.0 and Node 25.2.0, existing incremental
cache. Guest: Ubuntu 24.04 x86_64, glibc 2.39, GTK 3.24.41, WebKitGTK 2.52.6.
Both sides' ABI and guest `ldd` were checked before execution. Native debug
binaries were stripped for transfer. Native source is commit `3458d66f`;
final extension source is `b4fd802f` (later native changes are tests only).

| Artifact | SHA-256 |
| --- | --- |
| Desktop executable | `29063971072422fd68db5095522bfef4c10ea19f4669b04eb5ba6f753225293f` |
| Native host executable | `bf7f54f48a89bcac9b7ddfb36febc1b70e09ef5d421f43b9ec13adacabd5ff0d` |
| Beta extension 0.4.0 zip | `bb6291fd916f44c771c760e30d285c0d5dc8581241f0f48241ad7f97753f52db` |

The guest used a task-owned development installation layout, isolated HOME/XDG
and fresh library. Its native UI repaired registration and selected the fresh
root through the GTK folder chooser. Chrome for Testing 151.0.7922.34
(Playwright Chromium 1234) used only task-owned profiles. Its custom
`NativeMessagingHosts` directory received the generated manifest, refreshed
after the versioned native-host path changed. This is real native-messaging
and UI evidence, **not** a deb/AppImage installer or registration-update gate.
No ordinary host browser or personal library was used.

The independently generated private torrent has one 4,096-byte piece target,
no tracker, name `checkpoint.bin`, and infohash
`b802168899aecc03c92baafb328d7dff1d529a72`. DHT, PEX, port mapping and listening
were disabled before adding it through the extension's normal file input.
There was no public swarm or byte-transfer claim.

| Check | Exact observation |
| --- | --- |
| Cold extension open | Packaged popup bootstrap started one `--extension-background` process; zero WebKitWebProcess and no forced native main window; library loaded without code entry. |
| Warm attach / same library | Native Open desktop window and a second isolated browser profile saw the same root, torrent ID and infohash. Runtime instance stayed fixed across warm opens; persisted IDs survived restart. |
| Repeated opens | 12 concurrent worker opens retained one companion tab; repeated after deleting only the remembered tab ID. |
| Singleton races | 12 independent native `start_control` calls from stopped state returned one instance; one desktop process and zero native webviews. Eight concurrent ordinary launches subsequently exited successfully into that owner and showed its native window. |
| Pause/resume | Extension React Pause changed native AT-SPI to `Paused 1` for selected `checkpoint.bin`; native Start changed the extension row to Downloading and snapshot `desired_running=true`. Repeated with final native and extension artifacts; final library revision 11. |
| Authentication | Exact hello identity checked before mounting; invalid token and previous-runtime token both returned authentication_failed with no hello/library disclosure. |
| Explicit restart | After the final 35-second stopped observation, the visible Start button connected successfully to one new background process, with no WebKit native window. |
| Quit | Real exported tray-menu Quit action, not process termination; socket removed, zero desktop processes, open extension visibly disconnected with Start after 35 seconds of automatic retries. Repeated on the final artifacts. |
| Resource sampling | Final background process: 84,004 KiB RSS, zero WebKit webviews. With native window: 238,676 KiB process VmHWM, 35 threads. Earlier native run peak 254,764 KiB. These are bounded checkpoint samples, not sustained-load performance claims. |

Reproduce browser assertions with
`node scripts/verify-desktop-extension-checkpoint.mjs PHASE` inside a claimed
controlled guest, setting `RSTORRENT_PLAYWRIGHT_MODULE` to its Playwright module
and `RSTORRENT_TEST_CDP` to its owned browser if needed. Phases: `prepare`, `add`,
`inspect`, `pause`, `resume`, `ui-running`, `native`, `clicks`, `remember`,
`invalid`, `stopped`, `race`, `stale`, `start`. `remember`/`stale` retain the old
credential only in page memory. Native actions use Machine Control snapshots
and current references; tray Quit uses the observed exported dbusmenu action.
The script never launches a controller browser.

Builder validation completed:

- `cargo fmt --all -- --check` and `cargo clippy --workspace -- -D warnings`.
- `cargo test --workspace`: 1,524 pass, 18 ignored, before the final bounded
  HTTP/reconnect refinements; subsequent focused gates below cover those edits.
- `cargo test -p rstorrent-gateway desktop_control`: exact Host/Origin/token,
  four-client high-water, eight HTTP slots/ninth refusal, slow-header expiry,
  oversized-header refusal, shared library and joined idle/active shutdown.
- `cargo test -p rstorrent-native-host`: final 12 unit + two process tests,
  including exact beta control authority versus legacy production hello/launch
  authority, explicit-start versus attach-only intent, protected rendezvous,
  framing/EOF and stdout discipline.
- `cargo test -p rstorrent-desktop --lib`: 51 pass.
- `npm run typecheck --prefix clients/web`; `npm run test --prefix clients/web`:
  final 407 pass, two skipped; four new reconnect ownership cases.
- `npm test --prefix clients/extension`: final 34 pass;
  `npm run package --prefix clients/extension`; `node scripts/check-localization.mjs`;
  `npm run build --prefix clients/web`; desktop/native-host incremental builds.

Next: implement extension-driven native picker/Cancel/focus and its disconnect
ownership; perform full controlled-transfer/detach Rehearsal A and installed
package/registration repair, then macOS and Windows protected-bootstrap and
lifecycle gates. Broader browser update/suspension/discarded-tab cases,
endurance/resource-pressure evidence, media/shell integration and bounded
failure reports remain open. No importer, profile selector, production identity,
route, publication or migration was added. Android transport and service
semantics are unchanged; shared extension helper tests pass, but this Linux
run does not close physical ChromeOS or mobile acceptance.

Cleanup complete: native Quit and owned browser-unit stop left zero executables
from the test layout. Removed the controlled library, registration, browser
profiles/downloads and owned capture. Restored the recorded GNOME idle delay
(300 seconds) and lock setting (enabled), verified them after reboot, returned
the originally-off VM to off, and released the exclusive claim. The idle-lock
recovery followed Machine Control's Linux guide; no outer UI was used.

Machine Control was usable for discovery, read-only doctor, exclusive claim,
readiness, guest transfer/administration, native semantic UI and lifecycle.
Sibling commit `cda5fe9` repairs common `target reboot` dispatch and Linux
capability reporting. Its 100 client tests, Linux static suite (21 tests, one
skipped), claimed common reboot and post-reboot readiness pass. The guest was
shut down only after that validation; no inherited application data was changed.
