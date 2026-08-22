# Tactical 136: Shared Tracker Operation Executor

Status: **Completed 2026-08-11**. Explicit maintainer authorization on
2026-08-11 selected the focused resumable driver's missing HTTP(S)
composition, required this source-first tactical, and authorized end-to-end
implementation plus logical commits.

Topics: `tracker-discovery`, `capability-readiness`,
`oracle-driven-engine-campaign`, `public-torrent-testing`

## Decision And Motivation

Tactical [`122`](122-paired-public-download-performance-cohorts.md) exposed a
real alternate-engine-entry-point gap rather than an HTTPS wire failure. The
focused public probe passed retained HTTP and HTTPS `TrackerConfig` rows into
the resumable driver's nested tracker manager, but the manager rejected every
non-UDP endpoint before DNS, TCP, TLS, or HTTP. The official Ubuntu 26.04
metainfo contained only HTTPS rows, so RSTorrent found no candidate and
transferred no payload while pinned libtorrent reached the 10% target. Raw
standalone magnet intake had a second facet of the same gap:
`configured_magnet_trackers` filtered the already-bounded UDP/HTTP/HTTPS
catalog down to UDP before constructing the schedule.

The long-lived application path is not missing HTTP(S). Tactical
[`095`](095-bounded-http-https-tracker-transport.md) and Tactical
[`098`](098-authenticated-https-tracker-platform-trust.md) already provide the
bounded HTTP implementation, authenticated platform-default HTTPS, mixed
transport scheduling, peer intake, lifecycle, and controlled/public evidence.
The application intentionally passes an empty nested-driver tracker catalog
because its session-wide discovery-advertisement service is the sole product
owner.

The accepted change is therefore an ownership refactor plus composition:
extract one task-free tracker-operation executor that performs exactly one
selected UDP, HTTP, or HTTPS announce and returns one transport-neutral
outcome. The existing application and direct managers remain separate
lifecycle owners and call that executor. This removes demonstrated transport
dispatch duplication without turning tracker transports into a trait/plugin
framework or embedding the application session service in a standalone
download.

## Scope And Stopping Condition

This tactical owns:

1. a cohesive task-free operation boundary shared by the application and
   direct tracker owners;
2. relocation of the current UDP runtime mechanics out of the monolithic
   driver body into that boundary without changing their wire behavior;
3. transport-neutral announce inputs, accepted outcomes, declared failures,
   continuation state, and peer-address output;
4. HTTP(S) dispatch from the direct manager using the established system-trust
   client behavior, family policy, deadlines, redirects, parsing, and bounds;
5. preservation of application ownership, generation fencing, live settings,
   real counters/listener ports, and the session-wide eight-operation ceiling;
6. generalization of standalone magnet/config intake from the misleading
   `udp_trackers` field and UDP-only conversion to the retained supported
   tracker catalog; and
7. deterministic, scripted, controlled, Android-build, repository, and bounded
   public-comparison evidence.

The tactical completes only when:

- both outer owners dispatch UDP and HTTP(S) through the same operation
  executor and neither contains a second transport implementation;
- application configurations still disable the nested owner and existing
  application HTTP(S), UDP, settings-replacement, endpoint-generation, and
  shutdown tests remain green;
- a direct HTTP tracker and a direct authenticated HTTPS tracker independently
  introduce the resumable driver to an exact hash-verified peer with no hint or
  DHT dependency;
- invalid certificate/name HTTPS fails before an accepted HTTP announce under
  system trust, while mixed-tier fallback and plain HTTP remain usable;
- raw magnet HTTP(S) rows and explicitly supplied metainfo tiers reach the
  common schedule without losing order/source facts or exceeding established
  catalog and operation limits;
- cancellation joins every operation with no late peer publication, task,
  socket, or continuation mutation;
- the complete Rust baseline, applicable web contract checks, both Android ABI
  builds, and controlled pinned-libtorrent interoperability pass; and
- one authorized bounded Ubuntu comparison either observes RSTorrent attempt
  both official HTTPS rows and reach payload or records a changing-network
  failure after proving dispatch. A public result remains a dated observation,
  never the deterministic correctness authority.

## Non-Goals

- Replacing the application discovery-advertisement service with a per-torrent
  task or embedding that session owner in the public probe.
- One universal tracker trait, plugin interface, socket abstraction, or
  transport-independent wire class.
- Changing tracker tier scheduling, retry formulas, reannounce clamping,
  tracker URL limits, HTTP parsing, redirect policy, certificate policy, or
  response peer limits established by Tacticals `095` and `098`.
- Adding proxies, scrape/BEP 48, WebSocket trackers, web seeds, cookies,
  custom roots/pins, private-tracker policy, or HTTP/2 and later HTTP versions.
- Giving the standalone driver a persisted/live compatibility setting for
  unauthenticated HTTPS. Its bounded default is authenticated system trust.
- Adding a listener or reachability claim to the standalone driver. It retains
  the honest outbound-only port sentinel used by its existing UDP path.
- Claiming public Ubuntu tracker reliability or using public behavior to hide a
  failed controlled fixture.

## Accepted Boundary And Dependency Direction

```text
runtime-independent tracker URL + schedule (`tracker.rs`)
                         |
                         v
HTTP codec/runtime (`http_tracker.rs`)    UDP codec (`rstorrent-protocol`)
                         \                 /
                          v               v
             task-free tracker operation executor
                    /                    \
                   v                      v
application session owner          direct one-download owner
  generations/settings/            one schedule/task/join set/
  counters/endpoints/global cap     outbound-only announce facts
                   \                      /
                    v                    v
                   existing per-torrent peer registry
```

The operation executor owns no schedule, timer loop, `JoinSet`, channel,
torrent registration, peer registry, counter authority, listener policy, or
settings reconciler. It receives one selected endpoint plus an immutable
announce snapshot, network/family/source policy, transport continuation, HTTP
client pair, deadline, cancellation token, and diagnostic control. It returns
one common result and updated bounded continuation.

The application owner remains the only multi-torrent scheduler and global
operation-budget owner. The direct owner remains one supervised child of one
download and retains its existing eight-operation local ceiling plus the
session permit supplied by `DownloadControl`. Product callers continue to
disable the direct owner so no torrent ever has both owners.

Implementation should use plain structs, enums, and functions. A new trait is
out of scope unless an unanticipated concrete second implementation makes it
necessary and this tactical records that change first.

## Owner, Task, Cancellation, And State Map

| Owner | Mutable state | Work and termination |
| --- | --- | --- |
| `TrackerSchedule` | per-row deterministic lifecycle and retry state | Task-free; outer owner supplies monotonic elapsed time and accepted/failure transitions. |
| Shared operation executor | one owned UDP token cache or borrowed HTTP tracker ID plus one in-flight transport operation | Starts no task. Caller polls the returned future under its own `JoinSet`; cancellation wins before late outcome publication. |
| Application discovery-advertisement service | all torrent registrations, schedules, keys, UDP token caches, HTTP tracker IDs, current client pair, endpoint/settings generations, and global high waters | One session task; generation-fenced operations join on removal/replacement/shutdown under the established five-second stopped bound. |
| Direct `TrackerManager` | one schedule, key, per-row continuation maps, result queue, and at most eight operations | One download-owned task; cancellation drains/aborts its `JoinSet`, closes the bounded result channel, and is joined by `shutdown`. |
| Torrent peer state | accumulated peer observations and dial policy | Existing owner receives accepted `SocketAddr` values from either outer owner; the executor owns no peer list. |

Cancellation and stale-result invariants are exact:

- cancellation produces no schedule success/failure, tracker-ID update, token
  publication, warning, or peer update after the owner fences the generation;
- an application endpoint/client/settings generation change supersedes the
  captured operation exactly as before;
- the direct owner may publish only while its result receiver and cancellation
  generation remain live; and
- dropping either owner requests cancellation and retains an observable join
  path rather than relying on detached work.

## Announce And Outcome Contract

One transport-neutral announce snapshot contains:

- info hash, peer ID, key, uploaded/downloaded/left counters, event, requested
  peer count, family-specific advertised ports, and the live incoming-MSE
  capability fact;
- network policy, address-family policy, and optional selected source address
  per family; and
- the operation timeout chosen by the outer lifecycle owner.

One accepted outcome contains:

- bounded peer `SocketAddr` values, accepted interval, optional seeder/leecher
  counts, actual successful connection family, optional replacement tracker
  ID, and bounded warnings.

One failure is either cancellation, a redacted transport failure, or a bounded
tracker-declared failure with optional BEP 31 retry/disable advice. The outer
schedule remains the only owner that interprets failure into fallback, retry,
disable, and diagnostics.

UDP retains its per-remote 60-second connection-token cache. HTTP(S) retains
its per-row tracker ID. Neither continuation is durable. Moving those values
through the executor must not multiply their bounds or make one transport
interpret the other's state.

## Stable Resource And Security Invariants

All existing limits remain authoritative:

| Resource | Bound |
| --- | ---: |
| Application tracker operations | 8 session-wide across all torrents/transports |
| Direct tracker operations | 8 for the one nested owner, still charged to its supplied session permit |
| Direct result queue | 4 batches |
| HTTP redirects | 5 |
| HTTP aggregate ordinary/stopped time | 30 seconds / outer-owner 5-second stop bound |
| HTTP encoded and decoded body | existing 1 MiB limits |
| HTTP response peers | 200 |
| Tracker URL / request target | existing 2 KiB / 4 KiB limits |
| Resolved tracker addresses | 16 per hostname/family |
| Noncompact hostname resolution | 16 names, 4 concurrent |
| UDP token lifetime/cache | existing 60 seconds / bounded per row |
| Tracker error/warning/ID | existing 256-byte limits |

HTTPS defaults to platform certificate-chain and requested-host validation.
The direct path does not silently select the hidden disabled verifier. If a
platform-authenticated client pair cannot be constructed, HTTPS fails closed;
plain HTTP and UDP remain independently eligible when their clients/transports
are available. URLs and errors remain redacted according to the existing
transport rules, and no public/private credential enters retained evidence.

Tracker responses remain hostile hints. Policy and address-family checks apply
before tracker DNS/connect, during redirects, during noncompact peer
resolution, at common outcome conversion, and again at ordinary peer intake.
No tracker response verifies reachability, identity, seed status, or content.

## Source-First Record

No reference source, fixture, response, or certificate is copied.

### Normative protocol

Pinned BEP source revision
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06` was rechecked through the exact
documents recorded by Tactical `095`:

- `reference/bittorrent.org/beps/bep_0003.rst` owns HTTP announce fields,
  events, counters, failures, interval, and noncompact peers;
- `bep_0007.rst` owns source-family behavior and compact IPv6 peers;
- `bep_0012.rst` owns tier order and fallback;
- `bep_0015.rst` owns UDP connect/announce and transaction behavior;
- `bep_0023.rst` owns compact IPv4 plus continued noncompact acceptance; and
- `bep_0031.rst` owns failure retry advice.

This tactical changes composition, not those wire contracts.

### Pinned libtorrent oracle

Pinned libtorrent `2.0.13.0` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected:

- `src/tracker_manager.cpp::{queue_request,remove_request,
  abort_all_requests}` explicitly dispatches HTTP/HTTPS and UDP beneath one
  manager, retains transport-specific connection collections, bounds active
  HTTP announces, queues excess HTTP work, and closes both transport kinds on
  stop without inventing a common socket class;
- `src/http_tracker_connection.cpp::{start,on_response,
  parse_tracker_response}` constructs the announce, delegates bounded HTTP/TLS
  exchange, returns peers/interval/counts/warning/tracker ID, and maps tracker
  failures to the manager callback;
- `test/test_tracker.cpp::{http_peers,parse_hostname_peers,parse_peers4,
  parse_interval,parse_warning,parse_failure_reason}` proves common peer-list
  delivery and response behavior, while its TODO list confirms missing
  `peers6`, tracker-ID, and uneven-stride cases that RSTorrent already authors;
- `test/test_http_connection.cpp::{no_proxy_ssl,no_proxy,
  run_suite}` covers direct HTTP/HTTPS, redirects, same-origin credential
  retention, cross-origin stripping, authentication, timeout, and connection
  shutdown; proxy variants remain outside this tactical; and
- `src/tracker_manager.cpp::abort_all_requests` retains stopped requests during
  ordinary stop but closes every request at destruction, reinforcing explicit
  lifecycle ownership rather than transport-specific detached tasks.

Adopted completeness behavior is one manager-level dispatch decision with
transport-cohesive mechanics, bounded concurrent work, common outcome
delivery, and explicit cancellation. RSTorrent does not copy libtorrent's
class graph, callback architecture, OpenSSL objects, or separate HTTP queue.

### Existing RSTorrent owners

- `crates/rstorrent-engine/src/tracker.rs` already owns the pure mixed-
  transport schedule and accepted/failure transitions.
- `http_tracker.rs::{HttpTrackerClients,
  announce_http_tracker_with_address_families}` already owns the bounded
  HTTP/HTTPS operation and authenticated client behavior.
- `advertisement.rs::{fill_tracker_operations,apply_tracker_result}` is the
  complete application composition oracle, including exact counters/ports,
  continuation, endpoint generations, warnings, and peer intake.
- `driver.rs::{TrackerManager,run_active_tracker_manager,
  announce_udp_tracker}` is the incomplete direct composition and the current
  location of reusable UDP runtime mechanics.

The operation boundary is extracted from current independently authored code;
it does not change dependency licenses or add a crate.

### JSTorrent product history

The local first-party checkout was inspected at
`9895410beeed6aff554053769bd006a3fbd373ef`; unrelated untracked documentation
does not overlap the reviewed source:

- `packages/engine/src/tracker/tracker-manager.ts` constructs both
  `HttpTracker` and `UdpTracker`, keeps transport queues, supplies current
  announce stats, and fans common discovered-peer events into the torrent;
- `packages/engine/src/tracker/http-tracker.ts` owns the HTTP request/response,
  timeout, retry/warning, compact/noncompact, and connection-family behavior;
  and
- `packages/engine/src/interfaces/tracker.ts` supplies the common event/outcome
  vocabulary.

RSTorrent adopts the useful separation between orchestration and cohesive
transport mechanics. It does not inherit JSTorrent's flattened tiers,
per-tracker timer ownership, independent transport queues, unconditional `?`
URL construction, or BEP 31 seconds interpretation.

## Edge-Case Checklist

- HTTP and HTTPS rows parsed from a raw magnet are not filtered before the
  schedule; duplicate and catalog limits remain unchanged.
- Explicit metainfo tier/source/position facts survive the renamed direct
  configuration field.
- Mixed UDP/HTTP/HTTPS fallback, promotion, zero-peer success, ordinary
  reannounce, failure retry, and tracker-declared retry/never retain existing
  schedule semantics.
- A valid system-trust HTTPS certificate/name succeeds; unknown issuer,
  wrong name, and invalid TLS fail before an accepted announce.
- HTTP remains usable if HTTPS authentication construction is unavailable;
  HTTPS never downgrades to the disabled verifier.
- Existing passkey query and Basic credentials retain redaction and redirect
  stripping; HTTPS-to-HTTP downgrade remains rejected.
- Compact IPv4/IPv6 and bounded resolved noncompact peers become common
  `SocketAddr` results and pass the direct owner's ordinary policy checks.
- HTTP tracker ID is reused only by its row; UDP tokens remain per remote
  endpoint and are removed after a failed announce.
- Direct cancellation during DNS, connect, TLS, request, body, peer-hostname
  resolution, UDP retransmit wait, result-queue backpressure, and reannounce
  wait joins promptly and produces no late state.
- Application settings/client and advertised-endpoint generation changes keep
  their existing stale-result fences after executor extraction.
- Product application paths retain exactly one tracker owner; no duplicate
  started/completed/stopped announce is introduced.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure/config | Magnet UDP/HTTP/HTTPS conversion, explicit tier/source retention, renamed field call sites, and transport-neutral continuation/outcome tests. |
| Scripted direct HTTP | Tracker-only resumable download receives a controlled peer, verifies exact content, preserves tracker ID and lifecycle, and cleans up. |
| Scripted direct HTTPS | Installed test root plus matching name succeeds; wrong name/unknown issuer fails before HTTP; cancellation and mixed fallback pass. |
| Application regression | Existing mixed-operation ceiling, HTTP/HTTPS lifecycle, family/source, settings replacement, endpoint generation, and stopped-owner tests pass unchanged through the shared executor. |
| Controlled interoperability | A controlled HTTP and authenticated HTTPS tracker each introduce the focused direct driver to pinned libtorrent for exact content with no hint or DHT. |
| Repository/platform | Format, clippy all targets, workspace tests, generated-contract drift/typecheck/web tests, both Android ABI cross-builds, and architecture checks pass. No UI change or AVD presentation claim is expected. |
| Opt-in public | The catalogued Ubuntu pair reruns within Tactical `122`'s time/disk/privacy/cleanup bounds and records dispatch plus outcome without turning one run into reliability evidence. |

## Implementation And Commit Slices

1. **Tactical and source dossier.** Land this bounded ownership contract and
   activate Tactical `136` as the sole authoritative Now.
2. **Task-free operation extraction.** Move UDP runtime mechanics and common
   announce/outcome/failure/continuation values behind the executor. Convert
   the application owner first and prove no behavior drift.
3. **Direct HTTP(S) composition.** Generalize direct intake/config naming,
   construct the authenticated HTTP client pair, dispatch all supported
   transports, and emit common peer addresses/warnings/failure transitions.
4. **Focused controlled evidence.** Add direct HTTP/HTTPS success, trust
   failure, mixed fallback, cancellation, lifecycle, and pinned-libtorrent
   vertical coverage.
5. **Closure.** Run proportional repository/Android gates, rerun the bounded
   public pair if prerequisites pass, reconcile all owners, and record exact
   landed evidence and deliberate deferrals.

Each slice leaves the workspace formatted and its focused tests passing before
commit. A partial slice must continue reporting unsupported direct HTTP(S)
truthfully and must not change the application support claim.

## Completion Record

The tactical landed in five logical commits:

- `1051724` records the source-first ownership and evidence contract;
- `41c8f65` extracts `driver/tracker_operation.rs` and moves the application
  owner onto the shared task-free UDP/HTTP/HTTPS executor;
- `67962d3` composes the direct manager with authenticated system-trust
  HTTP(S), common address outcomes, HTTP tracker-ID continuation, all retained
  raw-magnet transports, and the generalized `trackers` configuration; and
- `7e8f0d6` returns the tracker owner after content discovery so successful
  focused downloads send bounded `completed` and `stopped` announces, while
  adding direct lifecycle, fallback, cancellation, and HTTPS interoperability
  evidence; and
- `0f4e2f0` makes successful finalization join an already exhausted tracker
  owner cleanly while retaining task-panic reporting.

The final shape retains two lifecycle owners and one transport operation
implementation. The application continues passing an explicit empty nested
catalog. The direct owner retains one schedule, one supervised task, its
eight-operation ceiling, its supplied session permit, per-row UDP tokens and
HTTP tracker IDs, and a five-second finalization deadline. Content discovery
returns that owner rather than dropping it, so success sends `completed` then
`stopped`; failure and cancellation still use immediate joined shutdown.

Raw magnets now preserve all bounded UDP, HTTP, and HTTPS rows. Full request
URLs remain private to operations, while schedule snapshots and activity
events expose only redacted scheme/host/port labels. A stopped announce uses
`numwant=0` and the bounded stop deadline. No dependency, trait framework,
daemon, IPC surface, persistence schema, product setting, or second product
tracker owner was added.

## Completed Evidence

Deterministic and scripted Rust evidence proves:

- a raw-magnet HTTP tracker is the sole discovery source for an exact
  hash-verified payload and receives `started`, `completed`, and `stopped`
  with tracker-ID reuse;
- a declared HTTP failure falls through to the configured UDP tier;
- stalled direct HTTP cancellation joins the manager and closes the socket;
- an owner exhausted before another discovery source completes can be joined
  without turning the successful download into a false tracker failure;
- direct and application mixed transports retain their ceilings, retry and
  endpoint-generation behavior; and
- the complete `driver::tests::discovery_metadata` group passes 34 tests with
  only its two public probes ignored.

`tests/interop/http_tracker_direct.py` used a locally generated matching chain
and pinned libtorrent `2.0.13.0`. The authenticated direct path received the
sole peer through HTTPS, independently hash-verified payload SHA-1
`576143b2992ecf25c780ff41c79552f3bb50941b`, and produced exactly
`started`, `completed`, `stopped`. Its untrusted-certificate control produced
zero accepted HTTP requests or lifecycle events. The fixture and all
certificates, metainfo, profiles, and payloads were temporary and removed.

The clean final tree passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings`;
- `cargo test --workspace`;
- `npm run typecheck --prefix clients/web`;
- `npm run test --prefix clients/web` with 241 tests passing and two skipped;
- warning-denying engine clippy with `test-platform-root` and all targets;
- the controlled direct HTTPS harness above; and
- `clients/android/build.sh`, including release
  `x86_64-linux-android` and `aarch64-linux-android`, both UniFFI bindings,
  Android unit tests, and the debug APK.

The authorized public command was one direct-metainfo Ubuntu 26.04
`matched-plain-30` pair, 10% target, 120 seconds per owner, ten-second cleanup,
and 10-GiB hard network authorization. It ran from clean commit `7e8f0d6` and
completed its report and cleanup. Libtorrent reached 292,651,008 verified
bytes in 5.399 seconds. RSTorrent no longer failed at tracker dispatch: two
HTTPS response batches reported two peers, the first candidate arrived at
0.148 seconds, first payload at 4.203 seconds, and six pieces / 1,572,864
bytes verified before the 120.003-second boundary. The pair therefore still
classified `reference_only`, but the original zero-response/zero-candidate
HTTP(S) integration gap is closed. The later one-peer stall is one dated
changing-swarm observation, not authority for another implementation slice.
The 292-KiB raw report and all temporary artifacts were removed after this
summary.

Every stopping condition owned by this tactical is satisfied. Separate
future work may reproduce the post-discovery public stall deterministically,
but this tactical does not infer peer-policy work from one public sample.

## Escalation Contract

The user has authorized this tactical, logical commits, controlled local
processes, existing pinned-oracle use, bounded public downloads under the
campaign policy, and ordinary temporary-artifact cleanup. Stop for direction
before adding a dependency, changing the product HTTPS default, weakening
certificate/name validation, adding a new public API or setting, modifying the
global operation ceiling, using a real private tracker credential, expanding
into proxy/web-seed/scrape support, or performing external/destructive action
outside the recorded harness cleanup.
