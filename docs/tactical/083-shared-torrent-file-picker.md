# Tactical 083: Shared Torrent File Picker

Status: Complete on 2026-08-04. The interaction and contract revision were
authorized by maintainer direction; all four implementation gates and the
stopping condition are satisfied.

Topics: `web-ui-design`, `client-surfaces`, `application-control`,
`application-connection-architecture`, `capability-readiness`

Dependencies: completed Tacticals
[`037`](037-live-magnet-toolbar-intake.md),
[`048`](048-unified-view-delivery-and-tauri-migration.md),
[`060`](060-multiplexed-application-websocket.md),
[`063`](063-live-file-selection.md), and
[`081`](081-v1-torrent-byte-intake.md) establish the shared Add toolbar,
in-process Tauri host, ordinary browser WebSocket, storage-root/start-content
options, and bounded raw `.torrent` byte operation used by this slice.

## Decision And Motivation

Make the existing shared React Add control capable of choosing and submitting
one local v1 `.torrent` file in both the browser-hosted product and the Tauri
webview. Preserve the deliberately compact interaction accepted by the
maintainer:

- submitting a nonempty input continues through the existing magnet path;
- submitting an empty input synchronously opens the platform file chooser;
- choosing one file continues through the existing root/start options and
  uploads it immediately after those choices are settled; and
- canceling the chooser does nothing.

Do not add a second visible button, source selector, drop zone, file preview,
or upload-progress treatment. The chooser is intentionally a low-discovery
first pass behind the existing **Add** action.

The browser must not calculate a source digest or require `SubtleCrypto` or a
secure context for this feature. Tactical `081` made SHA-256 an exact-source
and durable-receipt fingerprint, not a v2 torrent identity or a transport
integrity mechanism. Rust continues to calculate that fingerprint over the
complete outer source. The v1 torrent identity remains SHA-1 over the exact
bencoded `info` dictionary.

The WebSocket keeps its declaration/ready exchange because it reserves the
single upload permit and associates one bounded binary message with semantic
options before the browser queues up to 64 MiB. This is framing and admission,
not chunking. Only the caller-supplied digest is removed.

## Desired Outcome And Stopping Condition

The tactical stops when all of the following are true:

- activating **Add** with a whitespace-only input opens one single-file
  chooser synchronously from the pointer or keyboard activation;
- activating **Add** with any nonempty input never opens the chooser and
  retains the current magnet, remote-URL, file-URL, malformed-input, and byte
  bounds;
- canceling the chooser changes no status, root preference, application
  revision, request sequence, or torrent state;
- the chooser provides `.torrent` and `application/x-bittorrent` accept hints
  without trusting filename, extension, MIME type, local path, or other
  browser metadata;
- one selected `File` is rejected locally when empty or above 64 MiB, is read
  exactly once as an `ArrayBuffer` only after root/start options are settled,
  and is never converted to base64, JSON numbers, text, or a filesystem path;
- a usable default root with hidden Add options uploads immediately, while a
  missing root or enabled Add options reuses the existing dialog, folder
  choice, start-content checkbox, and post-success preference mutation;
- `.torrent` intake initially uses `FileSelectionIntent::All`; file-specific
  changes remain available after add through the paged Files view and do not
  require pre-add parsing in JavaScript;
- the shared React application issues the same local byte-intake intent in an
  ordinary browser and a Tauri webview, while the active adapter alone chooses
  WebSocket, explicit HTTP diagnostic mode, or raw Tauri IPC;
- the caller-facing `AddTorrentBytesRequest` retains exact source length but
  no longer accepts or requires `source_sha256`;
- Rust calculates SHA-256 from the received bytes before source persistence,
  keeps the source-table digest and exact-source semantics unchanged, and
  includes the calculated digest in durable request replay/conflict identity;
- successful receipts written before this tactical still replay for the same
  request/options/bytes, and the same request ID with different bytes remains
  a conflict without a schema migration;
- the WebSocket still performs
  `begin_torrent_upload -> torrent_upload_ready -> one binary message -> result`,
  validates the declared length against the binary message, and releases its
  one-host permit on every terminal path;
- HTTP and Tauri still derive length from their raw bodies, and common Rust
  preparation derives the digest from those bytes; neither gains a caller
  digest, path, loopback listener, multipart form, or alternate application
  mutation;
- the toolbar and Add dialog expose one indeterminate **Adding…** state across
  file read, transport, parse, and commit, with no byte percentage or transport
  queue presented as semantic progress;
- successful file intake produces the same application snapshot refresh,
  status, preference handling, and navigation behavior as successful magnet
  intake; typed local, transport, parser, duplicate, stale, root, and resource
  errors remain visible without a false success;
- generated Rust/TypeScript/schema artifacts, browser clients, Tauri raw IPC,
  component behavior, accessibility, headless WebSocket intake, and the full
  workspace validation matrix pass; and
- this tactical and its owning topics record the exact landed contract,
  evidence, deliberate deferrals, and next boundary.

## Stable Interaction Contract

### Empty and nonempty Add submission

`TorrentActions` retains one text input and one submit button. Its submit
handler first inspects `torrentInput.trim()`:

```text
nonempty -> existing validateTorrentInput -> magnet Add flow
empty    -> synchronously click hidden single-file input -> return
```

No `await`, root lookup, file read, or state transition may occur before the
hidden input is activated. This preserves browser user activation. Pressing
Enter in the empty text field and keyboard-activating the Add button follow the
same path as pointer activation.

The file input is present only on the live product toolbar, has no `multiple`
attribute, and uses an advisory accept value covering `.torrent` and
`application/x-bittorrent`. Demo mode remains unchanged. A canceled chooser
does not reuse the old empty-input validation message and does not report an
error.

After a change event, capture the first `File` and reset the DOM input value so
choosing the same file again produces a later change event. The local filename
may exist in the browser-owned `File` object but is not sent, persisted,
logged, or treated as a safe torrent path.

### Root and start options

Generalize component-local `PendingAdd` into a closed magnet/file source:

```text
PendingAdd
  magnet: bounded normalized magnet text
  torrent_file: browser File capability
```

Do not place either a `File` or its bytes in Zustand or a generated application
snapshot. When Add options must be shown, retain only the bounded browser
`File` capability until confirm or cancel; read no bytes while the user is
choosing a root. Cancel discards that capability and restores focus through
the existing dialog behavior.

When a usable default root exists and Add options are hidden, begin the read
immediately. Otherwise, confirmation supplies the selected root and existing
start-content value. A file add always begins with selection `all`. The
**Don’t show again** preference changes only after the file add succeeds, just
as it does for a magnet.

The existing checkbox wording may remain because it describes the durable
start-content intent. This tactical does not add a pre-add file catalog or
initial wanted-range editor.

### Busy, error, and completion behavior

The existing component-local one-add guard covers the file read and the whole
adapter call. While owned, the Add action and conflicting torrent actions are
disabled and the visible label is **Adding…**. There is no progress percentage,
fake upload rate, WebSocket `bufferedAmount` display, XHR-only progress path,
or stage-specific state machine.

An empty/oversize file or failed browser read produces a bounded local error
and no application call. A failed direct add may discard its invisible file
capability; retry begins from another empty-input Add activation. A failure
inside the visible Add-options dialog retains the pending file capability so
the existing dialog retry behavior remains useful. Success clears pending
state, applies any accepted preference update, refreshes the ordinary live
snapshot, and reports **Torrent added** without adding an auto-navigation
policy.

## Caller Contract And Server-Derived Digest

### Caller-visible request

`AddTorrentBytesRequest` continues to carry:

- application version;
- durable request ID;
- optional expected revision;
- storage-root identity;
- start-content intent;
- compact initial file selection; and
- exact source length.

Remove `source_sha256` from the caller-visible Rust DTO, generated TypeScript,
JSON Schema, WebSocket declaration, client validation, and tests. The upload
path must contain no `crypto.subtle`, JavaScript SHA implementation, digest
worker, or new dependency. This removes a source-upload secure-context
requirement; it does not change the existing host, authentication, Origin, or
remote-access posture.

The WebSocket declaration still needs source length so it can reject zero,
oversize, or mismatched binary input while holding the admitted upload. The
HTTP and Tauri adapters construct that value from the body they already own.

### Server preparation and durable identity

`prepare_torrent_bytes` remains the single common preparation owner. It:

1. validates semantic options and declared length;
2. compares the received byte length with the declaration;
3. calculates SHA-256 over the exact complete outer source in Rust;
4. projects and validates the bounded v1 metainfo;
5. calculates the v1 SHA-1 info hash from the exact `info` span; and
6. passes the source, server-derived digest, projection, and selection to the
   existing atomic transaction.

The database continues to key the torrent by its v1 SHA-1 info hash. The
source SHA-256 remains metadata beside the exact original source and a compact
request fingerprint; it is not a database torrent key, piece hash, peer-wire
value, or BEP 52 support claim.

Existing Tactical `081` receipts serialized the complete request, including a
verified caller digest. Preserve their canonical replay identity by building
the receipt fingerprint from normalized request fields plus the newly
server-derived digest in the same legacy shape. Add an explicit compatibility
test that seeds or creates the old receipt representation, then proves:

- the same options and bytes replay its stored response;
- different bytes of the same length conflict;
- different normalized options conflict; and
- no source bytes are copied into the receipt.

No schema migration or API-version increase is required. A previously built
WebSocket client that sends an extra `source_sha256` field may remain tolerated
through Serde's ordinary unknown-field behavior, but the new generated client
must not send or validate it. Stable public wire compatibility remains outside
the claim.

## Adapter Data Flow

```text
shared React toolbar
  -> File capability (options pending, no byte read)
  -> one ArrayBuffer + add_torrent_bytes UI intent
  -> LiveApplication assigns request ID and selection=all
  -> active ApplicationViewClient
       browser: begin(length/options) -> ready -> binary -> result
       HTTP diagnostic: raw POST body -> result
       Tauri: raw InvokeBody -> result
  -> Rust computes source SHA-256 and v1 info-hash SHA-1
  -> existing atomic session intake
  -> ordinary response snapshot -> React status/store
```

The browser product continues to use the ordinary multiplexed WebSocket and
does not open an HTTP side lane for this interactive operation. Explicit
`transport=http` diagnostics continue to use the existing raw endpoint. The
Tauri webview uses the same HTML file input and React action, then its existing
adapter passes the `ArrayBuffer` as a raw invoke body. Native code receives no
browser path and opens no user-selected source file itself.

## Owner, Task, Cancellation, And Resource Map

| Owner | Retained state and work | Terminal behavior |
| --- | --- | --- |
| `TorrentActions` | Empty/nonempty dispatch, hidden input, at most one pending `File`, one awaited read/add generation, busy and status state | Chooser cancel is a no-op; dialog cancel drops the `File`; unmount or superseded generation ignores late UI completion and drops the buffer |
| Add dialog | Existing root, start-content, preference, error and focus lifecycle | Cancel only while idle; failed confirm stays open; success closes through the parent |
| `LiveApplication` | One request ID, semantic byte intent, client call, and response snapshot mapping | Client close rejects or joins transport work under existing connection ownership |
| WebSocket adapter | One declaration, pending call, admitted upload ID, one `ArrayBuffer`, and one binary send | Abort closes the generation as already implemented; every error removes pending state and releases the server permit |
| HTTP adapter | One raw fetch body under explicit diagnostic selection | Fetch abort follows the existing signal path; the server owns a complete dispatched operation |
| Tauri adapter | One raw invoke body and bounded metadata headers | Window/client close follows existing IPC/application lifetime; no detached file task or path handle |
| Rust gateway/desktop adapter | Length validation, admission, raw-body ownership, and construction of the common request | Incomplete or invalid input mutates nothing; complete dispatch is owned by the application service |
| Session preparation/store | Server digest, metainfo projection, replay/conflict identity, and atomic SQLite mutation | Typed failure changes neither revision nor torrent/source rows; success commits one complete receipt and source |

One selected file may be as large as 64 MiB. Before options are confirmed, the
UI retains a browser `File` capability rather than an `ArrayBuffer`. Once read,
the path retains one source buffer and accepts the already measured adapter and
Rust copies from Tactical `081`; this slice adds no digest copy, base64
expansion, chunk list, progress history, second connection, temporary source
file, or second concurrent upload. File-size boundary tests should exercise a
numeric helper rather than allocating repeated 64-MiB JavaScript fixtures.

## Implementation Gates

### Gate 1: Server-derived caller contract

Remove caller `source_sha256`, retain declared length, move receipt identity to
the already calculated Rust digest, preserve legacy receipt replay, regenerate
contracts, and update HTTP, WebSocket, Tauri, session, and TypeScript tests.
The gate stops when no client source-hash input or digest validation remains
and all existing exact-source persistence semantics pass.

This is a useful commit boundary.

### Gate 2: Shared application byte intent

Add one UI-local `add_torrent_bytes` intent carrying the `ArrayBuffer`, root,
and start-content choice. Route it through `InspectionController` and
`LiveApplication` to the optional adapter operation, assign the ordinary
bounded request ID, use selection `all`, map the response snapshot, and produce
the same success/error vocabulary as magnet add. Demo mode rejects or omits
the live-only operation truthfully.

### Gate 3: Empty-Add picker and options integration

Add the hidden single-file input, synchronous empty submission behavior,
advisory accept hints, same-file reset, size/read validation, magnet/file
`PendingAdd`, existing options-dialog reuse, one busy owner, and no-progress
presentation. Cover pointer, keyboard, cancel, default-root, choose-root,
start-content, preference, retry, duplicate activation, and error cases.

This is a useful commit boundary after component and adapter tests pass.

### Gate 4: Headless product and closure evidence

Drive the actual empty Add button through a headless Chrome `filechooser`
event against an ordinary WebSocket gateway using an independently generated
small valid v1 `.torrent`. Confirm exactly one binary upload, no interactive
HTTP semantic lane, visible torrent addition, exact application result, and
clean shutdown. Run Tauri raw-IPC mocks and headless desktop build checks,
record evidence, update owning topics/readiness, and close the tactical.

No public network, libtorrent peer, payload download, visible browser, visible
Tauri window, Android build/runtime, emulator, physical device, or deployment
mutation is required for this presentation slice.

## Implementation And Evidence

All four gates completed. Commit `3b19aaa` removes `source_sha256` from the
caller-visible Rust, generated TypeScript, and JSON Schema request while
retaining exact `source_length`. HTTP and raw Tauri intake construct that
length from the body they already own; WebSocket retains declaration, ready,
one binary message, and correlated result. Common session preparation computes
SHA-256 once from the complete source. Its receipt helper inserts that
server-derived digest into the same legacy JSON fingerprint shape, so an old
successful request replays byte-for-byte while changed options or same-length
changed bytes conflict. A focused WebSocket case proves old clients with the
retired extra field remain tolerated.

Commit `7377b05` adds the transport-neutral React byte intent and empty-Add
interaction. `LiveApplication` assigns the ordinary namespaced request ID,
selection `all`, exact buffer length, root, and start intent, then invokes only
the active `ApplicationViewClient`. The toolbar synchronously clicks one hidden
single-file input for whitespace-only submission and leaves every nonempty
input on the magnet validator. It resets the DOM input after capture, rejects
numeric zero/over-64-MiB sizes before allocation, retains only the `File` while
the existing options dialog is open, and reads one `ArrayBuffer` when an add
attempt begins. One component-local busy owner covers read and transport;
dialog failures retain the file for retry, while success preserves ordinary
status, snapshot, focus, preference, and navigation behavior.

The final product harness generates an independent 157-byte v1 source and
drives the production React build in headless Chrome. Empty Add emitted one
real `filechooser`; metadata-only confirmation produced one application
WebSocket, one binary frame, no semantic HTTP request, the exact visible
info-hash row, no payload artifacts, and no serious or critical axe findings.
Gateway metrics recorded one upload declaration, one ready admission, one
accepted connection, and zero active connections after joined shutdown.

The closing gate passed `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, generated-contract regeneration without drift, all
web tests (161 passed, two skipped), TypeScript typechecking, production web
build and CSP check, a no-window desktop build, Python syntax validation, the
focused live picker harness, and `git diff --check`. Focused session, gateway,
desktop raw-IPC, WebSocket/Tauri client, LiveApplication, component, demo, and
pure file-intake tests also pass.

Visible picker affordances, progress, chunking/resume, multiple files,
pre-add parsing/selection, native desktop file associations, Android intake,
remote URL fetch, source export, HTTP tracker execution, and v2/hybrid support
remain deliberately deferred.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Rust semantic/store | Server-derived source digest, exact source and SHA-1 info-hash identity, length mismatch, malformed/oversize/duplicate/stale/resource rollback, same-request replay, different-byte conflict, and legacy Tactical `081` receipt compatibility |
| Gateway WebSocket | Declaration without a digest, ready-before-body, exact length, unexpected/repeated/oversize binary handling, one-host permit, timeout/disconnect release, result correlation, and unchanged 64-KiB text/64-MiB binary limits |
| HTTP and Tauri | Raw bodies still derive length/digest in Rust, HTTP query and Tauri headers contain only semantic options, JSON/base64/path inputs remain absent, and exact bytes reach the common operation |
| Generated contract | Rust/TypeScript/JSON Schema remove caller `source_sha256`, keep source length and selection, validators regenerate without drift, and an old extra WebSocket digest is harmless if compatibility coverage retains it |
| Pure web client | Source bounds and declared length, no digest input, no `SubtleCrypto` source work, WebSocket ready/body/result, raw HTTP body, raw Tauri invoke, abort/close, and bounded errors |
| React component | Nonempty magnet unchanged; empty pointer/keyboard submit opens one chooser; cancel no-op; same-file reselection; advisory accept; default and chosen roots; start disabled; preference after success only; one busy add; read/adapter/parser errors; no percentage or false success |
| Headless browser | Actual filechooser from empty Add, generated valid source, ordinary WebSocket binary path, no interactive semantic HTTP request, visible successful addition, empty serious/critical axe findings, and joined cleanup |
| Tauri desktop | Shared React/typecheck coverage, raw invoke mock, Rust command tests, and compile/build without opening a window or source path |
| Workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, generated-contract drift, web tests/typecheck/build/CSP, targeted Playwright, desktop tests/build, and `git diff --check` |

## Non-Goals And Deliberate Deferrals

- A visible **Choose file** button, source tabs, helper text, drag-and-drop,
  paste/file clipboard intake, selected filename preview, recent sources, or
  batch selection.
- Percentage progress, upload rate, remaining time, WebSocket
  `bufferedAmount`, XHR-only progress, application-level chunking, resumable
  upload, temporary-file spooling, or more than one admitted upload.
- JavaScript SHA-1, SHA-256, Web Crypto, a digest worker, WASM hashing, or a
  hashing dependency for source intake.
- Pre-add metainfo parsing, file/tracker preview, initial per-file selection,
  source editing, tracker editing, or duplicate-source enrichment.
- Android document intents, native desktop picker commands, desktop file
  associations, operating-system share/open-with integration, filesystem path
  handoff, remote URL fetch, or additional storage-root authority.
- Exact source export/copy, synthesized `.torrent` export, original magnet
  retrieval, or changes to source-storage policy.
- HTTP/HTTPS tracker execution, v2/hybrid torrents, BEP 52 identity, payload
  transfer changes, queue policy, or engine work.
- A stable public wire format, insecure remote-host mode, authentication or
  Origin changes, relay/account support, or a new listener.

## Escalation And Autonomous Authority

Once implementation is explicitly authorized, it may autonomously choose
internal component/helper names, local union shapes, whether source-size
validation lives beside `torrentInput` or the API client, and the exact
headless fixture/harness placement. It may refactor the current Add callbacks,
`InspectionCommand`, generated caller DTO, receipt fingerprint construction,
gateway pending-upload record, and tests as needed while preserving the
contracts above. Same-boundary bugs and adversarial cases may be fixed without
additional direction.

Ordinary compiler errors, generated artifact changes, component-test
ergonomics, `File.arrayBuffer()` failures, headless filechooser mechanics, and
preserving a legacy receipt encoding do not require maintainer input.

Stop for maintainer direction if implementation would require:

- client-side source hashing or a secure-context prerequisite;
- removing the WebSocket ready admission step, introducing a new binary
  envelope/codec, or routing ordinary browser intake through HTTP;
- changing the 64-MiB source limit, 64-KiB text limit, one-host upload permit,
  or 120-second pending-body timeout;
- changing source SHA-256 persistence, v1 SHA-1 torrent identity, exact-source
  retention, request replay/conflict semantics, or the SQLite schema;
- adding visible picker affordances, progress/chunking, multiple files,
  pre-add catalog UI, auto-navigation, or another product behavior beyond the
  accepted empty-Add interaction;
- changing current hosting, authentication, Origin, private-host, or remote
  product policy;
- adding a runtime dependency with material security, license, maintenance, or
  bundle-size tradeoffs; or
- public-network access, visible/physical UI operation, deployment, destructive
  user data, or external coordination.

## Next Slice Boundary

After this tactical, browser and Tauri users can add a local `.torrent` through
the compact existing toolbar. Revisit chunking or progress only if measured
real sources make the accepted one-frame path materially slow, memory-heavy,
or unreliable; Tactical `081`'s current maximum-size evidence does not justify
that complexity.

Later presentation may make file intake discoverable, add drag-and-drop or
desktop open-with integration, and expose source export. Those slices must not
silently broaden Android intake, remote URL fetching, source provenance,
tracker execution, or v2 support.
