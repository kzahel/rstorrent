# Tactical 126: Controlled Outbound uTP WAN Evidence

Status: **Closed, evidence-limited** on 2026-08-10. Human review accepted Stage
4 recommendation A after completed Tactical `125` reached its post-loopback
checkpoint. Commit `302d840` fixed the direct-route, outbound-only transfer,
evidence, resource, cleanup, and stopping contracts before external execution.
The authorized read-only preflight then found neither a directly routed IPv4
endpoint nor the exact libtorrent oracle on `pimom`, so the tactical stopped
before code, staging, or uTP traffic and makes no WAN interoperability claim.

Topics: `utp-transport-campaign`, `performance-and-live-evidence`,
`peer-lifecycle`, `protocol-support`, `capability-readiness`,
`oracle-driven-engine-campaign`

Dependencies: completed Tactical
[`125`](125-shared-udp-utp-runtime-and-loopback-interop.md) supplies the
bounded shared-UDP runtime, controlled outgoing peer composition, exact
fixture, pinned-libtorrent two-role loopback gate, transport observations, and
terminal cleanup contract. Completed Tactical
[`121`](121-deterministic-utp-loss-congestion-and-mtu.md) owns the unchanged
RFC 6817 controller and deterministic loss/MTU evidence.

## Decision And Motivation

Test the first non-local RSTorrent uTP path by having the local diagnostic
leecher dial a forced-uTP libtorrent seed on the authorized remote host
reachable for orchestration through `ssh pimom`. The uTP packets under test
must use a directly routed IPv4 endpoint, never SSH forwarding, a VPN or
overlay address, or a same-LAN endpoint. RSTorrent remains the initiator so
this test requires no claim about local incoming UDP reachability.

The host alias currently resolves for SSH control to an address in the
`100.64.0.0/10` shared range. That is acceptable only as the control plane and
is explicitly ineligible as the uTP evidence endpoint. Before adding WAN mode
to the diagnostic or staging a remote payload, a read-only preflight must find
an already usable directly assigned global-unicast IPv4 endpoint on the remote
host. The tactical does not create a router mapping, firewall exception,
Tailscale serve/funnel rule, or tunnel to manufacture reachability.

If that precondition is absent, close the tactical evidence-limited with the
observed route facts and no WAN transfer claim. Do not broaden the work into
reachability configuration merely to make the gate pass.

## Stopping Condition

This tactical reaches one of two explicit stops:

1. **Complete WAN evidence:** every positive gate below passes, the exact
   transfer and bounded transport observations are recorded, all local and
   remote temporary state is removed, owning topics are reconciled, and the
   campaign returns for human review before product policy; or
2. **Closed evidence-limited:** read-only preflight proves that `pimom` lacks
   an eligible directly routed IPv4 endpoint or the exact pinned libtorrent
   runtime, or the positive transfer cannot be obtained without a forbidden
   reachability change. The negative result and cleanup are recorded without
   claiming WAN interoperability.

The positive gates are:

1. SSH is used only to inspect and supervise the authorized remote host. The
   uTP peer endpoint is IPv4 global unicast and is neither the SSH control
   endpoint nor loopback, link-local, RFC 1918, shared `100.64.0.0/10`,
   documentation, multicast, or otherwise special-use space.
2. The remote seed uses the already installed Python libtorrent package at
   exactly `2.0.13.0`. TCP, MSE, DHT, LSD, UPnP, NAT-PMP, trackers, and web
   seeds are disabled; incoming uTP is enabled; and at most one peer is
   observed.
3. One explicit diagnostic-only RSTorrent WAN leecher binds an ephemeral IPv4
   UDP socket, selects `NetworkPolicy::Online`, accepts only an eligible
   global-unicast IPv4 target, and reuses the same controlled handshake,
   framed peer I/O, piece verification, and output path as loopback. Existing
   loopback roles remain closed to non-loopback endpoints.
4. The exact 2,097,883-byte single-file fixture, 65,536-byte piece geometry,
   and SHA-1 `cdce24126a8e65854d876c0b83ad3ba19748f6dc` pass without TCP or a
   discovery mechanism.
5. The result records direction and exact transient endpoints, version and
   settings, elapsed time, payload and packet/byte counts, loss reductions,
   timeout collapses, retransmission counts, RTT/RTO, raw base and queue delay,
   congestion and advertised receive windows, selected MTU, queue and byte
   high-waters, and terminal owner counts. The committed summary redacts IP
   addresses as required by the live-evidence safety policy.
6. Both processes terminate through their normal owner paths. Terminal local
   session-UDP tasks, uTP connections, half-opens, and queued datagrams are
   zero; the remote session has zero peers; and no owned process survives.
7. The remote working directory and every local temporary fixture, output,
   report, and raw diagnostic file are removed after the bounded summary is
   extracted. A cleanup failure is a failed gate, not a warning.
8. Focused deterministic/loopback tests, the complete Stage 3 loopback gate,
   the Rust workspace baseline, tactical evidence, campaign checkpoint,
   readiness queue, and unchanged **Unsupported** BEP 29 product claim are
   reconciled before the final commit.

## External Execution Contract

### Direct-route preflight

The first remote interaction is read-only and bounded. Through one
noninteractive SSH command it records only:

- operating-system and architecture identity;
- Python version and whether `import libtorrent` reports exactly `2.0.13.0`;
- directly assigned IPv4 interface addresses and route class; and
- availability of `mktemp`, Python, and ordinary process inspection needed
  for cleanup verification.

No public IP echo service, tracker, DHT node, public swarm, or unrelated host
is contacted. An address learned only from Tailscale endpoint state, a NAT
external-address guess, or an SSH socket is not accepted as the uTP target.
The preflight does not use `sudo`, install packages, modify a checkout, change
network configuration, or write outside an eventual `/tmp` work directory.

The exact addresses may exist in ephemeral command output while the gate is
running. Durable documentation records only address class, route mechanism,
and a nonreversible endpoint fingerprint when useful; it does not commit a
peer IP address.

### Remote seed ownership

If preflight passes, the local harness creates one remote directory using
`mktemp -d` beneath `/tmp`, validates the returned absolute path against its
fixed prefix, and copies only the independently authored seed helper, exact
metainfo, and 2,097,883-byte payload into it. The remote helper:

- opens one libtorrent session and one torrent;
- binds `0.0.0.0:0` with the transport settings above;
- emits bounded line-delimited JSON readiness, completion, stats, and errors;
- waits on its attached SSH standard input rather than detaching;
- accepts one `stop` command only after local hash verification; and
- removes the torrent, pauses the session, emits terminal state, and exits.

The local owner keeps the SSH child attached, applies a 90-second whole-case
deadline and a five-second forced-cleanup allowance, and always attempts
remote process and directory cleanup in `finally`. Cleanup uses only the exact
validated directory and process launched by this invocation. It never uses a
broad glob or recursive target derived from an unchecked variable.

### Local WAN role

The diagnostic binary gains an explicit `wan-leecher` role rather than
weakening the existing `leecher` argument validation. The WAN role accepts
one numeric IPv4 global-unicast peer, binds an ephemeral wildcard IPv4 UDP
socket, and uses the existing 30-second uTP connection and peer-I/O bounds
inside the harness's 90-second process bound. It has no DNS, tracker, DHT,
listener advertisement, fallback, retry peer, or product configuration path.

The ordinary product remains unable to select uTP. The diagnostic role is not
exported through the application service, clients, generated contract, or
persisted settings.

## Resource And Privacy Bounds

| Resource | Bound |
| --- | ---: |
| Payload | 2,097,883 bytes |
| Piece size / count | 65,536 bytes / 33 |
| Remote libtorrent connections / peer list | 4 / 8 |
| RSTorrent live uTP connections | exactly 1, global service maximum 64 |
| Scenario wall time | 90 seconds |
| SSH connect and cleanup allowance | 10 seconds / 5 seconds |
| Captured stdout/stderr diagnostics | 50 lines per stream |
| Remote staged bytes | at most 16 MiB |
| Local staged bytes outside build output | at most 16 MiB |
| Retained raw packet capture | none |

Counters saturate. No payload bytes, peer IDs, IP addresses, SSH material,
tokens, environment, home paths, or unbounded remote output enter committed
files. The final result may report endpoint families, route class, port
direction, and stable tool versions without identifying the remote network.

## Shape-Changing Failure Cases

- The SSH alias resolves to an overlay address but the remote host has no
  directly assigned eligible IPv4 address. Close evidence-limited before
  modifying the diagnostic or staging the fixture.
- A candidate address is private, shared, documentation, loopback, link-local,
  multicast, unspecified, reserved, or not assigned to the remote interface.
  Reject it before starting either uTP role.
- Remote libtorrent is absent or not exactly the locked version. Do not
  install or substitute another version under this tactical.
- The remote seed binds only the SSH/overlay interface, advertises port zero,
  or requires a mapping/firewall change. Stop and clean up without tunnelling.
- SSH disconnects while the seed runs. Cancel the local leecher, terminate the
  exact remote process if it still exists, remove the exact work directory,
  and report the orchestration failure separately from a uTP failure.
- SYN retry exhaustion, RESET, peer-handshake rejection, inactivity, wrong
  payload, or hash mismatch remains a failed WAN result with bounded local
  terminal observations. Do not retry over TCP.
- A transfer succeeds but either implementation reports a TCP peer, more than
  one peer, missing transport counters, or nonzero terminal ownership. Treat
  it as a failed evidence gate.
- Remote or local cleanup cannot be verified. Preserve only bounded failure
  diagnostics, do not mark the tactical complete, and do not target unrelated
  processes or paths in an attempted repair.

## Source-First Record

Re-read managed BEP 29 at BitTorrent BEP commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`, especially UDP layering,
advertised window semantics, packet sequence identity, timestamps, delay
feedback, loss behavior, and the fact that raw timestamp difference contains
clock offset while queue delay comes from subtracting the base.

Re-inspected Rasterbar libtorrent commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `test/test_utp.cpp`, `test_transfer`: disables TCP, MSE, DHT, LSD, UPnP, and
  NAT-PMP around an exact uTP transfer and joins both session proxies;
- `src/utp_socket_manager.cpp`, `mtu_for_dest`, `incoming_packet`, and
  `send_packet`: address-family MTU selection, UDP send ownership,
  endpoint-plus-ID lookup, and incoming-uTP admission;
- `include/libtorrent/aux_/utp_socket_manager.hpp`: finite socket ownership,
  controller settings, MTU restriction, writable/drained subscribers, and
  socket removal; and
- `bindings/python/test.py`: Python binding session construction with explicit
  `listen_interfaces` and alert-driven evidence.

Adopted behavior is explicit listener selection, forced-TCP-disabled uTP,
bounded alert/stat collection, and joined session cleanup. Intentional
differences remain RSTorrent's fixed 548-byte Stage 3 datagram MTU, one
diagnostic outgoing connection, explicit service snapshots, and no product or
incoming-reachability policy.

JSTorrent remains at tracked commit
`9895410beeed6aff554053769bd006a3fbd373ef` and has no uTP runtime or WAN uTP
test to preserve. No reference source, fixture, or test vector is copied. The
remote helper and local harness are independently authored orchestration over
the already locked external libtorrent package; no manifest, runtime
dependency, vendoring, unsafe code, or notice change is planned.

## Staged Execution And Validation

1. Commit this tactical and the authoritative queue/checkpoint update before
   contacting the remote host.
2. Run the read-only direct-route and exact-runtime preflight. Close evidence-
   limited immediately if it cannot meet the fixed precondition.
3. If eligible, add RSTorrent loss/retransmission observations and the
   explicit diagnostic WAN role with deterministic argument/network-policy
   tests. Re-run the Stage 3 loopback gate unchanged.
4. Add the attached remote-seed/local-owner harness, validate its route,
   output, deadline, failure, and cleanup policy without external execution,
   then commit it.
5. Run one authorized outbound WAN case. A failure may be repaired only within
   the accepted diagnostic/runtime boundary; it may not weaken route,
   integrity, transport, resource, or cleanup gates.
6. Run and record, in proportion to the path actually reached:

```text
source ~/.profile
cargo test -p rstorrent-engine utp
uv run --project tests/interop --locked \
  python tests/interop/utp_rstorrent_interop.py
uv run --project tests/interop --locked \
  python tests/interop/utp_rstorrent_wan.py --host pimom
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

## Result And Evidence

The first authorized SSH preflight reached the evidence-limited stopping
condition in 4.5 seconds. It used batch mode, a ten-second connection bound,
disabled forwarding, allocated no terminal, and passed one read-only Python
program over standard input. The command returned successfully and reported:

- Linux on `aarch64` with Python `3.13.5`;
- loopback, one RFC 1918 LAN address, and one `100.64.0.0/10` Tailscale/shared-
  range address as the complete assigned IPv4 set;
- zero directly assigned global-unicast IPv4 addresses; and
- `ModuleNotFoundError` for the system Python `libtorrent` import rather than
  the required locked `2.0.13.0` package.

The SSH control endpoint was the shared-range interface and was never used as
a uTP target. The preflight contacted no public-IP service, tracker, DHT node,
swarm, or other host. It did not run `sudo`, install a package, modify a
checkout or network rule, create `/tmp` state, stage a fixture, bind a
listener, launch a background process, or send a uTP datagram. The attached
remote Python process exited with the SSH command; therefore there was no
remote work directory, payload, listener, or owned process to remove.

Both positive prerequisites independently failed. A useful attempt would now
require installing the exact oracle and creating or identifying direct
external UDP reachability through the remote NAT/firewall. Both actions are
explicit non-goals and human-review gates, so no diagnostic WAN role,
loss/retransmission instrumentation, remote helper, local harness, or runtime
change was added. Tactical `125`'s loopback results remain the highest uTP
interoperability evidence, and BEP 29 remains **Unsupported** as a product
claim.

Validation for the path reached:

```text
ssh -o BatchMode=yes -o ConnectTimeout=10 \
  -o ClearAllForwardings=yes -o RequestTTY=no pimom python3 -
                                                     # read-only preflight passed
git diff --check                                     # passed
```

The full Rust and loopback baselines were not rerun because the tactical
stopped before any source, manifest, test, fixture, or runtime change.

## Non-Goals And Next Boundary

- No reverse-direction remote-initiated uTP, local public UDP listener,
  mapping, pinhole, NAT-PMP, PCP, UPnP, hole punching, or reachability claim.
- No SSH forwarding, SOCKS proxy, VPN/overlay uTP endpoint, relay, TURN-like
  service, same-LAN peer, public swarm, tracker, DHT, LSD, or web seed.
- No IPv6 uTP, active real-socket path-MTU probing, do-not-fragment socket
  change, congestion-controller change, bandwidth policy, or performance
  claim from one sample.
- No MSE-over-uTP, product selection/racing/fallback, listener advertisement,
  setting, status surface, generated contract, or client work.
- No package installation, remote checkout modification, long-lived service,
  firewall/network change, dependency, source import, unsafe code, notice
  change, or protocol-claim promotion.

After positive or evidence-limited closure, stop for human review. A positive
outbound result may support a separately reviewed product-policy design; it
does not authorize Stage 5. An unreachable remote seed selects either a
separate reachability tactical or a pause, never an implicit expansion here.

## Escalation

Ordinary diagnostic argument shape, bounded instrumentation, helper/harness
layout, typed error classification, cleanup mechanics, focused tests,
documentation reconciliation, and coherent commits proceed autonomously.

Stop for direction before installing or upgrading remote software, changing a
firewall/router/VPN, using a tunnel or overlay for the measured packets,
contacting another remote host, adding a dependency or unsafe socket option,
weakening any positive gate, enabling product uTP, expanding to reverse
incoming traffic or IPv6, retaining identifying artifacts, or changing the
protocol-support claim.
