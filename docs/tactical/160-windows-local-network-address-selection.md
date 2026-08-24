# Tactical 160: Windows Local-Network Address Selection

Status: **Decision-complete and selected as Now (2026-08-24).** This bounded
repair blocks completion of desktop release/updater Tactical `158` because a
fresh Windows profile cannot currently reach the product surface.

Topics: `beta-release-readiness`, `capability-readiness`,
`incoming-reachability-and-seeding`

Dependencies: completed incoming-listener Tacticals
[`088`](088-upnp-mapped-external-tcp-seeding.md),
[`089`](089-coordinated-session-listen-sockets.md), and
[`102`](102-ordinary-incoming-listener-settings.md); completed dual-stack
Tactical [`112`](112-dual-stack-transport-and-ipv6-dht.md); active desktop
release/updater Tactical
[`158`](158-desktop-signed-packaging-and-updater.md); and completed
cross-platform presubmit Tactical
[`159`](159-cross-platform-presubmit-ci.md).

## Decision And Desired Outcome

Repair fresh-profile Windows startup without weakening the listener contract
or introducing platform interface-enumeration code.

The product intentionally binds its ordinary IPv4 TCP and UDP sockets to
`0.0.0.0` so one listener can accept traffic arriving on any interface. It
must separately report one concrete, eligible IPv4 address for listener
status, tracker/DHT bookkeeping, and UPnP. The current selector connects a
zero-byte UDP route probe to the SSDP multicast endpoint and reads the source
chosen by the kernel. On the Windows release appliance that connect succeeds
but selects loopback even though Windows has one active eligible IPv4 address.
The selector rejects loopback, but `SessionSocketSet` then substitutes the
wildcard bind address. The web contract correctly rejects that wildcard as an
invalid local-network listener, leaving the installed app on its startup error
surface.

A target-native comparison on 2026-08-24 established:

- the SSDP multicast route probe connects but selects loopback, matching
  neither an active adapter nor Windows' best route;
- a zero-byte UDP connect to documentation address `192.0.2.1:1` selects an
  eligible address on the active adapter and exactly matches the source from
  Windows `Find-NetRoute`; and
- a public-unicast control chooses the same source, so the documentation
  target exercises ordinary route selection without involving a third party.

Keep the SSDP route as the primary choice because it best represents the LAN
interface relevant to UPnP. On Windows only, try the documentation-unicast
route on a fresh socket when the primary probe fails or yields an ineligible
address. The first eligible result wins. No datagram is sent by either
`connect` operation.

## Scope And Stopping Condition

This tactical owns:

- a small task-free IPv4 route-probe helper with explicit eligible-address
  selection;
- the existing SSDP target as the primary route on every platform;
- one Windows-only documentation-unicast fallback using `192.0.2.1:1`;
- removal of the invalid wildcard-as-peer-address fallback;
- typed address-unavailable degradation when no probe yields a concrete
  address, retaining the application and its UDP discovery service rather
  than publishing an invalid listener record;
- deterministic eligibility, probe-order, and error-path coverage;
- one native Windows engine regression in ordinary desktop presubmit CI;
- native Windows VM proof that a fresh application service starts with a
  concrete eligible IPv4 listener; and
- a clean installed Windows package smoke before Tactical `158` resumes its
  cross-version update gate.

The tactical stops when local Rust validation, the Windows-native selector
test, a clean Windows application-service/package launch, and hosted Windows
presubmit all pass, and the owning topics record the exact evidence. It does
not publish another release.

## Contracts And Invariants

- The TCP and UDP sockets continue binding `0.0.0.0`; route selection changes
  only the concrete peer/listener address reported above the socket layer.
- A reported local-network listener address is concrete, IPv4, non-loopback,
  non-multicast, and non-broadcast. Wildcard is never reported as a successful
  listener address.
- An explicit test override is still validated and never gains a fallback.
- Each route target gets a fresh ephemeral UDP socket. `connect` reads routing
  state and sends no payload.
- Windows uses at most two sequential probes per transport generation. Other
  platforms retain exactly one.
- Failure to find an eligible address becomes the existing typed
  address-unavailable listener degradation. It does not create an acceptor,
  mapping, or advertised endpoint from a wildcard.
- No background task, cancellation owner, queue, retry loop, persistence
  value, generated contract field, unsafe platform FFI, or new dependency is
  introduced.

## Source-First Dossier

The pinned libtorrent commit remains
`7d7fc38fac61177fa5e02148f791b2f65250b09d` from
`reference/pins.toml`.

- `reference/libtorrent/src/enum_net.cpp:628-968`
  (`enum_net_interfaces`) contains the cross-platform enumeration owner. Its
  Windows branch at `:810-928` uses `GetAdaptersAddresses`, retains adapter
  operational/multicast/loopback/PPP flags, and accepts only unicast addresses
  whose DAD state is preferred.
- `reference/libtorrent/src/session_impl.cpp:284-345`
  (`expand_unspecified_address`) expands wildcard listeners across preferred,
  up, same-family interfaces; `:376-391` supplies route classification; and
  `:2003-2039` resolves named interfaces to endpoints.
- `reference/libtorrent/test/test_listen_socket.cpp:326-488` covers wildcard
  expansion, missing default routes, PPP, down, loopback, link-local, and
  global addresses. `reference/libtorrent/test/test_enum_net.cpp` exercises
  local-address and route classification.

The oracle proves that robust mature clients keep wildcard binding separate
from concrete interface facts and exclude unusable adapter state. RSTorrent
does not copy its roughly thousand-line platform enumeration architecture for
this repair: the native best-route-equivalent UDP probe supplies the one
concrete address required by the current single-address product model.

JSTorrent's current desktop daemon uses `if_addrs::get_if_addrs` in
`desktop/io-daemon/src/http.rs` and its port-mapping manager selects an address
on the gateway subnet in
`packages/engine/src/port-mapping/port-mapping-manager.ts`. That confirms the
UPnP need for a concrete LAN address but does not provide a selector to copy.
No reference source or fixture is imported.

## Owner, Data Flow, And Failure Map

`incoming.rs` owns the task-free route probes and IPv4 eligibility rule.
`session_socket.rs` owns wildcard socket allocation and must either attach the
eligible concrete address or return `SessionSocketError::LocalNetworkAddress`.
`session_network.rs` already owns classification of that error as
`AddressUnavailable`, fallback to listener-disabled IPv4 UDP service, and the
bounded runtime/view status. The React validator remains the final hostile
contract boundary and continues rejecting wildcard or loopback listener data.

No task or cancellation topology changes. Probe sockets are local temporaries
dropped before construction returns. Existing transport generations remain
the only lifetime owner.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure | eligibility remains closed; ordered candidate selection takes the first eligible address and rejects an all-ineligible set |
| Scripted runtime | a route probe connects without sending; invalid explicit selection cannot become a wildcard listener; address selection failure produces typed address-unavailable degradation |
| Windows native | current SSDP result is ineligible while the fallback matches an active adapter and Windows' best route; production selector returns a concrete eligible address |
| Application | fresh default `ApplicationService` opens and its listener status passes the generated/web contract semantics |
| Presubmit | the Windows desktop job runs the focused native engine selector test before native desktop tests and package build |
| Installed | clean current Windows package reaches the product surface with a fresh profile, then shuts down without residue |

Run the repository Rust baseline after focused tests. The contract does not
change, so regeneration is not expected; run the web typecheck/tests only if
implementation touches its boundary.

## Non-Goals

- per-interface sockets, BEP 45 multi-address announcement, interface picker
  UI, VPN preference, metered-network policy, or network-change rebinding;
- replacing the product's wildcard IPv4 bind with one interface-specific
  socket;
- `GetAdaptersAddresses`, `GetBestRoute2`, unsafe Windows FFI, or a new network
  interface dependency;
- changing IPv6 source selection, UPnP discovery/mapping policy, or Android
  network behavior;
- accepting loopback or wildcard as a local-network listener address;
- release publication, signing changes, or an additional updater release; or
- Intel macOS installed testing and the separate Linux x86_64 updater gate.

## Escalation And Next Boundary

Implementation, focused CI coverage, disposable Windows toolchain setup,
clean package smoke, and documentation reconciliation are authorized. Stop for
direction if the documentation-unicast probe does not match Windows' best
route on a representative configuration, if correct behavior requires a new
dependency or unsafe native API, if the fix changes multi-interface/VPN
product policy, or before publishing another release.

After this tactical completes, return Tactical `158` to **Now**, repeat the
clean installed Windows `0.1.0`-to-newer update using the repaired package,
and finish its separate Linux x86_64 gate.
