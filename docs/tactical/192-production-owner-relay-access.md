# Tactical 192: Production Owner Relay Access

Status: **Ready, not active.** Completed controlled foundation Tactical
[`190`](190-opaque-wasm-relay-foundation.md) supplies the selected cryptographic
construction, Wasm boundary, dumb-relay behavior, real application trace and
measured proof limits. Starting this tactical requires explicit user direction.
Any public relay deployment, DNS/TLS mutation, release publication or external
account is a separately authorized operation within that execution, not implied
by this document.

Topics:
[`remote-access-authentication`](../topics/remote-access-authentication.md),
[`application-connection-architecture`](../topics/application-connection-architecture.md),
[`runtime-configurations-and-headless-deployment`](../topics/runtime-configurations-and-headless-deployment.md),
[`client-surfaces`](../topics/client-surfaces.md), and
[`capability-readiness`](../topics/capability-readiness.md).

## Motivation And Stopping Condition

The proof establishes that an account-free username/passphrase path is viable,
but it intentionally loses all authority on exit, binds only loopback, serves no
supported remote page and has no relay operations or recovery UX. The next
slice turns that evidence into one deliberately narrow supported capability:

> One owner enables remote access locally on a supported desktop or configured
> Linux headless host, then a freshly loaded supported browser uses the owner's
> relay-scoped username and passphrase to establish the existing bounded
> application connection through the operated relay with end-to-end encryption
> and blocking host pinning.

This tactical stops only when enable, login, ordinary reconnect, passphrase
change, disable, local recovery, host-identity warning, relay outage and release
rollback pass on the declared hosts and representative external browsers. The
proof harness is removed from product selection and remains an internal gate.

## Accepted Product Boundary

- The user model remains **username plus passphrase**. There is no Google/OIDC
  login, RSTorrent account, email recovery, account delegation, encrypted cloud
  sync, friend sharing or multi-user role.
- A complete password login is required for every connection. Remembered
  devices, passkeys, resume credentials and QR enrollment remain later work.
- Initial supported hosts are the ordinary desktop application and configured
  Linux headless service. Android, iOS, an extension-owned backend and a generic
  remote daemon are not host surfaces in this slice.
- The supported controller is the independently served remote React/browser
  client. Native Android/iOS remote-controller integration remains absent.
- Remote `.torrent` byte upload, media capability creation, payload streaming,
  filesystem selection and arbitrary HTTP proxying remain disabled. Magnet
  intake, ordinary commands and bounded application views use the encrypted
  application WebSocket.
- The portable-profile host-key tier is the honest initial claim. A protected
  local authority file improves ordinary at-rest handling but is exportable and
  clonable; no hardware-backed, non-exportable or attested identity is claimed.
- A public relay is an untrusted rendezvous and opaque byte forwarder. It never
  terminates application encryption or becomes an application principal.

## Durable Authority And State Transitions

One versioned application-private remote-authority record owns:

- random host ID;
- serialized OPAQUE server authority;
- OPAQUE password file;
- relay deployment ID and selected route;
- independently random relay registration credential;
- protocol/suite floor; and
- enabled/disabled generation plus last bounded operational status.

The record never stores the passphrase. It is written with owner-only platform
permissions using the repository's existing protected-secret and atomic-replace
patterns, remains outside SQLite diagnostics/support export, and is loaded only
by the application owner. Desktop and headless paths must prove their exact
permission and backup/restore behavior; a portable profile containing the full
record is explicitly a clonable authority.

The only state transitions in this slice are:

1. **Enable:** local authenticated UI validates the route and confirmed
   passphrase, creates a complete candidate generation, registers it, and
   atomically makes it current only after the relay and password record are
   complete.
2. **Passphrase change:** a locally authenticated owner performs a fresh OPAQUE
   registration under the existing host identity, atomically replaces the
   password file, and terminates every old circuit. There is no remote-only
   forgotten-password change.
3. **Relay credential rotation:** register a fresh credential before retiring
   the prior generation; late old sockets are generation-fenced.
4. **Disable:** stop and join the host owner, release the route, terminate live
   circuits, durably remove all remote authority, and report whether protected
   file removal completed. Torrent/profile data remains untouched.
5. **Local recovery/reset:** a locally authenticated owner who lost the
   passphrase disables and reprovisions. Existing clients see a blocking host
   identity change if reset creates a new host authority; they never silently
   repin.

Crash/restart at every transition must resolve to the complete prior or complete
new generation. A partial record cannot enable a route.

## Client Delivery And Trust

The remote client is built from the same reviewed React source and generated
application contract, with an explicit remote capability profile selecting the
Wasm transport and hiding unsupported bulk/media/filesystem actions. It is
served from one dedicated HTTPS origin independent of a user's host and uses a
release-pinned CSP, immutable hashed assets, no third-party script, no analytics
and no service-worker persistence in the first slice.

This protects ordinary delivery mistakes but does not turn Wasm, CSP or asset
hashing into a boundary from the page. Product copy must say that a compromised
client origin can observe the entered passphrase and decrypted application
state. Release evidence records the exact client build ID alongside protocol
version without sending either value inside OPAQUE secrets.

The browser retains a host pin only after authenticated readiness. Pin storage
is scoped by relay deployment ID plus username. Clearing browser storage
returns to password-authenticated first use; an existing mismatched pin remains
a blocking identity warning requiring an explicit local recovery explanation,
not another password prompt.

## Relay Operation And Bounds

The production relay preserves the proof's dependency direction: routing may
depend on transport/storage/operations, but never on OPAQUE, record keys or
application DTOs. It adds only the operational breadth needed for one service:

- WSS behind one exact TLS authority and stable random relay deployment ID;
- durable bounded username reservation with a non-reversible verifier for the
  relay registration credential rather than its plaintext;
- challenge-bound route reclaims so a captured old claim cannot evict a live
  waiting host;
- one waiting host and one active circuit per route, 1,024 proof-scale routes
  per process shard initially, and exact aggregate admission before upgrade;
- the proof message/queue/deadline/lifetime bounds or conservative tightening;
- per-route, per-source and aggregate token buckets for claims, pairing and
  unauthenticated attempts, with bounded expiry and no attacker-created task;
- generic client-facing unknown/offline/busy/authentication failure; and
- metadata-only metrics, retention limits and incident diagnostics with no
  opaque payload capture by default.

Namespace reservation, offensive-name policy, expiry, abuse response, capacity
planning, backup, key rotation, deletion and incident rollback must be written
before public exposure. Billing, regions, multi-relay discovery and a permanent
wire-compatibility promise remain absent.

## Owner, Task And Cancellation Map

| Owner | State/work | Termination |
| --- | --- | --- |
| Application remote owner | durable generation, local commands, host loop and status | disable, application shutdown or fatal authority error cancels and joins all children |
| Host route generation | one challenged relay claim and one waiting socket | replacement, timeout, relay failure or owner shutdown releases only its generation |
| Authentication attempt | bounded OPAQUE state and deadline permit | success, generic failure, disconnect or 20-second deadline wipes state and releases permit |
| Secure circuit | record state and one existing application connection | authenticated close, 24 hours, sequence exhaustion, application/relay close or owner shutdown |
| Browser transport | passphrase input, Wasm states, host pin and application adapter | failure/page close wipes best-effort state and requires a new password login |
| Relay route/pair | durable reservation metadata and bounded opaque pumps | generation replacement, timeout, either close or relay shutdown cancels and joins both pumps |

The application protocol still receives an authenticated owner context and
plain bounded frame channel. It does not learn a relay credential or accept a
principal asserted inside a client frame.

## Validation Sequence

1. Re-audit the exact Tactical `190` dependency graph, current RFC errata and
   advisories; pin any justified update before changing persisted formats.
2. Add the versioned durable-authority state machine and crash matrix without a
   public listener.
3. Add local desktop/headless enable, change, disable and recovery commands plus
   truthful status UI. Prove no secret reaches generated DTOs or diagnostics.
4. Turn the proof host adapter into a product-owned task beneath both declared
   host lifecycles; retain exact cancellation, deadlines and bulk rejection.
5. Build the remote-only React capability profile and independently delivered
   immutable client bundle. Prove first-use/repeated/mismatched pin behavior in
   real browsers.
6. Replace proof claim registration with challenge-bound durable relay routing,
   rate limits and operational cleanup. Run locally and in an isolated staging
   environment before any public mutation.
7. Run the complete direct-versus-relayed trace, active relay, clone, crash,
   flood, restart, outage and rollback matrices.
8. With separate deployment authorization, perform bounded external desktop
   and headless campaigns from at least two network paths and supported desktop
   plus phone-sized browsers. Remove or roll back all staging resources unless
   publication was explicitly requested.

## Required Evidence

- Pure core, Wasm/browser and dependency gates from Tactical `190` remain green.
- Authority enable/change/disable/reset passes atomic crash injection at every
  write and task transition, with exact secret-file permissions and cleanup.
- Fresh, repeated and changed-pin browsers produce the documented results;
  wrong/unknown/offline/busy outcomes remain generic and bounded.
- Direct and relayed negotiation, view snapshot/update/ack and benign command
  reduce identically on production adapters.
- Active modification, replay, reordering, reflection, route/relay substitution
  and record attacks fail closed without partially admitted application state.
- Password-file-only, relay-credential-only, portable-profile and complete
  live-process clone scenarios retain the documented distinctions.
- Slow/flood/name-churn pressure records CPU, resident memory, allocations,
  queues, task counts and rate-limit high waters; shutdown reaches zero owners.
- Desktop and configured headless restart/update/rollback preserve torrents and
  either preserve one valid remote generation or remain disabled.
- Remote client production build, CSP inspection, accessibility, desktop/mobile
  browser matrix and release provenance pass.
- Public/staging relay evidence records TLS, capacity, backup/restore, abuse,
  metrics retention and exact rollback without logging a protocol payload.

The proportional repository baseline remains the Tactical `190` baseline plus
desktop/headless package tests affected by the implementation and the approved
external staging runner.

## Non-Goals

- Google/OIDC, cloud accounts, delegation, sync, email recovery or social
  identity.
- Remembered devices, passkeys, QR enrollment, session resumption or roles.
- Hardware-backed host identity, attestation or non-exportable key migration.
- Remote media/file serving, torrent byte upload, arbitrary HTTP/filesystem
  proxying or payload relay.
- Multiple concurrent browsers, relay multiplexing, regions, direct NAT
  traversal, WebRTC/TURN, wake-up delivery or extension control.
- Android/iOS host mode or native remote-controller UI.
- Stable third-party API or permanent public wire compatibility.

## Escalation Contract

Routine refactoring, fixtures, local listeners, isolated staging construction
and conservative tightening inside these decisions are implementation work once
the tactical is activated. Stop for direction before changing the selected
OPAQUE construction, weakening password/pin behavior, adding an authority or
recovery provider, broadening remote payload/media access, claiming a stronger
key tier, publishing client/relay artifacts, mutating public DNS/TLS, or
retaining a staging/public service beyond its authorized campaign.
