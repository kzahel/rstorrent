# Tactical 111: MSE/PE Peer Stream Encryption

Status: Complete on 2026-08-09. All deterministic, runtime, controlled-
interoperability, performance, client, ABI, Android cross-build, API 34 AVD,
and physical Pixel 7a evidence passes. Direction, settings shape, method
preference, dependency, exponent width, and the performance-evidence direction
were accepted in product discussion on 2026-08-08.

Topics: `protocol-support`, `peer-lifecycle`, `client-persistence`,
`incoming-reachability-and-seeding`, `peer-flag-vocabulary`,
`performance-and-live-evidence`, `application-view-api`, `web-ui-design`,
`client-surfaces`, `capability-readiness`

Dependencies: completed Tacticals
[`078`](078-local-single-peer-tcp-seeding.md),
[`082`](082-bounded-multi-peer-upload-ownership.md), and
[`086`](086-long-lived-torrent-peer-runtime.md) establish the incoming
listener, bounded upload ownership, and one retained peer authority across
download and seed lifetimes. Completed Tacticals
[`084`](084-persisted-client-connection-and-seeding-settings.md),
[`097`](097-live-client-settings-and-replaceable-session-generations.md), and
[`102`](102-ordinary-incoming-listener-settings.md) establish the persisted
settings contract, live convergence, and replaceable session generations.
Completed Tactical [`051`](051-typed-peer-flags-and-legend.md) reserved the
encryption peer flag this slice fills. Completed Tactical
[`093`](093-bep6-fast-request-lifecycle.md) owns the cancel/reject/choke race
evidence that this slice must re-run under encryption. Completed Tactical
[`090`](090-peer-id-duplicate-connection-resolution.md) owns admission
ordering that MSE must not bypass.

## Decision And Motivation

Implement Message Stream Encryption / Protocol Encryption for TCP peer
connections in both directions, behind one user-facing policy setting, and
claim exactly the negotiated subset that recorded evidence supports.

Three concrete forces select this slice now:

1. **Reachability.** Peers configured to require encryption are unreachable
   today. Every such peer in a swarm is invisible to RSTorrent, and the
   failure is silent — it looks like an ordinary connection failure, not a
   capability gap. This is the largest remaining common-denominator peer
   population RSTorrent cannot use.
2. **Correctness under a stock oracle.** Pinned libtorrent defaults to
   `prefer_rc4 = false` with `allowed_enc_level = pe_both`
   (`reference/libtorrent/src/settings_pack.cpp:222, 370-372`). A default
   libtorrent responder therefore selects `0x01`, meaning an obfuscated
   handshake followed by a **plaintext** payload when both methods are offered.
   Under this tactical's accepted `0x03` offer, an implementation that only
   handles `0x02` therefore fails against the stock responder. JSTorrent also
   shipped a responder that always selected `0x02`, breaking initiators that
   only offered plaintext
   (`docs/archive/tasks/2025-12-16-fix-mse-plaintext-select.md`).
3. **Hot-path risk.** RC4 sits on every payload byte of an encrypted
   connection, and an MSE attempt performs two modular exponentiations and two
   request/response flights -- one more network round trip than the ordinary
   BitTorrent handshake. This is the first capability in the engine that can
   regress sustained throughput by construction, so it needs a measured
   performance guardrail rather than an assurance.

MSE is obfuscation, not a security boundary. It uses a 768-bit MODP group with
an unauthenticated key exchange and RC4, and it is trivially defeated by an
active attacker. This tactical must not describe it as securing, protecting,
or privatizing traffic anywhere in the product, the settings surface, or the
readiness matrix. Its honest purpose is interoperating with peers that require
it and avoiding naive protocol-header traffic shaping.

A narrower slice was considered and rejected. Handshake-only support without a
policy leaves no way to reach `pe_forced` peers deliberately; RC4-only support
would either violate the accepted `0x03` offer or lose plaintext-only MSE
peers; outgoing-only support leaves the listener unable to accept the peers
most likely to want encryption. Advertising the capability at all commits
RSTorrent to the full negotiation surface, including hostile inputs on a path
that runs *before* any torrent is identified.

## Stopping Condition

This tactical is complete when all of the following hold:

1. `rstorrent-protocol` owns RC4, MSE key derivation, DH-768 arithmetic, and a
   sans-IO handshake state machine for both roles, with no async runtime,
   socket, clock, or task dependency.
2. Outgoing connections negotiate MSE according to policy, fall back to plain
   BitTorrent only for the bounded downgrade-eligible case, and remember
   per-endpoint outcomes.
3. The incoming listener detects MSE versus plain BitTorrent on the first
   bytes, identifies a target torrent with an expected `O(1)` indexed lookup,
   validates the decrypted BitTorrent handshake before admission, and enforces
   policy.
4. Both the RC4 and plaintext negotiated methods carry a full verified
   download and a full verified upload.
5. One persisted `encryption` client setting converges live through the
   existing settings owner and is visible in the shared web settings surface.
6. The engine projects a typed encryption peer flag derived from the same
   coherent connection observation as the rest of the peer row.
7. Deterministic, scripted-runtime, controlled pinned-libtorrent, paired
   performance, and one physical Android RC4 smoke all satisfy the evidence
   contract and are recorded here.
8. `protocol-support.md`, `capability-readiness.md`, `peer-lifecycle.md`,
   `incoming-reachability-and-seeding.md`, `peer-flag-vocabulary.md`,
   `client-persistence.md`, `performance-and-live-evidence.md`,
   `application-view-api.md`, `web-ui-design.md`, `client-surfaces.md`, and
   `references.md` state the exact claimed subset, provenance, evidence, and
   limits.

## Scope

### Protocol primitives

A new `rstorrent-protocol::mse` module owning:

- RC4 with the mandatory 1024-byte keystream discard on each direction;
- `HASH("keyA"|"keyB", S, SKEY)`, `HASH("req1", S)`, `HASH("req2", SKEY)`, and
  `HASH("req3", S)` derivation;
- DH-768 public-key and shared-secret computation over the fixed MSE prime
  from caller-supplied private-exponent bytes, including
  degenerate-remote-key rejection and exact 96-byte left-zero-padded
  big-endian export; and
- the crypto-method bitfield and its selection policy as pure functions, with
  the negotiated result represented as `MseMethod::{PlaintextPayload, Rc4}`
  rather than leaking raw integers or a misleading boolean.

### Sans-IO handshake state machine

`MseHandshake` for both initiator and responder roles is driven synchronously
by input and resumable actions. `feed(&[u8])` reports how many bytes it consumed
and may yield *need more bytes*, *send these bytes*, *compute this DH public key
or shared secret*, *identify this torrent*, *complete with this negotiated
method and carried-over payload*, or *fail with this typed reason*. The runtime
returns DH and lookup results through an explicit `resume(...)` entry point;
neither operation occurs inside `feed`.

Construction consumes caller-provided, bounded entropy for the exactly 160-bit
private exponent and all random padding choices/bytes. Tests can therefore be
fully deterministic, and the protocol crate gains no operating-system RNG,
async runtime, socket, clock, or task dependency. The state machine owns every
bound in the invariant table below and no allocation grows with peer input
beyond the stated handshake buffer. A concrete `Action`/`resume` ownership
contract and action-sequence tests land before runtime integration; an action
cannot be issued twice or resumed with a result for another state. At most one
action is outstanding. The runtime completes or fails a `Send` before
resuming, and does not call `feed` while a send, DH, or lookup action is
outstanding; already-read bytes remain in the bounded handoff buffer until
that action completes.

### Outgoing integration

`peer_socket.rs::connect_with_progress` gains an MSE phase between TCP connect
and ordinary peer-wire framing; for MSE the 68-byte BitTorrent handshake is
sent inside `IA` rather than through the existing separate handshake write.
`PeerIo` gains an optional RC4 duplex
applied at one ordered frame-commit point on write and in place on each read
chunk before `FrameDecoder`. `PeerRecord`/`PeerHistory` in `peer.rs` gain a
bounded per-endpoint encryption-outcome memory so `Prefer` can retry the other
way without thrashing. A downgrade-eligible MSE failure permits at most one
immediate plain reconnect, using a new socket under the same peer-budget and
generation fences; malformed MSE never triggers that retry.

### Incoming integration

`incoming.rs::run_handshake` keeps its existing `read_exact(HANDSHAKE_LENGTH)`
and, when the bytes are not `\x13BitTorrent protocol`, reinterprets them as the
first 68 bytes of `Ya` and reads the remaining 28. The seed registry gains a
precomputed `HASH("req2", info_hash)` index for expected `O(1)` identification.
That lookup is provisional: the connection is not attached to the torrent or
entered into the peer registry until the decrypted Initial Payload / BitTorrent
handshake carries the same info hash and Tactical `090` duplicate admission
passes. `IncomingPeerIo` owns receive RC4 state while its split
`IncomingWriter` owns send RC4 state, applied in the writer task at the point a
frame is committed.

The index retains all registered info hashes for an obfuscated key, bounded by
`MAX_SEED_REGISTRATIONS`; lookup succeeds only when the bucket contains exactly
one candidate. A synthetic collision is therefore an ambiguous typed failure,
not silent replacement or a per-handshake linear scan. Removing a registration
can make the remaining candidate unique again.

### Policy, settings, and observability

One `encryption: EncryptionPolicy` field on `ClientSettings` with values
`disabled`, `allow`, `prefer`, and `required`, defaulting to `allow`; a schema
14 to 15 durable migration with a checked column; an independent encryption
settings-convergence domain (`DOMAIN_COUNT = 6`,
`SettingsDomain::Encryption`); `effective_encryption` and
`encryption_application` fields on `ClientSettingsRuntimeView`; regenerated
TypeScript, JSON Schema, UniFFI, and Kotlin consumers; and a control labelled
"Protocol obfuscation (MSE/PE)" in
`clients/web/src/inspection/components/ConnectionSeedingSettingsSection.tsx`.
Its helper text is: "Improves compatibility with peers that require MSE/PE.
This is protocol obfuscation, not privacy or security."
Policy is captured when a connection handshake starts: changes affect future
connection generations, while established connections and already-started
handshakes retain their negotiated/captured behavior. No listener or torrent
restart is required.

The v15 migration adds `encryption TEXT NOT NULL DEFAULT 'allow' CHECK
(encryption IN ('disabled', 'allow', 'prefer', 'required'))`; the fresh-table
DDL carries the same constraint. Reads still match the string explicitly and
fail profile open on an unknown durable value rather than trusting only SQLite
or silently defaulting. Existing and fresh profiles both begin at `allow`. The
fields travel in the existing complete-replacement client-settings projection;
they add no view kind, contract-version bump, queue, lease, or task.

The coherent engine observation carries `Option<MseMethod>` rather than an
"encrypted" boolean. Existing `PeerFlagView::Encrypted` is derived for either
MSE-negotiated method (`0x01` or `0x02`), because that flag means encrypted or
obfuscated transport. Its web legend label changes from "Encrypted" to
"Encrypted or obfuscated" so `0x01` does not make the UI overclaim, while
structured events retain the exact method and
typed failure reason. The terminal handshake event also records role, captured
policy, whether a fallback socket was used, pre-framing raw wire bytes sent and
received, and the embedded BitTorrent-protocol byte counts. Their difference
is the MSE-only overhead. Every raw MSE read/write contributes to
`PeerWireSent/Received`; the decrypted 68-byte BitTorrent handshake and
subsequent frames contribute to `PeerProtocolSent/Received`, while MSE-only
overhead does not. Those totals reconcile against the terminal event. The
handshake runtime records each successful raw read/write result, including
partial progress before an error, rather than crediting an entire action only
after `write_all`. The 68-byte `IA` and responder handshake are each credited
to BitTorrent-protocol accounting exactly once despite crossing the MSE
boundary. Secrets, peer public keys, shared secrets, and obfuscated torrent
identifiers are never logged.

### Performance evidence

Paired RC4-versus-ordinary-plain measurement for both RSTorrent and pinned
libtorrent on the existing controlled loopback comparator, plus a
microbenchmark for RC4 and for the modular exponentiation, recorded against
the targets and broad regression guardrail in the performance contract below.
The retained Android bootstrap runner gains a named `product-mse` profile. One
run on the physical Pixel 7a uses the actual Android engine against a
controlled host peer forced to `0x02`, verifies the published payload hash,
exercises five concurrent MSE attempts while observing the four-job DH ceiling
and complete drain, and leaves no APK, host peer, test root, socket, task, or
capture artifact.

## Non-Goals

- uTP, and therefore MSE over uTP. Peer transport remains TCP only.
- Exposing `allowed_enc_level` or `prefer_rc4` as user settings. Both are fixed
  by this slice's policy table.
- Per-torrent encryption policy. The setting is session-wide.
- Any security, privacy, anonymity, or traffic-confidentiality claim.
- A public-swarm reliability claim for MSE connections. Live evidence may
  be recorded as an observation only.
- Encrypting the application gateway, tracker, or DHT traffic. This slice is
  peer-wire only.
- A broad physical-device matrix. The one named Pixel 7a `product-mse` smoke is
  required; ChromeOS ARCVM, Moto X4, storage-provider, lifecycle, and UI
  matrices are not repeated.
- An Android Compose settings control. As with the existing connection/seeding
  group, Android carries the generated typed value and default but has no
  Compose settings screen in this slice.
- Keypair pre-generation pooling, SIMD RC4, or any optimization not required to
  satisfy the broad regression guardrail or explain a missed diagnostic target.

## Normative And Reference Dossier

### Specification provenance

MSE has no BEP. The de facto normative document is the Vuze/Azureus wiki
description of Message Stream Encryption, which is **not** present in the
pinned `reference/bittorrent.org` checkout at revision
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`. The original wiki URL returned
404 during source review. This tactical instead pins the Internet Archive
capture from 2022-03-08 15:52:49 UTC, whose rendered source identifies wiki
revision `oldid=16077`:

<https://web.archive.org/web/20220308155249id_/http://wiki.vuze.com/w/Message_Stream_Encryption>

That immutable capture is the normative source for this slice. The wire
contract below is an independently written description cross-checked against
the pinned libtorrent implementation and the first-party JSTorrent
implementation, so a future pin change can be audited without silently using
a different wiki revision. No specification prose, source, or fixture is
copied. `docs/references.md` gains the capture URL, timestamp, revision, its
non-BEP/non-vendored status, and how it was used.

RC4 primitive tests use selected vectors from IETF RFC 6229, not from
libtorrent or JSTorrent. Before vector bytes enter the repository, the test
fixture cites the RFC section and `docs/references.md` records the IETF Trust
origin. Treat the extracted vector bytes conservatively as RFC Code Components
and preserve the RFC's required Simplified BSD notice in the fixture or
`THIRD_PARTY_NOTICES.md`. Those vectors test RC4 only; they do not substitute
for independently authored MSE derivation and handshake cases.

### Independently written wire contract

`A` initiates, `B` responds. `HASH` is SHA-1 and commas below mean byte
concatenation. `S` is the DH shared secret and `SKEY` is the 20-byte v1 info
hash. DH uses generator 2 and the fixed 768-bit MSE prime from
`reference/libtorrent/src/pe_crypto.cpp`:

```text
P = 0xFFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD1
      29024E088A67CC74020BBEA63B139B22514A08798E3404DD
      EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C2
      45E485B576625E7EC6F44C42E9A63A36210000000000090563
g = 2
```

The line breaks are presentation only. Both `S` and public keys are represented
as exactly 96 bytes, big-endian, **left zero-padded**. `VC` is eight zero
bytes. All multi-byte integers are unsigned and big-endian. The specification
permits private exponents of at least 128 bits and recommends 160 bits;
RSTorrent samples an exactly 160-bit value.

```text
1. A -> B   Ya (96)  || PadA (0..512 random)
2. B -> A   Yb (96)  || PadB (0..512 random)
3. A -> B   HASH("req1", S) (20)
         || HASH("req2", SKEY) xor HASH("req3", S) (20)
         || E( VC (8) || crypto_provide (u32) || len(PadC) (u16)
              || PadC (0..512) || len(IA) (u16) || IA (0..65535) )
4. B -> A   E( VC (8) || crypto_select (u32) || len(PadD) (u16)
              || PadD (0..512) )
5. payload continues under the selected method
```

`E(...)` in step 3 is RC4 keyed `HASH("keyA", S, SKEY)`; in step 4 it is RC4
keyed `HASH("keyB", S, SKEY)`. Each RC4 instance discards its first 1024
keystream bytes before use. `A`'s send stream and `B`'s receive stream share
the keyA instance; the opposite pair shares keyB. Method bit `0x01` means
plaintext after the handshake and `0x02` means RC4 for the remainder of the
stream. Unknown method bits are ignored for forward compatibility. After that
masking, a responder selects exactly one known method from the offer and an
initiator accepts exactly one known method that it offered; unknown-only,
zero, or multi-known-bit results fail.

Five consequences are easy to get wrong and are called out as required
behavior:

- **Steps 3 and 4 are always RC4, including `IA`, regardless of the method
  selected.** `crypto_select` governs only the bytes *after* the handshake.
  Pinned libtorrent decrypts `IA` unconditionally
  (`src/bt_peer_connection.cpp:3252-3282`) and only then decides whether to
  keep the cipher attached.
- **When `0x01` is selected, both RC4 instances are discarded** and any bytes
  already buffered past the handshake are *not* decrypted. Pinned libtorrent
  guards exactly this at `src/bt_peer_connection.cpp:2750-2761`.
- **`B` cannot know where `PadA` ends**, so it scans for `HASH("req1", S)`, and
  `A` scans for the encrypted `VC`. Each scan is bounded to 512 bytes of slack
  past the minimum position. A marker may begin at every offset in `0..=512`;
  failure is reported only after proving it is absent at all 513 positions,
  while retaining the marker-length-minus-one overlap across input chunks.
- **The specification's generic IA field permits `0..=65535`, but this
  BitTorrent implementation deliberately narrows it to `0..=68`.** The
  initiator sends the ordinary 68-byte BitTorrent handshake as `IA`, matching
  pinned libtorrent. A responder accepts every `len(IA)` in `0..=68`; if it is
  shorter, the remaining BitTorrent-handshake bytes arrive under the selected
  post-MSE method. The transition must join those bytes without decrypting
  plaintext a second time or skipping RC4 keystream. Pinned libtorrent sends
  `IA = 68` at `src/bt_peer_connection.cpp:2829-2834` and rejects larger IA at
  `src/bt_peer_connection.cpp:3218-3222`.
- **The responder's PE4 is RC4 even when it selected `0x01`; its subsequent
  68-byte BitTorrent response is not.** Under `0x02`, the same send cipher
  continues into that response. The ordered write handoff must make this
  transition atomically, before any ordinary peer frame can overtake it.

### Pinned libtorrent

Revision `7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected:

| Concern | Path and symbols |
| --- | --- |
| DH-768 prime, keygen, degenerate-key rejection, `req3` xor mask | `src/pe_crypto.cpp:61-133` (`dh_key_exchange`, `export_key`) |
| RC4 init, 1024-byte discard, length-preserving transform | `src/pe_crypto.cpp:300-427` (`rc4_handler`, `rc4_init`, `rc4_encrypt`) |
| Send-barrier model for switching cipher mid-stream | `src/pe_crypto.cpp:135-240` (`encryption_handler`) |
| `keyA`/`keyB` derivation and RC4 pair construction | `src/bt_peer_connection.cpp:90-125` (`init_pe_rc4_handler`) |
| PE1/PE2, PE3, PE4, and the `VC`+cryptofield writer | `src/bt_peer_connection.cpp:539-732` |
| Seven-state receive machine | `src/bt_peer_connection.cpp:2778-3290` |
| Outgoing policy, `pe_support` toggle, `fast_reconnect` | `src/bt_peer_connection.cpp:214-285`, `2741-2775`, `3590-3599` |
| Plain-versus-MSE fallback detection and `in_enc_policy` gate | `src/bt_peer_connection.cpp:3286-3379` |
| Indexed obfuscated-hash torrent lookup | `include/libtorrent/aux_/torrent_list.hpp:113-154`; `src/session_impl.cpp:4672-4679` (`find_encrypted_torrent`) |
| Settings surface and defaults | `include/libtorrent/settings_pack.hpp:1900-1919, 2246-2258`; `src/settings_pack.cpp:222, 370-372` |

Edge cases extracted from that source and adopted here:

- reject a remote public key outside `[2, p-2]`, which otherwise forces the
  shared secret into a tiny subgroup (`src/pe_crypto.cpp:115-133`);
- left-zero-pad short exported keys to exactly 96 bytes
  (`src/pe_crypto.cpp:65-87`), which silently corrupts key derivation when
  missed and occurs whenever `S` or a public key has a leading zero byte;
- reject `len(PadC)`/`len(PadD)` outside `0..=512`
  (`src/bt_peer_connection.cpp:3172-3177`);
- reject `len(IA)` outside `0..=68` (`src/bt_peer_connection.cpp:3218-3222`);
- accept a sync marker beginning at offset 512, but disconnect after proving
  it absent at every offset through 512 in either role
  (`src/bt_peer_connection.cpp:2896-2911`, `3049-3066`);
- reject `crypto_provide & allowed == 0` and a `crypto_select` the initiator
  never offered (`src/bt_peer_connection.cpp:3144-3170`); RSTorrent ignores
  unknown high `crypto_provide` bits while selecting from the known
  intersection, but requires `crypto_select` to contain exactly one known bit
  that it offered; and
- reject a second encrypted handshake inside an encrypted connection, and
  reject MSE fallback on an outgoing connection
  (`src/bt_peer_connection.cpp:3333-3337`).

The pin has more MSE evidence than the first review of this tactical recorded:

- `test/test_pe_crypto.cpp:110-195` repeats DH agreement, exercises the
  degenerate-key boundary (`0`, `1`, `p-1`, and `p` rejected; `2` and `p-2`
  accepted), and checks RC4 round trips;
- `simulation/test_pe_crypto.cpp:70-190` runs disabled; forced plaintext,
  RC4, both-method, and RC4-preferred variants; enabled RC4, both-method, and
  RC4-preferred variants; plus the disabled-versus-forced failure. Its test
  named `enabled_plaintext` actually passes `pe_forced`, so it is not evidence
  for enabled-plus-plaintext despite the name;
- `simulation/test_transfer.cpp:64-71` and
  `simulation/test_metadata_extension.cpp:56-58, 87-88, 208-215` cover
  encrypted TCP transfer and encrypted metadata exchange; and
- `test/swarm_suite.cpp:106-107` and `test/test_auto_unchoke.cpp:85-86` force
  MSE in ordinary integration tests.

There is still no deterministic sans-IO handshake-parser suite at the pin for
fragmentation, every length boundary, malformed method fields, or carried
payload. This slice independently authors that missing evidence. The simulator
tests were inspected as source but are not imported or linked: their
`libsimulator` dependency is a GPL-licensed submodule outside RSTorrent's test
harness, as already recorded in `docs/references.md`.

The pinned Python bindings expose both the configuration and the observation
needed for controlled interoperability: `lt.enc_policy`, `lt.enc_level`, and
`prefer_rc4` (`bindings/python/src/session_settings.cpp:56-133`), and
`peer_info.rc4_encrypted` / `peer_info.plaintext_encrypted`
(`bindings/python/src/peer_info.cpp:138-139`). The oracle can therefore confirm
which method was negotiated rather than leaving it inferred.

### rqbit

Revision `4e5f94cbcf1d57ec500885c77cf1e24d70232d89` contains no MSE
implementation. Pinned libtorrent is the sole strong oracle for this
capability, which raises the required depth of independently authored tests.

### JSTorrent

The first-party checkout at revision
`9895410beeed6aff554053769bd006a3fbd373ef` was inspected for product behavior
and known failures; the referenced paths had no local modifications:

- `packages/engine/src/config/config-schema.ts:273-280` and
  `packages/client/src/components/SettingsOverlay.tsx:1067-1091` define the
  single `encryptionPolicy` enum (`disabled`/`allow`/`prefer`/`required`,
  default `allow`) presented as "Protocol encryption (MSE/PE)". This slice
  adopts that shape and default.
- `packages/engine/src/core/connection-manager.ts:203-236` and
  `packages/engine/src/core/bt-engine.ts:706-737` show the policy applied
  asymmetrically: `allow` accepts incoming MSE but never initiates it. This
  slice adopts that asymmetry.
- `docs/archive/tasks/2025-12-16-fix-mse-plaintext-select.md` records shipping
  a responder that always answered `crypto_select = RC4`, breaking plaintext
  negotiation. The policy table and a dedicated test cover this.
- `docs/archive/investigations/mse-handshake-race-condition.md` records a
  re-entrancy defect where unawaited async work let a second `onData` re-enter
  the state machine on a half-processed buffer. A sans-IO state machine with a
  single synchronous `feed` entry point makes that class of defect
  unrepresentable, which is the primary reason for that boundary.
- `docs/archive/plans/mse-sha1-optimization.md` records `O(N)` hashing per
  incoming connection and the fix: precompute `HASH("req2", info_hash)` per
  torrent, compute `HASH("req3", S)` once per connection, and look up. This
  slice implements the fixed form directly.
- `packages/engine/test/crypto/mse-socket-close.test.ts:87-146` preserves two
  close-event ordering bugs in the wrapper approach. Runtime tests here cover
  close and half-close before callback/owner handoff so MSE cannot lose a
  terminal socket event.
- `packages/engine/src/crypto/dh.ts` accepts arbitrary remote public keys and
  uses a 768-bit random exponent. RSTorrent deliberately does neither: it
  validates `[2, p-2]` before shared-secret work and uses the accepted bounded
  160-bit exponent.
- `packages/engine/test/crypto/mse-handshake.test.ts` covers the common path,
  plaintext detection, unknown torrent, timeout, and cancellation, but not the
  adversarial length, fragmentation, method-selection, or carried-byte matrix
  required below. Those gaps inform this tactical's independently authored
  cases.

No JSTorrent source, fixture, or test data is imported.

## Owner, Task, And Data-Flow Map

```text
                    ClientSettings.encryption (persisted)
                                  |
                    settings convergence / session generation
                                  |
                +-----------------+------------------+
                v                                    v
     outgoing dial policy                   incoming admission policy
                |                                    |
     peer_socket::connect                incoming::run_handshake
                |                                    |
                +----------------+-------------------+
                                 v
              rstorrent-protocol::mse::MseHandshake  (pure, sans-IO)
             feed/resume -> Action, no socket/RNG/clock/task
                                 |
        +---------------------+---------------------+
        v                     v                     v
 tracked/bounded DH     req2 index lookup    negotiated MseMethod
 blocking owner          (provisional)       + optional Rc4Duplex
                              |                     |
                       validate BT handshake       PeerIo /
                       + duplicate admission       IncomingPeerIo/Writer
                                                    |
                                      existing FrameDecoder, byte metrics,
                                      watermarks, cancellation, and joins
```

Ownership rules this slice must not violate:

- The state machine is pure. It never reads a socket, consults a clock, spawns
  a task, obtains entropy, or allocates proportionally to peer input beyond its
  fixed buffer. Dependency direction points inward: `rstorrent-engine` depends
  on `rstorrent-protocol::mse`, never the reverse.
- The runtime obtains private-exponent and padding entropy from the existing OS
  randomness boundary. The private exponent is sampled uniformly from
  `[2^159, 2^160-1]` by consuming exactly 20 uniform bytes and setting the high
  bit. Every result therefore has the specification's recommended 160-bit
  width without unbounded rejection sampling. Tests inject deterministic
  entropy and assert the transform.
- Each MSE attempt's public-key and shared-secret exponentiations run through a
  session-scoped `DhWorkOwner`, never on a reactor thread. It combines a
  four-permit semaphore (`MAX_MSE_DH_JOBS = 4`) with
  `tokio_util::task::TaskTracker::spawn_blocking`. The pending-connection limit
  is the outer bound. An owned permit moves into the blocking closure so
  cancelling a connection cannot release capacity while an uninterruptible
  exponentiation is still executing.
- `SessionNetwork` constructs this owner beside the existing shared
  `PeerBudget` and injects the same clone into incoming service configuration
  and every torrent's outgoing peer runtime. Standalone engine/test entry
  points construct a scoped owner explicitly and must close/wait it with their
  existing shutdown owner; no process-global singleton is introduced.
- Tokio blocking work cannot be cancelled after it starts. A connection may
  discard its result on cancellation, but the task tracker continues to own
  its termination observation even if the returned join handle is dropped;
  incoming `JoinSet::abort_all()` therefore cannot orphan it. Shutdown first
  closes new connection/DH admission, then closes and awaits the tracker before
  reporting terminal state. Tests use a barrier-controlled test operation in
  the same owner to prove cancellation, drain, and the four-job concurrency
  high-water without timing races; this does not require a production executor
  trait.
- MSE adds **no new long-lived task**. It runs inside the existing dial task
  and the existing incoming handshake task, under their existing deadlines and
  cancellation tokens. The blocking-work gate/tracker is task-free durable
  state; its short-lived jobs are nevertheless included in task accounting.
- The existing pending-connection and peer-budget bounds are enforced *before*
  any DH work begins. Tactical `078` already recorded that pinned libtorrent
  admits before this bound; RSTorrent keeps its stricter ordering, so an
  unauthenticated peer cannot make the process do modular arithmetic before
  passing admission.
- The peer registry remains the only merger of endpoint knowledge. Encryption
  outcome is a bounded field on the existing record, not a parallel store.
- Post-handshake RC4 is length-preserving, so peer-wire frame lengths, payload
  metrics, send watermarks, and partial-write bookkeeping are structurally
  unchanged. Total connection wire bytes are *not* identical: MSE key exchange
  and padding add bounded overhead. Tests assert identical payload and
  BitTorrent-protocol accounting, then reconcile the exact raw-wire delta with
  the event's raw and embedded-protocol byte fields.

### Cipher application points

The two IO implementations have different commit semantics and must be handled
differently. This is the highest-risk part of the slice.

`PeerIo` (`crates/rstorrent-engine/src/peer_io.rs`) uses `try_write` over
`queued_frames`, but `send_message` bypasses that queue with `write_all`. The
direct path can already overtake queued bytes and would make RC4 ordering
unrecoverable. Gate 3 first unifies all outgoing peer-wire frames behind one
ordered queue/commit path and audits every direct `TcpStream` write and handoff
after cipher attachment. A frame is encrypted exactly once at a point after
which it cannot be discarded or reordered. `prepend_messages` only reorders
already-decoded **inbound** messages and is not a send-keystream hazard; its
existing delivery-order behavior remains covered but is not rewritten for MSE.

`IncomingPeerIo` (`crates/rstorrent-engine/src/incoming/peer_io.rs`) splits the
stream and runs a separate writer task. Receive RC4 state therefore remains
with the reader while send RC4 state moves into `IncomingWriter`; a duplex
stored only on `IncomingPeerIo` would be owned by the wrong task. A queued frame
can currently be invalidated before its first write. Under RC4,
`FrameValidity` is resolved *before* encryption; after the cipher advances, the
frame is committed to completion without another validity race. Connection
cancellation may still close the stream mid-frame because the connection is
then terminal and no future keystream must remain synchronized.

The consequence is recorded as an intentional behavior change: on an RC4
connection, an upload frame already picked up by the writer can no longer be
cancelled mid-flight. The exposure is bounded by one frame, at most 16 KiB plus
framing. Pinned libtorrent has the same property, since a message committed to
its send buffer is never unsent. Ordinary plain and `PlaintextPayload`
connections keep today's stronger behavior unchanged. Tactical `093`'s
reject/refill evidence is re-run in both post-handshake methods so the
difference is measured rather than assumed.

Reads decrypt in place on each freshly read chunk before `FrameDecoder`.
Bytes already read beyond MSE completion are handed to the decoder exactly
once: untouched under `0x01`, decrypted with the continuing receive stream
under `0x02`. The same rule applies when a partial `IA` is followed by the rest
of the BitTorrent handshake under the selected method.

## Policy Table

One session-wide setting maps onto the negotiation as follows. `Offer` is the
`crypto_provide` bitfield when RSTorrent initiates MSE; `Select` is its
responder preference.

| Setting | Outgoing | Incoming | Offer | Select |
| --- | --- | --- | --- | --- |
| `disabled` | Plain only | Plain only; an MSE handshake is refused | none | none |
| `allow` (default) | Plain only | Accept MSE or plain | not used | Prefer `0x02` |
| `prefer` | Try MSE, one plain fallback only after an eligible pre-response failure | Accept MSE or plain | `0x03` | Prefer `0x02` |
| `required` | MSE only, no fallback | MSE only; plain is refused | `0x03` | Prefer `0x02` |

Accepted decisions this table encodes:

- **RC4 is preferred when we select and both methods are offered.** This
  differs from stock libtorrent, whose `prefer_rc4 = false` default selects
  plaintext, and matches JSTorrent and libtorrent configured with
  `prefer_rc4 = true`. A peer's valid plaintext selection is still accepted,
  so this preference costs no interoperability.
- **Whenever RSTorrent initiates MSE, `0x03` is provided.** Restricting the
  offer gains nothing and loses peers. Consequently, `required` means
  "require an MSE/PE handshake", not
  "require RC4 payload bytes"; a remote responder may select `0x01`. The UI
  says protocol obfuscation rather than promising payload encryption.
- **`allow` never initiates.** It is a compatibility posture, not a preference,
  and initiating costs two MSE flights and two modular exponentiations per
  attempt.

Under `prefer`, endpoint memory is a bounded typed state, not a boolean:
`Unknown`, `MseCapable`, or `PlainPreferred`. `Unknown` and `MseCapable` start
MSE; `PlainPreferred` starts plain. A TCP-connect failure proves nothing about
MSE and does not update this state. Successful MSE negotiation records
`MseCapable` regardless of whether `0x01` or `0x02` was selected. An ordinary
plain connection made under `disabled` or `allow` does not prove MSE is absent
and therefore does not update this memory.

Only close, reset, EOF, or timeout before a complete remote DH public key in
`[2, p-2]` is received is a downgrade-eligible failure. It records
`PlainPreferred` and permits one
immediate reconnect on a fresh socket; the reconnect remains under the ordinary
connection-attempt budget and inside the same `DialAttemptId`, worker,
peer-budget permit, captured policy, and settings/torrent generation. Each
socket retains the existing connect and peer-IO operation deadlines, so the
two-socket sequence has a calculable maximum rather than an unbounded shared
timer. Invalid DH keys, bad `VC`, invalid method fields, length violations,
or any failure after the peer demonstrated MSE are protocol errors and never
trigger a plain retry. There is no third attempt. This preserves the intent of
libtorrent's `pe_support`/`fast_reconnect` behavior
(`src/bt_peer_connection.cpp:244-270`) while giving the retry an explicit bound
and owner. If that plain fallback, or a later plain attempt from
`PlainPreferred`, reaches TCP but fails the BitTorrent handshake, the memory
returns to `Unknown` for the next scheduler attempt; it does not add a third
socket to the current sequence. The memory is volatile and cleared with the
torrent generation. `disabled`, `allow`, and `required` do not consult it when
choosing their outgoing mode.

## Resource And Security Invariants

| Resource | Bound |
| --- | --- |
| `Ya` / `Yb` | Exactly 96 bytes each |
| Local `PadA` / `PadB`; remote implicit pads | Generated in `0..=512`; remote excess fails the bounded sync search |
| Declared `PadC` / `PadD` | `0..=512` bytes, length rejected before buffering outside that range |
| `IA` | Initiator emits 68; responder accepts `0..=68` and rejects larger before buffering |
| Sync scan start offsets, either role | Every offset in `0..=512`, then typed failure |
| Responder bytes before candidate torrent lookup | 648 (`96 + 512 + 20 + 20`) |
| Responder bytes before candidate `VC` validation | 656 (the preceding 648 + encrypted `VC` 8) |
| Maximum initiator-to-responder bytes through IA | 1,244 |
| Maximum responder-to-initiator bytes through PE4 | 1,134 |
| Per-connection handshake buffer | One fixed 2 KiB buffer, released at completion |
| Steady-state per `MseMethod::Rc4` connection | Two inline RC4 states: 1,056 bytes on the supported 64-bit targets after the measured throughput optimization, and always bounded by the 4 KiB differential guardrail; plaintext-payload MSE retains neither |
| Incoming `req2` index | At most one 20-byte candidate entry per `MAX_SEED_REGISTRATIONS` registration, bucketed by a 20-byte key |
| DH private exponent | Uniform integer in `[2^159, 2^160-1]` from OS entropy |
| Per-socket handshake deadline | The existing per-direction peer handshake deadline; MSE does not extend it. The one fallback socket gets its own existing connect/handshake operations |
| Modular exponentiations per completed MSE attempt | Exactly two; a rejected/stalled attempt may complete zero or one |
| Concurrent DH jobs | At most `MAX_MSE_DH_JOBS = 4`, and never more than admitted pending connections |

Security and hostile-input rules:

- Every peer-controlled length bound above is enforced before the associated
  bytes are buffered or any torrent-visible state changes. A failed handshake
  leaves no admitted peer record or registration effect. Endpoint retry memory
  may change only for the downgrade-eligible outgoing failures defined above.
- Admission and peer-budget limits are checked before DH work, so handshake
  cost is bounded by admitted connections rather than by arriving sockets.
- A remote public key outside `[2, p-2]` is rejected before deriving anything.
- The obfuscated-hash lookup is expected `O(1)` in the number of registered
  torrents and never linearly hashes/scans them. It is not claimed to be
  cryptographic constant-time, and MSE/plain failure timing is not claimed to
  be indistinguishable.
- Colliding `HASH("req2", info_hash)` registrations share one bounded index
  bucket. An ambiguous bucket fails MSE closed without preventing ordinary
  plain admission; registration removal restores uniqueness deterministically.
- A `req2` match identifies only a candidate `SKEY`. Before attaching the
  connection, RSTorrent validates the decrypted BitTorrent handshake's protocol
  string, matching info hash, peer ID, and Tactical `090` duplicate-admission
  result. A mismatch closes without creating or replacing a peer record.
- Failure reasons are typed and logged, but the wire behavior for every
  handshake failure is a plain close. RSTorrent does not send a distinguishing
  error, which would help an observer classify the endpoint.
- The 160-bit private exponent is an intentional, recorded difference from
  pinned libtorrent's 768-bit random exponent and follows the normative
  document's recommendation. Interoperability is unaffected because exponent
  width is a purely local choice. This rationale is not a modern security claim
  for the legacy group. Constant-time modular APIs are used for
  private-exponent operations; `_vartime` variants are forbidden on that path.
- Private exponents, shared secrets, key material, public-key byte dumps, and
  obfuscated torrent identifiers never appear in logs or application views.
- No claim of confidentiality, integrity, or authentication is made anywhere.
  The settings surface describes the feature in terms of peer compatibility.

## Intentional Differences From The Specification And Oracle

| Behavior | Normative specification / pinned libtorrent | RSTorrent | Why |
| --- | --- | --- | --- |
| Private exponent | At least 128 bits, 160 recommended / 768-bit random | Exactly 160 bits | Follows the recommendation and avoids excess legacy-group work |
| IA accepted by the BitTorrent responder | Generic maximum 65,535 / maximum 68 | Maximum 68 | A BitTorrent initiator cannot safely append beyond its 68-byte handshake before the responder replies; matches the oracle's bounded subset |
| `PadC` / `PadD` padding bytes | Zero recommended for current-version padding / random | Random | Matches the oracle; the bytes are encrypted and semantically opaque in this version |
| Method preference when selecting | Unspecified / plaintext by default (`prefer_rc4 = false`) | RC4 | Product prefers payload obfuscation while still accepting the peer's valid plaintext selection |
| User-visible knobs | Not specified / four (`in`/`out` policy, level, `prefer_rc4`) | One four-value policy | Adopts JSTorrent's accepted product shape; the rest are fixed by the policy table |
| Admission versus DH ordering | Not specified / admits before the pending bound | Enforces the bound first | Keeps unauthenticated work bounded, per Tactical `078` |
| Mid-flight upload cancel | Not specified / not possible once buffered | Possible on ordinary/`PlaintextPayload`, not on RC4 | Required for keystream integrity; bounded to one frame |
| Method-field strictness | Ignore unknown bits and select one method / accepts some ambiguous known selections | Ignores unknown bits but requires exactly one known, offered selected bit | Makes the negotiated stream mode unambiguous while retaining extension compatibility |
| Retry memory | Reconnect guidance only / `pe_support` coupled to `fast_reconnect` | Typed endpoint outcome plus at most one owned immediate retry | Preserves prompt compatibility fallback with an explicit bound and failure classification |

## New Dependency

`crypto-bigint = "=0.7.5"` is added to `rstorrent-protocol` with default
features disabled for fixed-width 768-bit modular exponentiation. The reviewed
release supports Rust 1.85 (below this workspace's 1.97 baseline), is
`Apache-2.0 OR MIT`, exposes stack-allocated `U768` and constant-modulus
Montgomery arithmetic, and provides bounded-exponent constant-time APIs.

Rationale: hand-rolling Montgomery arithmetic is numeric code whose failure
mode is a silent interoperability or key-agreement defect, on a path with no
natural self-check. `ConstMontyForm::pow_bounded_exp(..., 160)` (or the exact
equivalent in the pinned API) is used; no variable-time exponentiation or crate
RNG feature is enabled. `THIRD_PARTY_NOTICES.md` and the dependency audit are
updated in the same gate. `rstorrent-android` is cross-built for both supported
Android ABIs to confirm portability. No other crate gains the dependency --
RC4, SHA-1 key derivation, and the state machine are first-party.

## Implementation Gates

Each gate is independently committable and leaves the workspace green.

1. **Primitives.** `rstorrent-protocol::mse` with RC4, key derivation, and
   DH-768 using the pinned dependency. Proven against independently transcribed
   RFC 6229 RC4 vectors with an adjacent origin citation, the 1024-byte
   discard, exact MSE derivation vectors, private-exponent bounds,
   degenerate-key rejection, 96-byte padded export including short-secret
   cases, and round-trip agreement over many deterministic pairs. The prime is
   checked byte-for-byte against the pinned oracle. Run the single-stream and
   production-concurrency RC4 benchmarks plus the DH benchmark here, before
   either primitive establishes an IO architecture. A material target miss
   triggers profiling here rather than after the IO architecture lands; only
   the broad graduation guardrails below are stops.
2. **State machine.** Freeze the `Action`/`resume` contract, then implement both
   roles, every bound, every typed failure, and caller-supplied entropy. Prove
   it under byte-at-a-time, single-shot, exhaustive state-boundary truncation,
   and seeded random chunk splits with the full hostile-input matrix.
3. **Outgoing and blocking ownership.** Add the bounded/tracked DH work gate,
   dial integration, the one-retry `Prefer` transition, and `PeerIo` cipher.
   Remove the direct-write ordering split so every outgoing frame has one
   ordered commit point. Prove a verified download in both negotiated methods,
   downgrade classification, cancellation/drain, and exact task joins.
4. **Incoming.** Detection, provisional `req2` index, decrypted-handshake and
   duplicate validation, and split reader/writer cipher ownership with the
   validate-before-encrypt rule. Prove a verified upload and re-run Tactical
   `093`'s reject/refill evidence in both methods.
5. **Policy, settings, and observability.** The `encryption` setting, live
   convergence domain, schema-15 migration, generated contracts/consumers, web
   control, derived peer flag, exact negotiated-method observation, and
   structured log events. Prove the full four-policy matrix, rapid replacement,
   replay/no-op/rollback, migration, ephemeral, and reopen cases.
6. **Evidence and claims.** Controlled pinned-libtorrent matrix, performance
   targets and guardrails, the physical Pixel 7a `product-mse` smoke, and
   updates to `protocol-support.md`,
   `capability-readiness.md`, `peer-lifecycle.md`,
   `incoming-reachability-and-seeding.md`, `peer-flag-vocabulary.md`,
   `client-persistence.md`, `performance-and-live-evidence.md`,
   `application-view-api.md`, `web-ui-design.md`, `client-surfaces.md`, and
   `references.md`.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Primitives | RFC 6229 RC4 vectors; 1024-byte discard; RC4 length/chunk invariance; `keyA`/`keyB`/`req1`/`req2`/`req3` derivation vectors; DH round-trip agreement; rejection of remote keys `0`, `1`, `p-1`, `p`, `p+1`, and all-`0xff`, with `2` and `p-2` accepted; 96-byte export for values with 1, 2, and 8 leading zero bytes; fixed 20-byte private-exponent entropy consumption, high-bit transform, `2^159` and `2^160-1` boundaries, and no `_vartime` call on the secret path |
| State machine, common | Both roles reach completion for `0x01` and `0x02`; initiator emits `IA = 68`; responder accepts every `IA` length `0..=68`, including boundaries that split the protocol string, info hash, and peer ID; zero-length and 512-byte pads at every stage; carried-over bytes are delivered exactly once, decrypted for RC4 and untouched for plaintext |
| State machine, framing | Byte-at-a-time, single-shot, exhaustive truncation at every wire byte, and reproducible seeded chunk splits; each sync marker start offset `0..=512`, including a chunk-boundary straddle; handshake coalesced with the first payload frame; invalid action resumption and repeated action consumption rejected |
| State machine, hostile | Sync absent at every permitted start offset and failure only after offset 512 is excluded; `len(pad)` of 513 and `0xFFFF`; `len(IA)` of 69; `crypto_provide` of `0`, unknown-only, and known bits plus unknown high bits; `crypto_select` of `0`, both known bits, unknown-only bits, known plus unknown bits, and a method never offered, proving unknown bits are ignored only when exactly one valid known selection remains; wrong `VC`; candidate `SKEY` followed by a mismatched plaintext-handshake info hash; synthetic ambiguous `req2` bucket before and after one registration is removed |
| Policy | Four settings x two directions x {peer MSE-only, peer plain-only, peer both} expectation table, asserted as pure transitions and as scripted runtime behavior; `required` accepts MSE with either `0x01` or `0x02`; downgrade only before a complete valid remote DH key; ordinary-policy plain success does not poison `Prefer` memory; failed remembered-plain handshake resets to `Unknown`; no retry for TCP-connect or malformed-MSE failures; never more than one immediate retry |
| Runtime | Verified download and upload on loopback in both methods with exact payload hashes; handshake timeout; junk flood; `Ya`-then-stall; close/half-close before and during owner handoff at every state; controlled cancellation during admitted DH with the permit retained until work ends and every job joined; a second MSE or BitTorrent handshake on an established stream closes; `Prefer` fallback/memory; `required` refusing ordinary plain in both directions |
| Settings/persistence | Fresh and migrated profiles default `allow`; schema-15 checked-value migration; malformed durable value fails profile open; exact no-op, request replay, transaction rollback, ephemeral, and reopen behavior; independent convergence state; `A -> B -> A` stale-generation fencing; in-flight handshake retains captured policy while the next generation observes the replacement; no listener/torrent restart |
| Regression | Payload and post-handshake protocol accounting, frame lengths, send watermarks, and partial-write bookkeeping identical between plain and RC4 transfers of the same content; pre-framing raw-wire and embedded-protocol event fields reconcile independently, and their MSE-only overhead explains the total-wire delta from plain; Tactical `093` reject/refill evidence in both methods |
| Resource | Handshake-buffer high-water at most 2 KiB per connection; steady-state duplex cipher state at most 4 KiB; a deterministic barrier test observes exactly four DH jobs running and the fifth waiting while never exceeding admitted pending connections; the product smoke exercises five real attempts and observes a high-water in `1..=4` because sub-millisecond jobs need not overlap; zero/one/two exponentiations match the terminal handshake state; cancelled jobs drain; terminal zero connections, tasks, jobs, permits, and sockets |
| Controlled interoperability | The explicit matrix below in both initiator directions, with `peer_info.rc4_encrypted` / `plaintext_encrypted` asserted on the oracle and exact content hashes on both; method-forcing cases in each direction; scripted capture proving the exact known 68-byte BitTorrent handshake is absent in both directions under `0x02`, while under `0x01` the initiator's `IA` remains concealed and the responder's post-PE4 handshake is plaintext as specified |
| Performance | The paired targets and broad regression guardrail below |
| Client | Component tests cover the labelled four-option "Protocol obfuscation (MSE/PE)" control, non-security helper text, draft refresh/save semantics, and keyboard operation; persistence, live convergence, and restart pass; the `E` legend says "Encrypted or obfuscated"; `PeerFlagView::Encrypted` appears for both MSE methods and not for ordinary plain; the exact method remains observable in engine diagnostics; generated web/UniFFI/Kotlin consumers, web tests, typecheck, production build, and both Android ABI cross-builds pass |
| Physical Android | The existing bootstrap runner's named `product-mse` profile passes once on the explicitly selected Pixel 7a: controlled peer forces RC4, the actual Android engine publishes the exact verified payload, five concurrent attempts observe no more than four DH jobs and drain to zero, and device/host cleanup is exact |

The controlled interoperability matrix records expected connection sequences,
not just final success:

| RSTorrent initiates | libtorrent `pe_disabled` | libtorrent `pe_enabled` | libtorrent `pe_forced` |
| --- | --- | --- | --- |
| `disabled` / `allow` | Plain succeeds | Plain succeeds | Plain is rejected; terminal failure |
| `prefer` | MSE is refused, then the one plain reconnect succeeds | MSE succeeds | MSE succeeds |
| `required` | MSE is refused; terminal failure | MSE succeeds | MSE succeeds |

| libtorrent initiates | RSTorrent `disabled` | RSTorrent `allow` / `prefer` | RSTorrent `required` |
| --- | --- | --- | --- |
| `pe_disabled` | Plain succeeds | Plain succeeds | Plain is rejected |
| `pe_forced` | MSE is rejected | MSE succeeds | MSE succeeds |
| fresh `pe_enabled` through `torrent_handle::connect_peer` | Initial MSE is rejected; libtorrent's one plaintext retry succeeds | Initial MSE succeeds | Initial MSE succeeds |

The manually connected oracle peer begins MSE-capable because
`torrent_handle::connect_peer` defaults its PEX flags to `pex_encryption`
(`include/libtorrent/torrent_handle.hpp:1260`) and `peer_list.cpp:1049` maps
that flag to `torrent_peer::pe_support`. This overrides the constructor's
ordinary no-evidence default of `false` (`torrent_peer.cpp:174`). The
`pe_enabled`/RSTorrent-`disabled` case therefore observes two sockets: an
initial MSE refusal followed by libtorrent's successful plaintext retry. The
other incoming `pe_enabled` cases complete on the first MSE socket. Treating
the policy as a static one-attempt choice, or assuming a manually connected
fresh peer starts plain, would miss the pinned oracle's actual behavior.
Method selection is forced separately:
with RSTorrent initiating and offering `0x03`, libtorrent `pe_both` selects
`0x01` when `prefer_rc4 = false` and `0x02` when true. With libtorrent
initiating under `pe_forced`, `pe_plaintext` makes RSTorrent select `0x01`,
while `pe_both` makes RSTorrent select `0x02`. The oracle flag and RSTorrent's
typed method must agree in every successful MSE case.

The controlled interoperability harness requires no public swarm, physical
device, or destructive action. Live public-swarm behavior may be recorded as
an observation only. The separate retained Android gate supplies the physical
product evidence recorded below.

The final command gate, in addition to focused tests and the two new interop
harness modes, is:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace --no-fail-fast
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
npm run build --prefix clients/web
cargo check -p rstorrent-session --features uniffi
cargo ndk -t x86_64 -t arm64-v8a -P 28 \
  check -p rstorrent-android --lib
experiments/android-engine-bootstrap/build.sh
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target pixel7a --profile product-mse --runs 1
git diff --check
```

The controlled handshake/policy harness lives at
`tests/interop/mse_peer_encryption.py`. The existing
`tests/interop/local_throughput_compare.py` gains an explicit MSE method and
the paired methodology below. Both use the pinned Python-libtorrent environment
through `uv run --project tests/interop --locked python <script>`; neither
launches a visible product client.

## Performance Contract

MSE payload obfuscation is the first engine capability that can regress
sustained throughput by construction, so this slice carries measured targets
and a broad catastrophe guardrail rather than a guessed low-single-digit stop.
Consistent with
[`performance-and-live-evidence.md`](../topics/performance-and-live-evidence.md),
these are same-session paired measurements on one machine, not permanent CI
thresholds.

| Measurement | Method | Target or guardrail |
| --- | --- | --- |
| RC4 single-core throughput | Release microbenchmark over 64 MiB aggregate, once contiguous and once in production-shaped 16 KiB chunks; output is checksummed to prevent elision | Diagnostic target: at least 1 GiB/s in both shapes; a miss triggers profiling but does not override the paired result |
| RC4 production-concurrency throughput | Four independent RC4 states processing 16 KiB chunks on the same executor/thread shape as the retained `4/4` transfer profile; aggregate rate and CPU utilization recorded | Diagnostic prerequisite: reconcile it with the retained plain rate and paired result |
| DH-768 modular exponentiation, 160-bit exponent | Warmed release microbenchmark of generator-base public-key work and valid random remote-base shared-secret work, at least 100 samples of each on the recorded host; median and p95 reported | Diagnostic target: worse median at most 2 ms; a miss triggers profiling and Android comparison |
| Verified-publication wall clock, RC4 versus plain | `tests/interop/local_throughput_compare.py` extended with an encryption mode; at least six same-cohort pairs for RSTorrent and pinned libtorrent with identical payload/profile and alternating plain-first/RC4-first and implementation order | RSTorrent target at most 10% median paired regression; a result over 10% requires a recorded profile, explanation, and comparison with the oracle's own penalty; below 75% of plain throughput blocks graduation pending optimization or an explicit product decision |
| Local connection setup CPU/scheduling, MSE versus plain | Same-host scripted harness, TCP connected to validated remote BitTorrent handshake, with fixed deterministic pad lengths for comparability | Diagnostic target: median added latency at most 25 ms; a miss triggers profiling and is recorded |
| Network-flight shape | Scripted transport with a fixed one-way delay, comparing ordinary and MSE setup after removing measured local work | MSE shows exactly one additional network round trip within timer tolerance |
| Steady-state memory per RC4 connection | Existing resource high-water instrumentation | At most 4 KiB above the plain baseline |

The microbenchmarks and paired transfer result are independent: a fast isolated
RC4 loop does not waive a large transfer regression, and missing a diagnostic
target is not by itself a stop. Record CPU model, OS, Rust version, build
profile, sample count, raw per-run results, CPU utilization, and median
calculation in the execution record. The throughput result is the median of
within-pair RC4/plain wall-clock ratios, not a ratio of two independently
pooled medians. Graduation stops only when RC4 retains less than 75% of paired
plain throughput, the network-flight or memory bound fails, or profiling
exposes an avoidable hot-path defect that remains unaddressed. An explicit
product decision may revise the broad throughput floor after the measured
cost and compatibility benefit are recorded.

## Deferred With Reason

- **uTP and MSE over uTP.** Blocked on a uTP transport owner that does not
  exist. The sans-IO state machine boundary is chosen partly so that a future
  uTP slice can reuse it without change.
- **Keypair pre-generation pooling.** Only worthwhile if the measured
  modular-exponentiation cost materially misses its target. Deferred until
  measurement says otherwise.
- **Per-torrent encryption policy.** No current product surface asks for it.
- **Exposing method preference to the user.** Fixed by the policy table until
  evidence shows a real peer population that needs plaintext-preferred.
- **BEP 40 canonical peer priority.** Related only in that both harden peer
  selection; independently gated, as recorded in Tactical `094`.

## Escalation And Next Boundary

Stop and ask for direction if any of the following occurs:

- controlled interoperability shows a common client rejecting a handshake that
  the pinned oracle accepts, which would imply the wire contract recorded here
  is wrong rather than the implementation;
- bringing a result below the broad 75%-of-plain floor back within the
  guardrail would require changing the framing, buffering, or upload-ownership
  architecture rather than optimizing the cipher path;
- the mid-flight cancel difference measurably degrades upload responsiveness in
  Tactical `093`'s scenarios rather than merely changing the commit point; or
- the settings shape needs to grow past one enum to express a real peer
  population's requirements.

## Execution Record

### 2026-08-09: Gate 1 primitives

Implemented `rstorrent-protocol::mse` with:

- typed MSE roles and negotiated methods, including extension-bit handling and
  strict single-known-method selection;
- independently authored RC4 with the mandatory 1,024-byte discard, chunk
  invariance, directional cipher ownership, and redacted state;
- exact-width 160-bit private exponents from fixed caller entropy, constant-time
  DH-768 public/shared exponentiation, degenerate remote-key rejection, and
  96-byte big-endian export; and
- allocation-free SHA-1 request, obfuscated-SKEY, and keyA/keyB derivation.

Added exact `crypto-bigint = 0.7.5` resolution with default features disabled,
updated the protocol architecture allowlist and third-party notices, and added
the release primitive profiler. The initial Apple M4 Pro / macOS 26.5 / Rust
1.97.0 profile measured:

| Primitive | Result |
| --- | ---: |
| RC4 contiguous, 64 MiB | 1.241 GiB/s |
| RC4 16 KiB chunks, 64 MiB aggregate | 1.271 GiB/s |
| RC4 four-stream 16 KiB chunks, 64 MiB aggregate | 4.646 GiB/s |
| DH public-key work, 100 samples | 0.024 ms median / 0.029 ms p95 |
| DH valid remote-base shared secret, 100 samples | 0.024 ms median / 0.029 ms p95 |

Validation passed:

- `cargo test -p rstorrent-protocol --no-fail-fast` (100 passed, 2 ignored;
  architecture test passed);
- `cargo clippy -p rstorrent-protocol --all-targets -- -D warnings`;
- `cargo run -p rstorrent-protocol --release --example
  mse_primitives_profile`; and
- `cargo fmt --all` and `git diff --check`.

Gate 1 is complete. No unsafe code, RNG feature, runtime dependency, copied
oracle source, or variable-time secret exponentiation was introduced.

### 2026-08-09: Gate 2 sans-IO handshake

Implemented the synchronous initiator/responder state machine with explicit
compute-public-key, compute-shared-secret, torrent-lookup, and ordered-send
actions. The caller owns entropy and every external operation. One action may
be outstanding, mismatched or repeated resumption fails terminally, and no
runtime, socket, clock, task, or operating-system RNG entered the protocol
crate.

Both methods complete only after assembling the full remote 68-byte BitTorrent
handshake. The implementation covers every `IA` length from zero through 68,
the encrypted-handshake to selected-payload cipher transition, carried bytes,
and every padding/synchronization bound. Protocol failures are typed and make
the state terminal so partially advanced RC4 state cannot be reused.

Deterministic tests cover byte-at-a-time and coalesced input, seeded chunk
splits at BitTorrent-handshake field boundaries, zero and maximum pads, every
sync offset through 512, post-handshake carried frames in both methods, the
complete method-field hostile matrix, invalid verification constants,
oversized pad and initial-payload declarations, lookup absence/mismatch, and
the action ownership contract.

Validation passed:

- `cargo test -p rstorrent-protocol --no-fail-fast` (112 passed, 2 ignored;
  architecture test passed);
- `cargo clippy -p rstorrent-protocol --all-targets -- -D warnings`;
- `cargo fmt --all -- --check`; and
- `git diff --check`.

### 2026-08-09: Gates 3--4 peer runtime integration

Commits `90477c6`, `b37764a`, `ef22b2f`, and `ed22ecc` integrated both roles
without adding a long-lived task. One session-scoped `MseDhWorkOwner` owns a
four-permit semaphore and tracked blocking work; its deterministic barrier
test holds exactly four jobs active, observes the fifth waiting, cancels an
attempt without releasing its in-flight permit, and drains every tracked job.
The same owner is injected into the incoming service and every metadata and
content peer runtime, and shutdown closes and joins it after peer admission
ends.

Outgoing `Prefer` now retains bounded endpoint evidence and uses at most one
new-socket plaintext fallback for an early transport close, reset, EOF, or
timeout. A complete invalid DH key and every later protocol error fail without
downgrade. `PeerIo` has one ordered outbound queue and applies the send cipher
only at its commit point; receive bytes are decrypted before framing. Runtime
tests cover both methods, carried bytes, the eligible two-socket fallback,
invalid-DH no-fallback, exact payload hashes, cancellation, and owner drain.

Incoming detection shares the existing handshake deadline, distinguishes the
ordinary 68-byte header from a 96-byte MSE public key, and uses the bounded
collision-preserving `req2` index for provisional routing. The decrypted
BitTorrent handshake must name the same info hash before duplicate admission.
RC4 receive state remains with the reader and send state moves to the split
writer. The writer checks generation validity before advancing RC4 and then
finishes the one committed frame. Tests cover both negotiated methods,
carried handshake/frame bytes, ambiguous buckets and uniqueness restoration,
policy rejection, invalid provisional routing, generation fences, and the
Fast reject/refill path. A live incoming test proves that an established
plaintext peer survives `allow -> required`, a new plaintext peer is refused,
and a new RC4 peer transfers successfully.

### 2026-08-09: Gate 5 settings, product, and observability

Commits `5a95418` and `7b904cd` added the four-value persisted policy and the
terminal handshake evidence. Schema 15 adds the checked `encryption` column;
fresh and migrated profiles default to `allow`, unknown durable values fail
profile open, and an independent convergence domain applies later handshakes
without restarting a listener or torrent. Generated TypeScript, JSON Schema,
UniFFI, and Kotlin consumers carry configured/effective/application state. The
shared React settings section renders the exact four options and the accepted
non-security helper text.

Both MSE methods derive the existing `E` peer flag from the coherent
connection observation, and its legend now says "Encrypted or obfuscated."
The engine's terminal `MseHandshakeObservation` records role, captured policy,
fallback use, exact method or typed failure, raw wire and embedded protocol
bytes, carried raw bytes, and exponentiation count. Outgoing events project
through torrent diagnostics and incoming events through a session-wide
diagnostic sink. Tests reconcile raw and protocol byte metrics under both
methods and cover successful incoming RC4 plus fallback failure. No secret,
public key, shared secret, key material, or obfuscated torrent identifier
enters an event or application view.

### 2026-08-09: Gate 6 controlled interoperability and performance

Commits `47afbff`, `eb2b4a9`, `d856357`, and `426f2e0` added and exercised the
retained controlled harnesses. Against pinned libtorrent `2.0.13.0`, all 28
cases passed in both initiator directions: the complete policy matrix, forced
`0x01` and `0x02` selection in each direction, two delayed-flight probes, and
exact SHA-1 verification of an 8,389,339-byte payload. The proxy asserted
socket counts, fallback wire shape, absence of the known handshake header
under RC4, the specified plaintext response after a `0x01` PE4, and oracle
`peer_info` method flags. This run exposed and corrected the manually
connected `pe_enabled` assumption documented above.

The local setup medians were 8.872 ms plain and 9.319 ms MSE when RSTorrent
initiated, adding 0.447 ms. With libtorrent initiating they were 509.741 ms
plain and 511.817 ms MSE, adding 2.075 ms; those absolute values include the
oracle's roughly 500 ms connection-scheduling cadence. Both deltas meet the
25 ms diagnostic target. A transport proxy adding a fixed 25 ms one-way delay
observed two delayed turns for ordinary setup and four for MSE. The expected
extra round trip was 50 ms and the measured added setup was 62.346 ms, within
the 20 ms timer tolerance.

Profiling the original byte S-box found an avoidable RC4 hot-path cost. The
first-party implementation now uses an inline `u16[256]` S-box, native index
arithmetic, and an explicitly unrolled 16-byte production loop. This makes a
duplex state 1,056 bytes on the supported 64-bit targets, still inline and
well below the 4 KiB memory bound. A final release primitive profile on Apple
M4 Pro / macOS 26.5.2 / Rust 1.97 measured:

| Primitive | Result |
| --- | ---: |
| RC4 contiguous, 64 MiB | 0.990 GiB/s |
| RC4 16 KiB chunks, 64 MiB aggregate | 1.515 GiB/s |
| RC4 four-stream 16 KiB chunks, 64 MiB aggregate | 5.629 GiB/s |
| DH public-key work, 100 samples | 0.021 ms median / 0.023 ms p95 |
| DH valid remote-base shared secret, 100 samples | 0.021 ms median / 0.023 ms p95 |

The contiguous RC4 diagnostic misses 1 GiB/s by 1%; the production-shaped and
multi-stream profiles exceed it, DH is far below 2 ms, and no remaining
avoidable scalar defect was found. Commits `0cf771c` and `3f7a52f` extended
the release paired comparator with symmetric pinned-libtorrent cohorts and a
six-case balanced mode/implementation order. The clean final run used six
alternating 1 GiB pairs per implementation at 1 MiB pieces and storage `4/4`.
RSTorrent's plain and RC4 medians were 473.781 and 364.813 MiB/s; its median
within-pair RC4/plain ratio was `0.779873`, a 22.013% regression. Libtorrent's
corresponding medians were 493.383 and 366.520 MiB/s; its median within-pair
ratio was `0.758292`, a 24.171% regression. RSTorrent therefore retained 2.158
percentage points more of its plain throughput, or `1.028x` the oracle's
relative RC4 retention. The 10% diagnostic target misses, but RSTorrent clears
the explicit 75%-of-plain graduation guardrail and is not worse than the
pinned mature oracle, so no further RC4 optimization is justified by this
result.

Raw process-tree CPU measurements report RSTorrent at 2.087 plain and 2.077
RC4 median core-equivalents and libtorrent at 2.754 plain and 2.615 RC4. These
rates are normalized by each run's wall time and are diagnostic rather than
the decision metric. Every run verified the exact 1 GiB SHA-1, asserted RC4
on both libtorrent endpoints in forced cases and no MSE in ordinary-plain
cases, retained the payload and storage bounds, and cleaned up. The clean
report names repository commit `c5e80074a6fd49b111397dd6d7769ce60bfa55f2`,
libtorrent `2.0.13.0`, and production release binary SHA-256
`97466986206f9d11697db6b6624db3cc061396f986471804a3dd98a1a833883d`.

### 2026-08-09: Android product evidence and physical graduation

Commit `fb273e9` added the retained `product-mse` bootstrap profile. It selects
internal SAF storage, applies `required` before adding the magnet, starts five
controlled host seeds forced to RC4, verifies all five oracle sessions and the
published payload hash, samples the session DH owner, and checks exact device
and host cleanup. The full Android build cross-compiled release Rust and
generated Kotlin for x86_64 and arm64-v8a, built the APK, and passed Kotlin
tests.

One API 34 AVD product run passed with five forced-RC4 attempts, DH
`active=0`, `high_water=2`, `tracked=0`, and `waiting=0` at termination; file
descriptors were `baseline=116`, `high_water=140`, and `final=140`; storage
observed `limit=40`, `owned_high_water=6`, and `pending_high_water=3`; the
published info hash began `f2c09c855`; cleanup passed. Requiring exactly four
overlapping real DH jobs on a device would be timing-sensitive because these
operations complete in well under a millisecond. The deterministic
barrier-controlled Rust test owns exact saturation; the product run owns five
real attempts, the `<=4` ceiling, and complete drain.

A final no-build revalidation after the RC4 and diagnostic changes also
passed: five forced-RC4 attempts, the same exact info hash, DH
`active=0/high_water=2/tracked=0/waiting=0`, storage
`limit=40/owned_high_water=6/pending_high_water=2`, and exact cleanup.

The first physical attempt reached package installation but Android rejected
an older `org.rstorrent.bootstrap` installation signed with a different key.
The harness's documented failure cleanup cleared and uninstalled that owned
experimental package. A second exact run from the clean package state passed
on configured Pixel 7a serial `33031JEHN17672`, model `lynx`, API 37,
`arm64-v8a`, at repository commit `0b25152`:

- all five controlled oracle attempts negotiated forced RC4 and published the
  exact fixture with info hash
  `f2c09c855c0749be70ae5b5caa5f79077f914932`;
- the session DH owner terminated at `active=0`, `waiting=0`, and `tracked=0`
  with `high_water=3`, below the four-job ceiling;
- process descriptors were `baseline=158`, `high_water=177`, and `final=174`;
- storage reported `limit=40`, `owned_high_water=6`, and
  `pending_high_water=1`; and
- the runner reported exact device/host cleanup and final result `pass`.

The deterministic barrier test remains the authority for exact four-active,
one-waiting saturation. The physical run proves five real product attempts,
the production ceiling, full owner drain, exact verified publication, and
cleanup on the named device. This closes the final stopping condition and
graduates Tactical `111`.
