# Tactical 076: Authenticated Private Web Host

Status: In progress from maintainer direction on 2026-08-04.

Topics: `application-connection-architecture`, `client-surfaces`,
`download-roots`, `capability-readiness`

## Decision And Motivation

Add one deliberately bounded browser host for a maintainer-operated private
deployment. It serves the production React bundle and the existing typed
application WebSocket from one process and one origin, runs the engine with a
separate durable profile and an explicitly configured payload root, and
requires HTTP Basic authentication before static, health, HTTP API, or
WebSocket access.

Machine names, addresses, domains, credentials, service definitions, DNS,
reverse-proxy configuration, deploy checkout paths, and runtime paths remain
outside this repository. RSTorrent owns only the generic authenticated host,
build-time same-origin bootstrap, lifecycle, and executable evidence.

This is a private preview for maintainer testing from a phone. It is not a
public remote-administration product, relay, pairing system, stable API, or
release-readiness claim.

## Desired Outcome And Stopping Condition

The tactical stops when:

- one release binary can serve an exact production web root plus the existing
  `/api/v1/connect` application WebSocket on one configured address;
- every hosted route requires one bounded Basic credential, with constant-time
  comparison and a browser-compatible challenge on failure;
- the WebSocket still requires the exact configured Origin and does not place
  a credential in a URL, generated asset, application frame, or log;
- a production-only web build can select its own HTTPS origin as the live
  application endpoint while ordinary no-mode builds remain the named demo;
- the process handles both interrupt and terminate signals, cancels the
  gateway, joins application shutdown, and leaves durable restart authority
  to the existing application service;
- the service accepts an explicit configured payload root and never invents a
  hidden product download directory;
- static paths have exact-file behavior and missing assets return `404`
  rather than the application shell;
- deterministic Rust and TypeScript tests cover authentication, Origin,
  static hosting, same-origin bootstrap, and unchanged loopback behavior;
- a production build and isolated process smoke pass; and
- the external deployment activates only an exact validated pushed revision,
  keeps profile/content outside its source checkout, and passes authenticated
  local and public health plus WebSocket evidence.

## Scope And Invariants

- Extend the existing gateway rather than adding another semantic API.
- Add one explicit Basic authentication mode. Bearer and unauthenticated
  loopback-development modes retain their existing behavior and bounds.
- Apply Basic authentication before every route, including static files and
  the WebSocket upgrade. Reject missing or wrong credentials without revealing
  which field differed.
- Keep application WebSocket frame, connection, call, attachment, message,
  snapshot, queue, cursor, and lease limits unchanged.
- Serve only a configured directory whose index exists at startup. Do not
  expose the repository, profile, payload root, logs, or arbitrary filesystem
  paths.
- A hosted build may default to `window.location.origin` only when explicitly
  selected at build time. The client accepts that endpoint only for exact
  same-origin HTTPS; explicit loopback HTTP remains available for local
  development.
- Basic credentials enter through bounded runtime configuration. The password
  may be read from a local file; it must not appear in arguments, logs,
  generated assets, checked-in fixtures, or repository documentation.
- A reverse proxy may terminate public TLS. The application still verifies
  the browser Origin and its own Basic credential so direct access does not
  bypass the intended door.
- A configured storage root is an explicit deployment capability. Remote UI
  may use established roots but does not gain an ambient path field or remote
  native-picker authority.

## Owner, Task, And Cancellation Map

```text
process owner
  -> immutable production web root
  -> Basic authentication configuration
  -> ApplicationService (durable profile + configured root)
  -> GatewayServer
       -> static request service
       -> existing HTTP application adapter
       -> existing multiplexed WebSocket connection owners
  -> interrupt/terminate cancellation
       -> gateway graceful shutdown
       -> ApplicationService joined shutdown
```

The external service manager owns crash restart and release activation. The
deployment worker owns its exact source revision, build process, smoke process,
release staging, activation, and status record. Neither owner mutates the
developer checkout or the durable profile.

## Resource And Security Bounds

- Existing maximum connections, calls, view attachments, frame bytes, response
  bytes, view-set count, queue bytes, and leases remain unchanged.
- Basic username is at most 64 bytes, contains no colon, and is nonempty.
- Basic password is 1 through 128 bytes after removing one conventional line
  ending from its local file.
- Encoded authorization input is bounded before comparison.
- Hosted build identity is printable ASCII and at most 128 bytes.
- Static service resolution remains beneath its configured root and follows
  `tower-http` exact-path behavior.
- Authentication is an intentionally small private-preview boundary. It does
  not satisfy future relay requirements for device identity, pairing,
  end-to-end encryption, replay protection, capability authorization, or
  credential rotation.

## Stable Scenarios And Evidence

- Missing and incorrect Basic credentials receive `401` and a Basic challenge
  for `/`, `/healthz`, HTTP API calls, and WebSocket upgrade.
- Correct Basic credentials serve the production index, health/build identity,
  current hashed assets, and one authenticated WebSocket connection.
- A wrong Origin remains forbidden after successful Basic authentication.
- A missing static asset returns `404` and never returns `index.html`.
- Bearer-authenticated and unauthenticated loopback gateway tests continue to
  pass unchanged.
- An ordinary no-mode frontend build still opens the named demo; the explicit
  hosted build opens the same-origin live application over `wss`.
- Interrupt and terminate both lead to gateway cancellation, connection
  cleanup, and joined application shutdown.
- Restart reopens the same isolated profile and configured root without using
  the source or release directories as application data.
- Failed, rejected, or superseded pushes do not activate a release. Quick
  successive accepted pushes converge on the newest desired revision.

## Non-Goals

- Relay hosting, public accounts, OAuth, passkeys, device pairing, per-command
  authorization, multi-user tenancy, end-to-end encryption, or stable public
  compatibility.
- Torrent payload transfer, playback, upload, or filesystem browsing through
  the application connection.
- Remote creation or repair of native filesystem roots.
- Tauri, Android, extension, ChromeOS, or installer changes.
- Zero-downtime backend replacement or compatibility between an old open tab
  and every unreleased application-contract change.
- A hosted service tied to any particular domain, address, machine, user,
  credential, service manager, reverse proxy, or DNS provider in this
  repository.

## Validation Plan

1. Add Basic configuration and request middleware with pure/configuration and
   live HTTP/WebSocket tests.
2. Add bounded static hosting and exact-file/health tests.
3. Add explicit same-origin hosted bootstrap with TypeScript and component
   coverage while retaining demo and Tauri selection order.
4. Add terminate-signal handling and an isolated release-process smoke with a
   temporary durable profile, fixed root, web root, and secret file.
5. Run formatting, warning-denying clippy, workspace tests, web typecheck,
   web tests, production build, and Git whitespace checks.
6. Install and validate the externally owned exact-SHA deploy worker, service,
   reverse proxy, DNS, authentication, restart, and public WebSocket path.

## Deliberate Deferrals

Client build polling, retained prior hashed assets, automatic reload notices,
zero-downtime process handoff, and stronger authentication remain follow-up
work. The initial deployment prefers a short explicit reload after a push over
claiming compatibility across unreleased contract changes.
