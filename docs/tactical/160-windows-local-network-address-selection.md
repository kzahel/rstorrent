# Tactical 160: Windows Local-Network Address Selection

Status: **Complete (2026-08-24).** The source defect and native presubmit gap
are closed. Desktop release/updater Tactical `158` again owns the remaining
signed installed-package and cross-version proof because the public `0.1.0`
and `0.1.1` packages necessarily predate this repair.

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
- native Windows VM proof that the fallback selects the active eligible IPv4
  adapter and matches Windows' best route; and
- hosted Windows proof that fresh default desktop application services open,
  the production selector returns an eligible address, and the unsigned
  package still builds.

The tactical stops when local Rust validation, the Windows-native selector
test, fresh-default desktop application-service tests, hosted Windows package
build, and the owning documentation pass. A clean installed smoke cannot use
a repaired package until Tactical `158` publishes one, so it remains that
release tactical's gate rather than forcing publication into this repair.

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
| Installed | transferred to Tactical `158`: publish a repaired signed package, then prove fresh-profile launch and the exact older-to-newer update |

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

Implementation, focused CI coverage, disposable Windows toolchain setup, and
documentation reconciliation are authorized. Stop for
direction if the documentation-unicast probe does not match Windows' best
route on a representative configuration, if correct behavior requires a new
dependency or unsafe native API, if the fix changes multi-interface/VPN
product policy, or before publishing another release.

After this tactical completes, return Tactical `158` to **Now**, repeat the
clean installed Windows `0.1.0`-to-newer update using the repaired package,
and finish its separate Linux x86_64 gate.

## Implementation And Evidence

Commit `1f408d3` keeps the SSDP probe primary, adds the Windows-only TEST-NET-1
fallback on a fresh UDP socket, factors the task-free probe and eligibility
check, and turns an unsuccessful selection into the existing typed socket
error. `SessionSocketSet` no longer substitutes `0.0.0.0` as a successful peer
address. The React validator and generated contract are unchanged.

The same commit adds an ineligible-override regression and one
Windows-native selector test. The Windows desktop CI leg runs that engine test
in addition to its eight desktop tests; the latter include two fresh-default
`ApplicationService` opens. Hosted Windows x86_64 run
[`32701109115`](https://github.com/kzahel/rstorrent/actions/runs/32701109115)
passed the selector, desktop tests, and unsigned NSIS build. That run's Linux
Rust job independently exposed a scheduler-dependent storage-pool test;
commit `7be2397` replaces its assumed overlap with the existing controlled
platform broker and passes 25 consecutive focused runs locally. Follow-up
`main` run
[`32703372543`](https://github.com/kzahel/rstorrent/actions/runs/32703372543)
passes all seven jobs, including the repaired storage test and the same native
Windows selector, fresh-default desktop, and package evidence. Its first iOS
attempt hit a hosted-runner application-launch timeout; one failed-job rerun
passed the unchanged simulator tests and unsigned archive. No product check
was skipped or weakened.

Target-native Windows comparison observed one eligible active IPv4 address.
The current SSDP connect selected loopback and matched neither that adapter nor
Windows' best route. Both the TEST-NET-1 fallback and a public-unicast control
selected the eligible active adapter; the fallback exactly matched
`Find-NetRoute`. No concrete appliance address, user, or identifier is
retained.

An additional ARM64 appliance source build was attempted only as
corroboration. Rust and the ARM64 MSVC workload were already installed, but
OpenSSH required explicit developer-environment initialization. The clean
native dependency build exceeded its bounded validation interval and is not
claimed as a pass. Its exact build processes and temporary clone were removed;
the isolated workspace was cleanly stopped and discarded without force. This
does not weaken the passing hosted Windows x86_64 production-selector and
application-service evidence.

Local validation passes:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings`;
- `cargo test --workspace` before and after the deterministic CI-test repair;
- focused local-network/session-socket tests;
- 25 consecutive controlled storage singleflight tests; and
- `actionlint` `v1.7.9` against every workflow.

No dependency, unsafe code, persistence value, generated contract, background
task, or non-Windows route choice changed. Tactical `158` is again **Now** and
owns the first signed repaired package, clean fresh-profile launch, exact
Windows updater repetition, and the separate Linux x86_64 update gate.
