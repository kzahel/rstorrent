# Tactical 187: Compact Metadata Acquisition Progress

Status: **Accepted; implementation in progress on 2026-08-28.** Explicit user
direction temporarily yields Tactical `176`'s unavailable macOS-only compile
gate to this bounded end-to-end product slice.

Topics:
[`application-control`](../topics/application-control.md),
[`application-view-api`](../topics/application-view-api.md),
[`client-view-delivery-policy`](../topics/client-view-delivery-policy.md),
[`web-ui-design`](../topics/web-ui-design.md),
[`client-surfaces`](../topics/client-surfaces.md),
[`protocol-support`](../topics/protocol-support.md), and
[`capability-readiness`](../topics/capability-readiness.md).

## Motivation And Desired Outcome

Magnet downloads currently reduce the General tab to an indeterminate
"Downloading metadata" state. The engine already owns exact cross-peer BEP 9
block state, but the application exposes it only through a large diagnostic
snapshot that is neither interest-selected nor a stable product contract.

Show truthful metadata acquisition progress in General without turning every
16 KiB transition into a repeated torrent row. The selected-torrent projection
will carry small scalar facts and a packed two-bit block map. The React client
will decode it into an accessible progress card whose compact Canvas echoes the
Pieces tab. Pure-v1, pure-v2, and hybrid magnets use the same BEP 9 map because
all three acquire the exact bencoded `info` dictionary through the same owner.

Pure-v2 and hybrid torrents may subsequently need BEP 52 piece or leaf hashes.
That work is a separate integrity-preparation phase with a truthful coarse
active/waiting state and counts, not a percentage derived from unrelated hash
ranges. The ordinary torrent summary must not call that time payload transfer.

## Stable Scenarios

1. **MAP-001, unknown geometry:** before a peer advertises a valid metadata
   size, General shows indeterminate discovery/acquisition plus current active
   peer and in-flight request counts. It emits no invented total or empty map.
2. **MAP-002, known geometry:** after size agreement, each block is exactly
   missing, requested, or received. Unique received bytes and percent account
   for the shorter final block and cannot exceed total size.
3. **MAP-003, cross-peer and out-of-order:** assignments and accepted blocks
   from every worker update one torrent-owned map; disconnect or rejection
   returns affected requested blocks to missing without losing received blocks.
4. **MAP-004, integrity retry:** a metadata hash mismatch increments the retry
   count and atomically returns the current received byte count and block map
   to the assembler's reset state. Cumulative diagnostic bytes are never shown
   as current progress.
5. **MAP-005, completion:** accepted hash-verified metadata clears the active
   metadata card. It cannot remain as a misleading permanent 100% transfer.
6. **MAP-006, format parity:** v1 validates the acquired dictionary against
   SHA-1, v2 against SHA-256, and hybrid against its reconciled identities, but
   all formats use this same BEP 9 geometry and encoding.
7. **MAP-007, v2/hybrid preparation:** when authenticated piece or leaf hashes
   are still needed, General shows "Fetching piece hashes" while requests are
   active or "Waiting for a hash-capable peer" otherwise, with bounded needed
   range and active request counts. It shows no percent or synthetic bitmap.
8. **MAP-008, generation and aliases:** metadata and hash-preparation updates
   carry the active task generation. Stale updates after stop/restart and the
   losing provisional hybrid identity cannot mutate the retained projection.
9. **MAP-009, interest and recovery:** only an explicit selected-torrent
   preparation view retains and delivers this state. Leaving General removes
   the view; lease expiry, cursor reset, alias reconciliation, and reconnect
   recover from one coherent current snapshot.
10. **MAP-010, bounded presentation:** the card remains useful on phone and
    desktop widths, provides a native progressbar and text legend, and never
    creates one DOM node per block.

## Source And Product Survey

The normative local specification checkout is revision
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`:

- `bep_0009.rst` fixes metadata pieces at 16 KiB, communicates total size in
  the extension handshake/data message, and numbers request/data/reject
  messages independently of content pieces;
- `bep_0052.rst` defines the separate `piece layers` metainfo dictionary and
  peer messages 21--23 for hash request, hashes, and rejection. Their range
  and proof semantics do not form an honest continuation of BEP 9 percent.

Pinned libtorrent `2.0.13` revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` remains the completeness oracle:

- `src/ut_metadata.cpp`, especially `metadata_size`, `received_metadata`,
  `maybe_send_request`, and `on_extended`, keeps bounded torrent-level block
  knowledge across peer plugins, rejects invalid or oversized total sizes,
  retries rejected/missing work, and resets poisoned metadata before retry;
- `test/test_fast_extension.cpp` covers extension negotiation, metadata size,
  request/data/reject, and invalid metadata behavior; and
- the existing Tactical `155`/`156` dossiers cover authenticated BEP 52 hash
  acquisition and the one-owner pure-v2/hybrid runtime transitions reused here.

Local JSTorrent revision `0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`
supplies product history. `packages/engine/src/core/metadata-fetcher.ts` exposes
known size and received pieces but is peer-local; RSTorrent retains its already
proven torrent-level owner. `packages/ui/src/components/PieceVisualization.tsx`
and `android/.../PieceMap.kt` demonstrate bounded Canvas rendering and a text
legend. No source, fixture, color asset, or class layout is copied.

`scripts/references.py status` confirmed the pinned specification,
libtorrent, and rqbit checkouts. Its aggregate status remains non-green because
the unrelated optional libutp and librqbit-utp checkouts are absent and the
local JSTorrent remote differs only by repository-name case; the exact revisions
used above were inspected directly.

## Semantic Contract And Encoding

Add one `TorrentPreparation` view selected by canonical torrent ID. Its current
value is absent when no preparation phase is active and otherwise carries the
task generation plus exactly one of:

- metadata acquisition: phase, optional total size, unique accepted bytes,
  block count, packed block states, active peers, requests in flight, and
  metadata hash-retry count; or
- integrity preparation: active hash acquisition or waiting for a hash-capable
  peer, needed logical hash-range count, and active request count.

Metadata block states are packed four per byte, least-significant block first,
with two bits per block:

| Bits | Meaning |
| --- | --- |
| `00` | missing |
| `01` | requested by at least one current peer |
| `10` | received and retained by the current assembler generation |
| `11` | reserved and rejected by clients |

JSON carries the packed bytes as canonical padded base64. The schema also
carries `block_count`; clients require exactly `ceil(block_count / 4)` decoded
bytes, zero unused high bits, no reserved state, exact size/count geometry,
and `received_bytes <= total_size`. This is a compact semantic field rather
than a generic serialization or transport codec. A future negotiated binary
encoding may carry the same bytes directly without changing state meaning,
view lifecycle, or cursor acknowledgement.

BEP 9's existing 30 MiB metadata limit and 16 KiB blocks cap the projection at
1,920 blocks, 480 packed bytes, and 640 base64 characters. Scalars use decimal
strings only where the shared contract already requires lossless 64-bit JSON.
No peer endpoint, payload byte, metadata byte, request history, or per-peer row
crosses this product boundary.

## Ownership, Tasks, Cancellation, And Data Flow

```text
TorrentMetadataDownload (pure authoritative block state)
       | bounded immutable observation
metadata workers / supervisor
       | generation-tagged DownloadActivityEvent
       v
ViewHub selected-torrent preparation projection
       | existing lease / queue / cursor / reset
generated contracts and validated client replicas
       |
React General progress card + compact Canvas

V2HashScheduler (authoritative logical needs/attempts)
       | deduplicated coarse observation, same generation path
       +----------------------------------------------^
```

The protocol assembler remains runtime-independent and gains only pure bounded
observation helpers. Workers and the existing content owner retain sockets,
tasks, cancellation, scheduling, retries, and completion. `DownloadControl`
emits state changes through its existing sink and deduplicates coarse hash
preparation observations. `ViewHub` generation-fences mutations and remains
the only product projection owner. No new background task, command, scheduler,
socket, persistence table, transport acknowledgement, or mutable client-to-
engine path is added.

Hybrid alias reconciliation continues to choose one canonical application row
and one active runtime generation. Removing the losing row removes its view;
late events carrying another generation are ignored. Pause, stop, terminal
failure, completion, and task replacement clear preparation state through the
same joined lifecycle that clears other runtime projections.

## Implementation Order And Intermediate Gates

1. Add pure metadata-byte/block-state observations and exact reset/geometry
   tests in `rstorrent-protocol`.
2. Add generation-tagged metadata and coarse v2 hash-preparation activity,
   deduplication, lifecycle clearing, and engine transition tests.
3. Add the selected-only application projection, progress-reason integration,
   view-set validation/diff/reset behavior, generated TypeScript/UniFFI
   boundaries, and exhaustive first-party reducers.
4. Add validated web decoding, General progress card/Canvas, accessible text,
   responsive styling, demo fixtures, and controller/component tests.
5. Run proportional repository, web, Android, and Linux-available Apple gates;
   update this tactical and living topics with exact evidence before restoring
   Tactical `176` as the sole **Now**.

Each stage must leave focused tests green before the next commit. Ordinary
internal refactoring at these owners and adversarial cases implied by the
contract are authorized.

## Validation Matrix

### Pure state

- unknown/known size, zero accepted bytes, short final block, out-of-order and
  duplicate blocks, assignment/remove/reject, completion, mismatch reset, and
  maximum-size packing;
- packed-state decoder rejection of malformed base64, wrong length, reserved
  state, dirty padding, impossible geometry, and received-byte overflow; and
- coarse hash active/waiting/ready transitions without invented completion.

### Scripted runtime and application

- multiple workers visibly share one map and release requested states on
  reject/disconnect;
- hash mismatch resets current progress and increments retries before later
  completion;
- pure-v1 clears after BEP 9 while pure-v2 and hybrid move to truthful hash
  preparation only when logical needs exist;
- stale generations, provisional alias removal, terminal cleanup, lease
  expiry, reconnect, coalescing, queue bound, cursor replay, and fresh snapshot
  recovery cannot retain or apply stale preparation.

### First-party product and platform

- generated schema/TypeScript/UniFFI drift checks;
- web reducer/controller/component tests, typecheck, production/CSP build, and
  desktop/phone General presentation assertions;
- Android dual-ABI generated-boundary, native Rust, APK, and unit-test build.
  Android may reduce and ignore this selected view until a native presentation
  tactical; no Compose screen is implied here;
- `cargo build -p rstorrent-ios --release`, generated Swift/source inspection,
  and exhaustive reducer tests where runnable. This Linux host cannot claim
  Xcode, simulator, archive, or Swift execution; Tactical `176` retains that
  pre-existing macOS-only gate; and
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`,
  `cargo test --workspace`, complete web tests, and `git diff --check`.

The slice changes no interoperability behavior, so a new public-swarm run is
not a stopping gate. Existing pinned-libtorrent BEP 9, pure-v2, and hybrid
interop tests are included proportionally if covered by the workspace suite.
No visible client, emulator, physical device, public network, package, or
release is launched without separate direction.

## Non-Goals

- No per-peer metadata table, endpoint, request timeline, byte-rate graph,
  ETA, historical retry record, persistence, notification, or control.
- No BEP 52 hash bitmap or percent, arbitrary Merkle base-layer UI, piece-layer
  persistence, or change to authenticated hash scheduling.
- No Android Compose or iOS presentation, Pieces-tab redesign, binary wire
  codec, compression, API compatibility bridge, telemetry, or public claim.
- No change to metadata size, peer cohort, request, connection, scheduler,
  payload, memory, queue, or delivery bounds.

## Escalation And Stopping Condition

Stop for direction if truthful progress requires retaining metadata payload,
exposing peer identity, adding a task or dependency, changing BEP 9/BEP 52
scheduling or resource limits, inventing hash completion, altering durable
state, adding a second delivery acknowledgement, or materially expanding
native presentation. Internal names, layout, conservative validation,
generated-boundary repair, and same-owner bugs exposed by deterministic tests
do not require escalation.

The tactical is complete when selected General shows the bounded accessible
metadata map end to end for all torrent formats; pure-v2/hybrid hash preparation
is separately truthful; current progress resets/completes and generation/alias
lifecycle are exact; hidden views produce no repeated preparation traffic; all
first-party reducers remain exhaustive; the declared Linux-available gates
pass; living topics record the result; and Tactical `176` resumes as the sole
**Now**.
