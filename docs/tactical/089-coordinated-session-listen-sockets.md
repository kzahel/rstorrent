# Tactical 089: Coordinated Session Listen Sockets

Status: Complete on 2026-08-05. Schema 11 persists the preferred port; one
application-generation allocator coordinates TCP and UDP; incoming peers and
DHT consume its supplied transports; and Rust, web, Android, loopback, and
eligible local-network evidence passes.

Topics: `incoming-reachability-and-seeding`, `client-persistence`,
`dht-discovery`, `application-view-api`, `protocol-support`,
`code-organization-and-refactoring`, `capability-readiness`

Dependencies: completed Tacticals
[`084`](084-persisted-client-connection-and-seeding-settings.md),
[`086`](086-long-lived-torrent-peer-runtime.md), and
[`088`](088-upnp-mapped-external-tcp-seeding.md) establish the persisted
settings waist, joined peer listener, session reachability owner, and proven
external TCP path used by this slice.

## Decision And Motivation

Create the session listen-socket owner required before truthful tracker or DHT
peer advertisement. The application currently starts two unrelated owners:
`IncomingPeerService` binds TCP from listener policy while `DhtService` binds
UDP to an independent ephemeral port. UDP tracker announces still send the
provisional peer port `6881`, DHT does not self-announce, and uTP is absent.

The next advertisement slice cannot safely derive public claims from this
shape. TCP and UDP may use the same numeric port because they are different
transport protocols, but DHT and future uTP traffic cannot be implemented as
independent UDP listeners on the same local endpoint. This tactical therefore
adds one application-generation socket allocator and one session UDP receive
owner, then migrates DHT onto that transport without yet advertising a peer.

Port `6881` becomes a persisted, user-editable preferred listen port rather
than a second internal constant. Automatic listening starts there, uses the
pinned libtorrent retry and system-fallback policy, and reports the actual TCP
and UDP endpoints. Fixed listening remains exact by explicit RSTorrent product
policy. Actual bound ports and future mapped external ports remain runtime
facts and are never persisted as configuration.

## Stopping Condition

This tactical is complete when all of the following hold:

1. schema version 11 persists and validates one preferred listen port,
   defaults it to `6881`, migrates every version-10 row to that default, and
   exposes it through the existing Rust, generated TypeScript, web, and
   UniFFI/Android contracts;
2. automatic listening attempts the preferred TCP port and at most ten
   successive ports, then asks the operating system for a port; UDP begins at
   the actual TCP port, consumes the remaining automatic retry budget on
   address conflicts, and finally uses an OS-selected port;
3. fixed listening binds both TCP and UDP to the configured numeric port or
   reports a typed bind failure without leaving a TCP listener alive;
4. one session socket set retains the bound TCP listener until it is handed to
   the incoming-peer service and retains one UDP socket behind a single
   bounded receive owner; DHT sends and receives through that UDP owner rather
   than constructing its own application socket;
5. runtime state exposes the configured preferred port, actual TCP endpoint,
   actual UDP endpoint, and existing mapped external TCP endpoint as distinct
   facts, including the legitimate automatic case where the UDP numeric port
   differs after a UDP-only collision;
6. a controlled loopback DHT exchange and incoming TCP exchange both succeed
   through the coordinated endpoints, shutdown joins DHT before the UDP owner
   and joins the incoming listener, and declared terminal owner counts are
   zero; and
7. deterministic bind-policy tests, scripted socket-conflict tests, DHT
   runtime tests, application restart/migration tests, generated-contract
   checks, product tests, and the full workspace baseline pass.

This is socket and lifecycle evidence. It does not satisfy tracker
advertisement, DHT `announce_peer`, uTP, UDP gateway mapping, or public-swarm
seeding claims.

## Scope

- Add `preferred_listen_port` to the typed client settings group with range
  `1024..=65535`, default `6881`, atomic replacement, corruption rejection,
  restart-required comparison, and schema version 10-to-11 migration.
- Expose the preferred port in the existing Connection & Seeding product
  settings. It remains editable while fixed listener modes retain their own
  exact port.
- Add one engine session-socket subsystem that owns address selection,
  TCP/UDP coordinated allocation, retry accounting, concrete endpoints, and
  typed bind errors independently from DHT or peer protocol state.
- Let `IncomingPeerService` start from an already-bound `TcpListener` while
  retaining its focused convenience bind path for engine tests.
- Add one bounded session UDP ingress task and transport handle. The ingress
  task is the only receiver on the socket; DHT gets one bounded datagram
  route and may send through the same socket. Future uTP and UDP-tracker
  consumers must extend this dispatch owner rather than start competing
  receivers.
- Let `DhtService::start` retain a standalone convenience path for focused
  engine tests, implemented using the same UDP owner. Application startup uses
  the supplied session transport path.
- Keep DHT available when the peer listener is disabled or cannot bind by
  creating an independent application-owned ephemeral UDP socket under the
  same UDP subsystem. That endpoint is observable but not a peer endpoint.
- Add structured startup and terminal diagnostics for actual endpoints and
  UDP task/queue ownership without logging datagram payloads.

## Port Policy

### Automatic modes

The durable preferred port is the first candidate. The allocator holds every
successful socket while it allocates the remaining transport, preventing a
time-of-check/time-of-use gap.

1. Try TCP on `preferred_listen_port`.
2. On `AddressInUse`, increment without wrapping and retry at most ten times.
3. If those candidates remain occupied, bind TCP to port zero and read the
   OS-selected port.
4. Begin UDP on the actual TCP numeric port.
5. On UDP `AddressInUse`, increment without wrapping while the shared retry
   budget remains.
6. If still occupied, bind UDP to port zero and report its actual port.

Like the pinned libtorrent implementation, the ten-retry counter is shared:
TCP conflicts reduce the retries left for UDP. A UDP-only conflict may
therefore produce different actual TCP and UDP ports. Port `65535` never wraps
to zero as an increment; only the explicit system fallback requests port zero.
Non-`AddressInUse` failures are typed immediately.

Automatic loopback and automatic local-network modes use this same policy.
This keeps product semantics consistent; parallel tests may override the
preferred candidate or rely on bounded fallback rather than assuming `6881`
is free.

### Fixed modes

Fixed loopback and fixed local-network modes attempt the configured numeric
port exactly once for TCP and exactly once for UDP. Either conflict fails the
coordinated listener atomically, drops any socket already opened by the
attempt, and leaves settings intact for diagnosis and replacement. There is no
increment or system fallback.

This is an intentional product difference from libtorrent, whose global retry
and fallback settings also apply to explicit listen-interface ports. RSTorrent
already promises that `Fixed` means exact, and silent substitution would make
manual forwarding and operator diagnosis misleading.

### Disabled or failed peer listener

Disabling TCP does not disable the existing DHT capability. The application
binds one UDP socket at the DHT network-policy address with port zero and owns
it through the same session UDP subsystem. If coordinated TCP/UDP allocation
fails, listener status reports the failure and DHT receives a fresh ephemeral
UDP transport. That UDP port is never treated as an advertisable TCP peer
port.

## Runtime Contract

`ClientSettingsRuntimeView` keeps four meanings separate:

- `configured.preferred_listen_port`: durable next-generation preference;
- `listener_status`: disabled, failed, or actual TCP address and port for this
  generation;
- `session_udp_status`: actual UDP address and port plus whether its numeric
  port is coordinated with the live TCP listener; and
- `port_mapping_status`: the existing external TCP mapping, if any.

The active settings retain the preference that produced the bind attempt.
Changing either listener policy or preferred port is restart-applied. No
actual local or external endpoint is written back to SQLite.

## Owner, Task, Cancellation, And Dependency Map

```text
ApplicationService generation
  -> SessionListenSockets allocator (task-free; consumes policy)
       -> bound TcpListener (optional)
       -> bound UdpSocket (exactly one)
  -> IncomingPeerService (consumes TcpListener)
       -> accept task + bounded handshake/peer tasks
       -> upload-scheduler task
  -> SessionUdpService (consumes UdpSocket)
       -> one receive/dispatch task
       -> bounded DHT ingress queue
       -> send handle sharing only the socket send side
  -> DhtService (consumes DHT ingress + send handle)
       -> one DHT actor task
```

Dependency direction is inward: listener policy and deterministic candidate
selection do not depend on Tokio tasks; socket allocation depends on that
policy; DHT and incoming runtime consume already-bound transports; the
application composes and observes them. Protocol codecs and DHT state do not
learn about application settings or persistence.

Shutdown order is reachability withdrawal, incoming intake/peer shutdown,
DHT cancellation and snapshot, session UDP cancellation/join, then the
remaining application owners. `Drop` cancels as a fallback but is not the
successful shutdown path. DHT queue closure, UDP read failure, task panic, and
partial startup all have observable typed outcomes and cannot detach a task.

The UDP ingress queue is finite. It retains datagrams no larger than the
existing DHT maximum plus the one-byte oversize sentinel needed for truthful
malformed accounting. Queue saturation drops the new datagram, increments a
bounded/saturating counter, and never blocks socket intake on DHT work.

## Normative And Reference Dossier

### Specification

- `reference/bittorrent.org/beps/bep_0005.rst` at pinned revision
  `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`: Mainline DHT is UDP, a DHT
  node endpoint is distinct from a TCP BitTorrent peer endpoint, BEP 5's PORT
  message carries the node's UDP port, and `announce_peer` may carry an
  explicit peer TCP port or use the observed UDP source port only when
  `implied_port` is set. This tactical changes only transport ownership and
  does not send PORT or `announce_peer`.

TCP/UDP numeric-port coexistence and OS-selected port zero are socket API
semantics rather than a BitTorrent wire-protocol extension. Tests prove the
behavior on every supported CI/runtime target instead of treating it as an
advertisement claim.

### Pinned libtorrent oracle

Revision `7d7fc38fac61177fa5e02148f791b2f65250b09d` from
[`reference/pins.toml`](../../reference/pins.toml) was inspected:

- `reference/libtorrent/src/settings_pack.cpp`: default
  `listen_interfaces` at `0.0.0.0:6881,[::]:6881`, enabled system-port
  fallback, and `max_retry_port_bind = 10`;
- `reference/libtorrent/include/libtorrent/settings_pack.hpp`:
  `listen_interfaces`, `listen_system_port_fallback`, and
  `max_retry_port_bind` contracts;
- `reference/libtorrent/src/session_impl.cpp`:
  `session_impl::setup_listener` binds TCP, starts UDP from the resulting TCP
  address/port, shares the remaining retry counter, and applies explicit
  system fallback; `session_impl::on_udp_packet` gives one session UDP stream
  to uTP first, then DHT-shaped bencoded packets, then UDP trackers;
- `reference/libtorrent/include/libtorrent/aux_/session_impl.hpp` and
  `session_udp_sockets.hpp`: `listen_socket_t` owns the related TCP acceptor,
  session UDP socket, mapping handles, and observed local/external ports;
- `reference/libtorrent/test/test_direct_dht.cpp`, case
  `direct_dht_request`: DHT request/response uses a session listen port;
- `reference/libtorrent/test/test_utp.cpp`, case `utp`: uTP transfers through
  session listen-interface setup and joined session abort; and
- `reference/libtorrent/test/test_session.cpp`, loopback mapping/reopen case:
  listen-socket eligibility gates mapping work.

The pinned tests exercise shared session endpoints and consumers but do not
directly pin retry exhaustion, a UDP-only collision, counter sharing, or
overflow. RSTorrent adds independent deterministic and socket-level tests for
those source-discovered edge cases rather than assuming they are covered.

Adopted behavior:

- one listen-socket owner coordinates TCP and UDP;
- default preferred port `6881`, ten incremental address-in-use retries, one
  OS-selected fallback, and a retry counter shared across the two binds;
- UDP begins from the actual TCP numeric port but may diverge on conflict;
- actual TCP and UDP ports remain separately observable; and
- one UDP receive owner is the future demultiplexing waist.

Intentional differences:

- RSTorrent's persisted `Fixed` modes are exact and disable retry/fallback;
- the first slice remains IPv4 and one selected interface rather than
  libtorrent's multi-interface IPv4/IPv6 listen-socket collection;
- partial coordinated construction is atomic: an unrecoverable UDP bind does
  not leave a peer listener advertised as a complete session socket set;
- disabled or failed TCP retains DHT on an independently bound ephemeral UDP
  endpoint rather than disabling session UDP networking; and
- RSTorrent keeps deterministic policy, Tokio socket ownership, DHT actor
  state, application persistence, and product views in separate modules.

Libtorrent is BSD-3-Clause and is used only as a source and executable
interoperability oracle. No source, fixture, or test data is imported.

### JSTorrent product history

The local sibling was inspected at its current `main` checkout:

- `packages/engine/src/core/bt-engine.ts` defaults `port` to `6881`, maps TCP
  on that port, creates DHT at `port + 1`, and maps that UDP port;
- `packages/engine/src/dht/krpc-socket.ts` lets the DHT KRPC owner construct
  and receive from its own UDP socket; and
- `packages/engine/src/port-mapping/port-mapping-manager.ts` assumes the same
  internal and external numeric port for each mapping.

That history confirms the product value of a configurable base port and
visible UPnP state, but it also demonstrates the `listener + 1` convention and
independent UDP ownership this topic explicitly rejects. No JSTorrent source
or fixture is copied.

## Invariants And Failure Cases

- TCP and UDP sockets are both held before coordinated endpoints are
  published; no probe-then-rebind race is permitted.
- Candidate generation is finite, deterministic, and cannot overflow or wrap.
- Only `AddressInUse` advances an automatic candidate or reaches system
  fallback. Permission, unavailable-address, descriptor, and other failures
  retain their typed cause.
- Fixed policy never silently changes either numeric port.
- Exactly one task calls `recv_from` for a session UDP socket.
- DHT receives at most one bounded route and never owns a second application
  socket.
- A full DHT ingress queue drops rather than allocates or blocks without bound;
  counters saturate.
- UDP datagrams remain hostile input and keep the existing maximum decode
  length, rate limits, transaction limits, and malformed accounting.
- UDP bind success is not proof of public UDP reachability and does not create
  a gateway mapping or advertised peer endpoint.
- Mapping remains TCP-only and consumes the actual listener endpoint exactly
  as in Tactical 088.
- Settings failure is atomic; runtime bind failure does not mutate settings.
- Startup rollback and normal shutdown cancel and join every created task.

## Validation Plan

| Layer | Required evidence |
| --- | --- |
| Pure | Preferred/fixed validation; initial plus ten candidates; shared retry consumption; `65535` non-wrapping fallback; exact fixed policy; runtime-view equality. |
| Persistence | Fresh schema 11 defaults; version-10 migration to `6881`; automatic/fixed round trip; corrupt preferred port rejection; atomic command/revision/replay behavior. |
| Socket runtime | Same-number TCP/UDP bind; TCP conflict; UDP-only conflict and divergence; eleven-conflict system fallback; exact fixed TCP and UDP conflict; no leaked listener. |
| UDP owner | One receiver; bounded saturation/drop counters; oversize forwarding for DHT malformed accounting; send/receive; cancellation, socket error, join, and zero terminal task/queue counts. |
| DHT | Existing scripted query/response, bootstrap, lookup, rate, malformed, timeout, and shutdown suites pass through supplied and standalone session UDP transports. |
| Application | Disabled, automatic loopback, automatic local-network, fixed, bind-failure, restart-required, mapping eligibility, coordinated endpoints, DHT continuity, and exact shutdown order. |
| Product | Generated Rust/JSON Schema/TypeScript/UniFFI contracts; web validation, settings form, runtime display, demos/fixtures, and Android reducer fixtures. |
| Baseline | `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`; web lint/typecheck/test/build and Android unit/build gates established by the repository. |

No public DHT or external-network run is required to prove socket ownership.
The controlled DHT server must observe the source UDP endpoint reported by the
runtime, while a controlled TCP peer connects to the reported TCP endpoint in
the same application generation.

## Implementation Slices

1. **Tactical and settings contract.** Land schema 11, the preferred-port
   field and migration, generated/product contracts, validation, and settings
   UI. Commit after persistence and product contract gates pass.
2. **Coordinated socket allocator.** Land deterministic policy, exact
   automatic/fixed runtime behavior, bound-socket handoff to incoming peers,
   and conflict tests. Commit after engine and application listener tests pass.
3. **Shared UDP owner and DHT migration.** Land the bounded receive owner,
   supplied DHT transport, application lifetime/shutdown ordering, runtime UDP
   status, and scripted joint TCP/DHT evidence. Commit after workspace and
   product gates pass.
4. **Closure.** Record exact commands, results, owner high-water/terminal
   counts, update every owning topic and support claim, and mark this tactical
   complete only if the stopping condition is met.

## Deliberate Deferrals

- Replacing `DEFAULT_ADVERTISED_PEER_PORT` in tracker announces, tracker stop
  correction, DHT `announce_peer`, and advertisement withdrawal.
- BEP 5 PORT messages, BEP 10 listen-port messages, UDP tracker migration onto
  the shared receive socket, uTP packet classification/state, and uTP peers.
- UDP UPnP mapping, PCP, NAT-PMP, IPv6 sockets/pinholes, multiple interfaces,
  interface-change rebinding, VPN/metered policy, and Android local-network
  permission evidence.
- Live settings application without restart, manual bind-address selection,
  a separate user-facing DHT port, port randomization policy, and automatic
  firewall configuration.
- Any public reachability or discovery support claim based solely on matching
  TCP and UDP local port numbers.

## Completion Evidence

- Schema version 11 adds the constrained `preferred_listen_port` column,
  defaults fresh and version-10 profiles to `6881`, retains atomic group
  replacement, and rejects corrupt values below `1024`.
- The generated JSON Schema, TypeScript, validators, web form, runtime
  equality checks, fixtures, and UniFFI Android constructor carry the same
  setting. The product distinguishes configured preference, actual TCP,
  actual UDP and its coordination bit, and existing mapped external TCP
  status; actual endpoints never enter SQLite.
- Focused Rust settings tests pass `9` cases and focused durable command tests
  pass `2` cases. Fresh version-11 creation, version-10 migration, corrupt-row
  rejection, atomic command/revision/replay behavior, and restart-required
  comparison all pass.
- `rstorrent-engine::session_socket` holds successful sockets across the
  complete allocation. Its `7` focused cases prove preferred same-port TCP
  and UDP, TCP-conflict advancement, UDP-only divergence, shared ten-retry
  exhaustion into explicit system ports, fixed UDP-conflict atomicity,
  disabled-listener UDP continuity, and non-wrapping candidate generation.
- `rstorrent-engine::session_udp` is the only `recv_from` owner. Its capacity
  is `64` DHT datagrams and its receive buffer is the existing DHT maximum
  plus one oversize sentinel byte. Its `4` focused cases prove bidirectional
  use of one socket, bounded oversize delivery, drop-on-full/high-water
  accounting, saturating lifetime counters, one task high water, and terminal
  `tasks=0, queued=0` after joined shutdown.
- `IncomingPeerService::start` validates and consumes an already-bound TCP
  listener. `DhtService::start_with_transport` consumes the bounded session
  UDP route and shared send handle; the standalone DHT constructor composes
  the same UDP owner for focused use. Partial application startup drops the
  unused route and joins UDP; normal shutdown joins DHT before UDP. The full
  DHT suite passes `11` active cases with its one public-network smoke ignored.
- A controlled loopback application generation reports its chosen TCP and UDP
  endpoints, accepts TCP on the reported listener, and sends a DHT query from
  the reported UDP source. A second test exercises the same exchange on the
  host's eligible non-loopback IPv4 interface when present; it ran on the
  implementation host. Both use the persisted preferred port and observe
  matching TCP/UDP ports. The fixed-TCP-conflict application case reports a
  typed listener failure while DHT remains bound independently.
- The generated-contract validator rejects a runtime claim that marks
  different TCP and UDP endpoints as coordinated. Web typecheck and build
  pass; Vitest passes `178` active tests with `2` skipped. Regenerated UniFFI
  Kotlin bindings compile; Gradle `testDebugUnitTest assembleDebug` succeeds.
- `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace` pass. The workspace test gate includes `257`
  active engine tests with `3` ignored and, after the local-network evidence
  case landed, `148` active session tests with `1` ignored. Public DHT or
  public-swarm traffic was intentionally not used for this ownership claim.
- Logical commits are `5a7a42d` (tactical), `ca338cc` (persisted setting),
  `d77495d` (coordinated allocator, UDP owner, DHT/application migration and
  product contract), and `e5969be` (eligible local-network integration
  evidence).
