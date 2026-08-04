# Tactical 081: V1 Torrent Byte Intake

Status: Planned and authorized from maintainer direction on 2026-08-04.
Implementation has not started. This planning commit changes documentation
only.

Topics: `client-persistence`, `application-control`,
`application-connection-architecture`, `application-view-api`,
`tracker-discovery`, `protocol-support`, `client-surfaces`,
`capability-readiness`

Dependencies: completed Tacticals
[`060`](060-multiplexed-application-websocket.md),
[`073`](073-unified-storage-and-complete-recheck.md),
[`074`](074-context-specific-metainfo-limits.md),
[`075`](075-ephemeral-application-state.md), and
[`076`](076-authenticated-private-web-host.md) establish the application
connection, unified v1 storage path, context-specific parser limits, bounded
ephemeral persistence, and authenticated private browser host used by this
slice. Completed Tacticals [`011`](011-one-shot-udp-tracker.md),
[`014`](014-scheduled-udp-tracker-lifecycle.md), and
[`043`](043-live-tracker-inspection.md) establish bounded UDP tracker
retention, scheduling, and inspection.

## Decision And Motivation

Add one bounded v1 `.torrent` byte-intake operation to the application
service and adapt it to the ordinary browser WebSocket, an HTTP automation
endpoint, and the in-process Tauri desktop connection. The caller supplies
bytes; no portable command supplies a local path, file descriptor, document
URI, or remote URL.

The accepted boundary separates three kinds of data:

- **operational metadata** is the exact hash-authorized `info` dictionary,
  normalized tracker tiers and peer hints, file selection, verified-piece
  state, and user/runtime intent needed to operate and restart the torrent;
- **original source** is the bounded verbatim magnet string or complete outer
  `.torrent` byte sequence retained for provenance and later exact export but
  never required for runtime restart; and
- **synthesized export** is a later magnet or `.torrent` reconstructed from
  operational state and must not be described as the original source.

SQLite remains the one application-state authority in durable and ephemeral
modes. Durable mode retains an original `.torrent` as a BLOB in a source table,
not beside payload data. Ephemeral mode stores the same source row in its
bounded private in-memory database and loses it with the rest of that service
lifetime. This slice does not add a profile-private sidecar blob store,
payload-adjacent copy, export directory, or second catalog authority.

The explicit-import parser profile remains 16 MiB. Explicit selection or an
authenticated private host changes the applicable resource profile, not the
hostile-input posture: every byte, count, string, path, and tracker remains
bounded and validated before durable state or engine work changes.

## Desired Outcome And Stopping Condition

The tactical stops when all of the following are true:

- one transport-neutral application operation accepts bounded v1 outer
  metainfo bytes plus the existing request identity, expected revision,
  storage-root, start-content, and file-selection intent;
- exact source length and SHA-256 participate in durable request replay and
  conflict detection without placing the source bytes in the JSON command or
  request-receipt representation;
- one atomic session transaction creates the torrent identity, exact
  `raw_info`, zeroed verified-piece state, normalized source intent, metainfo
  tracker tiers, original source BLOB, file selection, and ordinary lifecycle
  state;
- a malformed, unsupported, oversized, duplicate, stale, digest-mismatched,
  canceled, or resource-exhausted intake changes neither revision nor torrent
  state and leaves no source artifact;
- schema migration preserves every existing torrent, labels its retained
  magnet canonicalized unless an unambiguous successful add receipt proves the
  verbatim submission, and removes the assumption that every torrent has a
  magnet source;
- new magnet adds retain the bounded verbatim submitted URI separately from
  normalized operational hints, while existing canonical-only rows remain
  truthfully distinguishable;
- imported and persisted `raw_info` may be up to 16 MiB under the explicit and
  durable profiles, while peer-controlled BEP 9 assembly remains at one MiB;
- restart re-hashes and re-parses exact `raw_info`, restores normalized
  operational discovery, and neither selects nor parses the original outer
  source BLOB;
- BEP 12 `announce-list` tier order and `announce` fallback are normalized
  under explicit bounds, UDP trackers execute through the existing manager,
  and retained HTTP/HTTPS trackers are truthfully visible as configured but
  unsupported rather than silently discarded;
- a private torrent with no supported discovery source imports successfully
  but projects an actionable unsupported-discovery explanation rather than
  pretending to download or treating source validity as tracker reachability;
- the ordinary browser connection accepts one declared, bounded binary upload
  without base64 while preserving the existing 64-KiB text-frame bound;
- an authenticated raw HTTP endpoint invokes the same semantic operation for
  automation without becoming the ordinary interactive browser lane;
- Tauri accepts a raw IPC body and invokes the same semantic operation without
  a loopback listener, path handoff, base64 expansion, or JSON number array;
- durable and ephemeral metadata-only, running, replay, restart, and
  exhaustion scenarios pass;
- controlled libtorrent evidence downloads and verifies content from an
  imported metainfo source through its outer UDP tracker configuration; and
- the validation matrix, owning topics, readiness scoreboard, generated
  contracts, and this execution record contain the exact final evidence.

The result is an API-level `.torrent` intake capability usable by the bounded
private browser host, Tauri adapter, and scripted HTTP clients. A visible
browser/Tauri file picker is deliberately the next presentation slice.

## Existing Baseline And Gap

The current application command union accepts `AddMagnet` but no byte-bearing
source. The `torrents` table requires `magnet TEXT NOT NULL`, stores optional
one-MiB `raw_info`, and reconstructs supported peer hints and UDP trackers by
re-parsing a canonicalized magnet. The submitted magnet's parameter spelling,
ordering, display name, unsupported fields, and equivalent encodings are not
retained verbatim.

`Metainfo::from_bytes_with_limits` can already identify and parse the exact
outer `info` span. Tactical `074` proves a parser-only 16-MiB explicit-import
profile and keeps generic bencode, BEP 9, and durable `raw_info` at independent
call sites. The session schema remains version 7 and constrains `raw_info` to
one MiB, so a large explicit parse cannot yet become durable application
state.

The ordinary browser uses one multiplexed `/api/v1/connect` WebSocket. Its
text messages, frames, and handshake are capped at 64 KiB, and any binary
message is currently fatal. HTTP routes share the same 64-KiB default body
limit. Tauri invokes typed JSON commands directly and has no byte-intake
command. The pinned Tauri 2.11 API supports raw `InvokeBody` bytes and exposes
them through `tauri::ipc::Request` on desktop; this slice must use that path
instead of serializing a byte array as JSON.

Tactical `076` already permits one maintainer-operated same-origin private
host behind HTTPS and Basic authentication. This intake operation may run
there under the existing authentication, exact-Origin, connection, and
storage-root capabilities. It does not broaden that host into a public remote
administration product.

## Accepted Source And Persistence Model

### Operational state and original source are distinct

The exact `raw_info` bytes remain the cryptographic and runtime authority.
They are extracted as a slice of the accepted outer source without
re-encoding, hashed with SHA-1 for the v1 identity, parsed under the durable
profile on restart, and stored with the ordinary piece count and zeroed have
state.

Outer source bytes are provenance only. They may contain unknown keys,
comments, creation fields, web seeds, DHT bootstrap nodes, unsupported tracker
schemes, and private tracker credentials. Preserving those bytes does not
claim operational support for their fields. Runtime startup and tracker
configuration use the hash-authorized `raw_info` and normalized rows, not a
fresh parse of the original source.

Source bytes and exact magnets are sensitive bounded user data. Routine
diagnostics, snapshots, tracker errors, HTTP errors, and logs must not emit
them or unredacted tracker credentials. A later explicit export/copy operation
may reveal source data to the requesting user; this tactical adds no such
operation.

### Schema direction

Advance the session schema from version 7. Exact table and column names may
follow existing store conventions, but the resulting relational contract must
express these values:

```text
torrents
  info_hash + runtime/resume/storage state
  raw_info BLOB <= 16 MiB
  no mandatory magnet source

torrent_source (one row per torrent in this slice)
  info_hash foreign key
  kind = magnet | metainfo
  source fidelity = verbatim | canonicalized
  exact bounded magnet text, or exact outer metainfo BLOB <= 16 MiB
  exact source byte length and SHA-256

torrent_trackers
  info_hash foreign key
  compact tier + position
  bounded normalized URL and transport
  source = magnet | metainfo

torrent_peer_hints
  info_hash foreign key
  bounded normalized host + port
  source = magnet
```

One source row is deliberate because duplicate info hashes remain errors.
Do not create a multi-source provenance ledger, source-merge history, mutable
tracker editor, or source replacement operation in this slice.

Migration parses each existing canonical magnet under the current bounded
magnet parser and moves its supported trackers and peer hints into normalized
rows. The bounded request-receipt table may still contain the successful
original `AddMagnet` envelope for some torrents, but receipts are evictable
retry infrastructure rather than source authority. Migration may label a
source verbatim only when one retained successful receipt unambiguously
matches the row and its parsed v1 identity; otherwise it creates a
`magnet/canonicalized` source record. New magnet intake creates a
`magnet/verbatim` source record while deriving the same normalized operational
rows. Migration is transactional and retains request receipts, revisions, raw
metadata, have state, selection, roots, archive/removal state, and every
existing restart invariant.

An implementation may retain a legacy column for one migration gate if doing
so makes rollback and evidence safer, but the completed read/write path may
not depend on a non-null magnet or use a canonical magnet as the only
operational tracker/peer representation.

### Database BLOB, not a sidecar

The exact outer metainfo lives in a separate table so ordinary startup,
snapshot, and resume queries do not materialize it. Its transaction, backup,
removal cascade, and ephemeral lifetime therefore match the catalog without a
cross-filesystem commit window. The accepted duplication of `raw_info` inside
the original source is simpler than prefix/suffix reconstruction,
compression, or a file-backed blob store for this bounded first slice.

SQLite page exhaustion in ephemeral mode maps to the existing typed
`resource_limit` response and rolls back the current transaction. Durable WAL
and `synchronous=FULL` behavior remain unchanged. No implementation may spill
an ephemeral source to a temporary file or silently omit exact-source
retention merely to make an otherwise oversized transaction succeed.

### Removal and corruption posture

Removing a torrent under the existing application policy cascades its source,
tracker, and peer-hint rows; the source is application state, not payload
data. Keeping or deleting managed payload does not preserve the catalog row.

Original-source corruption or absence must never invalidate independently
hash-authorized `raw_info` or verified payload. It removes the ability to
claim an exact future export and should be diagnosable when source access is
explicitly requested, but startup does not inspect it. Conversely, a valid
outer source cannot authorize corrupt `raw_info`, tracker rows, or have state.

## Metainfo And Tracker Semantics

### V1 identity and structural policy

Accept only a bencoded outer dictionary containing one supported v1 `info`
dictionary. Preserve the exact info span and reject v2 or hybrid identity,
unsafe paths, path collisions, invalid piece geometry, excessive files or
pieces, excessive depth/items/collections/strings, integer overflow, and all
other Tactical `074` parser errors before mutation.

The initial limits are:

| Resource | Maximum |
| --- | ---: |
| Outer upload and retained original source | 16 MiB |
| Explicit-import and durable exact `info` | 16 MiB |
| BEP 9 peer metadata | 1 MiB, unchanged |
| Files | 4,096 |
| Pieces | 52,428 |
| Tracker URLs retained for operation | 32 |
| Tracker URL bytes | 2,048 |
| File-selection entries | 4,096 |
| Concurrent buffered imports per application host | 1 |

All other depth, decoded-item, path-component, path-byte, and collection
limits remain those established by Tactical `074`. A later implementation may
tighten a transport timeout from measured evidence, but may not raise these
byte/count limits without direction.

The durable metainfo profile rises to 16 MiB because a durable row may now
originate from an explicit import. This does not change the BEP 9 advertised,
assembled, or interoperable maximum.

### BEP 12 tracker projection

Survey BEP 12 and pinned libtorrent behavior before finalizing the pure outer
projection. The accepted baseline is:

- walk `announce-list` tiers in source order;
- ignore malformed tier/entry shapes and invalid tracker URLs while
  preserving the original outer bytes;
- retain valid HTTP, HTTPS, and UDP URLs, deduplicating by normalized tracker
  identity at the first occurrence;
- compact nonempty retained tiers to monotonically increasing bounded tier
  numbers while preserving tier grouping and within-tier source order;
- use a valid top-level `announce` only when `announce-list` yields no valid
  retained tracker; and
- reject more than 32 valid unique trackers as a resource limit rather than
  silently truncating discovery intent.

Pinned libtorrent shuffles trackers within each tier before operation. The
RSTorrent manager may retain its deterministic/testable ordering and existing
selection policy, but it must preserve the tier groups and source attribution
and must record any intentional scheduling difference in this tactical.

Magnet trackers continue to form one synthetic tier. Metainfo UDP trackers
enter the existing bounded schedule with `Metainfo` source and their compact
tier. The schedule and runtime snapshots must no longer manufacture
`Magnet/tier 0` for every record. Existing maximum records, maximum eight
concurrent startup operations, retries, token cache, cancellation, and
session network policy remain unchanged unless reference evidence exposes a
tier correctness bug that must be fixed at this boundary.

HTTP and HTTPS tracker protocols remain absent. Persist those configured URLs
with explicit transport/source and project a credential-redacted display
identity plus unsupported status; do not send them to the UDP manager,
classify them as failed UDP operations, expose passkey-bearing paths/query
values, or drop them. A private torrent with only unsupported trackers has no
DHT fallback and must project an actionable discovery blockage. A non-private
torrent may still progress through DHT.

`url-list`, `httpseeds`, `nodes`, comments, creation metadata, collections,
similar-torrent fields, and other outer keys are retained only in the exact
source BLOB. Operational parsing, presentation, web-seed transfer, and DHT
bootstrap effects for those keys are non-goals.

## Semantic Application Operation

Introduce a typed `AddTorrentBytesRequest` or equivalently named value with:

- application API version;
- request ID;
- optional expected revision;
- storage-root ID;
- `start_content` intent;
- sorted unique skipped file indices;
- exact source length; and
- exact source SHA-256.

The byte payload is an attachment to that operation, not a field in the JSON
request or ordinary `Command` union. HTTP may let the server compute fields
that the WebSocket handshake must declare, but all adapters produce the same
internal operation identity before dispatch.

Generalize the durable receipt representation only as far as necessary to
record the operation kind, normalized semantic options, source length, and
source digest. Retrying the same request ID with the same options and bytes
returns the recorded response without creating another torrent or engine
generation. Reusing it with different options, length, or digest is a request
conflict. New metainfo and magnet receipts do not retain a second exact copy
of source input; legacy receipt compatibility and any migration-time verbatim
magnet recovery remain supported.

The service validates the storage root, request envelope, declared bounds,
digest, outer metainfo, exact info hash, tracker projection, and file indices
before opening the mutation transaction. The transaction then inserts all
catalog, source, operational discovery, metadata, selection, and initial-have
rows and advances the revision once. After commit, the existing application
reconciliation path owns metadata-ready paused/running state, storage
preparation, checking, download generation, observations, and terminal
cleanup.

`start_content=false` must create no payload, staging, part, or publication
artifact. `start_content=true` follows the current common storage path and
does not bypass full recheck, selection, root capabilities, or publication
rules. Skipped indices are validated against the parsed non-padding file
catalog, not only the global 4,096 bound.

An existing info hash returns the current duplicate error without attaching
the source, replacing trackers, satisfying a magnet's missing metadata, or
changing revision. Supplying `.torrent` metadata to an existing magnet is a
later explicit enrichment operation.

## Transport Contracts

### Shared ownership and cancellation

Each application host admits at most one buffered torrent import across its
byte-bearing adapters. The gateway shares one permit across HTTP and all
WebSocket connections. Tauri holds the equivalent permit beside its in-process
application service. Admission occurs before accepting or allocating the full
declared body.

```text
browser / HTTP / Tauri caller
  -> adapter upload admission (one buffered import)
  -> bounded source buffer + length/SHA-256 verification
  -> pure outer/source projection
  -> ApplicationService semantic mutation
       -> one SQLite transaction
       -> ordinary post-commit torrent reconciliation
```

Disconnect, timeout, invalid framing, or shutdown before a complete accepted
body drops the buffer, releases admission, and mutates nothing. Once a
complete operation has entered semantic dispatch, the application owner—not
the transport connection—owns it through commit or typed failure. A lost
response is recoverable by retrying the durable request ID. Shutdown must
cancel or join in-flight parsing/dispatch before closing SQLite and must not
commit after the service has entered its terminal state.

Potentially expensive parsing/hashing may run on a bounded blocking worker so
it does not stall the async executor. One accepted upload must have one
observable task owner and joined terminal result; do not detach unbounded
blocking work or add a generic job framework.

### WebSocket

Extend the closed application frame family with a bounded upload handshake:

```text
client text:  begin_torrent_upload(call_id, upload_id, request metadata)
server text:  torrent_upload_ready(call_id, upload_id)
client binary: exact outer metainfo bytes
server text:  ordinary correlated semantic result or typed call error
```

Names may follow existing generated conventions. The stable rules are:

- text frames remain bounded to 64 KiB and ordinary calls/views continue to
  use text JSON;
- WebSocket frame/message configuration permits one binary message of at most
  16 MiB, but manual validation must not accidentally raise the text bound;
- one connection has at most one pending upload and the gateway has at most
  one admitted buffered upload across connections;
- the binary body belongs to the sole pending upload and must exactly match
  declared length and SHA-256;
- the client waits for `upload_ready` before sending bytes;
- an admitted upload expires after 120 seconds without a complete body,
  releases its permit, and returns or records a bounded timeout outcome;
- unexpected binary data, oversize framing, repeated begin, conflicting IDs,
  or binary data during handshake remains a protocol violation;
- digest or semantic validation failure clears only that upload and yields a
  correlated bounded error without durable mutation;
- ping/pong, close, pending-call, call-byte, heartbeat, view attachment,
  outbound queue, and invalid-message limits otherwise remain unchanged; and
- the execution record measures control-message delay during a representative
  maximum-size upload and states the known one-frame head-of-line cost.

Chunking, resumable upload, progress events, multiple concurrent uploads,
temporary-file spooling, and a second browser connection are deferred.

### HTTP automation

Add authenticated `POST /api/v1/torrents` with
`Content-Type: application/x-bittorrent`. Use one bounded query/header
metadata representation for request ID, optional expected revision,
storage-root ID, `start_content`, and optional canonical comma-separated skip
indices. Exact field placement may follow Axum ergonomics, but it must be
documented, independently validated under the same semantic bounds, and
convenient for `curl` without multipart or base64.

The route receives a known-length or streamed HTTP body into the same bounded
in-memory representation, rejects over 16 MiB before or while collecting,
computes SHA-256, and invokes the common operation. Apply a route-specific
body limit without raising the 64-KiB default for JSON routes. Existing
Bearer, Basic, unauthenticated-development, exact-Origin/CORS, connection,
and owner-isolation behavior remains intact; Tactical `076` authentication
must cover this route before body processing.

This endpoint is for automation, diagnostics, and compatibility. The shared
interactive browser must use the ordinary WebSocket attachment rather than
silently opening a concurrent HTTP command lane.

### Tauri desktop

Add one in-process Tauri command that accepts request metadata and a raw IPC
body. The shared web adapter will eventually pass a browser-selected
`ArrayBuffer`; Tauri exposes it as `InvokeBody::Raw` through
`tauri::ipc::Request`. Reject JSON bodies for this operation so a 16-MiB file
cannot become base64 or a JSON number array by accident.

The Tauri adapter acquires its bounded import admission, validates the same
metadata, and calls the same application operation. It opens no socket and
does not receive or derive a filesystem path. A Tauri mock/raw-IPC test and
desktop compile/build evidence must prove the request body reaches the common
operation without launching a visible window.

Android's Tauri raw-body limitation is irrelevant because Android Compose is
not hosted by Tauri. A later Android SAF intent may read document bytes and
call the semantic application operation through a platform-specific adapter;
that is not part of this tactical.

## Reference Survey Required Before Implementation

Record exact findings and intentional differences from these sources before
finalizing state transitions:

### Normative specifications

- BEP 3 for v1 outer metainfo and exact info-hash identity;
- BEP 12 for multi-tracker tier structure and fallback intent;
- BEP 27 for private-torrent discovery gating; and
- BEP 9 only to confirm that its peer metadata remains an info-dictionary-only
  one-MiB path and does not gain outer-source semantics.

Use the checkouts pinned by `reference/pins.toml`.

### Pinned libtorrent `v2.0.13`

- `src/torrent_info.cpp` around outer `announce-list`, `announce`, nodes, web
  seeds, URL validation, tier attribution, and malformed-field tolerance;
- `src/load_torrent.cpp::update_atp` and `load_torrent_buffer` for moving outer
  trackers, tiers, web seeds, and DHT nodes into `add_torrent_params`;
- `include/libtorrent/add_torrent_params.hpp` for the separation among parsed
  metainfo, tracker tiers, resume fields, and payload `save_path`;
- `test/test_torrent_info.cpp` and `test/test_torrent.cpp` for malformed outer
  values, tracker URL schemes, whitespace, duplicate/invalid trackers, tier
  behavior, many files/pieces, and load-buffer cases;
- `include/libtorrent/session_params.hpp` and
  `examples/client_test.cpp::{resume_file,add_torrent}` plus its resume scan
  for the fact that session state does not own a torrent catalog and the
  embedding client chooses persistence; and
- `src/write_resume_data.cpp::write_torrent_file` only to distinguish later
  synthesis from retention of original source bytes.

Libtorrent is a completeness and edge-case oracle, not an architecture or
source donor. No source or fixture is copied without separate provenance and
license review.

### JSTorrent product history

Inspect the pinned sibling revision, especially:

- `packages/engine/src/core/bt-engine.ts` and `torrent-parser.ts` for byte
  intake and exact info-dictionary handling;
- `packages/engine/src/core/session-persistence.ts` for its explicit split
  among file-source torrent bytes, magnet info dictionaries, and mutable
  state;
- `packages/engine/src/node-rpc/controller.ts` and
  `packages/client/src/engine-manager/daemon-engine-manager.ts` for current
  remote/base64 and native event paths; and
- Android `LinkHandlerActivity.kt` and `PendingLinkManager.kt` for document
  intent and handoff lessons.

RSTorrent adopts the useful source/runtime distinction and browser-selected
byte semantics. It does not adopt base64 storage or transport, JavaScript KV
keys, an IO daemon, native-path RPC, or Android UI work in this slice.

## Shape-Changing Edge Cases That Must Land Now

- Existing version-7 rows use canonicalized magnets as catalog state; an
  evictable successful add receipt may prove the verbatim submission, but
  migration must not manufacture provenance when that evidence is absent or
  ambiguous.
- The outer dictionary may contain unknown or unsupported fields; exact
  retention must not turn them into runtime authority.
- `announce-list` may be present but contain no valid tracker, in which case a
  valid `announce` is the fallback.
- Empty/malformed tiers, duplicate URLs across tiers, leading whitespace,
  invalid schemes, excessive valid trackers, long URLs, HTTP-only private
  torrents, and mixed supported/unsupported tiers must have deterministic
  outcomes.
- A valid v1 info dictionary can exceed one MiB through bounded multi-file
  paths even though the piece-hash string remains structurally bounded;
  explicit import and durable restart must agree on its admissibility.
- A maximum-size source can exhaust a bounded ephemeral database; the whole
  mutation and revision must roll back while the service remains usable.
- Request replay compares source digest and semantic options. It cannot depend
  on the original bytes still being resident in a transport buffer.
- A duplicate upload cannot silently upgrade a magnet, replace source
  provenance, or merge tracker tiers.
- Source BLOB corruption cannot establish or remove runtime metadata
  authority, and startup must not materialize that BLOB.
- An HTTP/HTTPS tracker cannot be mislabeled UDP or failed merely because its
  transport is unimplemented.
- WebSocket text and JSON body limits must remain 64 KiB even though one
  declared binary/raw route accepts 16 MiB.
- Disconnect before the body completes and disconnect after semantic dispatch
  have different owners and must be tested separately.
- Tauri must use raw IPC rather than an implementation that appears binary in
  TypeScript but expands to JSON internally.

## Staged Implementation And Intermediate Gates

### Gate 1: Pure outer-source projection

Implement runtime-independent extraction of exact `raw_info`, v1 metainfo,
source digest, normalized tracker tiers, and bounded unsupported transports.
Complete the normative and pinned-source audit and pure adversarial fixtures.
No session, SQLite, async runtime, socket, or platform type enters this layer.

### Gate 2: Schema and source migration

Advance the schema, migrate existing canonical magnets truthfully, normalize
magnet operational hints, retain exact new sources, raise durable `raw_info`,
and prove atomic migration/rollback/restart/removal. Existing application
commands and durable/ephemeral behavior remain green before byte intake is
exposed.

### Gate 3: Semantic byte operation

Add request identity/digest receipts, atomic metadata-ready insertion,
duplicate/stale/error behavior, ordinary reconciliation, metadata-only and
running lifecycles, and bounded parse ownership. Prove direct in-process
durable and ephemeral operation before transport work.

### Gate 4: Tracker tiers and truthful unsupported state

Pass normalized UDP tier/source records into the existing manager, preserve
volatile lifecycle bounds, combine unsupported configured records into the
tracker view, and project private unsupported-only blockage. Complete pure,
scripted UDP, restart, and generated-contract evidence.

### Gate 5: WebSocket and HTTP adapters

Add shared gateway admission, one-frame binary handshake, route-specific raw
HTTP body handling, authentication-before-body behavior, timeout/cancellation,
protocol errors, replay after lost response, and resource high-water metrics.
Retain all existing connection/view tests.

### Gate 6: Tauri raw IPC

Add and prove the in-process raw-body command, shared semantic result, no-path
contract, and no-visible-window compile/mock evidence. Do not begin picker UI.

### Gate 7: Controlled interoperability and closure

Use an independently controlled UDP tracker and pinned libtorrent seed to
download exact content from imported outer metainfo. Run the complete matrix,
record high-water values and intentional reference differences, update topics
and readiness, and close the tactical only when no required row is missing.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure parser/source | Exact info span/hash, 16-MiB and one-byte-over limits, v2/hybrid rejection, structure/path/piece bounds, BEP 12 tiers/fallback/dedup/malformed cases, mixed transports, private flag, and deterministic source digest. |
| Migration/store | Version-7 canonical-magnet migration, verbatim new magnet, exact outer BLOB, 16-MiB durable raw-info path, transaction rollback, request replay/conflict, stale revision, duplicate identity, removal cascade, source-independent startup, and unchanged durable checks. |
| Ephemeral | Same schema and semantics in private memory, no profile/temp/source file, metadata-only empty payload root, source disappearance on close, page-cap rollback, typed resource limit, and following-call availability. |
| Application lifecycle | Metadata-only add, running add, skip validation, pause/resume/recheck/removal, lost-response replay, disconnect-before-dispatch, disconnect-after-dispatch, joined shutdown, and no second engine generation. |
| Tracker runtime | Metainfo tier/source scheduling, 32-record and eight-operation bounds, fallback/retry/cancel, unsupported HTTP/HTTPS projection, private unsupported-only blockage, mixed DHT/tracker behavior, and restart reconstruction without volatile history. |
| WebSocket | Ready/body/result sequence, exact length/digest, 64-KiB text bound, 16-MiB binary bound, unexpected/repeated frames, timeout, disconnect, global one-upload admission, heartbeat/control before and after upload, lost result plus replay, and queue/metric high-water values. |
| HTTP | Raw content type, bounded known/chunked body collection, route-only 16-MiB limit, semantic metadata validation, auth rejection before body work, existing auth modes and Origin behavior, replay/conflict, and script-friendly response. |
| Tauri | Raw `InvokeBody`, JSON-body rejection, shared request/result, exact bytes, admission release, no path/base64/number-array representation, mock invocation, and desktop compile/build without a visible window. |
| Interoperability | Runtime-generated or independently authored v1 metainfo points through a controlled UDP tracker to pinned libtorrent; RSTorrent imports, discovers, downloads, hash-verifies, publishes or retains metadata-only as configured, restarts, and removes with exact cleanup. |
| Resource profile | Parser time/RSS or allocator high water for size-heavy and structure-heavy accepted inputs, SQLite/WAL growth for a maximum retained source, ephemeral pages, one concurrent upload, WebSocket control delay, and zero leaked task/permit/buffer after every terminal case. |
| Workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, generated TypeScript/schema/Kotlin checks, web tests/typecheck/build, desktop checks, and `git diff --check`. |

No public swarm, fixture download, visible browser, visible Tauri window,
Android build/runtime, emulator, physical device, new external service, or
deployment mutation is required. Loopback sockets and controlled pinned
libtorrent execution are authorized. The implementation may add independent
fixtures generated from public protocol behavior; importing a reference
fixture requires provenance and license review first.

## Non-Goals And Deliberate Deferrals

- Visible browser/Tauri file picker, Add-dialog integration, upload progress,
  source filename presentation, drag-and-drop, or clipboard file handling.
- WebSocket/HTTP chunking at the application level, resumable upload,
  temporary-file spooling, streaming bencode, or SQLite incremental BLOB I/O.
- More than one simultaneous upload or a separate upload socket/data plane.
- Adding a `.torrent` by remote URL, fetching arbitrary URLs, filesystem
  browsing, or allowing remote callers to create storage roots.
- Android intent/SAF intake, ChromeOS extension handoff, desktop file
  association, operating-system share targets, or native picker behavior.
- Payload upload, seeding, playback, or HTTP content transfer.
- HTTP/HTTPS/WebSocket tracker execution, proxying, authentication extensions,
  scrape, web seeds, DHT bootstrap nodes from metainfo, or BEP 41 URL data.
- V2/hybrid torrents, piece layers, mutable torrents, or non-v1 integrity.
- Duplicate-source enrichment, magnet-to-file upgrade, tracker editing,
  source replacement, or a multi-source provenance ledger.
- Exact-source copy/export UI, synthesized `.torrent` export, configurable
  export directory, payload-adjacent copy, or profile-private sidecar store.
- Fast-resume redesign, optimistic trust, peer-cache persistence, transfer
  history, or replacement of SQLite with libtorrent-style resume files.
- Stable public wire compatibility, pairing, accounts, relay, per-principal
  authorization, TLS termination, credential rotation, or general Internet
  exposure beyond Tactical `076`.
- Implementing planned Tacticals `077`, `078`, or `080` as incidental work.

## Escalation And Autonomous Authority

Implementation may autonomously choose internal Rust names, module placement,
SQL column names and indexes, pure parser helper shapes, exact generated frame
names, the bounded HTTP query/header split, and test fixture construction. It
may refactor the current receipt, magnet normalization, tracker schedule, view
projection, gateway state, and Tauri adapter where necessary to establish the
accepted owners without widening public compatibility. It may add
adversarial cases implied by these invariants, correct a same-boundary bug
exposed by them, and update generated contracts and owning topics with actual
evidence.

Ordinary compiler errors, migration fixture changes, failed deterministic
tests, conservative error classification, Tauri raw-IPC ergonomics, Axum body
collection details, and internal differences from libtorrent do not require
maintainer input. Tightening an in-scope timeout or count below its stated
maximum from measured safety evidence is allowed if common `.torrent` intake
still passes and the execution record explains it.

Stop for maintainer direction if implementation would require:

- abandoning exact outer-source BLOB retention or making it runtime
  authority;
- a sidecar/blob filesystem, payload-adjacent source copy, or cross-store
  transaction;
- raising the 16-MiB import/durable limit or the unchanged one-MiB BEP 9
  limit;
- silently truncating valid trackers or accepting duplicate-source merge;
- implementing HTTP/HTTPS tracker transport, web seeds, v2/hybrid integrity,
  or chunked/resumable application uploads to meet the stopping condition;
- adding an external dependency with material runtime, security, license, or
  maintenance tradeoffs;
- changing the accepted private-host authentication or remote-product
  posture;
- visible UI, public-network, deployment, emulator, device, destructive user
  data, or external coordination not already authorized; or
- a product behavior materially different from the decisions above.

## Next Slice Boundary

After this tactical, the preferred next product slice is the shared
browser/Tauri Add flow: select a local `.torrent`, read it as an `ArrayBuffer`,
send it through the adapter-specific byte operation, reuse established
storage-root/start options, and report bounded progress and errors. That slice
may evaluate chunking from the one-frame latency and memory evidence recorded
here, but chunking is not a prerequisite unless the measurements contradict
this tactical's accepted bound.

Later source/export work may expose exact original magnet copy, exact original
`.torrent` download, synthesized export when no original exists, explicit
export directories, and duplicate metadata enrichment. Those operations must
continue distinguishing source provenance from runtime authority.
