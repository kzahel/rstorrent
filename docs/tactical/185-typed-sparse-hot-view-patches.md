# Tactical 185: Typed Sparse Hot-View Patches

Status: **In progress (2026-08-28).** Explicit user direction temporarily
yields Tactical `176` to the second measured application-delivery repair.
Tactical `176` retains only its unchanged macOS-hosted iOS simulator/archive
compile gate.

Topics:
[`client-view-delivery-policy`](../topics/client-view-delivery-policy.md),
[`application-view-api`](../topics/application-view-api.md),
[`performance-and-live-evidence`](../topics/performance-and-live-evidence.md),
and [`capability-readiness`](../topics/capability-readiness.md).

## Motivation And Desired Outcome

Tactical `184` restores current-state coalescing and reduces active detail
traffic by 70--86%, but the retained run still repeatedly sends complete rows
when only progress, rates, ages, or a small state group changed. Library and
Summary alone each contribute roughly 29--42 KiB over an eight-second detail
window, while Peers and Pieces contribute complete volatile rows of their own.

Replace those repeated-row updates with one closed typed semantic delta model.
New rows and snapshots remain complete. Existing torrent, file, peer, and
active-piece rows carry only changed fields. Every first-party reducer rebuilds
the same full client state, and pending coalescing merges the typed values
before serialization. At the stopping condition the clean production-browser
baseline is lower again without lost state, reset, client divergence, or a
transport-specific semantic contract.

## Stable Scenarios

1. **SP-001, torrent fields:** every mutable `TorrentView` field round-trips
   through diff/apply, including setting and clearing nullable values.
2. **SP-002, torrent lifecycle:** insertion uses a full upsert, subsequent
   changes use sparse updates, removal remains explicit, and reinsertion is a
   new full upsert. Selected Summary represents complete absence explicitly.
3. **SP-003, files:** selection, completed/verified bytes, and media
   availability update sparsely without allowing immutable file geometry or
   identity to drift.
4. **SP-004, peers:** handshake, lifecycle, role/flags, counters, rates,
   requests, ages, disconnect detail, and nullable peer metadata update
   sparsely while connection and torrent identity remain fixed.
5. **SP-005, active pieces:** stage, request/receive/store ranges, age, and
   error update sparsely while piece identity, attempt, and length remain
   fixed.
6. **SP-006, coalescing:** later fields replace earlier values by
   `(view_id, row_id, field kind)`; unrelated fields compose; full upserts and
   removals retain their keyed ordering and barrier semantics.
7. **SP-007, hostile input:** empty or duplicate field sets, identity mismatch,
   and update-to-missing-row fail continuity rather than inventing state. A
   fresh snapshot/reset recovers authoritatively.
8. **SP-008, client parity:** Rust, generated TypeScript/schema, web, Android,
   and iOS reducers implement the same exhaustive closed variants. Settings
   draft convergence continues to observe a merged complete `TorrentView`.
9. **SP-009, measured result:** the identical Tactical `183` production run
   lowers steady application bytes again with exact gateway/browser agreement,
   progress, bounds, cleanup, and zero resets.

## Semantic Contract

Each measured collection patch separates three meanings:

- `upsert`: complete rows for insertion or coherent replacement;
- `update`: `RowUpdate { row_id, fields: Vec<FieldUpdate> }` for an existing
  row; and
- `removed`: explicit row identities.

`TorrentFieldUpdate`, `FileFieldUpdate`, `PeerFieldUpdate`, and
`ActivePieceFieldUpdate` are closed tagged enums. A nullable value lives inside
the present variant: omitted variant means unchanged, while a present variant
whose value is null means clear. Field vectors are nonempty, contain each field
kind at most once, and use canonical order. Stable identities and immutable
geometry are not patchable.

Selected Summary uses a closed change that can either replace the complete
optional torrent or update its existing row sparsely. A sparse update against
no selected row is a continuity failure. Fresh and reset snapshots remain
complete authoritative values.

Pure Rust helpers own row diff, validation, application, and coalescing. The
hub compares complete previous/current rows and emits exactly unequal mutable
fields. Coalescing applies a later sparse update into a pending full upsert when
possible, otherwise retains only the newest value for each row and field kind.
Removal discards superseded pending updates; reinsertion is complete.

## Encoding Independence And Later Binary Format

The semantic model is deliberately independent from JSON object paths and
wire frames. JSON remains the only codec in this tactical, but a later
negotiated binary codec can assign explicit stable numeric IDs or bit positions
to these closed field meanings and encode the same snapshots, patches,
cursors, resets, and acknowledgements.

Rust enum declaration order and generated union layout are not binary field
numbers and must never silently become them. A future binary tactical must own
version/codec negotiation, an explicit field-number registry, unknown-field
behavior, golden vectors, payload/CPU benchmarks, and mixed-version policy.
This tactical adds no binary frames, compression, schema dictionary, or codec
handshake.

## Ownership, Client Boundary, And Failure Policy

The session view hub remains authoritative for complete rows and semantic
diffs. `ViewSetInner` remains authoritative for queue coalescing and bounds.
Transport adapters remain ignorant of field meaning. Each first-party client
store applies patches into full local rows before presentation; components and
settings drafts continue to consume full models.

No compatibility aliases or old whole-row update path are retained. Malformed
or inapplicable sparse state is rejected and triggers the existing continuity/
reset recovery rather than partial best-effort mutation.

## Validation

- pure diff/apply/validation/coalescing tests covering every field variant and
  all stable scenarios;
- view-hub, view-set bound/reset/replay, command/view convergence, and settings
  regression suites;
- generated Rust schema/TypeScript/UniFFI drift checks and exhaustive reducers;
- web unit tests, typecheck, production/CSP build, Android dual-ABI generated
  boundary/build, and iOS generated/source inspection on Linux;
- Rust formatting, workspace Clippy, and workspace tests; and
- the identical opt-in production WebSocket baseline with exact browser/
  gateway cross-check and temporary cleanup.

The updated iOS source and generated Swift boundary remain subject to Tactical
`176`'s existing macOS-only simulator/archive compile gate; this Linux host
must not claim that compile.

## Non-Goals

- No sparse Tracker, Swarm, Disk, Diagnostics, DHT, or session-rate history
  until measurement selects it.
- No incremental rate-history window, overlap-elimination projection, delivery
  profile, viewport paging, hidden-client policy, or user bandwidth control.
- No binary codec, compression, polling fallback, relay, or TLS/carrier-byte
  measurement.
- No engine scheduling, product presentation, or settings behavior change.

## Escalation And Stopping Condition

Stop for direction if the shared contract cannot express nullable changes
across generated clients, correct recovery requires weakening the cursor/reset
model, a new task or transport-specific reducer is required, or a dependency
or binary wire decision becomes necessary. Ordinary DTO/reducer replacement,
generated-boundary work, deterministic validation, Android parity, and the
bounded live rerun remain in scope.

This tactical is complete only when all stable scenarios pass on every
available first-party boundary, the old repeated whole-row update path is
removed, the clean retained run proves a further causal reduction with no lost
state/reset, all owning topics record exact evidence and deliberate deferrals,
and temporary artifacts are removed.
