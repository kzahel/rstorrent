# Tactical 127: Mapped uTP WAN Interoperability

Status: **Active** on 2026-08-10. Human review rejected Tactical `126`'s
incorrect assumption that an Internet-reachable peer must have a global IPv4
address assigned directly to its interface. The user authorized setup of the
exact pinned libtorrent oracle on `pimom`, a temporary UDP UPnP mapping on its
NAT gateway when available, and a temporary UDP UPnP mapping on the local
network as the fallback direction. This tactical supersedes Tactical `126` as
the executable Stage 4 slice while retaining its evidence-limited result.

Topics: `utp-transport-campaign`, `incoming-reachability-and-seeding`,
`performance-and-live-evidence`, `peer-lifecycle`, `protocol-support`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tactical
[`125`](125-shared-udp-utp-runtime-and-loopback-interop.md) supplies the
bounded shared-UDP runtime, exact fixture, two-role loopback interoperability,
transport observations, and terminal cleanup contract. Completed Tactical
[`088`](088-upnp-mapped-external-tcp-seeding.md) supplies the bounded UPnP IGD
v2 discovery, lease, verification, renewal, and deletion owner. Closed
Tactical [`126`](126-controlled-outbound-utp-wan-evidence.md) records the
initial read-only `pimom` facts but does not constrain a NATed peer to a
directly assigned public address.

## Decision And Motivation

Prove one direct public-path uTP transfer between this development machine and
the authorized remote host `pimom`. SSH over Tailscale remains a control plane
only. The uTP data plane must use a globally routable external IPv4 address
and an exact temporary UDP mapping reported by an Internet gateway; it must
not use the Tailscale address, an SSH tunnel, port forwarding through SSH, or
a same-LAN path.

Try the least invasive remote-listener direction first:

1. install or build libtorrent `2.0.13.0` in an isolated user-owned environment
   on `pimom`;
2. start a forced-uTP libtorrent seed on one fixed UDP port, with its built-in
   UPnP enabled and NAT-PMP disabled;
3. require a successful UDP mapping plus gateway-reported globally routable
   external IPv4 address; and
4. have the local RSTorrent diagnostic leecher dial that public endpoint.

If the remote gateway cannot provide an eligible UPnP UDP mapping or globally
routable external IPv4 address, reverse the roles rather than treating the
remote host as unusable. Extend RSTorrent's existing UPnP owner to express TCP
or UDP explicitly, preserve all product TCP behavior, obtain and verify a
temporary local UDP mapping, then have a remote forced-uTP libtorrent leecher
dial the local RSTorrent diagnostic seed. A protocol, integrity, or runtime
failure after one direction has established reachability does not authorize
silently switching direction; only a reachability-capability failure selects
the fallback.

Neither path enables uTP in the product, advertises uTP to discovery, persists
a mapping or setting, nor changes the BEP 29 support claim.

## Stopping Condition

The tactical reaches one of two explicit stops:

1. **Complete WAN evidence:** one mapped public-path transfer passes every
   positive gate below, all temporary mappings/processes/artifacts are removed,
   focused and repository validation passes, the owning documents are
   reconciled, and the campaign returns for human review before product policy;
   or
2. **Closed evidence-limited:** both authorized gateways lack usable UPnP UDP
   mapping capability or a globally routable external IPv4 address, or the
   exact oracle cannot be established within the bounded setup contract. The
   negative capability facts and verified cleanup are recorded without a WAN
   interoperability claim.

The positive gates are:

1. The selected data endpoint is the exact externally mapped IPv4 address and
   UDP port. It is global unicast and not loopback, link-local, RFC 1918,
   shared `100.64.0.0/10`, documentation, multicast, unspecified, reserved, or
   the SSH control endpoint.
2. Route inspection on the dialing host proves that the selected peer address
   leaves through its ordinary Internet interface, not a Tailscale/overlay
   interface. No SSH forwarding or tunnel carries uTP.
3. The remote oracle reports libtorrent `2.0.13.0`. TCP, MSE, DHT, LSD,
   NAT-PMP, trackers, and web seeds are disabled. UPnP is enabled only in the
   remote-listener direction; the remote-leecher fallback uses no remote
   mapping. At most one peer is observed.
4. The exact 2,097,883-byte single-file fixture, 65,536-byte piece geometry,
   and SHA-1 `cdce24126a8e65854d876c0b83ad3ba19748f6dc` pass without TCP or a
   discovery mechanism.
5. The result records direction, redacted transient endpoints, oracle version
   and settings, elapsed time, payload and packet/byte counts, loss reductions,
   timeout collapses, retransmission counts, RTT/RTO, raw base and queue delay,
   congestion and advertised receive windows, selected MTU, queue/byte
   high-waters, and terminal owner counts.
6. The gateway mapping uses UDP, matches the listener's internal and requested
   external port, has a finite lease, and is queried or alert-confirmed after
   creation. Cleanup explicitly deletes or disables the exact lease and then
   confirms absence when the gateway supports query.
7. Both peer processes terminate through normal owner paths. Terminal local
   session-UDP tasks, uTP connections, half-opens, and queued datagrams are
   zero; the remote session has zero peers; and no owned process survives.
8. Every local and remote temporary fixture, output, report, and raw diagnostic
   file is removed after the bounded summary is extracted. An uncertain
   mapping or artifact cleanup result fails the gate.

## External Setup And Execution Contract

### Remote inventory and oracle setup

The first active step is a bounded read-only inventory over `ssh pimom` of:

- installed compiler, linker, CMake, Ninja/Make, pkg-config, Python/pip/venv,
  Git, Boost, OpenSSL, and package-manager facilities;
- free disk and memory relevant to the bounded build;
- existing user firewall state and ordinary route/interface shape without
  changing either; and
- UPnP discovery reachability or available diagnostic clients without creating
  a mapping.

Prefer an isolated user-owned virtual environment and build directory. Use
the official Python package when an exact compatible `2.0.13.0` wheel exists;
otherwise build the exact libtorrent commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. The authorized setup may install
only exact named build/runtime prerequisites through the host package manager
when user-space setup is insufficient. It must not perform a distribution
upgrade, remove or replace unrelated packages, enable a service, alter a
firewall, write permanent router configuration, add credentials, or change
Tailscale. A password or a need for one of those broader changes stops setup
rather than weakening the boundary.

The reusable isolated oracle environment may remain after the run so the
pinned test is reproducible. Temporary source/build trees may also remain only
when they are the owned oracle environment documented by the result. Payloads,
metainfo, run directories, logs, and mappings are always ephemeral and must be
removed. Record exact installed packages and retained paths without committing
machine-specific home paths.

### Remote-listener primary direction

The attached remote helper owns one libtorrent session and torrent, listens
on one explicit IPv4 port, and emits bounded line-delimited JSON. It waits for
UPnP `portmap_alert` success and an eligible external-address observation
before declaring readiness. It reports port mapping errors separately from
uTP transfer failures, accepts one stop command, removes the torrent, disables
UPnP or pauses/aborts the session, waits for mapping deletion evidence, emits
terminal state, and exits.

The local owner retains the SSH child, validates its structured output, checks
the local route to the external endpoint, and starts the explicit diagnostic
WAN leecher. The existing loopback role remains loopback-only.

### Local-listener fallback direction

RSTorrent's UPnP API gains an explicit transport value used consistently by
query, add, verify, renew, and delete. Existing application reachability calls
remain explicitly TCP. Only the diagnostic WAN seed requests UDP, and it does
not enter persisted settings or ordinary reachability presentation.

The diagnostic seed binds the existing shared IPv4 session UDP owner on an
ordinary eligible local address, obtains the UDP mapping, reports the eligible
external endpoint only after verification, and accepts exactly one incoming
uTP peer through the existing upload owner. A remote attached libtorrent
leecher checks its route to that public endpoint, downloads and verifies the
fixture, emits bounded terminal state, and exits. The local owner then removes
the exact mapping before closing the socket generation.

### Orchestration and cleanup

One local harness selects the primary or capability-gated fallback direction,
uses `mktemp -d` for run state, applies a 90-second whole-case deadline and a
five-second forced-cleanup allowance, and keeps every process attached. Remote
cleanup targets only the exact validated run directory and process created by
the invocation; no broad glob, unresolved environment variable, or unrelated
process name is accepted. Cleanup runs in `finally` for success, protocol
failure, timeout, SSH loss, malformed output, and interruption.

## Resource, Privacy, And Security Bounds

| Resource | Bound |
| --- | ---: |
| Payload | 2,097,883 bytes |
| Piece size / count | 65,536 bytes / 33 |
| Remote libtorrent connections / peer list | 4 / 8 |
| RSTorrent live uTP connections | exactly 1, global service maximum 64 |
| Temporary UPnP mappings | exactly 1 during selected direction |
| UPnP lease | finite, at most 3,600 seconds |
| Scenario wall time | 90 seconds |
| SSH connect and cleanup allowance | 10 seconds / 5 seconds |
| Captured stdout/stderr diagnostics | 50 lines per stream |
| Remote per-run staged bytes | at most 16 MiB |
| Local per-run staged bytes outside build output | at most 16 MiB |
| Retained raw packet capture | none |

Counters saturate. No payload bytes, peer IDs, IP addresses, SSH material,
tokens, environment, home paths, router identifiers, or unbounded remote
output enter committed files. Exact addresses may exist in ephemeral output
while the gate runs. Durable evidence reports address class, direction,
mapping mechanism, and stable versions; it redacts identifying endpoint data.

UPnP is hostile network input. XML/SOAP bodies, URLs, redirects, timeouts, and
mapping descriptions retain Tactical `088`'s existing bounds. A reported
external address is validated as global unicast before any dial or claim.

## Shape-Changing Failure Cases

- Remote UPnP discovery is unavailable, the gateway rejects UDP, the external
  address is ineligible, or post-create verification fails. Clean any possible
  remote lease, record a capability failure, and try the authorized local
  mapping direction.
- Both networks lack an eligible verified UPnP UDP endpoint. Close evidence-
  limited after proving no mapping/process/run-artifact residue.
- Exact libtorrent cannot be installed or built without a broad OS, firewall,
  router, credential, or service change. Stop setup and record the missing
  prerequisite; do not substitute a different version.
- A mapping succeeds but the peer cannot establish uTP. Preserve the selected
  direction and diagnose protocol/runtime behavior; do not call it a mapping
  capability failure or retry over TCP.
- A transfer succeeds but either implementation reports TCP, multiple peers,
  the wrong endpoint, missing required counters, a hash mismatch, or nonzero
  terminal ownership. Treat it as a failed evidence gate.
- SSH disconnects during a run. Cancel the local owner, terminate the exact
  remote child if reachable, remove the exact run directory and mapping, and
  distinguish orchestration failure from uTP failure.
- Mapping deletion cannot be verified. Do not create a second mapping or mark
  the run complete; retain bounded identifying information only long enough to
  target the exact cleanup operation.

## Source-First Record

Re-read managed BEP 29 at BitTorrent BEP commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`, especially UDP layering,
advertised receive windows, packet sequence identity, timestamps, delay
feedback, retransmission, and raw timestamp difference versus base-subtracted
queue delay.

Re-inspected Rasterbar libtorrent commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `test/test_utp.cpp`, especially `test_transfer`, for TCP/MSE/discovery-
  disabled exact uTP transfer and joined session ownership;
- `src/utp_socket_manager.cpp` and
  `include/libtorrent/aux_/utp_socket_manager.hpp` for UDP send ownership,
  endpoint/connection-ID dispatch, MTU selection, controller settings, and
  finite socket removal;
- `src/upnp.cpp` and `include/libtorrent/upnp.hpp` for explicit TCP/UDP mapping,
  external-address observation, renewal, and deletion;
- `test/test_upnp.cpp`, including its UDP `add_mapping` case, for protocol-
  typed mapping behavior;
- `examples/upnp_test.cpp` for `portmap_alert`, `portmap_error_alert`, external
  address alerts, and cleanup after disabling UPnP; and
- `bindings/python/test.py` and the Python alert bindings for explicit listen
  interfaces, alerts, and session lifecycle.

Re-read the UPnP Device Architecture 2.0 and Internet Gateway Device v2
documents already pinned by `docs/references.md` and Tactical `088`, especially
`WANIPConnection:2` `GetExternalIPAddress`, `GetSpecificPortMappingEntry`,
`AddPortMapping`, `DeletePortMapping`, protocol values, finite leases, and SOAP
faults.

Adopted behavior is an explicit UDP mapping, alert/query-confirmed external
endpoint, finite lease, and explicit cleanup. Intentional differences remain
RSTorrent's fixed 548-byte Stage 3 datagram MTU, one diagnostic connection,
bounded service snapshots, and no product uTP selection or advertisement.
The local UPnP generalization preserves product TCP policy and exposes UDP
only to the controlled diagnostic.

JSTorrent remains at tracked commit
`9895410beeed6aff554053769bd006a3fbd373ef` and has no uTP runtime or mapped WAN
case to preserve. No reference source, fixture, or test vector is copied. The
remote helper and harness are independently authored; there is no new
RSTorrent runtime dependency, vendoring, unsafe code, or notice change.

## Owner, Task, Cancellation, And Dependency Map

| Owner | Bounded work | Cancellation and termination |
| --- | --- | --- |
| Local WAN harness | Direction choice, SSH child, route checks, deadlines, exact cleanup | Whole-case timeout or interruption cancels children and runs `finally` cleanup |
| Selected listener | One uTP socket generation and one fixture owner | Stops after verified transfer or case cancellation |
| Selected gateway-mapping owner | One UDP lease and bounded renewal/query state | Deletes exact lease before socket teardown; finite expiry is crash backstop |
| Selected leecher | One forced-uTP connection and verifier | Stops after exact hash, peer error, or case cancellation |
| Remote oracle environment | Pinned reusable libtorrent runtime only | No background task; per-run processes remain SSH-attached |

Protocol values, codecs, and deterministic connection state remain independent
of sockets, SSH, filesystems, and UPnP. The existing engine UPnP runtime may
depend inward on a small protocol enum; protocol/domain code does not depend
outward on the diagnostic harness. Peer framing continues to consume the
transport-neutral ordered stream.

## Staged Execution And Validation

1. Commit this tactical and authoritative queue/checkpoint changes before any
   further remote mutation.
2. Inventory `pimom` and both gateways' non-mutating capability surface.
3. Establish and verify the isolated exact libtorrent oracle; commit any
   repository-owned setup helper separately.
4. Add required RSTorrent retransmission/loss observations and the explicit
   WAN diagnostic roles. Generalize UPnP to typed TCP/UDP while proving all
   existing product calls remain TCP. Re-run the complete Stage 3 loopback
   gate unchanged and commit the bounded runtime change.
5. Add the attached remote helper and local orchestration/cleanup harness with
   deterministic failure tests; commit it before external data-plane traffic.
6. Run the remote-mapped direction or its capability-gated local-mapped
   fallback once, repair failures only inside this tactical, and clean all
   leases and per-run state.
7. Run and record, in proportion to the reached path:

```text
source ~/.profile
cargo test -p rstorrent-engine utp
cargo test -p rstorrent-engine port_mapping
uv run --project tests/interop --locked \
  python tests/interop/utp_rstorrent_interop.py
uv run --project tests/interop --locked \
  python tests/interop/utp_rstorrent_wan.py --host pimom
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

8. Record exact bounded evidence, reconcile all owning topics while leaving
   BEP 29 **Unsupported**, commit the completed or evidence-limited result, and
   stop for human review before product selection, advertisement, or broader
   public testing.

## Result And Evidence

### Remote capability and oracle checkpoint

The bounded inventory found Debian 13 on Linux/aarch64 with Python `3.13.5`,
GNU C++ `14.2.0`, `make`, `pkg-config`, a noninteractive package-manager path,
and ample space on the home filesystem. Available memory at inspection was
about 662 MiB. CMake, Ninja, Boost headers, Python development headers, and
system libtorrent were absent; Debian's package candidate was libtorrent
`2.0.11`, not the pinned version. No package-manager or `sudo` action followed.

The installed MiniUPnP client discovered one connected remote Internet Gateway
Device through the ordinary Ethernet route. Its reported external IPv4 address
passed global-unicast validation. This proves discovery and external-address
observation only: no UDP mapping was created at this checkpoint. The local
machine did not have the MiniUPnP command-line client, which does not affect
the existing engine-owned UPnP fallback.

The official PyPI `2.0.13` release supplied an exact CPython 3.13 manylinux
ARM64 wheel, avoiding a source build and all system-package changes. One
dedicated user-owned virtual environment now retains:

- wheel
  `libtorrent-2.0.13-cp313-cp313-manylinux_2_17_aarch64.manylinux2014_aarch64.whl`;
- SHA-256
  `065e36d476e3dc8df7680205f4134cbcd02bb00da48439ccd0023e19371a4983`;
- imported binding version `2.0.13.0`; and
- available `portmap_alert`, `portmap_error_alert`, `external_ip_alert`, listen
  and peer alerts, plus explicit UPnP, NAT-PMP, TCP, uTP, and listen-interface
  settings.

Installation used `venv` plus the direct official wheel URL with its SHA-256
fragment, `--no-deps`, and binary-only selection. The temporary atomic-install
directory was removed; only the authorized reusable oracle environment
remains. No fixture, metainfo, run directory, listener, mapping, background
process, firewall rule, router configuration, or Tailscale change exists yet.
