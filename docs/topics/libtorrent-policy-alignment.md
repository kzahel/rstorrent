# Libtorrent Policy Alignment

Topic: `libtorrent-policy-alignment`

Status: **Active.** The initial cross-policy audit was recorded on 2026-08-27
against pinned Rasterbar libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. Tactical
[`182`](../tactical/182-bounded-outbound-attempt-and-metadata-turnover.md) is
the active source-first implementation of `LPA-001` and `LPA-002`.

## Scope

This topic owns the continuing comparison of RSTorrent and pinned libtorrent
defaults, limits, deadlines, admission policies, and resource behavior. It is
the durable ledger for:

- values and observable behavior already aligned;
- deliberate differences retained for RSTorrent's ownership model or product
  platforms;
- candidate changes whose expected benefit and resource cost still need
  evidence; and
- completed tacticals that change the comparison baseline.

This topic does not replace:

- [`oracle-driven-engine-campaign.md`](oracle-driven-engine-campaign.md), which
  owns the source-first execution runbook and current engine checkpoint;
- [`performance-and-live-evidence.md`](performance-and-live-evidence.md), which
  owns comparative measurement and public-run evidence policy;
- [`peer-lifecycle.md`](peer-lifecycle.md), which owns concrete peer records,
  attempts, connections, replacement, and cancellation invariants;
- [`protocol-support.md`](protocol-support.md), which owns BEP claims; or
- numbered tacticals, which own one bounded implementation slice and its exact
  source/test dossier.

The comparison is broader than numeric equality. A timeout applied once to a
whole attempt is observably different from the same timeout applied to several
sequential phases. A per-torrent limit beneath a global owner is different
from a session-wide limit even when both display the same value.

## Desired Direction

Move common RSTorrent behavior closer to the reliability, interoperability,
and performance of pinned libtorrent where that behavior fits RSTorrent's
first-party Rust engine and explicit ownership model. Preserve bounded hostile
input, exact cancellation and join, truthful observability, and platform
resource safety.

Libtorrent is the required completeness and edge-case oracle, not an automatic
source of product defaults or an architecture template. Alignment may mean:

1. adopting the same default and observable transition;
2. adopting the behavior with a more explicit RSTorrent resource owner;
3. selecting a conservative platform-specific value under one shared
   contract; or
4. retaining an intentional difference and recording why.

Correctness, security, ownership, and cleanup outrank numeric similarity.
Reliability and startup latency outrank unmeasured connection breadth, and
measured throughput outranks a speculative larger queue.

## Reference Baseline

The current comparison pin is Rasterbar libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. No source, fixture, or test data is
copied.

The initial audit inspected:

- `src/settings_pack.cpp` and `include/libtorrent/settings_pack.hpp` for
  default values and their documented semantics;
- `src/session_impl.cpp::try_connect_more_peers` for session connection pace,
  smoothing, fairness, and the global limit;
- `src/torrent.cpp::{do_connect_boost,want_peers,want_peers_download}` for
  tracker-response startup breadth and metadata participation in ordinary
  peer admission;
- `src/peer_connection.cpp::second_tick` for the connecting timeout and
  adaptive request timeout;
- `src/utp_stream.cpp` and `aux_/utp_socket_manager.hpp` for initial uTP
  timeout and SYN retransmission behavior;
- `src/peer_list.cpp` for peer-list bounds, failure count, reconnect delay, and
  candidate selection; and
- `src/ut_metadata.cpp` for two per-peer metadata requests, three-second block
  reassignment, rejection cooldown, and hash-failure cooldown.

Every implementing tactical must re-open the exact relevant source **and
tests** rather than treating this cross-policy snapshot as its complete source
dossier. A future pin change requires reconciling this topic before comparing
new evidence.

## Behavior Already Closely Aligned

The following current defaults or shapes are intentionally close to the
pinned reference. “Aligned” here applies only to the stated behavior; it does
not claim identical internal state or every adjacent transition.

| Concern | RSTorrent | Pinned libtorrent | Current posture |
| --- | ---: | ---: | --- |
| Session peer connections | 200 | 200 | Exact configured default; RSTorrent additionally clamps against available file descriptors. |
| Incoming connection slack | 10 | 10 | Exact default beneath the shared peer budget. |
| TCP listen backlog | 5 | 5 | Exact default; distinct from accepted handshake tasks. |
| Desktop active downloads | 3 | 3 | Exact product default; mobile adaptation is recorded below. |
| Upload slots | 8 | 8 | Exact fixed-slot default. |
| Regular/optimistic unchoke cadence | 15 s / 30 s | 15 s / 30 s | Exact default cadence with independently authored scheduling. |
| Storage file pool | 40 | 40 | Exact ordinary default. |
| Ordinary peer failure ceiling | 3 | 3 | Exact default ceiling. |
| Reconnect base | 60 s | 60 s | Shared base; exact failure-scaled eligibility remains a transition-level comparison. |
| Peer TCP connect phase | 15 s | 15 s | Same configured number, but RSTorrent may apply additional sequential phase deadlines as recorded below. |
| Incoming BitTorrent handshake | 10 s | 10 s | Exact ordinary default. |
| Peer activity/inactivity | 120 s / 600 s | 120 s / 600 s | Closely aligned long-lived incoming policy. |
| Initial/max requests per peer | 4 / 500 | 4 / 500 | Adopted request-window endpoints with independent feedback state. |
| Request queue horizon | 3 s | 3 s | Exact default target. |
| Maximum request timeout | 60 s | 60 s | Same terminal maximum with independently authored adaptive timing. |
| Tracker peers/body | 200 / 1 MiB | 200 / 1 MiB | Exact bounded response breadth and body maximum. |
| BEP 9 peer metadata receive | 30 MiB | 30 MiB | Exact receive ceiling; a hint above 4 MiB is ignored until piece zero supplies authoritative bounded geometry. |
| Metadata requests/reassignment | 2 / 3 s | 2 / 3 s | Exact per-peer in-flight count and block reassignment interval. |

Completed Tactical
[`181`](../tactical/181-paced-metadata-connection-cohort.md) deliberately
adopts libtorrent's 30-peer startup breadth without adopting its immediate
30-attempt boost. RSTorrent bounds pending dials plus connected metadata
workers at 30 and spaces accepted attempts at a configurable no-burst default
of ten per second. Pending work continues to consume the 200-connection
session budget and fair outbound admission.

## Current Delta Ledger

Priorities describe likely user impact, not authorization to implement.
Suggested values are starting hypotheses for a source-first tactical and may
change after tests and resource measurements.

### LPA-001: Whole Outbound Attempt Lifetime

Priority: **High; strongest next startup candidate.**

RSTorrent currently gives a preferred uTP connection up to 15 seconds. On
failure it tries TCP within the same attempt with another 15-second connect
deadline, then uses the 60-second peer I/O deadline for the outgoing
BitTorrent handshake. Because these are phase-local deadlines, an accepted
attempt can occupy one peer-budget permit for much longer than the displayed
15-second connect value; the worst sequencing can approach 90 seconds before
the BitTorrent handshake completes.

Pinned libtorrent uses `peer_connect_timeout = 15` while the peer remains in
its connecting state. Its uTP transport starts with a three-second timeout and
two configured SYN retransmissions; it does not implement RSTorrent's exact
sequential uTP-then-TCP attempt shape.

Recommended investigation:

- make 15 seconds a total outbound transport-plus-BitTorrent-handshake budget
  rather than a fresh allowance for each phase;
- inspect a bounded roughly three-second uTP-to-TCP fallback or hedge point
  without misrepresenting libtorrent's three-second value as a total uTP
  lifetime;
- retain one attempt, peer-budget permit, registry generation, cancellation,
  and terminal observation across the policy; and
- prove black-holed uTP, slow successful uTP, silent TCP accept, late TCP
  success, MSE preference/fallback, cancellation, and Android behavior.

### LPA-002: Saturated Metadata-Worker Turnover

Priority: **High; directly adjacent to Tactical `181`.**

RSTorrent already makes an unanswered metadata block eligible elsewhere after
three seconds. A connected peer that contributes no accepted metadata can
nevertheless occupy one of the hard 30 combined cohort slots until the common
60-second metadata-progress deadline expires. Pinned libtorrent has the same
three-second block reassignment shape, but no separate 30-worker metadata
ceiling beneath its ordinary torrent and 200-session connection admission.

A conservative RSTorrent adaptation should consider replacing a
zero-contribution metadata worker after roughly 10--15 seconds **only** when
all 30 slots are occupied and another eligible candidate is waiting. A sparse
swarm with no replacement should retain the existing longer deadline. Any
design must distinguish accepted metadata progress from unrelated peer-wire
chatter and avoid repeatedly evicting a slow peer that is actually
contributing.

### LPA-003: Session-Wide Dial Rate

Priority: **High for resource shape; medium for measured latency.**

Metadata attempts now have a ten-per-second no-burst pacer. Ordinary content
has a 30-pending local bound plus fair session outbound turns, but no
clock-based session attempt rate. Pinned libtorrent distributes a nominal
`connection_speed = 30` across torrents, enables `smooth_connects`, and may
spend a one-time `torrent_connect_boost = 30` after the first tracker response.

The likely RSTorrent direction is one session-global no-burst dial pacer above
the torrent-local owners. A candidate initial profile is 30 attempts per
second on desktop and ten on Android, with metadata still limited to the lower
ten-per-second value. The exact profile is not accepted until a tactical
proves concurrent-torrent fairness, no catch-up burst, permit accounting,
Android task/socket pressure, and no startup regression.

### LPA-004: Ordinary Per-Torrent Connection Breadth

Priority: **Medium; evidence-gated.**

RSTorrent permits 30 established content peers plus 30 pending dials for one
torrent. Libtorrent's default per-torrent connection value is effectively
unlimited beneath its 200-session cap. RSTorrent's explicit local bound is an
intentional ownership and memory guard rather than an accidental eight-peer
legacy value.

Do not replace it with 200 blindly. Consider a larger or dynamically shared
desktop value only when representative evidence repeatedly shows all 30
established slots occupied, useful eligible peers waiting, unused session
capacity, and request/storage resources below pressure. Android may retain 30
even if desktop grows. A connection that contributes no useful work is not a
reason by itself to allocate more sockets when bounded replacement can solve
the same problem.

### LPA-005: uTP Service Connection Cap

Priority: **Medium for multiple active torrents; low for one metadata cohort.**

RSTorrent owns at most 64 live uTP connection workers, independently below the
200-peer session budget. Pinned libtorrent has no comparably low uTP-only
default cap. One 30-peer metadata cohort cannot reach this boundary, but
several active downloads can and may fall back to TCP despite available global
peer permits.

A later tactical may derive the desktop uTP ceiling from the effective peer
budget or raise it through measured steps. Android should retain 64 until
per-connection retransmission, reorder, datagram-queue, timer, task, CPU, and
RSS high waters support more. Global peer admission remains authoritative in
either case.

### LPA-006: Retained Peer Records

Priority: **Low.**

RSTorrent retains at most 1,000 peer records per torrent. Pinned libtorrent
defaults to 3,000 for an active torrent and 1,000 while paused. A tracker
reporting roughly 300 seeders is far below RSTorrent's current bound, so this
does not explain the motivating metadata delay.

A 3,000-record desktop profile and 1,000-record mobile profile is reasonable
only if diagnostics show potentially useful records being evicted. Any change
must measure retained memory, candidate scan work, source balance, duplicate
merging, and pause/restart ownership rather than assuming more records imply
more useful peers.

### LPA-007: Pending Incoming Handshakes

Priority: **Low for download startup; separate seeding concern.**

RSTorrent admits eight pending incoming handshake tasks beneath a five-entry
kernel backlog and the effective connection budget plus ten incoming slack.
Libtorrent's deprecated half-open limit defaults to unlimited, leaving its
global connection owner as the principal bound.

Raising RSTorrent to ten or 16 may improve short incoming bursts, but it does
not improve outbound metadata acquisition and increases hostile socket,
handshake, MSE, timer, and task exposure. Any change belongs to the incoming
reachability/seeding owner with saturation and denial-of-service evidence.

### LPA-008: Mobile Active Downloads

Priority: **Deliberate platform difference; no current change recommended.**

Desktop matches libtorrent's default of three active downloads. Android and
iOS currently clamp the shared setting to two. Raising the mobile cap may
reduce queue wait for a third torrent while making each active torrent's
connection, hashing, storage, and memory contention worse. Retain two until
representative devices prove that three improves product outcomes within
thermal, lifecycle, memory, and cleanup bounds.

## Broader Audit Backlog

Future audits may add stable ledger entries for:

- peer turnover percentage, cutoff, interval, and RSTorrent's
  usefulness-aware replacement grace;
- active seed and checking admission rather than only active downloads;
- tracker-operation session breadth, tier policy, announce cadence, and
  failure backoff;
- DHT lookup, routing, transaction, peer-store, and announce bounds;
- PEX cadence and retained contacts;
- socket receive/send watermarks and maximum peer receive buffering;
- uTP retransmission counts, timers, ingress queues, connection queues, and
  mixed TCP/uTP policy;
- MSE CPU-work admission and plaintext fallback timing;
- piece-stall detection, duplicate timing, endgame thresholds, and peer
  turnover after request stalls;
- disk queue bytes, storage/hash concurrency, file checking memory, and
  platform file-handle limits;
- upload request intake, send watermarks, seed choking, and active seed
  lifetime; and
- event/diagnostic queue bounds where dropped observability could hide a
  policy stall.

These are audit areas, not a proposal to copy every libtorrent setting. Some
libtorrent values describe owners RSTorrent deliberately does not have, while
some RSTorrent owners need explicit bounds absent from libtorrent.

## Change Contract

Each alignment tactical must:

1. state the user-visible failure or measured opportunity rather than merely
   cite a different number;
2. inspect the exact pinned libtorrent implementation **and tests**, relevant
   specifications, RSTorrent owner, and JSTorrent product history where useful;
3. record whether it adopts the same behavior, an adapted platform value, or
   an intentional difference;
4. define configured, effective, per-torrent, per-session, pending,
   established, queue, and high-water meanings without conflating them;
5. preserve one identifiable owner, cancellation path, joined termination,
   hostile-input bound, and truthful diagnostics;
6. validate pure transitions, scripted time/failure cases, controlled
   interoperability, concurrent-torrent fairness where applicable, and both
   maintained Android ABIs; and
7. record CPU, RSS, task, socket/permit, queue, and retained-byte high waters
   in proportion to the changed resource.

A public-swarm observation may identify a problem and a repeated controlled
cohort may support a policy choice. One changing swarm does not establish the
new default by itself.

## Active Tactical Boundary

Tactical
[`182`](../tactical/182-bounded-outbound-attempt-and-metadata-turnover.md)
implements a bounded **outbound attempt deadline and metadata cohort
turnover** slice covering `LPA-001` and the tightly related `LPA-002`
transition. It stops when:

- one explicit total attempt budget bounds uTP selection/fallback, TCP
  connection, plaintext/MSE negotiation, and the BitTorrent handshake;
- a full metadata cohort can replace a zero-contribution worker without
  evicting a contributing slow peer or churning a sparse swarm;
- black-holed, silent, late-success, slow-useful, rejection, chatter,
  cancellation, and policy-change cases release exact registry generations,
  tasks, sockets, and peer-budget permits;
- controlled pinned-libtorrent metadata acquisition remains identity exact;
  and
- desktop/workspace gates plus Android arm64-v8a and x86_64 builds pass with
  recorded resource high waters.

The tactical is the sole **Now** while active. On completion, this topic must
record the landed timing/turnover behavior and the ordinary queue restores
Tactical `176`; this living topic remains a ledger rather than a competing
backlog.
