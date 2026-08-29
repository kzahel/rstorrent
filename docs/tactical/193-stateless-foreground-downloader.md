# Tactical 193: Stateless Foreground Downloader

Status: **Active as of 2026-08-29.** The user accepted a deliberately simple
one-source foreground downloader, all files by default, with the one exception
that a magnet's BEP 53 `so` selection must behave like the same magnet added to
the first-party client, then explicitly authorized end-to-end autonomous
implementation with logical commits.

Topics:
[`direct-filesystem-storage`](../topics/direct-filesystem-storage.md),
[`client-persistence`](../topics/client-persistence.md),
[`application-control`](../topics/application-control.md),
[`download-roots`](../topics/download-roots.md),
[`runtime-configurations-and-headless-deployment`](../topics/runtime-configurations-and-headless-deployment.md),
[`client-surfaces`](../topics/client-surfaces.md),
[`capability-readiness`](../topics/capability-readiness.md), and
[`oracle-driven-engine-campaign`](../topics/oracle-driven-engine-campaign.md).

Dependencies: completed Tacticals
[`075`](075-ephemeral-application-state.md),
[`100`](100-bep53-select-only-and-duplicate-add-feedback.md),
[`143`](143-dual-identity-and-persistence-foundation.md),
[`188`](188-existing-payload-adoption-and-recheck.md), and
[`191`](191-direct-filesystem-storage.md) supply the private bounded in-memory
application databases, compact pre-metadata selection, opaque runtime owner,
common complete checker, and direct final-path storage this slice must compose
rather than duplicate.

## Motivation And Accepted Outcome

RSTorrent has two diagnostic command-line processes today, but neither is the
requested product:

- `rstorrent-download-piece` drives engine APIs directly and exposes
  diagnostic peer, timeout, skip-file, and checkpoint controls; and
- `rstorrent-session --ephemeral` accepts line-delimited application JSON but
  remains a diagnostic control harness rather than a finite downloader.

Add one native executable for the ordinary case:

```text
rstorrent-download [--output DIRECTORY] SOURCE
```

`SOURCE` is exactly one magnet URI or local `.torrent` file. `--output`
defaults to the current directory. The command opens one online, ephemeral
`ApplicationService`, adds the source through the normal application command,
downloads directly into safe metainfo paths under the output directory, prints
bounded human progress, and exits after every wanted file is verified and its
final storage checkpoint is durable. It does not continue seeding.

The command selects every non-padding file for an ordinary magnet or local
`.torrent`. If and only if a magnet contains BEP 53 `so`, it preserves the
existing strict, bounded RSTorrent interpretation and downloads that resulting
selection. There is no second file-selection syntax and no selection prompt.

The tactical stops when this behavior passes deterministic, controlled
pinned-libtorrent, interrupted/restarted, concurrent-invocation, no-profile-
file, native macOS/Linux/Windows, and proportional Android/iOS regression
gates with exact output, resource high waters, terminal task ownership, and
cleanup recorded.

## Product Contract

### Invocation and sources

- The binary name is `rstorrent-download`. It is a finite foreground program,
  not a daemon, TUI, alternate application server, or renamed diagnostic.
- It accepts exactly one source. A value beginning with `magnet:?` is parsed by
  the existing magnet codec. Any other value is an existing local regular file
  read under the existing bounded `.torrent` intake limit.
- HTTP(S) `.torrent` fetching, stdin source bytes, source lists, recursive
  watch folders, shell expansion, and interactive Add options are absent.
- The output directory is an explicit native path capability. The program
  creates a missing final directory when its parent is valid, then resolves
  and retains its canonical absolute identity. A non-directory, inaccessible
  path, unsafe alias transition, or unavailable current directory fails before
  starting torrent content.
- `--help`, `--version`, `--output DIRECTORY`, and `--` are the complete initial
  option surface. Parsing is strict: unknown options, missing values, multiple
  sources, or extra positionals are usage failures. No argument-parser
  dependency is justified by this grammar.

### Selection

| Source | Initial wanted files | Result |
| --- | --- | --- |
| Magnet without `so` | every non-padding file | ordinary all-files download |
| Magnet with one or repeated valid `so` values | current compact BEP 53 union after metadata resolution | only those in-range, non-padding files |
| Magnet whose valid `so` ranges resolve to no eligible files | none | successful no-payload result after metadata, never fallback to all files |
| Malformed or over-limit `so` | none | existing typed magnet rejection before content |
| Local `.torrent` | every non-padding file | ordinary all-files download |

The CLI sends the original magnet through `Command::AddMagnet`; it does not
preparse and rebuild the URI, expand ranges, or send a separate `skip_files`
list. This preserves Tactical `100`'s 16-KiB URI, 128-parameter, index, range,
exception, repeated-parameter, padding, and empty-selection behavior exactly.
Using `so` is therefore equivalent to pasting that URI into the application
for selection semantics even though this finite surface has no Add dialog.

### Completion, progress, and exit

- Success means every currently wanted non-padding byte is verified and the
  application's final direct-storage checkpoint is synchronized and committed.
  A metadata name, received byte count, `100%` display rounding, or merely idle
  engine is not success.
- A valid `so` selection resolving to zero wanted files exits successfully
  after authenticated metadata establishes that fact and reports `0 files
  selected`.
- The program exits promptly after success and performs joined shutdown. It
  does not seed, wait for ratio/time goals, archive the torrent, or retain a
  session for later commands.
- Interactive stderr may redraw one concise progress line at no more than four
  updates per second. Non-interactive stderr emits state changes and at most
  one progress record per ten seconds. A final human summary goes to stdout.
  This text is not a versioned machine API.
- Progress derives from bounded application views/snapshots and names metadata,
  checking, downloading, stalled/waiting, and final verification truthfully.
  The CLI does not scrape diagnostic strings or create a parallel progress
  reducer.
- Exit `0` is verified selected completion or the explicit zero-selection
  no-op; `2` is usage or local source classification; `3` is an already-locked
  output directory; `4` is rejected source/metainfo/application input; `5` is
  terminal metadata, network, storage, or engine failure. User interruption
  exits `130`; Unix `SIGTERM` exits `143` after the same joined cleanup path.
- Errors and progress may show the safe torrent name and selected output path,
  but never echo the complete magnet, tracker credential, source bytes, peer
  payload, or unbounded remote text.

There is no whole-download timeout. Existing bounded connect, handshake,
tracker, DHT, metadata, peer inactivity, retry, and progress-explanation rules
remain authoritative. A slow but progressing download continues until success,
terminal failure, or user cancellation.

## What Stateless Means

The downloader is stateless at the application layer, not memory-only at the
payload layer:

- it constructs `ApplicationConfig::ephemeral` and therefore uses one private
  bounded in-memory SQLite session database and one private bounded in-memory
  SQLite metrics database;
- the existing schema, transactions, request receipts, source provenance,
  metadata, selection, views, DHT snapshot, have state, and speed history work
  normally for that one process lifetime;
- it supplies no profile root and creates no `session.db`, `metrics.db`, WAL,
  shared-memory file, DHT snapshot, resume file, copied `.torrent`, or other
  durable application catalog;
- all in-memory application state disappears only after the service cancels
  and joins its owners and closes both SQLite connections; and
- final payload files remain in the selected output directory. On a later
  invocation, they regain authority only through Tactical `188`'s common
  content checker; no hidden resume fact is reconstructed from names, sizes,
  timestamps, or the previous process.

This is intentionally the existing application composition with a finite
owner around it. The binary must not call the direct driver used by
`rstorrent-download-piece`, create a second session database model, special-
case torrent state outside `ApplicationService`, or make SQLite optional in
the ordinary application path.

## Direct Payload And Transient Selective Storage

Wanted bytes use Tactical `191` unchanged: they are written directly to their
final safe metainfo paths. An incomplete wanted file is visible at that final
path, an already complete file is checked and reused, and success performs no
publish, move, tree rename, or copy.

BEP 53 makes one CLI-specific auxiliary decision necessary. For a v1 or
hybrid piece spanning a wanted and skipped file, the checker and hasher need
the skipped span in a validated part artifact. Ordinary durable products bind
that adjacent artifact to a random opaque `TorrentId`. A new stateless process
receives a new owner ID, so leaving the part file beside the payload would
strand a different random artifact after each invocation.

Add one narrow path-storage configuration seam:

```text
PathPartLocation =
  AdjacentToPayload
  | AuxiliaryDirectory(PathBuf)
```

Exact names may follow existing conventions. Ordinary desktop, headless,
Android SAF, and iOS configurations retain their current adjacent/platform
part location. `rstorrent-download` alone supplies an invocation-owned
auxiliary directory beneath its control workspace. The part filename, header,
opaque `TorrentId`, `ContentFingerprint`, geometry checks, lazy creation,
positional I/O, descriptor and storage-pool accounting, and corruption
fail-closed behavior remain unchanged. This is a path-placement seam, not a
new storage representation or a relaxation of artifact identity.

After verified wanted completion, cancellation, or any handled failure, the
CLI first shuts down and joins every application/engine/storage owner, then
removes its exact auxiliary run directory. It never deletes a part artifact
while a storage handle or checkpoint task can still reference it. Final wanted
files are retained on failure or interruption and are conservatively checked
on the next invocation.

An abrupt process or machine death may leave the exact invocation workspace.
The next invocation for that output root, after obtaining the exclusive lock,
removes only the prior CLI-owned run directory and starts clean. It never scans
the payload root for `.rstorrent-parts`, adopts a prior random owner, derives a
`TorrentId` from protocol input, or deletes an unknown/legacy artifact. If the
program is never run again, the abandoned workspace remains in the operating
system's temporary area and is eligible for normal OS temporary cleanup; it is
not mixed with user payload.

The implementation must bound the auxiliary part file by the same logical
geometry and storage limits as the existing adjacent part file. It records the
largest controlled selective part allocation and workspace residue. A new
unbounded memory part file is not an acceptable substitute.

## Cooperative Output-Root Lock

Two foreground downloader processes controlled by the same user must not write
the same output directory concurrently. The initial rule deliberately locks
the whole canonical output root rather than attempting a metadata-delayed,
per-torrent path lease:

- a small versioned control directory lives beneath the operating system's
  per-user temporary directory and is keyed by SHA-256 over a domain separator
  plus the canonical native output-root identity;
- its fixed rendezvous file is opened and `std::fs::File::try_lock()` obtains
  an exclusive, non-blocking advisory lock;
- the file handle is retained from before `ApplicationService` starts until
  application shutdown, task join, and auxiliary-workspace cleanup complete;
- lock contention exits `3` immediately with the canonical output directory
  but no other process details; there is no polling, timeout, stealing, PID
  liveness guess, stale-lock deletion, or `--force`; and
- the empty rendezvous may remain in the OS temporary control directory so
  unlock/unlink races cannot create two lock inodes. It contains no torrent,
  source, credential, path text, resume data, or payload.

Canonicalization must make ordinary relative, symlink, case, separator, and
dot-segment aliases for the same root converge on every claimed desktop OS.
The hash prevents raw path disclosure in the temporary filename; it is a
rendezvous key, not an authentication or content identity. Tests must cover
the aliases each platform supports.

The standard-library lock is cooperative. This tactical guarantees exclusion
between `rstorrent-download` invocations using the same per-user runtime and
root. Existing desktop, headless, Android, iOS, third-party clients, another OS
user, a changed temporary-runtime namespace, and arbitrary filesystem writers
do not participate. The CLI must document that the output root cannot be used
concurrently by another torrent client. Broadening the lock into a system-wide
storage lease shared by every first-party product is later architecture, not a
hidden requirement for this useful safety fence.

If native evidence shows that `File::try_lock`, canonical root identity, or
the selected temporary directory cannot provide the stated macOS/Linux/
Windows same-user guarantee, implementation must stop for direction instead
of silently falling back to a PID file or pretending existence is ownership.

## Owner, Task, Cancellation, And Data Flow

```text
rstorrent-download foreground owner
  -> strict argv/source/output validation
  -> canonical output-root lock handle
  -> exact transient-workspace guard
  -> ephemeral ApplicationService
       -> private in-memory session + metrics SQLite owners
       -> existing session network / DHT / tracker owners
       -> one metadata/content/checkpoint torrent generation
       -> bounded application view used by progress observer
  -> terminal outcome
  -> cancel and join ApplicationService owners
  -> remove exact transient run workspace
  -> release root lock and exit
```

The foreground owner retains every guard in that order. It installs Ctrl-C
and, on Unix, `SIGTERM` observation before torrent work starts. A signal,
progress-output error, broken stdout pipe, source rejection after service open,
metadata failure, storage failure, or success all converge on one idempotent
shutdown routine. A second signal may shorten presentation but must not detach
owners or bypass safe storage cancellation.

Application commands and views remain the semantic boundary:

1. validate the output root, acquire its lock, and prepare the exact auxiliary
   workspace;
2. open one ephemeral online service with one configured native path root;
3. read a local metainfo source under the existing limit or retain the magnet
   text only inside the bounded add command;
4. submit `Command::AddMagnet` or the existing
   `ApplicationService::add_torrent_bytes` request with content start enabled
   and no caller-synthesized skips;
5. retain the returned opaque torrent ID, open only the minimum summary/
   progress subscription, and observe metadata, checking, content, waiting,
   terminal failure, or wanted completion;
6. on success, require the application's synchronized committed completion
   fact before reporting; and
7. execute the joined shutdown and cleanup sequence above.

If implementation reveals that `start_content=true` can create payload before
the root lock or auxiliary location is installed, it must instead use the
existing metadata-only add followed by the normal resume command. It must not
add an engine-only start path.

Pure argument/source classification, progress formatting, exit mapping,
redaction, canonical lock-key derivation, and terminal-outcome reduction remain
runtime independent. Async networking, SQLite, filesystem handles, signals,
and task joins stay in the binary/application composition and do not leak into
protocol codecs or deterministic selection state.

## Initial Resource And Security Bounds

- Exactly one source, one torrent owner, one output root, one root lock, and
  one progress view are live per process.
- Magnet, metainfo, metadata, file-selection, application database, network,
  peer, piece, request, storage queue, descriptor, hashing, DHT, tracker, and
  diagnostic bounds remain the existing product bounds. The CLI does not raise
  them for convenience.
- Ephemeral SQLite retains the current verified 256-MiB session and 32-MiB
  metrics page maxima with memory-only temporary SQL; Tactical `081` raised
  the original Tactical `075` session limit for retained maximum-size source
  plus info bytes.
- The source reader admits no more than the existing context-specific
  `.torrent` source maximum and never follows a source that changes from the
  validated regular file during acquisition.
- Progress retains one latest state and bounded formatting buffers. TTY output
  is capped at four refreshes per second; non-TTY output at one periodic record
  per ten seconds plus state transitions.
- The temporary-control key uses existing SHA-256 code, fixed-size canonical
  input framing, and a versioned domain. No path, magnet, tracker, or torrent
  name is written into the rendezvous file.
- The auxiliary run directory has one random unguessable leaf, owner-only
  permissions where the platform exposes them, one marker/version checked
  before cleanup, and one part artifact governed by existing file-handle and
  geometry limits. Cleanup must not follow symlinks, junctions, or replacement
  objects out of the exact owned run directory.
- All metainfo and magnet input remains hostile. A display name cannot select
  a path, lock, workspace, format string, terminal escape, or log structure.
  Human output escapes control characters and bounds every peer-controlled
  field.

Implementation may tighten these bounds from evidence. Raising an existing
engine/application limit, retaining more than one torrent/view, or adding an
unbounded output/history buffer requires direction.

## Stable Scenarios

### SFD-001: Ordinary magnet, all files

A controlled multi-file magnet without `so` obtains metadata, selects every
non-padding file, writes directly to `<output>/<safe torrent name>/...`, shows
bounded progress, verifies exact content, commits completion, shuts down
without seeding, removes transient workspace, and leaves no profile files.

### SFD-002: BEP 53 deep link

A magnet with repeated, overlapping `so` ranges preserves Tactical `100`'s
canonical union before metadata. After metadata, only eligible selected files
are materialized. A v1 cross-file boundary uses the auxiliary part artifact,
not a skipped final file or adjacent random part. Exact selected bytes verify;
the transient part disappears only after joined shutdown.

### SFD-003: Empty and rejected selection

A valid padding-only `so` resolves to zero wanted files, writes no payload,
reports the no-op, and exits `0`. Out-of-range, malformed, inverted,
empty-token, or over-limit input follows the already accepted strict parser and
exits `4` with
no content. It never becomes an all-files download.

### SFD-004: Local metainfo

A bounded local v1, pure-v2, or strict hybrid `.torrent` source enters the
existing byte-intake command with all files wanted. The source is not copied to
a profile. Later mutation/removal of the source cannot change the already
authenticated raw info or output plan.

### SFD-005: Existing complete, partial, and corrupt final data

With a fresh in-memory catalog, an exact complete payload checks to success
without payload download. Partial matching pieces are retained, absent pieces
download, and a same-length corrupt span fails its hash and is repaired.
Output paths are never renamed or republished.

### SFD-006: Graceful interruption

Ctrl-C during metadata, checking, a payload write, part-file use, and the final
checkpoint reaches one cancellation path, joins every task, removes only the
exact transient run directory, retains final partial payload, releases the
lock, and exits `130`. A following invocation checks and continues safely.

### SFD-007: Abrupt death and recovery

Forced death at the same boundaries may leave direct partial payload and an
exact temporary run directory but no committed application state. The next
invocation acquires the released OS lock, deletes only that validated abandoned
workspace, checks final payload, redownloads unverifiable boundary pieces, and
completes without adopting a random part artifact or trusting stale have bits.

### SFD-008: Concurrent invocations

Two same-user processes target the same canonical directory through identical
and supported alias spellings. Exactly one obtains the non-blocking lock and
may open the service; the other exits `3` before source/network/payload work.
After the owner joins and exits, a new invocation succeeds. Two different
output roots may run independently under their separate existing session and
process resource bounds.

### SFD-009: Unsafe roots and workspace replacement

A file in place of the output directory, inaccessible parent, unsafe expected
content path, lock/control collision, symlink/junction replacement in the
transient workspace, malformed marker, or failed cleanup fails closed. The
program never broadens deletion to the output root, another run directory, or
an unknown legacy part artifact.

### SFD-010: Terminal failures and redaction

Invalid source, metadata exhaustion, tracker/DHT/peer failure, disk full,
permission loss, hash failure exhaustion, view failure, broken pipe, and
shutdown failure produce the declared exit class after proportional cleanup.
Captured stdout/stderr contains no full private magnet, tracker credential,
raw metainfo, peer bytes, terminal controls, or unbounded remote value.

### SFD-011: Native desktop behavior

The same release-built executable completes the controlled all-files case and
rejects a concurrent same-root process on native macOS, Linux, and Windows.
Each run independently verifies output, no profile files, no adjacent CLI part
artifact, joined termination, and exact temporary cleanup. No visible product
client or interactive desktop automation is required.

## Source-First Record

### Normative BEP

The required BEP checkout is exact commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`.

- `reference/bittorrent.org/beps/bep_0053.rst` defines zero-based file indices,
  comma-separated values, inclusive ranges, pre-metadata deep links, and the
  special additive behavior when an existing torrent receives another `so`.
- Tactical `100` is the accepted RSTorrent interpretation: repeated values are
  unioned into sorted coalesced ranges; malformed syntax is rejected rather
  than ignored; metadata filters padding and out-of-range indices; and a valid
  selection resolving empty remains no-payload.

This tactical adopts that exact current behavior. It does not reopen parsing,
duplicate-add, selection persistence, or generated application contracts.

### Pinned libtorrent

The required oracle is libtorrent `2.0.13` at exact pinned commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`.

Implementation and examples inspected while accepting this tactical:

- `src/magnet_uri.cpp::parse_magnet_uri` recognizes `so`, sets
  `default_dont_download`, and applies selected file priorities;
- `include/libtorrent/add_torrent_params.hpp` and
  `include/libtorrent/torrent_flags.hpp::default_dont_download` document the
  resulting default/exception policy;
- `examples/bt-get.cpp` is the smallest one-magnet, one-save-path foreground
  downloader and exits after finish/error;
- `examples/bt-get2.cpp` adds progress plus a resume file; and
- `examples/bt-get3.cpp` adds session state, periodic resume, and signal
  handling.

Tests inspected:

- `test/test_magnet.cpp` cases around `test_select_only` cover ordinary,
  overlapping, repeated, inverted, out-of-bound, malformed, empty, and quoted
  `so` values; and
- the magnet construction cases cover compact selected-range emission.

The useful oracle lesson is that a finite one-torrent executable and durable
resume/session files are embedding-policy choices, while `so` controls the
same file-priority state as a full client. RSTorrent adopts the simple finite
process and shared selection semantics, but intentionally differs by using its
strict bounded parser, `ApplicationService`, direct checker, opaque IDs,
transient CLI part location, joined owner tree, and no resume/session files.
No libtorrent source, fixture, or architecture is copied.

### JSTorrent product history

The local JSTorrent checkout was inspected read-only at exact commit
`25e4b701433fd815398ba89526546f5e4f072e3f`. It had unrelated untracked
documentation and attachments; the inspected source/test paths matched that
commit:

- `packages/engine/src/utils/magnet.ts` parses `so` into selected indices;
- `packages/engine/src/core/torrent-initializer.ts` carries that selection
  across metadata and filters it to the file catalog;
- `packages/engine/src/core/bt-engine.ts` retains pre-metadata selection and
  promotes explicit selected files on duplicate add;
- `packages/engine/test/utils/magnet.test.ts` covers ordinary and malformed
  select-only input; and
- `packages/client/src/AppContent.tsx` initializes the file-selection surface
  from a magnet selection and can explicitly choose all instead.

The adopted product lesson is that URL-embedded selection survives metadata
and initializes the actual download choice. The intentional differences are
RSTorrent's already accepted compact strict parser and this user's explicit
all-files-with-only-`so`-exception CLI: no modal, generic selection flags, or
lenient malformed-token recovery.

### SQLite, locking, and machine evidence

- Tactical `075` and the current `ApplicationConfig::ephemeral` implementation
  establish the private in-memory SQLite composition; this tactical reuses it
  without a schema or connection-policy change.
- The [Rust standard-library `File` documentation](https://doc.rust-lang.org/stable/std/fs/struct.File.html#method.try_lock)
  defines non-blocking exclusive `try_lock`, automatic release when the file
  closes, and platform-dependent advisory/mandatory enforcement. This slice
  relies only on cooperative CLI exclusion and must prove both Unix and Windows
  behavior rather than extrapolating from one host.
- Native Windows, Linux, and macOS execution uses the sibling
  `~/code/machine-control` common CLI and applicable platform guide. Exact
  private target selection remains outside this public repository.

## Implementation Stages And Gates

1. **Finite front-end shell.** Add `rstorrent-download` beside the session
   crate's existing binaries. Land strict argument/source parsing, exit
   classes, bounded/redacted progress formatting, help text, and deterministic
   tests without networking.
2. **Lock and workspace owner.** Add canonical-root keying, non-blocking
   `File::try_lock`, safe exact workspace creation/recovery/cleanup, and
   subprocess contention/crash tests. No application service starts until this
   gate passes.
3. **Auxiliary part placement.** Refactor only path-backed selective storage so
   callers may provide an auxiliary part directory while ordinary adjacent and
   platform behavior remains byte-for-byte compatible. Prove identity,
   corruption, cancellation, descriptor, pool, and cleanup invariants before
   connecting the CLI.
4. **Ephemeral application composition.** Open one bounded ephemeral service,
   add magnet or metainfo through existing commands, observe the minimal view,
   map exact terminal state, and join shutdown. Prove no profile files and no
   direct-driver bypass.
5. **Selection/recheck/crash matrix.** Close SFD-001 through SFD-010 with
   controlled all-files, `so`, cross-file part, empty selection, existing data,
   corruption, signals, forced death, disk/permission failure, and exact
   cleanup.
6. **Oracle and native platforms.** Run the controlled pinned-libtorrent roles,
   then release-built native macOS/Linux/Windows command and locking cases
   through target-native shell/file evidence from `machine-control`.
7. **Repository closure.** Run proportional Android/iOS build regressions,
   full workspace gates, reconcile topics/readiness with actual evidence,
   remove temporary downloads/logs/workspaces, and commit the completed slice.

Each stage should land as a logical commit when green. A later stage may repair
same-boundary defects exposed by its gate without approval; it must not hide an
earlier failing owner or weaken a declared assertion to proceed.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure CLI | strict argv/source grammar, help/version, exit mapping, terminal escaping, bounded progress cadence, redaction, root-key framing, completion reduction |
| Lock/workspace | same-root contention, alias convergence, different-root independence, release-after-join, stale exact-run cleanup, marker/symlink/junction refusal, no payload-root control artifact |
| Storage | ordinary adjacent behavior unchanged; auxiliary path v1 boundary create/open/hash/corrupt/cancel/remove; pure-v2 no-part behavior; pool/descriptor limits; no skipped final file or adjacent CLI part |
| Application | private session/metrics SQLite, no profile files, normal Add commands, all-files default, compact `so`, empty selection, view recovery, durable terminal completion, no seeding, joined shutdown |
| Existing/crash | complete/partial/corrupt direct data, graceful cancellation at every owner boundary, forced death before/during/after checkpoint, conservative recheck, exact transient recovery |
| Controlled interop | independently generated v1 single/multi-file and v2/hybrid sources; pinned libtorrent seed with ordinary and repeated/overlapping `so`; exact selected hashes and terminal zero ownership |
| Native desktop | release-built macOS, Linux, and Windows all-files completion plus same-root contention and cleanup through `machine-control`; no visible UI |
| Platform regression | Android dual-ABI/application build and maintained iOS simulator/archive compile prove the path-only configuration seam does not change SAF/iOS behavior; no CLI mobile capability claim |
| Repository | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, focused interop, generated-contract cleanliness if touched, exact artifact cleanup, and scoped diff review |

A public-swarm run is optional context, not a stopping gate. Controlled
libtorrent and native filesystem evidence are sufficient for this finite
composition; no public reliability or performance claim follows.

## Explicit Non-Goals

- Durable resume files, profile databases, session restore, remembered
  settings, retained queue, DHT warm start, post-completion seeding, ratio/time
  goals, or daemonization.
- Multiple sources, parallel torrent queueing, recursive input, watch folders,
  RSS, search, plugins, categories, labels, scheduling, bandwidth CLI options,
  tracker editing, or remote control.
- CLI file indices, globs, priorities, `--skip-files`, sequential mode, an Add
  dialog, interactive prompts, or overriding/removing a magnet's `so`.
- HTTP(S) `.torrent` download, arbitrary URL handlers, browser/extension
  activation, clipboard access, stdin metainfo, or shell execution.
- A stable JSON protocol, RPC server, REST/WebSocket gateway, native host,
  companion daemon, libtorrent dependency, alternate engine, or public third-
  party compatibility promise.
- Hidden full-payload staging, publication, move-on-completion, suffix files,
  packed storage, preallocation policy, RAM-backed payload, or deterministic
  protocol-derived `TorrentId`.
- A universal lock shared with desktop/headless/mobile/other-user/third-party
  writers. The documented same-user CLI cooperative lock is the complete
  initial concurrency claim.
- Android or iOS command-line product binaries, mobile background execution,
  SAF/document-picker CLI syntax, visible client changes, installers, shell
  completion, package-manager distribution, signing, release, tag, or push.
- Importing or deleting adjacent random part files from prior durable profiles,
  schema migration, compatibility aliases, or general orphan cleanup.

## Escalation Contract

Implementation may autonomously choose internal names and module splits,
refactor the path builder and application runner at the declared boundaries,
tighten limits, add hostile cases implied by the scenarios, repair same-owner
bugs, use existing dependencies, update build scripts/docs/topics, run
controlled loopback and target-native machine evidence, and commit logical
stages.

Stop for direction if evidence requires:

- weakening or changing current BEP 53 semantics, all-files default, direct
  final paths, completion definition, or no-seeding outcome;
- durable torrent/session/resume state or a protocol-derived application ID;
- a lock claim broader than same-user CLI cooperation, a blocking/stealing
  lock policy, or payload-root control artifacts;
- a different part-file representation/header or automatic deletion/adoption
  of unknown payload-root artifacts;
- a new runtime dependency with meaningful tradeoffs, schema/generated public
  contract change, external network service, visible/mobile product work,
  destructive user-data action, publication, or release; or
- native evidence disproving the stated cross-platform lock/workspace safety.

Ordinary compile/test failures, conservative rechecking, platform-specific
path spelling, safe internal refactoring, or a controlled swarm timeout are not
escalations.

## Stopping Condition And Next Boundary

Tactical `193` is complete only when a clean release-built
`rstorrent-download` accepts one magnet or local `.torrent`, uses the existing
ephemeral application service and in-memory SQLite stores, downloads the exact
all-files-or-BEP-53 selection directly to the requested final paths, safely
serializes same-user invocations per canonical output root, cleans transient
selective storage after joined termination or the next post-crash run, exits
with the declared outcome, and passes every required evidence row above.

The next slice, if user evidence justifies one, may consider packaging, a
machine-readable output mode, wider cross-product storage leasing, controlled
extra options, or durable resume. Completion of this tactical implies none of
them.
