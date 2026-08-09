# Tactical 113: IPv6 Firewall Pinhole And Incoming Reachability

Status: Planned on 2026-08-08 and source-reconciled on 2026-08-09. Not
started. The split from Tactical `112` and the decision to write this slice
against read-only gateway evidence, deferring any mutating gateway action to
implementation time, were accepted in product discussion on 2026-08-08.

Topics: `incoming-reachability-and-seeding`, `protocol-support`,
`dht-discovery`, `tracker-discovery`, `performance-and-live-evidence`,
`application-view-api`, `client-surfaces`, `capability-readiness`

Dependencies: Tactical
[`112`](112-dual-stack-transport-and-ipv6-dht.md) is a hard prerequisite. It
provides the IPv6 listener, the probe-selected global unicast bind address, the
per-family advertised endpoint, and the `ipv6_enabled` policy this slice
extends. Completed Tactical
[`088`](088-upnp-mapped-external-tcp-seeding.md) established the bounded UPnP
IGD discovery, SOAP, lease, renewal, and joined-cleanup owner that this slice
reuses for a second service. Completed Tactical
[`092`](092-truthful-tracker-and-dht-peer-advertisement.md) established the
rule that a port is advertised only when the claim is truthful. Completed
Tactical
[`097`](097-live-client-settings-and-replaceable-session-generations.md)
established the generation-fenced mapping lifecycle that a pinhole must join.

## Decision And Motivation

Add IPv6 firewall pinhole control through the UPnP IGD v2
`WANIPv6FirewallControl:1` service, distinguish a bound IPv6 listener from an
unfiltered gateway and a gateway-accepted pinhole without conflating any of
them with observed reachability, and prove an actual off-network IPv6 peer
transferring verified payload into RSTorrent.

Three forces select this slice, and one of them is a measurement:

1. **A bound IPv6 socket is not reachable, and this was measured, not
   assumed.** On 2026-08-08, unsolicited inbound IPv6 was dropped in both
   directions between the development host and the off-LAN validation host, for
   TCP and for ICMPv6, with the development host's application firewall
   disabled. On that measured network, Tactical `112` therefore lands an IPv6
   listener that no Internet peer can reach. This slice is what makes that
   listener useful there.
2. **IPv6 reachability is the point of IPv6 for BitTorrent.** BEP 32's opening
   rationale is that NAT makes an increasing number of peers unreachable and
   IPv6 fixes it. A dual-stack client that is outbound-only on IPv6 has taken
   on the protocol's cost without its benefit: it still cannot be dialled, so
   it still contributes nothing to the connectable-peer population.
3. **The mechanism exists on the validation network and its precondition is
   already satisfied.** A read-only inspection recorded below found the full
   pinhole action set advertised, with the gateway reporting
   `InboundPinholeAllowed = 1`.

An IPv4-style port mapping is *not* what IPv6 needs. There is no address
translation: the host already owns a globally routable address. The only thing
in the way is a stateful firewall, and IGD v2 models that as a pinhole with a
lease rather than as an external-to-internal port mapping. Reusing the
`WANIPConnection:2` vocabulary would be wrong, and this slice keeps the two
mechanisms distinct in state, diagnostics, and product wording.

## Stopping Condition

This tactical is complete when all of the following hold:

1. One session reachability coordinator owns at most one IPv6 pinhole beside at
   most one IPv4 port mapping, sharing the existing generation fence, task
   ownership, and joined cleanup, without conflating the two mechanisms.
2. The client discovers `WANIPv6FirewallControl:1`, reads firewall status,
   creates a finite-lease pinhole for the actual IPv6 listener endpoint, renews
   before expiry, and deletes it on clean shutdown.
3. A gateway that reports `FirewallEnabled = 0` produces typed unfiltered
   state and does not attempt `AddPinhole`, because the specification says
   inbound and outbound traffic are then allowed. A firewall that is enabled
   but disallows pinholes, an absent service, and an authorization or protocol
   failure remain distinct bounded states.
4. IPv6 advertisement retains Tactical `112`'s listener-backed
   `GlobalUnicast` eligibility and additionally distinguishes `Unfiltered` and
   `Pinholed`. Only the latter two claim gateway evidence; none claims an
   observed incoming connection. Port `1` remains reserved for no eligible
   listener, stopped/stopping state, or disabled IPv6 rather than being used to
   infer that a gateway without this optional service blocks traffic.
5. A controlled peer outside the local network dials the pinholed IPv6 endpoint
   and hash-verifies exact payload; independent query proves the pinhole
   existed; deletion proves it is gone; and a post-delete dial fails.
6. The negative control is recorded: the same dial before the pinhole exists
   times out.
7. Deterministic, scripted-gateway, and physical evidence all pass and are
   recorded here.
8. `incoming-reachability-and-seeding.md`, `protocol-support.md`,
   `application-view-api.md`, `client-surfaces.md`, and
   `capability-readiness.md` state exactly which mechanism, service version,
   and network produced the evidence, and what it does not generalise to.

## Scope

### Pinhole protocol client

A bounded `WANIPv6FirewallControl:1` client beside the existing
`WANIPConnection:2` client in `crates/rstorrent-engine/src/port_mapping/`,
sharing SSDP discovery, device and service description parsing, URL resolution,
bounded HTTP, SOAP envelope construction, and typed fault handling.

Discovery remains one `upnp:rootdevice` search per generation. The resulting
device description may yield either or both service clients; this slice does
not add a service-specific SSDP owner. Device, SCPD, and control URLs retain
Tactical `088`'s no-proxy, no-redirect, bounded, same-SSDP-responder IPv4
literal policy. The new service does not turn gateway-supplied URLs into a
general LAN HTTP client.

Actions used:

| Action | Use |
| --- | --- |
| `GetFirewallStatus` | Precondition. `FirewallEnabled` and `InboundPinholeAllowed` decide whether to attempt anything at all. |
| `AddPinhole` | Create the pinhole for the actual IPv6 listener endpoint. Returns `UniqueID`. |
| `UpdatePinhole` | Renew at 75 percent of lease, matching the existing IPv4 renewal policy. |
| `DeletePinhole` | Ordered cleanup before listener shutdown. |
| `CheckPinholeWorking` | Optional post-traffic evidence where the gateway implements it; never a create or advertisement gate. A pre-traffic `709 NoTrafficReceived` is not pinhole failure. |
| `GetPinholePackets` | At most two harness diagnostics: one post-traffic count and one post-delete request expected to return `704`; never a control input or production polling loop. |

Service selection requires the specification's device-required action set:
`GetFirewallStatus`, `AddPinhole`, `UpdatePinhole`, `DeletePinhole`, and
`GetPinholePackets`. Absence of optional `GetOutboundPinholeTimeout` or
`CheckPinholeWorking` does not reject an otherwise usable service.

`GetOutboundPinholeTimeout` is deliberately not called. The specification
defines it as the lifetime of a firewall's automatically created *outbound*
pinhole for a five-tuple; it is not a hint or clamp for an inbound
`AddPinhole` lease.

`AddPinhole` takes `RemoteHost`, `RemotePort`, `InternalClient`,
`InternalPort`, `Protocol`, and `LeaseTime`. A wildcard `RemoteHost` (empty)
and `RemotePort` of `0` are required, because a BitTorrent listener must accept
from any peer. `InternalClient` is the probe-selected global unicast address
from Tactical `112` — not a private address, and not a wildcard. `Protocol`
is `6` (TCP). This is an IANA protocol number, not an IGD
`PortMappingProtocol` string, and mixing the two vocabularies is a specific
failure this slice tests for.

`LeaseTime` is `ui4` in the inclusive specification range `1..=86400`; this
slice ordinarily requests 3,600 seconds and renews with a finite value. If the
first `AddPinhole` response is lost, one reconciliation submission keeps the
same five-field pinhole identity but requests 3,601 seconds. That deliberate
lease difference invokes the specification's exact guarantee: a gateway that
already created the matching pinhole must extend it and return its existing
`UniqueID`; one that did not process the first request creates it normally.
If that response is also lost, the owner records one uncertain lease through
the later possible expiry and blocks another pinhole until then. Deterministic
and scripted tests cover this ambiguous-create path rather than assuming that
an identical lease request is idempotent or allocating a second logical
pinhole.

### Reachability coordinator

The existing coordinator gains a second mechanism slot rather than a second
coordinator. One generation-fenced owner holds at most one IPv4 mapping and at
most one IPv6 pinhole, each with its own lease, renewal timer, typed state, and
deletion path, and joins both before listener shutdown.

Pinhole state must not be reachable through the IPv4 mapping's state, and the
product must be able to show one succeeding while the other fails.

### Runtime and product projection

The generated `ClientSettingsRuntimeView` keeps its existing IPv4
`port_mapping_status` and gains a sibling bounded `ipv6_pinhole_status`; the
IPv4 enum is not widened with IPv6-only addresses or fault meanings. The new
status distinguishes disabled, ineligible, discovering, service/action
unavailable, unfiltered, creating, pinholed, renewal-failed, cleanup-failed,
failed, and stopping phases. Pinhole data carries the current internal IPv6
address and port, finite lease, and bounded fault detail, never a persisted
gateway ID or control URL.

The shared web connection-and-seeding section renders the two gateway
mechanisms independently and keeps `GlobalUnicast`, `Unfiltered`, `Pinholed`,
and observed incoming evidence visibly distinct. There is no new control:
`port_mapping` and `ipv6_enabled` remain the two inputs. Affected generated
TypeScript and UniFFI bindings are regenerated, but Android gains no new
Compose surface in this tactical.

Applying `port_mapping = upnp` means the coordinator has converged the policy,
not that every optional gateway service exists. Absence or refusal of the IPv6
service is reported by `ipv6_pinhole_status` and does not turn a successful
IPv4 mapping into a settings-application failure; existing IPv4 mapping
failure semantics remain unchanged.

### Advertisement

Tactical `112` publishes the IPv6 port under a `GlobalUnicast` scope, on the
precise argument that a real listener owns that port on a globally routable
local address while incoming reachability remains unverified. This slice keeps
that wire eligibility and refines its evidence instead of treating the absence
of one optional gateway API as proof that traffic is blocked:

- `GlobalUnicast`: listener-backed, gateway state unknown or pinhole
  unavailable;
- `Unfiltered`: `GetFirewallStatus` reported `FirewallEnabled = 0`, for which
  the normative service says no pinhole is needed; and
- `Pinholed`: `AddPinhole` returned a `UniqueID` for the current listener
  generation and its lease remains active.

All three may publish the listener port under Tactical `112`'s deliberately
narrow BEP 7 semantics. Only `Pinholed` claims an installed lease, only
`Unfiltered` claims the gateway reported filtering disabled, and only an
actual peer row claims an observed incoming connection. `CheckPinholeWorking`
and `GetPinholePackets` do not upgrade the control state before traffic.

### Policy

No new setting. The existing `port_mapping` policy governs gateway mutation:
enabling mapping permits both the IPv4 mapping and the IPv6 pinhole, and
disabling it permits neither. It does not silently become an IPv6 tracker/DHT
advertisement switch; a bound `GlobalUnicast` listener keeps Tactical `112`'s
wire behavior. `ipv6_enabled` from Tactical `112` independently gates the
IPv6 half. A gateway offering only one service yields only that mechanism,
with typed state for the other.

## Non-Goals

- **PCP and NAT-PMP, in either family.** They remain independent later slices
  and neither responded on the validation network when probed for Tactical
  `088`.
- **UDP pinholes.** There is no advertisable UDP capability yet; the DHT UDP
  endpoint is transport state, not a peer listener, and uTP does not exist.
- **IPv6 prefix-change handling.** A delegated prefix change invalidates the
  bound address, the node identity, and the pinhole at once. Tactical `112`
  defers address-change watching, and this slice inherits that deferral: the
  existing generation-replacement path is the only recovery.
- **Multiple simultaneous pinholes.** One TCP pinhole for one listener
  endpoint.
- **`WANIPv6FirewallControl` on a gateway that does not advertise it.** No
  fallback to manual firewall guidance, router-specific behavior, or
  vendor-specific services.
- **A general public-IPv6-reachability claim.** Evidence from one gateway on
  one ISP is evidence about that gateway.
- **Android or ChromeOS physical pinhole evidence.** Cellular networks have no
  addressable gateway, and the local-network permission path is separate work.

## Normative And Reference Dossier

### Specifications

The normative source is the UPnP Forum's
[`WANIPv6FirewallControl:1 Service — Standardized DCP, version 1.00,
December 10, 2010`](https://upnp.org/specs/gw/UPnP-gw-WANIPv6FirewallControl-v1-Service.pdf),
especially sections 2.4.2--2.4.10, 2.6.1--2.6.9, 3.4, and the XML service
description in section 4. That document is **not** in the pinned
`reference/bittorrent.org` checkout, which contains BEPs only, and is not
vendored.

The client also continues to use the Open Connectivity Foundation's
[UPnP Device Architecture
2.0](https://openconnectivity.org/upnp-specs/UPnP-arch-DeviceArchitecture-v2.0-20200417.pdf)
for the SSDP, URL, HTTP, SOAP, and Boolean wire rules already adopted by
Tactical `088`; this slice does not redefine those shared layers.

The normative document, not the validation gateway's mutable SCPD, owns the
wire contract, data ranges, action requirements, idempotent matching behavior,
and fault meanings. The live device and service descriptions remain required
discovery and implementation evidence and may expose a supported subset or
vendor behavior. `docs/references.md` gains the exact title, URL, revision
date, sections used, and non-vendored status when the capability lands. No
specification prose or source is copied.

Three normative details materially constrain the implementation:

- `FirewallEnabled = 0` means all inbound and outbound traffic is allowed by
  the IGD and no pinhole is needed; it is not an outbound-only failure state.
- `GetOutboundPinholeTimeout` describes automatically created outbound
  pinholes and is unrelated to the `1..=86400` second inbound lease.
- IGD v2 recommends restricting unauthenticated operations to
  `InternalPort >= 1024` and an `InternalClient` equal to the control point's
  IP address. The listener port already satisfies the first rule. Control-
  transport source selection and `606 Action not authorized` therefore need
  first-class evidence rather than assuming an IPv4 SOAP connection can
  authorize an arbitrary IPv6 internal client.

BEP 7 remains the reason the listener port is announced for the selected local
address. Where the gateway firewall is enabled, an accepted pinhole supplies
additional gateway evidence; it does not redefine the listener-backed
advertisement rule established by Tactical `112`.

### Pinned libtorrent: a recorded absence

Revision `7d7fc38fac61177fa5e02148f791b2f65250b09d` was searched. Neither
`pinhole` nor `WANIPv6FirewallControl` appears anywhere in the tree.
`src/upnp.cpp` and `src/natpmp.cpp` are IPv4-only, and `listen_socket_t`
carries port mappings only through the `natpmp` and `upnp` transports
(`include/libtorrent/aux_/session_impl.hpp:249-275`).

The adjacent oracle surface and its tests were still inspected rather than
treating the search result as sufficient: `include/libtorrent/upnp.hpp`
(`add_mapping`, `delete_mapping`, mapping/lease state), `src/upnp.cpp`
(`create_port_mapping`, `on_upnp_map_response`, renewal, delete, and the
50-mapping ceiling), and `test/test_upnp.cpp` (`upnp_wanipconnection2`,
`upnp_max_mappings`, content types, add/delete lifecycle). Every one exercises
`WANIPConnection` mappings; none supplies an IPv6-firewall wire or lifecycle
case to adopt.

**The campaign's required completeness oracle does not implement this
capability.** That has three consequences this slice accepts explicitly:

- every deterministic test here is independently authored, with no oracle to
  cross-check edge cases against;
- the required depth of hostile-input and lease-lifecycle testing is higher
  than it would be for an oracle-backed feature, because a missed case will not
  be caught by comparison; and
- the structural design borrows from Tactical `088`'s IPv4 mapping owner, which
  *was* oracle-guided, rather than from a reference implementation of this
  service.

`reference/rqbit` at `4e5f94cbcf1d57ec500885c77cf1e24d70232d89` has
`crates/upnp/src/lib.rs`, which periodically invokes `WANIPConnection:1`
`AddPortMapping` and has no pinhole service, status, ID, update, delete, or
fault lifecycle to adopt.

The first-party JSTorrent checkout at revision
`9895410beeed6aff554053769bd006a3fbd373ef` does have IPv4 gateway control:
`packages/engine/src/port-mapping/gateway-device.ts` implements
`WANIPConnection:1`/`WANPPPConnection:1` add and delete,
`port-mapping-manager.ts` owns a 3,600-second lease, renewal, and cleanup, and
the client keeps `upnpStatus` distinct from
`hasReceivedIncomingConnection`. That product distinction is adopted here.
JSTorrent contains no `WANIPv6FirewallControl` or pinhole path, so its regex
XML parsing, origin-concatenated control URL, subnet fallback, Boolean-only
errors, and interval ownership are not protocol precedents for this service.
There is no first-party or third-party implementation precedent available for
the pinhole mechanism itself.

### Read-only gateway inspection, 2026-08-08

A non-mutating inspection established the target, mirroring the method Tactical
`088` used before implementing IPv4 mapping:

- SSDP `M-SEARCH` for `urn:schemas-upnp-org:service:WANIPv6FirewallControl:1`
  received a response from the default IPv4 gateway, which also answers as
  `urn:schemas-upnp-org:device:InternetGatewayDevice:2`;
- the device description advertised three services:
  `WANCommonInterfaceConfig:1`, `WANIPConnection:2` — the service Tactical
  `088` already uses — and `WANIPv6FirewallControl:1`, with distinct control
  and SCPD URLs;
- the service description advertised the complete action set:
  `GetFirewallStatus`, `GetOutboundPinholeTimeout`, `AddPinhole`,
  `UpdatePinhole`, `DeletePinhole`, `GetPinholePackets`, and
  `CheckPinholeWorking`;
- a read-only `GetFirewallStatus` SOAP action completed successfully over IPv4
  transport and returned `FirewallEnabled = 1` and
  `InboundPinholeAllowed = 1`; and
- independently, unsolicited inbound IPv6 from the off-LAN host was dropped for
  both TCP and ICMPv6, and the development host's application firewall was
  confirmed disabled, establishing that the observed block is at the gateway.

**No mutating action was taken.** `AddPinhole` has not been exercised. The
gateway address, UUID, model, control URL, and prefix are evidence, not
configuration: the implementation must rediscover the device and service, and
none of those values may be hard-coded or persisted.

One implementation note follows directly from that inspection: this gateway
answers `WANIPv6FirewallControl` SOAP over IPv4 transport. The first slice
therefore retains Tactical `088`'s source-bound, same-responder IPv4 control
transport instead of weakening its URL safety policy to accept an unrelated
IPv6 host supplied by description XML. Scripted tests cover the observed IPv4
control path and typed `606` rejection when source-identity policy is not
satisfied. The document does not label the observed IPv4 control URL
non-conformant merely because it is IPv4. An IPv6-only control plane needs a
separately justified discovery, responder-correlation, and link-local scope
design and is deferred below.

### Off-LAN verifier

The off-LAN host is on a different ISP with native global IPv6 and working
egress, and runs Python 3.13. Its own gateway advertises only
`InternetGatewayDevice:1` with no IPv6 firewall-control service. That absence
is not treated as proof about its general reachability; no attempt is made to
open it through this mechanism because it is only the dialer and RSTorrent is
the listener.

The existing `tests/interop/off_lan_peer_wire.py` verifier is a self-contained
standard-library peer-wire client streamed to that host over SSH, already used
by `advertised_seeding.py --mapped-external` through
`RSTORRENT_OFF_LAN_SSH_TARGET`. This slice extends it to dial an IPv6 literal
and reuses the same destination-value discipline: the target and network
identities are never printed or persisted.

## Independently Written Wire Contract

Recorded here so a future gateway or specification change can be audited
against the exact normative version named above.

```text
Discovery   SSDP M-SEARCH
              ST: upnp:rootdevice
            -> LOCATION of the root device description
            -> <service> with serviceType WANIPv6FirewallControl:1,
               its own SCPDURL and controlURL

Status      GetFirewallStatus()
              -> FirewallEnabled (bool), InboundPinholeAllowed (bool)
            FirewallEnabled=false -> unfiltered; do not create.
            Proceed to AddPinhole only when both are true.

Create      AddPinhole(RemoteHost="", RemotePort=0, InternalClient=<our global
              unicast IPv6 address>, InternalPort=<listener port>, Protocol=6,
              LeaseTime=<finite seconds>) -> UniqueID (u16)

Verify      CheckPinholeWorking(UniqueID) -> IsWorking (bool)
              [optional, after traffic]

Renew       UpdatePinhole(UniqueID, NewLeaseTime)

Delete      DeletePinhole(UniqueID)

Diagnose    GetPinholePackets(UniqueID) -> PinholePackets (u32)
              [post-traffic and post-delete harness evidence]
```

Six properties are easy to get wrong and are called out as required behavior:

- **`Protocol` is an IANA protocol number, not an IGD protocol string.** TCP is
  `6`, not `"TCP"`. The `WANIPConnection` service in the same device uses the
  string form, so the two clients sit next to each other with incompatible
  encodings for the same concept.
- **`InternalClient` must be the routable global address**, because there is no
  translation. Sending a private or wildcard address is a category error
  carried over from IPv4 mapping thinking.
- **`UniqueID` is gateway-assigned and volatile.** It is runtime state, never
  persisted, and never assumed stable across a gateway reboot. After process
  loss there is no tuple query that can recover it; the finite lease bounds the
  orphan until expiry before a later generation creates another pinhole.
- **The five-field `AddPinhole` identity is conditionally idempotent.** A
  matching existing pinhole with a *different* lease must be extended and
  return its existing `UniqueID`. The one bounded reconciliation retry uses
  3,601 rather than 3,600 seconds so a timed-out create actually exercises
  that guarantee.
- **`CheckPinholeWorking` is optional and traffic-sensitive.** A missing action
  or a `709 NoTrafficReceived` fault is recorded as unknown, never as create
  failure, and never blocks advertisement. It is attempted only after the
  controlled peer has generated traffic.
- **UPnP booleans are not only `0` and `1`.** The parser accepts the Device
  Architecture's allowed input forms (`0`/`1`, `true`/`false`, and deprecated
  `yes`/`no`) while emitting canonical numeric values in scripted responses.

## Owner, Task, And Data-Flow Map

```text
      ClientSettings.port_mapping        ClientSettings.ipv6_enabled
                    |                              |
                    +--------------+---------------+
                                   v
               session reachability coordinator (one owner,
               one generation fence, one joined cleanup)
                                   |
              +--------------------+--------------------+
              v                                         v
   IPv4 mapping slot                         IPv6 pinhole slot
   WANIPConnection:2                         WANIPv6FirewallControl:1
   AddPortMapping / Delete                   AddPinhole / Update / Delete
   3600s lease, renew at 75%                 finite lease, renew at 75%
   external addr may differ                  no translation; our own address
              |                                         |
              +--------------------+--------------------+
                                   v
                    per-family advertised endpoint
                    v4: Mapped | LocalNetwork | 1
                    v6: GlobalUnicast | Unfiltered | Pinholed | 1
                                   |
                     tracker and DHT announces, per family
```

Ownership rules this slice must not violate:

- **One coordinator, two mechanism slots.** No second coordinator, no second
  generation counter, no second shutdown path.
- **Mechanism independence.** An IPv4 mapping failure must not prevent an IPv6
  pinhole, and vice versa. The product must be able to show one active and one
  failed simultaneously.
- **The listener is the authority.** A pinhole is created only for an actual
  bound, accepting IPv6 listener endpoint from the current generation, and is
  invalidated before that listener can change.
- **Deletion is ordered before listener shutdown**, matching the IPv4 sequence
  Tactical `088` proved.
- **Nothing is persisted.** No `UniqueID`, no gateway identity, no control URL,
  no lease. Crash recovery is lease expiry plus rediscovery.
- **A pinhole is not evidence of a connection.** The `Pinholed` scope means the
  gateway accepted the request, and the product must still distinguish that
  from an observed incoming peer.

## Resource And Failure Bounds

| Resource | Bound |
| --- | --- |
| Pinholes | At most one owned logical TCP pinhole for one listener endpoint; any unobservable gateway orphan after process/response loss is bounded by the finite lease and never represented as verified state |
| Create submissions | Per process generation, one ordinary `AddPinhole` plus at most one same-identity, different-lease reconciliation; no further create while either result may still own a lease |
| Tasks | Zero new long-lived tasks; the existing coordinator task drives both mechanisms |
| SSDP discovery | Shared with the existing IPv4 discovery, same bounds, one search per generation |
| SOAP request and response bytes | The existing bounded HTTP/XML limits, unchanged |
| Lease | Finite, ordinarily 3,600 seconds; the one ambiguous-create reconciliation uses 3,601; both are within `1..=86400` and renew at 75 percent of the active request |
| Renewal failures before the pinhole is treated as lost | The existing IPv4 renewal policy, unchanged |
| `CheckPinholeWorking` calls | At most one per created pinhole and only after controlled traffic; zero in the ordinary production control path |
| `GetPinholePackets` calls | At most two harness calls per created pinhole: one post-traffic value and one post-delete `704`; zero in the ordinary production control path |
| Persisted state | None |

Failure rules:

- Absent service, absent required action, `FirewallEnabled = 0`,
  `InboundPinholeAllowed = 0`, and `606 Action not authorized` each yield
  distinct typed state. `FirewallEnabled = 0` selects `Unfiltered`; the other
  unavailable outcomes retain listener-backed `GlobalUnicast` advertisement
  without claiming gateway reachability.
- A SOAP fault on create, update, or delete is typed and bounded. A declared
  create fault is not retried within the generation; only a transport-ambiguous
  first response permits the one reconciliation submission after the existing
  bounded HTTP pause.
- A timed-out create receives exactly one reconciliation submission with the
  same identity tuple and a different finite lease. If its response is also
  lost, the latest possible expiry is recorded as an uncertain pinhole; a
  changed listener generation fences either response and no new pinhole begins
  until that uncertainty expires.
- A gateway that returns a `UniqueID` and then reports it unknown on renewal is
  treated as pinhole loss: state reverts from `Pinholed` to `GlobalUnicast` and
  creation is reattempted under backoff.
- A transport-ambiguous update retains the known ID for bounded retry and
  records the latest possible expiry of every submitted finite renewal. The
  status ceases to claim `Pinholed` after the last confirmed lease expires,
  but no replacement is created until the latest possible lease has expired or
  `704` proves the old ID absent. An ambiguous delete follows the same
  uncertain-lease block.
- Every hostile-input rule already applied to gateway XML and SOAP responses
  applies unchanged; none is relaxed for the new service.

## Implementation Gates

Each gate is independently committable and leaves the workspace green.

1. **Shared discovery and protocol boundary.** Refactor the existing bounded
   `upnp:rootdevice` result into independent IPv4-mapping and IPv6-firewall
   service clients without duplicating discovery or weakening URL validation.
   Add independently authored codec, fault, action-inventory, and scripted
   lifecycle tests, including the different-lease ambiguous-create path.
2. **Coordinator and product state.** Add the second generation-fenced slot,
   independent lease/uncertainty state, ordered joined cleanup, generated
   application contract, web status projection, and simultaneous
   success/failure tests. This gate performs no live gateway mutation.
3. **Controlled physical evidence and graduation.** Under the repository's
   explicit live-evidence opt-in, re-run the negative control, create one
   finite pinhole, transfer and hash-verify payload from the off-LAN peer,
   query the packet count, delete, prove `704`, and repeat the failed dial.
   Record bounded evidence and update every owning topic before graduation.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Codec | `AddPinhole`, `UpdatePinhole`, `DeletePinhole`, `GetFirewallStatus`, `CheckPinholeWorking`, and `GetPinholePackets` envelope construction and response parsing; `Protocol=6` asserted numeric; `InternalClient` asserted to be the bound global unicast address; wildcard `RemoteHost`/`RemotePort` asserted; all accepted boolean spellings parsed; lease and `UniqueID` integer ranges enforced; malformed, truncated, oversized, and faults `606` and `701..=709` rejected with typed errors |
| State transitions | Full lease lifecycle including create, lost create response followed by same-identity/different-lease recovery, two lost responses producing one finite uncertain lease, post-traffic optional verification, renew at 75 percent, ambiguous update with confirmed-versus-latest-possible deadlines, renewal failure, gateway-reported unknown `UniqueID`, ambiguous delete, delete, and delete-after-loss; listener generation change invalidating active or uncertain state before stale `Pinholed` state can survive |
| Precondition handling | `FirewallEnabled = 0` produces `Unfiltered` without `AddPinhole`; `InboundPinholeAllowed = 0`, service absent, required action absent, `606`, and control URL unreachable each produce distinct typed state while retaining `GlobalUnicast` listener advertisement |
| Mechanism independence | IPv4 mapping succeeds while the pinhole fails and vice versa; both active simultaneously; joined shutdown deletes both in order with terminal zero ownership |
| Scripted gateway | A scripted IGD v2 server exercising every used action, faults at every step, source-identity authorization failure, a lost 3,600-second create response that returns the same ID to the 3,601-second reconciliation, two lost responses producing bounded uncertainty, and a gateway that returns a `UniqueID` it then disowns |
| Advertisement | `GlobalUnicast`, `Unfiltered`, and `Pinholed` each publish the real IPv6 listener port while exposing distinct evidence; none exists without the listener generation; tracker and DHT announces carry that IPv6 port and the independently selected IPv4 port in the same session; only an observed incoming peer upgrades reachability evidence |
| Client | Generated contract and runtime validation preserve the IPv4 mapping status and add the bounded IPv6 pinhole status; the web surface shows simultaneous per-mechanism outcomes and does not describe `Pinholed` as an observed connection; absent optional service does not degrade an otherwise applied mapping policy |
| Physical, negative control | Before any pinhole exists, an off-LAN IPv6 dial to the listener times out. Already observed on 2026-08-08 and re-run as part of the gate. |
| Physical, positive | With the pinhole active: post-traffic `GetPinholePackets` returns a nonzero count; the off-LAN verifier dials the global IPv6 endpoint, completes the peer-wire handshake, and hash-verifies exact payload; RSTorrent's ordinary Peers and Swarm views show the incoming IPv6 generation with exact physical upload |
| Physical, cleanup | Joined shutdown deletes the pinhole; `GetPinholePackets` returns typed `704` for the deleted ID; a post-delete dial from the same off-LAN host fails; terminal zero tasks, sockets, and mapping owners |
| Resource | At most one owned logical pinhole and two create submissions for the same identity; finite uncertainty blocks replacement; no new long-lived task; no persisted gateway state; declared SOAP and SSDP bounds observed |

The harness asserts the exact listener address and published port in memory and
records the lease length, transferred and verified bytes, resource high-water
marks, terminal owner counts, and what the evidence does not prove. The
committed execution record does not retain the public address, gateway
identity, control URL, SSH target, or other network identity. Same-LAN
hairpinning is not evidence, matching Tactical `088`.

## What This Slice Will Not Claim

- That IPv6 incoming connections work on networks other than the one measured.
  One gateway, one ISP, one firmware.
- That `WANIPv6FirewallControl:1` is commonly available. It was found on one
  device; the off-LAN host's own gateway does not have it, which is a
  one-in-two miss rate in the only sample available.
- That an active pinhole means peers will connect. Advertisement, discovery,
  and swarm behavior are separate.
- Any PCP, NAT-PMP, UDP, or IPv4 capability beyond what Tactical `088` already
  proved.

## Deferred With Reason

- **PCP and NAT-PMP for IPv6.** PCP is the specified successor and handles both
  families, but neither responded on the validation network during Tactical
  `088`'s inspection. Source inspection alone is not a support claim.
- **Prefix-change and address-rotation recovery.** Inherited from Tactical
  `112`'s deferral of address-change watching. The trigger for promoting both
  is the same: observed identity or pinhole churn within a session.
- **A second pinhole for a future uTP or UDP endpoint.** Waits for an actual
  advertisable UDP capability.
- **Manual firewall guidance in the product.** A useful product affordance for
  gateways without the service, but a UI and documentation slice, not a
  protocol one.
- **Android and ChromeOS pinhole evidence.** No addressable gateway on
  the available cellular path, and the local-network permission path is
  separate work.
- **IPv6-only gateway control transport.** The observed service uses the
  already bounded IPv4 same-responder control path. Accepting IPv6 or scoped
  link-local URLs requires an explicit responder-correlation and interface
  scope design rather than weakening Tactical `088`'s URL safety invariant.

## Escalation And Next Boundary

Stop and ask for direction if any of the following occurs:

- `AddPinhole` succeeds but the off-LAN dial still fails, which would mean the
  gateway accepts and ignores pinhole requests and the mechanism is not
  actually usable on this network;
- the gateway requires IPv6 control transport that cannot be established from
  the bound address, or returns `606` on its only safely correlated control
  URL, which would couple this slice to an unplanned transport or
  authorization change;
- making the pinhole and the IPv4 mapping independent requires splitting the
  reachability coordinator rather than adding a slot; or
- adding the second mechanism would require weakening the existing
  same-responder URL policy or exposing a general LAN HTTP client.

## Execution Record

Not started. The read-only gateway inspection recorded above is the only
evidence gathered so far. No mutating gateway action has been taken.
