# Tactical 012: Bounded Diagnostics And Progress Assessment

Status: completed on 2026-07-31. The headless browser and Android validation
prerequisite, bounded typed diagnostics, derived progress assessment, prompt
task-terminal supervision, and equivalent web/Tauri and Android presentation
all landed in this slice.

## Motivation And Outcome

The first desktop run with the public Big Buck Bunny magnet remained at
`awaiting_metadata` with no visible activity. That magnet has no explicit peer
hint and the currently retained UDP trackers resolve only to public addresses,
while the implemented diagnostic runtime deliberately accepts only loopback
tracker and peer addresses. The runtime therefore exhausts every discovery
source available in the current build.

This is not necessarily a torrent error. A future DHT task, incoming-peer
listener, scheduled tracker retry, or newly enabled discovery capability could
still advance the same torrent without changing its identity or metainfo. A
failed or exhausted mechanism must not be promoted to a permanent torrent
error while another automatic mechanism is active or scheduled.

The current application owner also observes a finished download task only when
a later command happens to call `reap_finished()`. The durable row and
reactive view can consequently continue to report a running intent without
showing either the exhausted attempt or its reason.

Add one bounded end-to-end observability slice:

- every active engine task has an independently observed terminal outcome;
- a pure progress assessment distinguishes active work, automatic waiting,
  external blockage, and inactive torrents;
- typed diagnostic events explain engine, application, and platform activity
  without becoming product state or a text-scraping API;
- the shared browser/Tauri web presentation and Android Compose presentation
  expose equivalent progress and diagnostic controls; and
- all routine desktop UI development and validation runs through the existing
  loopback browser proof in headless Chrome, without launching or focusing a
  Tauri window.

This tactical explains why the public magnet cannot advance in the current
build. It does not add the public-network or DHT capability needed to download
it.

## Dependencies And References

- [`../engineering-principles.md`](../engineering-principles.md)
- [`../topics/application-control.md`](../topics/application-control.md)
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md)
- [`../topics/tracker-discovery.md`](../topics/tracker-discovery.md)
- [`008-reactive-multi-surface-control.md`](008-reactive-multi-surface-control.md)
- [`010-peer-registry-magnet-failover.md`](010-peer-registry-magnet-failover.md)
- [`011-one-shot-udp-tracker.md`](011-one-shot-udp-tracker.md)
- [`../test-torrents.md`](../test-torrents.md)
- Rasterbar libtorrent `v2.0.13`, pinned under `reference/libtorrent`
- [libtorrent alert reference](https://www.libtorrent.org/reference-Alerts.html)
- [libtorrent client tutorial](https://www.libtorrent.org/tutorial.html)

No libtorrent or JSTorrent source is copied. Their event vocabularies and
product behavior are references for independently authored RSTorrent
contracts and tests.

## Reference Findings

Libtorrent exposes typed alerts rather than requiring applications to scrape
ordinary logger output. Alerts carry a type, category, timestamp, and typed
context. An `alert_mask` enables independently selectable categories, and
disabled categories normally avoid the cost of posting. Error and status
alerts are distinct from verbose session, torrent, peer, picker, DHT, and
block-progress diagnostics.

Libtorrent also bounds its pending alert queue and emits an
`alerts_dropped_alert` when the queue overflows. Its human-readable
`message()` is documented primarily for debugging; applications are expected
to use alert types and fields for deliberate presentation.

RSTorrent should retain those useful properties without adopting libtorrent's
exact categories, inheritance hierarchy, strings, or compatibility surface.

## Headless Development And Validation First

### Existing seam

Tactical `008` already established the required non-interactive desktop seam:

```text
temporary profile and storage
           |
   rstorrent-gateway
           |
authenticated loopback WebSocket
           |
HTTP-served shared web application
           |
headless Chrome with a temporary browser profile
```

The gateway is a bounded repository test proof around the same application
service used in-process by Tauri and Android. It is not a product daemon,
native host, or proposed local-desktop architecture. No new HTTP command API
is required for this tactical. Vite serves static development assets over
HTTP; the existing authenticated WebSocket proof carries commands and view
subscriptions.

Before diagnostic UI implementation begins, make this harness convenient and
safe on macOS:

- discover or accept an explicit Chrome executable, including the standard
  macOS Google Chrome path;
- allocate loopback gateway and Vite ports for the individual run instead of
  assuming the user's ports are free;
- use temporary application, storage, and Chrome profile directories;
- run Chrome with headless mode, no first-run UI, no host window, and no use of
  the user's browser profile;
- drive and inspect the DOM through the existing Chrome DevTools Protocol
  helper, extending it only as needed for clicks, text entry, filters, and
  failure screenshots;
- capture bounded DOM and screenshot diagnostics on failure without retaining
  successful-run artifacts; and
- join the gateway, Vite, Chrome, seed, and observer processes and remove every
  temporary directory.

Playwright is not currently installed and is not required merely to rename the
working DevTools Protocol harness. It may replace the small helper only if it
materially improves deterministic interaction, cleanup, or failure evidence
without introducing an unnecessary browser download or parallel test stack.
The Codex browser connector is useful for investigation but is not a
reproducible repository gate.

Tauri must compile and its adapter must have direct tests, but automated
presentation validation uses the shared browser application. This tactical
does not launch a Tauri process or manipulate a macOS window. A later
maintainer-run Tauri smoke may be recorded separately; it is not agent-owned
validation.

### Android isolation

Android presentation remains independently implemented in Compose and must be
validated, not inferred from the web result. Inner-loop work uses generated
contract tests, reducer tests, and Compose semantics where they can run on the
JVM. Device UI validation uses only a dedicated test AVD launched with
`-no-window`, `-no-audio`, and an explicit serial owned by the harness.

The harness may install and clear only the RSTorrent test package, may create
only its own reverse mappings and temporary artifacts, and must shut down an
emulator it started. It must refuse an ambiguous serial or a device it did not
start when destructive setup would be required. It must not use an attached
physical Android device, ChromeOS hardware, an already visible emulator, or
host GUI automation without explicit user authorization.

## Progress Assessment

### Semantic shape

Introduce a pure application/domain assessment equivalent to
`assess_progress`, not a single `can_torrent_progress()` boolean. At minimum,
the portable result has:

- a **disposition**:
  - `active`: an identified owner is attempting the next transition;
  - `waiting`: an enabled mechanism or scheduled retry can still advance
    without user action;
  - `blocked`: no currently enabled or scheduled mechanism can produce the
    next prerequisite, so an external action or capability change is needed;
  - `inactive`: the torrent is paused, complete, or otherwise deliberately not
    trying to advance;
- a **phase** such as discovery, metadata, storage, transfer, verification, or
  publication;
- one stable bounded **reason code** suitable for presentation;
- bounded structured supporting facts; and
- zero or more bounded suggested action codes when an actual user action
  exists.

The assessment is a projection from authoritative durable state, installed
capabilities, active task ownership, scheduled work, and bounded mechanism
status. It is not a second mutable state machine and is not persisted merely
to make the UI convenient.

Examples:

- an in-flight tracker announce is `active/discovery`;
- an exhausted tracker while DHT is running is `waiting/discovery`, not
  blocked;
- a scheduled tracker retry is `waiting/discovery`;
- all peer hints and trackers exhausted with no other installed discovery
  mechanism is `blocked/discovery`;
- absence of a configured storage root is `blocked/storage` with a select-root
  action;
- a foreground Android SAF operation already in progress is
  `active/storage`, not blocked;
- a paused torrent is `inactive` even if its discovery inputs are exhausted;
  and
- complete is `inactive` without a warning reason.

Do not infer blockage from elapsed time alone. A future stall detector may
report that activity is unexpectedly quiet, but only capability and owner
facts can assert that no automatic transition is possible.

### Exhausted discovery is not a torrent error

Separate ordinary mechanism exhaustion from corruption, violated invariants,
invalid durable state, and unrecoverable storage failure. In the current
loopback-only runtime, exhausting all retained public tracker addresses:

- terminates and joins the attempt owner;
- leaves desired state as running and phase as `awaiting_metadata`;
- records bounded tracker and discovery diagnostic events;
- refreshes the summary view immediately;
- yields `blocked/discovery` with a reason equivalent to
  `no_enabled_discovery_source`; and
- does not set `TorrentState::Error`.

When a later capability is installed or a retry becomes scheduled, the same
assessment must become waiting or active. This tactical need not implement
that future capability, but its data model must not make the transition
impossible.

### Task terminal supervision

Replace command-driven polling as the only way to observe task completion.
The application owner must receive and handle each terminal outcome even when
no client sends another command. Handling an outcome joins or otherwise
observes the task, updates the mechanism status or true error state, emits a
diagnostic event, recomputes the progress assessment, and refreshes affected
views.

Exactly one owner consumes a terminal outcome. Pause and shutdown remain
explicit cancellation paths and cannot race a supervisor into double-joining,
overwriting paused intent, or reviving work during shutdown.

## Typed Diagnostic Stream

### Events and categories

Add a portable, versioned `DiagnosticEvent` family with:

- a stream sequence and bounded timestamp representation;
- severity independent from category;
- a stable event code;
- an optional torrent identity;
- bounded typed context appropriate to the event; and
- a short sanitized fallback summary for development and export.

The first flat category vocabulary is:

- `lifecycle`
- `discovery`
- `tracker`
- `peer`
- `metadata`
- `protocol`
- `scheduler`
- `piece`
- `storage`
- `integrity`
- `platform`
- `performance`

The first severities are `trace`, `debug`, `info`, `warning`, and `error`.
An unavailable tracker can produce a warning diagnostic without changing the
torrent to an error state.

Do not carry peer payload bytes, piece payload bytes, secrets, full magnets,
authentication values, filesystem capabilities, unbounded URLs, arbitrary
peer strings, or backtraces through this contract. Endpoint and tracker
context is normalized, bounded, and redacted where exposure is unnecessary.

### Default profiles

Provide the same named profiles on web/Tauri and Android:

- **Normal**, selected by default: informational lifecycle, discovery
  summaries, tracker outcomes, peer connection summaries, storage, integrity,
  platform warnings, and every warning or error;
- **Detailed**: Normal plus debug metadata, protocol, scheduler, and
  performance events; and
- **Trace**: all categories including high-rate piece, picker/scheduler, and
  protocol detail.

Trace is session-scoped, visibly marked as high volume, and off by default.
Category selection and minimum severity remain independently adjustable.
Warnings and errors remain visible in Normal even when their category is not
otherwise selected.

### Ownership and bounds

Engine components emit typed facts through a narrow sink. The application
service owns the diagnostic hub and a bounded recent-event ring. Platform
adapters may add their own typed platform events without converting Android
logcat or Tauri console text into authority.

Expose diagnostics as a filtered reactive subscription with the same
snapshot, sequence, bounded-delivery, reset, resynchronization, cancellation,
and independent-subscriber properties as existing views. Filtering occurs
before transport queues so a client that requests Normal does not receive
Trace traffic. Bound both record count and encoded bytes.

Overflow must be observable as a typed dropped-count or reset condition. It
must never silently create a plausible continuous timeline. Diagnostic ring
state does not advance durable torrent revision and is not required to resume
a torrent after process death.

Rust `tracing` or platform logging may mirror these typed facts for developer
convenience. Generic tracing fields and formatted log lines are not the
portable application contract.

## Cross-Surface Presentation

### Shared browser and Tauri web application

Add a prominent resizable diagnostics region associated with the selected
torrent, with an explicit global scope. It is visible by default in
development builds and automatically brought to attention for a blocked
assessment or warning/error. Production collapse policy may remain a simple
remembered presentation preference.

The first surface includes:

- current phase, disposition, and reason near the torrent summary;
- Normal, Detailed, and Trace profile controls;
- category and minimum-severity filters;
- torrent/global scope;
- bounded text search over already sanitized presentation values;
- pause/resume autoscroll without pausing event ingestion;
- a clear indication of dropped events or required resynchronization; and
- copy of the currently filtered bounded snapshot.

Rendering is keyed by stable event code and typed context. The fallback
summary is not parsed to decide icons, severity, category, actions, or torrent
state.

### Android Compose

Expose the same semantic information and controls in a phone-appropriate
layout: progress disposition and reason on the selected transfer, plus a
prominent diagnostics section or destination with the same default profile,
category/severity controls, scope, dropped-event indication, and bounded-copy
behavior.

Android does not need to reproduce a desktop split pane pixel-for-pixel, but
it must not omit event categories, filtering, progress reasons, or access to
the current bounded timeline. Its foreground service owns subscriptions;
Activity recreation reattaches without clearing the diagnostic hub or
affecting engine work.

Generated TypeScript and Kotlin declarations originate from the same Rust
contract. Rust, TypeScript, and Kotlin reducer fixtures must agree on progress
assessment values, diagnostic filtering, ordering, dropped/reset behavior,
and final rendered facts.

## Deterministic Scenarios

### Blocked public-style magnet without public traffic

Use a valid magnet with no peer hints and only a UDP tracker containing an
IANA documentation-only non-loopback literal address. The current diagnostic
policy rejects it before opening a public socket, avoiding DNS and dependence
on a live swarm.

Require:

- tracker address rejection and discovery exhaustion event codes;
- prompt terminal-owner observation without a follow-up command;
- durable phase `awaiting_metadata` and desired state running;
- no torrent error;
- `blocked/discovery` with the expected reason;
- a rendered explanation and suggested capability action on web and Android;
  and
- no leaked task, socket, profile, browser, emulator, or artifact.

### Waiting is not blocked

Pure assessment and cross-language fixtures cover an exhausted tracker while
another mechanism is active or a retry is scheduled. The result remains
waiting. This is required before DHT exists so tracker-centric assumptions do
not harden into the contract.

### Successful controlled transfer

Retain the existing loopback libtorrent transfer. Require a bounded sequence
covering add, discovery, peer connection, metadata verification, storage,
piece verification, completion, pause/resume, and clean task termination.
The final payload hash remains authoritative; diagnostic text is not success
evidence.

### Filters and overload

Inject deterministic mixed-category and mixed-severity records into unit and
cross-language fixtures. Verify each profile, independent subscriber filters,
search, ordering, snapshot recovery, overflow/dropped indication, and a
high-rate producer that remains within configured memory.

The headless browser and headless Android AVD each exercise at least one
profile change and category filter against the rendered UI.

## Contracts And Invariants

- Logs and diagnostics do not become durable torrent state, commands, or
  command responses.
- A mechanism failure does not establish torrent failure.
- `blocked` is asserted only when no enabled or scheduled automatic mechanism
  can produce the next prerequisite.
- Progress assessment is derived from authoritative state and identifiable
  owners rather than maintained as a competing state machine.
- Every task has one cancellation path and one observable terminal outcome.
- A terminal task updates views without requiring another client command.
- Diagnostic queues and stored recent history are bounded independently of
  swarm activity and transfer size.
- Filtered-out high-volume events do not consume a subscriber's transport
  queue.
- Sequence loss and overflow are explicit and recoverable.
- Untrusted and secret values cannot enter routine diagnostic output
  unbounded or unredacted.
- Browser/Tauri and Android expose the same semantic profiles, filters,
  progress reasons, and typed event facts.
- Desktop automation does not launch Tauri or focus a host window.
- Android automation uses an explicitly owned headless AVD and never an
  unapproved physical device.

## Nasty Cases Required Up Front

- one discovery mechanism fails while another is active, scheduled, disabled,
  unsupported, exhausted, or newly installed;
- the last candidate fails concurrently with pause, shutdown, or a capability
  transition;
- task completion arrives before or after a subscriber attaches;
- a stale terminal result from an old task generation cannot overwrite a new
  owner;
- a tracker warning is shown without setting torrent error;
- invalid durable state and storage corruption still produce true error or
  repair states rather than being mislabeled blocked;
- exact event record, ring-byte, per-subscriber queue, category-count,
  context-string, and copy-size boundaries;
- multi-byte truncation at every bounded diagnostic string boundary;
- slow, disconnected, resynchronizing, and concurrently filtered subscribers;
- overflow while no UI is attached and while an Activity is recreated;
- a filter that excludes every category still exposes its own dropped/reset
  state and the current progress assessment;
- hostile HTML-like strings remain escaped in the web surface;
- hostile bidi/control text cannot make copied diagnostics misleading;
- Android Activity recreation does not duplicate event rows or subscriptions;
  and
- headless process failure at every startup stage still joins and cleans up
  previously started children.

## Non-Goals

- public tracker or peer networking
- DHT, PEX, LSD, incoming peers, NAT traversal, uTP, or WebSeeds
- tracker reannounce scheduling or retry policy
- making the recorded Big Buck Bunny magnet download successfully
- a product daemon, new HTTP command API, or production remote-access design
- treating the test WebSocket gateway as the desktop product transport
- persistent log files, telemetry upload, crash reporting, or a support
  service
- packet capture, payload logging, or an unbounded wire console
- arbitrary user-defined tracing directives or a stable public log schema
- localization or exhaustive final visual polish
- a visible automated Tauri run or macOS GUI manipulation
- automated use of a physical Android or ChromeOS device
- different diagnostic features on web/Tauri and Android

## Validation

Run, in proportion during development and in full before completion:

```bash
source ~/.profile

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

npm ci --prefix clients/web
npm run generate --prefix clients/web
git diff --exit-code -- \
  clients/web/src/generated/contract.ts \
  clients/web/src/fixtures/reactive-trace.json
npm run typecheck --prefix clients/web
npm test --prefix clients/web
npm run build --prefix clients/web

clients/web/node_modules/.bin/tauri build \
  --config clients/desktop/src-tauri/tauri.conf.json --no-bundle

cargo clippy -p rstorrent-android --all-features -- -D warnings
cargo test -p rstorrent-android --all-features
clients/android/build.sh

uv run --project tests/interop --locked \
  python tests/interop/browser_diagnostics_surface.py \
  --chrome "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
uv run --project tests/interop --locked \
  python tests/interop/browser_reactive_surface.py \
  --chrome "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
uv run --project tests/interop --locked \
  python tests/interop/android_diagnostics_surface.py \
  --avd jstorrent-tablet --headless

python3 scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

The exact Android command may use a dedicated replacement AVD name if the
harness creates one. The execution record must identify the AVD, API, ABI,
headless flags, explicit serial, package cleanup, and process cleanup. It must
also state explicitly that no physical device, visible emulator, Tauri
process, or host GUI automation was used.

## Stopping Condition

Stop when:

- terminal task results update progress and views without another command;
- the deterministic no-public-traffic magnet remains `awaiting_metadata`
  while reporting a typed `blocked/discovery` reason rather than torrent
  error;
- fixtures prove that another active or scheduled discovery mechanism changes
  the same situation to waiting;
- bounded typed diagnostics, profiles, filters, overflow indication, and
  resynchronization pass Rust and generated-client tests;
- the shared web UI is driven end to end in headless Chrome without Tauri and
  renders both blocked and successful controlled scenarios;
- Android Compose exposes and exercises the equivalent progress and
  diagnostics behavior on an agent-owned no-window AVD;
- Tauri and Android adapters compile against the generated contract;
- exact controlled payload hashes and owner cleanup remain green; and
- the execution record lists exact evidence, memory/queue bounds, unsupported
  capabilities, and deliberate deferrals.

## Implementation Record

### Headless validation prerequisite

Completed on Apple silicon macOS on 2026-07-31 before diagnostics
implementation began.

The existing browser harness now:

- discovers standard macOS and Linux Chrome executables or accepts an explicit
  path;
- uses a per-run loopback Vite port and the gateway's existing ephemeral port;
- gives the gateway the exact per-run browser origin;
- uses temporary application, storage, and Chrome profile directories;
- starts Chrome in headless mode with no first-run UI or host window;
- clicks the rendered Pause and Resume controls, observes page exceptions, and
  detects controlled completion through the Chrome DevTools Protocol;
- captures an optional bounded full-page PNG and final DOM;
- terminates Chrome, Vite, gateway, observer, libtorrent seed, and application
  owners on success or failure; and
- removes successful-run temporary profiles and payloads.

The Android reactive harness now:

- can target an explicitly authorized serial as before or exclusively own a
  named AVD;
- allocates an unused emulator console/ADB port and refuses to take over an
  already-running instance of the requested AVD;
- starts its AVD read-only with `-no-window`, `-no-audio`, `-no-boot-anim`,
  and no snapshot;
- waits for an exact serial and completed boot before installing anything;
- provisions a uniquely named controlled SAF tree through the real
  DocumentsUI before injecting the magnet;
- captures the live Compose UI through `adb exec-out screencap`;
- clears only the RSTorrent test package and controlled SAF folder; and
- kills and joins only the emulator process it started.

The Android build script now selects the host UniFFI library extension:
`.dylib` on macOS and `.so` on Linux. Both established Android Rust targets
and the generated Kotlin/APK build therefore run from this Mac without
changing packaged Android `.so` artifacts.

### Recorded evidence

The controlled browser run used libtorrent `2.0.13.0`, a temporary gateway and
Vite origin, and headless Google Chrome:

```text
info_hash=a962f460b83861cfb5faa1d7ad7da9c3f3cc2fc4
metadata_size=26686
pieces=3
requested=16384
received=16384
stored=16384
payload_sha1=576143b2992ecf25c780ff41c79552f3bb50941b
pause_resume=ok
ui_clicks=pause,resume
gateway_shutdown=joined
cleanup=ok
```

The retained ignored evidence is a 1440 by 1000 full-page PNG and an
8,232-byte final DOM. The DOM records controlled completion, positive
requested/received/stored activity, and resumed control.

The first no-window AVD run terminated cleanly while exposing a stale harness
assumption: the current product correctly refuses a magnet before a SAF root
is selected. The harness was corrected to drive the existing controlled SAF
grant flow rather than weakening the product requirement.

The second run used the read-only `jstorrent-tablet` arm64 AVD at the exact
owned serial `emulator-5554`:

```text
info_hash=d0e63efc25c1aaccd5bb4263b1241dc64b858d9d
metadata_size=26788
pieces=8
view_updates=65
payload_sha1=2c49ff134a7b68f0104e9f82ffea5c760d9a35b9
pause_resume=ok
activity_recreation=ok
activity_background=ok
foreground_stop=joined
cleanup=ok
```

Its retained ignored screenshot is a 2560 by 1600 PNG captured from the live
Compose surface at one of eight verified pieces with active piece activity.
After the run, `adb devices` was empty and no emulator, Chrome, Vite, or
gateway process remained. No Tauri process, visible emulator, host GUI
automation, ChromeOS hardware, or physical Android device was used.

### Validation completed for this prerequisite

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
clients/android/build.sh
npm run typecheck --prefix clients/web
npm test --prefix clients/web
npm run build --prefix clients/web
uv run --project tests/interop --locked \
  python tests/interop/browser_reactive_surface.py \
  --screenshot target/headless-evidence/web-reactive.png \
  --dom-output target/headless-evidence/web-reactive.html
uv run --project tests/interop --locked \
  python tests/interop/android_reactive_surface.py \
  --avd jstorrent-tablet --headless \
  --screenshot target/headless-evidence/android-reactive.png
git diff --check
```

The Android build passed native x86_64 and arm64-v8a release builds, UniFFI
generation, debug APK assembly, and JVM unit tests. The web run passed
TypeScript checking, five Vitest tests with the controlled gateway test
skipped outside its live harness, and the production Vite build. Workspace
formatting, Clippy across all targets with warnings denied, all Rust unit and
architecture tests, and all Rust doc tests also passed.

## Completed Implementation

View contract version 2 now carries a pure progress assessment with typed
disposition, phase, reason, and action values. Its inputs distinguish an
active task, exhausted discovery, another active discovery mechanism, a
scheduled retry, and an installed DHT capability. Tests prove exhausted
tracker discovery remains waiting when DHT is enabled and becomes blocked
only when no automatic mechanism can act. Descriptor-backed storage is active
while its engine task owns the transition rather than being mislabeled as
blocked.

Application-owned engine tasks now run inside a terminal supervisor. The
supervisor consumes each result exactly once, refreshes progress immediately,
and emits a terminal diagnostic without waiting for another command.
Cancellation still belongs to pause or shutdown. Ordinary peer/tracker
exhaustion leaves desired state running at `awaiting_metadata`; repair-worthy
storage and true task failures retain their existing durable error paths.

The application view hub owns a diagnostic ring bounded to 512 records and
192 KiB. Every record has a sequence, bounded timestamp, independent severity
and category, stable code, optional torrent identity, sanitized summary, and
at most eight sanitized fields. Control, bidi, and oversized context cannot
pass the contract bounds. Normal, Detailed, and Trace filters run before each
subscriber queue. Ring drops are counted in snapshots and patches; transport
overflow continues to require an explicit typed reset and resynchronization.

The shared web/Tauri UI and Android Compose UI show progress on torrent
summaries and a prominent diagnostics region. Both expose named profiles,
category selection, minimum severity, global or selected-torrent scope,
bounded search, autoscroll control, dropped counts, and a 64 KiB bounded copy.
The web validator and TypeScript and Kotlin reducers understand diagnostic
snapshots and patches. Generated TypeScript and UniFFI Kotlin declarations
continue to originate from the Rust contract.

Two new repository gates use the hardened harnesses:

- `browser_diagnostics_surface.py` drives the documentation-address tracker
  scenario through a temporary gateway, Vite origin, and headless Chrome;
- `android_diagnostics_surface.py` drives the same scenario through a
  read-only, no-window, explicitly owned AVD and the real SAF selection UI.

Both select Detailed and the discovery category in rendered controls, assert
`blocked/discovery/no_enabled_discovery_source`, find the typed
`discovery_exhausted` event, retain screenshots, and clean every owned
process and artifact. The literal `192.0.2.1` tracker is rejected by policy
before a public socket is opened.

## Final Evidence

The final blocked browser run reported:

```text
browser=chrome
scenario=blocked
progress=blocked
reason=no_enabled_discovery_source
diagnostic=discovery_exhausted
ui_filters=profile,category
public_socket=none
cleanup=ok
```

The final successful browser run used libtorrent `2.0.13.0`:

```text
info_hash=a962f460b83861cfb5faa1d7ad7da9c3f3cc2fc4
metadata_size=26686
pieces=3
requested=16384
received=16384
stored=16384
payload_sha1=576143b2992ecf25c780ff41c79552f3bb50941b
ui_clicks=pause,resume
gateway_shutdown=joined
cleanup=ok
```

The final blocked Android run used the `jstorrent-tablet` API 34 arm64-v8a
AVD with `-no-window`, `-no-audio`, `-no-boot-anim`, `-no-snapshot`, and
read-only storage:

```text
progress=blocked
reason=no_enabled_discovery_source
diagnostic=discovery_exhausted
ui_filters=profile,category
activity_recreation=ok
activity_background=ok
foreground_stop=joined
cleanup=ok
```

The final successful Android run acquired a 26,788-byte metainfo dictionary,
verified all eight pieces, produced payload SHA-1
`2c49ff134a7b68f0104e9f82ffea5c760d9a35b9`, exercised pause/resume and
Activity recreation/backgrounding, joined the foreground service, and
cleaned its owned AVD. No physical device, visible emulator, Tauri process,
host GUI automation, or ChromeOS hardware was used.

Final validation:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm ci --prefix clients/web
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm test --prefix clients/web
npm run build --prefix clients/web
clients/web/node_modules/.bin/tauri build \
  --config clients/desktop/src-tauri/tauri.conf.json --no-bundle
cargo clippy -p rstorrent-android --all-features -- -D warnings
cargo test -p rstorrent-android --all-features
clients/android/build.sh
uv run --project tests/interop --locked \
  python tests/interop/browser_diagnostics_surface.py ...
uv run --project tests/interop --locked \
  python tests/interop/browser_reactive_surface.py ...
uv run --project tests/interop --locked \
  python tests/interop/android_diagnostics_surface.py \
  --avd jstorrent-tablet --headless ...
uv run --project tests/interop --locked \
  python tests/interop/android_reactive_surface.py \
  --avd jstorrent-tablet --headless ...
```

The stopping condition is met. Public networking, DHT, tracker scheduling,
and broader peer concurrency remain deliberate later tacticals; diagnostics
make their absence visible without pretending those capabilities exist.
