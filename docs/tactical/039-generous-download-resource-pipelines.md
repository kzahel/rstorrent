# Generous Download Resource Pipelines

Status: Complete.

Topic: `performance-and-live-evidence`

## Motivation

The product application inherited a 32 KiB `max_buffered_payload_bytes`
allowance. The swarm scheduler charged every outstanding 16 KiB peer request
against that allowance and retained the charge until its payload finished a
storage write. Consequently the whole torrent could reserve only two ordinary
blocks even though each established peer starts with a four-request window and
may adapt to 500 requests. Headless campaign tools often supplied larger
limits, so their evidence did not expose the desktop product default.

This slice corrects the resource model and makes the desktop and Android
product profiles explicit. It does not claim transfer parity from larger
limits alone.

## Scope

- Separate lightweight outstanding peer-request reservations from received
  payload retained by the storage pipeline.
- Give desktop a 256 MiB outstanding-request allowance, 32 MiB received
  payload allowance, and 256 MiB active-piece working set.
- Give Android a 128 MiB outstanding-request allowance, 16 MiB received
  payload allowance, and 128 MiB active-piece working set.
- Replace the fixed 64-active-piece gate with a byte-oriented working-set
  bound while always allowing one accepted piece larger than the configured
  bound to make progress.
- Keep the existing three-second adaptive per-peer request target, initial
  four-request window, 500-request per-peer maximum, block integrity rules,
  storage write batching, cancellation, and verification fences.
- Expose distinct request and resident-payload high-water diagnostics.
- Prove product profiles can fill every initial peer window and that resident
  payload remains independently bounded under delayed storage.

## Non-goals

- Raising established or half-open connection limits, adding socket creation
  throttling, or changing peer replacement policy.
- Concurrent storage workers, mmap, whole-piece resident assembly, a disk
  cache, or session-wide multi-torrent memory arbitration.
- Transfer-speed claims from public swarms, UI changes, incoming service,
  seeding, uTP, or Android network policy controls.

## Reference review

Pinned libtorrent `2.0.13` revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `src/settings_pack.cpp` sets `request_queue_time=3`,
  `max_out_request_queue=500`, and `max_queued_disk_bytes=1 MiB` independently.
- `include/libtorrent/peer_connection.hpp` initializes the desired request
  queue to four requests.
- `src/peer_connection.cpp::update_desired_queue_size()` derives each peer's
  target from three seconds of measured payload rate, bounded by the per-peer
  maximum; accepted blocks increase the slow-start target.
- `src/request_blocks.cpp::request_a_block()` fills the peer request queue and
  applies whole-piece preference without preallocating one payload buffer per
  outstanding request.
- `src/peer_connection.cpp` around queued disk writes stops socket reads when
  the received disk queue is over its independent byte limit.
- `include/libtorrent/settings_pack.hpp` documents that an undersized disk
  queue severely limits download rate.
- `test/test_settings_pack.cpp` asserts the 500-request default and settings
  round trips; `test/test_session.cpp` asserts the three-second request-queue
  default and reset behavior. The reference suite does not directly unit-test
  the complete adaptive formula at this pin.

Relevant JSTorrent product history at the local `main` sibling:

- `packages/engine/src/core/active-piece-manager.ts` separates active-piece
  memory from per-peer requests and uses a 256 MiB desktop/ChromeOS default
  with a smaller standalone-mobile profile.
- `packages/engine/src/core/peer-connection.ts` independently implements a
  three-second adaptive request pipeline.

RSTorrent keeps its independently authored state machine and writes accepted
blocks promptly rather than retaining complete pieces in memory. The adopted
behavior is the separation of request promises, resident received payload,
and piece-selection working state; libtorrent and JSTorrent internals are not
copied.

## Owners and lifecycle

- `SwarmState` owns outstanding request attempts, their byte reservations,
  the active-piece working set, and release on receive/cancel/timeout/choke or
  disconnect.
- `ContentStoragePipeline` owns accepted block buffers from enqueue through
  completion. Its torrent-local byte-derived job bound backpressures peer
  event intake and is cleared on joined shutdown.
- `DownloadControl` observes, but does not own, current and high-water request
  and resident-payload counters.
- `ApplicationConfig` selects the desktop profile by default. The Android
  adapter deliberately replaces it with the Android profile before opening
  the in-process service.

No new background task is introduced. Existing peer, storage, discovery, and
application cancellation and join paths remain authoritative.

## Invariants and bounds

- Outstanding request bytes never exceed their configured limit and are not
  retained merely because received payload is waiting for storage.
- Received payload bytes never exceed their configured limit. Verification
  jobs consume scheduling capacity conservatively but own no payload bytes.
- Each accepted block transfers exactly once from peer-message ownership to
  storage ownership and releases its resident charge exactly once on storage
  completion or joined shutdown.
- Duplicate, unsolicited, rejected, canceled, expired, and disconnected
  request attempts release only reservations they own.
- The byte-oriented active working set cannot activate another ordinary piece
  above its bound, but one individually oversized piece remains progressable.
- Desktop can reserve at least `30 * 4 * 16 KiB` so every default established
  peer can receive its initial pipeline. Android satisfies the same invariant.
- Profile constants are reservation and queue bounds, not claims that all
  bytes are committed eagerly.

## Validation

1. Deterministic swarm tests distinguish request release at payload receipt
   from storage completion, cover cancellation/duplicate ownership, fill the
   configured request allowance, and exercise byte-oriented piece activation.
2. Delayed-storage runtime tests show request high water can exceed resident
   payload high water while both remain within independent limits.
3. Desktop and Android product tests assert the exact platform profiles and
   initial-window capacity invariant.
4. Existing controlled storage, cancellation, endgame, corruption, resume,
   session, gateway, web UI, and Android tests remain green.
5. Run formatting, warning-denying workspace clippy, workspace tests, and both
   configured Android target checks.

## Stopping condition

The slice completes when product launches no longer inherit the two-block
limit, the three resources have distinct owners and diagnostics, deterministic
tests prove the bounds and release transitions, all proportionate green gates
pass, living performance documentation records the correction, and the tree
is committed cleanly.

## Result

`DownloadResourceLimits` now owns explicit desktop and Android profiles and is
selected at the application boundary. `SwarmState` reserves only outstanding
request bytes and releases them when accepted payload leaves the peer-message
owner. `ContentStoragePipeline` independently charges resident payload through
write completion, derives its job capacity from that byte allowance, and
backpressures supervisor intake at the same boundary. Active piece selection is
also byte-oriented and permits one individually oversized valid piece so a
small profile cannot deadlock on accepted metainfo.

Engine, probe, session, desktop, Android, and Kotlin diagnostics expose the
separate limits and high waters. The Android binding build also found two stale
exhaustive reducers and one stale fixture constructor left by the already
landed peer-view contract; they now explicitly ignore unsubscribed peer views
and compile against the current torrent projection.

## Evidence

- `product_profiles_are_generous_and_fill_every_initial_peer_window` proves
  both profiles can issue four ordinary blocks to all 30 default peers.
- `generous_request_allowance_fills_every_default_initial_peer_window` proves
  exact swarm-level reservation and release accounting.
- `active_piece_working_set_is_byte_bounded_and_refills_after_verification` and
  `one_piece_larger_than_the_working_set_limit_can_still_progress` prove the
  new selection bound and liveness exception.
- `request_pipeline_exceeds_independently_bounded_resident_payload` uses
  delayed storage to prove request high water can exceed a two-block resident
  test budget while both resources stay within their own limits.
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, and
  `cargo test --workspace` pass. The workspace suite includes 150 engine tests
  passing and three opt-in public tests ignored.
- `experiments/android-engine-bootstrap/build.sh` compiled release libraries
  for `x86_64-linux-android` and `aarch64-linux-android` and regenerated both
  UniFFI Kotlin surfaces. Gradle `assembleDebug testDebugUnitTest` then passed.

No visible product client or public swarm was launched. The slice makes no
comparative throughput claim until representative live evidence is rerun under
the corrected product profile.
