# Tactical 175: Retained Swarm Peer Transfer Totals

Status: **Complete.** Explicit maintainer direction on 2026-08-27 temporarily
yielded desktop release Tactical `158` to this bounded diagnostic slice. The
engine, portable contract, React presentation, Android boundary/build, and
installed LAN/tailnet service evidence pass; Tactical `158` has resumed as the
sole **Now**.

Topics: `peer-lifecycle`, `application-view-api`, `web-ui-design`,
`client-surfaces`, `capability-readiness`

Dependencies: completed registry-backed Swarm Tactical
[`064`](064-registry-backed-swarm-inspection.md), the existing active Peers
payload accounting, and the generated Rust/TypeScript/UniFFI application
contract.

## Motivation And Desired Outcome

The active Peers table exposes payload received from and uploaded to each live
connection generation. The Swarm table deliberately retains peer records
through disconnect and reconnect, but it omits transfer contribution. When a
torrent stalls and a useful peer is backed off or idle, the operator therefore
cannot tell whether that retained endpoint supplied most of the downloaded
payload before its connection disappeared.

Add default-visible **Downloaded** and **Uploaded** columns to Swarm. Each row
shows exact useful payload received from, and payload sent to, that retained
peer record during the current engine lifetime. Values update while any
connection generation is active, survive disconnect/backoff, and accumulate
across reconnects without double counting.

This tactical supersedes only Tactical `064`'s deferral of client-lifetime byte
totals. It preserves every other Swarm ownership, identity, selection, and
resource invariant from that tactical.

## Stable Scenarios

1. **SPT-001 live contribution.** An active content connection's useful
   download and payload upload totals appear on its matching Swarm record at
   the existing coalesced peer-observation cadence.
2. **SPT-002 retained disconnect.** Removing a connection transfers its final
   totals exactly once into the retained record. The Swarm values do not drop
   when the row becomes eligible, backed off, failure limited, or banned.
3. **SPT-003 reconnect accumulation.** A later generation for the same
   endpoint starts with the retained base and adds its own current totals.
   Closing it preserves the exact combined result.
4. **SPT-004 overlapping generations.** If incoming and outgoing generations
   temporarily share one retained record, the snapshot adds both active
   generations to the retained base. Removing either generation neither drops
   nor duplicates its contribution.
5. **SPT-005 bounds and lifecycle.** Addition saturates at `u64::MAX`; record
   eviction removes its totals with the row; inactive/torrent-missing Swarm
   catalogs remain empty; process restart does not invent durable history.
6. **SPT-006 portable projection.** Rust maps both counters to exact decimal
   strings. Browser validation, patch reduction, live mapping, decimal sorting,
   configurable units, and demo data preserve values above JavaScript's safe
   integer limit.
7. **SPT-007 first-party boundaries.** The generated TypeScript/schema and
   UniFFI Kotlin boundary carry the additive fields. React renders the two
   Swarm columns. Android Compose currently presents the Swarm count summary
   rather than a Swarm row table, so a new Android row presentation is outside
   this bounded slice; its generated boundary and build still must pass.

## Counter Semantics

- `payload_downloaded_bytes` means peer content bytes accepted as useful by
  the existing content owner. It excludes peer-wire protocol chatter,
  metadata, duplicate/redundant payload, and rejected payload.
- `payload_uploaded_bytes` means content payload written for the peer. It
  excludes peer-wire protocol chatter and queued-but-unsent bytes.
- Totals belong to one torrent-scoped `PeerRecord` identity and cover that
  record's retained lifetime in the current engine process. They are not a
  connection log, a durable session statistic, or an endpoint identity across
  process restart.
- Values are observational only. They do not affect eligibility, dial order,
  retry deadlines, failure limits, trust, parole, banning, choking, request
  windows, or integrity decisions.

## Reference Dossier

### Protocol semantics

BEP 3 defines peer-wire messages and payload exchange, but not a retained peer
catalog or product inspection totals. Record-lifetime accumulation and its UI
labels are RSTorrent product diagnostics.

### Pinned libtorrent oracle

The pinned oracle remains libtorrent `2.0.13` at exact commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d` from `reference/pins.toml`.
The design survey inspected:

- `include/libtorrent/peer_info.hpp`, where `total_download` and
  `total_upload` explicitly describe payload-only totals;
- `include/libtorrent/torrent_peer.hpp` and `src/torrent_peer.cpp`, where an
  active connection's counters and a retained prior amount form one peer
  observation;
- `src/torrent.cpp`, where final active payload totals move to retained peer
  state at connection removal;
- `src/peer_list.cpp`, where peer identity/lifecycle replacement preserves or
  clears the matching retained state; and
- `test/test_peer_list.cpp`, whose duplicate, active-retention, reconnect, and
  bounded-list cases establish record-lifecycle edges. The suite does not
  provide a focused assertion for transfer-counter precision, so RSTorrent
  supplies its own exact transition tests.

RSTorrent adopts payload-only semantics and the active-plus-retained mental
model. It intentionally differs by accumulating exact saturating bytes across
all generations retained by one record instead of keeping only coarse KiB
amounts from the previous libtorrent connection.

### JSTorrent product history

The local JSTorrent survey used revision
`0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`:

- `packages/ui/src/tables/SwarmTable.tsx` already presents default-visible
  Downloaded and Uploaded columns; and
- `packages/engine/src/core/swarm.ts` accumulates a connection's download and
  upload counters into its retained peer when the connection is removed.

This confirms the diagnostic value and established product vocabulary. No
JavaScript implementation, fixture, or table code is copied.

## Owner, Task, Cancellation, And Data Flow

```text
PeerRegistry / PeerRecord
  owns closed-generation exact payload totals (maximum 1,000 records)
                  +
PeerRuntime
  owns each active generation's current exact payload totals
                  |
                  v
TorrentPeerState coherent snapshot/removal boundary
  retained base + every active generation mapped to that record
                  |
                  v
existing task-free engine activity sink (100 ms coalescing)
                  |
                  v
ViewHub torrent_swarm exact decimal strings
                  |
                  v
generated contract -> strict live adapter -> Swarm table
```

`PeerRegistry` remains runtime independent and owns only closed-generation
totals. `PeerRuntime` remains the active connection owner. `TorrentPeerState`
is the existing boundary that may read both under one lock: it adds current
active values to immutable snapshots and moves one generation's final values
to the retained base immediately before removing that runtime observation.

The existing outgoing and incoming cleanup paths perform that transfer. There
is no new task, timer, channel, socket, cancellation token, storage write, or
dependency. Registry publication is allowed to compare an enriched snapshot
whenever the already-coalesced Peers observation is due, so live byte changes
become Swarm patches without increasing engine sampling frequency.

Dependency direction remains protocol/content activity -> runtime/registry
owners -> immutable engine snapshot -> session contract -> first-party
clients. Registry code does not depend on async runtime, views, JSON, or UI.

## Invariants And Resource Bounds

- One record adds two `u64` values: 16 logical bytes, at most 16,000 logical
  counter bytes at the existing 1,000-record per-torrent ceiling.
- Every active connection is already bounded by the torrent/session connection
  ceilings. Snapshot enrichment performs bounded work over existing runtime
  observations and registry rows; it creates no retained second catalog.
- Both counters use saturating addition. Converting the current download
  owner's platform-sized counter to `u64` also saturates.
- A live total is `retained_closed_total + sum(active_generation_total)`.
  Finalizing an active generation first adds its last observed total to the
  retained base, then removes that active observation under the same state
  lock. A snapshot can observe either representation but never both.
- A generation is finalized at most once because the runtime removal boundary
  rejects unknown/stale connection IDs. Failure/cancellation before payload
  contributes zero.
- Record replacement/eviction discards the counters with every other record
  field. Inactive publication and torrent removal expose no retained rows.
- The wire fields are nonnegative decimal `u64` strings. Browser validation
  rejects malformed values and sorting uses arbitrary-precision decimal
  comparison rather than clamped JavaScript numbers.
- No counter is persisted. Restart honestly begins new record lifetimes at
  zero.

## Implementation And Validation Sequence

1. Add retained counters and pure registry accumulation/saturation coverage.
2. Add the coherent active-plus-retained snapshot and exactly-once incoming
   and outgoing removal transitions, including overlapping-generation tests.
3. Extend the Swarm session projection and generated portable contract with
   exact decimal strings; cover full snapshot and keyed-patch updates.
4. Extend browser semantic validation, exact live mapping, demo fixtures, and
   default-visible sortable Swarm columns with component tests.
5. Regenerate TypeScript/schema/validators, run focused Rust/web tests, build
   both maintained Android ABIs, then run the proportional repository gates.
6. Install the resulting local headless package through the existing
   transaction/repair path and prove the healthy exact LAN and tailnet routes
   expose the updated Swarm contract. Inspect the current stalled torrent only
   through bounded local read-only observations; do not publish peer endpoints
   or use a public swarm as a completion gate.

The repository gate is:

```text
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
proportional production web/package checks
Android arm64-v8a and x86_64 build checks
```

## Non-Goals

- Diagnosing or changing the actual stalled torrent's selection, backoff,
  discovery, scheduling, request, storage, or retry behavior in this slice.
- Transfer rates, protocol-byte totals, per-connection history, charts,
  session-wide totals, durable persistence, or import/export.
- Joining active Peers rows into Swarm in the client or exposing peer IDs,
  client names, protocol messages, or new endpoint details.
- Per-peer commands, manual retry, bans, scoring, contribution-based policy,
  or reputation.
- A new Android Swarm row table, iOS presentation, public-swarm traffic,
  release tag, public package, updater publication, or firewall/tailnet change.

## Stopping Condition And Escalation

This tactical is complete when scenarios SPT-001 through SPT-007 pass, the
portable contract is regenerated, the proportional first-party/platform gates
pass, the installed local service exposes the two exact counters through both
existing authorities, and the owning topics record the evidence. Tactical
`158` then resumes as the sole **Now**.

Ordinary refactoring, additive tests, generated artifacts, local package
repair, and fixes at these exact ownership boundaries remain authorized. Stop
for direction if evidence requires persistent peer history, policy changes,
new dependencies, a different public compatibility contract, public traffic,
or mutation outside the existing current-machine service deployment.

## Implemented Outcome And Evidence

Completed on 2026-08-27.

- `PeerRecord` owns two saturating `u64` closed-generation counters.
  `TorrentPeerState` publishes their sum with every matching active
  generation and transfers a generation's final authoritative values into the
  record exactly once before removal. Outgoing failure/cancellation/closure
  and routed incoming cleanup all use that boundary. The existing 100 ms peer
  observation cadence drives live Swarm changes without a new task or timer.
- The application `SwarmPeerView`, Rust projection, JSON Schema, generated
  TypeScript and validators, and UniFFI record carry canonical decimal
  strings. Browser validation rejects malformed decimals; the reducer and
  live adapter preserve values beyond JavaScript's safe-integer range.
- React shows default-visible, right-aligned, arbitrary-precision sortable
  **Downloaded** and **Uploaded** columns. Header help states the payload-only,
  retained-record, volatile-lifetime semantics. Wide, 390-pixel, and
  456-pixel fixtures keep every configured Swarm column reachable.
- Deterministic engine coverage passes live contribution, overlapping
  generations, disconnect, stale removal, reconnect accumulation, and
  saturation. A session integration test caught a final-upload sample newer
  than the last coalesced Peers observation; incoming cleanup now publishes
  the authoritative final `UploadCounter` snapshot before retaining it, and
  the exact seven-byte case passes without loss or double counting.
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, and
  `cargo test --workspace` pass. The workspace test run includes 576 engine
  tests with 11 ignored and 256 session tests with two ignored, plus every
  other crate, binary, and documentation test with no failure.
- `npm run generate --prefix clients/web`, typecheck, and the production
  build/CSP scan pass. The complete web suite passes 292 tests with two
  skipped when Node 25's incomplete global Web Storage accessor is disabled
  with `NODE_OPTIONS=--no-webstorage`; the browser application itself uses
  normal Web Storage. The focused Playwright Swarm case passes desktop and
  both phone widths with zero serious or critical accessibility violations.
- `clients/android/build.sh` passes host UniFFI generation, x86_64 and
  arm64-v8a native release builds, the debug APK, and Android unit tests. The
  generated boundary carries both counters; the existing Compose Swarm count
  summary has no peer rows to render, so presentation work is inapplicable to
  this slice.
- The x86_64 headless package validates as 20 files and 69,422,763 bytes with
  SHA-256
  `5009322f3509e79dd7af54e122e77f3d4c0e919d4df2cab4448a14bb410d1b65`.
  Its supported same-version repair retained the enabled exact-access profile
  and health-checked the restarted service. One process now listens only on
  `192.168.1.129:3030` and `127.0.0.1:3031`; direct LAN, loopback with the
  configured tailnet authority, and
  `https://zblinux.tail71bc5c.ts.net:8445/healthz` all return the exact healthy
  `rstorrent-headless` `0.1.1` identity.
- A Playwright run against the installed tailnet HTTPS bundle passes the full
  Swarm desktop/phone lifecycle case. A bounded read-only live inspection
  sees both exact columns, no browser errors, and the two restored torrents'
  25- and 20-row Swarm catalogs. The service restart correctly begins their
  new volatile counters at zero; prior-process transfer history is not
  fabricated.
