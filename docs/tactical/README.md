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

The next tactical number is `004`.
