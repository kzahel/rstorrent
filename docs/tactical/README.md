# Implementation Tactical Docs

Bounded implementation plans and execution records live here.

Use zero-padded numeric filenames:

```text
000-first-slice.md
001-next-slice.md
```

Keep one coherent implementation slice per tactical. A tactical should be
small enough to have one falsifiable stopping condition while still producing
an end-to-end result. Parent sequencing documents are allowed when a campaign
needs them, but individual implementation work should still have bounded child
tacticals.

Create a tactical before substantial implementation. It should normally state:

- status;
- motivation and desired outcome;
- dependencies and references;
- scope;
- non-goals;
- contracts and invariants;
- implementation direction without unnecessary line-by-line prescription;
- exact validation and interoperability evidence; and
- the stopping condition or next-slice boundary.

Update the tactical as implementation reveals new facts. When complete, record
what landed, what validation actually ran, known gaps, and the recommended next
slice. Completed tacticals remain in place as execution records; living
direction belongs in `../topics/`.

## Decision-Complete Tacticals

A tactical intended for autonomous execution must settle enough direction that
ordinary implementation discoveries do not require repeated approval. In
addition to the fields above, record:

- the stable topic scenarios or observations that define the problem and the
  exact subset the tactical must make pass;
- the normative specifications and pinned reference source/tests that must be
  surveyed before finalizing state transitions;
- the owner, task, cancellation, dependency, and data-flow map, including
  which state must remain runtime independent;
- exact initial resource bounds, or a bounded range plus explicit authority to
  choose and tighten a conservative value from reference and test evidence;
- shape-changing edge cases that must land with the common path rather than be
  deferred into an incompatible architecture;
- the staged implementation order and the intermediate gates that keep a
  large slice diagnosable;
- a validation matrix separating pure state, scripted runtime, controlled
  interoperability, platform build, and opt-in live evidence;
- explicit non-goals and the next-slice boundary; and
- an escalation contract naming what does and does not require human input.

Unless a tactical says otherwise, in-scope implementation authority includes
ordinary refactoring, adding adversarial cases implied by its invariants,
choosing internal names and module layout, tightening declared limits, fixing
newly exposed bugs at the same ownership boundary, updating generated types,
and updating the tactical and owning topics with actual evidence. These are not
reasons to stop merely because the initial plan did not predict the exact code
edit.

Stop for human direction when evidence requires a materially different product
behavior, protocol-support claim, persistence or compatibility contract,
external dependency or license posture, destructive data action, visible or
physical-device interaction not already authorized, or a significant expansion
beyond the tactical's stated owner and non-goals. An ordinary test failure,
internal refactor, conservative bound choice, public-smoke timeout, or a
reference implementation whose architecture differs from RSTorrent is not by
itself an escalation.

Autonomy does not broaden permissions silently. A tactical that needs public
network access, fixture downloads, emulator/device use, schema migration,
generated-contract changes, or another externally visible action must state
that scope and its cleanup or compatibility rules explicitly.

## Current Tacticals

- [`000-first-verified-piece.md`](000-first-verified-piece.md): completed
  download and verification of one multi-block piece from a controlled
  libtorrent peer, establishing the pure protocol/runtime boundary.
- [`001-bounded-large-piece.md`](001-bounded-large-piece.md): completed
  block-granular staging and streamed verification of a 32 MiB piece under a
  256 KiB engine-owned payload allowance.
- [`002-selective-multi-file-storage.md`](002-selective-multi-file-storage.md):
  completed cross-file mapping, skipped-file part storage, mixed-source
  verification, durable reopen, and materialization through an edge-rich
  libtorrent fixture.
- [`003-android-storage-feasibility.md`](003-android-storage-feasibility.md):
  completed native descriptor, SAF, sparse-offset, reopen, cancellation,
  publication, filesystem, and allocation evidence in three runs each on an
  AVD, Chromebook ARCVM, physical Pixel 7a, and Moto X4 internal and removable
  exFAT storage.
- [`004-android-engine-bootstrap.md`](004-android-engine-bootstrap.md):
  completed in-process engine packaging behind UniFFI, foreground-service
  ownership, direct Rust networking, bounded app-private storage, cancellation,
  peer-failure, activity-recreation, and exact cleanup evidence on an AVD,
  Chromebook ARCVM, and Moto X4.
- [`005-saf-selective-storage.md`](005-saf-selective-storage.md): closed after
  proving descriptor-backed selective download, provider publication, forced
  restart verification, and exact cleanup on an AVD, Chromebook ARCVM, and
  Pixel 7a. Unavailable Moto rows and remaining provider-failure profiles are
  recorded as deferred rather than claimed.
- [`006-magnet-metadata-peer-hint.md`](006-magnet-metadata-peer-hint.md):
  completed bounded v1 magnet parsing, direct `x.pe` bootstrap, bidirectional
  BEP 9 metadata exchange, same-connection content download, and independent
  libtorrent evidence in both directions.
- [`007-durable-session-control.md`](007-durable-session-control.md):
  completed one transport-neutral application contract,
  profile-local SQLite authority, exact magnet metadata retention, durable
  verified-piece checkpoints, and conservative process-death resume.
- [`008-reactive-multi-surface-control.md`](008-reactive-multi-surface-control.md):
  completed recoverable bounded reactive views, generated TypeScript and
  Kotlin contracts, and controlled browser/WebSocket, Tauri/channel, and
  Android/UniFFI product threads.
- [`009-android-saf-session-storage.md`](009-android-saf-session-storage.md):
  completed durable Android SAF root identity, descriptor-backed restart,
  provider publication recovery, and controlled AVD and Pixel evidence.
- [`010-peer-registry-magnet-failover.md`](010-peer-registry-magnet-failover.md):
  completed bounded peer observations, records, deterministic selection,
  guarded dial lifecycle, connection and protocol failover, and
  same-connection magnet handoff.
- [`011-one-shot-udp-tracker.md`](011-one-shot-udp-tracker.md): completed
  bounded BEP 15 connect/announce exchange, tracker observations, session
  source retention, and tracker-only magnet metadata/content transfer from a
  controlled libtorrent seed.
- [`012-bounded-diagnostics-progress.md`](012-bounded-diagnostics-progress.md):
  completed prompt task-terminal supervision, active/waiting/blocked progress
  assessment, a bounded filtered typed diagnostic stream, equivalent
  web/Tauri and Android presentation, and isolated headless Chrome and
  no-window AVD evidence.
- [`013-explicit-live-network-policy.md`](013-explicit-live-network-policy.md):
  completed explicit offline, loopback-only, and online outbound policy,
  online desktop and Android product networking, loopback-isolated harnesses,
  offline progress, and bounded network-operation deadlines without a
  whole-download timeout.
- [`014-scheduled-udp-tracker-lifecycle.md`](014-scheduled-udp-tracker-lifecycle.md):
  completed supervised UDP tracker records, multi-tracker fallback, bounded
  retry and reannounce scheduling, loss recovery, token reuse, and equivalent
  waiting diagnostics on the web and Android surfaces.
- [`015-headless-live-comparison.md`](015-headless-live-comparison.md):
  complete; added the catalog-backed alternating comparator, controlled
  publication validation, deterministic result tests, and first paired
  metadata and full-download baselines.
- [`016-dht-discovery-foundation.md`](016-dht-discovery-foundation.md):
  complete; added the session-owned bounded IPv4 DHT participant, private
  gating, warm restart, peer integration, controlled libtorrent completion,
  and an honest public trackerless attempt.
- [`017-adversarial-multi-peer-liveness.md`](017-adversarial-multi-peer-liveness.md):
  complete; replaced the one-live-peer content boundary with a bounded
  torrent-owned connection set and request scheduler driven by adversarial
  liveness scenarios.
- [`018-inspectable-metadata-acquisition.md`](018-inspectable-metadata-acquisition.md):
  complete; added bounded peer-registry and BEP 9 acquisition snapshots,
  closed metadata-slot starvation, and classified tracker-only failure versus
  repeated public DHT metadata completion.
- [`019-torrent-owned-metadata-acquisition.md`](019-torrent-owned-metadata-acquisition.md):
  complete; replaced independent per-peer BEP 9 transfers with one bounded
  cross-peer block owner, added source-derived request pacing, and met the
  tracker, DHT functional, and catalog metadata gates.
- [`020-sustained-transfer-parity.md`](020-sustained-transfer-parity.md):
  complete; replaced the static four-request/four-piece transfer ceiling with
  a bounded source-derived per-connection feedback window and classified
  initial peer-source breadth as the remaining 50% boundary.
- [`021-initial-peer-working-set.md`](021-initial-peer-working-set.md): complete;
  adds bounded initial tracker-operation breadth, separates half-open and live
  peer capacity, adds per-peer diagnostics, and classifies a duplex peer-task
  deadlock as the next boundary.
- [`022-duplex-peer-task-liveness.md`](022-duplex-peer-task-liveness.md): active;
  breaks command/event backpressure cycles without dropping or unbounding
  peer messages, then repeats the controlled and live transfer gates.

Tactical `015` completed the oracle campaign's headless measurement
foundation. Current prioritization and the compaction-safe restart
checkpoint live in [`../topics/capability-readiness.md`](../topics/capability-readiness.md)
and [`../topics/oracle-driven-engine-campaign.md`](../topics/oracle-driven-engine-campaign.md).
