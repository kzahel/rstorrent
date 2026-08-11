# Tactical 140: Incoming uTP Reachability

Status: **Complete on 2026-08-11, with the bounded physical gate
evidence-limited.** Explicit maintainer direction selected this slice and
authorized end-to-end autonomous implementation, logical commits, the
generated first-party contract update, proportional Android evidence, and one
bounded off-LAN proof using the already established temporary testbed. Every
controlled stopping gate passes. The physical attempt budget ended with the
product TCP lease mapped but the product UDP lease still nonterminal; exact
cleanup was independently verified after every attempt, so no positive public
incoming-uTP claim follows. Source-ready Tactical
[`139`](139-incomplete-file-streaming-demand.md) remains unchanged for human
review; its implementation is not implied.

Topics: `utp-transport-campaign`, `incoming-reachability-and-seeding`,
`tracker-discovery`, `dht-discovery`, `application-view-api`,
`capability-readiness`, `oracle-driven-engine-campaign`, `protocol-support`

Dependencies: completed Tacticals [`088`](088-upnp-mapped-external-tcp-seeding.md),
[`089`](089-coordinated-session-listen-sockets.md),
[`092`](092-truthful-tracker-and-dht-peer-advertisement.md),
[`097`](097-live-client-settings-and-replaceable-session-generations.md),
[`125`](125-shared-udp-utp-runtime-and-loopback-interop.md),
[`127`](127-mapped-utp-wan-interoperability.md),
[`130`](130-utp-transport-solidification.md),
[`133`](133-utp-product-default-enablement.md), and
[`137`](137-product-utp-path-mtu-discovery.md) supply the session socket,
reachability, discovery, uTP product, mapped-WAN, and protected-send
foundations this slice composes.

## Decision And Desired Outcome

Make the ordinary IPv4 product uTP listener reachable and truthfully
discoverable outside its LAN when its gateway supports the existing UPnP
policy. The session maps the actual TCP and UDP listener ports independently,
publishes their independent state, keeps tracker announces tied to TCP, and
uses the active uTP UDP endpoint for IPv4 DHT `announce_peer`.

This is intentionally not a same-port design. A gateway may choose different
external ports for TCP and UDP. The truthful result in that case is:

```text
tracker port = verified external TCP port
IPv4 DHT port = verified external UDP port used by uTP
```

The DHT announce encodes that UDP port explicitly. It does not set
`implied_port`: the DHT query's source port need not equal a verified external
mapping, and explicit ownership remains diagnosable.

## Scope And Stopping Condition

This tactical owns one end-to-end slice:

1. extend the existing generation-fenced reachability coordinator to own one
   renewable TCP lease and one renewable UDP lease through one shared UPnP
   discovery result;
2. derive UDP eligibility only from the actual bound IPv4 session UDP socket
   that owns DHT and incoming uTP, never from a preferred or invented port;
3. expose independent TCP and UDP mapping runtime status without a new
   persisted setting or a second mapping policy;
4. retain tracker and BEP 10 listen-port advertisement as TCP facts while
   selecting the active IPv4 uTP UDP endpoint for DHT peer announcement;
5. keep TCP and UDP creation, renewal, expiry, delete uncertainty, replacement,
   and failure independent while preserving ordered discovery withdrawal;
6. prove different external ports through a scripted gateway and wire-level
   tracker/DHT observations;
7. prove DHT-only incoming uTP transfer against pinned libtorrent without a
   direct peer hint or TCP masking; and
8. prove one direct off-LAN incoming uTP transfer through a locally owned
   finite UDP mapping, followed by exact process, mapping, and artifact
   cleanup.

The tactical completes only when all of these conditions pass:

- at most one verified finite mapping exists per `TCP` and `UDP` protocol for
  the current session generation, and both target the concrete sockets they
  advertise;
- a TCP failure never retracts or prevents a healthy UDP lease, and a UDP
  failure never retracts or prevents a healthy TCP lease;
- an uncertain delete blocks only replacement of the same protocol mapping
  until its finite lease expires;
- mapped external ports may differ, tracker wire data still carries TCP, and
  IPv4 DHT wire data carries UDP;
- DHT withdrawal or port correction precedes deletion of the corresponding
  UDP lease, and late callbacks from an old generation change no state;
- a pinned-libtorrent peer obtains the advertised UDP endpoint through the
  controlled DHT route and completes the exact fixture over one uTP and zero
  TCP peer connections;
- both Android ABIs compile the same owner and generated contract, and the API
  34 application path proves bounded status/lifecycle and uTP behavior without
  adding an Android-only mapping runtime; and
- the bounded off-LAN run proves actual incoming UDP reachability and exact
  terminal cleanup, or records a typed environmental limitation after the
  declared attempt budget without weakening the controlled stopping gates.

## Stable Scenarios

The implementation preserves and extends these continuing scenarios:

- a product session with `Automatic` IPv4 listening and mapping `Upnp` binds
  concrete TCP and UDP endpoints, discovers a gateway once, and attempts both
  protocol mappings;
- a product session with mapping disabled, loopback listening, failed or
  absent IPv4 UDP service, or an ineligible address creates no UDP mapping and
  makes no mapped-uTP claim;
- local-network and direct global endpoints may still be advertised without a
  mapping under the existing scope rules, but mapped scope requires verified
  external address, port, internal client, internal port, enabled flag, and
  owned description;
- a settings replacement withdraws tracker/DHT discovery, drains registered
  incoming work, deletes each certain mapping, joins its one reachability
  owner, and only then replaces sockets;
- a gateway returning one TCP port and a different UDP port does not cause one
  protocol to impersonate the other; and
- IPv6 stays on the existing TCP advertisement plus independent firewall-
  pinhole contract because this slice does not add IPv6 uTP.

## Normative And Source Oracle

The public protocol basis is BEP 5 `announce_peer` and `implied_port`, BEP 15
tracker announce ports, BEP 29 uTP over UDP, and the UPnP IGD v2
`WANIPConnection:2` mapping operations already implemented by Tactical `088`.
BEP 5 permits an explicit announced port and defines `implied_port` as an
optional source-port shortcut. This slice uses the explicit verified port.

Pinned libtorrent `2.0.13` at commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected as the behavioral
oracle, not as source to copy:

- `include/libtorrent/aux_/session_impl.hpp`, `listen_socket_t`, retains
  independent `tcp_port_mapping` and `udp_port_mapping` slots and computes
  `tcp_external_port()` and `udp_external_port()` separately;
- `src/session_impl.cpp`, `session_impl::remap_ports()` and
  `session_impl::on_port_mapping()`, create and retain independent mappings
  against the concrete TCP and UDP sockets;
- `src/session_impl.cpp`, `session_impl::queue_tracker_request()`, selects the
  TCP listen/external port for trackers;
- `src/session_impl.cpp`, `get_listen_port()`, selects the UDP external port
  for DHT announcements;
- `test/test_upnp.cpp` and `test/test_natpmp.cpp` exercise independent TCP and
  UDP add/callback/delete ownership; and
- `test/test_session.cpp`, `reopen_network_sockets`, plus DHT test oracles in
  `test/test_dht.cpp` and `simulation/test_dht_rate_limit.cpp`, cover socket
  replacement and UDP listen-port selection.

The exact local JSTorrent checkout at commit
`9895410dc8155fce8399239ea793375342cc5d0c` was also inspected:

- `packages/engine/src/port-mapping/port-mapping-manager.ts` owns a list of
  finite TCP/UDP mappings with common discovery, renewal, and cleanup; and
- `packages/engine/src/core/bt-engine.ts` historically derives both the DHT
  bind and mapping as `port + 1` rather than consuming an authoritative bound
  UDP endpoint.

RSTorrent adopts the useful independent-protocol lease behavior and rejects
the invented-port shortcut. No reference source or fixture is copied.

## Owner, Task, Cancellation, And Data Flow

```text
SessionNetwork generation
  -> concrete TCP listener + concrete IPv4 SessionUdp socket
  -> one ReachabilityCoordinator task and cancellation token
       -> one bounded gateway discovery
       -> TCP renewable lease subowner -> TCP status/endpoint
       -> UDP renewable lease subowner -> UDP status/uTP endpoint
  -> task-free advertisement selector
       -> trackers and BEP 10 use TCP endpoint
       -> IPv4 DHT uses UDP/uTP endpoint
  -> existing tracker/DHT owners observe one fenced endpoint generation
```

The protocol values, endpoint selection, status transitions, and failure
classification remain runtime independent. The session reachability layer
owns gateway I/O, timers, cancellation, deletion, and joined termination. It
does not own sockets or peer tasks. The session network owner captures both
socket endpoints and tears discovery down before reachability and sockets.

## Resource And Security Bounds

- one reachability task, one cancellation token, and one shared discovery per
  session generation;
- at most two current IPv4 mapping leases, one per protocol;
- at most two finite uncertain-delete records, one per protocol;
- the existing 3,600-second lease, 75-percent renewal point, candidate-port
  bound, response byte bounds, timeouts, address checks, and installed-entry
  verification remain unchanged;
- no additional UDP socket, peer queue, DHT task, timer task, or long-lived
  gateway client;
- generated status exposes mechanism, external port, renewal, and bounded
  failure categories but no gateway URL, private address, public address,
  peer endpoint, capability, or testbed identity; and
- live evidence records only classifications, timings, counters, ports where
  required to prove protocol separation, and cleanup. Repository documents
  contain no SSH alias, public address, or private inventory value.

Metainfo, DHT, peer, and gateway input remains hostile. No remote packet may
select a local socket, mapping target, advertised port, allocation size, or
mapping lifetime.

## State And Compatibility Contract

The persisted `PortMappingPolicy` remains `Disabled | Upnp`. `Upnp` means map
each eligible concrete product listener transport; there is no user-facing
TCP-versus-UDP switch. The existing `port_mapping_status` remains the TCP
status for compatibility. An additive `udp_port_mapping_status` uses the same
bounded `PortMappingStatus` enum and defaults to `Disabled`/`Ineligible` by
the same eligibility contract.

The portable engine advertisement value gains independent DHT-family
selection without an async/runtime dependency. Its one monotonic generation
advances when either TCP/tracker or UDP/DHT truth changes. Existing callers
that need the peer TCP endpoint retain the current family values; DHT reads
the new family-specific announce value.

When no eligible IPv4 uTP endpoint exists, DHT does not borrow the TCP port to
make a false uTP claim. IPv6 retains the current TCP value. A healthy concrete
local IPv4 UDP endpoint may be advertised at `LocalNetwork` or equivalent
existing scope before UPnP succeeds, just as the TCP listener is today;
verified mapping upgrades only the UDP endpoint to `Mapped`.

## Shape-Changing Edge Cases

The common-path implementation includes:

- TCP success with UDP discovery/add/query/renew/delete failure and its
  converse;
- equal and unequal local TCP/UDP ports plus equal and unequal external ports;
- mapping renewal that changes only one external port;
- stale renewal, delete, timer, settings, socket, and gateway callbacks;
- one certain deletion and one uncertain deletion in either protocol order;
- UDP socket absent, ephemeral, replaced, or independently failed while TCP
  remains healthy;
- product uTP disabled or unavailable without changing DHT service ownership;
- DHT registration before mapping, port correction after verification, and
  ordered withdrawal at shutdown; and
- generated-client decoding of both legacy TCP status and additive UDP status.

## Staged Implementation And Commits

1. land this decision-complete tactical and select it as the sole **Now**;
2. extract/extend task-free endpoint selection and pure mapping state, with
   deterministic different-port and stale-generation tests;
3. compose the second protocol lease into the one session reachability owner,
   with scripted independent failure, renewal, uncertainty, and cleanup tests;
4. route tracker and DHT consumers to their transport-specific values and add
   controlled wire/discovery/forced-uTP evidence;
5. add the generated UDP mapping status through Rust, TypeScript, Kotlin, web,
   and Android compatibility tests;
6. run desktop and Android platform gates, then the authorized bounded off-LAN
   proof and independent cleanup audit; and
7. reconcile all owning topics, exact evidence, protocol claim, queue, and
   campaign checkpoint before the closing commit.

Each stage is a logical commit when its gate passes. Documentation may share
the commit whose implementation evidence it records.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure | eligibility, independent protocol state, equal/different ports, generation fencing, tracker/DHT selection, stopping |
| Scripted runtime | one shared discovery, two finite leases, independent add/query/renew/delete failure, uncertainty expiry, replacement, joined cleanup |
| Controlled wire | UDP/HTTP(S) tracker retains TCP port; DHT announces UDP port; corrected ports reannounce; port `1`/withdrawal remains truthful |
| Controlled interop | DHT-only pinned-libtorrent incoming transfer, exact fixture hash, one uTP/zero TCP peer, bounded queues and terminal owners |
| Generated/web | generated TypeScript/Kotlin matches, typecheck and web tests pass, UDP status is bounded and truthful |
| Android | both ABI builds plus API 34 no-window lifecycle/application evidence, no Android-only owner |
| Off-LAN opt-in | one locally mapped RSTorrent seed to established remote oracle leecher, ordinary public route, forced uTP, exact payload, mapping deletion and zero residue |
| Repository | format, warning-denying clippy, full workspace tests, focused interop, web tests, generated-contract cleanliness |

The off-LAN budget is one primary run and at most two diagnostic retries after
a concrete harness or implementation repair. Failure because the local
gateway lacks usable UDP UPnP is recorded as environmental evidence, but the
controlled gates still must pass. The remote side need not expose a mapping
because it is the outgoing leecher in this direction.

## Implemented Result

Commits `7a6a20e` through `9ee581b` implement and harden the slice:

- one task-free endpoint value retains independent tracker/TCP and DHT/uTP
  endpoints under one monotonic generation; trackers and BEP 10 keep the TCP
  value, IPv4 DHT uses the actual session UDP value, and IPv6 retains the
  existing TCP behavior because IPv6 uTP is out of scope;
- the one reachability coordinator discovers once and owns independent finite
  TCP and UDP lease state, renewal, failure, uncertainty, and cleanup. UDP
  eligibility comes only from the bound IPv4 session UDP socket;
- the additive `udp_port_mapping_status` crosses Rust, generated TypeScript,
  schema validation, React, UniFFI, and generated Kotlin while the existing
  `port_mapping_status` remains the compatible TCP fact;
- the product UI presents TCP and UDP/uTP status separately under the existing
  single UPnP policy, and diagnostic readiness can require a verified UDP
  lease without bypassing ordinary application ownership;
- the controlled advertised-seeding gate observes tracker-only TCP as a
  control, then obtains a different explicit endpoint through DHT and
  hash-verifies the exact fixture against pinned libtorrent over one uTP and
  zero TCP peer connections; and
- the physical harness owns the product process, remote outgoing peer,
  per-run artifacts, exact mapping inventory, and exact cleanup. A DHT
  identical-node bucket-distance underflow exposed by that path was repaired
  with an explicit zero-distance branch and a regression test.

No new task, socket, persisted policy, mapping protocol, or Android-only
runtime was introduced. The declared ceiling remains one reachability task,
one discovery, and at most one finite lease per TCP and UDP.

## Recorded Evidence

The controlled `advertised_seeding.py` run passes against pinned libtorrent
`2.0.13.0`: the tracker-only control uses TCP; the DHT-only case has no direct
peer hint, reports two `get_peers` observations, reaches a one-peer high water,
uses one uTP and zero TCP peer connections, and verifies the exact payload.
All 13 WAN-contract unit tests pass.

The generated web contract was regenerated cleanly; TypeScript typecheck and
247 web tests pass with two intentional skips. Both Android native ABIs build.
The API 34 no-window application gate observes the actual uTP listener and
dynamic IPv4 MTU mode, independent TCP/UDP `Disabled` mapping states on the
emulator network, joined shutdown, and zero terminal owners, mappings, tasks,
or panics. Android assembly and unit tests pass.

Formatting, warning-denying workspace clippy, the 237-test session library
gate with two ignored tests, and the 514-test engine library gate with seven
ignored tests pass. Two complete parallel workspace runs each exposed a
different pre-existing timing-sensitive assertion outside this slice: one
ephemeral TCP-port reuse assertion and one metadata-progress timeout count.
Each exact rerun and its complete owning crate then passed. This record does
not relabel those failed workspace invocations as green; a subsequent complete
workspace invocation passed.

The authorized physical budget comprised one primary run and two diagnostic
retries. The first harness version hid a readiness error after mapping TCP;
the repair made stderr and cleanup ownership explicit. The first retry exposed
and led to the DHT identical-ID repair. On the final retry the product started
without that panic, but UDP mapping did not become `Mapped` or `Failed` within
60 seconds even though TCP mapped. No remote peer or payload transfer was
started. Each exact finite TCP mapping was deleted and a fresh inventory found
zero owned TCP or UDP residue. Prior Tactical `130` evidence proves that this
gateway has established diagnostic UDP mappings, so the result is classified
as a current product/gateway interoperability limitation rather than a claim
that the gateway lacks UDP UPnP support.

The physical alternative in the stopping condition is therefore satisfied,
but public incoming uTP remains unproved. The implementation claim rests on
the deterministic, scripted, controlled-DHT, generated-client, desktop, and
Android evidence above.

## Non-Goals And Next Boundary

This tactical does not add PCP, NAT-PMP, BEP 55 hole punching, IPv6 uTP,
multi-interface binding, VPN/metered policy, a relay, a remote daemon, a new
mapping setting, ratio/time seeding goals, public-swarm support evidence, or a
complete BEP 29 claim. It does not implement Tactical `139`.

Trackers have only one BitTorrent peer port. They remain a truthful TCP
discovery path; this tactical does not claim that a differently mapped UDP
port is discoverable through tracker announces. DHT is the authoritative
incoming uTP discovery path in that case. A later slice may consider BEP 55,
NAT-PMP/PCP, IPv6 uTP, multi-address binding, or richer product presentation
from evidence after this slice closes.

## Escalation Contract

No review is required for internal refactoring, additive generated-contract
plumbing, conservative bound tightening, deterministic/scripted failures,
controlled local networking, Android builds/AVD use, or the bounded off-LAN
run and cleanup already authorized here.

Stop for human direction if evidence requires a new persisted/user-visible
policy, a new dependency or license posture, public-swarm participation,
changes to the external testbed beyond temporary per-run files/processes, a
destructive action, a complete protocol-support claim, or an architecture that
cannot preserve independent TCP and UDP truth.
