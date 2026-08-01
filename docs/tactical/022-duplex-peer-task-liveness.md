# Tactical 022: Duplex Peer Task Liveness

Status: Completed; strict endgame ownership continues in Tactical `023`.

Topics: `peer-lifecycle`, `performance-and-live-evidence`,
`oracle-driven-engine-campaign`

## Motivation And Outcome

Tactical `021` exhausted tracker and peer admission breadth, then exposed a
deterministic task/channel deadlock. The content supervisor marks and sends a
large adaptive request window through a bounded 16-command channel. A peer
task can simultaneously block while delivering decoded payload through the
bounded shared 64-event channel. The supervisor cannot consume events until
all sends finish, and the peer task cannot drain commands until an event is
accepted.

Break that cycle without dropping peer messages, weakening payload ownership,
making either channel unbounded, splitting request state across owners, or
launching visible clients. The outcome is a peer task that continues draining
bounded outbound commands while decoded inbound messages wait for event
capacity, followed by controlled and public evidence that the content
supervisor keeps cycling under a 500-request peer window.

## Source Dossier

Pinned libtorrent `2.0.13` at `7d7fc38fac61177fa5e02148f791b2f65250b09d`
is the completeness oracle. No source or fixture is copied.

- `peer_connection.cpp::incoming_piece` grows slow start and updates the
  desired queue after each accepted block.
- `peer_connection.cpp::send_block_requests` and `fill_send_buffer` retain
  request and serialized output queues inside the connection owner; inbound
  dispatch does not require another owner to finish enqueuing the entire
  request window first.
- `peer_connection.cpp::second_tick`, `update_desired_queue_size`, and
  `snub_peer` keep rate, queue target, timeout, and liveness policy separate
  from the transport's ability to make duplex progress.
- `peer_info` exposes queue length, target, current payload rate, queue time,
  request timeout, and snubbed state. Tactical `021`'s endpoint-free table
  mirrored these facts and made the frozen supervisor observable.

RSTorrent does not need libtorrent's socket or buffer architecture. The
reference invariant is narrower: bounded inbound backpressure must not prevent
the same connection owner from draining already-authorized outbound protocol
messages.

## Ownership And Design

`PeerSocketTask` remains the sole owner of one socket generation. It keeps
decoded messages in its existing bounded local queue. When at least one
message awaits the shared event channel, the task selects among cancellation,
an event-channel permit, and another outbound command. It pauses further
socket reads until the local queue drains, but it must continue accepting and
writing commands whenever event capacity is unavailable.

Reserve event capacity before removing the oldest local message. This retains
wire order and prevents loss if the receiver closes. A closed command channel
still terminates the socket generation; cancellation remains biased and joins
the task. The local pending-message bound is the messages decoded from one
16 KiB network read plus the already-bounded pre-content queue. No detached
writer, unbounded queue, or second socket owner is introduced.

The diagnostic snapshot records when its one-hertz content-peer table was
captured. A terminal report can then distinguish a current slow peer from a
supervisor that stopped observing state.

## Invariants And Bounds

- The command queue remains 16, the shared event queue remains 64, and one
  socket read remains 16 KiB.
- Every decoded message is delivered in wire order exactly once or the task
  terminates with a typed receiver-closure error.
- Event backpressure pauses further reads but never prevents draining an
  already-bounded command queue.
- Requests remain authorized and payload-reserved by `SwarmState` before a
  transport command exists; this tactical does not alter request selection.
- Cancellation, receiver closure, command closure, socket failure, and normal
  shutdown terminate and join exactly one task generation.
- Detailed peer diagnostics remain endpoint-free, contain at most 30 rows,
  and are computed at most once per second or once at completion.

## Adversarial Validation

- Fill a one-entry event channel with decoded inbound messages, leave another
  message pending inside the task, and send more than 16 outbound commands.
  Every command must reach the socket before the event receiver is drained;
  the old implementation must time out.
- Preserve exact inbound message order once event capacity resumes.
- Saturated event delivery must still cancel and join promptly.
- Fragmented input must not refresh the independent complete-message deadline.
- Existing adaptive-window, stall, reassignment, slow-storage, mixed-peer,
  tracker, and paired publication gates remain green.

## Live Gate

After deterministic and controlled gates, run three clean owner-only common
Big Buck Bunny attempts to 50% with the per-peer table and capture timestamp.
Require at least 2/3 success before an alternating paired screen. A failure
must show a current table rather than a multi-minute observation gap and must
select a new source-derived owner before policy changes.

## Implementation Evidence

`PeerSocketTask` now retains decoded messages in a bounded local queue and,
while event delivery is backpressured, selects between reserving event
capacity and draining the existing command channel. It reserves a permit
before removing the oldest message, preserving exact wire order and avoiding
loss if the supervisor closes. Reads remain paused until the local queue
drains, so this does not add an unbounded buffer or a second socket owner.

The deterministic regression fills a one-entry event channel with three
decoded keepalives, then sends 17 interested messages through the 16-entry
command channel. All 17 frames reach the socket before the event receiver is
drained, and the two pending inbound messages are subsequently delivered in
order. The former implementation blocks on the seventeenth command in this
construction. The existing event-saturation cancellation and fragmented
complete-message deadline tests also pass.

The diagnostic snapshot and public probe now expose the monotonic capture
time of the throttled per-peer table. The controlled mixed-peer liveness gate
passed, and the paired 79,000-byte complete gate classified `both_reached`:
RSTorrent verified and published all three pieces in 49 ms with exact
integrity and cleanup; libtorrent did so in 91 ms. This is a harness guard,
not a public performance claim. Formatting and warning-denying workspace
clippy pass; the workspace test gate passes 226 tests with three explicitly
ignored public-network tests.

The clean owner-only public gate reached 50% in 3/3 Big Buck Bunny runs at
34.70, 36.39, and 55.37 seconds. Every run verified 528 pieces, cleaned up
exactly, and retained a current peer table captured less than one second from
the terminal milestone. Terminal sampled payload rates were 2.97--3.83 MiB/s;
the former multi-minute supervisor freeze did not recur.

The alternating three-pair screen classified `both_reached` in 3/3 runs.
RSTorrent reached 50% in 30.74, 34.14, and 45.82 seconds versus libtorrent in
24.00, 25.80, and 24.82 seconds, for paired ratios of 1.28x, 1.32x, and 1.85x.
Both owners cleaned up exactly. RSTorrent's 34.14-second median is within the
campaign's provisional 2x screen threshold, but this three-pair screen is not
the two-cohort comparable confirmation.

## Non-Goals

- changing request-window growth, timeouts, piece selection, endgame, cancel,
  integrity attribution, tracker policy, DHT, or connection limits
- adding writer/reader task pairs, unbounded channels, session-wide budgets,
  upload/seeding, incoming sockets, or NAT traversal
- UI, Tauri, browser, Android, or physical-device work

## Validation And Stopping Condition

The stopping condition is met. Duplex progress, wire order, bounds, exact
cleanup, workspace gates, controlled interoperability, the 3/3 owner-only
gate, and the 3/3 paired screen pass. Tactical `023` now owns strict endgame
duplicates and verified publication; no human decision is required.
