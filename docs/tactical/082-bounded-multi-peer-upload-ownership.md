# Tactical 082: Bounded Multi-Peer Upload Ownership

Status: Completed on 2026-08-04 after explicit maintainer authorization. This
explicitly directed slice did not change the broader readiness queue or
authorize the separate product-surface work in Tactical
[`083`](083-shared-torrent-file-picker.md).

Topics: `incoming-reachability-and-seeding`, `peer-lifecycle`,
`protocol-support`, `capability-readiness`

## Decision And Motivation

Replace Tactical `078`'s one-established-peer/one-upload-slot proof with one
session-coordinated, bounded upload owner for multiple completed torrents and
peers. The slice coordinates inbound and outbound connection accounting,
applies a real seeding choker, pipelines bounded storage reads and serialized
responses without letting one slow reader block another, and records payload
bytes only when they actually reach the socket.

Pinned libtorrent `2.0.13` is the default-policy oracle. When RSTorrent has the
same semantic concept, this tactical adopts libtorrent's default value and
observable behavior. It does not substitute a locally invented conservative
number merely because the first slice used one. When the architectures or
product security boundaries are not equivalent, the tactical names the
difference and its reason explicitly.

This is behavioral adoption, not a source port. No libtorrent or JSTorrent
source, fixture, class graph, task graph, or persistence format is copied.
RSTorrent keeps task-free policy and protocol state inward of Tokio, keeps
socket and storage execution in `rstorrent-engine`, and keeps durable torrent
eligibility in `rstorrent-session`.

## Default-Adoption Contract

Implementation follows these rules in order:

1. Use the exact pinned libtorrent default when the setting has the same
   meaning and unit in RSTorrent.
2. Reproduce the relevant observable policy when libtorrent's owner shape is
   different; do not reproduce its classes or incidental container behavior.
3. Preserve a stronger already-proven RSTorrent safety bound only when the
   concepts are not equivalent or replacing it would expand this slice. Record
   that choice in the deviation table below.
4. Treat comments and setting documentation as the intended contract when an
   isolated implementation detail conflicts with them. In particular, admit
   exactly 2,000 queued requests rather than preserving libtorrent's apparent
   `size() > limit` off-by-one.
5. Do not tune values from one fast local run. A future change to an adopted
   default requires a pinned-reference change, representative measurement, or
   an explicit product decision recorded in the owning topic.

The later settings tactical may make these values configurable, but its
initial defaults remain the values established here unless new evidence is
recorded.

## Desired Outcome And Stopping Condition

The tactical stops when all of the following are true:

- one session connection budget counts outgoing connecting and established
  TCP peers plus accepted incoming TCP peers across torrent owners;
- the configured normal ceiling is 200 connections, reduced by the same
  file-descriptor safety calculation as pinned libtorrent where the platform
  exposes a process limit, and incoming intake has exactly 10 connections of
  slack above the effective normal ceiling;
- incoming peers are no longer restricted to one established connection per
  session or torrent, while the existing eight pre-handshake tasks remain a
  separate hostile-intake bound and the kernel listen backlog adopts
  libtorrent's default of five;
- all interested peers start choked and one session upload coordinator grants
  no more than eight upload slots across all eligible completed torrents;
- fixed-slot choking, one automatically derived optimistic slot, 15-second
  ordinary evaluation, 30-second optimistic rotation, and the seed
  round-robin 20-piece/one-minute rule match the pinned oracle;
- each unchoked peer can retain at most 2,000 validated 16 KiB request
  descriptors, while actual upload reads and serialized bytes remain under
  independent byte and job bounds;
- a 10-job upload read owner uses the existing 40-handle session storage pool,
  serves peers round-robin, and generation-fences choke, cancel, disconnect,
  registration replacement, and late completion;
- a blocked socket writer cannot block peer-message intake, upload scheduling,
  another peer's reads or writes, registration teardown, or application
  shutdown;
- peer, torrent, and session payload-upload counters and rates count piece
  payload successfully written to the socket, not bytes requested, read, or
  merely placed in a user-space queue;
- metadata payload, peer-wire bytes, and protocol overhead remain separately
  classified, and the existing session Speed catalog truthfully exposes
  `payload_uploaded` instead of also advertising the stale unavailable entry;
- deterministic policy, adversarial runtime, application lifecycle, resource
  high-water, and simultaneous controlled RSTorrent/libtorrent evidence pass;
  and
- the tactical and owning topics record the exact implementation, validation,
  deviations, and remaining settings boundary.

The controlled interoperability fixture must keep at least two RSTorrent and
two libtorrent leechers active against one RSTorrent seed concurrently and
must independently hash their complete results. Slot saturation and rotation
may use a deterministic scripted peer harness so correctness does not depend
on a minute-long wall-clock test.

## Stable Scenarios

### Shared connection admission

The application creates one `SessionPeerBudget` before starting the incoming
listener or an outgoing torrent generation. Outgoing connecting and
established sockets may occupy only the effective normal ceiling. An accepted
incoming socket may occupy the normal ceiling plus the 10-connection slack.
The slack follows libtorrent's meaning: it permits useful incoming candidates
to exist beyond the ordinary target and potentially replace existing
connections; it is not a second outbound pool or ten upload slots.

Every accepted socket acquires its accounting generation before handshake
work and holds it until the owning connection task has joined. An outgoing
dial acquires before creating its connecting socket and transfers the same
generation into established ownership without a release/reacquire gap. A
failed handshake, failed dial, duplicate rejection, cancellation, panic, and
normal close release exactly once.

The listener uses a kernel backlog of five, matching libtorrent's
`listen_queue_size`. This is distinct from the eight admitted handshake tasks:
the kernel backlog covers connections not yet returned by `accept`, while the
task cap covers accepted hostile sockets already owned by RSTorrent.

The configured normal default is 200. On Unix-like targets where the current
soft descriptor limit is available, the effective ceiling is:

```text
min(configured_connections, max(5, (max_open_files - 20) * 8 / 10))
```

Arithmetic is saturating and narrowing is checked. An infinite limit is
capped at 10,000,000 before the formula, matching libtorrent. If the query
fails, use libtorrent's 1,024 fallback; on Windows and another target without a
meaningful process-file limit, use libtorrent's 10,000 fallback. The resulting
effective value and the reason for any reduction are observable.

Libtorrent's per-torrent `max_connections = -1` default is retained for
incoming admission: there is no additional per-torrent connection-policy cap
beneath the session budget. RSTorrent's existing 30-established and
30-pending outbound working-set defaults remain the already measured download
dial/scheduling bounds from Tactical `021`; this upload slice does not
silently raise them to 200. They count against the shared session ceiling and
are reported as an explicit RSTorrent deviation, not as libtorrent defaults.

### Upload slots and seed rotation

Every new peer begins choked. Interest makes it eligible but does not
independently unchoke it. A task-free `UploadScheduler` sees bounded snapshots
of interested, connected peers across all registered complete seeds and
returns choke/unchoke decisions. A small runtime adapter owns the timer and
delivers the latest grant generation to each peer; policy state does not own
sockets, tasks, channels, storage, or an async clock.

The default policy is pinned libtorrent's fixed-slot choker:

- eight session-wide unchoke slots;
- `num_optimistic_unchoke_slots = 0`, meaning automatic 20%, which derives one
  optimistic slot at an eight-slot ceiling;
- ordinary reevaluation every 15 seconds;
- optimistic reevaluation every 30 seconds;
- immediate preemptive unchoke when a slot becomes vacant rather than waiting
  for the next periodic tick;
- seed `round_robin` selection with a quota of 20 times that torrent's piece
  length; and
- an already unchoked peer is quota-complete only after it has sent strictly
  more than that quota and has been unchoked for strictly more than one minute.

The seven ordinary slots retain currently productive peers until quota
completion, then prefer eligible peers that have waited longest. The
optimistic slot prefers the eligible peer that has waited longest since its
last optimistic grant. Equal candidates use a stable connection-generation
tie break so tests and diagnostics are reproducible; libtorrent's partial-sort
container order is not treated as protocol behavior.

All completed torrents have equal priority in this slice. There is no
reserved slot per torrent and no hidden four-slot JSTorrent policy.
Per-torrent `max_uploads` is semantically unlimited, matching libtorrent's
`-1` default, beneath the eight-slot session ceiling. Upload from a torrent
that is still downloading, tit-for-tat ranking, peer classes, and torrent
priority controls are later work because the current registration contract
admits only complete seeds.

### Request, read, and response ownership

An unchoked peer may queue exactly 2,000 validated request descriptors. Each
descriptor is checked against the existing request geometry and readable
verified availability before it changes state. There is no separately
invented 512 KiB logical-request ceiling: 2,000 maximum-size requests
represent at most 32,768,000 requested bytes per peer, but descriptors do not
allocate those payload bytes. Choking or loss of interest removes queued
descriptors and generation-cancels reads that have not produced a wire frame.

One session upload-read owner admits at most 10 blocking read jobs, translating
libtorrent's 10 `aio_threads` workers into RSTorrent's bounded blocking-job
model. Jobs use the existing session-wide
40-handle `StorageFilePool`, matching both libtorrent's `file_pool_size`
default and RSTorrent's existing path/SAF pool limit. The owner visits
eligible peers round-robin and admits at most one new read for a peer before
giving every other eligible peer a turn. No request spawns detached work.

Per-peer read plus serialized-send occupancy uses libtorrent's adaptive
watermark:

```text
target = clamp(successful_piece_payload_bytes_in_last_second * 50 / 100,
               10 KiB,
               500 KiB)
```

Admission checks the current serialized wire bytes plus bytes reserved by
active reads before starting another response. Like libtorrent, one admitted
16 KiB piece frame may cross the target. Read admission reserves its complete
16,397-byte wire frame (four-byte length, one-byte ID, two four-byte indices,
and 16,384-byte payload), so the exact worst case is 511,999 plus 16,397, or
528,396 charged bytes per unchoked peer and 4,227,168 bytes across eight full
upload slots. Initial bitfields and metadata bytes use shared immutable source
buffers and the same bounded writer rather than being copied once per blocked
peer; non-piece frames must fit the hard byte ceiling before enqueue.

Every established peer uses the same 528,396-byte logical writer charge, so
the connection-plus-slack absolute ceiling is 110,963,160 charged bytes at the
default 210 connections. That is a scheduling/backpressure bound, not an
instruction to allocate one buffer of that size per peer: bitfields, piece
geometry, availability, raw metadata, and metadata blocks remain immutable
registration-owned storage referenced by frame ranges. Unique resident piece
payload remains limited by the eight upload grants and the 10 read jobs.

The writer owns one socket write half and a byte-charged bounded queue. The
peer controller continues reading `Cancel`, interest, choke state, keepalives,
and disconnects while the writer is backpressured. Queued payload frames carry
the request and registration generations and are discarded before their first
byte if stale. Once any byte of a frame has been written, the writer must
finish that frame or close the connection; dropping a partial BitTorrent frame
would corrupt the stream. A maximum of 64 queued frame/control descriptors
reuses the established peer-event scale and prevents zero/small-frame floods
from bypassing the byte cap. Choke/unchoke state updates are coalesced to the
latest state.

Libtorrent's `max_queued_disk_bytes = 1 MiB` is not used as an upload-read
limit: its own setting documentation defines it as queued download writes.
The 10 read jobs and adaptive send occupancy above are the relevant upload
bounds.

### Timeouts and slow peers

The established upload connection adopts these pinned libtorrent behaviors:

- 120 seconds without peer activity closes the connection;
- a keepalive is sent after half that interval;
- an interested, unchoked seed peer that provides no request for 60 seconds
  closes only when no prior request is still being read or sent; and
- a mutually uninterested peer may close after 600 seconds when the session is
  near its connection ceiling.

The existing 10-second absolute handshake deadline remains unchanged because
it already matches libtorrent. RSTorrent retains a 60-second no-progress
deadline for one queued socket frame so a peer that continues to send
keepalives but never reads cannot hold a writer generation forever. This
has no exact libtorrent setting analogue and is an explicit joined-lifecycle
safety deviation. The timeout applies to write progress, not total transfer
duration, and a slow but progressing peer remains connected.

Storage delay and socket backpressure are distinct observations. A slow
storage read suppresses the no-request timeout for that peer but cannot occupy
more than one of the 10 read jobs and its charged bytes. A slow socket can
occupy only its slot and per-peer send allowance; it cannot stop other writers
or the accept loop.

### Exact upload accounting

Each encoded outbound frame carries payload ranges. Every successful partial
socket write advances those ranges and records:

- total peer-wire bytes;
- BitTorrent protocol bytes;
- BEP 9 metadata payload bytes; and
- piece payload bytes uploaded.

Piece payload is recorded at peer, torrent, and session scope only for the
payload portion actually written. Request length, completed storage reads,
queued frames, and an attempted `write_all` are not upload evidence. A cancel
or choke can still race with a frame whose first bytes already reached the
socket; those physically written payload bytes remain counted.

`payload_uploaded` means physical BitTorrent piece payload, including a
legitimate duplicate block requested and sent twice. It does not prove the
remote peer retained, hash-verified, or uniquely benefited from the bytes.
Future ratio policy should use this libtorrent-compatible physical-payload
definition unless its own tactical explicitly chooses another numerator.

The per-peer one-second payload sample feeds both the adaptive watermark and
the choker's uploaded-in-last-round/since-unchoke state. Existing session
speed tiers continue to aggregate the same byte metric. The duplicate stale
Speed catalog entry that says upload is unavailable is removed; no schema or
generated-contract change is needed for the already defined metric.

## Normative And Reference Dossier

### Normative behavior

- BEP 3 at `reference/bittorrent.org/beps/bep_0003.rst` defines initially
  choked state, interest/choke messages, request/cancel/piece messages,
  keepalives, the conventional 16 KiB request size, and the 120-second peer
  timeout guidance.
- BEP 9 at `reference/bittorrent.org/beps/bep_0009.rst` continues to govern
  bounded metadata upload on the same connection.
- BEP 10 at `reference/bittorrent.org/beps/bep_0010.rst` continues to govern
  extension framing and connection-local extension IDs.

Choking selection, upload slot counts, connection slack, buffer tuning, disk
worker counts, and per-torrent defaults are implementation policy rather than
BEP-mandated constants. Their baseline comes from the pinned oracle below.

### Pinned libtorrent oracle

The required checkout is `reference/libtorrent` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` (`v2.0.13`). The exact source
surveyed for this draft is:

- `src/settings_pack.cpp` supplies the adopted defaults: 2,000 incoming
  requests, 120/600-second peer/inactivity timeouts, 15/30-second choke
  intervals, 10 KiB/500 KiB/50% send watermarks, fixed-slot and round-robin
  seed chokers, 20-piece quota, automatic optimistic slots, unlimited upload
  rate, eight upload slots, 200 connections, 10 incoming slack, a five-socket
  listen backlog, 40 files, and 10 asynchronous I/O workers. Its adjacent
  30-attempts/second connection pace and 4%/90%/300-second peer-turnover
  defaults, plus the false `allow_multiple_connections_per_ip` default, were
  also audited but are deliberately outside or different in this slice.
- `include/libtorrent/settings_pack.hpp` defines the units and intended
  semantics of those settings, especially automatic optimistic 20%, the
  adaptive send target, connection slack, and the fact that
  `max_queued_disk_bytes` bounds download writes rather than upload reads.
- `include/libtorrent/add_torrent_params.hpp` and
  `src/torrent.cpp::{torrent,set_max_uploads,set_max_connections}` establish
  `-1` as the unlimited per-torrent connection and upload default.
- `src/session_impl.cpp::{session_impl,incoming_connection,
  recalculate_optimistic_unchoke_slots,recalculate_unchoke_slots,
  preemptive_unchoke,try_connect_more_peers}` owns the descriptor-aware
  connection clamp, normal/slack admission, global outbound ceiling,
  fixed-slot calculation, automatic optimistic count, timer cadence, and
  immediate vacancy fill. Its `second_tick` turnover branch chooses a torrent
  only when the session or torrent is near its connection limit and usable
  replacement candidates exist.
- `src/choker.cpp::{unchoke_compare_rr,unchoke_sort}` provides the complete-seed
  round-robin comparison: torrent/peer priority first, then 20-piece quota plus
  one minute, recent successful upload, and oldest last unchoke.
- `src/peer_connection.cpp::{incoming_request,fill_send_buffer,
  second_tick,disconnect_if_redundant}` provides request admission, adaptive
  read/send occupancy, one-second samples, 60-second no-request behavior, and
  pressure-sensitive inactivity cleanup.
- `src/bt_peer_connection.cpp::{write_piece,on_sent}` and
  `include/libtorrent/stat.hpp::stat` separate piece payload ranges from
  protocol bytes and propagate successful payload totals/rates.
- `src/platform_util.cpp::max_open_files` supplies the process-limit query and
  fallback values used by the connection calculation.
- `src/session_impl.cpp::session_impl` also marks local peer-class connections
  as exempt from ordinary unchoke slots and gives them separate connection
  weighting. RSTorrent deliberately does not adopt those implicit local
  exemptions, as recorded below.

The pinned tests and simulations used as the completeness checklist are:

- `test/test_settings_pack.cpp::{default_settings,default_settings2}` for
  exact default-table round trips;
- `simulation/test_optimistic_unchoke.cpp::optimistic_unchoke` for eventual
  rotation across 20 waiting peers;
- `simulation/test_swarm.cpp::{unchoke_slots_limit,
  unchoke_slots_limit_negative}` for slot ceilings;
- `simulation/test_timeout.cpp::{no_request_timeout,
  no_request_timeout_slow_upload}` for 60-second idle behavior without
  penalizing a response still in flight;
- `test/test_fast_extension.cpp::{reject_predictive_piece_requests,
  invalid_request,incoming_have_all}` for request rejection and seed
  availability cases;
- `test/test_peer_list.cpp::{incoming_size_limit,double_connection,
  double_connection_loose,double_connection_win}` for incoming saturation and
  duplicate-direction cases; and
- `test/test_transfer.cpp` for ordinary multi-peer payload transfer patterns.

No source or fixture from these paths is imported. RSTorrent tests are written
against the recorded behavior and independently constructed payloads.

### JSTorrent behavior and failure lessons

The local first-party reference is `../jstorrent` at
`9895410beeed6aff554053769bd006a3fbd373ef`. Relevant inspected paths are:

- `packages/engine/src/config/config-schema.ts` defaults to 200 global peers,
  20 peers per torrent, four upload slots, 500 pipeline requests, a 512 KiB
  fixed watermark, and unlimited upload bandwidth. Only the shared 200-peer
  convergence is corroborating evidence; libtorrent remains the default
  oracle for the other values.
- `packages/engine/src/core/bt-engine.ts::handleIncomingConnection` uses a
  50-peer pending set but enters optional MSE before that bound. RSTorrent
  retains its earlier eight-task pre-protocol bound.
- `packages/engine/src/core/torrent.ts::addPeer` coordinates pending and
  established counts and caps incoming peers at 60% of a torrent. That ratio
  is not adopted because libtorrent's matching per-torrent default is
  unlimited.
- `packages/engine/src/core/torrent-uploader.ts` has useful queue, read,
  watermark, choke, disconnect, and multiple-peer scenarios, but its
  fire-and-forget reads, 500-request queue, fixed 512 KiB watermark, cancel
  omission, and pre-socket upload accounting are not adopted.
- `packages/engine/src/core/peer-coordinator/unchoke-algorithm.ts` provides a
  useful task-free test seam but applies a four-slot downloading-oriented
  tit-for-tat/random-optimistic policy. RSTorrent implements the pinned
  complete-seed round-robin default instead.
- `packages/engine/test/core/{torrent-uploader.test.ts,
  torrent-connection-limits.test.ts}` and
  `packages/engine/test/core/peer-coordinator/unchoke-algorithm.test.ts`
  supply independently reauthored scenarios for count transitions, zero-slot
  revocation, watermark refill, read failure, disconnect/choke races,
  multi-peer isolation, slot caps, and optimistic rotation.

## Adopted Defaults And Explicit Deviations

| Concern | Tactical default | Oracle and disposition |
| --- | ---: | --- |
| Session connections | 200 configured | Adopt `connections_limit`; lower through the same FD formula. |
| Incoming slack | 10 | Adopt `connections_slack`; incoming only, so absolute maximum is effective limit plus 10. |
| Per-torrent connections | Unlimited | Adopt `add_torrent_params.max_connections = -1` for incoming policy. Existing 30/30 outbound working sets remain separately bounded. |
| Outbound established/pending working sets | 30 / 30 per active torrent | Retain measured Tactical `021` dial/scheduling bounds; explicit RSTorrent deviation beneath the shared 200 ceiling. |
| Pending incoming handshakes | 8 | Retain Tactical `078`; libtorrent has no equivalent pre-routing task cap. |
| Kernel listen backlog | 5 | Adopt `listen_queue_size`; distinct from accepted handshake tasks. |
| Per-torrent uploads | Unlimited | Adopt `add_torrent_params.max_uploads = -1` beneath session slots. |
| Session upload slots | 8 | Adopt `unchoke_slots_limit`. |
| Optimistic slots | Automatic, therefore 1 | Adopt `0`/20% calculation at eight slots. |
| Ordinary/optimistic cadence | 15 s / 30 s | Adopt both intervals. |
| Seed selection | Round robin | Adopt `seed_choking_algorithm`. |
| Seed quota | 20 pieces and more than 60 s | Adopt `seeding_piece_quota` and comparator behavior. |
| Upload bandwidth | Unlimited | Adopt `upload_rate_limit = 0`; finite rate limiting is deferred. |
| Requests per unchoked peer | 2,000 | Adopt documented `max_allowed_in_request_queue` exactly. |
| Request block | 16 KiB | Retain the BEP 3/libtorrent block maximum. |
| Send occupancy | 10-500 KiB at 50% of last second | Adopt all three send-watermark settings. |
| Upload read jobs | 10 | Translate `aio_threads` worker count into blocking-job permits. |
| Open storage handles | 40 | Adopt `file_pool_size`; already matches RSTorrent's shared pool. |
| Handshake/peer/inactivity | 10/120/600 s | Adopt the matching libtorrent timeouts. |
| Existing outbound peer I/O | Existing download policy | Deliberate scope boundary; this tactical changes incoming upload activity handling, not proven outbound download deadlines. |
| Multiple connections from one IP | Allowed for distinct connection identities | Deliberate loopback/NAT-friendly RSTorrent deviation from `allow_multiple_connections_per_ip = false`; IP address is not treated as peer identity. |
| Outbound connection pacing | Existing download scheduler | Libtorrent's 30 attempts/s was audited; changing the proven download scheduler is outside this upload slice. |
| Peer turnover | Deferred with general multi-torrent candidate scheduling | Libtorrent's 4% at 90% every 300 s was audited; there is no equivalent session-wide RSTorrent replacement-candidate owner yet. |
| No-request timeout | 60 s when no response remains active | Adopt source behavior and simulation evidence. |
| Writer no-progress | 60 s | Retain RSTorrent's bounded joined-I/O rule; no exact libtorrent setting. |
| Local-peer exemptions | None | Deliberate deviation: loopback peers consume connection and upload limits. |
| Choker tie | Stable generation order | Deliberate deterministic equivalent to unspecified container order. |
| Queued request off-by-one | Exactly 2,000 | Adopt setting documentation, not the apparent incidental `>` check. |

Applying limits to loopback is important even though this campaign's listener
is still loopback-only. Local processes are not trusted protocol input, and
exempting them would make the declared upload-slot and connection evidence
false precisely in the controlled environment. A later peer-class tactical
may add explicit trusted-local policy; it must not silently infer trust from
an IP range.

The 10 incoming slack connections remain a hard, observable ceiling even
without the deferred turnover owner. They may fall back under the normal limit
through ordinary close, timeout, duplicate rejection, or the existing
torrent-local replacement path; this tactical does not invent a partial
session-wide "worst peer" ranking and call it libtorrent parity.

## Ownership, Tasks, Cancellation, And Data Flow

```text
ApplicationService
  -> SessionPeerBudget (task-free counters + owned generations)
       -> outgoing PeerSocketSet connecting/established generations
       -> IncomingPeerService accepted/established generations
  -> IncomingPeerService
       -> listener + accept task + at most 8 handshake tasks
       -> UploadCoordinator runtime adapter + one joined timer task
            -> task-free UploadScheduler
            -> latest-value grant generation per connected seed peer
       -> SeedReadExecutor + at most 10 joined read jobs
            -> session StorageFilePool (40 handles total)
       -> SeedRegistration generation per eligible complete torrent
            -> bounded JoinSet of peer controllers and writers
            -> shared immutable metadata, piece geometry, bitfield,
               availability, and content plan
            -> task-free UploadPeerState per peer
                 -> reader/controller task
                 -> writer task + byte-charged frame queue
```

The budget owns counts and permits, never child tasks. The owner that creates a
socket task owns and joins that task. The coordinator owns policy membership
and grant generations, not sockets. The seed-read executor owns admitted read
jobs and returns typed completions to the exact request generation. A
registration owns every routed peer controller and writer for its torrent;
the single `Option<JoinHandle>` from Tactical `078` becomes a bounded
collection rather than a detached task per peer.

Registration stop order is:

1. remove the registration from routing and reject new peers;
2. remove its peers from scheduler eligibility and publish choke/revoke
   generations;
3. cancel queued request descriptors and not-yet-started frames;
4. cancel and join upload reads, close sockets, and join writers/controllers;
5. release connection generations and file leases; and
6. return only when per-registration peer/read/writer counts are zero.

Application shutdown first closes the listener, joins pending handshakes, then
performs this order for every registration, joins the coordinator and read
executor, and finally asserts that shared session counts are zero. Pause,
archive, selection change, force recheck, repair, relocation, and removal use
the same per-registration order before changing content authority.

## Module And Dependency Direction

Keep this work inside existing crates. Expected cohesive boundaries are:

- `rstorrent-engine::peer_budget` for runtime-independent connection
  admission, FD-derived effective limits, direction/slack rules, permits, and
  snapshots;
- an `upload` module directory retaining request/cancel state and adding a
  runtime-independent seed scheduler plus deterministic clock inputs;
- an engine upload runtime or focused `incoming` submodule for coordinator
  membership, latest-value grants, timer ownership, peer joins, and read-job
  admission;
- a direction-neutral split reader/writer seam adjacent to `peer_io`, with
  byte-charged frames and exact payload markers; and
- `seed_content` integration with the existing `StorageFilePool` rather than
  another file cache or blocking-task pool.

Exact filenames may follow what the implementation makes clearest. Do not add
a crate, generic actor framework, async trait layer, second session service,
or one-file-per-type layout. `rstorrent-protocol` continues to own only wire
values/codecs. The application passes shared budget/runtime handles inward; an
engine module must not depend outward on application or persistence types.

Refactor `peer_io` only as needed to split read/write ownership while
preserving its outbound users and byte metrics. Do not route outbound socket
traffic through the incoming service. `PeerSocketSet` retains outbound task
ownership and merely acquires the shared connection generation supplied by
the application.

## Resource Bounds And High-Water Assertions

| Resource | Default/hard bound |
| --- | ---: |
| Configured ordinary TCP connections | 200 session-wide |
| Effective ordinary TCP connections | FD-clamped, minimum 5 |
| Additional accepted incoming connections | 10 session-wide |
| Pending pre-handshake tasks | 8 session-wide |
| Seed registrations | Existing 1,024 |
| Interested peers holding upload grants | 8 session-wide |
| Optimistic grants inside the eight | 1 at defaults |
| Queued request descriptors | 2,000 per unchoked peer; 16,000 across eight grants |
| Logical requested bytes | 32,768,000 per peer; 262,144,000 across eight grants |
| Active upload read jobs | 10 session-wide |
| Bytes per upload read | 16 KiB maximum |
| Read plus serialized target | Adaptive 10-500 KiB per unchoked peer |
| Read plus serialized hard charge | 528,396 B per unchoked peer; 4,227,168 B across eight grants |
| All-peer writer charged occupancy | 110,963,160 B at the 210-connection absolute ceiling |
| Open path/SAF handles | Existing 40 session-wide pool |
| Writer frame/control descriptors | 64 per peer, byte bound still authoritative |
| Peer activity/handshake deadlines | 120 s / 10 s |
| Recent rejection detail | Existing bounded 32 records |

The logical requested-byte and all-peer writer rows describe hostile claims
and scheduling charge, not equivalent resident payload allocation. Tests must
measure descriptors, logical charge, shared backing, and unique payload
buffers separately. Shared immutable metadata, piece geometry, availability,
and bitfield buffers are retained once by their registration, not once per
peer; per-peer encoder cursors and headers remain charged per connection.
Snapshots expose configured/effective/slack connections, counts by direction
and phase, interested/choked/regular/optimistic peers, descriptor and
logical-byte high water, read jobs/bytes, writer frames/bytes, storage handles,
exact byte classes, timeout/rejection reasons, and terminal task counts.

## Shape-Changing Edge Cases

The common path must land with:

- effective connection limits below 200, exactly at normal capacity, every
  incoming slack value through 10, and rejection at normal plus 10;
- outgoing dial cancellation and incoming handshake failure without permit
  leaks or transient double counting;
- more than one incoming peer for one torrent and peers spread across multiple
  complete registrations competing for one global slot set;
- interest before/after coordinator registration, loss of interest, vacancy
  fill, disconnect, pause, and registration removal at each choke interval;
- exact 15/30/60-second boundaries, quota equal versus quota-plus-one, short
  final pieces, unequal torrent piece lengths, and clock advance across more
  than one missed interval;
- zero, one, seven, eight, nine, and at least 20 interested peers, including
  eventual optimistic service for every continuously eligible peer;
- request counts 1,999/2,000/2,001, maximum block geometry, duplicate
  descriptors, exact cancel, cancel during read, choke during read, and stale
  completion after a new grant generation;
- storage latency with all 10 read jobs occupied, fair admission after one
  completion, read error, short read, missing file, and file-pool pressure;
- a socket that reads normally, a fragmented writer, a peer that never reads,
  writer timeout, disconnect after a partial frame, and another peer making
  uninterrupted progress throughout;
- piece geometry and encoded bitfields for the largest supported v1 piece
  count without full per-peer copies, plus BEP 9 response pressure through the
  same bounded writer;
- exact accounting across partial writes, write failure, cancellation before
  first byte, cancellation after first byte, protocol-only traffic, metadata,
  duplicate piece payload, and successful full content; and
- pause, archive, selection change, recheck, removal, replacement, application
  shutdown, and task panic with zero terminal connection/read/writer/file
  owners.

Same-peer-ID cross-direction resolution and the existing conservative
duplicate-endpoint behavior receive regression tests, but a new general
peer-ID reputation/deduplication subsystem is not part of this slice. If the
shared budget exposes a correctness bug there, fix the exact generation race
without expanding into durable peer history.

## Implementation Sequence And Intermediate Gates

1. **Defaults and pure policy gate.** Add typed default values, FD-limit
   calculation, connection admission transitions, upload scheduler, adaptive
   watermark calculation, and table-driven boundary tests. No socket changes
   land before exact defaults and deviations are executable assertions.
2. **Shared connection gate.** Pass one budget from `ApplicationService` into
   outgoing and incoming owners. Remove the one-established-peer semaphore,
   make registration peers a bounded joined set, and prove mixed-direction
   capacity/release with injected small limits.
3. **Writer/accounting gate.** Split peer read/write ownership, add charged
   frame queues and payload markers, record only successful partial writes,
   repair the stale Speed metric catalog entry, and keep every existing
   outbound peer-I/O test green.
4. **Read pipeline gate.** Route seed reads through the 10-job owner and shared
   40-handle pool, replace the one-read state with generation-fenced adaptive
   fill, and prove request/read/writer high-water and round-robin admission.
5. **Multi-peer scheduling gate.** Wire interested peers into the global
   coordinator, start choked, implement preemptive vacancy fill plus default
   15/30-second evaluations, and prove quota/optimistic rotation with an
   injected clock.
6. **Adversarial runtime and lifecycle gate.** Exercise slow readers, storage
   stalls, cancellation, timeout, registration replacement, pause/recheck/
   removal, task failure, and joined shutdown with at least 10 scripted peers.
7. **Controlled interoperability gate.** Extend the isolated incoming-seeding
   harness to simultaneous RSTorrent and libtorrent leechers, independently
   verify every payload, record per-peer/session byte totals and resource high
   water, and clean all temporary owners and files.
8. **Truth and handoff gate.** Run workspace validation and update this
   tactical, the owning topics, readiness matrix, and protocol claims with
   only the evidence that passed. Record persisted settings as the next
   boundary without adding placeholders.

## Implementation And Evidence

All eight gates completed. The implementation keeps task-free admission,
scheduler, request, and accounting policy in focused engine modules while
`ApplicationService` owns one shared peer budget and the incoming runtime owns
the joined coordinator, peer readers/writers, and storage reads. Incoming and
outgoing sockets now consume the same configured 200-connection,
descriptor-clamped budget; incoming intake retains eight handshake tasks, a
five-entry listen backlog, and ten connections of slack. Internal
`ApplicationConfig` values expose the enforcing peer, scheduler, read, and
timeout policies for tests and later settings work without adding persistence,
generated contracts, or UI.

Deterministic and scripted tests prove the adopted default table and boundary
transitions: normal/slack admission and release, 0/1/7/8/9/20-peer scheduling,
one optimistic grant inside eight total slots, immediate vacancy fill,
15/30-second rotation, the strict 20-piece/60-second seed quota, exact
1,999/2,000/2,001 request admission, ten read permits, and the 40-handle shared
storage pool. A one-handle cross-file case proves that one logical upload read
does not deadlock by retaining multiple file leases. Injected time validates
the 120-second activity, 60-second keepalive, 60-second no-request, 600-second
near-cap inactivity, and 60-second write-progress boundaries.

The incoming writer is independently joined from its reader and enforces 64
queued descriptors plus 528,396 bytes of read/serialized charge per peer.
Request and registration generations suppress stale reads and frames before
their first byte after cancel, choke, lost interest, replacement, or teardown;
once a frame starts, it finishes or closes the connection. Partial-write tests
prove that peer, torrent, and session counters advance only for piece-payload
bytes successfully written, with protocol and BEP 9 metadata remaining
separate. Exact scoped totals and nonoverlapping one-second rates are available
from bounded engine snapshots, and the session Speed catalog now advertises
the existing `payload_uploaded` metric only as available.

The controlled loopback harness held two RSTorrent and two libtorrent 2.0.13
leechers active against one RSTorrent seed before releasing the throttled
libtorrent clients. All four independently hash-verified a 67,109,595-byte,
4,097-piece fixture. The seed recorded four established peers at high water,
500 queued request descriptors, 8,192,000 queued logical bytes, four active
reads, 65,536 read bytes, bounded writer charge, and exactly 268,438,380
physical piece-payload bytes: four complete copies and no requested/read/queued
inflation. A restarted RSTorrent seed independently served another exact
67,109,595-byte copy. Separate libtorrent and restarted-RSTorrent runs verified
an 8,401,233-byte multi-file fixture whose requests cross file boundaries and
include a short final piece.

The final gate passed `cargo fmt --all -- --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, Python
syntax validation for the controlled harness, the full controlled
`incoming_seeding.py` run, and `git diff --check`.

The implementation deliberately retains the prior 30-established/30-pending
outbound torrent working sets, the eight-task incoming handshake cap, the
60-second writer no-progress fence, stable generation tie breaks, and no
loopback exemption. Persisted or live-mutated settings, finite bandwidth,
ratio/time goals, incomplete-torrent upload, ordinary Swarm/Peers projection,
port advertisement, non-loopback binding, and gateway mapping remain outside
this completed slice.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure defaults/state | Exact adopted constants; FD clamp/fallback arithmetic; normal/slack direction rules; permit transfer/release; fixed slots; automatic optimistic count; 15/30-second cadence; vacancy fill; quota/minute boundaries; stable ties; adaptive watermark clamps. |
| Upload protocol state | Initially choked; interest eligibility; 2,000 descriptor limit; request geometry; duplicate/cancel/choke generations; no payload allocation on descriptor admission; stale read/frame suppression. |
| Storage/runtime component | Ten read jobs and 40 handles at high water; fair peer turns; slow/read-failure/short-read cases; adaptive writer occupancy; no shared immutable buffer multiplication; terminal zero jobs/leases. |
| Scripted TCP runtime | Mixed normal/slack admission; at least 10 peers; eight grants; optimistic rotation; partial/blocked writer; keepalive/no-request/inactivity/write deadlines; unrelated-peer progress; exact partial-write accounting; joined shutdown. |
| Application lifecycle | Multiple complete registrations compete under one session policy; outgoing and incoming counts share one ceiling; completion/restart/pause/archive/recheck/removal ordering remains fenced; zero owners at close. |
| RSTorrent interoperability | At least two simultaneous RSTorrent magnet leechers obtain metadata and independently hash complete content from the reported incoming endpoint. |
| Libtorrent interoperability | At least two simultaneous libtorrent 2.0.13 leechers connect inbound-only, complete and independently hash the same fixture without a reverse RSTorrent dial. |
| Mixed evidence | All four required leechers overlap in time; a scripted slow reader does not stall them; payload totals equal successful physical piece bytes and metadata remains separate. |
| Workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, focused interop commands, and `git diff --check`. |

Default-value tests should read the production default constructors rather
than restating a second test-only table. Timing tests use an injected clock or
paused Tokio time; runtime progress uses barriers and explicit completion
channels rather than correctness-sensitive sleeps. Full 200/210 socket
saturation is not required in routine tests when the identical state machine
is proven with injected smaller ceilings and the production defaults are
asserted separately.

## Non-Goals

This tactical does not add:

- persisted or live-mutated listener, connection, upload-slot, bandwidth, or
  seeding-goal settings;
- visible settings UI or new generated application contracts;
- finite upload bandwidth limiting, per-torrent rate limits, ratio/time goals,
  or automatic stop policy;
- upload from incomplete torrents, tit-for-tat, torrent priorities, peer
  classes, or trusted-local limit exemptions;
- libtorrent's general 30-attempts/second connection scheduler or
  4%/90%/300-second session peer-turnover policy;
- ordinary incoming-peer rows in the generated Peers/Swarm views or durable
  per-peer history; bounded engine/application snapshots are sufficient here;
- non-loopback binding, tracker/DHT port advertisement, LAN/public
  reachability, UPnP IGD, PCP, NAT-PMP, or external-router evidence;
- MSE/PE, uTP, FAST reject-request, PEX, LSD, hole punching, IPv6, super
  seeding, or v2/hybrid upload;
- platform-capability/SAF seed registration beyond using the already shared
  handle pool where a seed-content plan is currently eligible; or
- public-swarm performance claims or tuning away from pinned defaults.

## Escalation And Next Boundary

Ordinary implementation authority includes the focused module extractions
above, enabling an already present low-level dependency feature needed to read
process file limits, tightening an internal bound while retaining the adopted
user-facing default, independently authored adversarial cases, and fixing a
same-boundary accounting or cancellation bug revealed by the tests.

Stop for direction before adding a new third-party dependency with meaningful
tradeoffs, weakening a pinned default without representative evidence,
changing the complete-torrent-only eligibility contract, adding persistence
or generated schema, exposing a non-loopback socket, launching physical
clients, or expanding into general multi-torrent product scheduling.

After this tactical passes, the next campaign slice is persisted listener,
connection, upload-slot, bandwidth, and seeding-goal settings. That tactical
must reuse the enforcing owners and the exact default table established here;
it must not expose placeholders. Incoming peer view integration may accompany
that settings slice only if its lifecycle and bounded projection are planned
explicitly; port advertisement and gateway mapping remain later independent
slices.
