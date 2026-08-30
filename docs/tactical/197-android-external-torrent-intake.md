# Tactical 197: Android External Torrent Intake

Status: **Complete as of 2026-08-30.** External Android `magnet:` and
`.torrent` activation now passes deterministic, package, connected API 34,
hostile-provider, controlled-transfer, resource, privacy, grant-revocation,
and cleanup gates. No release action occurred.

Topics: `android-jstorrent-replacement`, `client-surfaces`,
`capability-readiness`

Dependencies: completed v1 torrent-byte intake Tactical
[`081`](081-v1-torrent-byte-intake.md), Android product Tactical
[`117`](117-jstorrent-shaped-android-product-ui.md), direct-storage Tactical
[`191`](191-direct-filesystem-storage.md), ChromeOS Android application-owner
Tactical [`194`](194-chromeos-android-extension-control.md), and the current
Compose Add/root workflow in `clients/android`.

Desktop Tactical
[`163`](163-desktop-external-torrent-intake.md) is a lifecycle, hostile-input,
and exact-once product reference, not a shell architecture to copy. Android
continues to use one Compose activity and one foreground in-process Rust
application service rather than a desktop activation queue or helper process.

## Decision And Desired Outcome

Make the installed Android product a real operating-system handler for
`magnet:` links and local `.torrent` documents. A cold activation starts the
existing product activity and service. A warm activation reaches the same
activity through `onNewIntent`. Both paths admit the source exactly once into
one bounded ephemeral intake owner, return to the Library, and present the
ordinary download-root plus **Start downloading immediately** decision before
submitting to the existing application service.

`MainActivity` remains the sole exported intake activity. Add separate narrow
`ACTION_VIEW` filters for:

- the exact `magnet` scheme with `DEFAULT` and `BROWSABLE` categories;
- `content://` data with exact `application/x-bittorrent` MIME; and
- `content://` URI paths ending in `.torrent` for providers whose URI retains
  that suffix while reporting a generic or absent MIME type.

Do not register `application/octet-stream` globally because that would make
the product a candidate for unrelated binary documents. Do not register or
accept `file://`; modern Android sharing uses content URIs and temporary URI
permission. Do not auto-add a received source in the background merely
because another application invoked an exported component.

The current provisional RSTorrent package and branding remain unchanged in
this tactical. The implementation must derive internal explicit intents and
test expectations from the actual build application ID so later JSTorrent
graduation does not require another intake architecture.

## Scope And Stopping Condition

This tactical owns:

1. separate least-privilege manifest filters for supported external magnet
   and content-document activation;
2. runtime validation for every received intent because explicit intents can
   bypass manifest filters;
3. one pure bounded intake model covering admission, duplicate suppression,
   queue order, presentation, submission, retryable failure, cancellation,
   overflow, and terminal consumption;
4. cold `onCreate` and warm `onNewIntent` delivery through the existing
   `singleTop` `MainActivity`, without a second activity, engine, profile,
   service, or task owner;
5. one service-owned ephemeral queue with at most eight source descriptors,
   at most one presented item, and at most one content-provider read/add job;
6. a refactored Compose Add presentation that shows an external source
   generically, retains the start-content choice, preserves the source while
   the user selects or repairs a root, and permits explicit cancel;
7. reuse of the current `ProductEngineService` magnet and bounded torrent-byte
   application operations, including typed added/already-present/selection-
   expanded results and existing error presentation;
8. bounded, cancellable, off-main-thread `content://` reading without base64,
   a Binder byte payload, a public file path, or a temporary metainfo copy;
9. privacy-preserving diagnostics and actionable empty, oversized,
   inaccessible, invalid, expired-grant, provider-timeout, queue-full, and
   duplicate outcomes; and
10. deterministic Kotlin/Compose coverage, merged-manifest inspection, both
    Android ABI builds, connected API 34 AVD instrumentation, and one
    controlled external-intake application profile.

The tactical stops when an installed debug product on the explicitly owned
API 34 AVD can:

- resolve and accept one controlled cold `magnet:` activation;
- accept one warm `content://` `.torrent` activation from a different test
  package under a temporary read grant;
- retain either source through missing-root selection and activity
  recreation, present it for confirmation, and submit it exactly once to the
  same `ProductEngineService` owner;
- report added and already-present outcomes without duplicate catalog rows;
- reject hostile and bounded failure cases without reading on the main thread,
  leaking source text, retaining URI permission, or leaving an intake task;
  and
- complete one tiny controlled transfer entered through an external source
  with exact payload hash and normal cleanup.

No physical-device claim is required for this bounded platform-integration
slice. The later signed replacement cohort in
[`android-jstorrent-replacement.md`](../topics/android-jstorrent-replacement.md)
still requires representative phone and ChromeOS proof.

## Non-Goals

- `ACTION_SEND`, `ACTION_SEND_MULTIPLE`, shared text, drag and drop, clipboard
  monitoring, watched folders, RSS, search, or batch import.
- HTTP/HTTPS `.torrent` fetching, remote content URLs, or following redirects.
- `file://` URI support, broad `application/octet-stream` registration, legacy
  storage permissions, or an ambient filesystem path command.
- Changing magnet parsing, metainfo parsing, duplicate reconciliation,
  download-root identity, file selection, start-content semantics, or
  application persistence.
- Add-time per-file selection, a new preflight metadata parser, or adopting
  JSTorrent's optional file-selection preference.
- Automatic addition without user confirmation, a background receiver, a new
  exported activity, a new service action callable by other packages, or an
  always-running intent daemon.
- Routing intake through the legacy JSTorrent raw I/O companion. On ChromeOS,
  the shared Android application owner accepts the source and any attached
  Compose or React presentation converges through its ordinary views.
- JSTorrent rebranding, `com.jstorrent.app`, legacy-state import, production
  extension publication, default-handler coercion, signing, Play upload, or
  store rollout.
- Android completion notifications, unmetered/VPN policy, proxying,
  background lifecycle policy, playback, or search/plugins.
- A new Rust, UniFFI, or generated application command. If the existing add
  result cannot support the required terminal outcomes, stop and split that
  contract change rather than hiding it in Android UI code.

## Current RSTorrent Findings

The implementation starts from useful existing owners:

- `clients/android/app/src/main/AndroidManifest.xml` exports `MainActivity`
  with `singleTop`, a launcher filter, and the existing exact RSTorrent
  ChromeOS-companion deep link. It has no magnet or torrent-document filters.
- `MainActivity.onCreate` and `onNewIntent` already converge through `route`,
  start/bind the one product service, and retain bounded product inputs while
  the service connects.
- `MainActivity` owns the in-application document picker and currently passes
  its `content://` result directly to the bound service or one pending URI
  slot. That picker may request a persistable grant because it initiated an
  `ACTION_OPEN_DOCUMENT`; externally supplied grants have a different
  lifecycle and must not be treated as persistable.
- `LibraryScreen` owns one Add dialog with manual magnet text, a
  start-content checkbox, a `.torrent` picker action, and a disabled state
  while no healthy root exists.
- `ProductEngineService.addMagnet` already dispatches the ordinary typed
  `AddMagnet` command through the selected SAF root.
- `ProductEngineService.addTorrentFile` already reads on `Dispatchers.IO`,
  rejects empty input, caps the source at 64 MiB, and calls the existing
  `addTorrentBytes` boundary. Its current `ByteArrayOutputStream` reader and
  fire-and-report wrapper should be factored so manual-picker and external
  sources share one bounded reader and return terminal add disposition to the
  intake owner.
- The application control boundary caps magnets at
  `MAX_MAGNET_LENGTH = 16 KiB`; the Android service caps raw metainfo at
  `MAX_TORRENT_SOURCE_BYTES = 64 MiB`.
- Current unit tests cover product state, SAF roots, power, and bootstrap
  contracts; Compose instrumentation covers navigation and manual Add reach.
  No test currently resolves an implicit magnet/document intent or supplies
  a cross-package temporary content grant.

The concrete boundary improvement is a small Android-only intake controller
and one shared bounded content-source reader. `MainActivity` should stop
growing unrelated pending scalar fields for each new production entry lane,
while `ProductEngineService` should stop hiding source-read/add completion
behind a fire-and-forget method when UI ownership depends on the outcome.

## Reference Inspection

### Current JSTorrent Android

The local sibling at `~/code/jstorrent` was inspected at
`25e4b701433fd815398ba89526546f5e4f072e3f` on 2026-08-30:

- `android/app/src/main/AndroidManifest.xml` declares a separate exported
  `LinkHandlerActivity` with magnet, exact BitTorrent MIME, content-path, and
  legacy file-path filters.
- `android/app/src/main/java/com/jstorrent/app/LinkHandlerActivity.kt` chooses
  standalone versus companion mode, forwards magnets as URI data, eagerly
  reads standalone torrent files into one cache file, base64-encodes companion
  torrent bytes, and uses `singleTask` delivery for its standalone activity.
- `android/app/src/main/java/com/jstorrent/app/link/PendingLinkManager.kt`
  retains input until a companion connection exists.
- `android/app/src/main/java/com/jstorrent/app/NativeStandaloneActivity.kt`
  consumes the temporary-file or magnet handoff after cold or warm launch.

RSTorrent adopts the public handler coverage, cold/warm handoff lesson, and
need to retain input until its application owner is ready. It deliberately
does not adopt mode routing, full-URI logging, unbounded `readBytes`, one
overwritable cache filename, base64 duplication, raw companion delivery,
`file://`, or another activity/service topology.

No JSTorrent test currently exercises `LinkHandlerActivity`; its behavior is
reference evidence, not a passing oracle. This tactical supplies independent
tests against Android's public intent and content-grant contracts.

### Android Platform Contract

The following official Android documentation was inspected on 2026-08-30:

- [Intents and intent filters](https://developer.android.com/guide/components/intents-filters)
  requires `DEFAULT` for implicit activity delivery, explicit `exported`
  values, separate filters for distinct action/data combinations, and runtime
  handling that does not treat filter matching as a security boundary.
- [Let other apps start your activity](https://developer.android.com/training/basics/intents/filters)
  recommends making every public filter as specific as possible.
- [Tasks and the back stack](https://developer.android.com/guide/components/activities/tasks-and-back-stack)
  defines `singleTop` warm delivery through `onNewIntent` when the existing
  activity is already at the top of its task. The RSTorrent product is one
  Compose activity, so this tactical must prove that shape before considering
  a broader launch-mode change.
- [Sharing files securely](https://developer.android.com/training/secure-file-sharing)
  identifies content URIs plus temporary URI grants as the supported
  cross-application file path and rejects `Uri.fromFile`/ambient storage
  permission as the modern contract.

Manifest matching is a discovery contract, not validation. An explicit
intent can name the exported activity without matching any filter, and a
content provider controls its MIME, metadata, length hints, bytes, latency,
and permission behavior. Every value remains hostile at runtime.

## Product, Input, And Privacy Contracts

- Only exact `ACTION_VIEW` is intake. Launcher, companion, diagnostic, and
  internal explicit actions retain their existing routing.
- A magnet requires an exact ASCII-case-insensitive `magnet` scheme, a
  nonempty URI, and at most 16 KiB of UTF-8 input. Kotlin does not parse or
  normalize BEP fields; the existing Rust parser remains authoritative.
- A document requires a nonempty `content://` URI of at most 16 KiB. Runtime
  admission accepts the exact BitTorrent MIME or a bounded provider display
  name/path ending in ASCII-case-insensitive `.torrent`. Provider metadata is
  a hint only; the metainfo parser decides validity.
- Metadata queries and all provider open/read work run off the main thread.
  A provider-supplied display name is capped at 256 UTF-8 bytes and used only
  for optional generic presentation. It never becomes torrent identity,
  output layout, a log field, or verified metadata.
- The external queue retains at most eight descriptors. A descriptor holds an
  opaque monotonic intake ID, kind, bounded magnet or URI string, optional
  bounded display label, and phase. Total retained source representation is
  at most 128 KiB plus small record overhead.
- The queue coalesces an exact source already queued, presented, or in flight.
  A later repeat after terminal consumption reaches the ordinary application
  duplicate result.
- Only one item is presented and only one content-provider read/add job is in
  flight. Later items remain ordered. Queue overflow rejects the new item and
  raises one generic bounded notice; it never drops or replaces an earlier
  user's choice.
- The service does not read a document before the user confirms and a healthy
  download root exists. Missing or revoked root state leaves the item pending
  and exposes the existing select/repair action.
- External grants remain temporary. Never call
  `takePersistableUriPermission` for an external activation, never copy the
  source into preferences/profile state, and never promise it across service
  shutdown, process death, reboot, or an expired caller grant.
- Open a document with a `CancellationSignal` where the platform API permits,
  retain the descriptor/stream under one intake job, reject a known length
  above 64 MiB before reading, and still read at most 64 MiB plus one byte
  because length hints are untrusted.
- The read deadline is 30 seconds. Timeout, cancellation, service shutdown,
  and every terminal path signal cancellation, close the stream/descriptor,
  and join the job before another content read begins. No automatic retry is
  allowed.
- Unknown-length input may require one bounded final copy for the existing
  whole-byte UniFFI operation. Peak source-buffer ownership is at most twice
  the 64-MiB source cap plus one 16-KiB read buffer, with only one such job.
  Prefer a known-length exact allocation and record actual Java/native/process
  RSS and source-buffer high water on the AVD.
- Submission retains the intake until the existing application response says
  Added, AlreadyPresent, or SelectionExpanded. Those are terminal success
  outcomes. Invalid/empty/oversized input is terminal after visible generic
  detail. An inaccessible, expired-grant, provider-I/O, or timeout failure may
  offer one explicit retry while the activity/service and grant remain live;
  cancel or a second failure consumes it.
- The confirmation presentation shows **Magnet link from another app** or
  **Torrent file from another app**, an optional bounded display label, the
  start-content checkbox, root readiness/action, Add, and Cancel. It does not
  render the full magnet, URI, provider authority, query, path, or raw error.
- No log, exception surfaced to Compose, saved-state label, diagnostic event,
  test failure, or accessibility semantics may contain a complete magnet,
  content URI, query string, provider authority, path, or metainfo bytes.
  Diagnostics may record only intake ID, source kind, phase, bounded reason,
  byte count, queue depth, duration, and terminal add disposition.
- `ACTION_VIEW` extras, `ClipData`, selector/package overrides, nested intent
  URIs, HTTP/HTTPS, `file://`, absent data, unknown schemes, unrelated MIME,
  directories, and malformed explicit calls do not become intake.

## State Transitions

The pure controller uses these states:

```text
received
  -> rejected
  -> queued
       -> presented
            -> cancelled
            -> awaiting_root -> presented
            -> submitting
                 -> succeeded
                 -> duplicate
                 -> retryable_failure -> submitting
                 -> terminal_failure
```

Only transient metadata `received`, `queued`, `presented`, `awaiting_root`,
`submitting`, and one retryable failure retain a descriptor. `rejected`,
`cancelled`, both successful outcomes, and terminal failure retain no source.
A root transition changes presentation eligibility, not source identity.
Activity recreation attaches to the service-owned current state and cannot
enqueue the same launch intent again. Process/service death drops unconfirmed
ephemeral state; the system may redeliver the original launch intent, but
RSTorrent does not persist a stale URI or silently auto-submit it.

## Ownership, Tasks, Cancellation, And Dependency Direction

```text
Android PackageManager / caller ACTION_VIEW
  -> MainActivity manifest + runtime classifier
  -> bounded activity-to-service admission
  -> ProductEngineService ExternalIntakeOwner
       -> ProductState generic intake presentation
       -> Compose confirmation / root action
       -> one service-scope provider read or magnet dispatch job
       -> existing add_magnet or add_torrent_bytes application operation
       -> ordinary TorrentList update and typed add disposition
```

- `MainActivity` owns Android `Intent`, `Uri`, categories, flags, lifecycle,
  and transfer into the bound service. It creates no intake coroutine and
  retains only bounded not-yet-admitted input while the service binds.
- A plain Kotlin controller owns queue order, source phases, duplicate and
  overflow decisions, one retry allowance, and generic presentation. Its
  deterministic core does not depend on Compose, content I/O, coroutines,
  UniFFI, or application state.
- `ProductEngineService` owns the admitted queue, one source-read/add job,
  `CancellationSignal`, descriptor/stream closure, 30-second deadline,
  dispatch, terminal result, and shutdown join. Existing service scope and
  cancellation remain authoritative.
- Compose observes immutable generic intake state and emits confirm, retry,
  cancel, and select/repair-root actions keyed by intake ID. It never opens a
  URI, reads bytes, or dispatches an application command directly.
- The existing Rust application service owns magnet/metainfo parsing,
  duplicate identity, selection expansion, persistence, root binding,
  start-content intent, views, networking, and torrent lifecycle.
- Android platform code depends inward on the existing generated application
  boundary. No Android `Intent`, `Uri`, `ContentResolver`, descriptor,
  callback, or coroutine type enters Rust protocol/domain state or generated
  transport-neutral commands.

One small Kotlin result type may adapt existing generated add disposition and
error categories for the intake controller. It is not a second application
result protocol and must remain local to Android presentation ownership.

## Implementation Plan

1. **Pure intake gate.** Add the bounded descriptor, classifier inputs,
   controller states/transitions, duplicate suppression, overflow, retry, and
   redaction tests. Extract a bounded source-reader seam with exact limit,
   close, deadline, and cancellation tests over hostile streams.
2. **Manifest and activity gate.** Add separate filters, validate all runtime
   fields, preserve launcher/diagnostic/companion routing, admit both cold and
   warm input, and clear or fence the handled launch data so recreation cannot
   enqueue it twice. Add merged-manifest assertions for the exact exported
   activity, schemes, MIME, categories, and absence of file/http/octet-stream
   breadth.
3. **Service ownership gate.** Add the ephemeral intake owner and one joined
   I/O job. Factor manual-picker/external URI reading and add-result mapping
   without changing the Rust/UniFFI contract. Prove shutdown, cancel, timeout,
   retry, queue advance, and descriptor cleanup.
4. **Compose gate.** Refactor the existing Add presentation around manual and
   external source variants, navigate/reveal Library on external admission,
   preserve root selection/repair and start-content choice, and show generic
   success/duplicate/failure feedback without source leakage.
5. **Instrumented platform gate.** Add an androidTest-only content provider in
   the test APK that streams valid, empty, oversized, denied, delayed, and
   failing content under temporary grants. Query real package resolution and
   exercise cold/warm activity delivery, root selection, recreation,
   duplicate, queue, timeout/cancel, and exact cleanup on the owned API 34 AVD.
6. **Controlled product gate.** Extend `clients/android/run_bootstrap.py` with
   one explicit external-intake profile using `am start` for a magnet and the
   test provider for content. Complete a tiny transfer, verify one catalog row
   and exact payload hash, inspect logs/resources, and perform normal app/AVD
   cleanup.
7. **Closeout gate.** Run the proportional repository baseline, record exact
   results and high-water marks here, reconcile the owning topics and JAR-006,
   and leave completion/error notifications as the next independent Android
   replacement slice.

## Validation Matrix

### Pure Kotlin/JVM

- Exact cold/warm classifier inputs accept `ACTION_VIEW` magnet and supported
  content, while missing/other action, empty/oversized data, file/http/https,
  nested/selector/ClipData, unknown MIME/name, and explicit-filter bypass
  cases reject.
- Queue lengths 0–8, ninth-item overflow, exact duplicate coalescing,
  monotonic IDs, one presentation, one submission, retry-once, cancel,
  terminal success/failure, and ordered advance are deterministic.
- The 16-KiB magnet and URI representation limits admit exactly; one byte over
  rejects without retaining or logging input.
- The reader accepts one byte and exact 64 MiB, rejects zero and 64 MiB plus
  one, honors a known oversized length without reading, closes on read error,
  cancellation and timeout, and never starts a second read before terminal
  join.
- Redaction tests scan logs, error presentation, saved-state descriptors, and
  accessibility text for sentinel magnet/query/authority/path values.

### Compose And Activity Instrumentation

- External admission returns to Library and shows one generic confirmation;
  manual Add remains unchanged.
- Start-content on/off reaches the existing application request exactly.
- Missing/unhealthy root keeps the source, exposes select/repair, survives
  picker cancel, and enables Add only after the root becomes healthy.
- Cancel consumes only the current item. Rotation/recreation does not add a
  second item. A second warm intent queues behind the first, and overflow is
  generic and bounded.
- Added, already-present, invalid, expired permission, provider failure,
  timeout, retry, and terminal dismissal each converge to the expected state.
- `PackageManager` resolves exact implicit magnet, BitTorrent-MIME content,
  and supported `.torrent` content intents to `MainActivity`; it does not
  resolve unrelated octet-stream, file, HTTP/HTTPS, or send intents.
- A caller-owned test provider grants read access only for the delivered URI.
  The app reads it without storage permission, never requests a persistable
  grant, and cannot reopen it after the caller revokes the temporary grant.

### Controlled Runtime And Resources

- Force-stop, cold magnet activation, warm magnet activation, and warm
  content-document activation retain one `MainActivity` task and one
  `ProductEngineService`/Rust application/profile owner.
- One controlled external source adds one catalog row; a repeat reports
  AlreadyPresent and retains one row. One start-enabled tiny transfer reaches
  the exact expected payload hash.
- During an unknown-length near-limit provider stream, record queue depth,
  one read task, descriptor count, Java/native/process RSS, source bytes, and
  cleanup. Assert declared queue/source/task bounds and no terminal descriptor
  or coroutine residue.
- `logcat`, saved instance state, app-private preferences/database, and
  diagnostics contain none of the sentinel full magnet, URI, provider
  authority, query, path, or metainfo bytes.
- Force-stop/uninstall and AVD teardown leave no intake temp file, persisted
  URI grant, source preference, extra process, or provider descriptor.

### Build And Repository Baseline

Run from the repository root after sourcing the configured profile:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
(
  cd clients/android
  ./gradlew lintDebug testDebugUnitTest assembleDebug assembleDebugAndroidTest
)
./clients/android/build.sh
```

Run the connected instrumentation suite only on an explicitly owned AVD,
then run the new bounded `product-external-intake` bootstrap profile with
`--target avd`. Record the exact AVD name, API, ABI, commands, outcomes,
resource high waters, and cleanup. No public swarm, physical device, Play,
production extension, or external publication is required.

No web/TypeScript generation or test is required unless implementation
changes the generated application contract, which this tactical forbids
without escalation.

## Completion Evidence

Implementation landed in these commits:

- `f9852e9` — activate the bounded tactical;
- `fc3763c` — add the classifier, pure intake controller, bounded reader, and
  JVM tests;
- `0a2f87b` — add exact manifest/activity/service/Compose integration;
- `4c58c62` — add package-resolution, lifecycle, Compose, and hostile-provider
  platform fixtures;
- `baf00cd` — make retry exhaustion a distinct terminal controller result;
- `54b012b` — add the controlled API 34 external-intake profile;
- `89009b2` — add implicit cold/warm magnet and near-limit resource/privacy
  evidence; and
- `ab97f9f` — stabilize real activity-lifecycle and dialog-scoped connected
  instrumentation.

The implementation is Android-local. The principal production owners are
`ExternalTorrentIntake.kt`, `BoundedTorrentSourceReader.kt`, `MainActivity.kt`,
`ProductEngineService.kt`, `ProductState.kt`, `ProductApp.kt`, and the exact
filters in `app/src/main/AndroidManifest.xml`. Unit tests cover classification,
queue/state/retry/redaction, source limits, closure, cancellation, timeout, and
buffer ownership. Instrumentation covers real package resolution, cold/warm
activity delivery, recreation, generic Compose confirmation, root action,
start choice, retry/cancel callbacks, and source redaction. The test APK owns
an unexported temporary-grant provider for valid, empty, known/unknown
oversized, near-limit, denied, delayed, failing, directory, and generic-MIME
cases.

The merged debug manifest was inspected with:

```bash
apkanalyzer manifest print \
  clients/android/app/build/outputs/apk/debug/app-debug.apk
```

It contains one exported `singleTop` `MainActivity`, separate
`DEFAULT`/`BROWSABLE` filters for `magnet`, exact
`application/x-bittorrent` plus `content`, and `content` paths ending in
`.torrent`. Both engine services remain unexported. Instrumented
`PackageManager` resolution accepts only those three declared shapes and
rejects file, HTTP, HTTPS, octet-stream-only, and `ACTION_SEND` inputs.

The proportional build and deterministic gates passed on 2026-08-30:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
(
  cd clients/android
  ./gradlew lintDebug testDebugUnitTest assembleDebug assembleDebugAndroidTest
)
./clients/android/build.sh
(
  cd clients/android
  ANDROID_SERIAL=emulator-5554 ./gradlew connectedDebugAndroidTest
)
```

The full Rust formatting, warning-denied Clippy, workspace unit/integration,
and doc-test baseline passed. The dual-ABI build packaged `x86_64` and
`arm64-v8a`. The connected suite ran 10 tests on the explicitly selected
`jstorrent-tablet` API 34 arm64 AVD and passed. The first connected run exposed
an `ActivityScenario`
pre-create tracking limitation after the real activity had displayed and an
ambiguous duplicate **Select folder** test selector; framework lifecycle
callbacks and dialog-scoped semantics retained the intended assertions, and
the clean repeat passed all 10 tests. No Rust/UniFFI/generated application
contract changed, so web generation and tests were inapplicable.

The controlled installed-product command was:

```bash
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet --storage saf-internal --runs 1 \
  --profile product-external-intake --no-build
```

The fresh AVD identified itself as API 34, `arm64-v8a`, model
`sdk_gphone64_arm64`, fingerprint
`google/sdk_gphone64_arm64/emu64a:14/UE1A.230829.050/12077443:userdebug/dev-keys`.
The profile passed implicit cold magnet presentation, warm exact coalescing,
warm cross-package content, start-disabled `PAUSED`, typed AlreadyPresent,
start-enabled exact transfer, empty/oversized/invalid terminal failures,
permission/provider retry then terminal, delayed cancellation, timeout then
explicit retry and exact transfer, directory/generic rejection, safe-name
generic acceptance, and exact torrent/root cleanup. The controlled v1 payload
completed all five pieces and every final non-padding file matched its expected
SHA-1.

The unknown-length 64 MiB provider read recorded 100,663,296 bytes peak
source-buffer ownership, below the two-source-cap-plus-buffer bound. AVD
baseline/high Java RSS was 35,108/100,840 KiB, native RSS was
20,772/88,020 KiB, and process RSS was 204,564/348,376 KiB. Process descriptor
baseline/high/settled was 142/168/142. Direct SAF owned/pending handle highs
were 6/3 under the existing limit of 40. Product-log and app-private-file scans
found none of the sentinel magnet, query token, or provider authority. The
external grant was absent after force-stop; application/test-package uninstall,
reverse transports, provider pipes, SAF child, staging/part/final paths, host
fixture, and fresh AVD cleanup all passed.

## Documentation And Completion Updates

Before marking this tactical complete:

- record exact commits, source/test paths, commands, AVD identity, manifest
  resolution output, controlled transfer result, resource high waters,
  failures, and cleanup evidence in this document;
- mark JAR-006 complete in
  [`android-jstorrent-replacement.md`](../topics/android-jstorrent-replacement.md);
- update Android intake truth and evidence in
  [`client-surfaces.md`](../topics/client-surfaces.md) and
  [`capability-readiness.md`](../topics/capability-readiness.md);
- update [`beta-release-readiness.md`](../topics/beta-release-readiness.md) if
  the new public handler changes Android beta claims or store declarations;
  and
- leave `JAR-004` production identity/migration and `JAR-005` production
  extension rollout open. The provisional handler evidence cannot be called a
  signed JSTorrent replacement.

## Escalation Contract

Implementation may choose internal Kotlin names, refactor the current Add
dialog and shared URI reader, add test-only provider components, tighten
presentation copy, and fix bugs within the declared owner without further
direction.

Stop for maintainer direction if evidence requires:

- changing `MainActivity` from `singleTop`, adding another exported component,
  accepting `ACTION_SEND`, `file://`, HTTP/HTTPS, or broad octet-stream input;
- persisting external sources, taking durable external URI grants, copying
  metainfo to a temporary profile file, or adding another process/service;
- changing the 16-KiB magnet or 64-MiB torrent-source application limits;
- adding a Rust/UniFFI/generated command or changing engine, persistence,
  metainfo, duplicate, storage-root, or start-content semantics;
- routing through the legacy raw companion or modifying the production
  extension/application identity; or
- using a physical device, signing key, store account, default-handler
  mutation outside normal package install, or any public release action.

An ordinary manifest, Kotlin, Compose, AVD, provider, or existing-add test
failure is not an escalation. Diagnose it within this tactical while
preserving the bounds and cleanup contract.

## Next Slice Boundary

After JAR-006 closes, the next recommended independent Android replacement
slice is JAR-007 completion and actionable failure notifications. Unmetered
network policy remains the following larger platform/application boundary;
neither belongs in this intake tactical.
