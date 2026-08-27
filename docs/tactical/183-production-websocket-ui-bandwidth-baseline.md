# Tactical 183: Production WebSocket UI Bandwidth Baseline

Status: **In progress (2026-08-27).** Explicit user direction temporarily
yields Tactical `176` to this measurement-only slice. Tactical `176` retains
only its unchanged macOS-hosted iOS simulator/archive compile gate.

Topics:
[`client-view-delivery-policy`](../topics/client-view-delivery-policy.md),
[`application-connection-architecture`](../topics/application-connection-architecture.md),
[`application-view-api`](../topics/application-view-api.md),
[`performance-and-live-evidence`](../topics/performance-and-live-evidence.md),
[`client-surfaces`](../topics/client-surfaces.md), and
[`capability-readiness`](../topics/capability-readiness.md).

## Motivation And Desired Outcome

The production web client already requests semantic projections from current
navigation rather than attaching every detail view. It also acknowledges at
most one delivered batch at a time and lets current-state changes coalesce.
Those are good pressure foundations, but they do not answer how many bytes an
ordinary remote browser actually receives while idle, downloading, or viewing
one detail.

The existing evidence has two different scopes. Tactical `057` measures
in-process producer and serialized-view cost under an intentionally extreme
local transfer. Tactical `060` recorded 20,102 WebSocket view-batch bytes over
one 2.815-second General-view transfer. Neither separates initial attachment
from steady traffic across the production navigation shapes, and neither
measures the default Normal Diagnostics capture.

Build and run one bounded, opt-in, production-browser baseline before changing
delivery policy or wire shape. Attribute exact application-frame bytes and a
transparent WebSocket framing estimate to transition and steady windows. Use
the result to rank later optimizations; do not optimize in this tactical.

At the stopping condition, a reproducible command drives the production React
build over exactly one `/api/v1/connect` WebSocket, retains a representative
multi-torrent Library plus one deliberately rate-limited active transfer,
measures the selected ordinary views, cross-checks browser-observed bytes with
gateway counters, and records the resulting baseline and dominant sources in
the owning topics.

## Product And Transport Decision

Ordinary and remote browser control requires WebSocket. Networks that block
WebSocket are unsupported; this project will not add automatic HTTP fallback,
a hybrid polling lane, or product-visible transport selection for that case.
HTTP remains only the existing explicit loopback diagnostic adapter.

Native Tauri, Android, and iOS keep their in-process adapters. This tactical
does not compare those byte encodings because the user question concerns the
detachable production browser over a bandwidth-limited link.

## Stable Scenario Subset

1. **WB-001, one production connection:** one production browser page creates
   exactly one application WebSocket and no semantic HTTP request. Every text
   application frame observed by the browser is counted; binary application
   frames are absent.
2. **WB-002, representative Library:** a bounded set of metadata-complete,
   stopped torrents is installed before the browser connects. The initial
   Library attachment and an unchanged idle steady window are measured
   separately.
3. **WB-003, active Transfers:** one deterministic direct-peer torrent is
   added through the ordinary product UI and remains actively downloading
   under a fixed source rate throughout the measurement windows. The add
   transaction is a transition, not steady traffic.
4. **WB-004, selected detail interest:** General, Peers, Files, Pieces, and
   Normal Logs are selected one at a time. Each selection records its
   attachment/snapshot transition separately from a fixed steady interval.
   No adversarial all-view subscription is manufactured.
5. **WB-005, default logs truth:** Diagnostics are absent before Logs is
   selected. Logs uses the current default Normal, `info+`, all-torrent
   capture. Its retained-history transition and ongoing feed are reported
   independently; local search/display filtering is not mistaken for server
   filtering.
6. **WB-006, attribution:** reports contain exact UTF-8 application payload
   bytes by direction and frame family, estimated WebSocket bytes using the
   standard header/masking shape, update counts and standalone encoded update
   bytes by `view_id` and update kind, reset batches/bytes, and window duration.
   Standalone update bytes are attribution evidence, not a claim that envelope
   overhead can be assigned exactly to one view.
7. **WB-007, cross-check and cleanup:** captured total text-frame bytes equal
   the gateway's corresponding frame-family counters. The run records one
   accepted connection, zero heartbeat timeout, no unexpected reset, advancing
   partial progress, and joined browser, gateway, seed, and temporary-root
   cleanup.

## Reference And Product-History Dossier

No source, fixture, test data, or benchmark implementation is copied.

- Pinned Rasterbar libtorrent `2.0.13` at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d` is not a wire-shape oracle for
  RSTorrent's application API. Its
  `include/libtorrent/session_handle.hpp::post_torrent_updates` explicitly
  reports only torrents changed since the previous request and limits output
  to subscribed torrents. `src/session_impl.cpp::post_torrent_updates` drains
  the changed-torrent list into one `state_update_alert`; the adjacent TODO
  notes that a row cap could round-robin very large updates. The alert identity
  remains covered by `test/test_alert_types.cpp::state_update_alert`. This
  supports measuring change selection and cardinality before inventing a new
  serialization shape.
- RSTorrent's production authority is
  `clients/web/src/inspection/controller.ts::desiredViewsFor` plus
  `live/LiveApplication.ts::viewSpecs`: Library/Transfers request the
  collection, wide Workbench retains the collection and selected Summary, and
  only one detail projection is requested. Files and Trackers still request a
  page independent from the rendered viewport; collection changes still carry
  complete `TorrentView` rows.
- The JSTorrent sibling at observed HEAD
  `0cad4dacf540f5be42ee53c4f1e1da27aa1b3685` uses engine events, a one-second
  stats repaint, and per-torrent filtering in
  `packages/client/src/hooks/useEngineState.ts`. It has no detachable typed
  WebSocket view protocol to copy. Its product history reinforces measuring
  selected-torrent and periodic-rate work independently.

Intentional differences are one typed, cursor-acknowledged WebSocket view set,
server-side current-state coalescing, exact production navigation, and an
ordered bounded Diagnostics projection. This tactical compares no engine
throughput and adopts no libtorrent application architecture.

## Owner, Task, And Data Shape

```text
Python baseline runner
  -> temporary fixtures + pinned libtorrent seed
  -> temporary gateway/profile + production Vite preview
  -> opt-in Playwright production-navigation scenario
       -> connection-local frame capture
       -> immutable before/after window summaries
  -> gateway terminal metrics cross-check
  -> machine-readable report + exact temporary cleanup
```

- The existing application service, view hub, WebSocket pump, and React
  controller are unchanged production owners.
- The Playwright process owns only connection-local captured frame payloads
  and bounded summaries. It adds no browser persistence, product telemetry, or
  production global.
- Pure TypeScript aggregation owns byte counting, frame classification, view
  attribution, and window subtraction. It has no browser, socket, clock, or
  application-store dependency and receives bounded captured frames.
- The Python runner owns child lifetimes, fixture cardinality, source rate,
  output report, and cleanup. It always stops the preview, gateway, and seed
  and removes its exact temporary root.

## Bounds And Measurement Contract

- Library fixtures are configurable from 1 through 32; the retained baseline
  begins with 12 total rows: 11 metadata-complete stopped rows and one active
  row.
- The active payload is configurable from 8 through 256 MiB. The retained run
  uses 64 MiB with a 256 KiB piece size and a 512 KiB/s source limit so it stays
  active without allocating a large benchmark corpus.
- A steady window is configurable from 1 through 60 seconds. The retained
  baseline uses the same duration for every steady view; transitions have an
  explicit 20-second UI/materialization timeout.
- Capture holds at most 100,000 application frames or 64 MiB of frame payload.
  Exceeding either bound fails the measurement instead of silently truncating.
- Exact application payload bytes are the primary actionable measure. The
  WebSocket estimate includes basic frame header and client masking bytes but
  excludes TLS records, TCP/IP, retransmissions, carrier accounting, and radio
  energy; the report must not call it billed cellular usage.
- Every output records repository revision/dirty state, platform, browser,
  payload/cardinality/rate/window configuration, exact window results, gateway
  connection metrics, and cleanup outcome. No payload, profile, credential, or
  temporary path is retained in the repository.

## Implementation And Validation Sequence

1. Add pure frame aggregation and deterministic tests for UTF-8 byte counts,
   client masking, frame-family totals, view/update attribution, reset bytes,
   binary handling, capture bounds, and immutable window deltas.
2. Add one opt-in Playwright scenario that uses the production UI, verifies
   one WebSocket/no semantic HTTP, separates transition and steady windows,
   and emits one machine-readable observation.
3. Add one bounded Python runner that prepares stopped Library rows and a
   rate-limited active fixture, starts and joins every owner, cross-checks
   browser and gateway counters, and emits a complete report.
4. Run focused unit/type/build gates and the complete retained baseline. Rank
   steady bytes per second separately from transition bytes and record Normal
   Logs history/feed results.
5. Update the owning delivery/performance/application topics, restore Tactical
   `176` as the sole **Now**, and retain optimization choices as evidence-led
   follow-up rather than part of this tactical.

## Non-Goals

- No delivery profiles, cadence changes, bandwidth token bucket, compression,
  binary codec, sparse row patch, pagination change, or structural-sharing
  optimization.
- No user-visible bandwidth meter, product analytics, persistent telemetry, or
  settings surface.
- No polling fallback, WebSocket-blocked-network support, relay design, or
  transport comparison.
- No exact TLS/carrier-byte or radio-energy claim and no WAN/cellular hardware
  run in the first baseline.
- No Android, iOS, or Tauri presentation change. Their semantic contracts and
  engine behavior are unaffected, so their build gates are inapplicable.
- No public swarm, physical device, visible browser, destructive cache control,
  or retained payload artifact.

## Escalation And Stopping Condition

Stop for direction if useful attribution requires a generated product API
change, persistent product telemetry, a new dependency, visible/physical
operation, public network access, or an optimization rather than measurement.
Ordinary test-harness repair, bounded fixture adjustment, or extending
diagnostic-only counters does not broaden product behavior.

This tactical is complete only when the reproducible production-WebSocket
matrix passes, exact bytes and cleanup are recorded, the existing topic states
what is already interest-selected and what remains over-broad, and the result
names the next evidence-led optimization without implementing it.
