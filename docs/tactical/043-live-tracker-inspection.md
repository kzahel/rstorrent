# Live Tracker Inspection

Status: In progress.

Topics: `tracker-discovery`, `application-view-api`, `web-ui-design`,
`desktop-inspection-surface`, `performance-and-live-evidence`

## Motivation

The web inspection surface can show torrent, peer, and file state but its
Trackers tab is still a placeholder. Tracker diagnostics explain individual
attempts, yet logs are not an authoritative current-state API: they cannot
truthfully answer which trackers are announcing, waiting to retry, waiting to
reannounce, inactive, or carrying the last accepted response.

This slice uses the Trackers table to establish that missing owner. It extends
the deterministic tracker schedule with bounded state that is logically part
of each tracker record, publishes immutable snapshots through the existing
leased view-set API, and renders them in the shared responsive virtual table.
It does not reconstruct state from diagnostics or make the session view model
a second tracker authority.

The existing optional **Re-sort live updates** table behavior remains intact.
RSTorrent performs an exact full sort while it is enabled and freezes the
current row order while it is disabled. It does not adopt JSTorrent's limited
two-adjacent-swaps-per-tick algorithm.

## Scope

- Extend the runtime-independent `TrackerSchedule` record state with the
  bounded response, error, and timing values needed for truthful inspection.
- Define an immutable engine tracker snapshot that describes every configured
  tracker, the schedule's current action, and whether the manager is active.
- Publish an initial active snapshot, every schedule transition, and one
  terminal inactive snapshot only after in-flight operations are cancelled
  and joined.
- Retain that projection in the application service, rebuild configured
  inactive tracker rows from durable magnets after restart, and preserve live
  state across unrelated durable piece checkpoints.
- Add `torrent_trackers` snapshots and keyed patches to the generated Rust,
  TypeScript, JSON Schema, UniFFI, and Kotlin contract.
- Add semantic frontend interest, strict validation, pure reduction, Zustand
  materialization, view eviction and lease-recovery behavior.
- Implement a responsive virtual Trackers table in the live and named-demo
  web surfaces, with a local one-second countdown derived from the server's
  remaining duration.
- Prove the projection with deterministic schedule/view tests and a controlled
  loopback UDP tracker plus libtorrent seed through the headless browser
  gateway.
- Record the implemented state, evidence, and remaining discovery gaps in the
  owning topics.

## Non-goals

- HTTP, HTTPS, or WebSocket trackers; metainfo `announce`/`announce-list`
  ingestion; tier failover beyond the current magnet source; scrape; or
  tracker editing.
- Force announce, add, remove, enable, disable, or reorder tracker commands.
- Persisting tracker failures, response statistics, or schedule deadlines
  across process restart.
- A cumulative per-tracker unique-peer set. The current peer registry does not
  retain tracker-URL provenance, and adding a long-lived set solely for the UI
  would create a false owner and unnecessary memory cost.
- Incoming listening, real transfer-counter announces, UPnP, or changing the
  current compatibility port policy.
- Public-swarm evidence, performance comparison, or a new protocol-support
  claim.
- An Android Compose Trackers screen. Shared generated contracts and Android
  builds must remain valid; the current native UI may explicitly ignore this
  view until a separate product slice.

## Reference Review

### Specification

`reference/bittorrent.org/beps/bep_0015.rst` defines the UDP connect and
announce transactions, interval, leecher/seeder counts, compact peer list,
transaction matching, retransmission, and exponential timeout expectations.
The existing codec and operation owner already implement this bounded wire
exchange; this tactical retains the accepted response fields instead of
discarding them after peer admission.

### Pinned libtorrent oracle

Pinned libtorrent `2.0.13` revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/announce_entry.hpp` retains per-URL message,
  `last_error`, next/min announce deadlines, scrape seed/leecher/download
  counts, failure count, updating state, and lifecycle-event acknowledgements.
- `src/announce_entry.cpp::{reset,failed,can_announce,working}` defines the
  record transitions and retry eligibility independently of sockets.
- `src/torrent.cpp` tracker selection and response/failure paths around
  `announce_with_tracker`, `tracker_response`, and `tracker_request_error`
  keep schedule state with the announce entry, clear failures on success, and
  choose tiers and retries without deriving truth from alerts.
- `test/test_tracker.cpp` and `test/udp_tracker.cpp` cover tracker lifecycle,
  timeout/error response, transaction validation, interval handling, and
  fallback behavior.

RSTorrent adopts the per-record ownership, retained accepted response, and
explicit in-flight/wait states. It does not copy libtorrent's alert, torrent,
or tracker-manager architecture, scrape vocabulary, tier breadth, or
connection policy.

### JSTorrent product reference

Local JSTorrent `main` revision
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/engine/src/interfaces/tracker.ts::TrackerStats` supplies the useful
  product vocabulary: URL, type, status, interval, seeders/leechers, peers from
  the last response, next announce, and last error.
- `packages/ui/src/tables/TrackerTable.tsx` establishes a familiar dense
  tracker information hierarchy.
- `packages/engine/src/tracker-manager.ts`, `udp-tracker.ts`, and
  `http-tracker.ts` show product-level lifecycle observations and known
  multi-transport expectations.
- `packages/ui/src/tables/VirtualTable.solid.tsx` implements its optional live
  sort as a deliberately gradual insertion step. RSTorrent keeps its own exact
  comparator and live-sort policy instead.

No source or fixture is copied.

## Vocabulary And Runtime Contract

`TrackerSchedule` remains the single mutable authority for configured tracker
records and deterministic transitions. Each record retains:

- canonical tracker ID/URL, tier, source, and transport;
- whether an announce operation is currently in flight and which event it is
  sending;
- total attempts and consecutive failures;
- last accepted response peer, seeder, and leecher counts;
- last accepted announce interval;
- bounded last error, at most 256 UTF-8 bytes after safe truncation;
- last success and failure times; and
- next eligible schedule time and whether it represents retry or reannounce.

The initial transport/source vocabulary is `udp` and `magnet`; enums reserve a
clean extension boundary without pretending unsupported transports exist.
Canonical normalized URL is the stable row identity because current magnet
parsing deduplicates it and the schedule owns one record per URL.

Runtime status is one of:

- `inactive`: the manager is not running;
- `idle`: active but not yet attempted and eligible now;
- `announcing`: an operation is in flight;
- `retry_wait`: a failed record is waiting for its retry deadline; or
- `reannounce_wait`: a successful record is waiting for its accepted
  reannounce deadline.

The next action is `announce`, `retry`, or `reannounce`. The snapshot carries
remaining durations and ages captured against the same monotonic instant;
wall-clock timestamps are not fabricated. A first-run record has no accepted
interval or response counts. Success clears the error and consecutive failure
count. Failure retains the previous successful response statistics but updates
the error and retry deadline.

The schedule exposes immutable snapshots and contains no Tokio, channel,
socket, filesystem, session, Serde, or UI types. Snapshot publication is not a
second state mutation path.

## Owner, Task, Cancellation, And Data Flow

```text
TrackerSchedule (pure mutable authority)
       |
       | immutable snapshots after transitions
       v
tracker-manager task -> typed activity sink -> application tracker model
                                               |
                                       ViewHub / leased view set
                                               |
                                  validated TS reducer / Zustand
                                               |
                                     virtual Trackers table
```

- `TrackerSchedule` owns records and their transitions.
- The tracker-manager task owns UDP operations, token cache, operation budget,
  clock sampling, cancellation, and publication ordering.
- At manager start it publishes active state. Selecting an action marks the
  record announcing before publication. A response or failure mutates the
  schedule first and then publishes one complete immutable snapshot.
- Manager shutdown first cancels, aborts, and joins all operation tasks. An
  outer owner then publishes exactly one inactive snapshot, preserving last
  bounded record values while clearing all in-flight status. Every return path
  crosses this terminal publication boundary.
- Diagnostics remain ordered observations for logs. The application tracker
  view consumes the typed snapshot event and never parses diagnostic text.
- The application model is a read-only retained projection. It reconstructs
  inactive configured rows from the durable canonical magnet after process
  restart and loses volatile history honestly. Replacing durable checkpoint
  data with the same tracker catalog preserves the current runtime projection.
- Closing/removing a torrent evicts its tracker model through the existing
  torrent lifecycle owner. View-set removal evicts only the browser replica.

The existing schedule bound of 32 magnet trackers and manager-wide maximum of
eight concurrent operations remain unchanged. BEP 15 responses already cap
compact peers at 200. No additional unbounded histories, peer sets, or tasks
are introduced.

## View Contract

Add `ViewSpec::TorrentTrackers` and capability `torrent_trackers`. Its
conceptual payload is:

```text
TrackerView {
    tracker_id: String,
    url: String,
    transport: udp,
    source: magnet,
    tier: u32,
    status: inactive | idle | announcing | retry_wait | reannounce_wait,
    announce_event: Option<started | update>,
    total_attempts: u32,
    consecutive_failures: u32,
    last_peer_count: Option<u32>,
    seeders: Option<u32>,
    leechers: Option<u32>,
    interval_seconds: Option<u32>,
    next_action: Option<announce | retry | reannounce>,
    next_action_in_millis: Option<decimal-u64 String>,
    last_success_age_millis: Option<decimal-u64 String>,
    last_failure_age_millis: Option<decimal-u64 String>,
    last_error: Option<String>,
}

ViewSnapshot::Trackers {
    torrent_id,
    state: available | torrent_missing,
    trackers: Vec<TrackerView>,
}

ViewPatch::Trackers {
    torrent_id,
    upsert: Vec<TrackerView>,
    removed: Vec<tracker_id>,
}
```

An available empty list means the torrent has no configured tracker. Unknown
or older servers produce the existing explicit unsupported materialization,
not an empty list. A 250 ms requested minimum delivery is sufficient for state
transitions; the browser derives a local deadline on receipt and repaints the
visible countdown once per second without backend timer patches.

The selected Trackers tab alone requests this view. Switching detail tabs
removes and evicts it. Suspension, lease expiry, application restart, and
cursor reset follow the existing fresh-snapshot recovery contract.

## Presentation Contract

Default columns are URL, Status, Tier, Peers, Seeds, Leeches, Next announce,
and Error. Optional columns include Transport, Source, Event, Attempts,
Failures, Interval, Last success, and Last failure. Numeric and duration
columns use the shared typed comparator, nulls remain consistently last, and
row ties are stable.

The table is read-only, responsive, keyboard accessible, virtualized, and
uses the existing persisted column visibility, width, sort, and live-sort
facilities. Compact and phone layouts reduce default columns without hiding
the selected tracker's full values from future progressive detail. An error is
text, not the sole status signal.

The permanent named `tracker-recovery` demo scenario exercises announcing,
failure, retry wait, success, and reannounce states without a backend. It uses
the same semantic adapter and store as live mode.

## Implementation Order And Gates

1. Land this decision-complete tactical and reference record.
2. Extend pure schedule state/snapshots and cover initial, in-flight, failure,
   success, retry, reannounce, and inactive transitions deterministically.
3. Publish snapshots through the manager and prove terminal publication after
   operation join/cancellation.
4. Add the application tracker model, view-set snapshot/patch behavior,
   generated contracts, eviction, and recovery tests.
5. Add live/demo frontend validation, reducer/store handling, local deadline
   derivation, and responsive table tests.
6. Run one isolated controlled UDP tracker/libtorrent browser proof and the
   proportional repository gates; update topics and close the tactical.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Schedule snapshots for untouched, announcing, failed/retry, successful/reannounce, repeat failure, recovery, and inactive states; accepted counts and error bounds |
| Runtime | Scripted tracker manager success/failure/cancellation proves transition order, operation bounds, and terminal inactive publication after joins |
| Application views | Snapshot, keyed diff, torrent missing/removal, same-catalog durable refresh, view eviction, queue/reset, and lease recovery |
| Generated clients | Generation check plus TypeScript/schema/Kotlin/UniFFI compilation and explicit Android ignore handling |
| Frontend | Strict decoder/reducer/store tests; typed/null sorting; local countdown; demo transitions; virtualized scale and responsive keyboard/accessibility checks |
| Controlled interoperability | Headless loopback UDP tracker returns exact peer/seeder/leecher/interval values, libtorrent seeds content, browser observes tracker state and verified completion, all children join, artifacts removed |
| Repository | `cargo fmt --all -- --check`, warning-denying workspace Clippy, workspace tests, web typecheck/tests/build, deterministic Playwright, and proportional Android build/tests |

No public network or visible application launch is authorized or required.
Headless servers must use isolated ports and state so the maintainer's running
`./scripts/webui` instance is not interrupted.

## Stopping Condition

The slice is complete when a configured magnet UDP tracker is visible in the
selected torrent's Trackers tab before its first response, each schedule
transition and retained response value is truthful, manager termination cannot
leave an announcing row, suspension/lease recovery produces a coherent fresh
view, the controlled tracker-to-libttorrent transfer completes through the
headless browser proof, all proportional gates pass, evidence is recorded,
and the working tree is clean.

## Escalation Contract

Ordinary schedule refactoring, new pure snapshot types, generated-contract
changes, focused session-module extraction, bounded error truncation, demo
fixtures, test-harness extensions, and same-boundary fixes are authorized.
Stop for direction if evidence requires a new tracker transport, persistence
migration, public remote authentication policy, command/action semantics,
public-swarm traffic, a new dependency with material tradeoffs, visible app or
physical-device interaction, or a tracker architecture that materially moves
authority away from the accepted deterministic schedule.
