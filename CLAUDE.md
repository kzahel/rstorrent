# Repository Instructions

`AGENTS.md` points here so repository automation shares one instruction source.

## Project Entry Points

Start with [`README.md`](README.md), then read
[`docs/topics/product-direction.md`](docs/topics/product-direction.md) and
[`docs/references.md`](docs/references.md). Once an implementation tactical
exists, read it before changing code in its scope.

For maintainer-specific cross-project context, see
`~/code/dotfiles/projects/README.md` when that checkout is available.

## Current Product Direction

RSTorrent is a new product with a first-party Rust BitTorrent engine. It is not
a line-by-line JSTorrent port and does not inherit JSTorrent feature parity as
an initial requirement.

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

These are direction guardrails, not permission to invent a complete
architecture before the relevant tactical.

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
