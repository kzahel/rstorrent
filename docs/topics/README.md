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
