# Owner Remote Access Authentication

Topic: `remote-access-authentication`

Status: Direction and investigation background accepted from maintainer
discussion on 2026-08-04. Owner end-to-end remote control is the first desired
future remote feature, with username/passphrase login available from a new
device without prior pairing or a central product account. No production
listener, relay, password protocol, cryptographic library, platform key
dependency, persistence schema or stable remote wire contract is authorized
or implemented by this topic.

## Purpose And Scope

This topic owns the security background and decision boundary for authenticating
an owner to one RSTorrent host through a direct or relay-mediated application
connection. It records:

- the password-authenticated login semantics desired by the product;
- the relationship among password, host, device, relay and connection
  identities;
- the SRP and OPAQUE distinction without prematurely selecting either;
- profile theft, verifier theft, host cloning, active proxy, downgrade,
  resumption and live-process attack scenarios;
- hardware-backed, operating-system-protected and portable host-key tiers;
- the evidence required before choosing libraries or implementing a protocol;
  and
- the known boundary between cryptographic transport privacy and trust in the
  browser application that receives the password and plaintext.

[`application-connection-architecture.md`](application-connection-architecture.md)
owns the typed inner application frames, multiplexing, view attachment,
fairness, bounds and opaque relay layering. This topic owns how a remote
principal and encrypted record channel may eventually be established around
those frames. [`application-control.md`](application-control.md) continues to
own command meaning and the rule that authorization is verified transport
context rather than a caller-provided field.

Tactical
[`076-authenticated-private-web-host.md`](../tactical/076-authenticated-private-web-host.md)
is a separate maintainer-operated preview protected by HTTPS deployment and
HTTP Basic authentication. Its credential is intentionally not owner E2E
remote authentication, device identity, relay authentication or evidence that
this topic has been implemented.

Tactical
[`101-first-run-web-authentication.md`](../tactical/101-first-run-web-authentication.md)
is the separate ordinary loopback product boundary. Its HttpOnly browser
sessions, four-digit approval ticket, and explicit restart recovery make a
headless local UI understandable and revocable; they are intentionally too
weak and too host-local to become a password, remembered remote device, relay
credential, or E2E authentication claim.

[`http-file-serving-and-streaming.md`](http-file-serving-and-streaming.md)
owns local verified-file capability URLs and future incomplete-file streaming.
Its loopback capability is not sufficient for nonlocal access; any remote
media route additionally requires the principal, authenticated encryption,
host identity, and authorization selected here.

## Product Outcome

The desired ordinary owner experience is:

1. On the host, choose a remote routing name and strong passphrase.
2. From any new browser or first-party client, enter that name and passphrase.
3. Authenticate directly with the host through an untrusted relay without
   transmitting the passphrase to the relay or making the relay the identity
   authority.
4. Establish an authenticated end-to-end encrypted application connection.
5. Optionally remember that installation as a named, individually revocable
   device.
6. Retain passphrase login as the universal path when a remembered credential
   is absent, expired, revoked or cleared.

Mandatory QR pairing is not the root of owner access. A QR code may later
carry a URL, routing name or one-time device-enrollment capability, but losing
all paired devices must not prevent an owner who still knows the passphrase
from reaching an online uncompromised host.

The host remains the owner credential authority. A relay may own routing-name
registration, abuse limits and connection rendezvous, but remote access does
not require an RSTorrent cloud account, recovery email or another paired
device's approval.

## Terms

| Term | Meaning here |
| --- | --- |
| **E2E** | End-to-end protection in which the controller and RSTorrent host, but not the relay, can read or modify authenticated application content. |
| **PAKE** | Password-Authenticated Key Exchange, the protocol family that proves password knowledge and derives a shared key without sending the password. |
| **aPAKE** | Augmented or asymmetric PAKE, where the server stores a password-derived credential record rather than the password. |
| **SRP** | Secure Remote Password, a specific aPAKE and the protocol family already used by YepAnywhere. |
| **SRP-6a** | The improved SRP variant normally intended by a current SRP design. Exact parameters and library behavior still require review. |
| **OPAQUE** | A specific newer aPAKE standardized by RFC 9807. Its name is less important than its credential-envelope and authenticated-key-exchange properties. |
| **AKE** | Authenticated Key Exchange, a protocol that derives a key while authenticating the parties. |
| **AEAD** | Authenticated Encryption with Associated Data, which hides record content and rejects modification. |
| **KDF** | Key Derivation Function, used to derive separately scoped keys from an authenticated shared secret. |
| **OPRF** | Oblivious Pseudorandom Function, the password-processing primitive used by OPAQUE. |
| **HSM** | Hardware Security Module, a service or device that can use a private key without exporting it. |
| **TPM** | Trusted Platform Module, security hardware available on many PCs. |
| **TEE** | Trusted Execution Environment, isolated execution used by some platform keystores. |
| **TOFU** | Trust On First Use, where a client remembers the first host key it observes and rejects later changes. |

SRP is already a PAKE; `PAKE` is not an alternative user experience. SRP and
OPAQUE both support username/passphrase login and produce a shared secret.
The protocol name must not leak into product copy unless it helps diagnostics
or an advanced security explanation.

## Identity And Authority Model

Remote authentication must not collapse several distinct identities into one
token:

```text
owner principal
  +-- password credential
  |     +-- PAKE bootstrap and universal new-device login
  +-- remembered device A
  |     +-- independently revocable device public key
  +-- remembered device B
        +-- independently revocable device public key

RSTorrent host
  +-- host identity public/private key
  +-- password credential record
  +-- device registry
  +-- bounded resume records

relay
  +-- opaque host routing identity
  +-- independently rotatable host-to-relay registration credential
  +-- no application principal or decryption authority
```

The password credential authenticates owner knowledge. It does not identify a
particular browser, phone or installation because every new device uses the
same passphrase. A remembered device therefore generates or obtains a separate
key and proves possession of it on later connections.

The host identity key answers which RSTorrent installation accepted the
connection. A device key answers which remembered controller initiated it.
The relay registration credential answers which backend may claim a routing
slot. A connection traffic key protects one connection generation. None is a
substitute for the others.

An eventual authenticated connection context should be able to distinguish at
least:

- the application principal and attached authorization capabilities;
- password versus remembered-device versus resume authentication;
- a stable device identity when one has been enrolled;
- the verified host identity and its protection tier;
- the connection generation and negotiated protocol version; and
- the request, replay and rate-limit context.

Application frames must never accept `is_admin`, `read_only`, `device_id` or
similar assertions from untrusted client payloads as their authority.

## Current Direction And Open Selection

The accepted requirement is a password-authenticated E2E key exchange through
an untrusted relay. SRP-6a is the concrete behavioral baseline because the
maintainer has shipped and extended that model in YepAnywhere. OPAQUE is a
candidate, not an accepted replacement.

The future security tactical must select one reviewed construction rather than
compose new cryptographic protocols from primitives. The selection must be
based on current library, platform and attack evidence rather than protocol
novelty.

The following remain deliberately open:

- SRP-6a, OPAQUE or another reviewed aPAKE;
- exact groups, curves, hashes, key-stretching and cipher suites;
- Rust, browser/Wasm, Android and Apple implementation libraries;
- whether the password protocol directly authenticates a host identity key or
  a separately reviewed transcript signature supplies that binding;
- whether a routing name is globally human-readable, scoped by relay or backed
  by a random opaque host identifier;
- credential storage, password normalization, password change and rate policy;
- device enrollment, key storage, attestation and revocation details;
- resume construction, duration, rotation and recovery;
- host backup, migration, replacement and identity-reset UX; and
- the exact assurance shown to the user for hardware-bound, OS-protected and
  portable host identities.

## SRP And OPAQUE Background

### Shared properties

Both are augmented password-authenticated key exchanges intended to provide:

- username/passphrase semantics;
- no password transmission;
- mutual password-based authentication;
- a shared secret suitable for deriving connection keys; and
- no dependency on a conventional public certificate hierarchy for the
  password exchange itself.

Neither protocol by itself identifies individual devices, defines application
authorization, supplies a relay, implements record framing, solves browser-code
integrity or makes a compromised live host process safe.

### SRP baseline

An SRP server stores a salt and password verifier. A successful ephemeral
exchange proves compatible password knowledge and derives a shared secret.
YepAnywhere derives a record-encryption key from that result and encrypts its
inner application protocol separately.

SRP's published security considerations state that an attacker who obtains the
verifier can masquerade as the server to that user and can attempt a dictionary
attack. Verifier possession alone does not prove the password to the original
server as a client. A transparent attacker that terminates both sides therefore
needs an additional client credential, resume secret, recovered password or
compromise of the original host.

### OPAQUE candidate

OPAQUE combines an OPRF-backed password credential envelope with an
authenticated key exchange. The standardized construction includes an
independent server AKE private key, client credential records, an OPRF seed and
ephemeral key exchange.

This separation can matter when the server AKE private key is protected more
strongly than the profile credential database. RFC 9807 specifically notes
that the AKE key may be held by an HSM so compromise of the OPRF seed and client
envelopes alone does not enable server spoofing. OPAQUE also specifies
precomputation resistance and forward-secrecy properties for a full login.

OPAQUE does not protect a perfect clone that possesses every long-term server
secret. A corrupted single server can still perform an exhaustive offline
dictionary attack, and a resumption design that persists or reuses the wrong
key can discard forward-secrecy advantages. Its newer standard and more
complex state are reasons for library and operational evaluation, not reasons
to reject or adopt it automatically.

### First-use distinction

A separate pinned host signing key can strongly augment SRP for clients that
already know that public key. It does not automatically protect a completely
new client that knows only the routing name and passphrase: an attacker holding
the SRP verifier may conduct the client-facing SRP exchange and offer its own
host key.

OPAQUE can bind the password credential to the server AKE identity. When that
identity private key is non-exportable and only profile credential records are
stolen, this may protect even a new password-only client. Achieving an
equivalent property around SRP requires a separately reviewed trusted binding;
an ad hoc signed extension is not acceptable merely because its individual
primitives are sound.

This is the material protocol-selection question:

> When stronger platform key protection is available, must a completely new
> password-only client reject a clone made from only the portable RSTorrent
> profile and relay credential?

If the answer is yes, OPAQUE or an equivalently reviewed password-to-host-key
binding has a concrete advantage. If the accepted threat model protects only
previously pinned clients from such a clone, SRP plus a separate host identity
may be sufficient.

## Attack Scenarios

The table assumes correct protocol validation, a strong passphrase, bounded
online attempts and an authenticated record layer after login. Missing those
conditions can dominate every distinction below.

| Scenario | Expected analysis and behavior |
| --- | --- |
| Passive network or relay observation | PAKE messages and encrypted records must not reveal the password, application frames or traffic keys. Routing identity, endpoints, timing and ciphertext sizes remain observable. |
| Blind active relay | Forwarding an honest handshake unchanged may connect the real endpoints but must not reveal their key. Modifying the transcript or records must be rejected. |
| SRP verifier theft | The thief may impersonate the SRP server to a client and attempt offline password guesses. The verifier alone does not authenticate the thief as a client to the original host. |
| Credential-record theft with protected OPAQUE server key | The stolen record and OPRF material must not substitute for the non-exportable server AKE private key. Exact guarantees depend on the selected construction and key separation. |
| Portable profile clone | If every password, host, relay, device and resume secret is exportable in the profile, the clone may be cryptographically indistinguishable from the original. No protocol name repairs identical long-term secrets. |
| Existing client meets profile-only clone | A client with a pinned host public key must reject a clone that lacks the corresponding private key. Password success must not override the mismatch. |
| New client meets profile-only clone | Pure SRP plus an unpinned host signature may accept the clone. A password-bound protected host identity may reject it; this is a required protocol-feasibility test. |
| Stolen client resume credential | A bearer-like resume secret may allow direct impersonation and can enable a terminating proxy when paired with server impersonation. Resume must be device-bound, replay-resistant, independently revocable and no more authoritative than its parent device. |
| Active terminating proxy | If the attacker can authenticate independently to both sides, UI and commands may appear normal while content is observed or changed. Host identity, transcript binding, device proof and record authentication must prevent undetected termination. |
| Host key removed or protection tier lowered | A previously verified client must stop with an identity-change failure. It must not silently accept SRP-only, a new software key or an unverified fallback. |
| Live RSTorrent process compromise | A process attacker may read plaintext, issue commands and use an OS or hardware key as a signing oracle while control persists. Non-exportability may prevent taking the private key away but does not make the active process trustworthy. |
| Host OS or privileged compromise | Platform key isolation may still impede extraction, but the attacker may invoke the key, replace application code, alter UI or control routing. Claims require platform-specific evidence and must not treat hardware as absolute. |
| Hosted-client compromise | JavaScript that receives the password and plaintext can exfiltrate both before E2E protection applies. Relay blindness does not protect against malicious client code delivered by the hosting origin. |
| Password guessing | Relay and host both require bounded per-route, per-source and aggregate attempt controls without turning different errors or timing into a reliable username/password oracle. |
| Protocol downgrade or rollback | Version, algorithms, host identity, capabilities and key-protection expectations must be transcript-bound and pinned where appropriate. A successful older handshake must not silently erase a previously established higher floor. |

## Host-Key Protection Tiers

Platforms expose materially different facilities under names such as keychain,
keystore, credential vault, Secure Enclave and TPM.

| Tier | Private-key behavior | Protection and limitation |
| --- | --- | --- |
| Hardware-backed non-exportable | RSTorrent holds a handle and requests signing or key-agreement operations; private bytes do not enter its process. | Strongest defense against offline profile copying and key extraction. A live process attacker may still use the key as an oracle. Availability, algorithms and background-use policy vary. |
| OS-protected exportable | The OS encrypts the key at rest but returns plaintext key material to an authorized process. | Separates the key from the RSTorrent profile and may require OS credentials, but a compromised process can normally request or read it. |
| Portable profile key | RSTorrent stores an encrypted or plaintext software key with portable profile state according to the eventual storage design. | Broadest compatibility and easiest migration, but a sufficiently complete profile copy can clone host authority. |

Graceful degradation applies at initial provisioning on a platform that lacks a
stronger capability. It does not mean runtime silent downgrade. The chosen tier
and public identity become part of the client's trust record. If a
hardware-bound key becomes unavailable, recovery creates an explicit host
identity transition rather than falling through to a software key under the
old identity.

The platform adapter should expose cryptographic capability and key handles;
protocol/domain code must not depend on Keychain, Secure Enclave, Android
Keystore, TPM, Windows CNG or Linux secret-service types. The feasibility
campaign must determine which common algorithm, if any, can be implemented
without moving long-term private bytes through Rust, Kotlin or JavaScript.

## Device Identity And Resumption

PAKE authenticates password knowledge, not a physical device. After a
successful password login, an installation may offer to become a remembered
device:

1. Generate a device key using the best available local platform facility.
2. Prove possession of the device private key inside the authenticated E2E
   connection.
3. Ask the host to create a bounded device record with a stable random ID,
   public key, user-visible label, creation time, last-use state and revocation
   state.
4. Authenticate later connections with that device key through a reviewed AKE
   or signature-bound exchange.

A displayed device fingerprint is a human representation of the public key,
not the authority itself. Browser or hardware characteristics, user-agent
strings and probabilistic fingerprinting are not cryptographic device
identity.

Session resumption is a connection optimization beneath the same device
principal. It must not persist the root PAKE traffic key as a transferable
full-owner bearer credential. Resume proof must be fresh, challenge-bound,
replay-resistant, expiry-bounded, independently revocable and cryptographically
bound to the host identity, device identity, protocol floor and parent
authorization.

Changing the passphrase, revoking one device, signing out everywhere and
replacing the host are distinct operations. Their invalidation matrix must be
chosen deliberately rather than falling out of which files happen to contain
which keys.

## Backup, Migration And Recovery

The eventual product must state whether a profile backup is intended to carry
remote-access authority.

- A portable complete authority backup makes recovery simple but also enables
  an offline thief to clone the host.
- A non-exportable host identity prevents silent cloning but cannot migrate by
  ordinary profile copy.
- A restored profile without its protected key must create a new host identity
  and require an explicit replacement flow.
- Existing clients must present an identity-change interruption, not a normal
  password retry or automatic repin.
- A completely new client has no prior pin; its protection depends on the
  selected password-to-host-identity binding.

Potential recovery authorities include local host access, an already trusted
device, a deliberately exported recovery secret or a central account. None is
free. The current direction guarantees new-device login while the original
host and password credential remain intact; it does not yet promise remote
recovery after loss of the host identity key or forgotten passphrase.

## Hosted Browser Integrity

E2E encryption begins only after trusted client code has processed the
passphrase and handshake. A malicious or compromised hosted login application
can send the passphrase, device key or decrypted content to its origin.

Future work must distinguish at least:

- an honest relay operator that can inspect routing metadata but not encrypted
  application records;
- a compromised relay transport that cannot forge transcript-bound endpoint
  authentication;
- a compromised static-client hosting origin that can replace JavaScript; and
- an installed, signed or otherwise independently anchored first-party client.

Strict content policy, no third-party scripts, minimal dependencies,
immutable/versioned assets and reproducible builds reduce risk but do not make
a web origin cryptographically unable to replace its own application. Security
claims must name which operator and code-delivery assumptions they cover.

## Reference Dossier

### Standards

- [RFC 2945](https://www.rfc-editor.org/rfc/rfc2945.html) specifies the original
  SRP authentication and key-exchange system.
- [RFC 5054](https://www.rfc-editor.org/rfc/rfc5054.html) specifies TLS-SRP,
  verifier construction, input validation, group concerns, username exposure,
  verifier-theft server impersonation and rate-limit requirements. Its older
  TLS cipher suites are not an RSTorrent transport recommendation.
- [RFC 9807](https://www.rfc-editor.org/rfc/rfc9807.html) specifies OPAQUE,
  including registration, online AKE, password stretching hooks,
  precomputation resistance, forward secrecy, server-key separation and the
  remaining offline-dictionary boundary after single-server compromise.
- [RFC 9180](https://www.rfc-editor.org/rfc/rfc9180.html) specifies HPKE and is
  relevant background for high-entropy PSK/public-key constructions. HPKE is
  not a password protocol and must not receive a human passphrase as a PSK.

Implementation must re-read the exact current documents, errata, library
documentation and test vectors rather than treat this summary as normative
protocol guidance.

### YepAnywhere

The local `~/code/yepanywhere` sibling was inspected during the 2026-08-04
discussion at commit `cc4732bf8d4ca9d0b4cce1c8eaf13de1ea4f11be`. It is a
product, architecture and failure reference, not an RSTorrent dependency or
wire contract. Relevant current paths include:

- `packages/server/src/crypto/srp-server.ts`: `tssrp6a`, a 2048-bit group and
  SHA-512 produce the stored salt/verifier and server SRP session;
- `packages/client/src/lib/connection/srp-client.ts`: the browser performs the
  corresponding SRP steps and verifies the server proof;
- `packages/client/src/lib/connection/SecureConnection.ts`: SRP/resume output
  is composed beneath the shared inner protocol and above encrypted records;
- `packages/server/src/routes/ws-srp-handlers.ts`: full login, challenge-bound
  resume, transport-key derivation, authentication state and session creation;
- `packages/server/src/remote-access/RemoteSessionService.ts`: retained resume
  session ownership and expiry;
- `packages/shared/src/crypto/srp-types.ts`: handshake and resume messages;
- `docs/project/ws-auth-state-model.md`: trusted-local and SRP-established
  authentication remain distinct; and
- `topics/relay-origin-and-share-gating.md`: owner Remote Access is E2E while
  the current public-share exception is relay-readable.

Lessons adopted as investigation inputs:

- username/passphrase login from an unpaired new browser is convenient and
  avoids central-account recovery dependence;
- password authentication, device identity and resume state are different
  concerns even when one implementation initially combines them;
- encrypted transport must wrap the same inner semantic application protocol
  used by direct connections;
- challenge-bound resume and protocol-floor pinning are security behavior, not
  socket convenience;
- relay routing, authentication, record encryption and application authority
  require separate state owners; and
- persisted symmetric resume material can enlarge profile-clone and client
  impersonation consequences.

RSTorrent does not adopt YepAnywhere's SRP library, NaCl construction,
session-file shape, field names, protocol versions, limits or public-share
exception without separate evidence and an explicit decision.

## Feasibility And Library Research Gates

Before selecting or implementing the owner authentication protocol, one
bounded security tactical must record and satisfy at least these gates.

### Protocol and library matrix

- Identify maintained SRP-6a and OPAQUE implementations for Rust and the
  actual browser delivery target, including Wasm feasibility where relevant.
- Record exact versions, licenses, maintainers, recent activity, audits,
  published security review, standards conformance, errata handling and test
  vector coverage.
- Confirm fixed approved parameters, hostile input validation, constant-time
  behavior, zeroization posture, randomness sources, password byte/string
  normalization and error behavior.
- Measure browser bundle cost, handshake CPU/memory, low-end Android behavior
  and denial-of-service cost before authentication succeeds.
- Reject a design that requires independently reimplementing protocol
  arithmetic or translating key material through an unreviewed custom bridge.

### Platform key matrix

- Determine non-exportable signing or key-agreement support and exact
  algorithms on macOS, Windows, Android and the supported Linux posture.
- Distinguish ordinary secret vaults that return private bytes from services
  that retain the private key and return only an operation result.
- Record background-use, user-presence, application-identity, backup,
  migration, deletion, attestation and failure behavior.
- Prove the platform adapter can expose a narrow handle/capability without
  leaking platform types into the protocol core.
- Define the initial-provisioning fallback and the fail-closed response to
  later capability loss or identity mismatch.

### Adversarial protocol evidence

- Passive relay capture cannot recover passwords, traffic keys or application
  frames.
- Modified, replayed, reordered, truncated, reflected and cross-route
  handshake messages fail without partially authenticated state.
- A blind active relay may forward an honest handshake but cannot terminate it
  invisibly.
- Stolen-verifier, stolen-credential-record, stolen-relay-credential,
  profile-only-clone and full-secret-clone fixtures produce the documented
  distinct outcomes.
- Existing pinned and completely new password-only clients are tested
  separately against a profile clone.
- Host-key removal, tier downgrade, algorithm downgrade and protocol rollback
  fail closed under an established trust record.
- Stolen, expired, replayed, revoked and cross-device resume material cannot
  silently gain owner authority.
- Concurrent original and clone registration/routing behavior is bounded and
  observable rather than last-writer-wins by accident.
- A slow handshake, invalid proof flood or offline host cannot consume
  unbounded relay or host state.
- Direct and relayed connections deliver the same authenticated inner
  application trace after establishment.

### Product and recovery evidence

- New-device passphrase login works without a central account or an existing
  paired device when the original host is healthy.
- Remembering a device creates an independently named and revocable identity.
- Password change, single-device revocation, sign-out-everywhere, host reset,
  profile restore and host replacement have an explicit invalidation matrix.
- Existing clients present a blocking host-identity change rather than
  repinning or falling back silently.
- Logs, diagnostics, crashes and support exports contain no password,
  verifier, private key, traffic key, resume secret or unbounded
  attacker-controlled identity text.
- User-facing claims distinguish relay blindness, hosted-client trust,
  hardware protection and portable fallback honestly.

## Recommended Next Work

Do not add a production remote listener or cryptographic dependency from this
topic alone. After the current authorized product work, create one bounded
owner-remote-authentication research tactical that:

1. refreshes the YepAnywhere dossier and standards/errata review;
2. fixes the exact threat model and answers the new-client/profile-clone
   requirement;
3. builds the protocol and platform-key library matrices;
4. prototypes only enough SRP and OPAQUE handshakes to exercise browser/Rust
   interoperability, hardware-key composition and hostile cases;
5. selects or rejects a construction with explicit reasons;
6. defines owner, host, device, relay and resume persistence plus cancellation
   ownership; and
7. records the separate implementation tactical and evidence gates before any
   production exposure.

Friend sharing, fragment-held capability links, offline encrypted snapshots,
UPnP/NAT traversal, wake-up delivery, public accounts and multi-user
authorization remain separate future topics or tacticals. Local media
byte-range serving and its separate future remote boundary now live in
[`http-file-serving-and-streaming.md`](http-file-serving-and-streaming.md).
Owner authentication should establish the reusable principal and encrypted
connection foundation without pre-solving those products.
