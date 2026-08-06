# Tactical 100: BEP 53 Select-Only And Duplicate Add Feedback

Status: Complete on 2026-08-06. The maintainer selected this as the next
bounded product slice and authorized end-to-end execution with incremental
commits.

Topics: `protocol-support`, `client-persistence`, `application-control`,
`application-view-api`, `table-interaction`, `web-ui-design`,
`capability-readiness`

Dependencies: completed Tacticals
[`006`](006-magnet-metadata-peer-hint.md),
[`007`](007-durable-session-control.md),
[`019`](019-torrent-owned-metadata-acquisition.md),
[`037`](037-live-magnet-toolbar-intake.md),
[`063`](063-live-file-selection.md),
[`069`](069-current-within-table-selection.md),
[`071`](071-copy-magnet-link.md), and
[`083`](083-shared-torrent-file-picker.md) establish bounded magnet and
metainfo intake, durable selection, metadata acquisition, current-row
selection, live priority changes, canonical magnet copy, and the shared Add
flow this slice extends.

## Decision And Motivation

Adding a torrent that is already in the session is not a second import and is
not a request to merge every field supplied by the new source. A plain
duplicate magnet or `.torrent` file succeeds as an idempotent no-op, identifies
the existing torrent, makes it the current singleton row, reveals it in the
Library, and reports `Already in your session`. A newly added torrent receives
the same focus and reveal treatment with an `Added` result. Neither case asks
for confirmation.

BEP 53's `so` parameter is one deliberate exception. It is an explicit user
request to download named file indices. A new metadata-less magnet retains
that selection until its file catalog is verified. When the same torrent is
already present, valid selected indices that are currently skipped become
wanted. This operation is additive: it never demotes a wanted file and never
changes the torrent's root, lifecycle intent, trackers, source, metadata, or
other add options.

The selection representation must change with the feature. Today's durable
shape assumes every file is wanted and stores sparse skipped rows. A select-
only magnet has the inverse shape. Expanding its complement at the accepted
374,998-file limit would create pathological database, snapshot, and runtime
state. Represent file selection as one explicit default plus bounded
exceptions, and preserve compact pre-metadata ranges until a verified catalog
exists.

This tactical adopts libtorrent as the required behavior and edge-case oracle,
not as an architecture template. Libtorrent returns an existing handle for a
duplicate by default and does not merge general add parameters. Its magnet
parser recognizes `so` before metadata and represents the inverse default
compactly, but its duplicate-add path does not itself apply BEP 53's additive
rule. RSTorrent applies that one rule transactionally in its application
owner.

## Stopping Condition

This tactical is complete when all of the following hold:

1. the bounded magnet parser accepts one or more BEP 53 `so` parameters made
   from zero-based unsigned decimal indices and inclusive `start-end` ranges,
   unions them, and produces a canonical sorted/coalesced range set without
   expanding ranges;
2. a present but empty, malformed, inverted, overflowing, out-of-product-
   bound, or over-budget `so` value rejects the add before any durable state
   changes instead of silently reverting to all files;
3. a new `so` magnet persists a compact `only` intent before metadata, resumes
   it across reopen, and resolves it against the verified file catalog without
   briefly preparing or requesting unselected payload;
4. catalog indices outside the verified file count and padding-file indices
   are ignored with bounded diagnostics; if no selected payload index remains,
   the result is an intentional no-payload selection rather than all files;
5. durable and runtime selection use a default-wanted/default-skipped value
   plus bounded opposite exceptions, with a transactional migration from the
   existing implicit-wanted plus skipped-row representation;
6. a plain duplicate magnet or `.torrent` add returns success as a no-op and
   retains the existing source, root, lifecycle intent, trackers, metadata,
   and file selection without advancing the durable revision or the
   `torrents_added` counter;
7. a duplicate magnet carrying valid `so` selection atomically promotes only
   newly selected skipped payload files, advances the revision exactly once
   only when selection changes, and never auto-starts, restores, or relocates
   the torrent;
8. a duplicate `.torrent` over a metadata-less magnet remains a no-op; verified
   metadata injection and general add-source augmentation remain separate
   future decisions;
9. exact request replay returns its stored semantic add result without
   reapplying selection, while a new request whose `so` adds nothing reports
   the existing torrent without a mutation;
10. a changed selection on an active torrent crosses the existing safe file-
    priority fence: persist intent, cancel and join the affected owner, recheck
    and replan, then restart only when the preserved lifecycle intent requires
    it; paused and archived torrents never resume implicitly;
11. magnet and metainfo add commands return a typed result identifying the
    torrent and distinguishing `added`, `already_present`, and
    `selection_expanded`; adapters and generated contracts preserve that
    meaning without parsing messages or diffing snapshots;
12. after any successful add result, the shared React product surface makes
    the target the singleton current selection, changes to the least
    disruptive category that can show it when necessary, scrolls it into view,
    and presents a bounded polite status message without stealing DOM focus
    from the Add field or opening torrent detail;
13. deterministic parser, selection-transition, migration, replay, runtime,
    generated-contract, and React interaction gates pass, followed by
    controlled libtorrent interoperability and maximum-geometry resource
    evidence; and
14. the protocol matrix remains `Unsupported` until those gates are recorded,
    then advances only to the exact evidence-backed BEP 53 claim.

This proves bounded select-only intake, its specified additive duplicate
behavior, and consistent product feedback. It does not create a general
duplicate merge policy or redesign Add into a confirmation workflow.

## Exact Product Contract

### Add outcome matrix

| Input and session state | Durable effect | Result | Presentation |
| --- | --- | --- | --- |
| New magnet without `so` | Add with all payload files wanted after metadata. | `added` | Select, reveal, and report added. |
| New magnet with `so` | Add with compact select-only intent. | `added` | Select, reveal, and report added. |
| New `.torrent` | Add using the existing file-selection intent. | `added` | Select, reveal, and report added. |
| Existing torrent, plain magnet | None. | `already_present` | Select, reveal, and report already present. |
| Existing torrent, duplicate `.torrent` | None. | `already_present` | Select, reveal, and report already present. |
| Existing torrent, `so` adds wanted files | Promote the valid union only. | `selection_expanded` | Select, reveal, and report the added-file count when metadata makes it knowable. |
| Existing torrent, `so` adds nothing | None. | `already_present` | Select, reveal, and report already present. |

Root, start, pause, archive, tracker, source, metadata, and ordinary selection
values arriving with a duplicate are ignored. The existing torrent wins.
There is no merge prompt because no ambiguous mutation occurs. BEP 53
selection is not ambiguous: the magnet explicitly asks to make those indices
wanted, and the operation is monotonic.

Removal in progress rejects every duplicate add rather than resurrecting or
mutating the disappearing torrent. A plain duplicate may identify an otherwise
stable errored torrent. A selection-changing duplicate fails atomically while
selection repair, publication, removal, or another state transition makes a
safe priority fence unavailable. It must never partially persist exceptions
or report success before ownership is reconciled.

### Typed semantic result

Both add paths return one semantic result through the application waist:

```text
AddTorrentResult
  torrent_id
  disposition
    added
    already_present
    selection_expanded { newly_wanted_count? }
  resulting_revision
```

Names may follow existing Rust and generated-contract conventions, but the
meaning is fixed. A no-op carries the unchanged durable revision. A real add
or selection expansion carries the newly committed revision. The ordinary
view API remains the state-recovery authority; a full snapshot is not the
substitute for identifying the command target or outcome.

`newly_wanted_count` is present only when verified metadata makes the exact
valid non-padding file delta knowable. A pre-metadata range union uses the same
`selection_expanded` disposition with no count; source-range cardinality must
not be mislabeled as a valid file count.

The durable request receipt records this result. Exact replay returns the
recorded disposition and count. It does not rerun duplicate lookup, re-promote
files, move UI focus at the service layer, or increment counters. UI handling
of a replayed successful result is allowed to reveal the identified torrent
again because that is local presentation, not durable mutation.

### Shared React focus and feedback

`currentTorrentId` and `selectedTorrentIds` change together to the result's
single `torrent_id`. The target must be visible before virtual-table reveal:

- retain the active category if it already includes the torrent;
- otherwise choose `archived` for an archived torrent and `all` for any other
  torrent;
- retain the current Library/Transfers/Workbench destination and detail
  layout; and
- scroll the logical row into view after the filtered model contains it.

This is programmatic current-row selection, not keyboard focus transfer. The
Add input or dialog trigger keeps DOM focus so repeated adds remain efficient.
Do not open the detail pane automatically. Report the result through the
existing polite live-status surface with bounded translated copy. A selection
expansion may say that N additional files were selected when the typed result
contains the exact count, and otherwise says only that file selection was
expanded. It must not include the magnet, tracker URLs, paths, or raw parser
error input.

The contract applies to the shared React browser/Tauri product surface.
Generated Kotlin types must compile, but Android Compose focus, scrolling, and
snackbar parity are not part of this tactical.

## BEP 53 Parsing And Canonicalization

`so` values consist only of comma-separated ASCII decimal indices or one
inclusive decimal range per item. Whitespace, signs, empty items, multiple
hyphens, non-ASCII digits, trailing commas, and partial numeric parses are
invalid. `0`, `7`, and `6-8` are valid. `8-6`, `-1`, `1-`, and an empty `so`
are invalid.

Apply that grammar after the existing bounded query percent-decoding step, so
encoded comma or hyphen separators have the same meaning as literal ones and
malformed escapes retain the parser's existing rejection behavior. Canonical
output uses literal ASCII digits, commas, and hyphens.

Each parsed endpoint must be less than `MAX_FILE_INDEX`, currently 374,998.
Across repeated parameters, accept at most 4,096 canonical disjoint ranges.
Retain the existing 16-KiB URI and 128-parameter ceilings. Parsing, validation,
union, sorting, and coalescing must be proportional to source tokens and range
count, not to the numeric width of a range. A source such as `0-374997` never
allocates 374,998 indices before metadata.

Repeated `so` parameters union. Overlapping and adjacent ranges coalesce.
Canonical operational magnet text emits one `so` value in ascending minimal
inclusive-range form. Exact original source text remains in the existing
source record; canonicalization must not destroy provenance. A duplicate
`so` add changes selection state but does not replace the original source or
append trackers.

The provisional add-magnet `skip_files` field remains a separate explicit
caller intent for compatibility in this slice. Product clients continue to
send it empty. Supplying both nonempty `skip_files` and `so` is rejected before
mutation instead of inventing precedence. A later API cleanup may replace
that field with the common compact selection type after durable receipt
compatibility is designed.

Ordinary **Copy magnet link** remains the hash-only product action established
by Tactical `071`. It does not silently serialize current file selection.
An explicit future **Copy magnet for selected files** action may use BEP 53,
but is not implied here.

## Selection State And Persistence

### Semantic state

Before verified metadata, magnet selection is exactly one of:

```text
all
only(canonical source ranges)
```

The duplicate transition is deterministic:

```text
all + only(B)       -> all
only(A) + only(B)   -> only(canonical_union(A, B))
```

`all` already wants every future payload file, so a duplicate `so` cannot
expand it. `only` survives restart and metadata retries. Once verified
metadata arrives, resolve source indices against the immutable catalog,
discard out-of-catalog and padding indices, and persist the resulting payload
selection. An empty valid result remains default-skipped with no wanted
exceptions.

After metadata, selection has this compact form:

```text
default: wanted | skipped
exceptions: bounded file indices whose value is the opposite of default
```

Existing torrents migrate to `default=wanted` with their current skipped rows
as exceptions. A select-only torrent normally resolves to `default=skipped`
with wanted exceptions. Normalize equivalent states deterministically; do not
flip defaults merely to save a few rows unless the transaction also preserves
request replay, view order, and runtime meaning exactly.

The schema must encode the exception value or make it unambiguous from the
parent default. Foreign keys, uniqueness, file-index validation, transaction
rollback, reopen, and migration interruption remain exact. Request receipts
survive the migration. A newer schema never expands a default-skipped torrent
into one row per skipped file.

### Runtime and view shape

Planner, storage, verification, file-priority, completion, and file-view code
consume the semantic default plus exceptions. They may materialize a bounded
per-file priority array only where an existing metadata-sized runtime
algorithm genuinely requires it; no durable command response or paged view
may acquire an unconditional all-file vector as a side effect of this feature.

The Files view derives each row's wanted state from the default and exception
lookup. Existing SetFilePriority semantics remain exact: setting one file to
its default deletes an exception; setting it to the opposite inserts one.
Batch validation completes before mutation. Padding files remain
nonselectable. Snapshot evolution must not expose a 374,998-entry complement;
use the compact selection summary and paged file rows.

## Duplicate Transaction And Runtime Fence

Duplicate identity is the canonical v1 info hash already owned by the session
store. Parsing and validation finish before duplicate policy runs. The store
then performs one serialized add transaction:

1. no matching row: persist the new torrent, source, trackers, and initial
   compact selection intent, then return `added`;
2. matching row without an effective `so` expansion: change no torrent row,
   receipt-visible counter, source, tracker, or revision, then return
   `already_present`; or
3. matching row with newly wanted valid files: validate the complete union and
   transition eligibility, persist the new compact selection and receipt at
   one revision, then return `selection_expanded` with the exact file delta
   when verified metadata makes it knowable.

When metadata is absent, case 3 unions pending source ranges transactionally.
When metadata is present, it promotes only currently skipped non-padding
indices. Invalid catalog indices contribute no mutation and only bounded
counts. The transaction does not create files or start tasks.

The application service owns the runtime fence after a committed expansion.
For an active torrent it uses Tactical `063`'s generation-fenced cancellation,
join, recheck, plan rebuild, and conditional restart. For paused or archived
intent it updates durable/runtime selection without starting transfer. A
failure before commit leaves both state and owner unchanged. A post-commit
runtime failure is reported through the existing durable-intent/runtime-error
model and remains recoverable on restart; it must not roll the selection back
behind the committed receipt.

## Ownership, Tasks, And Dependency Direction

```text
bounded magnet codec -> canonical select-only ranges
                    -> session add transaction
                         new: persist pending/default selection
                         duplicate: no-op or additive promotion
                    -> typed AddTorrentResult
                    -> application owner runtime fence when needed
                    -> adapters and generated contracts
                    -> React current/selection/reveal/status

verified metadata -> resolve pending ranges against file catalog
                  -> compact default plus exceptions
                  -> planner/storage and paged file projection
```

- Protocol code owns syntax, canonical ranges, and deterministic unions. It
  has no SQLite, async-runtime, filesystem, task, or UI dependency.
- Session persistence owns identity lookup, source provenance, selection
  authority, revision, counters, receipts, and transactional migration.
- The application service owns active torrent tasks and the only
  cancellation/join/restart sequence. This tactical adds no background task,
  channel, daemon, or alternate mutation path.
- Adapters preserve the semantic result and error shape. They do not infer a
  duplicate from text or compare snapshots.
- React owns presentation selection, category reveal, virtual scrolling, live
  status, and DOM-focus retention. It cannot change engine state by focusing a
  row.

Dependency direction remains protocol and deterministic selection state
inward, persistence and runtime ownership around it, then transport and
presentation outward.

## Resource, Integrity, And Privacy Invariants

- Preserve the 16-KiB magnet, 128 query-parameter, and 374,998-file-index
  limits. Accept at most 4,096 canonical `so` ranges after union/coalescing.
- Never iterate over every integer in a source range before verified metadata.
  Parse and persist ranges compactly.
- Bound durable exception rows by verified payload-file count and the existing
  file-selection command budget. Reject rather than partially apply an
  over-budget transition.
- Record maximum-geometry source bytes, token/range count, database rows and
  bytes, peak selection memory, transaction time, projection size, and active
  owner restart time before completion.
- Treat magnet text and metainfo as hostile. Parser failure is atomic and
  cannot fall back to downloading all files.
- Preserve verified metadata, piece hashes, path validation, boundary-piece
  storage, and publication rules. Selecting a file never verifies content.
- Do not log or surface raw magnet text, tracker URLs, credentials, file paths,
  or unbounded invalid tokens. Diagnostics use stable reason codes and bounded
  counts.
- Keep one session/application authority and existing command serialization.
  No preflight UI parser or client-side duplicate cache becomes authoritative.

## Reference Dossier

### Normative source

Inspected the repository-pinned BEP source at
`reference/bittorrent.org/beps/bep_0053.rst`. It defines `so` as zero-based
comma-separated indices and inclusive ranges, permits deep links before
metadata is available, and requires adding currently non-downloading selected
indices when the torrent already exists. The BEP is draft; this tactical does
not claim support before its evidence gates pass.

### Pinned libtorrent oracle

Inspected libtorrent revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` (v2.0.13) from
`reference/pins.toml`, including:

- `src/magnet_uri.cpp`, `parse_magnet_uri()` around lines 432-486: parses `so`,
  sets `default_dont_download`, accumulates sparse priorities, and bounds its
  accepted index;
- `test/test_magnet.cpp` around lines 600-683: ordinary, overlapping,
  repeated, inverted, bounded, malformed, and empty `so` cases;
- `include/libtorrent/add_torrent_params.hpp` around lines 188-197 and
  `include/libtorrent/torrent_flags.hpp` around lines 290-294: file-priority
  defaults and pre-metadata default-dont-download intent;
- `test/test_priority.cpp` around lines 422-470 and `src/torrent.cpp` around
  lines 1845-1880 and 5680-5790: deferred priorities and metadata arrival;
  and
- `src/session_impl.cpp` around lines 5088-5097 plus
  `test/test_session.cpp` around lines 151-230: duplicate error or existing-
  handle return without general add-parameter merging.

Adopt the compact inverse default, pre-metadata intent, and no-general-merge
lessons. Intentionally differ by rejecting malformed `so` instead of silently
skipping tokens, using RSTorrent's established bounds, and applying the BEP's
duplicate selection rule explicitly in the application transaction.

### JSTorrent product history

Inspected sibling JSTorrent commit
`9895410beeed6aff554053769bd006a3fbd373ef`, including:

- `packages/engine/src/utils/magnet.ts` and
  `packages/engine/test/utils/magnet.test.ts`: parsing, repeated-value union,
  and malformed-token behavior;
- `packages/engine/src/core/torrent-factory.ts`,
  `packages/engine/src/core/torrent-initializer.ts`, and
  `packages/engine/src/core/bt-engine.ts`: pending selection, metadata-time
  filtering, and duplicate promotion;
- `packages/engine/test/core/awaiting-file-selection.test.ts`: duplicate
  metainfo, duplicate `so`, stopped-torrent, pre-metadata, and out-of-bounds
  cases;
- `packages/engine/src/core/session-persistence.ts`: persistence of pending
  select-only intent;
- `packages/client/src/AppContent.tsx`: duplicate notification and add-count
  behavior; and
- Android `EngineServiceRepository.kt`, `TorrentListViewModel.kt`, and
  `TorrentListScreen.kt`: carrying duplicate identity into highlight,
  scrolling, and snackbar feedback.

Adopt the product lesson that duplicate identity must reach presentation and
that selected indices promote skipped files. Intentionally do not copy its
unbounded range expansion, lenient token handling, automatic start of a
stopped duplicate, or incomplete pre-metadata duplicate union. No source,
fixture, or test data is imported.

## Validation Gates

### Deterministic parser and state

- Cover absent, one, repeated, overlapping, adjacent, duplicate, singleton,
  percent-encoded separator, maximum-index, and maximum-range-budget `so`
  inputs.
- Cover empty values/items, whitespace, signs, non-ASCII digits, bad hyphens,
  inversion, overflow, index-bound, range-budget, URI-size, and parameter-
  count rejection with zero mutation.
- Prove canonical output and parse/canonicalize/parse stability without range
  expansion.
- Cover every `all`/`only` duplicate union, post-metadata promotion, padding,
  out-of-catalog, no-valid-index, all-already-wanted, and exact delta case.

### Persistence and command semantics

- Migrate fresh, existing all-wanted, sparse-skipped, empty-selection, maximum-
  geometry, old-receipt, interrupted-migration, and unsupported-newer-schema
  databases.
- Reopen a metadata-less select-only torrent, complete metadata, and prove the
  exact default/exceptions state before any payload scheduling.
- Prove add, no-op duplicate, selection expansion, request replay, request-ID
  conflict, stale revision, rollback, counter, source, tracker, root,
  lifecycle, and removal-race semantics for magnet and `.torrent` paths.
- Prove no command response or snapshot expands the skipped complement.

### Runtime and interoperability

- Use a deterministic multi-file fixture with selected, unselected, padding,
  and shared-boundary pieces. Verify only selected payload is planned and
  published while boundary bytes follow existing part-storage rules.
- Add `so` before metadata, restart during metadata acquisition, accept
  verified metadata, download, reopen, and seed the selected content.
- Expand selection on active, paused, archived, errored, and metadata-less
  torrents. Observe exact cancellation/join/replan ownership and prove no
  implicit start or restore.
- Run controlled RSTorrent/libtorrent metadata and payload interoperability in
  both useful directions, including a duplicate promotion followed by exact
  hash verification.
- Record maximum-geometry time, memory, database, and response high-water
  marks and terminal task cleanup. Public swarms and visible product clients
  are not routine gates.

### Product and adapters

- Regenerate and validate Rust, TypeScript, JSON Schema, Tauri, WebSocket, and
  Kotlin contracts for every disposition and revision outcome.
- In headless React tests, cover fresh and duplicate magnet and `.torrent`
  adds, `so` expansion, archived/category reveal, sorted and virtualized row
  scrolling, singleton current/selection, status text, repeated adds, DOM-
  focus retention, and screen-reader live announcement.
- Prove snapshot refresh cannot override the explicit command-result target
  and that a missing/deleted result target fails safely without selecting an
  unrelated row.
- Run the proportional workspace format, clippy, test, web typecheck/test/build,
  adapter compile, and Android cross-build baselines. Do not launch Tauri, an
  AVD, or a physical ChromeOS session merely for this planned slice.

## Implementation And Evidence

The protocol magnet codec now retains `so` as sorted, coalesced inclusive
ranges. Parsing is strict after percent decoding, unions repeated parameters,
accepts indices only through `374,997`, and remains inside the existing
16-KiB URI and 128-parameter bounds. The accepted representation never
enumerates a range; a maximum-span `0-374997` value remains one eight-byte
`FileIndexRange` value.

Session schema 13 records an explicit wanted-or-skipped selection default,
sparse opposite exceptions, and compact pending pre-metadata ranges. Its
transactional migration preserves prior wanted-by-default rows. Metadata
acceptance filters out-of-catalog and padding indices before committing,
retains an intentional all-skipped result when nothing valid remains, and
rejects a catalog that would require more than 4,096 exceptions before
writing metadata or selection state. The 4,097-file rejection fixture passes
in 0.01 seconds and leaves the metadata and exception tables unchanged.

Magnet and metainfo adds now return one generated `AddTorrentResult` through
the Rust, JSON Schema, TypeScript, WebSocket, Tauri, and UniFFI/Kotlin
adapters. Plain duplicates are revision-stable successful no-ops. An explicit
duplicate `so` union is the only add-time merge: it promotes skipped files,
uses the existing active-owner cancellation/recheck fence, and preserves
paused or archived intent. Exact receipt replay returns the stored result.

The shared React controller consumes that result directly. It makes the
target the singleton current selection, reveals archived targets or retains a
visible current category, scrolls the logical virtual row without moving DOM
focus, leaves detail closed, and announces Added, Already present, or the
exact selection-expansion count through the existing polite status surface.
A pending reveal target prevents an older list snapshot from replacing this
explicit command result.

Evidence recorded on 2026-08-06:

- the complete pinned libtorrent `test_magnet` executable passed, including
  its BEP 53 select-only, overlap, repetition, inversion, bounds, malformed,
  and magnet round-trip cases;
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, and
  `cargo test --workspace` passed; the workspace run included 88 protocol,
  348 engine, 170 session, 20 gateway, desktop, Android bridge, platform, and
  binary tests, with only explicitly ignored opt-in/maximum-resource tests;
- the focused post-cleanup session/gateway rerun passed 170 session and 20
  gateway tests, including compact selection, schema reopen, duplicate union,
  atomic exception-budget rejection, typed replay, runtime fences, and the
  existing hash-verified application/libtorrent transfer paths;
- `npm run generate`, `npm run typecheck`, `npm test -- --run`, and
  `npm run build` passed in `clients/web`: 201 tests passed, two remained
  explicitly skipped, generated artifacts were stable, CSP inspection passed,
  and the existing large-bundle warning remained informational; and
- `experiments/android-engine-bootstrap/build.sh` passed the x86_64 and arm64
  release Rust cross-builds, regenerated both Kotlin bindings, and passed
  `assembleDebug` plus `testDebugUnitTest`.

There is no new peer-wire extension in BEP 53. The interoperability claim is
therefore limited to matching the pinned oracle's URI behavior and composing
the resulting durable file plan with the already controlled metadata and
hash-verified payload paths; it does not claim that another peer observes an
`so` value on the wire.

## Explicit Non-Goals

- General merging of trackers, sources, metadata, roots, start intent,
  archive state, or arbitrary file selection from a duplicate add.
- A duplicate confirmation dialog, alternate Add dialog, preflight duplicate
  API, or client-side identity authority.
- Metadata injection from a duplicate `.torrent` into a metadata-less magnet.
- Removing files from selection through `so`; live deselection remains the
  explicit Files action.
- Encoding current selection in ordinary Copy magnet, or adding a selected-
  files copy action.
- BEP 52/v2 `btmh`, hybrid identity, name-based selection, nested glob syntax,
  piece priority, playback policy, or sequential mode.
- Android Compose focus/snackbar parity, extension integration, remote daemon,
  native host, or new IPC transport.
- Public-swarm reliability claims or unrelated product UI implementation.

## Escalation Boundary

Stop for maintainer direction if implementation would broaden duplicate adds
beyond BEP 53's monotonic file promotion, change the accepted strict parsing
policy, introduce a second selection authority, auto-start or restore a
duplicate, expose selection in ordinary copied magnets, add a dependency with
meaningful tradeoffs, or require a new background owner or product surface.
