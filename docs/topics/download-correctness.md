# Download Correctness

Topic: `download-correctness`

Status: Controlled one-peer v1 downloads verify, persist, resume, and publish
correctly within their recorded profiles. Reliable completion across ordinary
multi-peer swarms is not established. A macOS desktop run on 2026-07-31 was
observed near 99.9% without further progress; the exact cause was not captured,
but the current single-live-peer architecture contains several sufficient
causes for that class of stall.

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

The current driver keeps one live content connection, one active piece, and a
piece-local block pipeline. Multi-file wanted pieces are visited in index
order. Choke releases outstanding block reservations for the same peer, but a
content disconnect is terminal and newly discovered peers are not connected
while the current content connection remains installed.

That architecture admits several independent near-completion stalls:

- the current peer never advertised the final wanted piece;
- the final block is withheld while keepalives or other timely messages keep
  the connection-level read deadline from expiring;
- a useful peer is discovered after content transfer begins but cannot join;
- the current peer disconnects and content work terminates instead of moving;
  or
- a final piece hash fails and the torrent terminates instead of retrying.

Endgame is also absent. Duplicate piece messages are correctly rejected under
the current single-request model, but that behavior cannot remain torrent-
fatal once intentional duplicate endgame requests exist. The peer wire codec
does not yet implement the core cancel message.

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
- **Plausible sufficient mechanisms:** sole peer lacks the final piece,
  stranded request without a per-request expiry, inability to use a later
  discovered peer, or terminal hash-retry behavior.
- **Required next evidence:** reproduce with controlled peers split so that
  the final wanted piece exists only on the second peer, then retain scheduler
  and peer-availability diagnostics through verified completion.

## Scenario Ledger

States in this ledger describe the full required result, not merely whether a
related unit test exists.

| ID | Scenario | Required result | Current state and evidence |
| --- | --- | --- | --- |
| DL-C01 | One peer supplies a complete healthy torrent | Every wanted piece verifies and selected storage publishes. | Passing: controlled runtime and libtorrent interop across small, large-piece, selective, magnet, and tracker fixtures. |
| DL-C02 | Peer A lacks the final wanted piece; peer B has it | Scheduler keeps or opens B, assigns the piece, and completes. | Failing by architecture: only one content connection and no transfer failover. Defining scenario for the next transfer tactical after the DHT campaign. |
| DL-C03 | Active content peer disconnects with requests outstanding | Its generation releases requests and another eligible peer may receive them. | Partial: reservations are cancelled locally, but the torrent returns a terminal content error. |
| DL-C04 | Peer chokes with requests outstanding | Requests cease belonging to that peer and remain schedulable without budget leakage. | Partial deterministic evidence: same-peer re-request after unchoke works; alternate-peer assignment is absent. |
| DL-C05 | Peer remains responsive but withholds one requested block | Per-request expiry releases the block and another peer can serve it. | Absent: connection I/O deadlines do not constitute a per-request deadline. |
| DL-C06 | Tracker discovers a useful peer after content begins | Observation joins the registry and may become a live content source while work remains. | Partial discovery only: observation is retained, but the one live content connection prevents use. |
| DL-C07 | Final blocks are slow across several peers | Endgame issues bounded duplicates without violating payload or request budgets. | Absent. |
| DL-C08 | One endgame copy arrives after another completed | First valid block wins, losers are cancelled, and late duplicates are harmless. | Absent; duplicate blocks are terminal under the current single-request model. |
| DL-C09 | A received piece fails its SHA-1 hash | No have bit is set; the whole piece becomes schedulable again; contributor evidence is bounded. | Partial: mismatch is detected and no have bit is set, but execution terminates and attribution is absent. |
| DL-C10 | Storage write fails after a block arrives | The block is not considered received and no false verified state is committed. | Passing deterministic state evidence; broader filesystem recovery policy remains incomplete. |
| DL-C11 | Restart claims all but one piece | Claimed pieces are rechecked, the missing piece downloads, and publication occurs once. | Passing controlled process-death and conservative recheck evidence for the established multi-file profile. |
| DL-C12 | A claimed piece changed on disk before restart | Recheck clears only the bad claim and never publishes it as verified. | Passing controlled corruption and resume evidence. |
| DL-C13 | Final wanted piece crosses selected and skipped files | Full logical piece is reconstructed, hash-verified, and bytes land in their correct storage classes. | Passing deterministic and controlled libtorrent selective-file evidence. |
| DL-C14 | Wanted piece contains BEP 47 padding bytes | Synthetic zeros participate in verification without writing a padding file. | Passing deterministic and controlled selective-storage evidence. |
| DL-C15 | Pause or shutdown occurs during active work | No new work starts, owners cancel and join, durable state stays conservative, and sockets close. | Passing runtime, web, AVD, and selected physical lifecycle evidence within current one-peer scope. |
| DL-C16 | All current trackers fail but retain retries | Torrent reports waiting with the next automatic discovery action, not blocked. | Passing deterministic, runtime, web, and AVD evidence. |
| DL-C17 | Network policy is offline | No DNS or socket work occurs and UI requests network enablement without rewriting torrent intent. | Passing deterministic, runtime, web, and AVD evidence. |
| DL-C18 | Torrent reaches displayed 100% before publication finishes | State remains incomplete until verified content completes the publication contract. | Passing path and Android SAF publication-state evidence for controlled fixtures. |
| DL-C19 | Multi-piece single-file torrent | The ordinary product path downloads, verifies, resumes, and publishes all pieces. | Absent: current execution rejects this profile even though metainfo parsing understands it. |
| DL-C20 | Every established slot is occupied by peers that never unchoke | A useful eligible candidate eventually replaces an unproductive peer after bounded grace and can receive work. | Absent: there is one live connection and no capacity-pressure replacement policy. |
| DL-C21 | Every established peer lacks the remaining wanted piece | A peer advertising that piece is retained or opened, while irrelevant peers cannot monopolize every slot. | Absent: availability is one-connection state and newly discovered peers cannot join content transfer. |
| DL-C22 | Pending dial slots connect but never finish handshake | Per-operation deadlines release dial capacity and another candidate can be tried without exceeding socket/task bounds. | Partial: individual handshake deadlines exist, but there is no bounded parallel dial set or capacity scenario. |
| DL-C23 | An expired request is reassigned and the old generation sends its block late | Current ownership and payload accounting remain correct; valid late data is harmless and cannot release another attempt. | Absent: there is no torrent-level request generation or request expiry. |
| DL-C24 | All current peers are unproductive and no replacement is eligible | The torrent retains discovery/retry deadlines and avoids destructive reconnect churn; it reports waiting rather than blocked. | Absent for a multi-peer connection set. |
| DL-C25 | Hostile peer churn and observations fill every configured bound | Registry, connection, dial, request, payload, event, task, history, and diagnostic limits hold while uniquely useful or active state is protected. | Partial registry evidence only; multi-peer runtime resources do not exist. |

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

The next transfer-correctness slice stops when DL-C02 through DL-C06 and
DL-C20 through DL-C25 pass through one bounded multi-peer request and
connection-set owner; all existing single-peer, storage, resume, tracker, and
DHT evidence remains green; and OBS-2026-07-31-001 has enough new scheduler
observability that a future occurrence can be classified from retained state.
Endgame and recovery scenarios DL-C07 through DL-C09 remain the following
transfer slice.

DHT discovery is now installed, so late and decentralized peer observations
can exercise this campaign rather than merely populate the registry. Routine
engine validation remains headless; no additional product UI is required by
these slices.
