# Tactical 101: First-Run Web Authentication

Status: complete

## Motivation And Desired Outcome

RSTorrent's browser gateway currently has bearer authentication for controlled
development, an explicitly unauthenticated ephemeral loopback mode, and HTTP
Basic authentication for the maintainer-operated private host. It does not
have a low-friction product answer for a person who starts the headless web UI,
opens it in a browser, later uses another browser profile, or returns after an
initial setup window has expired.

This tactical adds a deliberately lightweight local trust model. A fresh
profile opens a clearly identified ten-minute loopback onboarding window. The
first browser that completes the setup wizard may either keep loopback access
open or remember that browser and require a four-digit pairing code for later
browsers. The four-digit code is displayed by an already authorized browser
and entered in the new browser. If no authorized browser remains, restarting
the gateway with an explicit pairing-window switch provides local recovery.
No password is required by this slice.

The desired first-run experience is proportionate to the initial authority:
a fresh RSTorrent profile can control torrent intake and downloads but does
not begin with remote exposure, an owner account, or unrelated personal data.
The implementation must make the boundary understandable and recoverable
without presenting ordinary localhost use as a high-security account system.

## Dependencies And Current Boundaries

- [Application connection architecture](../topics/application-connection-architecture.md)
  owns the browser gateway, exact Origin behavior, multiplexed application
  WebSocket, and distinction from in-process Tauri.
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md) owns the adaptive
  shared React presentation and the Settings information architecture.
- [`../topics/remote-access-authentication.md`](../topics/remote-access-authentication.md)
  owns future password-authenticated E2E remote access, host and device
  identity, PAKE selection, relay behavior, and cryptographic resumption. A
  local cookie session or pairing code is not that authority.
- [`076-authenticated-private-web-host.md`](076-authenticated-private-web-host.md)
  established the existing Basic-authenticated HTTPS reverse-proxy deployment.
  This tactical preserves it as an explicit deployment mode.
- `crates/rstorrent-gateway/src/lib.rs` owns gateway configuration,
  authentication middleware, HTTP routes, hosted assets, and WebSocket
  admission today.
- `crates/rstorrent-gateway/src/main.rs` owns the current environment-driven
  executable. It has no durable product authentication state or pairing
  recovery switch.
- `clients/web/src/inspection/live/LiveApplication.ts` and the shared Settings
  surface own the browser connection bootstrap and user-facing controls.
- `scripts/webui` remains a maintainer development launcher. Its explicit
  ephemeral unauthenticated behavior is not the fresh headless-product
  default and need not acquire onboarding friction.

No BitTorrent protocol behavior is changed, so the pinned libtorrent oracle is
not applicable. Before finalizing cookie parsing and emission, implementation
must inspect the then-current HTTP cookie specification, browser Fetch and
WebSocket Origin behavior, and the exact Axum/tower-http header behavior in the
pinned workspace versions. These standards constrain browser mechanics; they
do not choose RSTorrent's product policy.

## Reference Dossier

YepAnywhere was inspected as a product and failure reference at local commit
`7e78c59c90086ab711bf26bb7500b1e57ac9f4f1`. It is not a dependency or wire
contract. Relevant paths are:

- `packages/server/src/auth/AuthService.ts`: persistent single-user auth state,
  opaque server-side sessions, expiry, and localhost-open policy;
- `packages/server/src/auth/routes.ts`: status, setup, login, logout, password,
  localhost-access, and cookie behavior;
- `packages/server/src/services/NetworkBindingService.ts`: durable listener
  state and CLI-override precedence; and
- `packages/server/src/routes/network-binding.ts`: authenticated runtime
  binding changes.

Adopt the separation between browser-cookie trust, local bootstrap authority,
network binding, and future remote cryptographic authentication. Do not copy
passwords in command arguments, a six-character password floor, an unbounded
session collection, silent binding changes, or YepAnywhere's SRP/session wire
shape.

Browser mechanics were checked on 2026-08-07 against
`draft-ietf-httpbis-rfc6265bis-22`, the WHATWG Fetch Living Standard, and the
WHATWG WebSockets Living Standard updated 2026-03-15. The cookie draft defines
the selected host-only, `HttpOnly`, `SameSite=Strict`, `Path=/`, and conditional
`Secure` behavior. Fetch requires explicit credential inclusion and exact
credentialed CORS responses; WebSockets constructs its Fetch-integrated
handshake with credentials mode `include` and an `Origin`. The implementation
therefore validates the exact configured Origin on every cookie-authenticated
mutation and WebSocket upgrade rather than treating cookie delivery alone as
request authority.

The exact locked framework behavior was inspected in Axum `0.8.9` /
axum-core `0.5.6` `extract/default_body_limit.rs` and tower-http `0.6.11`
`cors/mod.rs` plus `services/fs/serve_dir/mod.rs`. RSTorrent applies its own
64-KiB default semantic/auth body limit, the pre-existing separately admitted
64-MiB torrent-source override, explicit credentialed CORS origin, and outer
Host/Basic middleware. No source, fixtures, or specification prose were
imported.

## Product States

One profile has exactly one persisted local web-access policy:

| State | Meaning | Unauthenticated loopback behavior |
| --- | --- | --- |
| `unconfigured` | No first-run choice has been committed. | The backend admits loopback application access for the first ten minutes, while the first-party UI asks the user to complete the one-choice setup wizard before entering the application. Afterward only the authentication/onboarding shell and bounded public auth endpoints remain. |
| `local_open` | The owner chose convenience for localhost. | Full access remains available only through an exact loopback listener and allowed Origin. |
| `paired` | Browser sessions are required. | Static application/auth assets and bounded auth endpoints remain reachable; semantic HTTP and WebSocket access require a valid session. |

Restarting an `unconfigured` profile starts a new ten-minute window. Once
`local_open` or `paired` is committed, restart never silently returns it to an
unconfigured window. An explicit maintainer development mode remains separate
and does not persist a product policy.

The initial window is not a hidden race. The first-party UI displays the
wizard and countdown such as:

> Initial setup is open on this computer for 9:42. Complete setup to choose
> whether future browsers need approval.

Merely fetching an HTML document, preloading assets, or opening a WebSocket
does not claim the profile. A browser claims it only by explicitly completing
the wizard. The server-side window remains open to loopback semantic access so
automation and a concurrent setup page cannot be stranded, but the product UI
does not enter the application until the choice is committed. The first
completed wizard atomically commits the policy. Other concurrent attempts
reload the committed result rather than overwriting it.

## First-Run Wizard

The wizard presents two choices in this slice:

1. **Keep localhost open.** Any browser on this computer can use this
   loopback-only RSTorrent service without signing in. Host and Origin checks
   still apply.
2. **Remember this browser.** The completing browser receives a persistent
   session cookie. Other browser profiles must be approved with a four-digit
   code.

The copy explains that network and future remote access remain off regardless
of this choice. It does not imply that the paired option protects against an
attacker who already controls the local user account or RSTorrent process.

When `--open` launches a browser, the implementation may include a random
single-use bootstrap capability in the launch URL so the intended browser can
enter the wizard directly. It must redeem the capability once and immediately
remove it from the visible URL and browser history. Manual navigation to the
loopback URL remains fully supported and does not require typing that
capability.

## Settings Information Architecture

The current Settings surface is an accessible modal right-side sheet, but its
23rem single-column stack is already occupied by Appearance, Downloads, and
Connection & seeding. Session approval and revocation are management tasks,
not another fieldset to append below that scroll.

This tactical graduates Settings into one adaptive modal workspace:

- wide desktop and tablet layouts use a wider dialog with a persistent
  vertical category rail and one independently scrolling content panel;
- phone layouts use the full viewport and a compact horizontally scrollable
  category control above the active panel;
- the initial categories are **Appearance**, **Downloads**, **Connection &
  seeding**, and **Web access**; and
- category selection uses correct tab/list semantics, arrow-key navigation,
  visible focus, an active indication that is not color-only, and an
  associated labelled panel.

The modal retains Escape/backdrop dismissal, focus containment, close-button
initial focus, and focus restoration to the Settings trigger. Switching
categories neither commits nor discards a panel's draft. Existing Appearance,
Downloads, and Connection & seeding save/application behavior remains owned by
those panels rather than being replaced by one misleading global Save action.
Web-access approval, revocation, sign-out, and policy changes are discrete
confirmed operations with their own progress and result feedback.

The Web access panel shows:

- effective access policy and loopback/listener scope;
- whether this is the current authorized browser;
- **Approve another browser**, including the four-digit code and countdown;
- a bounded remembered-browser list with user-visible label, **This browser**
  marker, creation time, last-used time, and individual revoke action;
- **Revoke all other browsers** and **Sign out this browser**; and
- the exact `--pairing-window` recovery instruction, explicitly framed as the
  path to use after every authorized browser profile or cookie is gone.

The remembered-browser list is compact ordinary content, not a virtual table
at the 32-session ceiling. Destructive session actions require confirmation
proportionate to their effect. The current session cannot be revoked through
the row action; it uses the separately named sign-out action.

Web access is exposed only when the active application adapter reports gateway
authentication management capability. Tauri's in-process adapter, demo mode,
Basic-only deployments without cookie-session management, and bearer
automation do not display an inert Web access category. The onboarding and
unauthorized screens reuse the same policy labels and pairing components
without rendering the full Settings modal before application admission.

## Expired-Window And Unauthorized-Browser Experience

Static application assets and the minimal authentication state endpoint stay
available in paired and expired-unconfigured states. The browser must render a
purpose-built access screen instead of exposing a raw `401`, failed WebSocket,
blank application, or indefinite reconnect loop.

For an unconfigured profile whose ten-minute window expired, the screen says
substantially:

> Initial setup was available for 10 minutes and has now closed. Restart
> RSTorrent with the same profile to open a new 10-minute setup window.

The page links to concise restart help and explains that this works because no
access policy has yet been committed. It must not reveal the host filesystem
path.

For a paired profile opened from another browser or browser profile, the
screen says substantially:

> This browser has not been approved for RSTorrent.
>
> - In an approved browser, open Settings > Web access > Approve another
>   browser and enter the displayed code here; or
> - if no approved browser remains, stop RSTorrent and restart the same server
>   command with `--pairing-window`. Return here and choose Approve this
>   browser within 10 minutes.

The authorized Settings action creates one bounded four-digit ticket.
Successful redemption authorizes only the redeeming browser, consumes the
ticket, installs its cookie, and enters the application without requiring a
server restart. If the ticket expires or reaches its attempt limit, the UI
states that a new code must be generated by an approved browser.

The recovery switch does not create a separate administrative process, reset
the stored policy, revoke existing sessions, or make the application itself
temporarily open. On a loopback listener it opens only a ten-minute enrollment
window. The first unauthorized browser to explicitly choose **Approve this
browser** receives a session and atomically consumes the window. Fetching
assets or status, preloading, or merely opening a tab does not consume it. The
access screen shows the recovery countdown and says that another restart with
the switch is required after expiry.

An authorized browser can list remembered browser sessions by bounded label,
creation time, and last-used time, approve another browser, revoke one other
session, or revoke all other sessions. It cannot revoke its current session
without using the ordinary sign-out action. User-agent text is display
metadata, not identity or authority.

## Pairing And Session Contracts

### Pairing ticket

- The user-facing claim code is exactly four decimal digits, including leading
  zeroes. It is generated with the operating-system cryptographic RNG.
- A ticket is valid for ten minutes, authorizes one browser session, and is
  consumed atomically on successful redemption.
- One profile has at most one active ticket. Creating another invalidates the
  previous ticket and clearly says so at the generation point.
- Five incorrect redemption attempts invalidate the ticket. The failure count
  is aggregate across the ticket rather than reset by a new connection or
  browser storage clear.
- The stored record contains a one-way digest of the code plus random ticket
  salt, creation/expiry metadata, failure count, and purpose. Plaintext code is
  returned only once to the authorized browser that created it and is never
  logged.
- The code is entered in the unauthorized browser and displayed in an already
  authorized browser. The inverse flow is not part of this slice.
- Pairing requests and failures use bounded request bodies and a shared
  per-profile admission limit in addition to the five-attempt ticket ceiling.
  At most four redemption requests may execute concurrently.
- Pairing tickets exist only after `paired` is committed and an authorized
  browser requests one. An expired unconfigured profile instead uses the
  stated plain restart path; there is no provisional-session state.

Five guesses against 10,000 possibilities deliberately provide only a small
online barrier. That is accepted for this local, short-lived bootstrap flow;
the four-digit code must not be reused for nonlocal remote authentication,
password recovery, relay access, or capability URLs.

### Browser session

- A session token contains 32 random bytes. Only a one-way digest is persisted;
  equality is checked without early-exit secret comparison.
- A profile retains at most 32 sessions. Pairing fails with actionable session
  cleanup guidance when full rather than silently evicting an unrelated
  browser.
- A session has a bounded user-visible label of at most 80 UTF-8 bytes,
  creation time, last-used time, and revocation state. Supplied labels and
  user-agent-derived defaults are untrusted display text.
- The server expires a session after 180 days without successful use. Active
  use renews the idle deadline, making routine local use effectively
  persistent while still bounding abandoned credentials.
- Last-used persistence is coalesced to no more than one durable write per
  session per hour.
- The browser stores only the opaque token in a host-only `HttpOnly` cookie
  with `SameSite=Strict`, `Path=/`, no `Domain`, and an expiry matching the
  rolling server deadline. HTTPS deployments also set `Secure`. Script code
  cannot read or synthesize the credential.
- Signing out revokes the current server-side session and expires its cookie.
  Revocation takes effect for new HTTP requests and WebSocket handshakes;
  revoking a live session also closes its existing application connection
  promptly with a typed authentication terminal reason.
- Cookie state authenticates one browser to the directly reached gateway. It
  is not a device key, owner password, remote resume credential, or proof of
  host identity.

## Routes, Origin, And Public Surface

Cookie mode must separate public bootstrap delivery from authenticated
application authority:

- the production application assets, a bounded health/build response, auth
  status, first-run completion, ticket redemption, and sign-in shell are
  reachable without a session as required for onboarding;
- torrent commands, snapshots, view-set mutations, download-root operations,
  diagnostic HTTP, and the multiplexed application WebSocket require the
  selected policy;
- every cookie-authenticated state-changing request and WebSocket upgrade
  requires the exact configured Origin; and
- Host validation accepts only the configured host/origin relationship and
  loopback spellings deliberately enabled by configuration. DNS rebinding may
  not turn a loopback listener into a cross-site control endpoint.

Public auth responses reveal only the state needed to render the page:
`initial_window_open` with remaining seconds, `initial_window_expired`,
`local_open`, `session_required`, or `session_valid`. They do not expose
session tokens, ticket digests, profile paths, torrent state, session lists, or
configuration secrets.

Basic mode retains Tactical 076's whole-site challenge and exact HTTPS Origin
contract. Bearer mode retains its automation behavior. Authentication modes
do not silently stack and a browser is not asked to satisfy both Basic and a
cookie wizard in this slice.

## Listener And CLI Contract

The gateway executable gains an ordinary argument parser and help output while
retaining documented environment compatibility for existing scripts and the
private-host deployment. Exact internal parser choice may use a small existing
workspace dependency or a plain parser; adding a new parsing dependency
requires ordinary license/version review but not product escalation.

The product-facing behavior must cover these conceptual commands and options;
the implementation may choose exact hyphenation consistent with the existing
binaries and document it before completion:

```text
rstorrent-gateway serve
  --profile-root PATH
  --listen 127.0.0.1:3030
  --open | --no-open
  --pairing-window
  --auth auto | local-open | paired | basic | bearer | development-none
  --origin URL
  --basic-username NAME
  --basic-password-file PATH
  --bearer-token-file PATH
```

- `auto` is the default product policy. It follows persisted `local_open` or
  `paired` state and enters `unconfigured` onboarding for a fresh profile.
- The ordinary default listener is `127.0.0.1:3030`. Port zero remains an
  explicit test/development choice; a busy fixed port fails with a clear
  message rather than silently moving a bookmarked service.
- `local-open`, `paired`, bearer, and development-none modes may bind only an
  exact loopback address in this slice.
- A non-loopback unicast bind remains available only to the explicit Basic
  mode established by Tactical 076, with its exact externally configured HTTPS
  Origin and reverse-proxy assumption. Unspecified and multicast binds remain
  invalid.
- `development-none` is explicit, loopback-only, ephemeral, and never changes
  durable product auth state.
- Secret values are accepted from bounded files or existing compatible
  environment variables, never as literal command arguments.
- CLI values override environment values, which override persisted/default
  deployment values. A CLI listener or authentication override is visible in
  diagnostics and cannot be changed silently from the web UI.
- `--pairing-window` is valid only for a loopback `auto`/persisted-`paired`
  launch. It grants one explicit browser-session enrollment for ten minutes,
  is never persisted, and does not weaken semantic route admission before the
  new session exists. It is rejected for local-open, Basic, bearer,
  development-none, and non-loopback launches.
- The startup log and unauthorized browser page both state when the recovery
  window is active, how long remains, and that the first explicit approval
  consumes it. No cookie or pairing secret is printed.

A future unified `rstorrent web` spelling may wrap these behaviors. Creating a
repository-wide command hierarchy or migrating unrelated diagnostic binaries
is not required here.

## Persistence, Owners, Tasks, And Data Flow

Gateway authentication is host/application state, not torrent engine state.
It belongs in a dedicated gateway-owned SQLite store beneath the configured
profile root rather than in protocol/domain types or the torrent session
schema. Reusing the workspace's existing `rusqlite` dependency is authorized.

The runtime-independent auth module owns:

- persisted policy transitions;
- session-token digest and lookup records;
- pairing-ticket creation, attempt, expiry, and atomic consumption;
- injected clock and random-byte inputs for deterministic tests; and
- bounded public/authenticated snapshots with no Axum request types.

The gateway adapter owns cookie/header parsing, Host and Origin enforcement,
route admission, response cookies, and mapping authenticated sessions to
application connection authority. The React client owns onboarding,
unauthorized, expired-window, pairing, session-list, and revocation UX. These
layers depend inward on the pure auth decisions; the auth store does not
depend on Axum, Tokio tasks, WebSockets, React, or application view types.

One gateway process owns store access, pairing-window state, and bounded
last-used write coalescing. SQLite busy handling is time-bounded and returns
actionable failure. The server reads and consumes the current Settings-created
ticket transactionally during redemption; no background watcher or secondary
administrative process is required.

One optional reaper task removes expired tickets and sessions at a coarse
bounded interval. The gateway cancellation token owns it, shutdown joins it,
and request-time expiry checks remain authoritative if the reaper has not run.
No task owns an unbounded queue or detached lifetime.

Data flow is:

```text
authorized Settings action
  -> transactional four-digit ticket digest
  -> code displayed once
  -> unauthorized browser redemption
  -> atomic attempt/consume decision
  -> 32-byte opaque session token
  -> HttpOnly cookie
  -> authenticated HTTP/WebSocket application authority

or:

restart paired gateway with --pairing-window
  -> ten-minute in-memory loopback enrollment authority
  -> first explicit Approve this browser action
  -> 32-byte opaque session token and consumed window
```

## Shape-Changing Edge Cases

The common implementation must include:

- two browsers racing to commit the initial wizard;
- a request crossing the initial-window expiry boundary;
- restart before and after an initial policy is committed, including two
  browsers racing to consume an explicit recovery pairing window;
- code replacement, expiry, five failures, simultaneous correct redemption,
  and a correct code racing its final failed attempt;
- rejection of pairing-ticket creation before a paired browser exists;
- a full 32-session store;
- cookie missing, malformed, unknown, expired, revoked, and from a different
  profile/origin;
- revocation while the browser has a live WebSocket;
- clock movement across persisted wall-clock deadlines while runtime countdown
  uses a monotonic clock where available;
- SQLite busy, truncated/corrupt auth store, failed durable commit, and process
  cancellation during a request;
- hostile Host, Origin, forwarded headers, cookie sizes, labels, user-agent
  text, and request bodies; and
- Tauri/in-process startup, demo mode, Basic, bearer, and development launcher
  paths bypassing or retaining exactly their intended behavior.

Corrupt auth persistence must fail closed for semantic browser access and
provide a local recovery message. It must not reset to a fresh open window or
discard authority automatically.

## Implementation Stages And Commit Slices

1. Add the runtime-independent policy, ticket, session, persistence, and pure
   transition tests. Gate: all time, concurrency, corruption, and resource
   bounds pass without networking or a browser.
2. Add cookie/Host/Origin middleware, public auth endpoints, WebSocket session
   authority, revocation-driven closure, and scripted HTTP/WebSocket tests.
   Gate: semantic routes cannot be reached in expired or paired states without
   the required authority, while Basic/bearer/development behavior remains
   intact.
3. Add `serve` configuration and the explicit `--pairing-window` recovery
   switch with environment compatibility. Gate: recovery preserves policy and
   existing sessions, authorizes exactly one explicit loopback browser, and
   all invalid listener/auth combinations fail before binding.
4. Graduate Settings to the adaptive category modal, then add the first-run
   wizard, initial-window countdown, unauthorized/expired access screen,
   four-digit redemption, Web access approval/session management, and sign-out
   behavior. Gate: deterministic component tests cover responsive category
   navigation, retained panel drafts, every stated message, and recovery action.
5. Run headless end-to-end browser evidence across fresh setup, local-open,
   paired first browser, second browser profile, expired window, restart-switch
   recovery, revocation, restart, and existing auth modes. Update owning topics,
   readiness matrix, this execution record, and operator help with actual
   evidence.

Each stage is a reasonable commit slice once its gate passes. Internal module
extraction, generated-type updates, same-boundary bug fixes, and conservative
tightening of declared bounds are authorized.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Deterministic policy, window, ticket, session, expiry, limit, race-resolution, persistence, redaction, and corruption tests with injected clock/randomness. |
| Scripted runtime | HTTP and WebSocket tests for public/authenticated route separation, cookies, Host/Origin rejection, pairing attempts, live revocation, restart, cancellation, and SQLite contention. |
| CLI | Argument/environment precedence, bounded secret files, bind validation, busy port, pairing-window validation and expiry, redacted help/errors, and compatibility with existing deployment scripts. |
| Web components | Adaptive Settings category navigation, retained drafts, wizard choices, countdown, exact expired-window recovery, unauthorized second-browser instructions, code input, ticket expiry/failure, session list/revoke, sign out, capability gating, accessibility, and narrow viewport behavior. |
| Controlled browser | Two isolated browser contexts prove initial claim, Settings-code pairing, recovery-switch pairing after all cookies are lost, restart persistence, expiry recovery, and revocation without exposing a credential to JavaScript. |
| Regression | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and the web lint/type/test/build baseline pass. |
| Platform | Tauri builds and remains in-process without displaying headless gateway auth. No visible desktop, Android, physical device, public network, or external host run is required. |

Browser evidence must inspect cookies through automation/browser facilities
without printing token values into retained logs. Temporary profiles, cookies,
databases, screenshots, and runtime logs are removed after validation unless a
redacted artifact is explicitly recorded.

## Non-Goals

- Password login, password recovery, OAuth, passkeys, multi-user accounts, or
  per-torrent roles.
- SRP, OPAQUE, relay authentication, E2E record encryption, host identity,
  remembered cryptographic devices, remote resume, or friend sharing.
- Cookie-authenticated LAN HTTP, automatic TLS, certificate provisioning,
  reverse-proxy discovery, NAT traversal, or changing the accepted private
  Basic-host deployment.
- Treating a four-digit code as strong authentication or using it outside the
  bounded local bootstrap flow.
- Runtime listener changes from the Settings UI.
- Requiring headless web authentication inside Tauri, Android, demo mode, or
  engine-only commands.
- Redesigning the semantic application protocol, view-set authority, torrent
  persistence, or HTTP file-serving capability system.

## Escalation Contract

Implementation may proceed autonomously through the five stages, including
ordinary refactoring, an argument-parser choice after license review, the
gateway-owned SQLite schema, generated types, UI wording refinements that
preserve the exact recovery actions, and tighter resource limits supported by
tests.

Stop for direction if evidence requires non-loopback cookie exposure, TLS or a
new reverse-proxy trust model, a password or cryptographic identity scheme, a
new external dependency with material security/maintenance tradeoffs, a
repository-wide CLI migration, destructive profile recovery, or behavior that
silently reopens a configured profile.

## Stopping Condition And Next Boundary

This tactical is complete when a fresh headless profile provides the
ten-minute communicated loopback setup window; the user can choose local-open
or paired-browser policy; another browser profile can recover through an
authorized Settings code or an explicit restart pairing window after all
authorized cookies are gone; expired-window, full-session, invalid-code,
restart, revocation, Host, Origin, and persistence behavior pass the validation
matrix; Settings provides the adaptive capability-gated Web access management
panel; and existing Basic, bearer, development, Tauri, and application-protocol
paths retain their stated contracts.

The next slice may add password login and TLS-backed non-loopback cookie
access, or begin the separately gated remote PAKE feasibility work. Neither is
implied by completing local browser pairing.

## Execution Record

Completed on 2026-08-07 in these implementation slices:

- `eadffb6` added the gateway-owned SQLite policy, pairing-ticket, and bounded
  opaque-session store with expiry, coalesced use updates, persistence,
  attempt, revocation, and 32-session limit tests.
- `885b44e` added cookie/Host/Origin admission, public authentication routes,
  semantic HTTP and WebSocket enforcement, prompt live-session revocation,
  restart recovery, hosted assets, and the product `serve` CLI while
  preserving Basic, bearer, and explicit development modes.
- `dc24948` added the first-run and recovery gates, four-digit redemption,
  capability-gated Web access management, and the adaptive category-based
  Settings workspace with retained panel drafts and keyboard navigation.
- `eb6f731` fixed the real-browser Fetch binding found by controlled evidence,
  made the gateway print/open the explicit same-origin live application URL,
  and added reusable production-hosted Playwright lifecycle coverage.
- `2be95b0` added local-open/restart browser evidence and made a revoked live
  application socket emit typed `authentication_failed` before closure.
- `19627ab` added multi-connection initial-policy and correct-redemption race,
  active-ticket replacement, and corrupt persisted-policy fail-closed tests.

The runtime-independent store tests cover atomic initial choice, four-digit
single-use tickets, five-attempt exhaustion, ticket/session expiry, rolling
touch, store reopen, revocation, label bounds, and the full 32-session ceiling.
The gateway integration test covers public/semantic route separation,
HttpOnly cookie admission, paired second-browser redemption, session listing,
Origin enforcement inherited by every state-changing route, live WebSocket
revocation, restart persistence, explicit recovery, and one-shot recovery
consumption. Existing gateway tests retain Basic whole-site authentication,
bearer automation, hostile Origin rejection, and explicit ephemeral
development behavior.

Controlled headless Chrome served the actual production bundle from the Rust
gateway. Isolated contexts proved paired first-run setup, cookie attributes,
Settings-code handoff, second-browser application entry, two-session listing,
revocation and rejection after reload. A same-profile restart with
`--pairing-window` proved cookie-loss recovery and rejection of a later clean
context. A separate fresh profile proved local-open entry from two cookieless
contexts and persistence across restart. The expired-window copy and recovery
instruction are deterministic component evidence rather than a retained
ten-minute wall-clock browser run; request-time monotonic expiry remains the
authoritative runtime boundary.

Final validation passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings`;
- `cargo test --workspace`;
- `npm run typecheck` in `clients/web`;
- `npm test` in `clients/web` (`206` passed, `2` skipped); and
- `npm run build` in `clients/web`, including the CSP bundle check.

No visible desktop client, Android target, physical device, external host, or
public network was used. Temporary browser profiles, SQLite profiles, and
Playwright runtime artifacts were not retained.
