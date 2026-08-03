# Typed Peer Flags And Legend

Status: Complete on 2026-08-02.

Topics: `peer-flag-vocabulary`, `peer-lifecycle`, `application-view-api`,
`web-ui-design`

## Motivation

The Peers table currently renders an opaque `flags: string` assembled by the
React live adapter. Its three live letters conflict with libtorrent and
JSTorrent meanings, while deterministic demos emit a different historical
string. Users cannot discover a trustworthy legend, incoming direction is
omitted despite already being present in the application view, and future
engine-owned states have no stable cross-client semantic vocabulary.

This slice replaces the accidental string with a typed semantic set computed
by Rust, normalizes demos and live rows through one frontend definition table,
and adds an accessible column-header legend. It emits only flags supported by
the current coherent peer observation while reserving reference-justified
semantic values for later engine features.

## Stable Scenarios

- An outgoing TCP content peer that supports extensions and is not choking
  RSTorrent displays `Dx`, not the ambiguous current `EI`.
- The same peer displays `dx` while it chokes RSTorrent.
- A representable incoming uTP peer displays `IT` without confusing incoming
  direction with discovery source.
- Upload and metadata flags appear only when their nullable facts are known;
  current unsupported values produce no fabricated characters.
- Demo scenarios and live rows use the same semantic values, characters,
  ordering, sorting, and accessible expansion.
- A visible help control in the Flags header opens the complete grouped legend
  by keyboard, pointer, or touch without changing sort order.
- Escape and outside activation dismiss the legend, and every flag cell has an
  accessible full-text meaning.
- Existing v1 peer rows without the additive flag field remain accepted and
  receive a bounded compatibility derivation from their existing typed facts.

## Scope

- Add the reference-backed `peer-flag-vocabulary` living topic.
- Add a closed `PeerFlagView` semantic enum and optional/defaulted bounded flag
  list to the Rust `PeerView` application projection.
- Compute current `incoming`, `download_allowed`/`download_choked`,
  `upload_allowed`/`upload_choked`, `extension_protocol`,
  `metadata_extension`, and `utp` values from authoritative row facts.
- Reserve the accepted mature semantic enum values for encryption,
  hole-punching, parole, optimistic unchoke, snubbed, upload-only, endgame, and
  seed without emitting them from missing engine state.
- Regenerate TypeScript, JSON Schema, and deterministic trace fixtures and
  retain additive v1 compatibility.
- Replace the frontend string with typed flags and one exhaustive definition
  table for glyphs, order, group, and short label.
- Normalize every demo scenario to the typed vocabulary.
- Extend the generic table header with optional accessible help content and use
  it for a grouped Peer flags legend.
- Add Rust projection, runtime validation, adapter, component, accessibility,
  responsive, and visual evidence.
- Update the owning topics, tactical index, and reference record.

## Non-goals

- Implement incoming listening, uTP networking, payload upload, encryption,
  hole punching, full parole selection, optimistic unchoke, seeding,
  upload-only negotiation, or new endgame policy.
- Pretend `stalled` is libtorrent's snubbed state or infer seed from incomplete
  availability.
- Add per-peer disk/rate/network blocking state.
- Redesign or complete the Source column's multiple-provenance presentation.
- Add the Swarm peer-record view or disconnected history.
- Adopt libtorrent's bit positions, terminal colors, exact combined layout, or
  JSTorrent's mutable UI/engine coupling.
- Add a tooltip/popover dependency, icon dependency, or table framework.
- Commit, push, publish, or launch a visible product client without separate
  authorization.

## Reference Dossier

The complete survey and deliberate differences live in
`docs/topics/peer-flag-vocabulary.md`.

Pinned Rasterbar libtorrent revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/peer_info.hpp::peer_info` and its peer/source/bandwidth
  flag definitions;
- `src/peer_connection.cpp::peer_connection::get_peer_info`;
- `src/bt_peer_connection.cpp::get_specific_peer_info`;
- `examples/client_test.cpp::print_peer_info` and `print_peer_legend`;
- `test/test_peer_list.cpp` incoming/source cases; and
- `test/test_fast_extension.cpp` seed flag cases.

Pinned JSTorrent revision `9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/ui/src/tables/PeerTable.tsx::formatFlags`; and
- `packages/engine/src/core/peer-coordinator/types.ts` incoming direction.

No reference code, test data, fixture, or asset is copied.

## Accepted Design

Rust owns a `PeerFlagView` enum with stable semantic names. `PeerView` carries
an additive `peer_flags` array that defaults to empty and is omitted when
empty, keeping older v1 producers structurally acceptable. New producers emit
each present semantic flag once in canonical order.

The initial catalog is:

```text
incoming, encrypted,
download_allowed, download_choked,
upload_allowed, upload_choked,
extension_protocol, metadata_extension, utp, hole_punched,
on_parole, optimistic_unchoke, snubbed, upload_only, endgame, seed
```

The engine observation does not gain speculative booleans. The Rust
application mapper derives only values justified by fields it already maps:

```text
direction == incoming                       -> incoming
transport == utp                            -> utp
supports_extensions == true                 -> extension_protocol
supports_ut_metadata == true                -> metadata_extension
local_interested == true + remote choke     -> download_allowed/choked
remote_interested == true + local choke     -> upload_allowed/choked
```

The remaining variants are accepted vocabulary but cannot appear until a
later engine tactical installs their actual owners.

The frontend maps the semantic enum to provisional glyphs `I`, `E`, `D`/`d`,
`U`/`u`, `x`, `m`, `T`, `h`, `p`, `O`, `S`, `L`, `e`, and `s`. The mapping is
one exhaustive typed data table shared by cell formatting, accessible labels,
sort keys, demo values, and legend rows. No glyph is parsed back into state.

The header help trigger is a distinct button beside the sortable Flags label.
It opens a nonmodal, viewport-bounded popover and does not nest one button in
another. It exposes `aria-haspopup`, expanded state, an owned labeled dialog,
Escape/outside dismissal, and stable trigger focus. The legend contains static
content and does not trap focus. The interaction remains available on touch
and does not depend on hover.

## Compatibility

Adding a required field would violate the accepted v1 compatibility policy.
The Rust field therefore uses Serde default plus omission when empty; generated
TypeScript and schema treat it as optional. The semantic validator bounds the
array, rejects duplicates, and accepts only the closed catalog.

The live adapter prefers Rust `peer_flags` when present. When an older v1 row
omits it, the adapter derives only the current initial subset from the existing
typed direction, transport, interest/choke, and capability fields. This is a
compatibility bridge, not a competing long-term owner.

Fixtures regenerate from Rust. Existing handwritten old-producer fixtures
that omit the field remain intentional compatibility cases rather than being
mechanically given the new property.

## Invariants And Bounds

- Rust semantic state flows outward; React owns glyph presentation only.
- `peer_flags` contains at most the 16 catalog values, no duplicates, in
  canonical order from new Rust producers.
- A transfer pair emits at most one of its allowed/choked values.
- Unknown choke or interest state emits neither member of that pair.
- Incoming derives from `direction`, not the `incoming` discovery source.
- uTP derives from `transport`, not a client fingerprint.
- Empty flags mean no currently known present flag, not universal observed
  falsehood.
- Lifecycle state remains in State; discovery provenance remains in Source.
- Every rendered glyph sequence has a full accessible label.
- The help control cannot sort or resize the column and the sort control cannot
  open help.
- One table may have at most one header-help popover open. It adds no recurring
  task, timer, global retained history, or dependency.
- Virtual row count, overscan, offsets, and stable row identity are unchanged.

## Ownership, Tasks, And Data Flow

```text
task-free coherent PeerConnectionObservation
  -> PeerView::from_observation
       -> raw typed facts
       -> bounded canonical PeerFlagView list
  -> generated schema / TypeScript / fixtures
  -> LiveApplication typed mapping (old-v1 fallback only)
  -> PeerRow semantic flags
  -> PeerTable definition table
       -> compact glyph cell + accessible expansion
       -> grouped header legend
```

No new Rust or browser background task is introduced. The generic table owns
only ephemeral open/closed help state and document listeners while open; it
removes those listeners on close/unmount. Engine, view-set, controller, and
virtual-table lifecycle ownership remain unchanged.

## Shape-Changing Edge Cases

- outgoing versus incoming and TCP versus uTP vocabulary fixtures;
- interested/unchoked, interested/choked, uninterested, and unknown download
  state;
- remote-interested/local-unchoked, remote-interested/local-choked, and current
  unsupported upload state;
- extension true, false, and unknown independently from metadata extension;
- old v1 peer row with no `peer_flags` versus new row with an empty or populated
  set;
- duplicate, unknown, and over-bound wire flag arrays;
- multiple simultaneous flags with canonical glyph and accessible ordering;
- empty row display;
- help activation while the column is unsorted or sorted;
- keyboard Tab/Enter/Space/Escape, pointer outside dismissal, and touch-sized
  interaction;
- popover positioning near viewport edges and under Compact/Standard/Spacious;
- light, dark, wide, compact, and minimum viewport where Flags remains visible;
  and
- large peer collections retaining the existing bounded materialized DOM.

## Implementation Order

1. Record the living vocabulary/reference topic and this tactical before code.
2. Add the Rust enum, additive field, canonical derivation, and deterministic
   projection tests for current and representable future direction/transport.
3. Export the enum, regenerate declarations/schema/fixtures, update semantic
   validation bounds, and prove old-row compatibility plus hostile arrays.
4. Replace the frontend string model with semantic flags, add the exhaustive
   definition/formatting module, normalize demos, and test live fallback.
5. Add optional generic column help and the peer-specific grouped legend with
   keyboard/pointer dismissal and accessible cell labels.
6. Run representative browser accessibility, responsive, sort-independence,
   theme, and visual evidence and inspect captures.
7. Update topic/tactical evidence and run proportional workspace gates.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Rust pure projection | every currently derivable flag, paired-state exclusivity, unknown omission, canonical order, incoming/uTP vocabulary, and no speculative advanced flags |
| Generated boundary | deterministic generation, optional/defaulted v1 field, enum/schema agreement, old-row acceptance, duplicate/unknown/over-bound rejection |
| Live/demo mapping | Rust authority when present, old-producer fallback, identical semantic demo values, deterministic sort and glyph order |
| Component | empty and multi-flag cells, accessible expansion, help separate from sort, keyboard and outside dismissal |
| Browser | wide/compact responsive geometry, Light/Dark legend contrast, touch-capable activation, no serious/critical axe findings, and bounded large-peer DOM |
| Repository | Rust formatting, Clippy, relevant/workspace tests, frontend typecheck/tests/build, generated drift, and headless browser suite |

No public swarm, live engine, visible Tauri window, Android build, emulator, or
physical device is required. A controlled live peer run is optional only if
deterministic evidence cannot prove the generated Rust-to-React path.

## Stopping Condition

This slice is complete when the ambiguous raw strings are gone; Rust emits one
bounded typed semantic set for every new peer row; old v1 rows remain safe;
live and demo views share the provisional reference-backed glyph mapping; the
Flags header exposes an accessible complete legend without interfering with
sorting; only currently owned states appear; generated, Rust, frontend,
browser, and documentation evidence passes; and the resulting changes are
ready for maintainer review.

## Escalation Contract

The topic/tactical docs, additive generated field, reserved semantic enum,
current Rust derivation, frontend model refactor, generic header-help support,
legend, demo normalization, tests, and owning-topic updates are authorized.
Stop for direction if evidence requires implementing a missing engine feature,
changing the API major version, adding a dependency, redefining lifecycle or
choke semantics, redesigning Source/Swarm, launching a visible/physical client,
or publishing/committing externally.

## Implementation And Evidence

Implemented:

- Rust now exports the closed 16-value `PeerFlagView` vocabulary and an
  optional/defaulted `PeerView.peer_flags` list. `PeerView::from_observation`
  emits only coherent currently owned facts: incoming direction, uTP
  transport, extension support, metadata-extension support when available,
  and known download/upload choke relationships. Projection tests exercise
  incoming, uTP, extension support, and both download choke outcomes.
- Generated TypeScript and JSON Schema expose the optional field and closed
  enum. Runtime validation bounds the set to 16, rejects duplicates and
  unknown values, and retains an omitted-field old-producer case.
- The live adapter treats a present Rust list, including an empty list, as
  authoritative. Its old-producer fallback derives only the same representable
  subset from existing typed facts. Tests prove both the fallback and that a
  populated Rust list wins over contradictory legacy facts.
- React and every deterministic demo now carry semantic flags rather than raw
  strings. One exhaustive definition table owns canonical glyph order, labels,
  groups, sort formatting, and cell accessibility. The former live `EIC`
  meanings and separate demo strings no longer exist.
- `VirtualTable` accepts optional header help. The separate question-mark
  button opens one viewport-bounded, scrollable, nonmodal dialog, focuses that
  keyboard-scrollable content, leaves sorting untouched, restores the trigger
  on Escape, dismisses on outside activation, and adds no dependency or
  recurring task. The Peer table supplies all 16 entries in four groups.
- Browser inspection at 1,024 px Standard Light and 920 px Compact Dark showed
  legible contrast, a 24 px help target, bounded popover geometry, and usable
  scrolling. Axe found no serious or critical findings in either theme. The
  captures were inspected and removed with the temporary evidence directory.
- A 2026-08-03 presentation refinement removed the introductory and per-flag
  prose, reduced the popover to 260 px, changed its body to caption type, and
  condensed every section to low-profile headings plus single-line glyph/name
  rows. The semantic vocabulary, cell accessibility, and interaction contract
  did not change. Standard Light and Compact Dark captures showed every entry
  in a panel no wider than 260 px and no taller than 460 px; both were inspected
  and removed.
- A second 2026-08-03 refinement corrected the legend's document-portal type
  fallback. Interface-size variables live on the application element and do
  not inherit into the body-level portal, so the intended caption declaration
  had fallen back to the 16 px document default. The peer-specific legend now
  sets explicit 11 px type, removes the redundant title and strong or semibold
  weights, and uses 16 px rows with tighter gaps. The help target, semantic
  content, and general popover behavior remain unchanged.

Validation completed:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace` (321 passed, 3 ignored opt-in public-network tests)
- `npm run generate` followed by a clean generated rerun
- `npm run typecheck`
- `npm test` (95 passed, 2 skipped)
- `npm run build`
- `npm run test:e2e` (13 passed, 3 intentionally skipped live-engine tests)
- targeted Light/Standard and Dark/Compact legend browser checks, including
  canonical cells, all 16 entries, sort independence, Escape/outside
  dismissal, focus, viewport bounds, and axe analysis

The 2026-08-03 compact-legend refinement additionally passed:

- `npm run typecheck`
- `npm test` (106 passed, 2 skipped)
- `npm run build`
- targeted `npm run test:e2e -- --grep "peer flags expose"` (1 passed),
  covering absent prose, all 16 glyph/name pairs, the 260 by 460 px bounds,
  Standard Light, Compact Dark, focus/dismissal, and serious/critical axe
  analysis

The follow-up portal-type correction passed:

- `git diff --check`
- `npm run typecheck`
- `npm test` (113 passed, 2 skipped)
- `npm run build`
- targeted `npm run test:e2e -- --grep "peer flags expose"` (1 passed),
  asserting at most 11 px and weight 400 for the section, glyph, and name; no
  redundant title; a complete height below 360 px; Standard Light and Compact
  Dark contrast; focus/dismissal; and empty serious/critical axe findings
- inspected Standard Light and Compact Dark captures, then removed them

Deliberate gaps remain those in `peer-flag-vocabulary`: the reserved advanced
states do not appear until their engine owners exist; ordinary incoming and
uTP operation are not implied; upload and metadata state remain unavailable
in current runtime observations; Source still collapses multiple provenance
values; and blocking reasons remain a separate future dimension.
