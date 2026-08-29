# Tactical 190: OPAQUE WebAssembly Relay Foundation

Status: **Active as of 2026-08-29 by explicit user direction.** The
cryptographic pre-code gate below is closed and implementation may proceed.
Desktop signed package and updater Tactical
[`158`](158-desktop-signed-packaging-and-updater.md) remains independently
active under the concurrent-work policy.

Topics:
[`remote-access-authentication`](../topics/remote-access-authentication.md),
[`application-connection-architecture`](../topics/application-connection-architecture.md),
[`runtime-configurations-and-headless-deployment`](../topics/runtime-configurations-and-headless-deployment.md),
[`application-view-api`](../topics/application-view-api.md),
[`client-surfaces`](../topics/client-surfaces.md), and
[`capability-readiness`](../topics/capability-readiness.md).

Dependencies: completed multiplexed application WebSocket Tactical
[`060`](060-multiplexed-application-websocket.md) supplies the inner typed
application frames, bounds, acknowledgement and lifecycle behavior; completed
Tacticals [`101`](101-first-run-web-authentication.md) and
[`109`](109-stable-same-origin-web-launch.md) supply the local browser trust
and exact-origin setup surface; completed headless Tacticals
[`170`](170-configured-linux-headless-service.md),
[`171`](171-signed-headless-release-and-lan-service.md), and
[`174`](174-exact-tailnet-headless-access.md) supply one detachable
application owner without claiming owner E2E access.

## Motivation And Desired Outcome

RSTorrent already has one bounded application connection suitable for an
encrypted relay circuit, but it has no owner password protocol, cryptographic
host identity, relay service, encrypted record layer or remote browser login.
The first remote slice should prove the smallest understandable product:

1. An owner locally chooses one relay-scoped username and strong passphrase.
2. The host opens an outbound connection to a deliberately dumb relay.
3. A new browser enters the same username and passphrase without an account,
   QR code or previously paired device.
4. OPAQUE authenticates the browser directly to the host through the relay.
5. Authenticated encrypted records carry the existing typed application
   connection without exposing application frames to the relay.

Use one Rust protocol implementation natively on the host and through
WebAssembly in the browser. WebCrypto remains a platform capability for secure
randomness and later non-extractable device keys; it does not become a second
OPAQUE or record-protocol implementation.

This tactical is a source-first, controlled vertical proof. It may add pure
crypto/Wasm code, a bounded test relay and the adapters needed to carry a real
application trace, but it does not deploy or advertise a public relay, add a
production Internet listener, persist a supported remote authority in a user
profile, or claim supported remote access. Its stopping condition selects the
exact production construction and leaves one smaller production tactical
ready from executable evidence.

## Accepted Product And Architecture Decisions

### Username and account posture

- The first UI uses **username** and **passphrase** because that is the
  understandable account-free experience. The username is a public,
  relay-scoped routing name, not an email address, cloud account or durable
  cross-relay identity.
- The proof accepts lowercase ASCII names 3 through 32 bytes long, beginning
  and ending with `[a-z0-9]` and containing only `[a-z0-9-]`. Production
  reservation, offensive-name, expiry and recovery policy remain a later
  relay-operation decision.
- Passphrases are exact UTF-8 bytes from 12 through 256 bytes. Clients do not
  trim, case-fold or Unicode-normalize them. The UI recommends a generated
  multiword passphrase and confirms it during local provisioning.
- Google/OIDC login, an RSTorrent cloud account, account delegation, encrypted
  sync, recovery email, friend sharing and multi-user roles are absent. A
  later account layer may issue an independently explicit authorization, but
  it cannot replace or silently alter this host-owned password path.

### OPAQUE and shared implementation

- The selected feasibility target is OPAQUE as specified by RFC 9807, using
  its Ristretto255/SHA-512/3DH configuration and a measured Argon2id key
  stretching function. SRP remains product history, not an implementation
  fallback inside this tactical.
- One runtime-independent Rust crate owns OPAQUE messages and state,
  transcript inputs, derived connection secrets, host-identity extraction,
  encrypted record state and hostile decoding. It compiles natively and to
  `wasm32-unknown-unknown`.
- The browser binding exposes narrow byte-oriented start/finish/seal/open
  operations. It does not duplicate group arithmetic, KDF, AEAD, sequencing or
  protocol state in TypeScript.
- Browser secure random bytes enter through the platform CSPRNG adapter;
  native builds use the operating-system CSPRNG. Deterministic tests inject a
  test RNG only below production construction.
- WebAssembly is not a security boundary from JavaScript. A compromised
  hosting origin can inspect inputs or replace the client before OPAQUE runs.
  Relay-blindness claims therefore assume honest client delivery and do not
  become hosted-code integrity claims.

### Host, relay and connection identity

- Provisioning creates a random 32-byte RSTorrent host ID. It never derives
  identity from `/etc/machine-id`, a hardware serial, hostname, MAC address,
  browser fingerprint or platform installation identifier.
- The OPAQUE server setup/static AKE key is the cryptographic host identity for
  this slice. The browser records its public identity after the first
  successful password-authenticated login and blocks a later mismatch.
- The first proof uses the portable-profile key tier. A complete authority
  copy can therefore clone the host; no hardware-backed or OS-protected claim
  is made. Non-exportable host-key composition remains a later tactical.
- A separate random 32-byte relay registration credential proves which
  backend may occupy the username. It is not the OPAQUE password record, host
  AKE key, application principal or connection traffic key.
- OPAQUE context and identifiers bind the protocol label, relay deployment ID,
  exact username, generated host ID and selected version so cross-route,
  cross-relay and rollback substitution fails instead of authenticating an
  unintended endpoint.
- Every full login derives fresh directional record keys. The passphrase is
  required again after disconnect; remembered devices, bearer sessions and
  resumption are deliberately absent.

### Dumb-relay boundary

- The relay owns username registration, a bounded waiting-host map, pairing,
  byte/message ceilings, attempt accounting, idle timeouts and joined socket
  cleanup.
- After a client selects a username, the relay forwards bounded opaque
  handshake and record messages between exactly one client and one waiting
  host. It does not parse OPAQUE internals or application frames, verify the
  passphrase, derive connection keys, authorize commands, retain torrent
  state, terminate application encryption or proxy payload files.
- The proof admits one waiting host connection and one active client circuit
  per username. A second client receives a bounded busy response. Circuit
  multiplexing, multiple simultaneous browsers and multi-host client sockets
  remain later work.
- The relay observes the username, host/client network endpoints, online and
  connection timing, message sizes and failure/disconnect timing. It can deny
  service or misroute traffic; endpoint authentication must turn rerouting or
  modification into a generic failed connection.
- No compression occurs before or after application encryption in this slice.

## Stable Scenarios

1. **ORF-001 local registration.** An authenticated local browser provisions
   one username/passphrase through the Wasm OPAQUE client. The native host
   receives only bounded registration messages and the resulting password
   record; the passphrase never enters an application command, host log,
   persistence value or diagnostic export.
2. **ORF-002 separate route claim.** The host registers the username with a
   distinct relay credential. Reconnect with the same credential restores the
   waiting route; a missing, wrong or replayed claim cannot evict the current
   host accidentally.
3. **ORF-003 new-browser login.** A clean browser with only relay URL,
   username and passphrase completes RFC 9807 OPAQUE against the native host.
   Both endpoints derive equal connection secrets while the relay capture
   contains neither passphrase nor usable application plaintext.
4. **ORF-004 host pin.** First successful login records the authenticated host
   ID/public identity. The same host succeeds later; a changed host identity
   produces a blocking identity-change result rather than password retry,
   silent repin or downgrade.
5. **ORF-005 encrypted application trace.** The established circuit carries
   negotiation, one view-set snapshot/update/acknowledgement and one benign
   semantic call through the existing connection frames. The reduced client
   state and host trace match the direct adapter exactly.
6. **ORF-006 active relay.** Modified, replayed, reordered, reflected,
   truncated, cross-route and cross-relay handshake messages fail without an
   authenticated application owner. Record tamper, duplicate sequence,
   skipped sequence, direction reflection and post-close input fail closed.
7. **ORF-007 credential failures.** Wrong passphrase, unknown username,
   offline host, fake password record and missing host setup yield bounded
   generic client failures. Host and relay diagnostics retain a useful class
   without exposing a reliable remote password oracle.
8. **ORF-008 clone distinctions.** Password-record-only, route-credential-only,
   portable-profile and complete-authority clone fixtures produce the outcomes
   recorded by the owning topic. The portable-tier limitation is explicit;
   OPAQUE is not claimed to distinguish a perfect clone.
9. **ORF-009 bounded pressure.** Slow handshakes, invalid-proof floods,
   oversized messages, name churn, client disconnect and host replacement do
   not create unbounded state, steal another route or leak a task.
10. **ORF-010 native/Wasm equivalence.** RFC vectors, deterministic RSTorrent
    fixtures and independent serialized messages produce the same accepted
    result natively and in a real browser. Browser bundle size, peak Wasm
    memory and KSF latency are recorded rather than inferred.

## Reference Dossier And Pre-Code Gate

No external source, fixture or test vector is imported merely by this design.
The exact selection, audit result and record construction below close the
pre-code gate. Any change to those choices must be recorded before dependent
protocol code changes.

### Normative specifications

- [RFC 9807](https://www.rfc-editor.org/rfc/rfc9807.html) owns OPAQUE
  registration, 3DH login, context/identifier binding, KSF requirements,
  server compromise analysis, forward secrecy and Appendix C vectors.
- [RFC 9497](https://www.rfc-editor.org/rfc/rfc9497.html) owns the VOPRF
  construction used by the selected OPAQUE configuration.
- [RFC 9106](https://www.rfc-editor.org/rfc/rfc9106.html) owns Argon2id and its
  parameter/security guidance. Browser parameters require measured adaptation,
  not silent replacement with the identity KSF.
- [RFC 5869](https://www.rfc-editor.org/rfc/rfc5869.html) and
  [RFC 8439](https://www.rfc-editor.org/rfc/rfc8439.html) are the candidate
  HKDF and ChaCha20-Poly1305 record primitives. This tactical must record the
  exact directional derivation, nonce, associated-data, sequence, exhaustion
  and close construction before implementing records.
- [Web Cryptography Level 2](https://www.w3.org/TR/webcrypto/) owns browser
  randomness and future non-extractable `CryptoKey` behavior. It does not
  expose OPAQUE, its VOPRF/Ristretto construction or Argon2id.

### `opaque-ke` candidate

The initial exact candidate is
[`opaque-ke` `4.0.1`](https://crates.io/crates/opaque-ke/4.0.1), crates.io SHA-256
`ded22991b43cd15561b62b2e1cf9ace1344a8534eebec96202d5c96a77a6616a`,
tag `v4.0.1` commit `75fe4cdddb7946440054da0c8e7cdd73828af3f9`,
licensed `MIT OR Apache-2.0`, with Rust 1.85 minimum. RSTorrent's Rust 1.97
baseline satisfies that toolchain floor. The source survey identified:

- `src/opaque.rs` for server setup, registration/login state, context,
  identifiers, fixed serialization and reflected-value rejection;
- `src/messages.rs`, `src/envelope.rs` and `src/serialization/` for message,
  envelope, identity-element and hostile decoding behavior;
- `src/ksf.rs` for the `Ksf` seam and optional Argon2 integration;
- `src/key_exchange/tripledh.rs` and
  `src/key_exchange/group/ristretto255.rs` for the selected AKE/group;
- `src/tests/rfc9807_vectors.rs` and `test_opaque_vectors.rs` for the exact RFC
  real/fake vector corpus with client/server identifiers;
- `src/tests/full_test.rs` and `full_test_vectors.rs` for deterministic
  registration/login and malformed/scalar coverage;
- `src/serialization/tests.rs` for property-based round trips and invalid
  encodings; and
- `tests/remote_key.rs` for the external private-key seam that may matter to a
  later hardware-backed host identity.

The NCC Group audit cited by the project covered a 2021 pre-final release and
its fixes landed by `1.2.0`; it is useful evidence but not an audit of `4.0.1`
or the complete RSTorrent composition. The pre-code gate must review the audit,
the 4.0.1 change history, open security advisories and the final-RFC delta. It
must also reject the prerelease `4.1.0-pre.2` unless a separately recorded need
justifies adopting a prerelease.

### Browser/Wasm and product references

- [`@serenity-kit/opaque` `1.1.0`](https://www.npmjs.com/package/@serenity-kit/opaque)
  is a browser/Wasm feasibility and security-review reference built on
  `opaque-ke`, not an accepted dependency or independent cryptographic
  implementation. Its published measurements show
  roughly one-second 64-MiB Argon2id work on an M1 and much slower/memory-risky
  RFC-oriented parameters; RSTorrent must measure its own targets.
- The local YepAnywhere checkout was refreshed at clean relevant paths on
  commit `dcf0449d5336c866d37c50cb1f2e1df66ed50663`. Its
  `docs/project/relay-design.md`, `topics/relay-client-mux.md`,
  `docs/project/relay-head-of-line-blocking.md`,
  `packages/client/src/lib/connection/SecureConnection.ts`,
  `packages/shared/src/relay.ts`, and `packages/relay/src/mux-handler.ts`
  remain architecture/failure references for username routing, one inner
  protocol, opaque forwarding, fairness and cleanup. Its SRP, NaCl, resume,
  HTTP-like messages, limits and hosted deployment are not adopted.
- Tactical [`060`](060-multiplexed-application-websocket.md) remains the
  RSTorrent application-frame authority. The relay proof wraps those frames;
  it does not tunnel arbitrary HTTP paths or create another command API.

### Closed selection and audit record (2026-08-29)

The selected OPAQUE construction is RFC 9807 `OPAQUE-3DH` with the
Ristretto255/SHA-512 VOPRF ciphersuite, `TripleDh<Ristretto255, Sha512>`, and a
custom `Ksf` adapter using Argon2id version 1.3 with 64 MiB memory, three
passes, parallelism one and a 64-byte output. The adapter uses the same fixed
16-byte zero salt as `opaque-ke`'s Argon2 seam because the KSF input is already
the OPRF-derived randomized password. The browser measurement matrix must
confirm this selection remains within the tactical's five-second and 256-MiB
bounds; exceeding either bound is a recorded result and requires revisiting
the selection rather than falling back to the identity KSF.

The dependency selection is:

- exact `opaque-ke` `4.0.1` with `default-features = false` and only
  `ristretto255`; its fixed serialization is used instead of Serde;
- `argon2` `0.5.3` with allocation and zeroization support;
- `hkdf` `0.12.4`, the workspace `sha2` `0.10.9`, and
  `chacha20poly1305` `0.10.1` with only allocation support for records;
- the `rand` 0.8 line and `rand_chacha` `0.3.1` for native OS randomness and
  deterministic 32-byte browser-CSPRNG/test seeds; and
- `zeroize` `1.9.0`, plus `wasm-bindgen` `0.2.127` only in the browser binding.

The resolved OPAQUE normal graph inspected before adoption includes
`curve25519-dalek` `4.1.3`, `voprf` `0.5.0`, `elliptic-curve` `0.13.8`,
`hkdf` `0.12.4`, `hmac` `0.12.1`, `digest` `0.10.7`, `sha2` `0.10.9`,
`subtle` `2.6.1`, `zeroize` `1.9.0`, `derive-where` `1.6.1`, `displaydoc`
`0.2.7`, and pinned `generic-array` `0.14.7`. The final repository lockfile
and `cargo tree` remain the exact build authority.

All selected direct and normal-graph transitive licenses are permissive and
compatible with the RSTorrent MIT project: MIT/Apache combinations,
BSD-1-Clause, BSD-2-Clause, BSD-3-Clause, Unicode-3.0, and the optional
LLVM-exception form on target support. No selected published package contains
an upstream `NOTICE` file. Dependencies remain registry inputs with their own
license metadata; no source, fixture, vector or asset is copied, so no new
repository notice file is required.

The 2021 NCC Group review covered `opaque-ke` `0.5.0` against OPAQUE draft 03,
with a focused retest of fixes in `1.2.0`; it is not an audit of `4.0.1` or
this composition. Its identity-element validation, reflected OPRF value,
I2OSP length and constant-time transcript-MAC findings are represented by the
current implementation checks and upstream tests. Version `4.0.0` synchronized
to final RFC 9807, made dummy-record generation unconditional, and changed
serialized setup/registration state; `4.0.1` only repaired documentation.
There are no published GitHub advisories for the project and the 2026-08-29
RustSec database contains no `opaque-ke` entry. `cargo audit` reports no known
vulnerabilities in the current workspace lock. The `4.1.0-pre.2` prerelease is
rejected because its KEM work is outside this proof.

Exact tag `v4.0.1` at
`75fe4cdddb7946440054da0c8e7cdd73828af3f9` was checked out in a temporary
directory. Its source and tests named above were inspected, and its 83 library
tests with `--no-default-features --features ristretto255` pass, including the
RFC 9807 real/fake vectors, hostile deserialization properties and reflected
value/identity-element cases. The checkout and build tree were removed after
the run.

OPAQUE inputs use one canonical length-prefixed binding containing the fixed
protocol label, protocol version, 32-byte relay deployment ID, exact username
and 32-byte random host ID. The credential identifier, client identifier,
server identifier and OPAQUE context receive distinct fixed labels over that
binding. OPAQUE already authenticates its server static public key; a client
pins the resulting 32-byte server public key together with the host ID after a
successful login.

The encrypted record construction is fixed as follows:

1. HKDF-SHA-512 extracts from the 64-byte OPAQUE session key with the SHA-512
   digest of the canonical binding as salt.
2. Four domain-separated labels derive client-to-host and host-to-client
   32-byte ChaCha20-Poly1305 keys and four-byte nonce prefixes.
3. Each record begins with a 16-byte authenticated header: ASCII `RSR1`, one
   direction byte, one flags byte, two zero reserved bytes and a big-endian
   64-bit sequence starting at zero. The WebSocket message boundary supplies
   length; the header is AEAD associated data.
4. The 96-bit nonce is the direction's four-byte prefix followed by the
   sequence. Only flag bit zero is defined as authenticated close; a close has
   empty plaintext. Unknown flags, nonzero reserved bytes, wrong direction,
   duplicate/skipped sequence, malformed tag and post-close use fail closed.
5. Each direction stops before sequence `2^32`; adapters additionally enforce
   the 24-hour lifetime and their asymmetric plaintext bounds. Records are not
   compressed, padded or resumed.

## Owner, Task, Cancellation, And Dependency Map

```text
local or remote React client
  -> thin TypeScript transport/binding owner
       -> Wasm remote-crypto state
            OPAQUE client + host pin + record opener/sealer
       -> browser WebSocket to proof relay
              |
              v
proof relay: username claim/wait/pair + opaque bounded forwarding
              |
              v
native host relay task
  -> pure remote-crypto state
       OPAQUE server + directional record opener/sealer
  -> existing Rust application-connection core
       calls + view attachment + exact acknowledgement
```

| Owner | State and work | Termination |
| --- | --- | --- |
| Pure remote-crypto crate | Protocol values, fixed codecs, OPAQUE states, transcript inputs, key derivation and record sequence | Success, typed rejection, explicit zeroizing close or owner drop |
| Wasm binding | Opaque handles to one client registration/login/record generation and byte copies at the JS boundary | Finish/failure consumes state; page close drops remaining handles |
| Browser remote client | Username/passphrase form, secure RNG adapter, host pin, socket, application-call and view attachment lifecycle | User close, page lifecycle, authentication failure or socket close aborts and joins logical work |
| Host remote owner | Host/setup/record inputs for the proof, relay registration, one active login and application-connection generation | Disable, relay failure, circuit close or application shutdown cancels and awaits every child |
| Relay route owner | Bounded username claim, one waiting host and one paired circuit | Credential replacement, timeout, socket close or relay shutdown releases the exact route generation |
| Relay pair | Two bounded forwarding queues and pump tasks | Either socket close cancels both directions and awaits both pumps |
| Application connection | Existing calls, attachments, queues, cursors and joined teardown | Existing Tactical `060` lifecycle, additionally cancelled by authenticated circuit close |

The pure crate depends on cryptographic primitives but not Tokio, Axum,
WebSocket, filesystem, SQLite, browser APIs, task handles or application DTOs.
Native and Wasm adapters depend inward on it. Relay routing does not depend on
OPAQUE or application frame types. The application connection receives an
already authenticated principal and bounded plaintext frame channel; it does
not inspect the relay route or password.

## Initial Bounds And Secret Handling

| Resource | Tactical bound |
| --- | ---: |
| Username | 3..=32 lowercase ASCII bytes |
| Passphrase | 12..=256 exact UTF-8 bytes |
| Host ID / relay credential / relay ID | exactly 32 random bytes each |
| Registration or OPAQUE handshake message | 4 KiB |
| Full-login deadline | 20 seconds including client KSF work |
| Simultaneous unauthenticated logins per route | 2 |
| Simultaneous unauthenticated logins per host | 8 |
| Proof-relay registered routes | 1,024 |
| Proof-relay total paired circuits | 256 |
| Waiting hosts / active circuits per route | 1 / 1 |
| Client-to-host encrypted plaintext | existing 64 KiB application limit |
| Host-to-client encrypted plaintext | existing 16 MiB + 4 KiB application limit |
| Relay message | plaintext ceiling + 64 KiB crypto/outer allowance |
| Forward queues | one admitted message plus one write in flight per direction |
| Invalid pre-auth messages | 3, then generic close |
| Record lifetime | at most 24 hours or `2^32` records per direction, whichever comes first |

The KSF evaluation must compare Argon2id candidates from 32 through 128 MiB,
one through four passes and parallelism one. The final choice stays within
that range, keeps peak browser Wasm memory below 256 MiB, and targets at most
five seconds on the slowest controlled browser profile in this tactical while
recording the resulting offline-guessing tradeoff. This does not claim
representative low-end physical-device performance; the production tactical
must measure that separately. Failure to find an acceptable point is a
protocol/product result, not permission to use the identity KSF.

Passphrase, OPAQUE client state, server setup, registration record, relay
credential, session/export/record keys and deterministic RNG seeds never
implement `Debug` with contents and never enter URLs, environment variables,
command arguments, logs, metrics, crash reports or support exports. Wasm
linear-memory zeroization is best effort and is not described as protection
from the page. Controlled tests use generated temporary values and remove the
exact temporary root after joined shutdown.

## Shape-Changing Edge Cases

- Local provisioning interruption cannot install a partial password record or
  enable a relay owner. Retrying creates one explicit replacement generation.
- Username change, passphrase change, host reset and relay-credential rotation
  are distinct conceptual transitions even though only fresh provisioning and
  complete disable/recreate need product UI in the proof.
- Unknown-user processing must use the library's fake-record path where
  applicable and preserve bounded comparable behavior without manufacturing
  authenticated state.
- Host ID, route, relay ID, protocol version, algorithm suite and record
  construction are transcript-bound. A matching passphrase does not override
  a previously pinned host mismatch.
- WebSocket ordering does not excuse missing record sequence checks. Duplicate,
  missing, reflected or post-close records fail the circuit.
- Record nonces are deterministic from independently derived directional
  material and a monotonically checked counter; randomness per record is not
  trusted to prevent reuse.
- One maximum snapshot cannot allocate an uncounted encrypted copy in every
  relay or browser queue. Existing application reservations plus the exact
  one-message forwarding queues own the high water.
- Relay host replacement is generation-fenced. A late close or message from an
  old host/client cannot unregister, authenticate or feed a newer pair.
- Browser refresh destroys the connection and requires a full password login.
  No hidden resume token or root traffic key is retained.
- Binary `.torrent` attachment frames and media capability bytes are rejected
  by the remote adapter rather than passing unreviewed bulk data through the
  first record protocol.

## Implementation And Validation Sequence

1. **Close the cryptographic dossier.** Re-read RFC 9807/9497/9106 errata,
   `opaque-ke` 4.0.1 source/tests/audit and candidate record dependencies.
   Append the exact selected ciphersuite, Argon2id parameters, HKDF labels,
   AEAD record construction, dependency graph and license result before
   product protocol code.
2. **Build the pure core.** Add fixed versioned values/codecs, OPAQUE
   registration/login wrappers, transcript binding, directional record state
   and RFC/adversarial tests without sockets, files, async runtime or browser.
3. **Build the Wasm seam.** Compile the same core for
   `wasm32-unknown-unknown`, inject browser randomness, expose consuming opaque
   state handles, and prove byte-for-byte native/Wasm vectors in a real
   headless browser. Measure bundle, CPU and peak memory for the complete KSF
   matrix.
4. **Build the bounded proof relay.** Add an opt-in local-only relay harness
   with route registration, generation fencing, one waiting/paired circuit,
   exact forwarding bounds, generic failures, metrics and joined shutdown.
   It stores no durable public namespace and is not installed or deployed.
5. **Compose a real application trace.** Adapt the existing application-
   connection core beneath the native secure circuit and the existing React
   application client above the Wasm circuit. Prove negotiation, view update,
   acknowledgement and benign call equivalence; reject attachment/media
   breadth explicitly.
6. **Run adversarial/runtime gates.** Exercise wrong/unknown/offline, active
   modification, route substitution, clone fixtures, floods, oversize,
   disconnect races, relay/host restart and clean task/resource teardown.
7. **Record the production boundary.** Update every owning topic with actual
   measurements and selected constructions. Create the smaller production
   tactical covering durable profile authority, supported host platforms,
   public relay operation/client delivery and recovery UX; do not expose this
   proof as supported remote access.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure protocol | RFC 9807 real/fake vectors; fixed serialization; wrong password; identifier/context mismatch; malformed/identity/reflection cases; directional derivation and exact record sequence/tamper/exhaustion |
| Native/Wasm | Identical deterministic messages and secrets; real browser registration/login; secure RNG failure; state-handle reuse rejection; bundle/CPU/peak-memory measurements |
| Scripted relay | Route claim/reconnect/conflict, wrong credential, one-circuit busy, unknown/offline generic failure, bounds, slow peer, generation replacement and joined two-direction cleanup |
| Application composition | Same hello/snapshot/update/ack/call reducer trace direct and relayed; relay capture contains only routing metadata and opaque bytes; no semantic HTTP or application-frame parsing at relay |
| Adversarial clones | Record-only, relay-only, profile-only and complete-authority fixtures against new and pinned clients with the documented distinct outcomes |
| Platform builds | Rust workspace, production web typecheck/tests/build and Wasm build; Android/iOS integration is inapplicable because this proof adds no native mobile host/controller surface, but the pure crate must not preclude their later native build |
| Live/external | No public relay, public DNS, cloud account, port forwarding, firewall, physical device or third-party login is authorized or required |

The proportional baseline is:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
npm run build --prefix clients/web
```

The implementing record must add exact Wasm/browser and controlled relay
commands once their harness names exist and report exactly what ran.

## Non-Goals And Next Boundary

- No Google/OIDC login, RSTorrent account, delegated authorization, cloud
  recovery, encrypted sync or provider token.
- No remembered device, passkey, WebAuthn, session resumption, bearer token,
  QR enrollment or sign-out-everywhere behavior.
- No hardware-backed/OS-protected host key, attestation or migration of a
  non-exportable identity.
- No public relay deployment, DNS, TLS certificate, release package, stable
  namespace, abuse operation, billing, support promise or public wire
  compatibility.
- No multiple simultaneous clients per host, relay mux, multi-host client,
  regional discovery, direct NAT traversal, UPnP, WebRTC or TURN.
- No remote `.torrent` byte attachment, media serving/streaming, payload data,
  compression, padding or traffic-analysis-resistance claim.
- No Android/iOS/extension remote UI or background relay lifecycle.
- No generic daemon API, REST proxy, filesystem proxy, peer socket or payload
  path through the relay.

The next tactical begins only from a passing proof and owns durable
application-private authority, enable/disable/change/recovery UX, exact
desktop/headless host support, independently delivered remote client assets,
public relay operational policy and representative external evidence. Account
delegation remains a separate later decision even after that production path.

## Escalation And Stopping Condition

Routine module extraction, binding generation, deterministic fixtures,
headless-browser use, local temporary listeners and conservative tightening
within the declared bounds are authorized when implementation of this tactical
is user-directed. It may become active without pausing unrelated tacticals.
Stop for direction if evidence requires abandoning OPAQUE, using a prerelease
or materially different cryptographic library, accepting an identity KSF,
changing the username/passphrase product, persisting production authority,
deploying a relay, introducing an account or recovery authority, supporting
bulk/media data, or weakening host-pin/clone behavior.

This tactical is complete only when one native-host/browser-Wasm OPAQUE login
through the bounded dumb relay carries the exact real application trace;
normative vectors and adversarial cases pass; KSF, bundle, memory, queue and
task high waters are recorded; the exact cryptographic construction and
limitations are reconciled in the owning topics; all temporary state is
removed; and a separate production tactical is decision-ready. A successful
handshake alone, an encrypted echo, or source code that has not passed a real
browser and active-relay adversarial matrix does not satisfy the stopping
condition.
