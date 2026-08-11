# Tactical 131: Bounded Product uTP Composition

Status: **Active**. Human review selected recommendation A on 2026-08-11:
compose the proven uTP runtime into the ordinary application behind an
explicit, default-off policy, initially IPv4, plaintext, and fixed at 548
bytes. Default enablement and BEP 29 support graduation remain later human
gates.

Topics: `utp-transport-campaign`, `peer-lifecycle`,
`incoming-reachability-and-seeding`, `protocol-support`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: closed Tactical
[`130`](130-utp-transport-solidification.md) supplies the fixed-runtime,
impairment, hostile lifecycle, diagnostic-MTU, and two-direction WAN baseline.
Completed Tactical
[`125`](125-shared-udp-utp-runtime-and-loopback-interop.md) supplies the shared
UDP service, ordered stream, incoming admission, and controlled peer-wire
composition. Ready Tactical
[`129`](129-bounded-storage-intake-watermark.md) remains intact and queued.

## Decision And Desired Outcome

Make uTP a real but default-off application transport without yet making it a
product default or support claim. One explicit construction policy starts the
existing fixed-548 uTP service from the session's shared UDP owner, routes
incoming IPv4 uTP streams into the existing incoming peer registry, and gives
ordinary metadata and content dials a source-first uTP selection with bounded
TCP fallback.

The first policy is deliberately small:

- `TcpOnly` remains the constructor default used by desktop, Android, gateway,
  CLI, and tests that do not opt in;
- `PreferUtp` is a programmatic application-construction policy, not a
  persisted setting, UI choice, generated application contract, or default;
- outgoing uTP is eligible only for IPv4 endpoints and plaintext-capable
  `disabled` or `allow` encryption policy; IPv6 and `prefer`/`required`
  encryption continue over TCP;
- an eligible dial tries uTP first and falls back sequentially to TCP only when
  the uTP transport connection fails before the BitTorrent handshake;
- one logical dial attempt, outgoing peer-budget permit, cancellation token,
  registry generation, and final peer penalty span both transport subattempts;
  there is no racing or duplicate connection generation; and
- successful transport is reported through the existing transport-neutral
  peer lifecycle and view vocabulary.

This adopts libtorrent's conservative interoperability policy while fitting
RSTorrent's existing ownership: assume a fresh IPv4 peer may accept uTP, try
it first, and retry TCP immediately after a uTP connect failure. This tactical
does not need libtorrent's socket variant, callback graph, mixed-mode bandwidth
policy, or full endpoint capability cache. Remembering uTP success/failure
across later logical dials is deferred until measurements show that repeated
probing is a material product problem; fallback within the current attempt is
required now.

## Stopping Condition

The tactical stops at human review only after all applicable gates pass:

1. the default application configuration creates no uTP service, preserves
   TCP-only metadata/content dialing and incoming behavior, and passes the
   existing application and TCP interoperability regressions;
2. explicit `PreferUtp` starts exactly one fixed-548 IPv4 service from the
   session UDP owner and exposes one cloneable handle to all torrent
   generations without a second UDP socket or receive task;
3. incoming uTP admission uses the existing handshake, peer budget, torrent
   registration, upload scheduler, connection identity, view, cancellation,
   and shutdown owners; bounded admission tasks and the uTP service join before
   the incoming runtime and shared UDP owner disappear;
4. eligible outgoing metadata and content dials use uTP, while IPv6 and
   encryption `prefer`/`required` use TCP; one uTP transport-connect failure
   falls back to TCP inside the same attempt and does not consume a second peer
   permit or record an intermediate peer failure;
5. cancellation during either subattempt produces one cancelled logical dial,
   session socket replacement remains generation-fenced, queue and connection
   bounds remain unchanged, and terminal uTP connections, half-opens,
   admission tasks, and service tasks are zero;
6. controlled application-backed exact transfers against pinned libtorrent
   `2.0.13.0` pass with RSTorrent in both incoming and outgoing uTP roles, and a
   TCP-only oracle proves uTP-connect-to-TCP fallback with the final transport
   observed as TCP;
7. ordinary uTP stays fixed at 548 bytes, no UDP mapping or uTP endpoint
   advertisement is added, no user-visible setting or generated contract
   changes, and BEP 29 remains **Unsupported**; and
8. focused tests, controlled interoperability, formatting, clippy, and the
   complete workspace baseline pass, with owning topics reconciled.

If the existing application harness cannot expose transport evidence without
changing a user-facing contract, extend only its bounded JSON diagnostic
output. Do not add product presentation to satisfy a test.

## Scope Boundaries And Human Stops

This tactical authorizes:

- a small engine transport-selection value and one default-off application
  construction policy;
- session ownership for the existing `UtpService`, its handle, and bounded
  incoming-admission supervisor;
- outgoing uTP selection and one sequential TCP fallback inside the existing
  `PeerSocketSet` dial owner;
- exact transport observations and bounded diagnostics needed to distinguish
  uTP success, TCP selection, and fallback;
- independently authored deterministic, real-socket, application, and pinned-
  oracle tests on loopback; and
- evidence-backed fixes within these accepted owners, committed by coherent
  stage.

Stop before:

- enabling uTP by default in any shipped client or fresh profile;
- adding a persisted setting, UI, generated application-contract field,
  per-torrent policy, remote control, or migration;
- UDP UPnP mapping, endpoint advertisement, tracker/DHT port changes,
  `implied_port`, NAT traversal, hole punching, or public-swarm/WAN/device work;
- racing TCP and uTP, IPv6 uTP, MSE-over-uTP, dynamic PMTU, a mixed-mode
  bandwidth algorithm, or endpoint capability persistence;
- a dependency, foreign source, unsafe platform socket code, or materially
  different runtime architecture; or
- changing the BEP 29 support claim.

These are early human stops. Routine representations, test helpers, internal
diagnostic names, and repairs preserving this architecture proceed
autonomously.

## Invariants And Resource Bounds

- The session retains one UDP receive task per bound family. Starting uTP takes
  the existing independent 256-datagram route; it never binds another socket.
- Existing uTP bounds remain: 64 global connections, 16 incoming half-opens,
  64 datagrams per connection, a 256-datagram shared route, 1 MiB receive
  credit, 1 MiB unsent bytes, 1,024 sent packets, and a 1 MiB sent ledger.
- Ordinary runtime uses `UtpRuntimeConfig::fixed()` and a 548-byte IPv4 UDP
  payload. Tactical `130`'s diagnostic MTU constructor is not reachable from
  product composition.
- The uTP admission supervisor has at most the existing 64 queued streams and
  the incoming runtime's existing bounded pending-handshake permits. It owns
  and joins every task it starts.
- One outgoing logical attempt owns exactly one peer-budget permit across uTP
  and TCP. The two transports are never live concurrently for that attempt.
- Network and address-family policy is checked before either transport. TCP
  fallback cannot bypass a denied endpoint.
- uTP is plaintext only. `PeerEncryptionPolicy::Prefer` and `Required` select
  TCP before uTP; an incoming uTP plaintext handshake under `Required` is
  rejected by the existing incoming policy.
- Controlled tests use the existing 2,097,883-byte exact fixture and a
  180-second whole-case bound. They disable discovery and permit at most one
  peer per role. No payload, profile, log, or capture is retained.
- No committed diagnostic contains public endpoints, peer IDs, filesystem
  roots, payload bytes, or unbounded output.

## Source-First Record

Re-read managed BEP 29 at BitTorrent BEP commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`, especially `overview`,
`connection setup`, and connection-ID behavior. BEP 29 defines the UDP
transport and congestion behavior but does not prescribe TCP/uTP selection,
fallback, racing, product defaults, mapping, or capability memory. Those are
client policy and require implementation-oracle evidence.

Re-inspected Rasterbar libtorrent commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `src/torrent_peer.cpp::torrent_peer` initializes `supports_utp=true`, making
  a fresh peer uTP-eligible;
- `src/torrent.cpp::torrent::connect_to_peer` selects uTP when outgoing uTP is
  enabled and TCP is disabled or the peer is assumed/confirmed to support uTP,
  otherwise it constructs TCP;
- `src/peer_connection.cpp::peer_connection::connect_failed` clears assumed
  uTP support and schedules an immediate TCP reconnect only after a uTP
  transport connection failure;
- `src/peer_connection.cpp::peer_connection::on_connection_complete` records
  confirmed uTP support after successful transport establishment;
- `src/settings_pack.cpp` and
  `include/libtorrent/settings_pack.hpp` keep independent incoming/outgoing
  TCP/uTP switches and reserve `mixed_mode_algorithm` for bandwidth treatment,
  not dial racing;
- `test/test_utp.cpp::test_transfer` forces uTP by disabling outgoing and
  incoming TCP for exact transfer and joined cleanup; and
- `simulation/test_swarm.cpp::utp_only` proves a complete uTP-only swarm,
  while the adjacent connection-timeout and self-connect cases preserve
  bounded failure/identity expectations. No focused test directly asserts the
  uTP-to-TCP retry, so RSTorrent must cover the adopted branch independently.

Adopted behavior is fresh-peer uTP preference and sequential TCP retry after
uTP connect failure. Intentional differences are one logical RSTorrent dial
and permit across both subattempts, no endpoint capability cache in this
slice, no racing, default-off product construction, plaintext-only uTP, and no
mixed-mode rate policy. No reference source, test vector, or fixture is copied.

The local JSTorrent sibling is exactly
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/engine/src/core/connection-manager.ts::initiateConnection` constructs
  only `ITcpSocket` peer connections;
- `packages/engine/src/interfaces/socket.ts` exposes separate TCP and UDP
  platform interfaces but no uTP stream or transport-selection policy;
- `docs/archive/engine/legacy-migration/architecture_analysis.md` records BEP
  29 as missing because peer traffic relies on TCP; and
- `docs/archive/project/RELEASE-STATUS.md` records the shipped limitation as
  TCP-only.

There is therefore no JSTorrent uTP product behavior, fallback policy, or
default to preserve. Its application-backed completed-seed harness pattern is
useful product history; no source or fixture is copied.

## Owner, Task, Cancellation, And Dependency Map

| Owner | Bounded work | Cancellation and termination |
| --- | --- | --- |
| `ApplicationConfig` | default-off `TcpOnly` or explicit `PreferUtp` construction policy | immutable for one application generation; no persisted or live setting |
| `SessionNetworkOwner` | shared UDP service, fixed uTP service, handle, incoming uTP supervisor | cancellation stops acceptance, shuts down and joins uTP/admission work before incoming runtime and UDP shutdown |
| Incoming uTP supervisor | accept at most the existing bounded stream queue and admit through `IncomingPeerHandle` | session cancellation and incoming-runtime cancellation terminate handshakes; all admissions join |
| `DownloadControl` / torrent generation | clone the session uTP handle into metadata and content coordination | handle drop cannot outlive the joined application task; session service owns actual sockets/workers |
| `PeerSocketSet` dial | one uTP connect, optional TCP connect, peer handshake, permit, progress, final result | dial, budget, address-family, or application cancellation ends the current subattempt and suppresses fallback |
| `PeerRuntime` generation | one outgoing identity with actual successful transport | one terminal success/failure/cancellation; fallback is not a second peer generation |
| Controlled application harness | two forced-uTP roles and one forced fallback case | whole-case timeout stops child/oracle, verifies exact content, then removes temporary roots |

Protocol codec, reliability, congestion, and fixed MTU remain inward and
runtime-independent. `utp_runtime` depends on the shared session UDP boundary;
peer socket execution depends on a cloneable uTP handle; the application
session composes those owners. No inward protocol or peer-state module depends
on session, application, filesystem, or test-harness types.

## Staged Execution And Commit Plan

1. Commit this tactical and authoritative queue/checkpoint reconciliation.
2. Add default-off session uTP composition, bounded incoming admission, exact
   startup/shutdown observations, and lifecycle tests; commit.
3. Add transport-aware outgoing dial selection and sequential TCP fallback
   under one permit/cancellation/generation, including deterministic and real-
   socket tests; commit.
4. Extend the controlled application seed diagnostic and add pinned-libtorrent
   loopback cases for incoming uTP, outgoing uTP, and TCP fallback. Preserve
   exact content, one-peer, transport, resource, and cleanup assertions;
   commit.
5. Run and record:

```text
source ~/.profile
cargo test -p rstorrent-engine peer_socket
cargo test -p rstorrent-engine utp
cargo test -p rstorrent-session session_utp
uv run --project tests/interop --locked \
  python tests/interop/utp_product_integration.py
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

6. Reconcile every owning topic, retain default-off behavior and the
   **Unsupported** BEP 29 claim, commit the result, and stop at the product-
   enablement human review.
