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

- [`000-first-verified-piece.md`](000-first-verified-piece.md): ready plan for
  downloading and verifying one multi-block piece from a controlled libtorrent
  peer while establishing the pure protocol/runtime boundary.

The next tactical number is `001`.
