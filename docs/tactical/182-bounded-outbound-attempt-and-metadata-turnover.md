# Tactical 182: Bounded Outbound Attempt And Metadata Turnover

Status: **Complete (2026-08-27).** Commits `8cd2582` and `15828ca` implement
the bounded attempt lifetime and saturated metadata turnover. Tactical `176`
has resumed as the sole **Now** with only its unchanged macOS-hosted iOS
simulator/archive compile remaining.

Topics: [`libtorrent-policy-alignment`](../topics/libtorrent-policy-alignment.md),
[`peer-lifecycle`](../topics/peer-lifecycle.md),
[`performance-and-live-evidence`](../topics/performance-and-live-evidence.md),
[`oracle-driven-engine-campaign`](../topics/oracle-driven-engine-campaign.md),
and [`capability-readiness`](../topics/capability-readiness.md).

## Motivation And Desired Outcome

Tactical `181` expanded metadata acquisition from eight to 30 combined pending
dials and connected workers, paced at ten accepted attempts per second. A
remaining slow-startup failure is duration rather than breadth: one RSTorrent
outbound attempt may spend 15 seconds trying preferred uTP, another 15 seconds
connecting TCP, and then a fresh 60-second peer-I/O allowance on the outgoing
BitTorrent handshake. Thirty connected peers that send no accepted metadata
may separately occupy the complete cohort until the common 60-second metadata
progress deadline expires.

Align the observable timing more closely with pinned libtorrent while keeping
RSTorrent's explicit Android-conscious owners:

- make the existing 15-second peer-connect value one absolute outbound-attempt
  budget spanning uTP selection/fallback, TCP connection, MSE negotiation, and
  the BitTorrent handshake;
- try preferred uTP for at most three seconds before using the remaining
  attempt budget for TCP, while preserving exact uTP cancellation and join;
- cap the outgoing BitTorrent handshake at ten seconds and the remaining
  attempt budget, without changing the established peer-I/O timeout; and
- when all 30 metadata slots are occupied and another eligible candidate is
  waiting, replace at most one oldest zero-contribution worker after a
  15-second grace. Sparse swarms and peers that supplied accepted metadata do
  not churn.

At the stopping condition, a black-holed or silent endpoint cannot retain an
outbound connection permit beyond the one attempt budget, and an unproductive
connected metadata cohort cannot hide an eligible replacement until every
worker's 60-second general progress timeout.

## Stable Scenario Subset

1. **AT-001, one lifetime:** transport selection, uTP, TCP, plaintext/MSE, and
   the outgoing BitTorrent handshake share one 15-second absolute deadline.
   Sequential fallbacks do not renew it.
2. **AT-002, bounded uTP preference:** preferred uTP gets at most three
   seconds and then cancels and joins before TCP uses the remaining attempt
   time. A successful uTP connection proceeds without TCP.
3. **AT-003, handshake sub-budget:** an outgoing handshake gets at most ten
   seconds and never extends the attempt deadline. Established peer reads and
   writes retain the existing 60-second peer-I/O timeout.
4. **AT-004, encryption fallback:** MSE negotiation and the one permitted
   fresh-socket plaintext fallback remain within the same attempt lifetime.
   Timeout releases the socket, registry generation, and connection permit;
   already-running non-cancellable DH computation remains under the existing
   four-job session owner and is joined at session shutdown.
5. **MT-001, saturated replacement:** with 30 connected zero-contribution
   metadata workers and candidate 31 eligible, the oldest worker becomes
   replaceable after 15 seconds. Only one replacement is requested at a time,
   and the existing ten-per-second pacer governs the admitted refill.
6. **MT-002, useful and sparse protection:** a peer with an accepted metadata
   block is not selected for saturated turnover. No peer is replaced merely
   because its grace expired when there is cohort capacity or no eligible
   replacement.
7. **MT-003, exact terminal state:** turnover is distinguished from ordinary
   cancellation in diagnostics, gives the replaced endpoint ordinary failure
   backoff, and joins the old worker before its slot is refilled. Completion,
   policy change, and outer cancellation still drain every task, socket,
   registry attempt, and peer-budget permit.

## Source Dossier

Pinned Rasterbar libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` is the behavioral completeness
oracle. No source, fixture, or test data is copied.

- `src/settings_pack.cpp` sets `peer_connect_timeout = 15`,
  `handshake_timeout = 10`, `peer_timeout = 120`,
  `inactivity_timeout = 600`, `connection_speed = 30`,
  `connections_limit = 200`, `utp_connect_timeout = 3000`, and
  `utp_syn_resends = 2`.
- `include/libtorrent/settings_pack.hpp` defines `peer_connect_timeout` from
  initiation of the connection attempt and explains why bounded half-open
  work matters. Its connection-speed setting is attempts per second.
- `src/peer_connection.cpp::second_tick` disconnects a peer that remains in
  the connecting state beyond the connection deadline.
- `src/utp_stream.cpp` establishes the initial uTP deadline from
  `utp_connect_timeout` and uses the configured SYN resend bound.
- `simulation/test_fast_extensions.cpp::handshake_timeout` proves that a
  silent incoming peer is disconnected within the default ten-second
  handshake policy window. Its `peer_idle_timeout` case separately exercises
  the 120-second established-peer value.
- `simulation/test_swarm.cpp::dead_peers` lowers `peer_connect_timeout` to one
  second and proves all three dead connection attempts terminate.
- `src/ut_metadata.cpp` permits two outstanding metadata requests per peer,
  makes a requested block available elsewhere after three seconds, and owns
  bounded rejection and bad-hash cooldowns. It has no metadata-only
  30-connection sub-limit beneath ordinary torrent/session admission.

RSTorrent's current `peer_socket.rs::connect_with_progress` applies the same
`NetworkConfig::peer_connect_timeout` independently to preferred uTP and TCP,
then applies `peer_io_timeout` to outgoing negotiation. Plaintext handshake
write and read each receive a fresh deadline. MSE may reconnect a fresh TCP
socket for its one allowed plaintext fallback. `UtpHandle::connect_with_timeout`
already cancels and joins a timed-out uTP worker, so the new policy must keep
that explicit termination path rather than drop an outer future.

The metadata supervisor in `driver.rs::acquire_metadata_inner` owns one
`PeerSocketSet`, one joined worker set, the pure `TorrentMetadataDownload`, and
the Tactical `181` cohort/pacer. Each worker currently remains until metadata
completion, connection failure, cancellation, or the 60-second metadata
progress deadline. The pure metadata owner already records the source peer for
each accepted block and can expose that fact without gaining a clock or task.

JSTorrent's TypeScript connection manager currently applies a ten-second
internal connection timeout, while the older peer connection path records a
six-second product-history value. Neither supplies the combined lifetime or
turnover ownership required here; they support avoiding the current
approximately 90-second sequential worst case.

## Adopted Behavior And Intentional Differences

Adopt libtorrent's 15-second connecting lifetime and ten-second handshake
sub-budget as observable defaults. Use libtorrent's three-second uTP value as
RSTorrent's preferred-transport fallback point, not as a claim that the two
uTP state machines or retransmission schedules are identical. RSTorrent keeps
one sequential uTP-then-TCP attempt instead of adding a parallel connection
hedge.

Pinned libtorrent has no direct equivalent of RSTorrent's combined 30-peer
metadata cohort. The 15-second saturated turnover is therefore an explicit
RSTorrent adaptation: it recovers cohort diversity without raising socket
breadth, creating a 30-connection burst, or applying churn to a sparse swarm.
The existing ten-per-second no-burst pacer and 200-connection session owner
remain authoritative.

## Owner, Task, Cancellation, And Dependency Map

```text
NetworkConfig (plain validated durations)
  total attempt = 15 s; uTP fallback = 3 s; handshake = 10 s; peer I/O = 60 s
                              |
                              v
PeerSocketSet dial task -> one absolute attempt deadline
   uTP worker --cancel+join--> TCP/MSE/plain handshake -> established PeerIo
           one registry generation + one PeerBudget permit throughout

MetadataConnectionLimits (plain grace + Tactical 181 bounds)
                              |
                              v
metadata supervisor (only clock/task/cancellation owner)
  pending PeerSocketSet + joined worker controls + no-burst pacer
                              |
                              v
TorrentMetadataDownload (task-free accepted-block contributor facts)
```

- `NetworkConfig` owns values only. `PeerSocketSet` and its dial task retain
  the socket, attempt deadline, registry transition, transport outcome, and
  peer-budget permit.
- One small task-free deadline helper may compute remaining and clamped
  sub-deadlines. It must not depend on sockets, platform types, or a runtime
  task.
- The uTP service retains transport-worker cancellation and join. TCP and
  handshake work use the same absolute attempt deadline; no watchdog task or
  second semaphore is added.
- The metadata supervisor remains the only owner allowed to select and cancel
  a worker. A worker control records connection age, whether turnover was
  requested, and a shared terminal-reason flag solely so its joined result is
  diagnosed accurately.
- `TorrentMetadataDownload` exposes accepted current contributor state by peer
  ID. It remains deterministic and unaware of time, connections, or tasks.

## Bounds And Shape-Changing Edge Cases

- Defaults are 15 seconds total outbound attempt, three seconds preferred-uTP
  fallback, ten seconds outgoing handshake, and the unchanged 60 seconds for
  established peer I/O. All durations must be nonzero; sub-budgets are
  clamped to the remaining total at runtime.
- Metadata retains 30 combined peers, ten attempts per second, and gains a
  15-second saturated no-progress grace. The configurable internal grace is
  nonzero and at most 60 seconds; it is not a public setting.
- Expiry at a phase boundary must not start another transport or plaintext
  fallback. Timeout diagnostics retain the failed phase and the total budget
  remains the outer resource truth.
- A uTP timeout explicitly cancels and joins its worker before TCP starts.
  Dropped futures, late transport success, or cancellation must not leave a
  UDP connection, TCP socket, MSE job, attempt, or permit alive.
- Metadata turnover requires all of: a full combined cohort, an eligible
  waiting candidate, a ready dial pacer, a connected worker at or beyond the
  grace, zero accepted blocks currently attributed to that peer, and no
  already requested turnover. The oldest eligible worker wins
  deterministically.
- Turnover marks and cancels at most one worker, waits for its joined result,
  applies ordinary protocol-failure backoff, and then admits its replacement.
  Pending dials are not turnover candidates.
- Peer-wire chatter, extension negotiation, and rejected/duplicate metadata
  do not count as contribution. An accepted unique metadata block does. This
  protection is intentionally conservative even if a later peer supplies the
  same block.

## Implementation Sequence And Gates

1. Add plain validated timing values and one absolute-deadline helper. Prove
   default values, clamping, expiry, and one-lifetime behavior without a public
   application contract change.
2. Carry the deadline through uTP/TCP/plain/MSE connection and fallback paths.
   Strengthen real-socket scripted tests for uTP fallback, silent handshake,
   cumulative timing, late success, cancellation, and cleanup.
3. Add pure metadata contributor inspection and the internal validated
   saturated grace. Implement deterministic one-at-a-time worker selection in
   the existing supervisor without adding a task or parallel scheduler.
4. Prove saturated 30/31 replacement, contributing-peer protection, sparse
   no-churn behavior, pacing, backoff, diagnostic reason, and terminal zero
   tasks/permits in focused scripted tests.
5. Run focused engine tests, formatting, warning-denying workspace Clippy, and
   workspace tests. Run the existing controlled loopback pinned-libtorrent
   metadata gate if locally available.
6. Build the shared application/engine path for maintained Android arm64-v8a
   and x86_64 targets. Record exact build gates and exposed resource high
   waters. No emulator, physical device, or public-swarm run is authorized.
7. Reconcile this tactical and every owning topic, then restore Tactical `176`
   as the sole **Now** and commit the completed implementation record.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | timing defaults and validation; absolute deadline clamping/expiry; accepted-block contributor query; oldest eligible turnover selection |
| Scripted runtime | black-holed uTP to TCP fallback; silent/late TCP and plain/MSE handshake; cumulative total timeout; cancellation/join; saturated 30/31 replacement; useful/sparse protection; paced refill; terminal zero owners |
| Controlled interop | existing pinned-libtorrent loopback metadata acquisition remains identity/hash exact with bounded cleanup |
| Platform | focused and workspace Rust gates plus maintained Android arm64-v8a and x86_64 native builds |
| Live public | not authorized; the 2026-08-27 observation remains motivation rather than a performance claim |

## Implemented Result

- `NetworkConfig::peer_connect_timeout` is now one absolute deadline across
  preferred uTP, TCP, MSE/plain negotiation, fallback, and the outgoing
  BitTorrent handshake. The defaults are 15 seconds total, three seconds for
  preferred-uTP fallback, ten seconds maximum for the outgoing handshake, and
  the unchanged 60 seconds for established peer I/O. New duration values are
  nonzero-validated by both engine and application-service entry points.
- Preferred-uTP timeout still cancels and joins its transport worker before
  TCP starts. Plain and MSE paths share one clamped handshake deadline, and
  an expired MSE path cannot open the plaintext fallback socket. A timed-out
  attempt releases its socket, registry generation, and single peer-budget
  permit. CPU-only DH work cannot be preempted safely after `spawn_blocking`
  starts; the attempt stops awaiting it, while the existing session owner
  retains its unchanged maximum of four jobs and joins it on shutdown.
- `TorrentMetadataDownload` exposes only deterministic accepted-block counts
  by attempt. The supervisor owns connection age and selects the oldest
  expired zero-contribution worker only when the combined cohort is exactly
  full, another registry candidate is eligible, the no-burst pacer is ready,
  and no prior turnover is pending.
- Turnover cancels exactly one worker, joins it, records
  `metadata peer replaced after saturated no-progress grace`, and applies the
  ordinary protocol-failure backoff before the existing paced refill. A
  contributing worker is protected, and a sparse one-peer swarm remains
  connected beyond the shortened test grace until ordinary cancellation.
- The implementation adds no public setting, task, semaphore, connection
  slot, or retained peer record. Maximum metadata ownership remains 30
  combined dials/workers, the session connection owner remains 200, accepted
  starts remain ten per second, and only one turnover may be outstanding.

## Evidence And Resource Accounting

- Focused pure/runtime tests prove duration defaults and rejection, absolute
  deadline clamping, cumulative uTP/TCP/silent-handshake timing, the ten-second
  handshake sub-budget, expired-MSE no-fallback behavior, contributor queries,
  deterministic oldest-worker selection, saturated 30/31 turnover, useful-peer
  protection, sparse no-churn, and terminal zero pending dials/workers.
- The saturated fixture admits exactly 31 lifetime attempts, replaces exactly
  one of 29 idle zero-contribution workers, protects the one-block contributor,
  receives the remaining two blocks from candidate 31, and ends with zero
  active attempts. The unchanged cohort high water is 30 and only one
  replacement request is live. The sparse fixture retains one worker for 350
  milliseconds with a 100-millisecond grace and no waiting candidate, then
  cancellation joins it to zero.
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, and
  the final `cargo test --workspace` pass. The final feature-unified engine
  suite reports 596 passed and 11 ignored; the session suite reports 260
  passed and two ignored. One earlier parallel workspace run encountered the
  pre-existing exact-port availability assertion; its isolated rerun, full
  session rerun, and final workspace rerun all pass.
- `uv run --project tests/interop --locked python
  tests/interop/magnet_metadata.py` passes against pinned libtorrent
  `2.0.13.0`: RSTorrent verifies the exact 26,686-byte, two-block metadata and
  40,000-byte, three-piece payload in 0.311 seconds, then serves two exact
  metadata requests back in the same loopback run. Cleanup is exact. This is
  controlled correctness timing, not a public-swarm latency result.
- `clients/android/build.sh` passes release native builds for x86_64 and
  arm64-v8a, generated Kotlin bindings, the debug APK, and JVM unit tests.
  Existing Android deprecation warnings are unchanged. No emulator, device,
  or public-network run was performed.

## Non-Goals And Next-Slice Boundary

- a session-global dial pacer, libtorrent's immediate 30-attempt connect boost,
  or changes to fair active-download admission;
- raising the 30-peer metadata or content limits, the 64-worker uTP-service
  cap, 200-connection session default, incoming handshake count, tracker/DHT/
  PEX breadth, peer-record retention, or mobile active-download count;
- changing established peer activity/inactivity policy, adaptive content
  request timeouts, BEP 9 request count/reassignment/geometry/integrity,
  content scheduling, storage, upload, or seeding;
- a public setting, generated application boundary, persistence migration, UI,
  new dependency, parallel transport hedge, or copied libtorrent source/test
  fixture; and
- public-network traffic, AVD, physical Android, or macOS/iOS execution.

The next policy-alignment candidate remains `LPA-003`, a session-global
no-burst dial rate with concurrent-torrent fairness and a conservative Android
profile. It requires separate measurement and does not belong in this timing
and turnover owner.

## Stopping And Escalation

Complete when scenarios `AT-001` through `AT-004` and `MT-001` through
`MT-003`, the validation matrix, exact cleanup, Android builds, implementation
evidence, and topic/queue reconciliation pass. Then restore Tactical `176`'s
unchanged macOS-only gate as the sole **Now**.

No human decision is required for ordinary internal refactoring, names,
stronger edge cases, or conservative tightening within these declared bounds.
Stop for a product-visible setting or API, a new dependency, a changed
persistence/compatibility contract, destructive data action, external/public
execution, a default outside these bounds, or evidence that requires a
different connection owner.
