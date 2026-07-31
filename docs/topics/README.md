# Topics

Focused, living records of continuing concerns live here.

Prefer the smallest coherent topic whose status, decisions, evidence, gaps, and
next work benefit from continuity across sessions or commits. A topic can own a
product decision, contract, recurring problem, implementation campaign, status
question, or investigation. Split topics when their decisions can evolve
independently.

Create or update a topic when:

- work spans multiple tacticals or commits;
- important decisions or invariants need to survive the current session;
- new evidence changes the direction;
- current status is otherwise difficult to answer; or
- the user explicitly requests a living topic.

Do not create a topic for every small standalone change.

New topics should normally contain:

- a crisp scope;
- a `Topic: <slug>` line matching the filename;
- an honest status;
- the decisions and invariants currently in force;
- relevant evidence, known gaps, and recommended next work; and
- links to implementing tacticals when they exist.

Architecture and reference docs own durable system shape and external facts.
Topics own the current truth for a continuing concern. Tactical docs under
`../tactical/` own bounded implementation slices and execution records.

## Current Topics

- [`product-direction.md`](product-direction.md): initial product motivation,
  first-party Rust engine decision, platform posture, non-goals, open choices,
  and recommended bring-up sequence.
- [`capability-readiness.md`](capability-readiness.md): master engine and product
  scoreboard, evidence vocabulary, priority policy, and bounded current queue.
- [`download-correctness.md`](download-correctness.md): completion, integrity,
  request ownership, recovery invariants, observed incidents, and executable
  scenario ledger.
- [`protocol-support.md`](protocol-support.md): precise BEP support claims,
  deliberate limits, interoperability evidence, and protocol sequencing.
- [`client-persistence.md`](client-persistence.md): SQLite-backed client state,
  verified metadata and resume invariants, cross-platform storage-root
  identity, and the application-service boundary above the torrent engine.
- [`application-control.md`](application-control.md): shared semantic commands,
  responses, snapshots, revisions, and the boundary between in-process
  application control and future transports.
- [`client-surfaces.md`](client-surfaces.md): shared browser/Tauri web
  presentation, Android Compose adaptation, generated client types, reactive
  view delivery, and platform lifecycle boundaries.
- [`peer-lifecycle.md`](peer-lifecycle.md): peer observations, bounded records,
  derived dial eligibility, connection attempts, live connections, and the
  discovery-to-swarm boundary.
- [`tracker-discovery.md`](tracker-discovery.md): tracker URL and announce
  lifecycle, bounded results, scheduling direction, and the
  tracker-to-peer-observation boundary.
