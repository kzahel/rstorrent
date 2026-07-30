# Application Control

Topic: `application-control`

Status: Tactical `007` implemented the first transport-neutral semantic
control contract and in-process application service. Tactical `008` added
recoverable reactive views and browser, Tauri, and Android adapters. No stable
public wire format is accepted yet.

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

Storage roots and platform capabilities are installed when an application
service instance is constructed or through a later platform-specific
capability operation. A remotely meaningful command selects an established
root identity; it never supplies an ambient local path or open descriptor.

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
becoming the owner of torrent state. Its wire versioning, authentication,
discovery, wake-up relay, event cursors, and exposure policy require a
separate tactical and threat model.

## Invariants

- Every mutation has one application-service instance and one profile
  database as its authority.
- Request correlation survives asynchronous execution and retry.
- A rejected stale revision cannot partially change durable or engine state.
- Snapshots are coherent at one service revision; events may later optimize
  updates but cannot be the only recovery mechanism.
- Local and diagnostic callers do not gain alternate privileged code paths.
- Shutdown, pause, profile close, and task failure have observable terminal
  states and joined owners.
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

Later work must define multi-torrent scheduling, stable product error
taxonomy, capability installation, removals and deletion, production remote
authentication and relay semantics, and compatibility rules for any
published wire protocol.

The implemented subscription and client direction is recorded in
[`client-surfaces.md`](client-surfaces.md) and
[`../tactical/008-reactive-multi-surface-control.md`](../tactical/008-reactive-multi-surface-control.md).
Coherent snapshots remain recovery authority above typed patches and
independent bounded subscriber state. The WebSocket adapter is not the
application authority, and local Tauri control does not use networking.
