# Piece Map Visualization

Status: Planned after Tactical `044`.

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
