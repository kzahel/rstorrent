# Tactical 014: Scheduled UDP Tracker Lifecycle

Status: completed.

## Motivation And Outcome

The first routed desktop tracker exercise reached the public UDP path, waited
five seconds for one response, walked the magnet's retained tracker URLs once,
and then reported externally blocked discovery. That behavior was the
deliberate stopping point of tactical `011`; it is not a production tracker
lifecycle.

Replace the one-shot cursor with a bounded scheduled UDP tracker owner:

- every supported tracker retains independent attempt, failure, success, and
  next-announce state;
- temporary tracker and endpoint failures fall through to other trackers and
  remain eligible under bounded backoff;
- successful responses schedule regular reannounce from the tracker interval;
- UDP exchanges retransmit once before their bounded completion deadline and
  reuse unexpired connection IDs;
- tracker results continue to enter the existing peer registry through a
  bounded channel while peer work is active;
- scheduled automatic discovery is presented as waiting rather than blocked;
  and
- web/Tauri and Android expose equivalent structured tracker diagnostics
  through the existing application contract.

This slice makes retained UDP trackers durable automatic discovery sources for
the lifetime of an active torrent. It does not promise that a changing public
tracker or swarm will respond.

## Dependencies And References

- [`../engineering-principles.md`](../engineering-principles.md)
- [`../topics/tracker-discovery.md`](../topics/tracker-discovery.md)
- [`../topics/peer-lifecycle.md`](../topics/peer-lifecycle.md)
- [`../topics/application-control.md`](../topics/application-control.md)
- [`011-one-shot-udp-tracker.md`](011-one-shot-udp-tracker.md)
- [`012-bounded-diagnostics-progress.md`](012-bounded-diagnostics-progress.md)
- [`013-explicit-live-network-policy.md`](013-explicit-live-network-policy.md)
- BEP 12: Multitracker Metadata Extension
- BEP 15: UDP Tracker Protocol for BitTorrent
- Rasterbar libtorrent `v2.0.13` tracker entries, announce scheduling,
  backoff, and UDP tracker transport

No reference source or fixture is copied. RSTorrent independently implements
the public protocol and tests observable behavior with its own scripted
trackers.

## Reference Findings

BEP 15 makes UDP loss recovery the client's responsibility. It recommends
retransmission after `15 * 2^n` seconds and permits a connection ID to be
reused for 60 seconds. A successful announce response supplies the interval
before the next ordinary announce.

BEP 12 treats tracker URLs as ordered tiers. URLs within a tier are shuffled
when loaded, failures fall through the tier before later tiers, and a
successful URL moves to the front. Magnet `tr` parameters have no tier syntax,
so this tactical treats all retained magnet UDP trackers as one synthetic tier.

Libtorrent keeps failure count, last result, next announce, minimum announce,
in-flight state, and started/completed state independently per tracker and
info hash. Its default tracker failure limit is unlimited. Its default
failure delay follows:

```text
delay = 5 + 5 * 250 / 100 * consecutive_failures^2 seconds
```

The delay is capped at 60 minutes. Successful tracker intervals are clamped to
a minimum of five minutes to avoid hammering a misconfigured tracker.

RSTorrent follows those scheduling and fallback semantics without adopting
libtorrent's class graph, listen-socket matrix, hybrid-torrent state, or
settings surface.

## Scope

### Pure tracker schedule

Add a runtime-independent state owner containing a bounded ordered set of
tracker records. Each record retains:

- a stable bounded identifier and UDP URL;
- synthetic tier zero and source from the magnet;
- consecutive and total attempt counts;
- whether a valid started announce has succeeded;
- last success and failure times;
- next eligible announce time; and
- the current clamped regular interval.

The scheduler accepts explicit monotonic time. Runtime randomness shuffles the
initial URL list before construction; deterministic tests supply a known
order. State transitions select an eligible tracker, record failure, record
success, promote a successful tracker, or return the exact duration until the
next automatic action.

A failure increments a saturating count, retains unlimited future eligibility,
and applies the libtorrent-style quadratic delay capped at 60 minutes. A valid
response resets the failure count, changes later announce events from
`started` to ordinary, promotes the tracker, and schedules the response
interval clamped to five minutes through 24 hours. The upper bound prevents
hostile response data from suppressing the automatic owner indefinitely.

### Supervised per-torrent tracker owner

One child task per active magnet torrent owns the schedule, connection-token
cache, DNS and UDP operations, and retry timer. It sends only bounded accepted
tracker results to the `PeerSession` through a bounded channel. The
`PeerSession` remains the only owner that validates compact endpoints and
mutates the peer registry.

The parent download owner supplies cancellation, explicitly stops and joins
the tracker child on every normal or error return, and retains an aborting drop
guard only for panic or unexpectedly abandoned futures. Channel closure and
cancellation must interrupt a blocked send, timer, DNS operation, or UDP wait.

The tracker owner begins with the active torrent rather than only after peer
hint exhaustion. It continues periodic announce while metadata or content
work uses a peer. Tracker response backpressure is bounded independently of
swarm size and torrent lifetime.

### UDP operation

For each selected URL:

1. Resolve at most 32 addresses under the active network policy.
2. Try allowed resolved addresses sequentially.
3. Bind one address-family-matched socket.
4. Reuse an unexpired connection ID for the exact remote endpoint, or perform
   the connect exchange.
5. Send the request immediately, retransmit once after 15 seconds of silence,
   and fail the endpoint after a 30-second aggregate exchange deadline.
6. Ignore unrelated, undersized, and stale-transaction datagrams inside the
   deadline while rejecting malformed correlated responses.
7. Cache a valid connection ID for 60 seconds in a bounded cache.
8. Announce `started` until one valid started response succeeds, then send
   ordinary announces.

The existing stable per-torrent key, 200-peer response bound, 16 KiB
unknown-left value, zero counters, and zero listening port remain. Completed,
stopped, accurate transfer statistics, and an incoming listener require
owners not established by this tactical and are not fabricated.

### Multiple trackers and fallback

All supported magnet UDP trackers form one shuffled tier. A failed tracker or
resolved endpoint falls through immediately to another eligible tracker.
After every tracker in the round has failed, the owner sleeps until the
earliest record is eligible and starts a new round.

A valid tracker response, including a valid zero-peer response, completes the
round. It is not converted into tracker failure merely because reported peers
are absent, invalid, unreachable, or later fail. The next round starts with
the promoted tracker at its successful reannounce interval.

### Progress and diagnostics

Extend the existing typed engine activity sink with tracker and peer-discovery
facts. The application maps them into bounded diagnostics:

- `tracker_announce_started`
- `tracker_udp_retransmitted`
- `tracker_announce_failed`
- `tracker_fallback_selected`
- `tracker_retry_scheduled`
- `tracker_reannounce_scheduled`
- `tracker_announce_succeeded`
- `tracker_peers_unavailable`
- `peer_dial_started`

Tracker identity is a sanitized bounded `udp://host:port` label. Failure text
is bounded by the existing diagnostic context limits. Future transports must
redact credentials and passkeys rather than treating full URLs as safe labels.

While a tracker operation is active, awaiting-metadata progress is active
discovery. While every tracker is sleeping under backoff or a successful
interval and no peer can advance metadata, progress is
`waiting/discovery/waiting_for_discovery`. It is blocked only when networking
is disabled or no supported enabled source has a future automatic action.

The engine task may remain alive while the progress projection reports
waiting. Task existence alone must not override explicit discovery activity.

## Contracts And Invariants

- Tracker schedule transitions remain independent from Tokio, sockets, DNS,
  random generation, wall-clock time, and application views.
- Each retained supported tracker remains eligible indefinitely unless the
  torrent or network owner is cancelled.
- Failure of one tracker or address cannot discard another tracker, existing
  peer observation, or torrent intent.
- A successful response interval and a failed retry delay both have explicit
  lower and upper bounds.
- At most one UDP tracker operation runs per torrent in this tactical.
- Tracker result and diagnostic queues remain bounded independently from
  response count and torrent lifetime.
- The peer registry remains the only owner of peer records and policy checks.
- Every tracker child has cancellation, observable termination, and a join
  path.
- A tracker timeout or rejection does not become torrent failure or externally
  blocked progress while an automatic retry exists.
- Diagnostic strings are never parsed to decide tracker or progress state.

## Nasty Cases Required Up Front

- exact first, second, large, and saturated failure delays;
- success resets failures, promotes the tracker, clamps short and enormous
  intervals, and changes `started` to ordinary announce;
- one, several, and all trackers failing, including a prior tracker becoming
  eligible while a later operation is running;
- valid zero-peer response ending the fallback round;
- dropped first connect and announce requests recovered by retransmission;
- connection-ID reuse before 60 seconds and reconnect after expiry;
- stale transactions, malformed correlated packets, tracker error responses,
  and alternate resolved-address failure;
- invalid or policy-denied compact peers cannot bypass registry validation;
- cancellation during DNS, UDP wait, timer sleep, bounded channel send, peer
  work, pause, and shutdown;
- retry activity concurrent with Activity recreation or browser subscriber
  resynchronization;
- a torrent with no supported tracker and exhausted peer hints remains
  blocked, while the same torrent with one retrying tracker remains waiting;
  and
- no public Internet dependency in deterministic gates.

## Non-Goals

- HTTP, HTTPS, WebSocket, authenticated, proxied, or BEP 41 tracker transports
- parsing `.torrent` `announce-list` tiers or exposing announce-all settings
- DHT, PEX, LSD, WebSeeds, incoming peers, uTP, or NAT traversal
- completed, stopped, scrape, real transfer counters, or a nonzero listen port
- persistence of volatile failure and timer state across process restart
- more than one simultaneous tracker operation per torrent
- a mature session-wide active-torrent or tracker-operation budget
- guaranteeing the recorded Big Buck Bunny trackers or swarm are reachable

## Validation

Run:

```bash
source ~/.profile

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

npm ci --prefix clients/web
npm run generate --prefix clients/web
git diff --exit-code -- \
  clients/web/src/generated/contract.ts \
  clients/web/src/fixtures/reactive-trace.json
npm run typecheck --prefix clients/web
npm test --prefix clients/web
npm run build --prefix clients/web

clients/web/node_modules/.bin/tauri build \
  --config clients/desktop/src-tauri/tauri.conf.json --no-bundle

cargo clippy -p rstorrent-android --all-features -- -D warnings
cargo test -p rstorrent-android --all-features
clients/android/build.sh

uv run --project tests/interop --locked \
  python tests/interop/udp_tracker_magnet.py --runs 3
uv run --project tests/interop --locked \
  python tests/interop/browser_diagnostics_surface.py \
  --chrome "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
uv run --project tests/interop --locked \
  python tests/interop/android_diagnostics_surface.py \
  --avd jstorrent-dev --headless

python3 scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

The browser gate must use headless Chrome and the loopback gateway without
starting Tauri. Android device validation must use an explicitly owned
no-window AVD and may not select an attached physical device.

The ignored bounded public metadata probe may be run once as current evidence.
Its success is not required and its failure must leave no artifact.

## Stopping Condition

Stop when deterministic tests prove repeated tracker lifecycle, bounded UDP
loss recovery, token reuse, multi-tracker fallback and promotion, correct
waiting progress, and cancellation/join behavior; controlled libtorrent
transfer remains green; equivalent waiting/retry diagnostics render in
headless Chrome and the owned no-window Android AVD; all repository gates pass;
the living topics and entry points describe the new capability honestly; and
the completed implementation is committed.

## Execution Record

### Implemented lifecycle

The engine now has a runtime-independent tracker schedule with one record per
retained magnet UDP tracker. URLs are shuffled into one synthetic tier.
Records retain attempts, consecutive failures, started acknowledgement,
success and failure time, bounded interval, and next eligibility. Failure
falls through the tier and uses the recorded quadratic delay with unlimited
future eligibility and a 60-minute cap. Success resets and promotes the
record, distinguishes routine reannounce from failure retry, and clamps the
reported interval to five minutes through 24 hours.

One supervised task per active magnet owns that schedule, DNS and UDP
operations, a bounded 60-second connection-token cache, and a four-result
channel. The peer session alone validates results and mutates the peer
registry. Normal completion, failure, cancellation, pause, and shutdown cancel
and join the child. An aborting drop guard remains for abandoned or panicking
futures.

UDP connect and announce exchanges retransmit once after 15 seconds and fail
after one aggregate 30-second deadline. Scripted tests shorten only those
timings. Started is repeated until a valid response; later announces use the
ordinary event. A valid zero-peer response is success, schedules reannounce,
and separately reports that no peer is currently eligible.

The application maps typed attempt, retransmit, failure, fallback, failure
retry, successful reannounce, success, unusable response, and peer-dial facts
into the bounded diagnostic stream. Failure retry and routine reannounce no
longer overwrite one another. A retrying tracker with no eligible peer renders
`waiting/discovery/waiting_for_discovery`; blocked remains reserved for no
enabled automatic source or disabled networking.

### Deterministic validation

These gates passed:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

npm ci --prefix clients/web
npm run generate --prefix clients/web
git diff --exit-code -- \
  clients/web/src/generated/contract.ts \
  clients/web/src/fixtures/reactive-trace.json
npm run typecheck --prefix clients/web
npm test --prefix clients/web
npm run build --prefix clients/web

clients/web/node_modules/.bin/tauri build \
  --config clients/desktop/src-tauri/tauri.conf.json --no-bundle

cargo clippy -p rstorrent-android --all-features -- -D warnings
cargo test -p rstorrent-android --all-features
clients/android/build.sh

cargo tree --workspace --locked
git diff --check
```

The workspace passed 53 engine tests with the changing public probe ignored,
50 protocol, 28 session, six Android, two gateway, three engine-CLI, two
session-CLI, one desktop, one metadata-seed, and one architecture test plus
doc tests. Engine coverage includes exact backoff, saturation, fallback,
promotion, interval bounds, earliest-retry identity, zero-peer success,
started-to-ordinary transition, dropped connect and announce packets, token
reuse and expiry, and cancellation with socket release.

The web client passed type checking, six tests with one fixture-dependent test
skipped, generated-contract drift checking, and its production Vite build.
Tauri produced `target/release/rstorrent-desktop` without launching it.

Android validation rebuilt release libraries for API 28 x86_64 and arm64-v8a,
regenerated both UniFFI packages, passed the Rust and Compose/JVM tests, and
assembled the debug APK. The Kotlin compiler repeated existing deprecated
Android API warnings and reported no new failure.

### Controlled interoperability and presentation

Rasterbar libtorrent `2.0.13.0` passed three consecutive tracker-only magnet
runs against the final implementation. Every run observed exactly one connect
and one announce, acquired the 26,686-byte two-block metadata dictionary,
verified all three content pieces and 40,000 payload bytes, and cleaned every
process and artifact.

Headless Chrome over the loopback gateway rendered the controlled unavailable
tracker as `waiting/discovery/waiting_for_discovery`, exposed
`tracker_announce_started`, `tracker_announce_failed`, and
`tracker_retry_scheduled`, exercised detailed-profile and tracker-category
filters, opened no public socket, and cleaned the browser, gateway, and
profiles.

The owned `jstorrent-dev` no-window AVD passed on API 34 arm64-v8a with the
same progress and diagnostic result. It exercised profile/category controls,
survived Activity recreation and backgrounding, joined foreground-service
stop, cleared package state, and shut down the emulator. Browser and Android
screenshots were visually inspected and remained temporary validation
artifacts. No attached physical device, visible emulator, Tauri process,
desktop window, or host GUI automation was used.

The ignored Big Buck Bunny public probe was not rerun. Changing public tracker
or swarm availability is not a deterministic gate.

### Reference and dependency status

`cargo tree --workspace --locked` passed and the implementation introduced no
dependency. `python3 scripts/references.py status` was run but reported the
same machine-local state recorded by tactical `013`: optional
`bittorrent-beps`, `rqbit`, and `libtorrent` checkouts are absent, and the
sibling JSTorrent checkout has unrelated maintainer changes. None of those
external trees or changes was modified.
