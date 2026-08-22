# Tactical 112: Dual-Stack Session Transport And IPv6 DHT Participation

Status: Completed on 2026-08-09. All six gates and the stopping condition are
satisfied. The plan was source-reconciled after Tactical `111` graduated; the
two-tactical split, IPv6 bind-address strategy, and settings shape and default
were accepted in product discussion on 2026-08-08.

Topics: `dht-discovery`, `incoming-reachability-and-seeding`,
`tracker-discovery`, `protocol-support`, `client-persistence`,
`application-view-api`, `client-surfaces`, `capability-readiness`

Dependencies: completed Tactical
[`016`](016-dht-discovery-foundation.md) established the bounded session DHT
actor, its routing state, and its warm-restart snapshot. Completed Tactical
[`089`](089-coordinated-session-listen-sockets.md) established the coordinated
TCP/UDP allocator and the single bounded UDP receive owner that this slice
makes per-family. Completed Tactical
[`092`](092-truthful-tracker-and-dht-peer-advertisement.md) established the
advertised-endpoint selector and the port-`1` outbound-only sentinel this
slice extends per family. Completed Tactical
[`095`](095-bounded-http-https-tracker-transport.md) established
family-selected tracker transport and `peers6` intake. Completed Tactical
[`097`](097-live-client-settings-and-replaceable-session-generations.md)
established replaceable transport generations that every new socket must join.
Completed Tactical [`111`](111-mse-peer-stream-encryption.md) owns persistence
schema version 15 and the final TCP peer-stream path that family-policy
convergence must close in both plaintext and encrypted modes. This tactical
therefore consumes schema version 16.
Completed Tactical [`102`](102-ordinary-incoming-listener-settings.md) owns the
ordinary listener product boundary this slice must not widen. Completed
Tactical [`065`](065-dht-observatory.md) owns the routing-space projection that
gains a second family.

Successor: Tactical
[`113`](113-ipv6-firewall-pinhole-and-incoming-reachability.md) owns the IPv6
firewall pinhole and the physical incoming-reachability proof. This slice
deliberately stops before both.

## Decision And Motivation

Give the session one coordinated socket set per address family, run a real
BEP 32 IPv6 DHT node beside the existing IPv4 node, make the reachable peer
port a per-family fact, and put every IPv6 path behind one persisted client
setting that defaults to enabled.

Four concrete forces select this slice now:

1. **Half the DHT is unreachable.** BEP 32 defines the IPv6 DHT as a
   *separate* network with its own routing table and its own stored peers, not
   as an option on the IPv4 one. RSTorrent is absent from it entirely. Unlike a
   missing tracker, there is no fallback path: peers that announce only to the
   IPv6 DHT cannot be discovered by any other mechanism RSTorrent implements.
2. **The current IPv6 tracker posture is deliberately untruthful and
   permanent.** Every IPv6 tracker announce sends port `1`
   (`crates/rstorrent-engine/src/advertisement.rs:1473-1495`), which correctly
   says "do not publish me" but can never say anything else, because there is
   no IPv6 listener whose port could be published. Tactical `095` recorded this
   as an accepted temporary state; nothing downstream can improve until a
   per-family reachable endpoint exists.
3. **IPv4 assumptions are hardening faster than they are being removed.**
   `SocketAddrV4` and `Ipv4Addr` appear in 250 places across the crates, and
   the concentration is in exactly the owners this slice must change:
   `port_mapping/upnp.rs` (49), `advertised_endpoint.rs` (32), `dht.rs` and
   `reachability.rs` (28 each), `incoming.rs` (26), and `session_socket.rs`
   (21).
   Every further reachability slice widens that surface.
4. **Measurement on the validation network shows the interesting case is the
   normal one.** The development host has one stable RFC 7217 address, one
   active RFC 8981 temporary address, and six deprecated temporary addresses on
   a single interface, and the kernel selects the *temporary* address as the
   source for global destinations. A design that assumes one durable IPv6
   address per host is wrong on ordinary consumer hardware, and it is cheaper
   to face that now than after two more slices depend on it.

A single tactical covering pinholes as well was considered and rejected. The
pinhole has no oracle in the pinned reference at all, a different failure mode,
and an environment-dependent stopping condition; folding it in would make this
slice's completion depend on the least controllable evidence in the campaign.

## Stopping Condition

This tactical is complete when all of the following hold:

1. One allocator produces an independent coordinated TCP + UDP socket pair per
   enabled address family, and a failure in one family leaves the other family
   fully serving.
2. The IPv6 pair binds one probe-selected eligible global unicast address.
   That bound address is the authority for the IPv6 listener, advertised local
   endpoint, and tracker source address; it seeds the BEP 42 identity, while
   independently bounded external-address votes may correct the identity if
   remote nodes consistently observe a different public source address.
3. One DHT actor owns one node per active family, each with its own BEP 42
   node identity, routing table, token secrets, bootstrap state, and
   family-partitioned peer store, sharing one command route, one observation
   owner, and one snapshot.
4. The IPv6 node bootstraps from AAAA-reachable routers, completes iterative
   `find_node` and `get_peers` traversals, answers incoming queries, and
   implements the BEP 32 `want` rules in both directions including
   cross-family answering. One product lookup and announcement fan out to
   every active family and merge their independently terminal outcomes.
5. Tracker announces and DHT self-announcements carry the reachable port *of
   the family the message is sent over*, and port `1` only when that family has
   no eligible endpoint.
6. One persisted `ipv6_enabled` client setting, default enabled, converges live
   through the existing settings owner; when disabled, no IPv6 socket, DHT
   node, tracker connection, retained dialable candidate, or peer connection
   exists.
7. The DHT observatory projects both families with exact family attribution,
   retaining the existing per-family bounds and stating the doubled combined
   maximum explicitly.
8. Deterministic, scripted-runtime, controlled pinned-libtorrent, and physical
   outbound-IPv6 evidence all pass and are recorded here.
9. `dht-discovery.md`, `incoming-reachability-and-seeding.md`,
   `tracker-discovery.md`, `protocol-support.md`, `client-persistence.md`, and
   `capability-readiness.md` state the exact claimed subset and its limits,
   including that no IPv6 incoming reachability is claimed.

## Scope

### Per-family session sockets

`SessionSocketSet` (`crates/rstorrent-engine/src/session_socket.rs:149-224`)
becomes a set of at most two independent family entries, each holding an
optional TCP listener, a UDP socket, the bound address, and the concrete peer
address used for bookkeeping. `tcp_bind_intent` (`:226-238`) and
`bind_automatic` (`:253-313`) become family-parameterised; the existing
ten-retry-then-system-fallback policy applies per family, and the preferred
port is attempted first for both families exactly as pinned libtorrent applies
one `listen_interfaces` port to every expanded endpoint.

Family independence is an invariant, not a convenience: an IPv6 bind failure
must produce typed state and leave IPv4 TCP, IPv4 UDP, and the IPv4 DHT
untouched, and vice versa. Fixed construction remains atomic between TCP and
UDP *within one family*; it is not atomic across families. A failed candidate
family must not roll back a serving sibling family, mirroring Tactical `089`'s
generation behavior.

### IPv6 address selection

A new `select_global_ipv6` beside the existing `select_local_network_ipv4`
(`crates/rstorrent-engine/src/incoming.rs:1090-1117`) uses the same
connect-probe technique: bind an unspecified UDP socket, `connect` it to
`[2001:db8::1]:1`, and read back the source address the kernel selects.
`connect` on an unbound UDP socket performs a route lookup and sends
nothing, so the probe reaches the documentation prefix only through the local
routing table and puts no third-party address into the product.

The returned address must be in IPv6 global-unicast space to be eligible.
Unspecified, loopback, link-local, site-local, unique-local `fc00::/7`,
multicast, IPv4-mapped and IPv4-compatible, and the documentation prefix are
rejected. Teredo `2001::/32` and 6to4 `2002::/16` are also rejected as a
deliberately stricter native-address product policy. BEP 32 only recommends
preferring another global address over Teredo when one exists and says nothing
equivalent about 6to4; neither rejection is attributed to the BEP or to pinned
libtorrent.

Both the IPv6 TCP listener and the IPv6 UDP socket bind that exact address, not
`[::]`. This satisfies BEP 32's source-address requirement directly and removes
any need for an `IPV6_V6ONLY` socket option, because a socket bound to a
specific IPv6 address is unambiguously single-family on every supported
platform. No new dependency is required.

### DHT family nodes

The actor (`crates/rstorrent-engine/src/dht.rs:891-...`) currently holds
`routing_v4` and `routing_v6` but drives only `routing_v4`. It gains an
explicit per-family `DhtNode` holding node identity, routing table, token
secrets, bootstrap and refresh state, and external-address votes. The actor
keeps one command queue, one transaction map keyed by `(transaction, source)`,
one observation forwarder, and one snapshot. Every transaction records both
its logical owner family and its wire family. That distinction is required for
cross-family bootstrap or test queries: a query owned by the IPv6 node may be
sent through the IPv4 sibling socket, and its response must resume the IPv6
traversal rather than mutate the IPv4 node merely because it arrived on IPv4.

The stored-peer table becomes keyed by `(info_hash, family)` because BEP 32
requires that no IPv6 data is stored in the IPv4 DHT and vice versa.

`response_nodes` (`crates/rstorrent-engine/src/dht.rs:1672-1683`) is replaced.
Its current form returns a single node list and takes the IPv4 branch whenever
`want` is empty *or* contains `n4`, so a `want` of `["n4","n6"]` silently drops
`nodes6`. The replacement follows pinned libtorrent's `write_nodes_entries`: no
`want` yields only the receiving family's key; a present `want` yields one key
per recognised token drawn from the requested family's table. The codec already emits
both keys from a mixed slice
(`crates/rstorrent-protocol/src/dht.rs:458-473`), so this is an actor-side fix.

Outgoing `want` adopts pinned libtorrent's rule at
`src/kademlia/rpc_manager.cpp:492-495`: include `want: [<our family>]` only
when the destination address family differs from the querying node's family,
and omit `want` entirely otherwise. This replaces the current unconditional
`want: [Want::Ipv4]` (`crates/rstorrent-engine/src/dht.rs:1508-1511`).

Bootstrap adds AAAA resolution for the configured routers. `dht.libtorrent.org`
and `dht.transmissionbt.com` publish AAAA records; `router.bittorrent.com` and
`router.utorrent.com` do not, so the IPv6 node bootstraps from a smaller router
set plus persisted `nodes_v6`, and that asymmetry is recorded rather than
hidden. Unlike pinned libtorrent, this first slice does not feed saved or
resolved foreign-family endpoints into the sibling node's bootstrap traversal;
that deliberate BEP 32 bootstrap optimization remains deferred below.

`DhtSnapshot` moves to version 2 with bounded `(observed address, node_id)`
identity records per family beside the existing `nodes_v4`/`nodes_v6`. An
identity is restored only for the same address or after it passes the ordinary
BEP 42 observation path; this matches the actual pinned libtorrent state shape
and prevents an address change from silently restoring an invalid identity. A
version-1 snapshot is accepted, not rejected: its unkeyed single `node_id`
becomes the legacy IPv4 candidate and must pass the existing BEP 42 voting
path, while the IPv6 identity is derived fresh. This preserves the current
IPv4 hint without claiming that a v1 snapshot knew the address it did not
store.

One product `get_peers` or announcement command creates one child traversal
per active family under that family's limits. Results are merged and
deduplicated at the actor boundary; one family's useful result is retained if
the sibling times out, and each family uses its own advertised peer port for
`announce_peer`. Cancellation joins every child before the command completes.

### Per-family reachable ports and BEP 7 announcing

`PeerAdvertisementEndpoint`
(`crates/rstorrent-engine/src/advertisement.rs:43-76`) and
`AdvertisedPeerEndpointState`
(`crates/rstorrent-session/src/advertised_endpoint.rs:28-49`) become
per-family, each carrying its own endpoint, scope, and stopping flag under one
shared generation. A new `GlobalUnicast` scope covers the IPv6 case.

`tracker_port` (`crates/rstorrent-engine/src/advertisement.rs:1473-1495`) and
the DHT port selector (`:1338-1352`) select by the family the message is
actually sent over, following pinned libtorrent's `listen_socket_t::can_route`
(`src/session_impl.cpp:376-391`). An IPv4 announce carries the IPv4 endpoint or
`1`; an IPv6 announce carries the IPv6 endpoint or `1`. Tactical `095`'s rule
that an IPv6 tracker request always receives port `1` is superseded exactly
here and nowhere else.

HTTP and HTTPS tracker clients set `reqwest`'s local address to the selected
per-family bind address, so the address a tracker observes is the address whose
port we publish and the initial input to that family's DHT identity. UDP tracker
announces currently bind a family-matched *wildcard* ephemeral socket
(`crates/rstorrent-engine/src/driver.rs:3073-3078`). The IPv6 path must instead
bind an ephemeral socket on the selected IPv6 address before connecting, so it
meets the same BEP 7 source-address invariant. The socket remains operation-
owned and does not become another long-lived receive owner.

This slice does not make one retained tracker record issue simultaneous
announces over both families when a tracker is reachable over both. It makes
the source address and port correct for the family the existing tracker
schedule selects, including AAAA-only trackers. Full BEP 7 per-local-address
announce fan-out remains an explicit deferral rather than an implied claim.

### Family policy

One `ipv6_enabled: bool` field on `ClientSettings`
(`crates/rstorrent-session/src/settings/contract.rs:73-98`) at schema version
16, after Tactical `111` consumes version 15 for `encryption`, defaulting to
`true` for both `default()` and `fresh_profile_default()`,
converging live through the existing settings runtime, with a generated
TypeScript type and a control in
`clients/web/src/inspection/components/ConnectionSeedingSettingsSection.tsx`.
Android receives and enforces the generated setting but, consistently with the
existing client-surface boundary, gains no Compose settings control here.

The setting is carried as an `AddressFamilyPolicy` value on `NetworkConfig`
(`crates/rstorrent-engine/src/network.rs:45-68`) rather than threaded through
all 216 `NetworkPolicy` sites. Its primary enforcement is structural: when
disabled, the allocator creates no IPv6 socket and the actor creates no IPv6
DHT node, so most IPv6 paths become unreachable by construction. The predicate
`AddressFamilyPolicy::permits(IpAddr)` then guards four remaining owner
boundaries, each named in tests that exercise every input source:

- tracker URL family selection and connection
  (`crates/rstorrent-engine/src/http_tracker.rs:417-460`);
- UDP tracker resolved-address selection;
- peer-registry admission for every `PeerSource`, including tracker `peers6`,
  PEX `added6`, manual, magnet-hint, and cache contacts, with PEX bookkeeping
  applying the same predicate before retaining its own contact state; and
- the peer-connection owner, both as a defense at selection/dial time and as
  live convergence that cancels and joins every existing IPv6 connection and
  removes inactive IPv6 candidates before the disabled setting reports
  `Applied`.

Disabling IPv6 must reproduce today's IPv4 network behavior under deterministic
inputs, excluding the deliberately changed schema and generated-contract
shape. This gives the setting a falsifiable event/wire-trace regression test
rather than an impossible whole-profile byte-identity promise.

### Observability

`DhtObservation` and `dht_views.rs` gain explicit family attribution rather
than only appending a second bucket list. Per-family lifecycle, local node ID,
routing summary, counters, buckets, and lookup summaries remain
distinguishable; every lookup row carries its family. The current unsuffixed
`local_node_id` and aggregate lookup/counter fields must therefore either
become explicit v4/v6 fields or a bounded family record in the pre-release
generated contract. The observatory keeps 16 lookups and 160 buckets per
family, for combined maxima of 32 lookup summaries and 320 bucket slots.

The same no-overwrite rule applies to `ClientSettingsRuntimeView`'s current
single `listener_status`, `session_udp_status`, and
`advertised_peer_endpoint`: the generated contract gains a bounded v4/v6
transport record (or equivalently explicit family fields) rather than letting
the second bind replace the first. Runtime state and diagnostics expose
per-family configured, bound TCP, bound UDP, advertised endpoint, and observed
external DHT address without conflating them.

## Non-Goals

- **IPv6 firewall pinholes and any IPv6 incoming-reachability claim.** Owned by
  Tactical `113`. This slice binds and advertises; it proves nothing about
  packets arriving from the Internet.
- **BEP 45 multi-address announce and per-interface multi-homing.** One socket
  pair per family, not one per interface address. This is the largest
  deliberate divergence from pinned libtorrent and is justified below.
- **Interface enumeration.** No `getifaddrs`, netlink, or
  `GetAdaptersAddresses` path, and therefore no detection of deprecated,
  tentative, or temporary address flags. The probe accepts whatever the OS
  prefers.
- **BEP 5 `PORT` peer-wire messages.** BEP 32 defines their per-family
  semantics, but they are a peer-wire change with their own admission and
  hostile-input surface.
- **IPv4-disable or IPv6-only mode.** The setting is a boolean, not a
  tri-state. 464XLAT-style IPv6-only mobile networks are a real case with no
  current product driver.
- **uTP, LSD, NAT-PMP, PCP, and UDP mapping of any family.**
- **Rebinding on IPv6 address rotation.** The bound address is selected once
  per transport generation. Rotation is handled by the existing
  settings-driven generation replacement, not by a new address-change watcher.
- **Physical Android or ChromeOS IPv6 evidence.** Limited to confirming the
  Android crate builds and that the family policy is honored on an AVD.
- **A Compose IPv6 setting control.** Android consumes the generated value and
  enforces the default, but the existing client-surface topic reserves mobile
  connectivity settings for a separate product slice.
- **BEP 7 per-local-address tracker fan-out.** A dual-stack tracker record still
  follows the existing selected-family operation. This slice makes that
  operation's bind address and announced port truthful; it does not create two
  simultaneous tracker lifecycles for one record.

## Normative And Reference Dossier

### Specifications

Read from the pinned `reference/bittorrent.org` checkout at revision
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`:

| BEP | Behavior adopted |
| --- | --- |
| [32](https://www.bittorrent.org/beps/bep_0032.html) | Two independent DHTs with separate routing tables; no cross-family data storage; `nodes6`; `want` as a list of `n4`/`n6` flags; hybrid `values` lists must be *parsed* but not sent, and never sent in reply to a `want`-less IPv4 request; 1024-octet datagram ceiling; bind a global unicast address rather than `::`, preferring non-Teredo when available |
| [7](https://www.bittorrent.org/beps/bep_0007.html) | Bind the selected tracker operation to the corresponding local source address and publish that family's port; `&ipv4=`/`&ipv6=` announce parameters are discouraged and are not implemented; `peers6` intake is unchanged. The BEP's one-announce-per-published-local-address fan-out is explicitly deferred. |
| [42](https://www.bittorrent.org/beps/bep_0042.html) | Address-derived node identity per family, using the eight-octet IPv6 mask |
| [5](https://www.bittorrent.org/beps/bep_0005.html) | Unchanged query, token, and routing semantics, now instantiated twice |

Two BEP 32 recommendations are deliberately not adopted and are recorded as
deferrals below: requesting both families during bootstrap, and occasionally
requesting the foreign family in steady state.

BEP 32's "a node should use the same node id in both tables" conflicts with
BEP 42, which derives the identity from the external address, and the two
addresses differ by construction. This slice follows BEP 42 and pinned
libtorrent, which persists identities as a list of (address, node id) pairs.

Our existing BEP 42 implementation already handles IPv6 correctly: the
eight-octet mask `01 03 07 0f 1f 3f 7f ff` at
`crates/rstorrent-protocol/src/dht.rs:834-849` matches libtorrent's `v6mask` at
`src/kademlia/node_id.cpp:89-110` exactly. No change is needed there, and a
test asserts the equivalence rather than assuming it.

`MAX_DATAGRAM_SIZE` is already 1024
(`crates/rstorrent-protocol/src/dht.rs:12`), so BEP 32's packet ceiling is
already enforced on both send and receive.

### Pinned libtorrent

Revision `7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected.

| Concern | Path and symbols |
| --- | --- |
| Listen socket state, flags, external port derivation | `include/libtorrent/aux_/session_impl.hpp:164-282` (`listen_socket_t`, `accept_incoming`, `local_network`, `was_expanded`, `tcp_external_port`) |
| Listen endpoint value and matching | `include/libtorrent/aux_/session_impl.hpp:284-308` (`listen_endpoint_t`) |
| Refusal to bind wildcard; per-interface expansion; local-network classification | `src/session_impl.cpp:284-345` (`expand_unspecified_address`) |
| Netmask and device resolution | `src/session_impl.cpp:347-374` (`expand_devices`) |
| **Per-family routing predicate for advertisement** | `src/session_impl.cpp:376-391` (`listen_socket_t::can_route`) |
| Interface-name to endpoint expansion | `src/session_impl.cpp:2003-2039` (`interface_to_endpoints`) |
| Socket setup and retry | `src/session_impl.cpp:1608-1700` (`setup_listener`) |
| `IPV6_V6ONLY` always set on both transports | `src/session_impl.cpp:1692-1700`; `src/udp_socket.cpp:492-500` |
| Default dual-stack listen configuration | `src/settings_pack.cpp:146` (`"0.0.0.0:6881,[::]:6881"`) |
| One DHT node per listen socket; family dispatch | `src/kademlia/dht_tracker.cpp:116-170`, `:255-275`, `:313-325`, `:344-350`, `:481-490` |
| Node family descriptor and native-address tests | `include/libtorrent/kademlia/node.hpp:224-235`; `src/kademlia/node.cpp:133`, `:1235-1253` |
| **Cross-family `nodes`/`nodes6` answering** | `src/kademlia/node.cpp:1206-1233` (`write_nodes_entries`) |
| **Outgoing `want` emitted only for cross-family queries** | `src/kademlia/rpc_manager.cpp:492-495` |
| Per-family node identities and bootstrap lists in persisted state | `src/kademlia/dht_state.cpp:40-135`; `include/libtorrent/kademlia/dht_state.hpp:63-73` |
| BEP 42 IPv6 mask and identity generation | `src/kademlia/node_id.cpp:85-115` |
| Address-family-aware external address voting | `src/ip_voter.cpp` |
| Interface flags feeding the `preferred` decision | `src/enum_net.cpp:439` (Linux `IFA_F_DADFAILED\|DEPRECATED\|TENTATIVE`), `:883` (Windows `IpDadStatePreferred`) |

Oracle tests inspected: `test/test_listen_socket.cpp` (17 cases, including
`expand_unspecified`, `expand_unspecified_global_address`,
`expand_unspecified_link_local`, `expand_unspecified_loopback`,
`expand_unspecified_ppp`, `expand_unspecified_down_if`, and the seven
`partition_listen_sockets_*` cases); `test/test_dht.cpp` (61 IPv6-related
assertions covering `want`, `nodes6`, and per-family routing);
`test/test_enum_net.cpp`; `test/test_ip_voter.cpp`.

Edge cases extracted from that source and adopted here:

- an IPv6 listen socket and its UDP peer must both be single-family; libtorrent
  achieves this with `v6_only(true)` on each, and this slice achieves it by
  binding a specific address (`src/session_impl.cpp:1692`,
  `src/udp_socket.cpp:495`);
- advertisement eligibility is a *per-socket routing* question, not a global
  one: `can_route` rejects a different family, a mismatched IPv6 scope id, and
  an off-subnet address on a local-network-only socket
  (`src/session_impl.cpp:376-391`);
- a query with an explicit `want` may need node data from a routing table the
  receiving node does not own, so the responder must be able to reach its
  sibling (`src/kademlia/node.cpp:1228-1231`);
- `want` on outgoing queries is a cross-family bridge, not a per-query
  preference, and sending it for same-family queries is pure waste
  (`src/kademlia/rpc_manager.cpp:492-495`); and
- persisted DHT identity is per external address, so a restored snapshot must
  survive one family's address changing while the other's does not
  (`src/kademlia/dht_state.cpp:40-70`).

**Pinned libtorrent has no IPv6 firewall-pinhole implementation.** Neither
`pinhole` nor `WANIPv6FirewallControl` appears anywhere in the tree. That
absence is the reason the pinhole is Tactical `113` rather than part of this
slice.

### rqbit

Revision `4e5f94cbcf1d57ec500885c77cf1e24d70232d89` implements IPv6 and takes a
**different** approach from libtorrent on every axis this slice touches, which
is why it is recorded rather than treated as confirmation:

| Concern | libtorrent | rqbit | This slice |
| --- | --- | --- | --- |
| Sockets | one per preferred interface address, `v6_only(true)` | one dual-stack `[::]` socket, `request_dualstack: true` (`crates/librqbit/src/listen.rs:88-96`) | one per family, IPv6 bound to a specific global address |
| DHT node identity | one per family, BEP 42 from that family's external address | one shared `self.id` across both tables (`crates/dht/src/dht.rs`) | one per family, BEP 42 |
| Routing tables | one per node | `routing_table_v4` + `routing_table_v6` (`crates/dht/src/dht.rs:712-735`) | one per family |
| Outgoing `want` | only cross-family, requesting own family | always the destination's family (`crates/dht/src/dht.rs:660-664`) | libtorrent's rule |
| Family toggle | `listen_interfaces` string | `ipv4_only: bool` (`crates/librqbit/src/session.rs:151`, `crates/librqbit/src/listen.rs:58`) | `ipv6_enabled: bool`, inverted sense, default enabled |

rqbit's `Want::Both` responder handling
(`crates/dht/src/dht.rs:712-735`) independently confirms the cross-family
answering behavior extracted from libtorrent, and its family-partitioned peer
store (`crates/dht/src/peer_store.rs:187-193`) independently confirms the
BEP 32 storage-partitioning requirement. Two independent implementations
agreeing on those two points raises confidence that they are load-bearing
rather than incidental.

rqbit's single dual-stack socket was considered and rejected for this slice: it
requires an explicit `IPV6_V6ONLY` decision that differs by platform, it leaves
the DHT source address unpinned in exactly the multi-address case measured on
the validation host, and it contradicts BEP 32's source-address section.

### JSTorrent

The first-party checkout at `~/code/jstorrent` revision
`9895410beeed6aff554053769bd006a3fbd373ef` was inspected for product and
platform history. IPv6 handling appears in `packages/engine/src/core/swarm.ts`,
`packages/engine/src/dht/types.ts`,
`packages/engine/src/tracker/http-tracker.ts`, and
`packages/engine/src/extensions/pex-handler.ts` — that is, in peer intake,
tracker parsing, and PEX, but not in listening or DHT participation. JSTorrent
therefore contributes compact-parsing and product-behavior history and provides
no IPv6-listener, IPv6-DHT, or IPv6-reachability precedent for this slice. No
JSTorrent source, fixture, or test data is imported.

## Measured Network Facts

These were observed on 2026-08-08 by non-mutating inspection and are recorded
as the environment this slice targets. They are evidence about two networks,
not a general claim.

- The development host has native global IPv6 with one stable RFC 7217
  address, one active RFC 8981 temporary address, and six deprecated temporary
  addresses on one interface. The kernel selects the **temporary** address as
  the source for global destinations.
- A UDP `connect` to `[2001:db8::1]` returns the same source address as a
  `connect` to a real global destination, confirming the probe technique
  without any third-party address or transmitted packet.
- `dht.libtorrent.org` resolves to `2a02:752:0:18::128` and
  `dht.transmissionbt.com` to `2001:41d0:203:4cca:5::`.
  `router.bittorrent.com` and `router.utorrent.com` have no AAAA record.
- The off-LAN validation host is on a different ISP with native global IPv6 and
  working IPv6 egress.
- **Unsolicited inbound IPv6 is dropped in both directions** between the two
  hosts, for TCP and for ICMPv6. The development host's application firewall is
  disabled, so the block is at the customer premises equipment. This is the
  negative control for Tactical `113` and the reason this slice claims no
  incoming reachability.

## Owner, Task, And Data-Flow Map

```text
              ClientSettings.ipv6_enabled (persisted, schema 16)
                                 |
                  settings convergence / transport generation
                                 |
                    SessionSocketAllocator (one per generation)
                                 |
              +------------------+-------------------+
              v                                      v
        family = v4                            family = v6  (if enabled)
   bind 0.0.0.0:port                     probe -> global unicast addr
   TCP listener + UDP socket             TCP listener + UDP socket
              |                                      |
     SessionUdpService(v4)                  SessionUdpService(v6)
     64-datagram DHT route                  64-datagram DHT route
              |                                      |
              +------------------+-------------------+
                                 v
                        one DhtService actor
                    one command route, one snapshot
                    one observation forwarder
                                 |
              +------------------+-------------------+
              v                                      v
        DhtNode(v4)                            DhtNode(v6)
   id / routing / tokens                  id / routing / tokens
   bootstrap / refresh                    bootstrap / refresh
              \                                      /
               \--- family-partitioned peer store --/
               \--- cross-family want answering ----/
                                 |
                    per-family advertised endpoint
                                 |
              +------------------+-------------------+
              v                                      v
     v4 tracker + DHT announce            v6 tracker + DHT announce
     (endpoint port or 1)                 (endpoint port or 1)
```

Ownership rules this slice must not violate:

- **Family independence.** No family's bind failure, address loss, socket
  error, or DHT bootstrap failure may degrade the other family. This is
  asserted directly, not inferred from the absence of shared state.
- **One actor, two nodes.** The DHT gains a second node, never a second actor,
  second command queue, second snapshot, or second observation owner. This
  follows pinned libtorrent's `dht_tracker` and keeps Tactical `065`'s
  projection owner intact.
- **No new long-lived task per family beyond the second UDP receiver.** The
  second `SessionUdpService` is the only added long-lived owner, and it joins
  the same transport generation as the first.
- **The bound address is the local authority.** The IPv6 listener, advertised
  local endpoint, tracker source binding, pinhole input, and initial BEP 42
  identity all derive from one observed bind address. DHT external-address
  votes are separate remote evidence and may correct only that family's node
  identity; no component independently re-probes the local address.
- **Dependency direction is unchanged.** `rstorrent-protocol` gains no runtime,
  socket, or family-policy knowledge; the family split lives in the engine's
  runtime owners.
- **Advertisement stays truthful in the existing sense.** A family with no
  eligible endpoint sends port `1`, exactly as an unroutable IPv4 torrent does
  today.

## Advertisement Semantics And Their Exact Limit

Today `tracker_port` publishes a real port for the `Mapped` and `LocalNetwork`
scopes (`crates/rstorrent-engine/src/advertisement.rs:1481-1489`). A
`LocalNetwork` scope is an RFC 1918 address, which is definitively unreachable
from the Internet, and the campaign accepts publishing it because the *listener
is real*. A bound global unicast IPv6 port is strictly more likely to be
reachable than that, so the new `GlobalUnicast` scope is treated the same way.

The claim this slice makes is therefore precise and deliberately narrow: the
published IPv6 port belongs to a listener that is actually accepting on a
globally routable address. It is **not** a claim that packets arrive. The
measured negative control above shows they currently do not. The readiness and
protocol topics must state this in those words, and no row may be promoted on
the strength of a successful bind.

## Resource And Failure Bounds

| Resource | Bound |
| --- | --- |
| Coordinated socket pairs | 2 (one per family), each TCP + UDP |
| Address-selection probes | 1 IPv6 route probe per transport generation, plus the existing bounded IPv4 concrete-address selection |
| Long-lived UDP receive owners | 2, each with the existing 64-datagram DHT route |
| DHT nodes | 2, each with independent identity, routing table, and token secrets |
| Routing nodes and replacements | Existing per-table bounds, applied per family |
| Active transactions | `MAX_ACTIVE_TRANSACTIONS` (256) per family, 512 combined, so one family cannot consume the sibling's transaction capacity |
| Active lookups | `MAX_ACTIVE_LOOKUPS` (16) per family, 32 combined |
| Stored peers | `MAX_PEER_STORE_HASHES` (256) per family, `MAX_PEERS_PER_HASH` (100) per (hash, family); worst-case entries double to 51,200 |
| Persisted nodes | `MAX_PERSISTED_NODES_PER_FAMILY` (64) unchanged, now used for both |
| Datagram size | 1024 bytes send and receive, unchanged, already BEP 32-conformant |
| Query rate limits | Existing per-source and 250-query/s windows applied per receiving family; combined maximum 500 queries/s |
| DHT observation | 160 buckets and 16 lookup summaries per family; 320 and 32 combined |
| Snapshot | Version 2, at most one address-keyed identity and 64 saved nodes per family |
| Bind retries | Existing ten candidates then system selection, per family |

Failure and hostile-input rules:

- An ineligible or absent probe result disables the IPv6 family for that
  generation with typed state; it is never a startup failure.
- A datagram whose source family does not match the receiving socket's wire
  family is dropped before transaction lookup. A valid cross-family response
  resumes the logical owner recorded in the transaction rather than the node
  associated with the receiving socket.
- A response carrying `nodes6` to an IPv4 node is admitted only into the IPv6
  routing table, and vice versa, and never into the peer store.
- A `values` list mixing 6-byte and 18-byte entries is parsed, per BEP 32, but
  entries of the wrong family for the receiving DHT are discarded rather than
  stored.
- A response never emits a hybrid peer `values` list. An explicit `want`
  controls only `nodes`/`nodes6`; peer values, when present, still match the
  request's wire family and family-partitioned peer store.
- The existing external-address voting runs independently per family; a vote
  from one family can never change the other family's identity.
- Every existing hostile-input rule for KRPC applies unchanged to the second
  node; none is weakened to share state.

## Intentional Differences From The Oracle

| Behavior | Pinned libtorrent | RSTorrent | Why |
| --- | --- | --- | --- |
| Listen socket count | One per preferred interface address | One per family | BEP 45 multi-address announce is out of scope; one socket pair per family is the smallest shape that satisfies BEP 32, and interface enumeration is ~1000 lines of per-platform code with no current product driver |
| IPv6 bind address | Every preferred address, `v6_only(true)` | One probe-selected eligible global unicast address | Directly satisfies BEP 32's source-address section, needs no socket option, and needs no dependency |
| Teredo and 6to4 | Teredo is a lower-preference address; 6to4 remains global | Both are ineligible | A deliberately stricter native-address policy avoids known transition-address identity instability; BEP 32 itself only advises avoiding Teredo when another GUA exists |
| Deprecated/temporary address filtering | `preferred` flag from netlink or Windows DAD state | Not detected; the OS's own source choice is accepted | Requires the enumeration this slice defers; the selected address is the local authority, while bounded DHT votes remain the authority for what remote nodes observe |
| `IPV6_V6ONLY` | Always set | Never set | Unreachable by construction when binding a specific IPv6 address |
| Bootstrap endpoints and `want` | Feeds each node saved endpoints from both families; a foreign-family query asks for the logical node's own family | Native-family router and saved endpoints only; the same outgoing `want` rule is implemented for controlled cross-family queries | BEP 32 recommends both-family acceleration, but native bootstrap keeps the first runtime slice and BEP 42 source identity simple; foreign-family bootstrap is deferred explicitly |
| Tracker announces | One announce per publishable local address | One existing selected-family operation per tracker record | This slice fixes source/port truth without duplicating tracker schedule state; full BEP 7 fan-out remains a named later slice |
| Family toggle | Absent; expressed through `listen_interfaces` | One boolean setting | The product needs one comprehensible control, not an interface-specification string |
| DHT actor count | One `node` per listen socket under one `dht_tracker` | One actor, one node per family | Matches the existing single-actor design and Tactical `065`'s single projection owner |

## Implementation Gates

Each gate is independently committable and leaves the workspace green.

1. **Family-parameterised allocation.** `SessionSocketSet` per family,
   `select_global_ipv6`, eligibility rejection table, per-family typed bind
   failure. Proven by scripted conflict, fallback, and single-family-failure
   cases with no cross-family effect.
2. **Second UDP receiver and family routing.** A second `SessionUdpService`
   joined to the same generation, family-checked dispatch, terminal zero
   ownership for both.
3. **DHT family nodes.** Per-family identity, routing, tokens, bootstrap,
   refresh, and peer partitioning; the `response_nodes` correctness fix; the
   libtorrent `want` rule; snapshot version 2 with version-1 acceptance. Proven
   deterministically and against a controlled IPv6 loopback oracle.
4. **Per-family reachable ports.** Per-family advertised endpoint, the
   `GlobalUnicast` scope, family-selected tracker and DHT ports, per-family
   HTTP and UDP tracker source binding. Proven by controlled announces observed
   on both families.
5. **Family policy and product surface.** `ipv6_enabled` at schema 16, live
   convergence, generated contract, web control, the four-predicate
   owner-boundary tests, and the disabled-equals-today regression.
6. **Observability and evidence.** Per-family DHT observatory projection,
   diagnostics, the physical outbound-IPv6 run, and topic updates.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Address selection | Eligibility accepts a global unicast address and rejects unspecified, loopback, link-local, ULA, multicast, IPv4-mapped, IPv4-compatible, Teredo `2001::/32`, 6to4 `2002::/16`, and `2001:db8::/32`; probe sends zero bytes, asserted by a counting socket |
| Allocation | Both families attempt the preferred port; per-family conflict walks ten candidates then system selection; IPv6 bind failure leaves IPv4 TCP, IPv4 UDP, and the IPv4 DHT fully serving, and vice versa; fixed TCP/UDP construction is atomic within each family but not across families; no family's socket outlives its generation |
| DHT codec parity | Our BEP 42 IPv6 mask asserted equal to libtorrent's `v6mask` vectors; `nodes6` round-trip; hybrid `values` list parsed; a `want`-less IPv4 reply asserted to contain no 18-byte value |
| DHT actor, common | Independent bootstrap, traversal, refresh, token rotation, transaction and lookup ceilings per family; compact contacts admitted only to the table matching their encoded family; a product command retains one family's useful result when its sibling times out; peer store partitioned so a v4 `get_peers` never returns a v6 value; identity votes isolated per family |
| DHT actor, `want` | No `want` on same-family queries; `want: [<own family>]` on cross-family queries; responder returns own family for absent `want`, the requested family for a single token, and **both** keys for `["n4","n6"]` — the case the current code drops |
| Snapshot | Version 2 round-trip with address-keyed identities; version 1 accepted with its unkeyed ID treated as a legacy IPv4 candidate and a fresh IPv6 identity derived; a changed address cannot silently restore the old ID; family-mismatched, oversized, and corrupt entries rejected; same-address warm restart preserves both network positions |
| Family policy | Disabled creates no IPv6 listener, UDP socket, or DHT node; selects no IPv6 tracker connection; rejects IPv6 contacts for every `PeerSource`, explicitly including tracker `peers6`, PEX `added6`, manual, magnet-hint, and cache input; starts no IPv6 dial; and cancels and joins active IPv6 peer connections before convergence reports `Applied`. Tests cover all four owner boundaries, enable-disable-enable replacement, and a deterministic pre-slice IPv4 event/wire trace apart from the new schema and contract fields |
| Advertisement | IPv4 announce carries the IPv4 port, IPv6 announce carries the IPv6 port, each family independently falls back to `1`; stopping cancels both; controlled HTTP and UDP trackers observe the same selected IPv6 source address whose port was published |
| Runtime and cancellation | Both generations replace live without losing registrations or peer tasks; same-address replacement retains DHT identity and routing state, while an address change revalidates identity and invalidates only that family's address-bound state; joined shutdown leaves zero tasks, sockets, and DHT operations for both families |
| Controlled interoperability | Pinned libtorrent `2.0.13` configured for IPv6 loopback: RSTorrent completes a hash-verified download from it over IPv6; a libtorrent DHT-only leecher on the IPv6 loopback DHT discovers an RSTorrent announcement and completes; libtorrent's incoming `ping`, `find_node` with each `want` form, `get_peers`, and `announce_peer` verified against the RSTorrent IPv6 node |
| Physical, outbound only | On the development host: the IPv6 node bootstraps against real AAAA routers, reaches a recorded healthy routing threshold, completes a `get_peers` traversal, and acquires metadata for a public torrent; recorded with time to first valid response, time to threshold, node counts per family, and datagram counters |
| Client | Setting renders, persists, converges live, survives restart; schema 15 migrates to 16 with `encryption` unchanged and IPv6 enabled; disabling closes both plaintext and MSE IPv6 peer streams without disturbing IPv4 peers; DHT observatory shows both families within existing bounds; existing web tests and typecheck pass with the regenerated contract |
| Android | `rstorrent-android` builds for both supported ABIs; an AVD run confirms the family policy is honored and that an absent or ineligible IPv6 address degrades to IPv4-only without error |

No pinhole, no mutating gateway action, and no incoming-IPv6 claim appear in
this matrix. The physical run is explicitly outbound-only.

## Deferred With Reason

- **IPv6 incoming reachability, pinholes, and mapping.** Tactical `113`. The
  measured CPE block makes this unprovable inside this slice.
- **Interface enumeration and address-flag filtering.** Deferred until evidence
  shows the OS's source choice is inadequate. The concrete trigger is recorded:
  if the physical run shows the DHT node identity churning because the bound
  temporary address rotated within a session, enumeration becomes justified.
- **Rebinding on IPv6 address change.** Same trigger. Today the existing
  generation-replacement path is the only rebind mechanism.
- **BEP 32 dual-family bootstrap requests, foreign-family saved endpoints,
  and occasional foreign-family steady-state requests.** These are
  reliability optimizations. Native-family bootstrap plus the exact responder
  and transaction model lands first, giving a baseline against which the
  cross-family optimization can be measured rather than assumed.
- **BEP 7 per-local-address tracker fan-out.** The selected family's source and
  port are correct in this slice; a second simultaneous announce lifecycle for
  a dual-stack tracker remains separate scheduling work.
- **BEP 45 multi-address announce and multi-homing.** Needs interface
  enumeration and a per-interface announce model; no product driver.
- **BEP 5 `PORT` messages.** Independent peer-wire surface.
- **IPv6-only mode for 464XLAT networks.** Real, but no current product driver
  and no way to validate it on either available network.
- **NAT64/DNS64 synthesis for IPv4 peer literals.** Only relevant on IPv6-only
  networks, and coupled to the above.

## Escalation And Next Boundary

Stop and ask for direction if any of the following occurs:

- the probe returns a usable address on the development host but no usable
  address on a common configuration, which would mean the no-enumeration
  decision is wrong rather than merely narrow;
- keeping families independent requires duplicating the DHT actor, the command
  route, or the observation owner rather than adding a second node;
- the bound IPv6 address rotates within a single session often enough that the
  node identity cannot stabilise, which promotes interface enumeration from a
  deferral to a prerequisite;
- pinned libtorrent rejects a `want` or `nodes6` form this tactical records as
  correct, implying the extracted rules are wrong; or
- making the family policy enforceable requires threading a new value outside
  the four named owner boundaries, or cannot close existing IPv6 peer
  generations without disturbing IPv4 peers, which would mean the structural
  argument for the setting does not hold.

## Execution Record

### Gate 1: Family-parameterised allocation

Completed on 2026-08-09 from baseline `af531ca`. `NetworkConfig` now carries a
small address-family policy and the coordinated allocator owns independent
family states. Each bound family owns its TCP listener, UDP socket, observed
bind endpoints, and concrete peer endpoint; a typed failure in one family is
retained without dropping or degrading the serving sibling. IPv4-only callers
retain their prior behavior until the product setting is connected in Gate 5.

The IPv6 route probe binds `[::]:0`, connects to the documentation-prefix
target without calling `send`, and accepts only native global-unicast space.
Deterministic cases reject unspecified, loopback, link-local, site-local,
unique-local, multicast, IPv4-compatible, IPv4-mapped, documentation, Teredo,
and 6to4 addresses. Loopback allocation proves both families independently
attempt the preferred port; forced UDP conflicts in either family prove
within-family TCP rollback and cross-family survival. A counting receiver
proves the connect probe transmits no datagram.

Validation:

- `cargo test -p rstorrent-engine session_socket --lib` (14 passed);
- `cargo check --workspace`; and
- `git diff --check`.

### Gate 2: One family-aware UDP receive owner

Completed on 2026-08-09. `SessionUdpService` now owns one replaceable receiver
generation per bound address family while retaining one bounded ingress queue,
one DHT transport, one counter set, and one shutdown owner. Every ingress item
carries its receiving family, outbound sends select the family socket from the
destination address, and a missing family returns a typed error. Adding,
replacing, or removing one family cancels and joins only that generation.

The session runtime connects both independently allocated UDP sockets to that
owner at startup and settings-driven transport replacement. The DHT actor still
rejects IPv6 ingress explicitly until Gate 3 installs its IPv6 node, so this
slice cannot accidentally treat IPv6 traffic as IPv4 traffic. Deterministic
loopback coverage proves both families share the one route, either receiver can
be retired independently, family-selected sends use the matching source, and
shutdown joins both tasks. The steady task count is two; sequential candidate-
first replacement reaches a bounded high-water mark of three and returns to
two, then terminal shutdown reaches zero.

Validation:

- `cargo test -p rstorrent-engine session_udp --lib` (7 passed);
- `cargo test -p rstorrent-engine
  ipv6_wire_datagrams_wait_for_an_ipv6_dht_node --lib` (1 passed);
- `cargo test -p rstorrent-session
  application_coordinates_tcp_and_dht_udp_endpoints --lib` (1 passed);
- `cargo test -p rstorrent-session
  rapid_client_settings_changes_converge_only_to_latest_generation --lib`
  (1 passed); and
- `git diff --check`.

### Gate 3: One actor with independent family nodes

Completed on 2026-08-09. The one DHT actor now reconciles one `DhtNode` for
each family present in the session UDP transport. Each node independently owns
its BEP 42 identity, routing table, token secrets, bootstrap state, refresh
timers, query-rate windows, and external-address votes. Transactions retain
both their logical owner family and wire family, and the active transaction
and lookup ceilings are enforced per logical family. Removing one UDP family
retires only that node and its family-bound work; restoring the same bound
address restores its identity without restarting the actor.

Product lookups fan out to every active family and merge useful terminal
results while the stored-peer table remains keyed by `(info_hash, family)`.
Native-family queries omit `want`; controlled cross-family queries request the
logical node's own family. Incoming queries without `want` receive only their
wire-family table, while explicit `n4`, `n6`, or both requests receive exactly
the requested `nodes` keys. Responses never emit hybrid peer values.

The persisted snapshot is version 2 and stores bounded address-keyed identity
records plus both routing samples. Version 1 remains accepted as a legacy IPv4
identity hint while IPv6 starts with a fresh address-derived identity. Session
schema version 16 adds the bounded identity table and the default-enabled
`ipv6_enabled` field; transport enforcement and the product control remain
Gate 5 work.

Validation:

- `cargo test -p rstorrent-protocol dht --lib` (8 passed);
- `cargo test -p rstorrent-engine dht --lib` (29 passed, 2 ignored live tests);
- `cargo test -p rstorrent-session
  dht_snapshot_round_trips_and_rejects_corrupt_rows --lib` (1 passed);
- `cargo test -p rstorrent-session settings::tests --lib` (11 passed before
  the additional schema-15 migration assertion); and
- `cargo check --workspace`.

### Gate 4: Per-family reachable ports

Completed on 2026-08-09. Advertised endpoint state is now keyed by address
family. IPv4 and IPv6 listener generations independently publish their actual
TCP port or the existing port-`1` sentinel, and stopping or losing one family
does not change its sibling. The selected global-unicast IPv6 address carries
the deliberately narrow `GlobalUnicast` scope: it proves a bound listener,
not gateway permission or observed incoming reachability.

HTTP/HTTPS and UDP tracker operations select the advertisement only after
selecting the destination family. IPv6 operations bind their source socket to
the same selected IPv6 address, and DHT self-announcement selects the port for
the logical node family. Controlled IPv4 and IPv6 tracker cases observe the
expected source address and exact family port; either absent listener produces
`1` without borrowing the sibling's value.

Validation:

- focused engine tracker and advertisement tests;
- focused session advertised-endpoint tests; and
- family-selected HTTP and UDP tracker integration cases.

### Gate 5: Family policy and product surface

Completed on 2026-08-09. Schema version 16 adds one checked, default-enabled
`ipv6_enabled` value to the existing atomic client-settings row. Migration
from schema 15 preserves encryption policy and enables IPv6. The configured
value remains durable intent; the effective value reflects whether an
eligible global-unicast address produced a serving family. An unavailable or
ineligible address is a typed transport degradation and leaves IPv4 serving.

The existing session-network reconciler applies enable/disable/enable live.
Disabling removes the IPv6 acceptor, UDP receiver, DHT node, tracker
eligibility, every IPv6 peer-source candidate, and active plaintext or MSE
IPv6 connection before reporting `Applied`; it neither restarts nor cancels
the IPv4 sibling. A final dial gate closes the race between earlier candidate
admission and policy convergence. Generated TypeScript/schema and UniFFI
Kotlin bindings carry configured/effective/application state, and the shared
React Settings surface exposes the control without adding a Compose screen.

An API 34 arm64 AVD product-policy run observed the fresh default enabled,
applied disable, disabled state after forced process restart, and an expected
`Degraded` re-enable with `effective=false` because the AVD had no eligible
global-unicast IPv6 address. It then removed its application and artifacts.
The same profile subsequently passed on the named API 37 Pixel 7a: exact
serial/model/API/ABI verification, default enabled, applied disable, forced-
restart persistence, expected `Degraded`/`effective=false` re-enable on its
current no-eligible-address network, and cleanup all passed.

Validation:

- focused settings migration, persistence, convergence, source-filter, dial-
  gate, plaintext-cancellation, and MSE-cancellation tests;
- `npm run generate --prefix clients/web` with no generated drift;
- `npm run typecheck --prefix clients/web`;
- `npm run test --prefix clients/web`;
- `clients/android/build.sh` for `x86_64` and
  `arm64-v8a`; and
- `python3 clients/android/run_bootstrap.py --target avd
  --profile product-ipv6-policy --no-build`.

### Gate 6: Observability and recorded evidence

Completed on 2026-08-09. The bounded DHT observation now reports independent
IPv4 and IPv6 lifecycle, identity, routing, transaction, lookup, rejection,
and datagram facts while retaining one actor, command route, observation
owner, and UDP owner. Transport diagnostics separately retain configured and
effective family state and typed bind/address-selection degradation.

The controlled pinned-libtorrent `2.0.13.0` IPv6-loopback profile passed:

```text
ipv6_direct_download=verified ipv6_dht_discovery=verified
payload_hashes=verified find_node_queries=1 get_peers_queries=3
announce_peer_queries=2 incoming_bep32_queries=8
```

The DHT-only libtorrent leecher discovered the RSTorrent announcement through
the IPv6 node and hash-verified the exact payload. The direct IPv6 TCP control
completed in 0.739 seconds. Incoming `ping`, `find_node` with absent, `n4`,
`n6`, and dual `want`, `get_peers`, and `announce_peer` exercised the same
runtime node.

One bounded outbound-only public Big Buck Bunny metadata run then completed
through the ordinary dual-family product lookup in 107.553 seconds. IPv4
bound an ephemeral wildcard endpoint, observed a public external address,
reached 18 routing nodes, received its first valid response and eight-node
threshold at 0.621 seconds, issued 189 queries, received 149 responses,
discovered 72 peers, and sent/received 19,191/48,719 datagram bytes. IPv6 bound
the probe-selected temporary global-unicast address on an ephemeral port,
observed that same address externally, reached 40 routing nodes, received its
first valid response at 0.621 seconds and eight-node threshold at 1.218
seconds, issued 60 queries, received 41 responses, discovered no peer value in
this run, and sent/received
7,308/17,293 datagram bytes. The merged lookup acquired and hash-verified the
21,307-byte info dictionary; no payload file was requested. This proves live
IPv6 DHT participation and dual-family product progress, not that the IPv6
leg supplied the winning peer or that incoming IPv6 is reachable.

The final repository baseline passed workspace formatting, clippy with
warnings denied, all Rust workspace tests, generated-contract drift,
TypeScript typecheck and unit tests, the full Playwright suite, both Android
ABIs, the API 34 AVD and API 37 Pixel 7a policy profiles, the pre-existing IPv4
DHT harness, and the new controlled IPv6 DHT harness. The refactor audit
retained the single DHT actor and session-network reconciler; their post-slice
size is recorded
in `code-organization-and-refactoring.md` as a watch point rather than a new
owner or prerequisite split.
