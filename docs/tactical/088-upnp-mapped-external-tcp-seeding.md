# Tactical 088: UPnP-Mapped External TCP Seeding

Status: Planned and authorized for autonomous implementation on 2026-08-05.

Topics: `incoming-reachability-and-seeding`, `client-persistence`,
`application-control`, `application-view-api`, `protocol-support`,
`peer-lifecycle`, `code-organization-and-refactoring`,
`capability-readiness`

Dependencies: completed Tacticals
[`078`](078-local-single-peer-tcp-seeding.md),
[`082`](082-bounded-multi-peer-upload-ownership.md),
[`084`](084-persisted-client-connection-and-seeding-settings.md), and
[`086`](086-long-lived-torrent-peer-runtime.md) establish the joined listener,
bounded upload owner, persisted settings waist, and ordinary incoming
Peers/Swarm projection used by this slice.

## Decision And Motivation

Prove one real externally reachable incoming TCP path before tracker or DHT
self-announcement. RSTorrent currently binds only IPv4 loopback, while UDP
tracker announces carry a provisional `6881` and DHT deliberately omits
`announce_peer`. A loopback endpoint cannot receive gateway-forwarded traffic,
and a local listening port is not by itself an authoritative public endpoint.

This tactical adds one explicit local-network listener mode, one
application-lifetime reachability coordinator, and the UPnP IGD v2 mechanism
observed on the current maintainer validation network. It graduates only when
a controlled peer on an independent external network dials the mapped public
endpoint and verifies exact torrent payload through the ordinary application
seeding owner.

The existing master topic remains the campaign owner. This tactical is one
bounded implementation slice rather than a new campaign or an amendment to
the completed local-seeding tacticals.

## Stopping Condition

All of the following must hold in one controlled run:

1. a durable, explicitly enabled local-network listener binds a concrete
   non-loopback IPv4 address and nonzero TCP port;
2. the session reachability coordinator discovers an IGD v2
   `WANIPConnection:2` service, obtains its external IPv4 address, requests a
   finite TCP mapping for that exact listener, and independently queries the
   installed entry;
3. a maintainer-controlled off-LAN verifier connects directly to the mapped
   public endpoint, completes a BitTorrent handshake, downloads every piece,
   verifies every SHA-1 piece hash and the exact whole-payload digest, and uses
   neither an SSH tunnel nor an overlay-network address for payload traffic;
4. ordinary RSTorrent Peers/Swarm observations show the external incoming
   peer, truthful flags and endpoints, nonzero requests, and exact physical
   upload accounting;
5. shutdown withdraws mapping eligibility before closing the listener,
   deletes the exact mapping, independently observes that it is absent, makes
   a later off-LAN connection fail, joins every mapping/listener/peer task, and
   leaves all declared owner counts at zero; and
6. deterministic, scripted-gateway, workspace, product-contract, and
   controlled interoperability gates pass with the declared bounds.

An SSDP response, successful SOAP status, mapping-table entry, same-LAN NAT
hairpin, TCP connect without BitTorrent payload, or source inspection alone is
not sufficient.

## Scope

- Add restart-applied local-network listener variants without changing the
  existing default-disabled policy or silently broadening loopback variants.
- Persist an explicit automatic port-mapping policy. Existing and migrated
  profiles remain mapping-disabled unless the user selects the new policy.
- Keep configured intent, concrete local bind, mapping state, mapped external
  endpoint, and observed incoming success distinct.
- Add a session `ReachabilityCoordinator` whose generation is tied to the
  application listener generation and whose task is cancelled and joined.
- Implement the IPv4 UPnP control-point subset required by the observed IGD v2
  path: SSDP M-SEARCH, bounded device and service-description retrieval,
  `WANIPConnection:2` selection, `GetExternalIPAddress`,
  `GetSpecificPortMappingEntry`, `AddPortMapping`, renewal, and
  `DeletePortMapping`.
- Use a one-hour requested lease and renew at three quarters of the granted or
  requested duration, following the pinned libtorrent default and renewal
  timing. Treat a permanent-lease-only response as typed failure in this
  tactical rather than silently leaving durable router state.
- Publish bounded runtime mapping status through the existing client-settings
  application contract. Product presentation may expose only the semantic
  state needed to operate and diagnose this slice.
- Add an opt-in, generic off-LAN verifier configuration. No committed file,
  test output, fixture, diagnostic, or commit message may contain a private
  machine name, SSH alias, private machine path, router UUID or serial number,
  DNS name, credential, or observed public address.

## Non-Goals

- Tracker-port replacement, DHT `announce_peer`, BEP 10 listen-port
  advertisement, LSD, PEX, or any public-swarm support claim.
- PCP, NAT-PMP, UPnP IGD v1/WANPPP compatibility, IPv6 firewall pinholes,
  uTP/UDP mappings, multiple simultaneous mappings, or protocol fallback.
- Dynamic live application of settings, automatic rebind after interface
  replacement, multi-interface ranking, VPN/metered-network policy, Android
  local-network permission, or platform firewall configuration.
- A permanent mapping fallback, router administration, manual port forwards,
  DNS publication, remote daemon, relay, proxy, or tunnel.
- General-purpose XML, HTTP, SSDP, SOAP, or UPnP framework APIs.
- Copying source, fixtures, or test data from libtorrent or JSTorrent.

## Normative And Reference Dossier

### Specifications

- Open Connectivity Foundation, [UPnP Device Architecture
  2.0](https://openconnectivity.org/upnp-specs/UPnP-arch-DeviceArchitecture-v2.0-20200417.pdf):
  IPv4 addressing, SSDP discovery, device-description URL handling, HTTP
  control, SOAP 1.1 action and fault envelopes, and version compatibility.
- Open Connectivity Foundation, [Internet Gateway Device v2 resource
  set](https://openconnectivity.org/developer/specifications/upnp-resources/upnp/internet-gateway-device-igd-v-2-0/):
  `InternetGatewayDevice:2`, `WANConnectionDevice:2`, and
  `WANIPConnection:2` service relationships and action contracts.
- Open Connectivity Foundation, [InternetGatewayDevice:2 Device Template
  1.01](https://openconnectivity.org/wp-content/uploads/2015/11/UPnP-gw-InternetGatewayDevice-v2-Device-20100910.pdf):
  public/basic access expectations and the WAN connection service inventory.

The implementation independently summarizes and tests the required behavior;
it does not copy specification prose or XML fixtures.

### Pinned libtorrent oracle

Revision `7d7fc38fac61177fa5e02148f791b2f65250b09d` from
[`reference/pins.toml`](../../reference/pins.toml) was inspected:

- `reference/libtorrent/src/upnp.cpp`: `start`,
  `discover_device_impl`, `resend_request`, `on_reply`, `on_upnp_xml`,
  `get_ip_address`, `create_port_mapping`, `on_upnp_map_response`,
  `delete_port_mapping`, `on_upnp_unmap_response`, `on_expire`, and `close`;
- `reference/libtorrent/include/libtorrent/upnp.hpp`: mapping identity,
  per-device state, external-port result, lease and specific-port capability,
  and UPnP fault vocabulary;
- `reference/libtorrent/src/settings_pack.cpp`: the 3,600-second UPnP lease
  default;
- `reference/libtorrent/test/test_upnp.cpp`: scripted IGD v1, WAN IP v1,
  WAN IP v2 add/delete flows, the mapping ceiling, and accepted SOAP content
  types;
- `reference/libtorrent/test/test_xml.cpp` plus `root1.xml`, `root2.xml`, and
  `root3.xml`: URL base/control URL selection, WAN IP/PPP and v2 service
  parsing, SOAP fault/external-address extraction, and malformed XML cases.

Adopted behavior:

- discover `upnp:rootdevice` rather than assuming one IGD search-target
  version;
- bind discovery and HTTP control to the selected local interface;
- deduplicate and cap discovered devices and fetch device descriptions before
  mapping;
- accept case-insensitive UPnP XML content types and non-200 SOAP fault bodies;
- treat the external port as derived mapping state;
- request a 3,600-second lease and renew at 75 percent;
- serialize operations per device, cap retries, and delete on joined close;
- retain typed common errors including 714, 718, 724, 725, 726, and 727.

Intentional differences:

- this slice accepts only the observed IPv4 HTTP IGD v2
  `WANIPConnection:2` path; libtorrent's v1, WANPPP, HTTPS, multi-device,
  NAT-PMP, and PCP breadth remains unclaimed;
- a `725` permanent-lease-only response does not automatically retry with
  lease zero because the live-evidence safety contract forbids creating a
  potentially orphaned permanent mapping unattended;
- a conflict does not replace an unrelated entry. The client queries first
  and may select another unoccupied high port only under a fixed retry bound;
- the installed entry is queried after add and before the endpoint becomes
  advertisable; libtorrent treats the successful add response as sufficient;
  and
- RSTorrent keeps deterministic codecs/state separate from Tokio sockets and
  owns cancellation with joined Rust tasks rather than libtorrent's single
  network-thread callback object.

Libtorrent is BSD-3-Clause and is used only as a source and executable
interoperability oracle. No source or fixtures are imported.

### JSTorrent product history

The first-party `main` checkout was inspected:

- `packages/engine/src/port-mapping/ssdp-client.ts` owns a three-second SSDP
  search and location deduplication;
- `gateway-device.ts` owns device retrieval, service selection, SOAP add,
  delete, external-address lookup, and generic mapping enumeration;
- `port-mapping-manager.ts` owns local-interface selection, one-hour leases,
  half-lease renewal, and cleanup; and
- the client settings surface exposes enablement and coarse discovery/mapping
  status.

Useful product lessons are explicit enablement, status visibility, one
coordinator, finite leases, renewal, and clean removal. RSTorrent deliberately
does not adopt JSTorrent's IGD-v1-only search/service lists, regular-expression
XML parsing, assumed `/24` selection, first-interface fallback, origin-only
control URL construction, silent Boolean errors, lack of installed-entry
verification, or absent UPnP tests. Those shortcuts would fail or weaken the
observed IGD v2 target.

JSTorrent is MIT and is used only for behavioral and architectural study. No
source or fixtures are imported.

### Observed validation network

Non-mutating inspection on 2026-08-05 established:

- the default IPv4 gateway answers SSDP as `InternetGatewayDevice:2` and
  advertises `WANIPConnection:2`;
- the bounded device and service descriptions advertise external-address,
  specific/generic lookup, `AddPortMapping`, `AddAnyPortMapping`, delete, and
  mapping-range actions;
- a read-only `GetExternalIPAddress` SOAP action succeeds;
- the returned IPv4 address is globally routable and matches an independent
  Internet-side observation, so no additional IPv4 CGNAT layer was observed;
  and
- three bounded NAT-PMP and three PCP probes received no response.

This selects UPnP IGD v2 as the first mechanism. Runtime discovery remains
generic within the tactical's protocol bounds; no observed address, device
identity, model, path, or external endpoint is committed or hard-coded.

## Accepted Settings And Product Semantics

`ListenerPolicy` gains restart-applied automatic and fixed-port
local-network IPv4 variants. The existing loopback variants retain exact
behavior. Automatic selects an OS port; fixed continues to require
`1024..=65535`. The local address is selected by the operating-system route to
the SSDP multicast destination, must be concrete IPv4 unicast and
non-loopback, and is published as runtime state rather than persisted.

`PortMappingPolicy` is `Disabled` or `Upnp`. It defaults to `Disabled` for new
and migrated profiles. UPnP is eligible only when the active listener is a
concrete non-loopback IPv4 endpoint and the active policy is `Upnp`. A
loopback listener plus `Upnp` is a valid but ineligible configuration and must
not emit SSDP traffic.

The runtime projection distinguishes at least disabled, ineligible,
discovering, mapping, mapped, unavailable/failed, renewal-failed, and stopping
states. A mapped state includes mechanism, external address and port, local
address and port, and lease duration/age without persisting those facts.
Bounded diagnostics exclude raw XML, full URLs, device identity, and public
addresses.

## Ownership And Dependency Direction

```text
ApplicationService
  -> persisted ClientSettings intent
  -> IncomingPeerService (bound local endpoint + listener generation)
  -> ReachabilityCoordinator
       -> task-free reachability state/snapshot
       -> one joined mapping task
            -> engine UPnP runtime
                 -> SSDP socket
                 -> bounded HTTP/SOAP client
                 -> one renewable TCP mapping
  -> ViewHub runtime projection
```

- `rstorrent-engine::incoming` owns listener socket construction and reports
  its exact bound endpoint. It does not discover gateways or choose public
  advertisement.
- A focused engine `port_mapping::upnp` boundary owns deterministic SSDP/XML/
  SOAP values and the bounded network client. Pure parsing/state does not
  depend on Tokio handles, application views, persistence, or platform APIs.
- Private `rstorrent-session::reachability` owns mapping policy, listener
  generation, state projection, the mapping task, cancellation, renewal, and
  shutdown ordering. It is not folded into `ApplicationService` or
  `incoming.rs`.
- `ApplicationService` constructs the listener first, passes its observed
  endpoint into the coordinator, and shuts the coordinator down before the
  listener. It retains the existing one-application-generation ownership.
- `SessionStore` persists intent only. `ViewHub` receives immutable bounded
  reachability snapshots and never performs network work.
- Browser, Tauri, Android, gateway, or extension code never proxies SSDP,
  SOAP, peer wire, or payload bytes.

Every task has one cancellation token and join path. A generation fence makes
late discovery, add, renewal, query, or delete results unable to make a
replacement listener advertisable.

## Resource And Security Bounds

| Resource | Bound |
| --- | --- |
| Active mapping owners | One per application generation |
| TCP mappings | One |
| SSDP attempts | Three |
| SSDP discovery duration | At most 8 seconds |
| SSDP datagram | 8 KiB |
| SSDP headers | 64 |
| Distinct device locations | 8 |
| URL length | 2 KiB |
| HTTP redirects/authentication/proxies | None |
| HTTP scheme | IPv4 literal `http` only |
| HTTP connect/action timeout | 5 seconds each |
| HTTP response body | 256 KiB |
| XML nesting depth | 32 |
| XML events | 8,192 |
| XML text value | 2 KiB |
| Device/service candidates | 64 |
| SOAP arguments/fault detail | 32 / 512 bytes of retained detail |
| Mapping conflict alternatives | At most 4 unoccupied high ports |
| Requested lease | 3,600 seconds |
| Renewal point | 75 percent of lease |
| Joined delete timeout | 5 seconds, with typed residual-state report |

The SSDP response source must match the IPv4 literal host in `LOCATION`.
Device-description `URLBase`, service description, and control URLs must
resolve to that same host. Reject userinfo, fragments, port zero, non-HTTP
schemes, global/loopback/multicast location hosts, cross-host redirects,
oversized or malformed HTTP/XML, duplicate critical values, and external
addresses that are unspecified, multicast, or private.

Before add, query the requested external TCP port. Reuse is allowed only when
the entry already matches the exact local address, local port, protocol, and
RSTorrent-owned description; otherwise select another bounded high port
without deleting or replacing the existing entry. After add or renew, query
again and compare every authoritative field. Delete only the exact owned
external-port/protocol pair, and independently query absence during the live
gate.

## Implementation Gates

### Gate 1: Tactical, settings, listener, and state waist

- Land this dossier and topic links.
- Extend the typed settings schema, SQLite migration, generated contracts,
  reducers/fixtures, and existing Settings form without changing defaults.
- Bind automatic/fixed local-network IPv4 through a deterministic injectable
  route-selection helper.
- Add task-free reachability transitions and stale-generation tests.
- Preserve all loopback behavior and public API bounds.

### Gate 2: Deterministic UPnP protocol and scripted gateway

- Add bounded SSDP parsing/request construction, URL policy, device/SCPD XML
  parsing, SOAP requests/responses/faults, and mapping-entry comparison.
- Add a loopback scripted UDP/HTTP gateway with exact add/query/renew/delete
  transcripts.
- Cover loss, malformed/oversized responses, duplicate devices, wrong source
  or host, missing actions, invalid external address, conflicts, lease fault,
  renewal loss, deletion absence, cancellation, and all declared ceilings.

### Gate 3: Joined application mapping lifecycle

- Start the reachability coordinator only from an eligible real listener.
- Project mapping state and bounded diagnostics through existing application
  views.
- Prove mapping starts after bind, renews only for the current generation,
  becomes ineligible before listener shutdown, deletes exactly owned state,
  and joins with terminal zero resources.
- Retain listener operation and local seeding when discovery or mapping fails.

### Gate 4: Real gateway and independent off-LAN transfer

- Use the normal durable application path and a generated deterministic
  complete torrent.
- Query for collision, request a temporary high-port finite mapping, query its
  exact installed fields using an independent harness, and expose the mapped
  endpoint only after verification.
- Invoke a generic operator-supplied off-LAN runner. Stream a repository-owned
  standard-library peer-wire verifier without installing packages or leaving
  remote artifacts.
- Verify exact pieces, whole payload, application Peers/Swarm state, physical
  upload bytes, mapping deletion, failed post-delete connect, process cleanup,
  and terminal owner counts.
- Redact all environment identity and address values from retained evidence.

### Gate 5: Roll-up and baseline

- Run formatting, warning-denying all-target clippy, workspace tests,
  generated-contract drift checks, web tests/typecheck/build and CSP scan, and
  focused desktop/Android compile gates in proportion to changed contracts.
- Update this tactical with exact evidence and resource high-water marks.
- Update the incoming-reachability, capability, protocol, persistence,
  application-control, view/API, peer-lifecycle, and code-organization topics.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure | Settings validation; local-address eligibility; reachability generations; SSDP/header/URL/XML/SOAP/fault/entry codecs; lease and conflict transitions. |
| Scripted runtime | UDP loss/retry, HTTP timeout/body bounds, device/action selection, add/query/renew/delete, gateway loss, stale results, cancellation at each await, joined zero resources. |
| Application | Restart-applied policy, listener-before-mapping order, recoverable mapping failure, view transitions, exact shutdown order, unchanged loopback seeding. |
| Controlled interop | Existing RSTorrent/libtorrent local seeding remains exact; an independent external peer verifies exact public-path payload. |
| Physical gateway | Finite mapping installed and queried, external incoming observed, exact delete and absent query, failed reconnect, sanitized evidence. |
| Product baseline | Generated contracts, web reducers/settings, Tauri/Android consumers, workspace gates, no identity leakage. |

## Check-In Boundary

Completing the stopping condition ends this tactical. Do not continue into
tracker/DHT advertisement, IGD v1, PCP, NAT-PMP, or broader product status
without a new tactical or explicit queue update. Stop earlier for user
direction if the physical gateway requires a permanent lease, the remote
verifier requires persistent machine changes, an existing mapping would need
replacement, or interface selection requires a materially broader platform
architecture.

## Implementation Progress

- Not started.
