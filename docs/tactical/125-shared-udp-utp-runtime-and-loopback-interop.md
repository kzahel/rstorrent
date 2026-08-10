# Tactical 125: Shared-UDP uTP Runtime And Loopback Interoperability

Status: Complete on 2026-08-10 at the required Stage 3 human-review
checkpoint. Human review accepted recommendation A at Tactical `121`'s Stage
2 checkpoint. Commits `2d33516`, `5dd6d3c`, `c9ab011`, `7de2974`, `fed430c`,
`2384d7c`, and `dc5ab32` implement and validate the bounded slice.

Topics: `utp-transport-campaign`, `peer-lifecycle`,
`incoming-reachability-and-seeding`, `dht-discovery`, `protocol-support`,
`code-organization-and-refactoring`, `capability-readiness`

Dependencies: completed Tactical
[`089`](089-coordinated-session-listen-sockets.md) established one bounded
session UDP receiver and DHT route. Completed Tactical
[`097`](097-live-client-settings-and-replaceable-session-generations.md)
established stable session-network ownership across socket replacement.
Completed Tactical
[`112`](112-dual-stack-transport-and-ipv6-dht.md) made those socket generations
independent per family. Completed Tacticals
[`119`](119-deterministic-utp-transport-core.md) and
[`121`](121-deterministic-utp-loss-congestion-and-mtu.md) supply the hostile
codec and complete deterministic transport state. Completed Tacticals
[`086`](086-long-lived-torrent-peer-runtime.md) and
[`124`](124-duplex-verified-piece-upload.md) supply the existing direction-
neutral peer lifecycle and active bidirectional content path that this slice
must reuse rather than duplicate.

## Decision And Motivation

Install the first real RSTorrent uTP runtime without making uTP an ordinary
product policy. The session's UDP socket remains the sole receiver for each
address family. It classifies uTP-shaped datagrams before the existing DHT
route, feeds a separately bounded uTP runtime, and retains generation identity
on every receive and send. The runtime exposes an ordered asynchronous byte
stream to the same BitTorrent handshake, framing, download, and incoming
upload owners already used by TCP.

The slice is deliberately end to end. A runtime seam that only talks to
another RSTorrent instance would leave the highest-risk question unanswered:
whether the independently authored state actually interoperates with the
deployed packet, handshake, flow-control, and close behavior of pinned
libtorrent. The stopping result is therefore one exact controlled transfer in
each role with TCP disabled, not merely a socket unit test.

This remains an engine and diagnostic capability. It does not choose when a
product dials uTP, advertise uTP availability, accept public uTP by default,
map UDP, compose MSE over uTP, or change the protocol-support claim.

## Stopping Condition

This tactical is complete only when all of the following hold:

1. The one session UDP receive task per active family classifies syntactically
   uTP-shaped input into a dedicated bounded route while all other input keeps
   the existing DHT route and its 1,025-byte malformed sentinel. Neither
   consumer can block or consume the other's queue allowance.
2. One supervised uTP service owns connection lookup, outgoing creation,
   incoming SYN admission, entropy, timers, datagram routing, connection
   workers, cancellation, and joined termination. Lookup uses remote endpoint,
   address family, socket generation, and local receive connection ID.
3. A bounded `UtpStream` implements ordered async read/write semantics. Bytes
   become receive-window credit only when the stream consumer actually reads
   them; writes cannot bypass the deterministic 1-MiB unsent bound; EOF,
   RESET, timeout exhaustion, cancellation, and local shutdown have distinct
   terminal behavior.
4. The existing peer I/O waist accepts either TCP or uTP without making peer-
   wire framing, scheduling, storage, or peer identity depend on UDP. Existing
   TCP behavior and MSE behavior remain green. The controlled uTP path is
   plaintext only.
5. Replacing or removing a family socket cancels and joins every uTP
   connection from the old generation. A stale worker cannot send through,
   receive from, or mutate the replacement generation, while DHT continues on
   every surviving family.
6. Scripted runtime cases cover malformed and unknown datagrams, lookup
   collisions, duplicate SYNs, half-open and queue saturation, read and write
   backpressure, remote RESET, retry exhaustion, graceful close, consumer
   drop, service cancellation, family replacement/removal, and terminal zero
   ownership.
7. Pinned libtorrent 2.0.13 transfers the exact controlled payload with
   RSTorrent as uTP leecher and as uTP seed. Both cases disable incoming and
   outgoing TCP and MSE, observe exactly one loopback peer, verify the complete
   payload SHA-1, record uTP packet counters and RSTorrent resource high-water
   marks, and remove all temporary state.
8. The focused tests, full Rust baseline, controlled Python interoperability,
   tactical evidence, campaign checkpoint, readiness queue, and unchanged
   **Unsupported** protocol row are reconciled before the final commit.

## Scope And Contracts

### Session UDP classification

`SessionUdpService` remains the only `recv_from` owner. Its fixed receive
buffer grows only to the larger of the DHT malformed sentinel and the Stage 3
uTP Ethernet-profile sentinel. Classification is intentionally shallow and
allocation-free: at least the 20-byte uTP header, version nibble `1`, and a
known packet-type nibble identify uTP-shaped traffic. Full hostile parsing and
state validity remain the uTP runtime's responsibility. Everything else
continues to the existing DHT route; this tactical does not add the currently
absent UDP-tracker receive consumer.

The two ingress queues are independent:

| Owner | Descriptor bound | Per-datagram byte bound | Full behavior |
| --- | ---: | ---: | --- |
| DHT | 64 | 1,025 | Drop the new DHT item and increment only DHT/drop totals. |
| uTP service | 256 | 1,473 | Drop the new uTP item and increment only uTP/drop totals. |
| One uTP connection | 64 | 1,473 | Drop the new routed item; transport loss recovery remains authoritative. |

Every routed uTP datagram carries address family and the exact session socket
generation that received it. The send handle requires the same generation and
fails stale before selecting a socket. Aggregate counters distinguish received
and classified datagrams/bytes, per-route queue high-water, per-route drops,
malformed uTP input, unknown-connection input, and stale-generation input.
Counters saturate and ordinary diagnostics never retain payload bytes.

### Connection admission and identity

The service owns at most 64 live uTP connection workers across all families,
of which at most 16 may be incoming transport handshakes that have not yet
produced an accepted ordered stream. The incoming ordered-stream route holds at
most 16 descriptors. An outgoing connection ID gets at most 16 entropy draws
to avoid a live `(family, generation, remote, receive_id)` collision, then
fails typed and atomically. Initial sequence numbers are independently drawn.

Only a fully decoded `ST_SYN` may create incoming state. It must have no
payload or SACK as already required by Tactical `119`; admission reserves all
connection, per-connection queue, and incoming-stream descriptor authority
before publishing the worker. A duplicate SYN reaches the existing worker.
Unknown non-SYN packets do not create state. Unknown RESET is ignored; another
unknown valid packet is counted and dropped rather than reflecting traffic to
a potentially spoofed source in this first runtime slice.

Stage 3 executes IPv4 uTP only. Classification remains family-correct on the
dual-stack session waist, but IPv6 SYN admission and outgoing IPv6 creation are
typed unsupported. The deterministic core's Stage 2 IPv4 bounds are used with
an initial and maximum runtime UDP payload of 548 bytes. Keeping floor equal to
ceiling disables active real-socket path-MTU probes until a later controlled
path slice can define portable per-datagram do-not-fragment execution without
adding a dependency or pretending that an ignored flag was applied. This is a
throughput tradeoff, not a wire-format difference.

### Runtime worker and ordered stream

Each connection worker is the only mutable owner of one `TransportState`.
It selects among its bounded datagram queue, one staged application write,
actual application-consumption notification, transport deadlines, socket-
generation changes, local shutdown, and service cancellation. It drains every
currently eligible emission in bounded iterations, awaits the UDP send, and
reports `Sent`, `WouldBlock`, or `MessageTooLarge` back to deterministic state.
No background loop calls wall time from protocol code.

One application write descriptor contains at most 16 KiB and its channel has
capacity one. It is admitted to `TransportState` only when the exact 1-MiB
unsent allowance permits it. Delivered uTP payload moves, rather than copies,
into a runtime-owned ordered queue still charged to the deterministic receive
window. `UtpStream::poll_read` reports exactly the bytes copied to the caller;
only then does the worker call `consume_received`. The runtime may hold at most
1 MiB of delivered/reordered receive payload, 16 KiB of staged write data,
and the already-declared 1 MiB unsent plus 1 MiB sent-ledger data per
connection. Descriptor and byte high-water values are observable.

Local async shutdown requests FIN after admitted writes drain. Remote FIN
becomes EOF only after every preceding byte is read. Bidirectional FIN and its
final ACK reach normal terminal cleanup; RESET, retry exhaustion, malformed
state that violates a connection invariant, dropped consumer, socket
replacement, and service cancellation abort and release all deterministic and
runtime ownership. `Drop` cancels as a fallback but is not successful joined
shutdown.

### Peer-stream boundary

A small engine-owned `PeerStream` enum contains `TcpStream` or `UtpStream`,
implements the async read/write operations used by peer handshakes and framed
I/O, and exposes local/remote endpoints and `PeerTransport`. This is the first
concrete two-transport boundary and replaces TCP-specific types only where the
same peer invariant genuinely applies.

Outgoing product dialing stays TCP. A controlled diagnostic injection supplies
an already-connected `UtpStream` to the existing outgoing BitTorrent
handshake/peer owner. Incoming uTP streams enter the existing bounded pending-
handshake and peer-budget path beside accepted TCP streams. They reuse torrent
registration, duplicate peer-ID resolution, upload scheduling, content reads,
peer observations, and terminal task joining. The uTP controlled path rejects
MSE-required policy and does not silently downgrade or retry over TCP.

## Owner, Task, Cancellation, And Data-Flow Map

```text
Session network generation
  -> SessionUdpService (stable service, per-family replaceable sockets)
       -> one recv task per active family
       -> DHT queue (64 descriptors, existing consumer)
       -> uTP ingress queue (256 descriptors)
       -> generation watch + generation-checked send handle
  -> UtpService (one supervisor task, maximum 64 workers)
       -> connection lookup/admission and incoming stream route
       -> one worker per live connection
            -> TransportState (runtime independent)
            -> per-connection datagram queue (64)
            -> staged application write (one, <= 16 KiB)
            -> ordered delivered-byte queue (charged to receive window)
            -> cancellation and joined terminal report
  -> existing DhtService (unchanged transport state and actor)
  -> existing peer runtime
       -> PeerStream::Tcp or PeerStream::Utp
       -> common BitTorrent handshake/frame/content owners
```

Dependency direction remains protocol values/state -> uTP runtime -> shared
session networking and peer adapters -> diagnostic composition. Protocol code
does not depend on Tokio, sockets, endpoint maps, tasks, or peer-wire state.
The UDP owner does not parse BitTorrent handshakes or own torrent policy.

## Shape-Changing Failure Cases

- A datagram that merely resembles uTP but has malformed extensions must be
  isolated from DHT and rejected without allocation beyond the route bound.
- Connection IDs are not globally unique. Lookup includes remote endpoint,
  family, and generation, and the map permits the same ID for different peers.
- Duplicate/reordered SYN, STATE, DATA, FIN, and RESET cannot create a second
  worker or strand a pending stream descriptor.
- Queue saturation drops new datagrams rather than blocking the sole socket
  receiver. It cannot stall DHT, the socket-generation watch, or shutdown.
- An application that does not read must drive advertised credit to zero and
  keep total delivered/reordered bytes at or below 1 MiB. An application that
  does not write-ready cannot create an unbounded command queue.
- An application close racing incoming data preserves data-before-EOF order.
  A dropped application stream cannot leave a worker waiting forever for FIN.
- A family replacement fences send before the old socket is shut down and
  cancels all old workers even if none has an active timer or datagram.
- One worker panic or terminal protocol error is observed, removes exactly its
  lookup entry, releases all permits, and does not cancel DHT or sibling uTP
  connections. Service shutdown joins every worker.
- Libtorrent may ACK the SYN with DATA, choose arbitrary responder sequence
  numbers, coalesce ACKs, advertise a zero window, or use FIN behavior that is
  valid but differs from RSTorrent's deterministic fixtures. These reach the
  existing state machine rather than runtime special cases.

## Source-First Record

### Normative source

Re-read managed BEP 29 at BitTorrent BEP commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`, especially the v1 header and
type rules, connection-ID reversal, SYN/STATE setup, packet rather than byte
sequence identity, advertised receive credit, DATA/FIN ordering, RESET,
retransmission, and packet-size sections. Tactical `121` already owns the RFC
6817 congestion choice; this slice does not change it.

### Pinned libtorrent completeness and executable oracle

Inspected Rasterbar libtorrent commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/aux_/utp_socket_manager.hpp` and
  `src/utp_socket_manager.cpp`: `incoming_packet`, endpoint-plus-receive-ID
  lookup, last-socket fast path, SYN-only admission, connection-limit flood
  guard, `new_utp_socket`, deferred ACK drain, writable/drained subscribers,
  per-listen-socket abort, tick deletion, and MTU selection;
- `include/libtorrent/aux_/utp_stream.hpp` and `src/utp_stream.cpp`:
  `async_connect`, `async_read_some`, `async_write_some`, `issue_read`,
  `issue_write`, `socket_drained`, `maybe_trigger_receive_callback`,
  `maybe_trigger_send_callback`, `abort`, `close`, `should_delete`, EOF after
  ordered FIN, RESET handling, SYN-with-DATA compatibility, and the
  `fin_sent`/error-wait/deleting transitions;
- `src/session_impl.cpp`, `session_impl::on_udp_packet`: uTP gets first
  classification opportunity, DHT-shaped bencode gets the next, socket drain
  flushes deferred uTP work, and receive errors are classified before the
  receive is reissued or terminated;
- `test/test_utp.cpp`, case `utp`: both TCP directions, DHT, LSD, UPnP,
  NAT-PMP, and MSE are disabled while one exact local torrent transfer runs
  over uTP and both sessions abort through owned proxies; and
- `simulation/test_utp.cpp`: the PMTU, ordinary, bufferbloat, constrained-
  path, and small-kernel-send-buffer cases remain read-only edge inventory.
  Its GPL-3.0 simulator submodule is not initialized, linked, run, copied, or
  distributed.

Adopted behavior is endpoint-plus-ID lookup, SYN-only admission, ordered async
stream semantics, deferred/bounded work, explicit close/error state, one
shared session UDP receive owner, and forced-TCP-disabled interoperability.
Intentional differences are RSTorrent's separate finite DHT/uTP queues,
generation in connection identity, one supervised worker per connection,
exact application-consumption credit, conservative fixed 548-byte Stage 3
runtime datagrams, and no product connection policy.

### JSTorrent product history

The tracked JSTorrent sibling is at
`9895410beeed6aff554053769bd006a3fbd373ef`. Its active engine has no uTP
implementation or product policy. Its controlled libtorrent helpers in
`packages/engine/integration/python/libtorrent_utils.py`,
`seed_for_test.py`, and `verify_lt_download.py` explicitly disable both uTP
directions. That is useful negative history: this slice creates new protocol
evidence and does not preserve a JSTorrent behavior. The sibling contains
unrelated untracked documentation/attachment directories, so only tracked
read-only source at the recorded commit was inspected.

No source, fixture, constant table, or test vector is copied. Libtorrent is a
separate BSD-3-Clause source/executable oracle; all RSTorrent code and fixtures
remain independently authored from public behavior. No manifest, dependency,
unsafe code, or third-party notice change is planned.

## Staged Implementation And Validation

1. Add deterministic FIN/wakeup/runtime-intent accessors required by an async
   owner, with pure close, EOF, timeout, and atomic-error tests.
2. Split the session UDP waist into independent DHT and uTP routes, add
   generation-checked send/watch behavior, and prove classification,
   saturation, family coexistence, replacement, and terminal task counts.
3. Add the bounded uTP supervisor, worker, ordered stream, metrics, and
   scripted runtime cases before peer integration.
4. Introduce the concrete two-transport peer-stream boundary and feed
   controlled outgoing and incoming uTP streams through existing handshake,
   framing, download, and upload paths. Re-run focused TCP/MSE tests after each
   refactor.
5. Extend the retained Python oracle into two RSTorrent/libtorrent roles. Use
   one deterministic 2 MiB + 731-byte v1 payload, 64-KiB pieces, loopback-only
   endpoints, a 30-second scenario bound per role, TCP/MSE/discovery/NAT
   disabled, exact SHA-1 verification, uTP counters, bounded diagnostics, and
   temporary-directory cleanup.
6. Run and record:

```bash
source ~/.profile
cargo test -p rstorrent-protocol utp::
cargo test -p rstorrent-engine session_udp
cargo test -p rstorrent-engine utp
uv run --project tests/interop --locked \
  python tests/interop/utp_rstorrent_interop.py
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

The controlled Python gate may build diagnostic binaries but may not launch a
visible product client. It records exact commands and removes all generated
payload, profile, capture, and temporary runtime state.

## Result And Evidence

The accepted owner shape held without a dependency, unsafe code, manifest
change, foreign source, or architectural repair. `SessionUdpService` remains
the only socket receiver and now routes DHT and shallow uTP-shaped datagrams
through independent finite queues. `UtpService` owns endpoint/family/socket-
generation/receive-ID lookup, SYN-only admission, one worker per connection,
timers, entropy, stream publication, cancellation, joined shutdown, and the
declared high-water counters. Socket replacement and removal fence and cancel
the old generation while the shared DHT transport remains independently
owned.

`UtpStream` supplies ordered async read/write plus the readiness/try operations
needed by framed peer I/O. Application consumption, rather than delivery into
the runtime queue, restores receive credit. The concrete `PeerStream` enum
boxes only its larger uTP arm and delegates the common byte-stream operations;
protocol framing and MSE-over-TCP behavior remain in their existing owners.
The controlled outgoing diagnostic uses the common plaintext handshake and
framed peer I/O, limits retained payload to 2 MiB + 731 bytes, and verifies
every piece before publication. Incoming uTP enters the existing pending-
handshake and shared peer-budget gates, torrent registration, peer-ID
admission, upload scheduler, published-content reader, observations, and
joined peer cleanup. MSE-required uTP is rejected without TCP fallback.

Ten real-runtime cases now cover ordered duplex graceful close, bounded
readiness/write admission, remote-scoped connection-ID reuse and duplicate
SYN, malformed/unknown packets, RESET, consumer drop, service cancellation,
socket replacement, socket removal, incoming half-open/stream saturation,
retry-terminal classification, worker-panic route cleanup, and terminal zero
ownership. Shared-UDP cases independently cover DHT/uTP classification and
queue isolation, malformed sentinels, generation-checked send, replacement,
dual-family joining, saturation, and saturating counters. The 83 protocol uTP
tests retain the deterministic retry, loss, receive-pressure, FIN, and exact
resource proofs beneath those socket cases.

The retained `utp_rstorrent_interop.py` gate passed both roles against the
locked libtorrent `2.0.13.0` package on IPv4 loopback. Both used one exact
2,097,883-byte payload with 65,536-byte pieces and SHA-1
`cdce24126a8e65854d876c0b83ad3ba19748f6dc`; both observed exactly one
loopback uTP peer and zero TCP peers, with TCP, MSE, DHT, LSD, UPnP, and NAT-
PMP disabled.

- With RSTorrent as leecher, 129 block requests completed in 0.557320 seconds.
  Libtorrent recorded 920 uTP packets in and 1,805 out; RSTorrent recorded
  1,805 classified packets in and 920 packets out. Its session-uTP and per-
  connection queue high-waters were both 12 descriptors; delivered ownership
  reached 15,974 bytes and unsent/sent ownership each reached 68 bytes.
- With RSTorrent as seed, completion took 0.805350 seconds. Libtorrent recorded
  4,148 uTP packets in and 2,248 out; RSTorrent recorded 2,249 classified
  packets in and 4,148 packets out. Its session-uTP and per-connection queue
  high-waters were both 25 descriptors; delivered, unsent, and sent ownership
  reached 288, 904,687, and 19,762 bytes. Existing upload ownership reached
  one peer/slot/read, 17 queued requests, 278,528 queued bytes, and exactly
  2,097,883 payload bytes.

The leecher retained smoothed RTT 25..1,489 microseconds, effective RTO
500,000..1,000,000 microseconds, raw peer-timestamp base delay
3,111,349,205..3,111,349,245 microseconds, queue delay 0..26 microseconds,
congestion window 1,056 bytes, advertised receive window
1,032,602..1,048,576 bytes, and selected MTU 548 bytes. The seed retained
smoothed RTT 50..427 microseconds, the same effective-RTO range, raw base delay
1,513,249,295..1,513,249,373 microseconds, queue delay 0..99 microseconds,
congestion window 1,056..19,833 bytes, advertised receive window
1,048,288..1,048,576 bytes, and selected MTU 548 bytes. These are bounded
aggregate observations; no payload or packet-level logging was added.

Both roles recorded zero malformed, stale-generation, dropped, or unknown-
connection uTP datagrams, zero worker panics, zero libtorrent loss/timeout/
resend counters, and terminal zero session-UDP tasks, uTP connections, half-
opens, incoming peers, registrations, and queued datagrams. The two-role gate
finished in 2.190952 seconds excluding its bounded build and removed its
temporary directory.

Final validation passed exactly:

```text
cargo test -p rstorrent-protocol utp::                    # 83 passed
cargo test -p rstorrent-engine session_udp                # 12 passed
cargo test -p rstorrent-engine utp                        # passed
cargo test -p rstorrent-engine utp_runtime::tests --lib   # 10 passed
uv run --project tests/interop --locked \
  python tests/interop/utp_rstorrent_interop.py            # both roles passed
cargo fmt --all -- --check                                # passed
cargo clippy --workspace -- -D warnings                   # passed
cargo test --workspace                                    # passed
```

No WAN, LAN peer, external device, visible client, public swarm, IPv6 uTP,
runtime path-MTU probe, UDP mapping, MSE-over-uTP, product selection, product
listener, advertisement, or support-claim work ran. The interoperability
result exposed no structural runtime gap requiring a repair slice.

## Non-Goals And Next Boundary

- No product TCP/uTP selection, preference, racing, fallback, setting,
  capability announcement, tracker/PEX flag, listener advertisement, status
  surface, or default incoming policy.
- No WAN, LAN peer, public swarm, `pimom`, physical device, emulator, browser,
  desktop client, Android client, or iOS work.
- No UDP port mapping, IPv6 pinhole, NAT-PMP, PCP, BEP 55 hole punching, LSD,
  multi-interface binding, or public incoming-reachability claim.
- No IPv6 uTP execution, runtime path-MTU probing, active zero-window probe,
  Nagle policy, bandwidth setting, controller change, or performance claim.
- No MSE over uTP. Existing plaintext/RC4 MSE over TCP remains unchanged.
- No foreign source, fixture, dependency, FFI, vendoring, mechanical
  translation, manifest change, notice change, unsafe code, or uTP support-
  claim promotion.

The next human checkpoint follows the two-role loopback result. It chooses
between controlled WAN evidence, a bounded runtime repair if interoperability
reveals a structural gap, or pausing before product policy. This tactical does
not pre-authorize that choice.

## Escalation

Ordinary module layout, peer-stream refactoring, diagnostic composition,
entropy plumbing, typed error mapping, conservative limit tightening,
adversarial cases implied above, interoperability repairs inside the accepted
wire/runtime boundary, documentation reconciliation, and coherent commits
proceed autonomously.

Stop for human direction before adding a dependency or unsafe/platform socket
path, weakening a declared resource limit, changing the RFC 6817 controller,
expanding to IPv6 or real path-MTU execution, enabling product uTP policy,
composing MSE, using any non-loopback network/device, copying foreign source,
or changing the protocol-support claim.
