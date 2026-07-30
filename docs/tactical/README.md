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

The next tactical number is `012`.
