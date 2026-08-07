# Tactical 109: Stable Same-Origin Web Launch

Status: Complete

Topics: `client-surfaces`, `application-connection-architecture`,
`remote-access-authentication`

## Motivation And Observed Failure

The manual browser launcher currently starts a production Vite preview on a
stable loopback port and a separate unauthenticated application gateway on an
ephemeral loopback port. It encodes the gateway address into a `live` query
parameter so the static bundle can find the application. That address is
launcher plumbing exposed as product navigation: a restart chooses a new
gateway port, leaves an existing browser tab pointed at a dead endpoint, and
prevents the existing WebSocket reconnect path from recovering through the
stable visible origin.

The same launch exposed a second contract failure after ordinary incoming
listeners began binding wildcard IPv4 sockets. Runtime state truthfully
reported a concrete routed TCP peer address, a wildcard UDP bind address, the
same numeric port, and `coordinated_with_tcp: true`. The Rust contract defines
coordination by the shared numeric port, but the TypeScript semantic validator
incorrectly required the two rendered addresses to be identical and aborted
the application before React mounted. Its protocol-failure cleanup then asked
the browser WebSocket API to send reserved close codes that script callers are
not permitted to supply, adding misleading console exceptions.

## Desired Outcome And Stopping Condition

The visible manual-launch URL is exactly the stable configured origin, such as
`http://127.0.0.1:4177/`. One gateway process owns that listener, serves the
exact production web bundle, exposes the existing HTTP and WebSocket
application routes, retains local native download-directory selection, and
owns joined application shutdown. A browser tab that survives process restart
continues to target that origin and can use the existing reconnect/fresh-view
recovery path when the server returns.

Browser live bootstrap has no caller-selected gateway-address query mode.
Explicit demo mode and Tauri selection remain; an explicitly built hosted
browser bundle selects its own `window.location.origin`. The loopback HTTP
diagnostic transport may still be selected on that origin for controlled
comparison, but it cannot redirect application authority elsewhere.

The slice stops when the launcher uses one process and one stable origin, the
browser accepts the valid wildcard/concrete coordinated endpoint report, all
runtime and documentation references to explicit live-destination URLs are
removed, focused lifecycle and browser evidence passes, owning topics are
current, and the implementation is committed in independently reviewable
slices.

## Scope

- Correct `ClientSettingsRuntimeView` semantic validation so a coordinated
  UDP socket requires an active TCP listener on the same numeric port, not an
  identical rendered address.
- Add a regression case for concrete TCP plus wildcard UDP on one coordinated
  port while retaining rejection of a falsely coordinated different port.
- Use browser-sendable private WebSocket close codes for client-detected
  application-frame and policy failures.
- Remove explicit live-destination parsing and URL generation from the React
  bootstrap, gateway executable, launcher, controlled browser tests, and
  current documentation.
- Make hosted browser builds select the exact page origin at build time.
- Permit an explicit local hosted gateway mode to combine production assets
  with the existing loopback-only development authentication and native
  download-directory picker. Keep remote hosted deployments without ambient
  picker authority.
- Make `scripts/webui` build that hosted bundle, bind the gateway directly to
  its configured stable loopback port, wait for hosted health, open the root
  URL, and own one child process through bounded joined shutdown.
- Adapt controlled browser harnesses that retain a separate bearer gateway to
  proxy same-origin `/api` and WebSocket traffic during the test only. The
  browser contract remains same-origin in both production and evidence.
- Update launcher, client-surface, authentication, connection, and development
  documentation to the resulting behavior.

## Non-Goals

- No change to application commands, view-set semantics, reconnect cursors,
  authentication credentials, torrent networking, storage ownership, Tauri,
  Android, remote relay design, or payload serving.
- No automatic transport fallback, gateway discovery protocol, port scan,
  local-storage endpoint cache, service worker, redirector, or second browser
  control lane.
- No native folder picker for Basic-authenticated or browser-session remote
  hosted deployments.
- No promise that a different process already occupying the configured manual
  port is displaced. The launcher fails clearly and leaves that process
  untouched.

## Invariants And Security Boundary

1. Browser live authority is derived only from `window.location.origin` in an
   explicitly hosted build. Query input cannot select another host or port.
2. Ordinary non-hosted web builds keep the named demo default, and Tauri keeps
   its in-process adapter without opening a server.
3. The manual launcher binds exactly one IPv4 loopback address and its chosen
   fixed port. It never silently falls back to a different visible port.
4. Local hosted development retains the current exact Origin, Host,
   connection, message, call, view, queue, and lease checks. Serving the
   first-party bundle does not broaden application authority.
5. Native download-directory selection is present only for the explicit local
   hosted/development owner. Existing private or product hosted modes retain
   unavailable remote picker behavior.
6. TCP/UDP coordination means the actual bound numeric ports match. Wildcard
   and concrete routed address representations may legitimately differ.
7. A client-detected invalid frame fails pending work and closes the socket
   with a browser-permitted private code. It does not throw before initiating
   closure or convert invalid data into accepted state.
8. Explicit diagnostic HTTP selection remains same-origin, loopback-only, and
   immutable for one application session.

## Owner And Lifecycle Map

```text
scripts/webui
  -> locked web dependency/build step
  -> gateway build step
  -> one fixed-origin rstorrent-gateway child
       -> immutable production asset root
       -> native local download-directory picker
       -> ApplicationService
       -> HTTP application routes
       -> multiplexed application WebSocket
  -> readiness probe against hosted health/root
  -> optional operating-system browser opener
  -> INT/TERM cleanup
       -> gateway signal
       -> bounded wait and escalation only for failed join

browser tab
  -> stable page origin
  -> same-origin application client
  -> existing reconnect, retained-view resume, or fresh-view recovery
```

No runtime discovery file or mutable endpoint registry is introduced. The
configured visible origin is the recovery rendezvous.

## Implementation Slices

1. Record this tactical and accepted contract.
2. Correct coordinated endpoint validation and browser close behavior with
   focused TypeScript tests.
3. Remove explicit live destinations from browser bootstrap and gateway URL
   generation; add local hosted gateway composition and focused Rust tests.
4. Collapse `scripts/webui` to one fixed-origin gateway and migrate controlled
   browser harnesses to the same-origin contract.
5. Run layered validation, update the owning topics and completed tactical
   evidence, and commit the final record.

Each slice must remain independently reviewable. Unrelated in-progress
Tactical 108 work in the shared checkout is preserved and excluded from these
commits.

## Validation

- TypeScript bootstrap tests prove demo, hosted same-origin, and Tauri
  selection without an explicit destination parameter.
- TypeScript semantic tests accept the real wildcard/concrete coordinated
  snapshot and reject a different coordinated port.
- WebSocket adapter tests assert only browser-sendable close codes.
- Gateway tests prove local hosted assets, hosted health, exact Origin/Host,
  native picker selection, fixed development loopback binding, and unchanged
  remote hosted picker denial.
- `bash -n scripts/webui` plus an isolated `--no-open` lifecycle smoke proves
  the root URL, hosted asset, application hello/WebSocket, one listener, fixed
  restart address, and joined shutdown without opening a visible browser.
- Controlled Playwright live and authentication paths navigate the root or
  diagnostic same-origin query only and retain exact semantic transport
  evidence.
- Repository search finds no explicit live-destination URL or bootstrap
  parameter outside this tactical's historical problem statement.
- Run `cargo fmt --all -- --check`, warning-denying workspace Clippy, workspace
  tests, web typecheck, web tests, production build, proportional controlled
  browser evidence, shell syntax, and `git diff --check`.

Public swarms, a visible desktop client, Android, physical devices, and an
external deployment are not required for this launcher and contract slice.

## Completion Evidence

Completed on 2026-08-07 in these implementation slices:

- `2787823` recorded this accepted contract before implementation.
- `66ae727` corrected coordinated endpoint validation and replaced the
  browser-reserved WebSocket close codes with private application codes.
- `493b619` removed caller-selected browser destinations and added the fixed
  local hosted gateway composition and tests.
- `ea959cd` collapsed `scripts/webui` to one hosted gateway and made the
  controlled browser harness same-origin through a test-only reverse proxy.
- `159ef30` updated launcher guidance, living architecture topics, and
  historical tactical references to the stable same-origin contract.

The production launcher was exercised twice on isolated port `44177` with a
temporary profile while the active port `4177` process remained untouched.
The gateway served the root document and exact `local-webui` health identity;
an owner-bearing application hello succeeded. Headless Chrome navigated only
to `http://127.0.0.1:44177/`, rendered the transfer grid, and opened
`ws://127.0.0.1:44177/api/v1/connect`. With that same tab held open, the first
launcher was stopped and a second launcher took the same port. The unchanged
page opened a second same-origin WebSocket and reported successful reconnect
without reload or navigation. Both launcher generations joined on `Ctrl+C`,
and the temporary profile was removed.

Validation passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings`;
- `cargo test --workspace`;
- all web unit/component tests: 34 files passed, 2 skipped; 222 tests passed,
  2 skipped;
- `npm run typecheck` from `clients/web`;
- the same-origin production build and CSP check;
- focused bootstrap, coordinated-listener, private-close-code, fixed
  development-bind, and local-hosted gateway tests;
- `bash -n scripts/webui` and `git diff --check`; and
- a repository-wide fixed-string search with no remaining destination query
  syntax.

The libtorrent-backed controlled browser scenario was not runnable because
the host Python environment lacks the `libtorrent` module; it failed at import
before starting any process or modifying scenario state. Its browser URLs and
transport observations were migrated to the same-origin contract, while the
real hosted Chrome lifecycle above supplies the required end-to-end launcher
and reconnect evidence. Public-swarm transfer evidence was not run.
