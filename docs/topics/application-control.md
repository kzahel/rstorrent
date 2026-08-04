# Application Control

Topic: `application-control`

Status: Tactical `007` implemented the first transport-neutral semantic
control contract and in-process application service. Tactical `008` added
recoverable reactive views and browser, Tauri, and Android adapters. Tactical
`012` added bounded typed diagnostics, derived progress assessment, and prompt
task-terminal supervision with isolated headless presentation evidence.
Tactical `013` added explicit application network configuration and a blocked
offline prerequisite without changing durable torrent intent. Tactical `033`
implemented the leased application-view owner, generated v1 JSON contract,
authenticated loopback polling adapter, and headless lifecycle client recorded
in [`application-view-api.md`](application-view-api.md). Tactical `040` adds
durable archive/restore and fenced removal with explicit keep-data or
delete-managed policy. Tactical `046` makes a successful pause receipt follow
the joined metadata/content supervisor result and its final empty peer
observation. Tactical `048` adapts the same commands and leased view sets to
the in-process Tauri product without introducing a local server. The live web
adapter now gives each application instance a random request-ID namespace so
durable receipts do not conflict across reloads or tabs. Tactical `060`
carries the same typed calls over one bounded loopback WebSocket without making
connection correlation the owner of durable request identity. No stable public
remote wire format is accepted. Tactical `049` evolves diagnostics into
hierarchical structured records with explicit capture interest and distinct
source, delivery, and local loss while keeping them separate from commands and
product-state authority. Tactical `063` adds metadata-only magnet intent and
durable live `Normal`/`Skip` file selection through the same semantic command
boundary.
Tactical `073` adds `force_recheck` as a semantic durable command with the
same expected-revision and request-receipt rules. It joins an active matching
generation, preserves durable run intent, and starts the common managed-
storage check without exposing paths, handles, or engine tasks.
Tactical `075` keeps that semantic contract and its request receipts intact in
an explicitly selected, private, bounded in-memory application-state mode.
SQLite `FULL` now has the typed `resource_limit` response classification in
both persistence modes. Planned Tactical `081` adds one byte-bearing v1
metainfo operation beside the JSON command union: source length and SHA-256
join semantic request identity, while WebSocket, HTTP, and Tauri adapters carry
the bytes without paths or base64.

## Scope

This topic owns the application-facing command, response, snapshot, and event
model above the torrent engine. It also owns the boundary between those
semantics and transports such as an in-process UI adapter, a diagnostic
process stream, or a future authenticated remote connection.

It does not authorize a daemon, listener, relay, account system, remote
authentication design, or payload traffic across the control boundary.

## Direction

Android, desktop, CLI, and application-level integration tests should drive
the same application service through the same typed semantic contract. The
local product still runs that service and the engine in-process. Sharing
semantics does not require local networking or serialization.

The initial contract has:

- a versioned request envelope with a caller-supplied request identity;
- an optional expected revision for rejecting stale mutations;
- typed commands referring to torrent and storage-root identities rather than
  sockets, file descriptors, paths, or platform objects;
- a correlated typed success or structured error response;
- monotonically increasing service revisions and complete bounded snapshots;
  and
- idempotent desired-state operations or persistent request deduplication
  where retrying a command could otherwise duplicate durable intent.

Commands express application intent. The application service translates that
intent into durable state and engine lifecycle operations. Peer messages,
piece buffers, SQL rows, logs, and task handles are not part of the contract.
Structured observability remains separate from command responses and product
state.

A bounded byte attachment may accompany a closed semantic operation when the
intent itself is binary, as for planned `.torrent` intake. The request still
owns version, request ID, optional expected revision, established storage-root
identity, start intent, selection, exact source length, and digest. Durable
receipt replay compares the digest and normalized options rather than storing
bytes in JSON. Adapters may frame or carry the attachment differently, but no
adapter gains alternate mutation, duplicate, revision, or storage policy.

Application views may expose a derived progress assessment without promoting
it to a second durable state machine. The assessment distinguishes an active
owner, an automatic mechanism or scheduled retry that is still waiting,
external blockage where no installed mechanism can advance, and deliberately
inactive torrents. Failure or exhaustion of one tracker, peer, or discovery
mechanism is not itself a torrent error and is not blockage while another
automatic mechanism can still act.

Application network permission remains separate from each torrent's desired
running or paused state. An offline policy prevents DNS and socket work and
reports `network_disabled` with an `enable_network` action; it does not turn
the torrent into an error or durable pause. Future Android connectivity,
metered-network, and VPN settings should combine platform facts and user
preferences in an application-level owner, then change the engine permission
without rewriting torrent intent.

Typed diagnostics use a separate bounded reactive projection. They may explain
the facts behind a progress assessment, but clients do not parse diagnostic
text to determine torrent state, available actions, or correctness. A view
begins from bounded recent history, filters capture before its transport
queue, detects overflow or sequence loss, and can resynchronize independently
from product-state views. Hierarchical categories and typed subjects/fields
are deliberately projected application semantics rather than arbitrary Rust
debug values.

Detailed clients aggregate their currently relevant named projections into a
leased view set. One view set owns an epoch, opaque cursor, bounded diff
accumulator, and independent recovery state. Periodic pull and later streaming
drain the same semantic update batches; transport authentication and wire
encoding remain adapters. View-set identifiers are resource locators, not
authentication credentials or durable application state.

Storage roots and platform capabilities are installed when an application
service instance is constructed or through a later platform-specific
capability operation. A remotely meaningful command selects an established
root identity; it never supplies an ambient local path or open descriptor. The
accepted first-add, default-root, local picker, and remote-presentation behavior
is recorded in [`download-roots.md`](download-roots.md).

Tactical `062` keeps publication naming behind this boundary. Verified
metainfo and the selected root resolve a durable relative publication
component inside the application/engine layers; no presentation command gains
a local path, proposed filename, or infohash-layout switch.

Tactical `063` keeps file selection semantic as well. The command carries a
bounds-checked torrent identity, file indices, and `Normal` or `Skip`; it does
not expose numeric engine priorities, storage paths, part slots, or picker
internals. Selection commits before the active immutable engine generation is
cancelled and joined. Metadata acquisition may continue for a paused
start-content intent, but content storage remains unopened until explicit
resume.

Force recheck is likewise semantic. A replayed successful request cannot
start another generation, and a stale request mutates neither runtime nor
durable state. During checking, presentation exposes no old verified total as
current authority. Valid paused content may return to complete; invalid paused
content retains its exact replacement bitmap without admitting peer repair,
while running intent downloads only the missing or corrupt wanted pieces.
Android application request IDs now include a process-random namespace before
their monotonic suffix, matching the browser contract and preventing a
restarted process from reusing `android-1` for different durable intent.

Authorization is transport context, not a user-supplied command field. A
future remote transport must authenticate a principal, attach verified
capabilities to dispatch, apply replay and rate limits, and redact sensitive
source data. The application service must not trust an `is_admin`-style value
inside an envelope.

## Compatibility Posture

The Rust semantic types may evolve while there is only an in-process client
and repository diagnostic. Serialization used by tests is a versioned
diagnostic encoding, not yet a public compatibility promise.

A future remote protocol should adapt to the semantic dispatcher rather than
becoming the owner of torrent state. The initial internal v1 shape, generated
TypeScript and JSON Schema, additive compatibility rules, and polling-to-stream
delivery model are recorded in
[`application-view-api.md`](application-view-api.md). Production
authentication, discovery, wake-up relay, and exposure policy still require a
separate threat model.

Successful commands should evolve toward command-specific results plus the
resulting durable revision rather than returning a complete service snapshot
after every mutation. Views remain the authoritative state-recovery path.
This change is internal while no stable public wire promise exists and must be
made with reducer and retry evidence rather than as an incidental transport
optimization.

## Invariants

- Every mutation has one application-service instance and one session
  database as its authority, either its durable profile database or its
  private ephemeral database.
- Request correlation survives asynchronous execution and retry.
- A rejected stale revision cannot partially change durable or engine state.
- Snapshots are coherent at one service revision; events may later optimize
  updates but cannot be the only recovery mechanism.
- Local and diagnostic callers do not gain alternate privileged code paths.
- Shutdown, pause, profile close, and task failure have observable terminal
  states and joined owners.
- A task terminal result is observed without requiring a later client command.
- Blocked progress is asserted only when no installed or scheduled automatic
  mechanism can provide the next prerequisite.
- Temporary application network restriction does not rewrite a torrent's
  desired running or paused state.
- User-controlled magnets, paths, peer hints, and errors are bounded and are
  not emitted unredacted as routine logs.

## Implemented Thread And Gaps

[`../tactical/007-durable-session-control.md`](../tactical/007-durable-session-control.md)
implemented the first envelope and in-process dispatcher alongside durable
magnet resume. The Rust dispatcher and newline-delimited JSON diagnostic use
the same request and response types. Unit and forced-process-death evidence
cover request correlation, persistent duplicate replay, request-ID conflict,
stale revision rejection, coherent snapshots, pause/join, shutdown/join, and
restart through the same commands.

The diagnostic encoding remains repository test infrastructure. Tactical
`008` moved the new Android product path and Tauri shell onto the application
service and adapted the same semantic contract to a bounded loopback WebSocket
gateway. Generated TypeScript and Kotlin values, independent reactive
subscriptions, explicit resynchronization, and controlled real downloads now
provide cross-client executable evidence.

Tactical `037` routes the new React toolbar's bounded magnet intent through
that generated `add_magnet` contract. Input convenience checks improve local
feedback, but the application service remains authoritative for syntax,
resource bounds, durable duplicate handling, storage policy, and busy state.
Remote `.torrent` URL fetching and file-byte intake remain absent rather than
being represented as successful magnet adds.

Tactical `040` makes archive orthogonal to running intent and gives removal a
durable generation. The application service first persists paused removal
intent, joins any active torrent owner, and then either retains payload,
deletes exact path-backed managed artifacts, or waits for a trusted platform
adapter. Failed cleanup stays visible and can be retried with either retention
policy. Android SAF plans and confirmations are in-process operations keyed by
the matching generation; browser commands never carry paths or document URIs.

Tactical `046` corrects the ordinary pause join beneath that application
contract. The application still persists paused intent first, requests safe
cancellation, and awaits the active task. Engine wrappers no longer race and
drop the metadata/content supervisor; the supervisor publishes disconnecting
and final removal observations after joining child owners. A deterministic
session test proves the pause response follows TCP close, and the same leased
view set receives the peer removal plus zero active-peer and payload-rate
summary fields without another command or a presentation-side clear. A
terminal owner-cleanup failure is recorded and propagated through pause rather
than being accepted as successful joined cleanup.

The live web adapter gives every application instance a random 128-bit request
namespace followed by a monotonic sequence. This preserves durable retry and
correlation semantics without reusing `web-1` for an unrelated command after a
reload or in another tab.

Tactical `063` implements live path-backed file selection as a deliberately
coarse control fence. The store validates all targeted non-padding files and
commits the sparse selection atomically. A matching active owner is then
cancelled and joined before a replacement generation rechecks and starts under
the new immutable plan. All-skipped content becomes idle without losing
running intent, while promotion from a complete selection returns to checking.
Dynamic platform-capability selection fails closed until descriptor reacquire
and provider lifecycle have their own design.

Later work must define multi-torrent scheduling, stable product error
taxonomy, capability installation, production remote
authentication and relay semantics, and compatibility rules for any
published wire protocol.

[`../tactical/012-bounded-diagnostics-progress.md`](../tactical/012-bounded-diagnostics-progress.md)
records the completed application-control slice. It corrects command-driven
task-completion polling, adds a derived progress assessment, and carries typed
bounded diagnostics through generated browser/Tauri and Android contracts
without treating the diagnostic WebSocket gateway as a product daemon.

[`../tactical/013-explicit-live-network-policy.md`](../tactical/013-explicit-live-network-policy.md)
records explicit offline, loopback-only, and online engine policy selection.
The current application configuration is immutable for the service lifetime.
A later control slice must own safe runtime mutation, cancel active network
resources promptly, preserve torrent intent, and restart eligible work when
network prerequisites return. Android network binding and VPN leak prevention
require separate platform evidence.

[`../tactical/075-ephemeral-application-state.md`](../tactical/075-ephemeral-application-state.md)
adds an immutable persistence-mode choice at the same service-lifetime
boundary. Presentation and transport detachment do not close or clear an
ephemeral service; joined service shutdown remains the owner of final DHT and
speed-history flushes. Durable open or write failure never falls back to the
ephemeral mode.

[`../tactical/018-inspectable-metadata-acquisition.md`](../tactical/018-inspectable-metadata-acquisition.md)
adds a coherent read-only engine diagnostic snapshot through `DownloadControl`.
It contains the bounded peer registry and active/recent BEP 9 attempts needed
by headless investigations. It is not yet projected into the application
snapshot, generated web/Kotlin contracts, or product UI; that later projection
should select stable fields rather than expose engine internals accidentally.

The implemented subscription and client direction is recorded in
[`client-surfaces.md`](client-surfaces.md) and
[`../tactical/008-reactive-multi-surface-control.md`](../tactical/008-reactive-multi-surface-control.md).
Coherent snapshots remain recovery authority above typed patches and
independent bounded subscriber state. The WebSocket adapter is not the
application authority, and local Tauri control does not use networking.
Tactical `033` aggregates those subscriptions behind one leased view set and
preserves the same recovery invariant through authenticated polling. Tactical
`048` implements streaming as an interchangeable Tauri delivery adapter with
explicit post-application acknowledgements; browser WebSocket streaming
remains deferred.
