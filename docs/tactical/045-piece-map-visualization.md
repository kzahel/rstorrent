# Piece Map Visualization

Status: Complete (2026-08-02).

Topics: `disk-and-piece-inspection`, `download-correctness`,
`application-view-api`, `web-ui-design`, `desktop-inspection-surface`,
`performance-and-live-evidence`

## Motivation

The Pieces tab is an empty scaffold even though the application contract
already carries verified ranges and one current active piece. A useful overview
must represent all bounded active pieces, survive bursty diffs and browser
suspension, and paint large torrents without thousands of React or DOM nodes.
This slice generalizes the semantic projection and adds a read-only Canvas
overview after the storage vocabulary from Tactical `044` is proven.

## Scope

- Replace the single `Option<ActivePiece>` replica with a bounded keyed active
  piece-attempt collection derived from typed engine lifecycle events.
- Preserve one coherent compact verified snapshot followed by verified
  additions/clears and active upserts/removals through leased view sets.
- Validate range order, bounds, piece identity, attempt identity, and snapshot
  replacement strictly in Rust and TypeScript.
- Materialize compact piece state for painting without one object or DOM node
  per piece.
- Implement a high-DPI, resize-aware, RAF-coalesced, bounded Canvas 2D Piece
  map with text legend and accessible aggregate description.
- Add named healthy, endgame, hash-retry, large-torrent, empty, and
  metadata-pending fixture states using the normal demo clock and adapter.
- Prove live verified/active diffs, cursor/lease recovery, suspension, scale,
  responsive geometry, cleanup, and exact verified completion headlessly.
- Update owning topics and capability status.

## Non-goals

- Clicking, selecting, hovering, hit testing, tooltips, piece navigation, or
  piece priority/selection commands.
- A DOM element, Zustand row, or application event for every piece or block.
- Replacing the full verified snapshot after every transition.
- Rendering storage jobs or block payload; Disk owns tabular active work.
- A WebGL or rendering-library dependency, animated decoration, or a long-lived
  history.
- Redesigning Android's PieceMap screen. Generated contracts and proportional
  Android builds remain valid; convergence may follow separately.
- Engine picker, request, hash, storage, or performance changes unrelated to
  truthful projection.

## Reference Dossier

Pinned libtorrent `2.0.13` revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/torrent_status.hpp` exposes compact piece possession and
  state without payload.
- `include/libtorrent/download_priority.hpp`, `src/piece_picker.cpp`, and
  `test/test_piece_picker.cpp` distinguish piece possession, downloading
  state, block progress, failure/retry, and picker generations.
- `src/mmap_disk_io.cpp::do_hash` verifies one piece incrementally from fixed
  storage chunks rather than requiring a contiguous UI-shaped piece buffer.
- `test/test_read_piece.cpp`, `test/test_storage.cpp`, and
  `test/test_hash_picker.cpp` cover complete piece boundaries, storage mapping,
  and hash state.

RSTorrent adopts compact possession plus bounded sparse current activity and
explicit retry/reset semantics. It does not expose libtorrent picker internals
or copy its status structures.

Local JSTorrent revision
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/ui/src/components/PieceVisualization.tsx` uses RAF-driven drawing
  and provides the familiar piece-state visual hierarchy.
- `android/app/src/main/java/com/jstorrent/app/ui/components/PieceMap.kt` uses
  Canvas, bitsets, density-aware geometry, and bounded aggregation.
- `android/app/src/main/java/com/jstorrent/app/ui/tabs/PiecesTab.kt` provides
  product vocabulary and a noninteractive legend.
- `packages/engine/src/core/torrent.ts` historically assembles a whole active
  piece before hashing; RSTorrent deliberately retains its incremental
  storage-hash path instead.

No source, asset, palette, or fixture is copied.

## Invariants And Bounds

- `piece_count` bounds all verified and active indices.
- Every active piece has a stable row/attempt identity, exact piece length, and
  sorted nonoverlapping requested, received, and stored ranges within that
  length.
- One block byte is represented in at most one active lifecycle bucket.
- A verified transition removes matching active state only after the engine
  accepts the hash. A failed hash clears completed stored state and begins or
  identifies a new bounded attempt without clearing unrelated verified pieces.
- Snapshot replacement is atomic. No diff from a previous view-set epoch is
  applied to a new piece map.
- The wire remains compact: verified state is ranges or packed bytes plus typed
  range/bitmap diffs; active state is sparse. Payload buffers never cross the
  boundary.
- Client memory is `O(piece_count)` bytes for materialized state plus bounded
  active pieces, not `O(piece_count)` objects or DOM nodes.
- Canvas backing pixels are bounded independently of `piece_count`; very large
  torrents use deterministic buckets and aggregate the most advanced/important
  state without creating an unbounded surface.
- At most one animation frame is pending. Updates while hidden mutate the
  replica but do not schedule an unbounded paint queue.

Initial renderer bounds are a maximum 16,384 visual cells, a maximum 1,024 CSS
pixels of canvas height, and device-pixel-ratio clamping at 3. These may change
only with recorded scale evidence.

## Owner And Render Flow

```text
typed piece lifecycle events
          |
ViewHub verified ranges + active map
          |
leased PieceActivity snapshot / keyed diffs
          |
strict TS reducer + compact client materialization
          |
Zustand revision selector -> canvas ref -> one RAF paint
```

The engine scheduler/storage/verifier remain authoritative. The application
retains only semantic ranges and attempt summaries. The pure view-set reducer
owns snapshot/diff correctness. The inspection adapter owns the client replica;
the Canvas component owns only geometry, backing pixels, and draw scheduling.
Unmount cancels the pending frame and disconnects `ResizeObserver`.

## View Contract

Evolve capability `piece_activity` without adding a second Pieces authority:

```text
ActivePiece {
  piece_id,
  piece_index,
  attempt,
  piece_length,
  stage,
  requested: Vec<IndexRange>,
  received: Vec<IndexRange>,
  stored: Vec<IndexRange>,
  age_millis,
  error,
}

ViewSnapshot::PieceActivity {
  torrent_id,
  piece_count,
  verified,
  active: Vec<ActivePiece>,
}

ViewPatch::PieceActivity {
  torrent_id,
  piece_count,
  verified,
  cleared,
  active_upsert,
  active_removed,
}
```

`verified` and `cleared` are piece-index ranges. The TypeScript adapter expands
them into a typed-array bitmask/state buffer once and applies patches locally.
When an old server supplies no capability, the tab renders unsupported rather
than an empty completed map.

## Visual Semantics

State priority for a visual cell is failed/retrying, hashing, stored, received,
requested, verified, then missing; when aggregation combines many pieces, the
legend explains that active/error state wins over a completed majority. A
future renderer may use proportional bucket fill, but the initial contract is
deterministic categorical state.

The panel also exposes plain aggregate text for total pieces, verified pieces,
active pieces, and state counts. Canvas uses `role="img"` and an updated
`aria-label`; the legend contains labels in DOM text and differentiates states
with swatch pattern/border as well as hue. There are no focusable cells.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure application model | simultaneous active pieces; interleaved block events; verify, hash fail/retry, clear, removal, durable refresh, and bounds |
| View sets | initial compact snapshot; keyed active diffs; verified add/clear; coalescing; queue reset; lease expiry and fresh replacement |
| TypeScript | strict decoder failures, pure reducer equivalence, typed-array snapshot/diff equivalence, epoch replacement, and no stale range application |
| Renderer | no per-piece DOM; one pending RAF under update burst; resize/DPR cleanup; deterministic colors/legend; 1, 4,000, and 250,000-piece scale |
| Responsive/accessibility | wide, compact, and phone screenshots; no clipping; text aggregate and legend; keyboard tab navigation; axe checks |
| Controlled live | loopback seed exposes requested/received/stored/verified transitions, exact content completes, active set cleans up, suspension recovers coherently |
| Repository | Rust format/Clippy/tests, generated checks, web typecheck/tests/build/Playwright, proportional Android compilation/tests |

## Implementation Order

1. Begin only after Tactical `044` is complete and clean.
2. Generalize the application model to bounded simultaneous active pieces and
   add adversarial transition tests.
3. Evolve snapshot/patch types, generation, strict validation, pure reducers,
   lease recovery, and explicit Android handling.
4. Add compact frontend state materialization and deterministic named fixtures.
5. Implement and unit-test the Canvas renderer, geometry, RAF, DPR, resize,
   aggregation, accessibility, and cleanup.
6. Add headless visual/scale/live evidence, update topics, run gates, mark
   complete, and commit cleanly.

## Stopping Condition

This slice is complete when the Pieces tab renders a truthful read-only Canvas
overview from one coherent snapshot plus diffs, simultaneous active pieces and
hash-retry cleanup are correct, no per-piece DOM or repeated full snapshot is
used, 250,000 pieces remain bounded and responsive, suspension/lease recovery
replaces stale state safely, a controlled transfer reaches exact verified
completion, all proportional gates pass, evidence is recorded, and the
working tree is clean.

## Escalation Contract

Active-piece map refactoring, compact-range/bitmap contract evolution,
generated types, test-only lifecycle fixtures, Canvas/CSS implementation,
bounded aggregation, and isolated harness changes are authorized. Stop for
direction if evidence requires piece commands, picker policy changes, a new
rendering dependency, a stable public API compatibility promise, Android UI
redesign, public swarm traffic, visible app launch, or scope outside the shared
topic.

## Implementation And Evidence

The engine now emits `PieceStarted` only after the first request for that
attempt is successfully sent, includes the attempt generation, and emits an
explicit hashing transition. The application retains simultaneous attempts in
a keyed map and reconciles stage, age, failure, retry, and cleanup from the
same bounded storage-runtime facts used by Disk. A corrupt-source test proves
attempts `1` and `2` have separate start/hash lifecycles, and session tests
prove interleaved attempts do not overwrite one another.

The `piece_activity` contract now carries a sparse active vector in snapshots
and keyed upserts/removals in patches. Verified state remains canonical piece
ranges. Rust and TypeScript validation reject bad indices, identities,
cross-state overlaps, and excess active work. The web replica expands
verified ranges into one `Uint8Array`, updates that same allocation within a
view-set epoch, and rebuilds it after an epoch replacement. Android retains
its distinct native Canvas but now reduces the same active collection; it
explicitly ignores the global Disk presentation until that screen is
authorized.

The web Pieces tab uses one Canvas 2D surface. Geometry is resize-aware,
clamps device scale at 3, never exceeds 16,384 visual cells or the 1,024 CSS
pixel hard limit, and currently targets a 320-pixel overview for very large
torrents. Aggregated cells distinguish complete from mixed buckets; sparse
requested, received, stored, hashing, and failed states override their bucket
without per-piece DOM. Mixed and failed states have patterns in addition to
color, while aggregate text and a DOM legend provide the accessible
description. The phone tab strip now recenters the active tab after its hidden
detail surface reopens.

Permanent named fixtures cover ordinary progress, metadata pending, endgame,
hash failure and clean retry, empty state, and a 250,000-piece torrent. The
large browser proof painted a 718 by 320 CSS-pixel canvas with 527 total DOM
elements and six sparse active attempts. Wide, compact, and phone captures
passed an axe serious/critical scan with no findings.

The controlled production-web proof used libtorrent `2.0.13.0` as a loopback
seed for a 4 MiB payload plus a 7,000-byte cross-file prefix: 122 files and 17
pieces. At a 256 KiB/s seed limit, the browser observed active piece work in
1.4 seconds, deliberately lost its 500 ms view-set lease, recovered from a
fresh epoch, and reached exactly 17 verified pieces with zero active attempts
in 18.3 seconds. External SHA-1 comparison passed and every browser, gateway,
seed, and application owner joined. No public traffic or visible client was
used.

Validation passed:

- generated TypeScript/schema and Kotlin/UniFFI regeneration;
- workspace format, warning-denying Clippy, and tests: 155 engine tests plus
  three ignored live probes, 68 session tests, six gateway tests, 63 protocol
  tests, six Rust Android tests, and the remaining workspace targets;
- 68 web unit tests plus two intentionally skipped opt-in cases, TypeScript
  checking, and the production Vite build;
- all nine deterministic demo Playwright tests, including accessibility,
  responsive screenshots, retry truth, and bounded scale;
- the controlled live piece/lease-recovery proof above; and
- Android `x86_64` and `arm64-v8a` native builds plus `assembleDebug` and
  `testDebugUnitTest`.

The deliberate deferrals remain piece interaction and priority commands,
picker internals, detailed block presentation, Android UI redesign, binary or
streaming transport, and changes to scheduling/storage policy.
