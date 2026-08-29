# Tactical 192: Local Production-Shaped Owner Relay Access

Status: **Complete as of 2026-08-29.** Completed controlled foundation Tactical
[`190`](190-opaque-wasm-relay-foundation.md) supplies the selected cryptographic
construction, Wasm boundary, dumb-relay behavior, real application trace and
measured proof limits. The user explicitly activated end-to-end implementation
on 2026-08-29 and requested incremental commits. The resulting composition is
implemented and validated only as the internal local mode defined here; it is
not deployed, published or described as supported remote access.
This tactical is strictly local-only: it may productionize relay/client/host
code and exercise distinct loopback HTTPS/WSS origins, but it must not create or
use an external account or host, mutate public DNS/TLS, publish a release or
retain any nonlocal service. A later separately authorized tactical owns every
deployment and supported-public-capability claim.

Topics:
[`remote-access-authentication`](../topics/remote-access-authentication.md),
[`application-connection-architecture`](../topics/application-connection-architecture.md),
[`runtime-configurations-and-headless-deployment`](../topics/runtime-configurations-and-headless-deployment.md),
[`application-control`](../topics/application-control.md),
[`application-view-api`](../topics/application-view-api.md),
[`web-ui-design`](../topics/web-ui-design.md),
[`client-surfaces`](../topics/client-surfaces.md), and
[`capability-readiness`](../topics/capability-readiness.md).

## Motivation And Stopping Condition

The proof establishes that an account-free username/passphrase path is viable,
but it intentionally loses all authority on exit, binds only loopback, serves no
supported remote page and has no relay operations or recovery UX. The next
slice turns that evidence into one deliberately narrow, production-shaped
local product composition:

> One owner enables the validation mode locally on a declared desktop or
> configured Linux headless host, then a freshly loaded declared browser uses
> the owner's relay-scoped username and passphrase to establish the existing
> bounded application connection through the separate local relay with
> end-to-end encryption and blocking host pinning. A private browser may then
> retain one revocable authorization and resume ordinary reconnects without
> another password entry, while the owner can inspect and terminate every
> authorization and live circuit from the RSTorrent host.

This tactical stops only when enable, first login, private-versus-shared-browser
choice, automatic resume, expiry, individual and global revocation, complete
authorization inspection, passphrase change, disable, local recovery,
host-identity warning, relay outage and package rollback pass through the
declared desktop/headless lifecycles and isolated local real-browser profiles.
The proof harness is removed; the production-shaped composition remains an
explicit internal validation mode and does not become a supported remote
product surface.

## Accepted Product Boundary

- The user model remains **username plus passphrase**. There is no Google/OIDC
  login, RSTorrent account, email recovery, account delegation, encrypted cloud
  sync, friend sharing or multi-user role.
- A complete password login remains the universal new-browser, expired-session
  and recovery path. After one successful login, a private browser may create a
  distinct named authorization and use fresh challenge-bound resume for routine
  socket loss, reload, browser restart and relay-route reattachment until that
  authorization expires or is revoked. A shared-browser choice retains only
  page-lifetime resume state and requires the password after the page closes.
- Authorized browsers are distinct revocable client identities, but every one
  has the same single-owner authority in this slice. Passkeys, QR enrollment,
  delegated roles and authorization without an initial password remain later
  work.
- Initial validation hosts are the ordinary desktop application and configured
  Linux headless service. Android, iOS, an extension-owned backend and a generic
  remote daemon are not host surfaces in this slice.
- The validated controller is the independently served release-built
  React/browser client at a loopback HTTPS origin. Native Android/iOS
  remote-controller integration remains absent.
- Remote `.torrent` byte upload, media capability creation, payload streaming,
  filesystem selection and arbitrary HTTP proxying remain disabled. Magnet
  intake, ordinary commands and bounded application views use the encrypted
  application WebSocket.
- The portable-profile host-key tier is the honest initial claim. A protected
  local authority file improves ordinary at-rest handling but is exportable and
  clonable; no hardware-backed, non-exportable or attested identity is claimed.
- The eventual public relay is modeled as an untrusted rendezvous and opaque
  byte forwarder. Tactical `192` implements that exact dependency boundary in a
  separate loopback-only service process; it never terminates application
  encryption or becomes an application principal.
- The application owner exposes one security surface that lists every current
  authorized browser and live circuit, supports exact revocation, and retains a
  bounded security-event history. A transport identifier, browser label or
  application-frame field never creates authority by itself.

## Durable Authority And State Transitions

One versioned application-private remote-authority record owns:

- random host ID;
- serialized OPAQUE server authority;
- OPAQUE password file;
- relay deployment ID and selected route;
- independently random relay registration credential;
- protocol/suite floor; and
- enabled/disabled and authorization generations plus last bounded operational
  status.

The record never stores the passphrase. It is written with owner-only platform
permissions using the repository's existing protected-secret and atomic-replace
patterns, remains outside SQLite diagnostics/support export, and is loaded only
by the application owner. Desktop and headless paths must prove their exact
permission and backup/restore behavior; a portable profile containing the full
record is explicitly a clonable authority.

The remote-authority domain also owns at most 32 current authorized-client
records. A record contains a random client ID, bounded user-visible label, the
selected resume verifier or public key, creation and last-full-login times,
last-resume and last-seen times, idle and absolute expiry, authorization
generation, client-build and route observations, a non-authorizing public-key
fingerprint when the selected construction supplies one, active-circuit count,
and current state. Expired or revoked clients lose their proof material and
become at most 128 non-authorizing tombstones retained for 180 days. User agent,
label and browser description remain bounded client-reported metadata rather
than proof of identity. No private client key, raw public key or reusable client
credential enters a generated DTO, log, metric, diagnostic capture or support
export.

The state transitions in this slice are:

1. **Enable:** local authenticated UI validates the route and confirmed
   passphrase, creates a complete candidate generation, registers it, and
   atomically makes it current only after the relay and password record are
   complete.
2. **Authorize browser:** after a complete password login and an explicit
   private-browser choice, the browser proves possession of fresh client
   credential material inside the authenticated circuit. The host commits one
   named authorization before returning its resume handle. Shared-browser login
   creates no durable authorization.
3. **Resume:** client and host prove fresh possession through a reviewed,
   challenge-bound exchange that binds the host identity, client ID, relay
   deployment and username, authorization generation, protocol/suite floor and
   fresh client/server nonces. Only successful mutual proof creates new record
   keys and touches last-used state; an old traffic key is never persisted as a
   transferable resume credential.
4. **Revoke, sign out or expire:** invalidate the exact authorization before
   closing all of its circuits, fence late proofs by generation, and retain a
   non-authorizing audit tombstone. **Revoke all other browsers** and **Require
   password on every browser** are bounded compositions over the same
   transition, not distinct authority paths.
5. **Passphrase change:** a locally authenticated owner performs a fresh OPAQUE
   registration under the existing host identity, atomically replaces the
   password file, revokes every resume authorization and terminates every old
   circuit. There is no remote-only forgotten-password change.
6. **Relay credential rotation:** register a fresh credential before retiring
   the prior generation; late old sockets are generation-fenced.
7. **Disable:** stop and join the host owner, release the route, revoke every
   client, terminate live circuits, durably remove all authority-bearing
   material, and report whether protected file removal completed.
   Non-authorizing security history remains under its declared retention until
   the owner explicitly clears it. Torrent/profile data remains untouched.
8. **Local recovery/reset:** a locally authenticated owner who lost the
   passphrase disables and reprovisions. Existing clients see a blocking host
   identity change if reset creates a new host authority; they never silently
   repin.

Crash/restart at every transition must resolve to the complete prior or complete
new generation. A partial record cannot enable a route, authorize a browser or
resume a circuit.

## Resume And Security Audit

Resume is required product behavior rather than a later convenience. A
supported private browser generates its client credential through a
non-exportable WebCrypto key where the selected construction and browser permit
that claim, stores only the minimum credential and public identifiers in the
dedicated remote-client origin, and automatically attempts resume before
showing the password form. A valid authorization uses a seven-day sliding idle
deadline and a 30-day absolute lifetime from the last complete password login;
successful activity advances the idle time with at most one durable touch per
hour. Expiry, explicit sign-out, individual revocation, passphrase change,
disable, reset or authorization-generation replacement makes the next
connection require the password. Resume rejection never silently creates a
new authorization or weakens a host-identity warning.

Before persisted formats freeze, implementation must re-audit the current
YepAnywhere challenge-bound resume, Android paired-server and security-client
registry as product and failure references, then select and record the exact
reviewed RSTorrent resume construction. The construction must provide fresh
mutual proof, fresh record keys, replay and reflection rejection, client and
host binding, key separation, bounded server state and prompt revocation. A raw
OPAQUE traffic key, an unbound bearer token or a client-asserted device ID is
not an acceptable shortcut.

The owner-facing **Remote access security** surface is available through local
desktop/headless administration and the authenticated remote capability
profile. It shows, without a default filter that can hide authority:

- every current authorization with label, **This browser** marker, created,
  last password login, last resume, last seen, idle/absolute expiry, current
  versus expired/revoked state, active circuit count, authentication method,
  client build, bounded route/browser observations and the public-key
  fingerprint when applicable;
- every live circuit with connection generation, start/last-activity time,
  route, its client authorization or shared-browser ephemeral marker, and
  eventual close reason; and
- individual rename/revoke, **Revoke all other browsers**, **Require password
  on every browser**, **Sign out this browser**, and explicit history-clear
  actions with proportionate confirmation.

One local security ledger retains at most 1,024 authenticated state-changing
events for 180 days: enable/disable/reset, password changes, authorization
creation/rename/revocation/expiry, successful full login/resume, circuit
open/close, host-identity recovery and relay-credential generation changes.
Failed authentication and rate-limit pressure use at most 256 aggregate time
buckets retained for 30 days, so an attacker cannot create one durable row per
attempt or evict owner actions. Events have stable random IDs, timestamps,
applicable client/circuit IDs, authentication method, result, route/deployment,
client-build observation and bounded reason/close class. They contain no
passphrase, OPAQUE record, raw private/public credential bytes, resume secret,
traffic key, protocol payload, torrent data or unbounded attacker text and are
excluded from ordinary diagnostics/support export. Revoked records and ledger
entries are audit evidence only and can never authorize a connection.

### Selected resume construction (2026-08-29)

The pre-persistence gate is closed. The current YepAnywhere checkout is
`b8b6987b1466a35ff818483002eea31472bed8c9`; its only local modification is an
unrelated `README.md`, and every audited resume/security-client path is
unchanged from the recorded clean `506ce0528ffe3ef44c5e4ee90780b44eb80d4a15`
checkpoint. RSTorrent adopts its separation of authenticated session, client
continuity, live connection and audit record. It does not adopt the persisted
symmetric SRP base key, session-file shape or wire messages.

The exact RSTorrent construction is:

- each private browser creates one origin-scoped, non-extractable WebCrypto
  P-256 ECDSA/SHA-256 continuity key and stores its handle in IndexedDB; the
  host stores only its 65-byte uncompressed SEC1 public key and SHA-256
  fingerprint;
- enabling remote access creates a separate random host P-256 ECDSA/SHA-256
  resume-signing key. Its 32-byte secret is authority-bearing protected state;
  its 65-byte public key is delivered only inside a successful OPAQUE record
  channel and retained beside the authenticated 64-byte OPAQUE host pin;
- private-browser authorization uses a fresh 32-byte host challenge inside the
  full-login record channel. The client signs a fixed length-prefixed transcript
  binding the protocol/operation domain, complete canonical OPAQUE binding,
  host pin, host resume public key, authorization generation, challenge,
  client public key and proof-excluded bounded authorization metadata;
- resume creates independent ephemeral P-256 ECDH keys and independent
  32-byte nonces on both endpoints. Distinct host and client ECDSA transcripts
  bind both nonces and ephemeral public keys, the complete canonical OPAQUE
  binding and host pin, host resume public key, client ID, global and client
  authorization generations, and protocol/suite floor;
- the host signs first. The browser verifies that signature with the public key
  retained through full OPAQUE login before releasing its client signature.
  The host rechecks current, unexpired, generation-matching authorization
  immediately before accepting the client proof;
- both sides derive a 64-byte intermediate secret with HKDF-SHA-512 from the
  32-byte ephemeral P-256 ECDH result, the SHA-512 resume-transcript digest as
  salt and `rstorrent.remote.resume.session.v1` as the expansion label. That
  intermediate enters the existing binding-scoped directional record
  derivation, then is erased; and
- ECDSA signatures use the WebCrypto-compatible fixed 64-byte `r || s` form.
  Every P-256 public value uses the exact 65-byte uncompressed SEC1 form and is
  fully validated before state changes or ECDH.

Fresh ephemeral ECDH supplies new record keys and forward secrecy for each
resume connection. Separate role domains, both fresh nonces, both ephemeral
keys and generation binding reject replay, reflection, cross-client,
cross-route and stale-authorization use. A copied browser profile without the
non-extractable key cannot resume; a compromised origin may still invoke that
key while it controls the browser and remains inside the documented hosted-
client threat boundary.

The native implementation uses exact `p256` `0.13.2` with only `ecdsa` and
`ecdh`, reusing the already-resolved RustCrypto `elliptic-curve` `0.13` graph
rather than adding the new parallel `0.14` graph. The inspected source and
tests are recorded in `docs/references.md`. RFC `9807` currently has no
verified erratum: its sole erratum `8675` is rejected and changes none of the
selected OPAQUE behavior.

## Client Delivery And Trust

The remote client is built from the same reviewed React source and generated
application contract, with an explicit remote capability profile selecting the
Wasm transport and hiding unsupported bulk/media/filesystem actions. Tactical
`192` serves the release-built bundle from one dedicated loopback HTTPS origin
independent of the locally tested host and relay origins. It uses a
release-pinned CSP, immutable hashed assets, no third-party script, no analytics
and no service-worker persistence in the first slice. Publication and a public
client origin remain later work.

This protects ordinary delivery mistakes but does not turn Wasm, CSP or asset
hashing into a boundary from the page. Product copy must say that a compromised
client origin can observe the entered passphrase and decrypted application
state. Local artifact evidence records the exact client build ID alongside
protocol version without sending either value inside OPAQUE secrets.

The browser retains a host pin only after authenticated readiness. Pin storage
is scoped by relay deployment ID plus username. Clearing browser storage
removes both that browser's local resume ability and its local trust record but
does not erase the server-side authorization or audit entry; the owner can
identify and revoke the abandoned authorization. A surviving mismatched pin
remains a blocking identity warning requiring an explicit local recovery
explanation, not a resume fallback or another password prompt.

## Local Production-Shaped Relay And Bounds

The production-shaped relay preserves the proof's dependency direction:
routing may depend on transport/storage/operations, but never on OPAQUE, record
keys or application DTOs. Tactical `192` turns the proof library into a
separately supervised service binary whose listeners fail unless every resolved
address is loopback. It adds the operational breadth needed to validate one
future service:

- WSS behind one exact local TLS authority and a durable random relay
  deployment ID inside the isolated evidence root;
- durable bounded username reservation with a non-reversible verifier for the
  relay registration credential rather than its plaintext;
- challenge-bound route reclaims so a captured old claim cannot evict a live
  waiting host;
- one waiting host and one active circuit per route, 1,024 proof-scale routes
  per process shard initially, and exact aggregate admission before upgrade;
- the proof message/queue/deadline/lifetime bounds or conservative tightening;
- per-route, per-source and aggregate token buckets for claims, pairing and
  password/resume attempts, with bounded expiry and no attacker-created task;
- generic client-facing unknown/offline/busy/authentication failure; and
- metadata-only metrics, retention limits and incident diagnostics with no
  opaque payload capture by default.

Namespace reservation, offensive-name policy, expiry, abuse response, capacity
planning, backup, key rotation, deletion and incident rollback must be drafted
and exercised locally where deterministic, but Tactical `192` cannot validate
public certificate operation, Internet abuse sources, external availability or
real incident response. Those are gates for the later deployment tactical.
Billing, regions, multi-relay discovery and a permanent wire-compatibility
promise remain absent.

## Owner, Task And Cancellation Map

| Owner | State/work | Termination |
| --- | --- | --- |
| Application remote owner | durable generation, local commands, host loop and status | disable, application shutdown or fatal authority error cancels and joins all children |
| Host route generation | one challenged relay claim and one waiting socket | replacement, timeout, relay failure or owner shutdown releases only its generation |
| Authorized client registry | at most 32 current named client proofs, 128 non-authorizing tombstones, resume deadlines, revocation and security ledger | disable/reset removes authority; expiry/revocation fences proofs and closes owned circuits; shutdown flushes bounded state |
| Authentication attempt | bounded OPAQUE or resume state and deadline permit | success, generic failure, disconnect or 20-second deadline wipes state and releases permit |
| Secure circuit | record state and one existing application connection | authenticated close, 24 hours, sequence exhaustion, application/relay close or owner shutdown |
| Browser transport | passphrase input when needed, Wasm states, host pin, client credential, resume attempt and application adapter | failure/page close wipes ephemeral state; a private authorization may resume through a fresh proof while shared-browser state cannot |
| Relay route/pair | durable reservation metadata and bounded opaque pumps | generation replacement, timeout, either close or relay shutdown cancels and joins both pumps |

The application protocol still receives an authenticated owner context and
plain bounded frame channel. It does not learn a relay credential or accept a
principal asserted inside a client frame.

## Validation Sequence

1. Re-audit the exact Tactical `190` dependency graph, current RFC errata and
   advisories plus the pinned YepAnywhere resume/security-client paths; select
   and record the exact resume/client-proof construction before changing
   persisted formats.
2. Add the versioned durable-authority, authorized-client and security-ledger
   state machines and their crash matrix without a public listener.
3. Add local desktop/headless enable, change, disable, recovery, authorization
   inspection/revocation and audit commands plus truthful status UI. Prove no
   secret reaches generated DTOs, the ledger or diagnostics.
4. Turn the proof host adapter into a product-owned task beneath both declared
   host lifecycles; retain exact cancellation, deadlines and bulk rejection.
5. Build the remote-only React capability profile and independently delivered
   immutable client bundle. Prove first-use/repeated/mismatched pin,
   private/shared choice, reload/restart/relay-route-reattachment resume,
   expiry, sign-out, revocation and audit behavior in real browsers.
6. Replace proof claim registration with challenge-bound durable relay routing,
   rate limits and operational cleanup. Run it only as a loopback-bound separate
   process with an isolated durable root and local TLS authority.
7. Run the complete direct-versus-relayed trace, active relay, password/resume
   clone, crash, replay, revocation race, audit-retention, flood, restart,
   outage and rollback matrices.
8. Run packaged desktop and configured-headless hosts against the separate
   local relay/client origins through isolated real-browser profiles, including
   desktop and phone-sized viewports, injected path changes and an exact
   upgrade/rollback cycle. Remove every certificate, authority root, profile,
   reservation, log and process created by the campaign.

### Implementation checkpoints

- Steps 1 and 2 are complete. Commit `77c6cbb` adds the strict, runtime-free
  P-256 resume messages and fresh mutually authenticated record derivation.
  Native tests cover replay, reflection, host/client/route-generation
  substitution, malformed points and strict fixed encodings.
- `rstorrent-remote-access` now owns the versioned authority candidate,
  authorized-client registry, expiry/revocation transitions, non-authorizing
  tombstones, authenticated security ledger, aggregate failed-attempt buckets
  and protected persistence without depending on a socket, async runtime,
  application service or generated DTO.
- The current persistence gate accepts only Unix owners, creates an exact
  `0700` authority directory and `0600` atomic-replace files, and rejects
  symlinks, wrong owners, weakened modes, oversized records and malformed or
  duplicate fields. This covers the declared macOS validation desktop and
  configured Linux headless host. Other desktop platforms fail closed until a
  platform-native owner-only store and its permission evidence land.
- Deterministic tests exercise prior-versus-new outcomes before and after
  replacement, failed mutation rollback, disable before/after authority-file
  removal, retained history and explicit history clearing. They also reach the
  32 current clients, 128 tombstones, 1,024 owner events and 256 failed-bucket
  ceilings, prune both retention periods, prove password/global generation
  fencing and complete a fresh resume through application record encryption.
  `cargo test -p rstorrent-remote-access` and strict all-target clippy pass.
- The first Step 6 boundary is also complete in the relay library. Its durable
  store retains only a deployment ID, sorted usernames and P-256 public keys;
  a fresh relay challenge plus signature authorizes every host claim and route
  release. One waiting generation and one active circuit remain exact per
  route, all forwarding stays opaque, public failures are generic, and bounded
  aggregate/source/route buckets create no attacker-owned tasks. Restart,
  idempotent reservation, conflicting-key, wrong-key, replay, exact-Origin,
  release, owner-mode, corruption and end-to-end forwarding tests pass. The
  separately supervised TLS service now also exists: it accepts only an
  explicit loopback bind, an absolute DER certificate path and an absolute
  owner-only `0600` PKCS#8 DER key path, negotiates TLS 1.3, bounds concurrent
  handshakes at 64 with a ten-second deadline, emits one metadata-only JSON
  readiness record and drains on interrupt/termination. A stalled handshake
  does not block valid clients and shutdown aborts pending handshakes. The
  isolated local-authority/client runner now creates a temporary server-auth
  certificate, serves the release bundle from a distinct HTTPS origin and
  removes the authority on exit; the service never creates or installs a trust
  root itself.
- The first Step 4 runtime boundary now lives in `rstorrent-remote-host` rather
  than the proof harness. Its product owner reserves and claims the durable
  P-256 relay identity, emits a relay/host greeting needed for account-free
  OPAQUE binding, completes password login plus explicit private/shared choice,
  commits private authorization before acknowledgement, completes fresh
  challenge-bound resume, records circuit open/close, lists live circuits and
  invalidates an authorization durably before cancellation. It injects a
  process-private credential only into the reused internal application adapter
  and rejects remote torrent-byte upload and media-capability creation. Native
  end-to-end tests carry the real application `Connect`/`Connected` exchange
  through password and resumed circuits, prove shared mode retains no
  authorization, prove revocation closes the resumed circuit and prove the
  serialized security view excludes passphrase, internal gateway token and raw
  client public key.
- The product owner now implements the complete local administration core:
  safe full-ledger/live-circuit inspection, rename, exact revocation, revoke
  all except a selected current browser, explicit circuit closure, require-
  password-everywhere, passphrase replacement, automatic expiry, disable with
  signed route release and non-authorizing retained history, history clearing,
  and local disable/reprovision recovery. Durable invalidation always precedes
  circuit cancellation. End-to-end tests prove the actual route appears in
  live-circuit inspection, explicit shared-circuit termination, protected
  authority removal, relay reservation release, retained disable evidence and
  successful reprovisioning.
- Desktop and configured headless now own the same product composition beneath
  their incumbent `ApplicationService`: an ephemeral process-private bearer
  gateway, one durable remote owner and joined remote-before-application
  shutdown. Desktop admits the composition only when both explicit validation
  relay environment values are present and exposes local Tauri administration
  commands. Headless configuration version 3 adds one exact loopback HTTPS
  relay/certificate block without changing its separately selected hosted
  access mode; versions 1 and 2 remain accepted and cannot opt in accidentally.
  Its CLI reaches the running owner only through an owner-mode Unix socket with
  same-UID checking, bounded requests/responses and deadlines, and accepts
  passphrases only from protected absolute files. A real TLS-relay test enables
  through that socket, joins shutdown, reopens the configured headless service,
  reloads the same authority and reclaims the route. The desktop lifecycle has
  a parallel real-TLS composition test covering app-data authority, route
  claim, audit and joined remote-before-application shutdown.
- The release-built remote React profile loads the Rust cryptographic core from
  a hashed Wasm artifact, fixes one WSS relay URL at build time, rejects remote
  torrent-byte/media breadth and stores private-browser continuity in a
  dedicated IndexedDB database. Its WebCrypto P-256 key is non-extractable;
  host trust and revocable authorization are separate records. The gate tries
  resume before showing a password, retries bounded transient route handoffs,
  automatically reconnects after unexpected authenticated transport loss and
  keeps changed-host recovery blocking and explicit.
- The local and authenticated-remote **Remote access** settings category shows
  every current authorization, tombstone, owner event, failed-attempt bucket
  and live circuit without a default filter. It marks **This browser** and
  supports rename, exact revoke, keep-only, require-password-everywhere,
  circuit close, sign-out and retained-history clear. Provisioning,
  passphrase replacement, disable and recovery remain local-only. Remote
  controls are independently bounded encrypted records; the host derives
  current-browser sign-out from the authenticated circuit rather than a
  caller-provided client ID.
- `scripts/verify-remote-product.mjs` replaces the retired proof crate/page.
  Chrome 152 on macOS arm64 passes first private password login, immediate
  reload resume, browser-process restart, phone viewport, shared-browser
  non-persistence, automatic relay-process restart/route reattachment, local
  installed-layout rollback, complete remote audit rendering, exact
  revocation/tombstone, changed-host blocking, strict HTTPS CSP plus immutable
  hashed assets and zero service workers. A final 256-invalid-circuit churn
  leaves both processes alive; relay RSS moves from 7,307,264 to 7,503,872
  bytes and headless RSS from 97,435,648 to 98,172,928 bytes, with both sampled
  at 0% CPU after drain. The runner uses the actual headless and relay binaries
  on distinct loopback origins and removes every temporary process,
  certificate, authority, profile, payload and reservation root.
- Deterministic native layers cover authorization expiry, replay/reflection,
  generation fencing, individual/global revocation, passphrase replacement,
  disable/recovery, crash prior-or-new persistence, ledger/failure-pressure
  ceilings, exact relay admission and opaque pump cleanup. The local package
  rollback uses the same working-tree binary under a prior immutable-layout
  identity; it is lifecycle evidence, not a cross-release compatibility or
  signed-publication claim.

The stopping condition is met for the deliberately local composition. No
browser persistence outside the isolated client origin and no supported or
public product surface is implied. Public relay/client deployment, DNS/TLS,
Internet abuse operations, signed cross-version rollback and a supported
remote-access claim require a separately authorized tactical.

## Required Evidence

- Pure core, Wasm/browser and dependency gates from Tactical `190` remain green.
- Authority enable/change/disable/reset passes atomic crash injection at every
  write and task transition, with exact secret-file permissions and cleanup;
  authorization/audit commits and revocations have the same prior-or-new rule.
- Fresh, repeated and changed-pin browsers produce the documented results;
  wrong/unknown/offline/busy outcomes remain generic and bounded.
- Private browsers resume without password entry across ordinary socket loss,
  reload, process restart and relay-route reattachment; shared browsers do not
  persist authority. Expired, signed-out, revoked, replayed, reflected, copied
  and generation-stale resume attempts fail closed and require the documented
  full login or identity-recovery path.
- The local and remote security surface lists every authority-bearing record
  and live circuit, closes revoked circuits promptly, keeps revoked state
  non-authorizing, and preserves owner events under failed-attempt pressure
  within the declared record and retention ceilings.
- Direct and relayed negotiation, view snapshot/update/ack and benign command
  reduce identically on the production-shaped adapters.
- Active modification, replay, reordering, reflection, route/relay substitution
  and record attacks fail closed without partially admitted application state.
- Password-file-only, relay-credential-only, resume-only, client-key-only,
  portable-profile and complete live-process clone scenarios retain the
  documented distinctions.
- Slow/flood/name-churn pressure records CPU, resident memory, allocations,
  queues, task counts and rate-limit high waters; shutdown reaches zero owners.
- Desktop and configured headless restart/update/rollback preserve torrents and
  either preserve one valid remote generation or remain disabled.
- Remote client production build, CSP inspection, accessibility, isolated
  desktop/phone-sized real-browser matrix and local artifact provenance pass.
- The loopback-only relay service records local TLS, exact bind rejection,
  capacity, backup/restore, abuse-control simulation, metrics retention and
  exact rollback without logging a protocol payload; the campaign proves no
  nonloopback listener, external account or retained service exists.

The proportional repository baseline remains the Tactical `190` baseline plus
desktop/headless package tests affected by the implementation and the
production-shaped local relay/browser runner.

## Non-Goals

- Google/OIDC, cloud accounts, delegation, sync, email recovery or social
  identity.
- Passkeys, QR enrollment, passwordless authorization, delegated roles or an
  account-wide device identity spanning more than one RSTorrent host. The
  bounded browser authorization and resume registry required above is in scope.
- Hardware-backed host identity, attestation or non-exportable key migration.
- Remote media/file serving, torrent byte upload, arbitrary HTTP/filesystem
  proxying or payload relay.
- Multiple concurrent browsers, relay multiplexing, regions, direct NAT
  traversal, WebRTC/TURN, wake-up delivery or extension control.
- Android/iOS host mode or native remote-controller UI.
- Stable third-party API or permanent public wire compatibility.
- Public relay/client deployment, public DNS or certificate mutation, external
  hosting/account creation, release publication, real Internet-path evidence or
  a supported remote-access product claim.

## Escalation Contract

Routine refactoring, fixtures, exact loopback listeners, temporary local TLS,
isolated local service construction, the source-first resume selection, and
conservative tightening inside these decisions are implementation work once
the tactical is activated. Stop for direction before changing the selected
OPAQUE construction, weakening password/pin/resume behavior, making resume a
transferable unbound bearer, adding an authority or recovery provider,
broadening remote payload/media access, claiming a stronger key tier, using any
nonloopback address or external service/account, publishing client/relay
artifacts, mutating public DNS/TLS, or retaining any service after the local
campaign.
