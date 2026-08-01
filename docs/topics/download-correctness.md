# Download Correctness

Topic: `download-correctness`

Status: Controlled v1 downloads now verify, persist, resume, and publish through
a bounded multi-peer request owner across ordinary single-file and selective
multi-file profiles. Tactical `017` closes the recorded one-peer liveness
mechanisms with request expiry, failover, late discovery, and replacement.
Tactical `020` adds useful-payload request windows and sampled connection
inactivity. Tactical `021` installed tracker fan-out and a bounded 30-peer
admission set, and Tactical `022` removed the resulting duplex channel
deadlock with 3/3 owner-only and paired 50% evidence. Tactical `023` completed
strict endgame duplicates, core cancellation, and public publication.
Tactical `024` completed whole-piece v1 hash recovery, exact-generation
contributor evidence, and bounded peer integrity reputation. Tactical `025`
completed one bounded asynchronous storage owner with exact write/hash,
resume, payload, cancellation, and join ownership. Tactical `026` completed
the paired live peer-utility timeline and selected the continuously occupied
eight-attempt half-open cohort. Tactical `027` expanded that cohort to 30
under the existing live-peer and payload bounds. Its public timeline exposed
discovery events queued behind continuously ready storage; Tactical `028` now
completed fair supervisor intake. Tactical `029` removed redundant selective
hash seeks with exact integrity, but controlled timing stayed neutral and live
storage occupancy remained saturated. Tactical `030` moved the complete
all-wanted piece hash behind one bounded blocking job with exact shared-engine
and Android-target evidence, but performance remained neutral. Tactical `031`
now measures queue wait and per-kind storage service before selection or
request policy changes.

## Scope

This topic owns the continuing definition and evidence for download integrity,
request liveness, piece completion, failure recovery, durable verification,
and publication. It is organized by invariants and adversarial scenarios
rather than by modules or BEPs.

It does not own discovery wire protocols, peer-record retention, application
commands, or platform UI composition. Those topics provide mechanisms and
presentation, while this topic asks whether wanted bytes can safely and
eventually become verified content.

[`capability-readiness.md`](capability-readiness.md) owns the prioritized
cross-product queue. [`peer-lifecycle.md`](peer-lifecycle.md) owns peer records
and connections. [`client-persistence.md`](client-persistence.md) owns durable
have state and storage-root identity.

## Correctness Vocabulary

- A **wanted byte** contributes to at least one selected non-padding file.
- A **wanted piece** contains wanted bytes and therefore must be hash-verified,
  including any skipped-file or padding bytes needed to verify its full hash.
- A **block** is a bounded request range within one piece.
- A **request attempt** is one block assignment to one peer connection
  generation with an issuance time and terminal disposition.
- **Available** means a specific live peer has advertised a piece. It does not
  mean that the peer will serve it correctly or promptly.
- **Verified** means the complete logical piece matched its trusted metainfo
  hash after storage writes completed.
- **Durable** means verified storage was synchronized as required before the
  corresponding have checkpoint could commit.
- **Complete** means every wanted piece is verified and the selected storage
  publication contract has completed. Displayed byte percentage alone never
  establishes completion.
- **Liveness** means that while some automatic action can plausibly advance a
  valid torrent, an owner eventually performs or schedules such an action.

## Required Invariants

### Integrity

- Peer payload is untrusted until the complete piece hash verifies.
- No unverified piece may become authoritative have state or published
  selected content.
- A hash mismatch invalidates the attempted piece, not the metainfo hash or
  unrelated verified pieces.
- Storage failure cannot leave a block counted as received when its bytes were
  not accepted by the storage owner.
- Restart trusts persisted have state only after the configured continuity or
  conservative recheck policy validates current bytes.
- Completion and seeding eligibility must derive from verified wanted pieces
  and completed publication, never a rounded percentage.

### Request Ownership

- Every outstanding request has exactly one torrent-level owner, one peer
  connection generation, and one bounded payload reservation.
- Choke, disconnect, request expiry, connection replacement, pause, and
  shutdown terminate or transfer ownership explicitly.
- A stale socket callback cannot complete or release a newer request attempt.
- Ordinary scheduling assigns a block to at most one peer. Endgame may create
  bounded duplicates only under explicit policy and accounting.
- The first valid endgame response wins; losing requests are cancelled where
  supported, and late valid duplicates are harmless rather than torrent-fatal.

### Availability And Selection

- Availability is connection-scoped and changes only from validated peer
  messages or connection termination.
- A block is assigned only to a live eligible peer known to have its piece,
  except where an explicit protocol extension supplies equivalent evidence.
- No wanted piece becomes permanently unschedulable merely because an earlier
  peer lacked it, choked, disconnected, or timed out.
- Piece selection may optimize rarity, locality, streaming, or fairness only
  after preserving the ability to schedule every wanted piece.

### Progress Assessment

- Failure of one tracker, peer, request, or piece attempt is not automatically
  torrent failure.
- A torrent is waiting, not blocked, while a retained tracker retry, eligible
  peer dial, request expiry, recheck, publication action, or other installed
  automatic mechanism can still act.
- A torrent is blocked only when the next prerequisite requires external
  action and no installed or scheduled automatic mechanism can supply it.
- Progress assessment is a projection from authoritative owners. Diagnostic
  text is explanatory evidence and is never parsed back into state.
- There must not be an unbounded state in which wanted work remains, no request
  can advance it, and no future automatic action is represented.

The earlier idea of `can_torrent_progress()` should therefore not become a
single optimistic boolean. Extend the existing active, waiting, blocked,
inactive, and complete projection with scheduler facts and the next automatic
action. Unknown future peers are not proof of impossibility; exhaustion is
asserted only over installed mechanisms and retained schedules.

## Current Architecture And Known Stall Mechanisms

One torrent supervisor owns a bounded set of live connection generations,
piece/block state, request attempts and deadlines, payload reservations,
storage acceptance, verification, and child-task joins. It schedules across
up to 64 active pieces and eight peers while tracker and DHT discovery remain
live. Each connection has a bounded useful-payload-driven request window;
the torrent payload allowance remains the aggregate authority. Useful response
samples derive a two-to-sixty-second connection inactivity deadline; a stall
releases that generation's window and leaves one probe request. Choke,
disconnect, expiry, and replacement release only the affected generation's
requests; valid late payload cannot release newer ownership.

A failed v1 piece generation now resets as a whole after hashing, preserves
unrelated verified pieces, and retains bounded exact-generation contributors.
A sole source is known bad and banned; ambiguous contributors accumulate
bounded suspicion without false immediate bans. Strict endgame duplicates and
core losing-request cancellation are installed and publicly exercised. The
remaining measured gap is completion performance rather than a known fatal
ordinary transition.

These are structural facts, not a diagnosis of any particular public run.

## Observed Incidents

### OBS-2026-07-31-001: Desktop Torrent Near 99.9%

- **Environment:** Tauri desktop application on macOS with online network
  policy.
- **Observation:** a real torrent reached approximately 99.9% and did not make
  further visible progress.
- **Captured evidence:** user observation only; no authoritative remaining-
  block, availability, peer, or request snapshot was retained.
- **Current classification:** open completion-liveness observation, not a
  confirmed tracker error and not proof of corrupt state.
- **Closed sufficient mechanisms:** controlled tests now pass when the final
  piece exists only on a second peer, a requested block is withheld while
  keepalives continue, a useful peer arrives later, or the first peer closes.
- **Remaining plausible mechanisms:** absent endgame duplicates or terminal
  hash-failure behavior. The observation remains open because its original
  cause was not captured.

## Scenario Ledger

States in this ledger describe the full required result, not merely whether a
related unit test exists.

| ID | Scenario | Required result | Current state and evidence |
| --- | --- | --- | --- |
| DL-C01 | One peer supplies a complete healthy torrent | Every wanted piece verifies and selected storage publishes. | Passing: controlled runtime and libtorrent interop across small, large-piece, selective, magnet, and tracker fixtures. |
| DL-C02 | Peer A lacks the final wanted piece; peer B has it | Scheduler keeps or opens B, assigns the piece, and completes. | Passing deterministic and two-peer split-availability runtime evidence. |
| DL-C03 | Active content peer disconnects with requests outstanding | Its generation releases requests and another eligible peer may receive them. | Passing deterministic and scripted disconnect/reassignment evidence. |
| DL-C04 | Peer chokes with requests outstanding | Requests cease belonging to that peer and remain schedulable without budget leakage. | Passing deterministic and alternate-peer choke/reassignment evidence. |
| DL-C05 | Peer remains responsive but withholds one requested block | Per-request expiry releases the block and another peer can serve it. | Passing explicit-clock and loopback keepalive/late-response evidence. |
| DL-C06 | Tracker discovers a useful peer after content begins | Observation joins the registry and may become a live content source while work remains. | Passing independent delayed tracker and DHT runtime evidence through the same intake boundary. |
| DL-C07 | Final blocks are slow across several peers | Endgame issues bounded duplicates without violating payload or request budgets. | Passing pure, scripted, and four exact public complete runs with 12--88 bounded assignments and zero active request attempts at termination. |
| DL-C08 | One endgame copy arrives after another completed | First valid block wins, losers are cancelled, and late duplicates are harmless. | Passing pure, scripted exact core-cancel-before-storage, and public complete evidence with 0--432 KiB bounded redundancy. |
| DL-C09 | A received piece fails its SHA-1 hash | No have bit is set; the whole piece becomes schedulable again; contributor evidence is bounded. | Passing pure and scripted evidence: sole corrupt and ambiguous multi-source generations reset, preserve unrelated state, apply exact-generation reputation, and complete from a clean generation. |
| DL-C10 | Storage write fails after a block arrives | The block is not considered received and no false verified state is committed. | Passing deterministic state evidence; broader filesystem recovery policy remains incomplete. |
| DL-C11 | Restart claims all but one piece | Claimed pieces are rechecked, the missing piece downloads, and publication occurs once. | Passing controlled process-death and conservative recheck evidence for the established multi-file profile. |
| DL-C12 | A claimed piece changed on disk before restart | Recheck clears only the bad claim and never publishes it as verified. | Passing controlled corruption and resume evidence. |
| DL-C13 | Final wanted piece crosses selected and skipped files | Full logical piece is reconstructed, hash-verified, and bytes land in their correct storage classes. | Passing deterministic and controlled libtorrent selective-file evidence. |
| DL-C14 | Wanted piece contains BEP 47 padding bytes | Synthetic zeros participate in verification without writing a padding file. | Passing deterministic and controlled selective-storage evidence. |
| DL-C15 | Pause or shutdown occurs during active work | No new work starts, owners cancel and join, durable state stays conservative, and sockets close. | Passing runtime, web, AVD, selected physical, saturated-queue, and multi-peer exact-join evidence. |
| DL-C16 | All current trackers fail but retain retries | Torrent reports waiting with the next automatic discovery action, not blocked. | Passing deterministic, runtime, web, and AVD evidence. |
| DL-C17 | Network policy is offline | No DNS or socket work occurs and UI requests network enablement without rewriting torrent intent. | Passing deterministic, runtime, web, and AVD evidence. |
| DL-C18 | Torrent reaches displayed 100% before publication finishes | State remains incomplete until verified content completes the publication contract. | Passing path and Android SAF publication-state evidence for controlled fixtures. |
| DL-C19 | Multi-piece single-file torrent | The ordinary product path downloads, verifies, resumes, and publishes all pieces. | Partial: controlled runtime and 16-piece libtorrent publication pass; durable single-file resume remains absent. |
| DL-C20 | Every established slot is occupied by peers that never unchoke | A useful eligible candidate eventually replaces an unproductive peer after bounded grace and can receive work. | Passing deterministic and full eight-slot loopback replacement evidence. |
| DL-C21 | Every established peer lacks the remaining wanted piece | A peer advertising that piece is retained or opened, while irrelevant peers cannot monopolize every slot. | Passing availability-aware retention/replacement and split-final-piece evidence. |
| DL-C22 | Pending dial slots connect but never finish handshake | Per-operation deadlines release dial capacity and another candidate can be tried without exceeding socket/task bounds. | Passing bounded three-dial runtime evidence with two silent handshakes and one useful peer. |
| DL-C23 | An expired request is reassigned and the old generation sends its block late | Current ownership and payload accounting remain correct; valid late data is harmless and cannot release another attempt. | Passing deterministic ownership/accounting and loopback late-payload evidence. |
| DL-C24 | All current peers are unproductive and no replacement is eligible | The torrent retains discovery/retry deadlines and avoids destructive reconnect churn; it reports waiting rather than blocked. | Passing deterministic deadline and loopback no-churn evidence. |
| DL-C25 | Hostile peer churn and observations fill every configured bound | Registry, connection, dial, request, payload, event, task, history, and diagnostic limits hold while uniquely useful or active state is protected. | Passing bounded deterministic state plus queue-saturation, cancellation, churn, and exact-join runtime evidence for the installed owner. |

## Required Scheduler Observability

The multi-peer owner should expose bounded typed facts sufficient to explain a
stall without dumping payload or scraping log text:

- wanted, verified, active, and missing piece counts;
- missing, requested, writing, and duplicate block counts;
- connected, eligible, dialing, backed-off, and banned peer counts;
- for the active or next pieces, how many live peers advertise availability;
- oldest request age and the next request-expiry deadline;
- the next eligible peer-dial or discovery deadline;
- endgame state and duplicate-request count when installed; and
- the derived reason no request can be issued now.

These facts belong to snapshots or typed scheduler diagnostics according to
their stability. Raw peer addresses, magnets, paths, and payload remain
bounded or redacted.

## Validation Model

Correctness work should normally use all applicable layers:

1. pure state tests for request ownership, stale generations, transitions,
   accounting, and selection;
2. scripted loopback peers for disconnect, choke, silence, corruption,
   duplication, and split availability;
3. controlled libtorrent exchange for independent wire and content evidence;
4. the authenticated loopback gateway with headless Chrome for shared web UI;
5. an explicitly owned no-window AVD for Compose parity; and
6. opt-in live or physical-device runs as additional evidence, never the sole
   correctness oracle.

Routine evidence must not launch, focus, or automate the visible Tauri desktop
application. Harnesses use temporary profiles and storage and must remove
downloads, captures, browser state, AVD state, and subprocesses they own.

## Next Stopping Condition

Tactical `024` completed DL-C09: whole-piece v1 reset, bounded contributor
attribution, known-bad exclusion, and clean retry. Tactical `025` proved that
slow writes and hashes no longer stop peer-event progress while preserving
request/payload accounting, hash recovery, resume ordering, exact publication,
and controlled interoperability. Its corrected localhost result disproved
storage as the retained speed owner.

Tactical `027` now proves exact 30/31 pending-dial ownership, prompt completion
from a useful peer in position 30 behind 29 silent handshakes, and exact
cancellation of 30 silent attempts. Its complete public screen found DHT peer
batches reported at content seconds 30 and 120 but not reflected in the
content registry until termination while storage stayed saturated.

Tactical `028` now proves prompt DHT intake and dial refill while all 66 storage
jobs are occupied. Live DHT reports and registry growth occur in the same
sample, closing the intake defect. Tactical `029` then reduced a common 256 KiB
selective hash from 16 seeks to one while preserving 16 fixed-buffer reads.
Its 32 MiB controlled median moved from 1.101 to 1.121 seconds, so it makes no
speed claim; public screens continued to hit the 66-job storage high-water
mark. Exact full publication completed at 180.64 seconds.

Tactical `030` now executes the common all-wanted piece hash behind one
blocking positional-I/O boundary with fixed-buffer, cross-file, padding,
truncation, task-failure, mixed-source, and Android-target evidence. Its 32 MiB
controlled median remained neutral at 1.139 seconds versus 1.121 immediately
before. Two public 50% samples took 79.47 and 223.85 seconds, one timed out at
359 pieces, and a complete screen timed out at 375 pieces. Every terminal
snapshot still held 66 storage jobs with zero hash failures.

The next stopping condition is exact bounded attribution of storage queue wait,
write service, and hash service through controlled delays and public timeline
evidence. No storage, request, or peer policy changes until those durations
identify the owner.

Routine engine validation remains headless; no additional product UI is
required by that slice.
