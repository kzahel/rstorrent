# Tactical 132: uTP Default-Readiness Evidence

Status: **Active**. Tactical `131`'s product-enablement review selected this
bounded recommendation A on 2026-08-11. Shipped/default clients remain
TCP-only throughout this tactical.

Topics: `utp-transport-campaign`, `peer-lifecycle`, `protocol-support`,
`public-torrent-testing`, `performance-and-live-evidence`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tactical
[`131`](131-bounded-product-utp-composition.md) supplies default-off application
composition, exact incoming/outgoing product uTP, and one sequential TCP
fallback. Closed Tactical
[`130`](130-utp-transport-solidification.md) supplies fixed-runtime impairment,
hostile lifecycle, diagnostic-MTU, and mapped-WAN evidence. Ready Tactical
[`129`](129-bounded-storage-intake-watermark.md) remains intact and queued.

## Decision And Desired Outcome

Determine whether the default-off `PreferUtp` application path is technically
ready for a later product-default decision without making that decision here.
Remove the known repeated five-second tax against TCP-only endpoints, prove
mixed endpoint behavior under one bounded owner, and take one dated ordinary-
swarm observation through an explicit headless uTP profile.

The selected policy is endpoint-scoped, runtime-independent, and conservative:

- a newly observed eligible IPv4 endpoint is uTP-unknown and receives one uTP
  transport attempt before sequential TCP fallback;
- successful uTP transport establishment confirms uTP before the BitTorrent
  handshake, so a later protocol failure cannot erase valid transport
  capability evidence;
- uTP transport-connect failure suppresses uTP for that endpoint while TCP
  remains immediately eligible; direct TCP selection, cancellation before a
  transport result, TCP failure, and BitTorrent handshake failure do not alter
  uTP capability;
- suppression starts at five minutes, doubles after each failed re-probe, and
  caps at one hour. The failure counter saturates rather than wrapping. Expiry
  permits one uTP re-probe, allowing recovery from transient UDP loss;
- a valid BEP 11 added-peer uTP flag clears suppression to advertised support,
  matching the pinned oracle's explicit capability refresh. A successful uTP
  connection upgrades advertised support to confirmed support;
- state lives only in the existing bounded per-torrent `PeerRecord`. It is not
  persisted, shared across torrents, keyed by IP alone, or allowed to create a
  second cache, timer task, retry queue, or endpoint-retention rule; and
- the dial attempt copies the deterministic decision made at `begin_dial`.
  Later callbacks update only the same active record and attempt, preserving
  generation fencing and making stale completions harmless.

Five minutes is intentionally longer than the ordinary 60-second peer
reconnect backoff and reduces repeated five-second probes by at least fivefold
for a continuously failing endpoint. The one-hour cap still permits recovery
during a long-running torrent. These are readiness bounds, not a user setting
or a claim that all networks share one ideal cadence.

## Stopping Condition

Stop at the next product-default human review only after:

1. pure peer-registry tests cover unknown, advertised, confirmed, suppressed,
   expired, repeated-failure, saturation, PEX refresh, stale callback, record
   eviction, source removal, and time-overflow behavior without runtime types;
2. the one logical dial owner records uTP transport success or failure exactly
   once, never mistakes TCP, cancellation, or peer-wire failure for uTP
   evidence, and selects direct TCP while suppression is live;
3. a real-socket mixed cohort contains at least one uTP-capable and one TCP-
   only IPv4 endpoint, proves the first fallback, proves a repeated attempt
   avoids the uTP timeout, proves expiry/re-advertisement recovery, retains one
   permit and generation per logical dial, and joins every worker;
4. the retained exact application-backed pinned-libtorrent incoming, outgoing,
   fallback, and default-TCP/Fast/MSE cases still pass with exact content and
   terminal zero ownership;
5. one explicit headless `product-utp` public profile starts fixed uTP on the
   public probe's existing shared session UDP owner, attempts the catalogued
   Big Buck Bunny magnet to verified metadata only, and reports endpoint-free
   TCP/uTP connection and capability aggregates plus bounded UDP/uTP resource
   counters;
6. the public attempt is limited to one fresh temporary root, 180 seconds,
   eight peer hints if any, 30 pending/connected peers, 64 MiB buffered
   payload, 512 MiB wire payload, no incoming advertisement or mapping, and a
   ten-second cleanup grace. Success or honest evidence-limited failure is a
   dated observation rather than a deterministic gate;
7. ordinary invocations and every shipped client remain TCP-only, no persisted
   setting or generated contract changes, fixed runtime stays at 548 bytes,
   and BEP 29 remains **Unsupported**; and
8. focused tests, the controlled harnesses, formatting, workspace clippy, and
   the complete workspace baseline pass, with all temporary public and
   controlled artifacts removed and owning topics reconciled.

## Scope And Human Stops

This tactical authorizes:

- the bounded endpoint uTP capability state and deterministic selection/update
  functions in the existing peer registry;
- carrying one immutable uTP decision on the existing `DialAttempt` and one
  bounded subattempt outcome through the existing socket-set event owner;
- consuming BEP 11's existing parsed uTP flag and advertising that flag for an
  actually established uTP peer;
- endpoint-free capability aggregates in diagnostic/test output without a
  generated product-contract field;
- deterministic, real-socket, and pinned-libtorrent loopback evidence; and
- exactly one opt-in, headless, metadata-only ordinary-swarm attempt under the
  live-run limits above. This is the explicit public-network authority selected
  at Tactical `131`'s review.

Stop before:

- changing `ApplicationConfig`, desktop, Android, gateway, CLI, or fresh-
  profile defaults to `PreferUtp`;
- adding a persisted setting, UI, generated application contract, per-torrent
  user policy, migration, remote control, or presentation;
- UDP UPnP mapping, endpoint advertisement, tracker/DHT announce-port changes,
  `implied_port`, hole punching, or any permanent network change;
- persisting or sharing capability state across torrents or application
  restarts;
- TCP/uTP racing, IPv6 uTP, MSE-over-uTP, dynamic product MTU, proxy behavior,
  or a mixed-mode bandwidth algorithm;
- using `pimom`, another external device, a visible product client, emulator,
  physical device, or packet capture; or
- changing the BEP 29 support claim.

Evidence may recommend any of those at the final review, but it cannot silently
authorize them. A public timeout, no uTP peer in the changing swarm, or a
reference architecture difference is recorded honestly and does not block
controlled closure.

## Invariants And Resource Bounds

- Capability state consumes constant space inside each of at most 1,000
  existing peer records. It adds no independently retained endpoint and cannot
  prevent the record's existing eviction or source-removal rules.
- Suppression uses monotonic per-torrent elapsed time. All multiplication and
  addition are checked or saturating; delay is bounded to five minutes through
  one hour and the failure counter cannot wrap.
- Only actual outgoing uTP transport connect results mutate the inferred
  state. An explicit, valid PEX uTP flag may refresh it; no tracker, DHT,
  incoming ephemeral source port, peer ID, TCP success, or payload result is
  treated as uTP support.
- One outgoing logical attempt still owns one peer record, generation,
  connection ID, peer-budget permit, cancellation token, final failure, and at
  most one live transport subattempt. Fallback begins after uTP joins to zero.
- The capability outcome is settled before the logical dial success/failure is
  settled. Stale or duplicate outcomes cannot modify a new attempt.
- Existing uTP limits remain 64 connections, 16 incoming half-opens, 64
  datagrams per connection, a 256-datagram shared route, 1 MiB receive and
  unsent-byte bounds, 1,024 sent packets, a 1 MiB sent ledger, and fixed
  548-byte IPv4 datagrams.
- Diagnostic aggregates reveal counts and state classes, never endpoint
  addresses, raw peer IDs, payload, filesystem roots, or packet logs.

## Source-First Record

Re-read managed BEP 29 at BitTorrent BEP commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`. It defines uTP transport setup
and congestion behavior but does not define endpoint memory, TCP fallback,
retry cadence, product defaults, or support claims. Re-read BEP 11's added-peer
flag bit `0x04`, which is an explicit uTP capability advertisement but does not
define its lifetime or override policy.

Re-inspected Rasterbar libtorrent commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `src/torrent_peer.cpp::torrent_peer` initializes `supports_utp=true` and
  `confirmed_supports_utp=false` for a fresh endpoint;
- `src/torrent.cpp::torrent::connect_to_peer` selects uTP when it is enabled
  and the peer is assumed or confirmed to support it;
- `src/peer_connection.cpp::peer_connection::connect_failed` clears assumed
  support after an outgoing uTP connection failure and schedules an immediate
  TCP reconnect;
- `src/peer_connection.cpp::peer_connection::on_connection_complete` records
  confirmed uTP support at transport establishment, before BitTorrent
  handshake success;
- `src/peer_list.cpp::add_peer` and `update_peer` restore assumed support when
  BEP 11 supplies `pex_utp`; ordinary tracker refresh does not restore that uTP
  bit;
- `src/bt_peer_connection.cpp` forces uTP support for an explicit hole-punch
  connect, which is outside this tactical; and
- `test/test_utp.cpp::test_transfer` plus
  `simulation/test_swarm.cpp::utp_only` prove forced-uTP transfer and cleanup.
  The pinned tests do not directly cover uTP-to-TCP fallback, repeated endpoint
  selection, or PEX restoration, so RSTorrent covers every adopted transition
  independently.

Libtorrent's per-torrent peer bit is retained until the peer record changes and
has no time-based re-probe after failure except an explicit PEX/hole-punch
refresh. RSTorrent intentionally adds bounded five-minute-to-one-hour re-probe
because a permanent negative inference from one UDP timeout is too strong for
changing NAT and firewall conditions. It retains libtorrent's fresh-peer
assumption, transport-level confirmation, immediate sequential TCP fallback,
and PEX refresh without copying its socket variant or callback architecture.

Also inspected pinned rqbit commit
`4e5f94cbcf1d57ec500885c77cf1e24d70232d89`:

- `crates/librqbit/src/listen.rs::ListenerOptions::default` remains TCP-only
  with a TODO to enable both after uTP stabilizes; and
- `crates/librqbit/src/stream_connect.rs::StreamConnector::connect` starts TCP
  first, starts uTP after one second or TCP failure, and races the transports.

That is useful evidence that another Rust engine also gates its default, but
RSTorrent deliberately retains the already accepted sequential uTP-first
single-attempt architecture rather than adopting a race. The local JSTorrent
sibling at `9895410beeed6aff554053769bd006a3fbd373ef` remains TCP-only and
supplies no uTP endpoint-memory policy to preserve.

No reference source, test vector, fixture, or third-party metainfo is copied.

## Owner, Task, Cancellation, And Dependency Map

| Owner | Bounded work | Cancellation and termination |
| --- | --- | --- |
| `PeerRecord` / `PeerRegistry` | deterministic endpoint uTP state, PEX refresh, attempt decision, retry deadline | no task or runtime dependency; record eviction/removal drops all state |
| `DialAttempt` | immutable endpoint capability decision for one generation | copied under the registry lock; stale attempt fencing remains authoritative |
| `PeerSocketSet` pending dial | one actual uTP result plus optional TCP fallback | cancellation joins the current subattempt; outcome is returned once before task removal |
| download/torrent peer owner | apply the subattempt outcome, then settle the logical dial and publish aggregates | same serialized event loop and torrent elapsed clock; no new timer task |
| PEX owner | pass an authenticated-to-the-current-connection but advisory uTP flag into peer observation; advertise actual uTP connections | existing cadence, source, privacy, endpoint, and removal bounds remain authoritative |
| controlled cohort | switchable uTP/TCP endpoints and repeated dials | whole-case timeout; one permit/generation per dial; every service and socket joins |
| public probe | explicit fixed-uTP profile on its existing session UDP owner | 180-second attempt, ten-second cleanup, fresh temporary root, joined uTP/DHT/UDP shutdown |

Dependency direction remains protocol PEX values and runtime-independent peer
state inward; socket outcomes and async services depend on them. No Tokio,
socket, channel, task, or application type enters `peer.rs`.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure transitions | table-driven selection/update tests at exact deadline boundaries, exponential cap and saturation, PEX refresh, stale/duplicate outcomes, overflow, eviction, and source removal |
| Scripted runtime | cancellation before outcome, uTP success then handshake failure, uTP failure then TCP success/failure, direct TCP selection, and same-attempt final accounting |
| Real-socket cohort | mixed uTP/TCP endpoints, first fallback, suppressed direct TCP retry, expiry and PEX recovery, one permit/generation, bounded counters, joined zero ownership |
| Controlled interop | retained exact pinned-libtorrent application incoming/outgoing/fallback suite and default TCP/Fast/MSE regression |
| Opt-in live | one catalogued Big Buck Bunny metadata-only `product-utp` observation with endpoint-free transport/capability and UDP/uTP resource summary |
| Repository | `cargo fmt --all -- --check`, workspace Clippy with warnings denied, and complete workspace tests |

The public run is supporting evidence only. Its success criterion is a safe,
truthful, fully cleaned observation; finding no uTP peer or timing out does not
rewrite deterministic results.

## Execution Order

1. Commit this source-first tactical and make it the sole authoritative
   **Now** without changing behavior.
2. Add pure endpoint capability state, PEX refresh, attempt decision, and
   bounded aggregate tests; commit the runtime-independent slice.
3. Carry exact subattempt outcomes through the socket/driver owner, add the
   mixed real-socket cohort, and commit lifecycle evidence.
4. Re-run and, where useful, extend the retained application-backed
   pinned-libtorrent cohort; commit controlled evidence.
5. Add the explicit public profile and cleanup/reporting harness, validate its
   no-opt-in/default behavior, then run exactly one bounded public observation.
6. Run complete repository gates, reconcile tactical/topics/readiness/support,
   commit the evidence, and stop at the product-default human review.

At that review, evidence may recommend default enablement, another bounded
readiness slice, or closing the uTP campaign. No recommendation changes product
policy without the review.
