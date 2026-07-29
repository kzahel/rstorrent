# Engine Engineering Principles

Status: accepted defaults for engine and product implementation.

## Purpose

RSTorrent should grow into a broad, mature torrent engine without becoming
opaque to the people who own it. These principles describe the engineering
character to preserve as features accumulate.

They are defaults, not ceremonial rules or substitutes for judgment. A
tactical may make a different choice when current evidence justifies it, but
the tradeoff should be deliberate and visible.

## Desired Character

Prefer an engine that is:

- explicit enough that ownership and state transitions can be followed;
- deterministic where behavior does not inherently require I/O or time;
- bounded in its treatment of peer-controlled input and resource use;
- observable enough to explain failures from product diagnostics;
- testable in layers and interoperable as a complete system; and
- composed from ordinary, coherent parts rather than architecture machinery
  built for hypothetical futures.

The goal is not minimal source code or maximum abstraction. The goal is local
reasoning: a maintainer should be able to determine who owns state, what caused
a transition, where an operation can fail, and how to reproduce it.

## Simplicity And Abstraction

Start with plain structs, enums, functions, and coherent modules. They normally
make protocol values, state machines, ownership, and failure paths easier to
see than deep trait hierarchies or highly generic frameworks.

Introduce a trait when there is a concrete seam such as:

- more than one real implementation;
- an operating-system or storage capability boundary;
- substitution required for important deterministic tests; or
- a dependency whose isolation materially improves the design.

Use generics when they improve correctness or measured hot-path behavior
without hiding control flow. Avoid speculative service layers, dependency
injection frameworks, async trait surfaces, or actor abstractions merely
because the engine may become large.

An abstraction should earn its place by clarifying ownership, enforcing an
invariant, isolating a dependency, enabling a valuable test, or removing
demonstrated duplication. Similar-looking code is not automatically the same
policy.

Keep an eye out for modules that accumulate unrelated responsibilities or
become difficult to understand and test. Refactor when the benefit is concrete
and proportionate to the active tactical, not as a ritual after every change.

## State Ownership And Concurrency

Mutable state should have an identifiable owner. Prefer designs where commands,
events, and state transitions make mutation explicit. Avoid using
`Arc<Mutex<Everything>>`, global mutable registries, or detached tasks as the
default answer to coordination.

Pure state transitions should not perform I/O. Network, storage, clock, and
runtime adapters translate external events into domain inputs and execute
domain actions.

Every background task must eventually have:

- a component that owns it;
- a defined cancellation or shutdown signal;
- a way for its completion and failure to be observed; and
- a join or supervision path.

Shutdown is a correctness path, not cleanup to add after downloading works.
One peer disconnect, timeout, malformed message, or task failure must not leave
session state internally contradictory.

## Runtime And Protocol Boundaries

Protocol values, codecs, and deterministic state transitions remain independent
from Tokio, sockets, filesystems, task handles, channels, and system clocks.
Runtime and platform code depend inward on those layers.

When scheduling policy needs time or randomness, make those inputs controllable
enough for deterministic tests. Do not spread calls to wall-clock time or
thread-local randomness through domain logic.

Platform integration should expose capabilities to the engine rather than move
hot-path networking or piece data through a generic proxy. A future extension
control channel carries application commands, snapshots, and events; it does
not become a peer socket or filesystem transport.

## Untrusted Input And Resource Bounds

Treat metainfo, tracker responses, DHT traffic, extension messages, and every
peer byte as untrusted.

- Validate lengths, ranges, identifiers, and state preconditions before
  allocation, indexing, or mutation.
- Apply explicit limits to frames, nesting, collections, pending work, peer
  counts, buffers, and queues.
- Do not panic on malformed network or metainfo input.
- Handle unknown extensions and unsupported behavior explicitly.
- Preserve enough error context to identify the peer, message, and violated
  contract without dumping payload contents.

Use distinct types for hashes, indices, offsets, lengths, and identifiers when
they prevent realistic confusion. Do not create a type hierarchy for its own
sake.

## Data Integrity And Storage

Piece verification is the authority for content validity. Unverified blocks may
eventually need temporary on-disk representation, but they must never be marked
or presented as verified content.

Storage behavior must eventually account for short reads and writes, partial
files, truncation, out-of-space failures, permissions, removal, concurrent
access, crash recovery, and platform-specific lifetime rules. Do not assume
desktop path semantics will transfer to Android SAF.

The preferred platform seam gives Rust a bulk-I/O capability such as a usable
file descriptor. Piece payloads should not bounce through high-frequency
language callbacks or serialized process messages.

## Buffer Ownership And Performance

Make buffer ownership and lifetime understandable before trying to eliminate
every copy. Each copy in a hot path should eventually have a known purpose, but
zero-copy machinery is not automatically simpler or faster.

Bound memory independently of peer claims and swarm size. Backpressure and
concurrency limits are product correctness, especially on mobile hardware.

Measure before making performance claims or introducing complexity. Preserve
realistic benchmarks and traces when they answer a specific question. Avoid
architectural choices that inherently require payloads to cross unnecessary
process or language boundaries.

## Observability And Diagnostics

Observability is a first-party engine feature. Structured events and tracing
should make it possible to follow important work using stable identifiers for
the torrent, peer, piece, block, and owning component.

Diagnostics should reveal:

- state transitions and their causes;
- requests, timeouts, retries, and cancellations;
- protocol rejection reasons;
- scheduling and storage decisions when relevant; and
- task and component lifecycle.

Logs are not the product state API. Clients should consume deliberate commands,
snapshots, and events rather than scrape diagnostic text. Avoid logging payload
bytes, secrets, or unbounded peer-controlled strings, and keep high-volume
diagnostics controllable.

Errors should retain actionable context without collapsing every failure into
one generic string or exposing internal backtraces as the user experience.

## Testing And Support Evidence

Prefer deterministic generated fixtures and tests that run without the public
Internet. Use the cheapest layer that can prove the behavior:

1. unit tests for values, codecs, invariants, and state transitions;
2. property or fuzz testing for parsers and stateful edge cases;
3. scripted peers for fragmentation, invalid input, reordering, timeout, and
   disconnect behavior;
4. mature independent implementations such as libtorrent for
   interoperability;
5. controlled real-swarm exercises where simulation is insufficient; and
6. product validation on the actual desktop and physical Android/ChromeOS
   surfaces.

A feature is not product-supported merely because a type or parser branch
exists. Once the first protocol behavior lands, maintain a capability ledger
that can distinguish:

```text
implemented
  -> unit tested
  -> scripted-peer tested
  -> independently interoperable
  -> real-world exercised
  -> product supported
```

Not every feature needs every stage before a development build, but public
support claims must state the evidence actually available.

## Feature Growth

Grow through falsifiable vertical slices that produce an externally observable
result. A slice may cut through several layers, but it should not quietly
expand into general infrastructure or feature parity.

libtorrent is a protocol and maturity oracle, not a single parity milestone.
Add capabilities deliberately, including their failure behavior,
interoperability evidence, resource limits, and product consequences.

Prefer completing a narrow end-to-end path over building a broad framework
whose first real caller is still unknown. Allow later slices to refactor from
evidence rather than requiring the first slice to predict the final engine.

## Dependencies And Toolchain

Use focused dependencies for commodities such as cryptographic primitives and
platform integration when they improve correctness and maintenance. Before
adopting one, consider its scope, maintenance, target support, license,
transitive weight, and whether it would own behavior that belongs to the
first-party engine.

Commit the Rust lockfile and pin or record the toolchain once the workspace is
created. Add dependency and license auditing when actual lockfiles exist.
Reference-repository dependencies do not become RSTorrent dependencies merely
because the references are checked out locally.

## North-Star Invariants

- Peer-controlled input cannot cause unbounded allocation or unbounded queued
  work.
- Unverified data is never reported or presented as verified content.
- Protocol and deterministic scheduling behavior can be tested without real
  sockets, storage, or wall-clock time.
- Every background task has an owner and an observable termination path.
- A peer-local failure cannot corrupt torrent- or session-wide state.
- Hot-path peer and piece data does not cross a generic process or language
  proxy.
- Product support claims are backed by recorded evidence.
- Diagnostics can explain important state transitions without becoming the
  application API or leaking payload data.

## Questions To Keep In Mind

These are prompts for ordinary implementation and review, not a required
checklist after every edit:

- Who owns this state and its invariant?
- Can the important behavior be tested without unrelated infrastructure?
- What bounds peer-controlled memory, queues, and work?
- Who cancels and joins this task?
- Can one component fail without leaving another component inconsistent?
- Can diagnostics explain why this transition occurred?
- Is a module accumulating unrelated responsibilities?
- Does an abstraction solve a current problem or only anticipate one?
- What evidence justifies saying this feature is supported?

When evidence changes one of these principles, update this document rather than
quietly accumulating exceptions.
