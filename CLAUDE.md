# Repository Instructions

`AGENTS.md` points here so repository automation shares one instruction source.

## Project Entry Points

Start with [`README.md`](README.md), then read [`docs/vision.md`](docs/vision.md),
[`docs/engineering-principles.md`](docs/engineering-principles.md),
[`docs/topics/product-direction.md`](docs/topics/product-direction.md), and
[`docs/topics/capability-readiness.md`](docs/topics/capability-readiness.md),
[`docs/topics/oracle-driven-engine-campaign.md`](docs/topics/oracle-driven-engine-campaign.md),
then [`docs/references.md`](docs/references.md). Once an implementation
tactical exists, read it and every focused topic it names before changing code
in its scope.

For maintainer-specific cross-project context, see
`~/code/dotfiles/projects/README.md` when that checkout is available.

## Current Product Direction

RSTorrent is a new implementation with a first-party Rust BitTorrent engine
and is the likely incubation path for a future generation of the JSTorrent
product. It is not a line-by-line port and does not inherit JSTorrent feature
parity as an initial requirement.

Preserve these starting constraints unless the user explicitly changes them or
a living topic records an accepted replacement:

- The torrent engine is implemented in this repository rather than delegated
  to libtorrent, librqbit, or another engine dependency.
- The engine owns ordinary peer networking, hashing, scheduling, session
  state, and hot-path data movement.
- Product clients are first-party and normally run the engine in-process.
- Platform adapters may own operating-system integration such as Android
  activities, lifecycle, permissions, notifications, and SAF document access.
- Do not introduce a native host, companion server, REST/WebSocket socket
  proxy, or separate IO daemon without an explicit architectural decision.
- Android/ChromeOS and desktop are the initial product surfaces. An extension,
  iOS client, remote daemon, and additional platforms are not implied work.
- A future JSTorrent extension is expected to control and integrate with the
  native engine rather than carry peer or file hot paths. This vision does not
  authorize extension or IPC work in an unrelated tactical.
- During the current engine-correctness campaign, do not add product UI unless
  the user explicitly changes the campaign. Operate and diagnose feature work
  through the headless application boundary.

These are direction guardrails, not permission to invent a complete
architecture before the relevant tactical.

## Engineering Character

Follow [`docs/engineering-principles.md`](docs/engineering-principles.md).
In particular:

- Prefer plain structs, enums, functions, and explicit state transitions.
  Introduce traits, generics, or framework layers when they solve a concrete
  ownership, dependency, testing, reuse, or measured performance problem.
- Give mutable state and background tasks identifiable owners. Every task needs
  a cancellation and observable termination path as concurrency grows.
- Treat all metainfo and network input as hostile. Bound peer-controlled
  allocation, queues, and work before changing state.
- Prefer depth before breadth for every new protocol or storage capability.
  Study specifications and mature references first, then front-load edge cases
  that change state, ownership, persistence, integrity, resource bounds, or
  interoperability into the initial tactical and tests. Do not let a
  happy-path implementation establish an architecture already known to fail
  those cases; keep unrelated breadth and policy-only cases bounded or
  explicitly deferred.
- Never present unverified data as verified content.
- Make structured observability part of engine behavior while keeping logs
  separate from application commands, snapshots, and events.
- Claim protocol support from recorded test and interoperability evidence, not
  from the existence of code paths.

These defaults favor local reasoning and debuggability. They do not prohibit an
abstraction or optimization supported by current evidence.

## Architecture And Module Boundaries

Treat code placement and dependency direction as part of correctness. Keep
protocol values, codecs, and deterministic state transitions independent from
async runtimes, sockets, filesystems, task handles, channels, and platform
adapters. Runtime and platform layers may depend inward on those components;
the dependency must not point back outward.

While working, keep an eye out for concrete refactoring opportunities:

- Does each type and operation live with the layer that owns its invariant?
- Did an infrastructure type leak into protocol or domain state?
- Is a module accumulating unrelated responsibilities or becoming a difficult
  to understand "god module"?
- Would a smaller boundary make important behavior testable without networking,
  storage, a clock, or a platform runtime?
- Is duplicated policy revealing a missing shared concept?

Refactor when the benefit is concrete and the work remains proportionate to the
current tactical. Do not interrupt bounded progress for speculative reshaping.
Extract a module or crate when ownership, dependencies, lifecycle, reuse, or
testing justify it; do not create speculative abstractions or
one-file-per-type layouts merely in anticipation of future features. Record a
deferral only when a material known problem is deliberately left in place.

## Feature Campaign Execution

[`docs/topics/capability-readiness.md`](docs/topics/capability-readiness.md)
owns the current queue. During the active engine-parity campaign,
[`docs/topics/oracle-driven-engine-campaign.md`](docs/topics/oracle-driven-engine-campaign.md)
owns the source-first runbook, graduation rules, restart checkpoint, and next
executable action. For each engine, protocol, discovery, scheduling, storage,
or performance feature:

1. Create or update one bounded tactical before implementation. State its
   stopping condition, non-goals, invariants, resource limits, and required
   evidence.
2. Read the normative specifications and inspect the exact pinned libtorrent
   implementation **and tests** before finalizing the design. Record the paths,
   relevant functions or cases, edge-case checklist, behavior adopted, and
   intentional differences. Libtorrent is the required completeness and
   edge-case oracle, not an architecture template or source donor.
3. Inspect JSTorrent behavior and known failures when the feature has relevant
   product or platform history. Record what was learned rather than assuming
   parity.
4. Write the owner/task/cancellation map and module-dependency direction before
   adding runtime work. Identify the concrete boundary improvement if the
   slice includes a refactor.
5. Implement the common path together with edge cases that change ownership or
   state shape, integrity or security, cancellation/retry/restart behavior,
   common interoperability, resource bounds, or stall diagnosis. Optional
   extensions, UI policy, unmeasured micro-optimization, and speculative
   abstraction may be explicitly deferred.
6. Validate in layers: deterministic transitions, scripted runtime failures,
   controlled interoperability, and representative live evidence where useful.
   Record resource high-water marks for work that changes hot paths or
   long-lived state.
7. Update the owning topics, readiness matrix, protocol claims, and tactical
   evidence before committing the completed slice.

Within an approved campaign, continue through these steps without requesting
routine implementation choices from the user. Stop for direction when a
choice materially expands scope, changes an accepted architecture or product
policy, adds a dependency with meaningful tradeoffs, requires destructive or
external action, or otherwise needs authority not already granted.

## Documentation Ownership

Active documentation has these roles:

- `README.md` and `DEVELOPMENT.md` are product and maintainer entry points.
- Durable architecture documents own accepted long-lived system shape.
- `docs/topics/` owns current truth for focused continuing concerns.
- `docs/tactical/` owns numbered, bounded implementation slices and execution
  records.
- `docs/references.md` owns reference provenance and usage policy.

Before changing a continuing concern, read its topic. Update the topic when the
work changes its status, decisions, evidence, validation, gaps, or recommended
direction. Do not create a topic for every standalone change.

New tactical documents use zero-padded numeric names such as
`000-first-download.md`. Keep one bounded implementation slice per tactical.
State scope, non-goals, dependencies, invariants, validation, and the stopping
condition before implementation. Update its status and evidence as work lands;
completed tacticals remain as execution records.

## Reference Discipline

Use protocol specifications and reference implementations to understand
behavior, construct interoperability tests, and compare outcomes. Do not copy
source mechanically or let a reference implementation silently dictate the
architecture.

For every engine feature, inspect the version pinned in
[`reference/pins.toml`](reference/pins.toml), including its tests, as required
by the feature-campaign contract. A tactical records exact paths and the edge
cases extracted from them so a future pin change can be audited. If a local
reference checkout is unavailable, use the pinned upstream source or add
bounded checkout tooling rather than silently substituting memory or an
unversioned implementation.

Before importing source, fixtures, or test data, identify its origin and
license, record why reuse is permitted, and preserve required attribution.
Prefer independently authored tests against public protocol behavior.

The normal local JSTorrent reference is `~/code/jstorrent`. Its most valuable
inputs are product behavior, integration scenarios, deterministic fixture
patterns, Android/ChromeOS lessons, and known failure cases.

## Toolchain And Validation

On configured development machines, source the shell profile before commands
that require Rust, Java, Android, or other locally installed tools:

```bash
source ~/.profile
```

Once the Rust workspace exists, use this default baseline in proportion to the
change:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Add interoperability, Android, desktop, and physical-ChromeOS validation as
their tacticals establish supported paths. Report exactly what ran. Remove
temporary logs, captures, downloads, and investigation artifacts before
finishing.

Live public-swarm and comparative performance smokes are opt-in and follow
[`docs/topics/performance-and-live-evidence.md`](docs/topics/performance-and-live-evidence.md)
and the active campaign runbook.
Engine-only work defaults to headless CLI or application-service validation;
do not launch visible product clients merely to exercise engine behavior.

## ChromeOS Hardware Testing

The authoritative physical-device controller is the separate checkout at
`~/code/chromeos-testbed`. Before ChromeOS hardware work, read
`~/code/chromeos-testbed/skills/SKILL.md`. Start a hardware session with:

```bash
~/code/chromeos-testbed/bin/chromeos doctor
```

Keep RSTorrent-specific build, deployment, and assertions in this repository.
Keep generic device transport, screenshots, UI automation, DevTools, ARCVM
ADB, Crostini, and recovery in the testbed repository.

## Commit Messages

Aim for a subject of 65 characters or fewer and strictly wrap commit bodies at
72 columns. Keep the subject as a scannable result. For nontrivial commits,
preserve the originating motivation, important constraints and non-goals,
implementation direction, validation, and deliberate deferrals when useful.

Prune secrets, transcript detail, and low-signal commentary. Do not mention
Claude, AI, or an AI assistant. Do not add AI co-author or generation trailers.

When a commit materially advances a living topic, append the exact
`Topic: <slug>` trailer.

## Git And Releases

Do not add a remote, push, publish, tag, or release unless the user explicitly
requests it. Before any future push, verify `git config user.name` and
`git config user.email`; stop if they are automation placeholders.
