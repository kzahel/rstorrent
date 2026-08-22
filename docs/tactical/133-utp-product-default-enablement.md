# Tactical 133: uTP Product Default Enablement

Status: **Complete** on 2026-08-11. Tactical `132` completed the bounded
default-readiness evidence, the maintainer selected recommendation A, and this
tactical made the existing fixed-548 IPv4/plaintext uTP path the common
application construction default. Tactical `129` is now the single
authoritative **Now**.

Topics: `utp-transport-campaign`, `peer-lifecycle`, `protocol-support`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tactical
[`131`](131-bounded-product-utp-composition.md) supplies the default-off
application path and completed Tactical
[`132`](132-utp-default-readiness-evidence.md) supplies bounded endpoint
memory, recovery, controlled interoperability, and one ordinary-swarm
observation. Ready Tactical
[`129`](129-bounded-storage-intake-watermark.md) remains intact and queued.

## Decision And Desired Outcome

Make the existing `PreferUtp` construction policy the default for every
ordinary `ApplicationConfig`. The desktop, Android, gateway, and application
CLI constructors already inherit that one value, so the policy change belongs
at the application owner rather than in four platform-specific overrides.

The enabled product subset remains deliberately narrower than libtorrent's:

- eligible outgoing IPv4 peers under encryption `disabled` or `allow` try uTP
  first, retain endpoint-scoped capability memory, and fall back sequentially
  to TCP after a joined uTP transport failure;
- incoming IPv4 plaintext uTP shares the session UDP socket and ordinary peer
  admission owner;
- IPv6 and encryption `prefer` or `required` remain TCP-only;
- ordinary uTP stays at the fixed 548-byte MTU and every existing connection,
  queue, stream, ledger, peer-record, and task bound stays unchanged; and
- `TcpOnly` remains an explicit construction policy for protocol-isolated
  diagnostics and tests, but is no longer the ordinary application default.

This is a construction default, not a persisted user preference. Existing
profiles adopt it on their next application generation without a schema,
migration, generated-contract, settings, or presentation change.

## Stopping Condition

The tactical is complete only when:

1. both durable and ephemeral `ApplicationConfig` constructors default to
   `PreferUtp`, while an explicit `TcpOnly` override still starts no uTP owner;
2. desktop and Android product configuration tests assert the common uTP
   default, and application startup/shutdown proves exactly one fixed uTP
   service shares the session UDP owner and terminates with zero connections,
   half-opens, queued datagrams, admission tasks, and worker panics;
3. the application-backed pinned-libtorrent suite exercises the ordinary
   constructor default, not an opt-in flag, for incoming uTP, outgoing uTP,
   and uTP-to-TCP fallback, with exact content and final transport evidence;
4. explicit TCP-only diagnostics retain the ordinary TCP, Fast, and forced-MSE
   regression with no uTP owner, so default enablement cannot weaken the
   transport-specific baseline;
5. cancellation before and after uTP establishment, endpoint suppression and
   recovery, one-permit fallback, and joined resource closure remain covered
   by the retained deterministic and real-socket tests;
6. desktop and Android Rust tests pass, both supported Android native targets
   build, and no platform-specific socket, lifecycle, setting, or generated
   contract is introduced;
7. the exact implemented BEP 29 product subset graduates from **Unsupported**
   to **Partial**, while every unimplemented boundary remains named; and
8. focused tests, controlled interoperability, formatting, workspace Clippy,
   and the complete workspace baseline pass, with owning topics reconciled.

## Scope And Human Stops

This tactical authorizes:

- changing the common `ApplicationConfig` default from `TcpOnly` to
  `PreferUtp`;
- explicit `TcpOnly` selection in diagnostic tools whose contract is a
  transport-isolated TCP or MSE baseline;
- focused application, desktop, Android, and diagnostic-harness assertions
  needed to distinguish inherited default policy from explicit override;
- using the retained loopback pinned-libtorrent application cohort without a
  public-network run; and
- graduating only the implemented BEP 29 subset to **Partial** after all gates
  pass.

Stop for human direction before:

- a persisted setting, UI or generated application-contract field, migration,
  per-torrent user policy, or remote-control surface;
- UDP UPnP mapping, tracker/DHT incoming-endpoint advertisement, `implied_port`,
  NAT traversal, hole punching, or any permanent network change;
- IPv6 uTP, MSE-over-uTP, TCP/uTP racing, dynamic product MTU, proxy semantics,
  or a mixed-mode bandwidth algorithm;
- another public-swarm, WAN, emulator, visible-client, or physical-device run;
- a dependency, foreign source, unsafe platform socket implementation, or
  materially different task/owner architecture; or
- a **Supported** BEP 29 claim.

Ordinary test repairs, internal diagnostic naming, and explicit test-only
policy selection within these boundaries proceed autonomously.

## Invariants And Resource Bounds

- `ApplicationConfig` remains the sole construction-policy owner. Desktop,
  Android, gateway, and CLI do not duplicate a uTP default.
- One application generation has one immutable `PeerTransportPolicy`; there is
  no stored/live setting and no reconfiguration task.
- The session retains one UDP receive task per bound family. Default uTP uses
  the existing IPv4 256-datagram uTP route and never binds another socket.
- Existing uTP bounds remain 64 connections, 16 incoming half-opens, 64
  datagrams per connection, a 256-datagram shared route, 1 MiB receive credit,
  1 MiB unsent bytes, 1,024 sent packets, a 1 MiB sent ledger, and 548-byte
  IPv4 datagrams.
- Endpoint capability state remains volatile and constant-size inside each of
  at most 1,000 existing per-torrent peer records. No new timer, cache, or
  retention rule follows from default enablement.
- One outgoing logical dial continues to own one record, generation,
  connection ID, peer-budget permit, cancellation token, and at most one live
  transport subattempt. TCP begins only after uTP joins to zero.
- Network policy and address-family checks remain authoritative before either
  transport. Default uTP cannot bypass offline, loopback, or IPv6 policy.
- Diagnostic output remains endpoint-free and bounded. No public endpoint,
  peer ID, payload, profile root, or packet log is committed.

## Source-First Record

Re-read managed BEP 29 at BitTorrent BEP commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`, especially `rationale`,
`overview`, and `connection setup`. It defines uTP's reliable UDP stream and
delay-based congestion behavior but does not prescribe product enablement,
TCP fallback, capability memory, mapping, or support-claim policy.

Re-inspected Rasterbar libtorrent commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `src/settings_pack.cpp` enables incoming/outgoing TCP and uTP by default;
- `src/torrent_peer.cpp::torrent_peer` assumes a fresh peer supports uTP;
- `src/torrent.cpp::torrent::connect_to_peer` selects uTP for assumed or
  confirmed support while retaining TCP as a separately enabled transport;
- `src/peer_connection.cpp::peer_connection::connect_failed` clears assumed
  support and schedules TCP only after the uTP connect failure, while
  `on_connection_complete` confirms uTP at transport establishment;
- `src/peer_list.cpp::add_peer` and `update_peer` refresh support from BEP 11's
  `pex_utp` flag;
- `src/torrent.cpp::dht_announce` uses DHT `implied_port` when incoming uTP is
  enabled, a reachability/advertisement behavior this tactical deliberately
  does not adopt;
- `test/test_utp.cpp` disables TCP to prove exact forced-uTP transfer; and
- `simulation/test_swarm.cpp::utp_only` and
  `simulation/test_metadata_extension.cpp::run_metadata_test` prove uTP-only
  swarm and metadata paths. The pinned suite does not isolate the product
  default plus sequential fallback, so RSTorrent retains its independent
  application cohort.

RSTorrent adopts default availability, fresh-peer preference, transport-level
confirmation, and TCP fallback, but retains its accepted endpoint suppression
and recovery, single-permit sequential owner, plaintext-only IPv4 policy, and
fixed MTU. It intentionally omits libtorrent's DHT implied-port behavior,
incoming reachability claims, broader address/encryption composition, and
mixed-mode bandwidth policy. No reference source, fixture, or vector is
copied.

The local JSTorrent sibling at
`9895410beeed6aff554053769bd006a3fbd373ef` has no uTP product behavior:

- `docs/archive/engine/legacy-migration/architecture_analysis.md` records BEP
  29 as missing because the engine relies on TCP; and
- `docs/archive/project/RELEASE-STATUS.md` records TCP-only as a known
  limitation.

There is no JSTorrent default or compatibility behavior to preserve. The new
Rust engine's already-proven application owner is the relevant product
boundary.

## Owner, Task, Cancellation, And Dependency Map

| Owner | Bounded work | Cancellation and termination |
| --- | --- | --- |
| `ApplicationConfig` | common `PreferUtp` default and explicit `TcpOnly` override | immutable for one application generation; no task, persistence, or live mutation |
| Desktop and Android constructors | inherit and assert the common policy | platform lifecycle continues to own one application open/shutdown pair |
| `SessionNetworkRuntime` | existing shared UDP, fixed uTP service, and incoming supervisor | shutdown joins uTP/admission before incoming peer and session UDP owners |
| `PeerSocketSet` / torrent generation | existing eligibility, endpoint memory, sequential fallback, and actual-transport result | one cancellation and peer permit span all subattempts; uTP joins before TCP |
| Diagnostic binaries | inherit the product default or opt out explicitly when their contract is TCP-only | existing process/application shutdown joins all owners |
| Controlled application cohort | default-policy uTP roles plus explicit TCP-only regression | whole-case timeout, child cleanup, exact content, and temporary-root removal |

Protocol values, peer capability transitions, and transport selection remain
runtime-independent. The application/session layer depends inward on those
owners; no platform, filesystem, socket, task, or test-harness type enters the
protocol or peer-registry layers.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Construction | durable/ephemeral defaults, explicit TCP override, and desktop/Android inheritance |
| Runtime | default startup/shutdown, cancellation, fallback, suppression/recovery, and terminal zero ownership |
| Controlled interop | pinned-libtorrent incoming uTP, outgoing uTP, and TCP fallback through the default application policy; explicit TCP/Fast/MSE regression |
| Platform | desktop and Android Rust tests plus both supported Android native target builds |
| Repository | formatting, workspace Clippy with warnings denied, and complete workspace tests |

No live or external-device run is authorized or needed. Tactical `132`'s dated
ordinary-swarm observation remains the live evidence supporting this decision.

## Staged Execution And Commit Plan

1. Commit this source-first tactical and make it the sole authoritative
   **Now**, without changing behavior.
2. Change the common default, add construction/runtime assertions, and make
   transport-isolated diagnostics explicitly select `TcpOnly`; commit.
3. Make the retained application interoperability cohort prove the inherited
   default and rerun the explicit TCP/Fast/MSE regression; commit any bounded
   harness changes with their evidence.
4. Run desktop/Android, native-target, focused engine/session, formatting,
   Clippy, and complete workspace gates.
5. Record exact evidence, graduate only the implemented BEP 29 subset to
   **Partial**, reconcile the campaign/readiness/restart topics, and commit.

The tactical stops after the bounded default and claim land. Reachability,
advertisement, presentation, MSE-over-uTP, IPv6, racing, and dynamic MTU remain
separate future decisions.

## Execution Result

Commits `457ad3a` and `d3ca426` implement and stabilize the bounded change.

- Durable and ephemeral `ApplicationConfig` construction now selects
  `PreferUtp`. Desktop, Android, gateway, and application CLI consumers inherit
  that single value without platform overrides.
- The application lifecycle test starts the inherited service, observes its
  active owner, and closes with zero connections, half-opens, queued datagrams,
  admission tasks, or worker panics. The same test proves that an explicit
  `TcpOnly` override starts no uTP service.
- The incoming-seed diagnostic now uses the product default unless
  `--tcp-only` is passed. The application integration cohort therefore proves
  inherited policy rather than an opt-in flag. Transport-isolated TCP, Fast,
  MSE, and throughput diagnostics explicitly select `TcpOnly`.
- The implementation adds no setting, schema, generated contract, migration,
  socket, task, dependency, unsafe code, or presentation surface. Existing
  profiles adopt the construction default when their next application
  generation opens.
- Two saturation tests were made deterministic without weakening their
  production assertions: the fallback/recovery socket case reserves both TCP
  and UDP port namespaces and drains stale SYNs, while the incoming queue case
  sends its bounded burst synchronously before workers can drain it.

The exact implemented BEP 29 subset is now **Partial**. It covers first-party
fixed-548 IPv4/plaintext uTP, ordinary application listening and preferred
outgoing selection, endpoint capability memory, actual transport views, and
joined sequential TCP fallback. Persisted policy, UDP mapping, tracker/DHT
incoming-endpoint advertisement, public incoming reachability, IPv6,
MSE-over-uTP, racing, portable per-datagram fragmentation protection, dynamic
product MTU, and repeatable WAN-cohort evidence remain outside the claim.

## Recorded Evidence

- The pinned-libtorrent `2.0.13.0` application cohort transferred and
  independently hash-verified the exact 2,097,883-byte fixture with SHA-1
  `cdce24126a8e65854d876c0b83ad3ba19748f6dc` in all three roles. Default
  incoming uTP completed in 1.374587 seconds with one incoming uTP peer and
  4,097 application uTP datagrams. Default outgoing uTP completed in 0.290656
  seconds with one outgoing uTP peer and 900 datagrams. The TCP-only fallback
  sent three uTP datagrams, recorded two retransmissions, joined that worker,
  exposed one final outgoing TCP peer, and completed in 5.339730 seconds.
  Both uTP roles retained the fixed 548-byte MTU; every case ended cleanly with
  zero worker panics and exact temporary-root cleanup.
- The explicit TCP-only application regression passes ordinary initiated TCP,
  accepted Fast TCP, RSTorrent-to-RSTorrent Fast TCP, and forced MSE with no
  uTP owner.
- `cargo test -p rstorrent-session` passes 228 library tests with two ignored,
  two throughput-profile tests, one incoming-seed test, and four CLI tests.
  `cargo test -p rstorrent-desktop` passes four tests, and
  `cargo test -p rstorrent-android` passes eight tests.
- `clients/android/build.sh` passes the supported
  cargo-ndk x86_64 and arm64-v8a release builds, host UniFFI generation,
  `assembleDebug`, and `testDebugUnitTest`. A raw Cargo cross-target attempt
  did not select the Android NDK compiler and was superseded by this documented
  project build path; no project compilation failed in the supported path.
- The coordinated-port recovery test passes five consecutive runs, the exact
  incoming saturation test passes ten consecutive runs, and the final engine
  library gate passes 484 tests with seven ignored.
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, and
  `cargo test --workspace` all pass after the final committed code. No web or
  generated application contract changed, so the web generation/typecheck
  gates do not apply. No public network, WAN, emulator, visible client, or
  physical device was used.

All stopping conditions are met. The campaign returns the authoritative queue
to Tactical `129`; broader uTP reachability and policy remain separate future
decisions.
