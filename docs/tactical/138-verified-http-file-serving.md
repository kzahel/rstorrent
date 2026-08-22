# Tactical 138: Verified HTTP File Serving

Status: **Completed 2026-08-11.** Explicit maintainer direction selected
verified HTTP file serving after Tactical `136` and superseded ready-but-
unimplemented Tactical `137` as the sole authoritative **Now** while this
slice was active.

Topics: `http-file-serving-and-streaming`, `capability-readiness`,
`application-connection-architecture`, `application-control`,
`application-view-api`, `client-surfaces`, `web-ui-design`,
`storage-throughput-architecture`, `android-saf-storage`, `download-roots`

Dependencies: completed Tactical
[`116`](116-platform-storage-coherence-and-ios-feasibility.md) supplies the
common path/SAF observation contract, shared bounded file pool, and root
health authority; completed Tactical
[`120`](120-per-torrent-trusting-fast-resume.md) supplies durable verified
piece authority; completed Tactical
[`124`](124-incomplete-torrent-duplex-correctness.md) supplies coherent
published partial-file state; and completed Tactical
[`134`](134-hierarchical-transfer-rate-enforcement.md) supplies peer-transfer
rate policy that local media reads must not consume or bypass.

## Decision And Desired Outcome

Expose one authorized torrent file as a short-lived HTTP capability URL that
ordinary browsers and media players can open. The URL serves only bytes whose
intersecting torrent pieces are durably hash verified and whose published
storage representation passes the existing path/SAF observation authority.

The shared React Files surface gains one `Open` action. Browser-hosted clients
open the returned URL in a new tab. Tauri starts one media-only listener on
exact `127.0.0.1:0` and opens the returned loopback URL with the system URL
opener. Android retains its existing complete-file `content://` open path and
does not start an HTTP listener.

This is verified file serving, not incomplete streaming. Creating or reading
a capability never starts, resumes, unskips, reprioritizes, repairs, relocates,
or rechecks a torrent.

## Scope And Stopping Condition

This tactical owns:

1. one reusable logical-file reader over the established published path and
   platform-storage authorities;
2. typed eligibility derived from durable publication and piece-verification
   state rather than UI counters or file existence;
3. one volatile, bounded, file-scoped capability registry owned by the
   application/profile generation;
4. one shared Axum media router mounted by the existing gateway or a Tauri
   media-only loopback listener;
5. exact full and single-range `GET`/`HEAD` behavior with bounded chunking,
   backpressure, cancellation, and response headers;
6. automatic revocation and active-response cancellation on torrent removal,
   force recheck, profile/service replacement, and shutdown;
7. the generated application contract and React/Tauri `Open` path while
   preserving Android's existing platform-native file open; and
8. deterministic, scripted HTTP, hosted-gateway, Tauri, web, Android build,
   and complete repository evidence.

The tactical completes only when:

- every response is confined to exactly one logical non-padding file and each
  intersecting piece was verified before capability creation;
- a replacement, missing, wrong-kind, wrong-length, unavailable-root, revoked,
  expired, or superseded representation cannot emit further bytes;
- missing, malformed, expired, revoked, and mismatched capabilities are
  externally indistinguishable `404` responses;
- full, bounded, open-ended, and suffix ranges return exact bytes and headers,
  while malformed, overflowed, multiple, empty, or unsatisfiable ranges return
  the selected deterministic `416` shape without a storage read;
- `HEAD`, including the deliberate single-range compatibility extension,
  returns the corresponding `200`/`206` headers without opening content;
- a slow or disconnected consumer retains at most one bounded body chunk and
  releases request, read, and file-pool ownership after cancellation;
- gateway mode retains its existing bind, Host, TLS, and Basic-auth boundary,
  while exact-loopback media mode exposes no application or enumeration route;
- browser and Tauri clients obtain URLs only through the semantic application
  call and successfully invoke their platform opener;
- Android's `content://` behavior is unchanged, both native ABIs build, and
  shared platform-reader tests prove the SAF-shaped logical contract; and
- formatting, Clippy with warnings denied, workspace tests, generated contract
  drift, web typecheck/tests, and applicable gateway/Tauri checks pass.

## Exact Initial Eligibility

A file is eligible only when all of these statements are true at capability
creation:

- the torrent identity resolves in the current application/profile
  generation and its metainfo is verified;
- the file index exists and names a non-padding file;
- payload storage is in the authoritative published state, no force-check or
  replacement generation is pending, and the selected root is currently
  available;
- every piece intersecting the file's logical byte interval is durably marked
  verified; and
- the current path or platform reference can be observed as the expected
  regular logical file with the exact expected length.

The whole torrent need not be complete. A zero-length non-padding file is
eligible when the same publication and observation requirements hold; it has
no intersecting pieces, returns a zero-length `200`, and rejects every range.
Paused and archived torrents may remain eligible because reads do not mutate
lifecycle. Current `Skip` selection does not invalidate already verified,
authoritatively published bytes. Active/staging, incomplete, checking,
removing, errored, and unavailable-root content is ineligible.

Eligibility is returned as a typed file-view fact for presentation and is
rechecked authoritatively when creating the URL. The call may therefore fail
with the same bounded typed reason after a stale view. HTTP never discloses
that reason.

## HTTP Contract

The sole byte route is:

```text
GET|HEAD /media/v1/<capability>
```

It accepts a complete representation or one byte range of the forms
`start-end`, `start-`, or `-suffix`. End offsets are inclusive and clamp to the
logical file length. Multiple ranges, invalid units, malformed syntax,
overflow, a zero suffix, and ranges beginning beyond end-of-file are rejected
with `416 Range Not Satisfiable` and `Content-Range: bytes */<length>`.

Full responses use `200`; accepted ranges use `206` and exact
`Content-Range`. Every successful response sets exact `Content-Length`,
`Accept-Ranges: bytes`, `Cache-Control: private, no-store`,
`Referrer-Policy: no-referrer`, and a bounded extension-derived `Content-Type`
with `application/octet-stream` fallback. No content sniffing, directory
resolution, query API, multipart range, write method, permissive CORS, or
compression exists.

RFC 9110 Section 14 formally defines `Range` for GET. Existing players and the
JSTorrent oracle also issue range-bearing HEAD probes. RSTorrent deliberately
answers a valid single-range HEAD with the same `206`, length, range, and MIME
headers a GET would use, but performs no storage read. Other unsupported
methods receive `405` with `Allow: GET, HEAD`.

## Capability, Resource, And Privacy Bounds

| Resource | Initial bound |
| --- | ---: |
| Capability entropy | 256 random bits, base64url without padding |
| Live capabilities | 128 per application/profile generation |
| Capability idle lifetime | 30 minutes |
| Capability absolute lifetime | 24 hours |
| Simultaneous HTTP bodies | 16 application-wide |
| Simultaneous bodies per capability | 4 |
| Logical media read jobs | 8 application-wide |
| Body/read chunk | 64 KiB, at most one prepared chunk per response |
| Range header | 256 bytes before parsing |
| Shared storage handles | existing 40-handle application pool |

Repeated creation for the same live profile/torrent/file may reuse and touch
one capability rather than exhaust the registry. Registry saturation fails
the application call; it never evicts an active unrelated capability.

Capabilities are memory-only credentials. They are never persisted, included
in request receipts, diagnostics, telemetry, access logs, panic context, or
normal view state. The returned URL is ephemeral call output. A registry entry
contains only the profile/server generation, torrent identity, file index,
verified logical reader, creation/last-use times, request admission, and a
cancellation token.

Local media bytes are disk/application traffic and do not count against peer
upload bandwidth limits. The independent eight-read admission and shared
40-handle pool prevent media responses from creating unbounded blocking work
or descriptors alongside peer seeding. There is no user-facing media rate
setting in this slice.

## Ownership, Tasks, And Cancellation

```text
ApplicationService / profile generation
  -> verified-file eligibility + reader construction
  -> volatile capability registry
       -> global and per-capability request permits
       -> capability cancellation generations
  -> hosting adapter
       -> existing gateway route, or
       -> Tauri exact-loopback listener task
            -> one response owner
                 -> one media-read permit
                 -> shared storage-pool lease per chunk
                 -> bounded body send/backpressure
```

The logical reader is runtime-aware at its outer method but retains the
engine's inward dependency direction: immutable layout/reference geometry and
exact positional reads remain independent from HTTP, headers, sockets, and
application presentation. The media router depends on the session service;
the session and engine do not depend on Axum.

The router locks application state only long enough to resolve and admit a
capability. A response owns cloned immutable reader state and generation
cancellation, never an application-service mutex guard. Revocation cancels
active bodies, while disconnect drops the stream and all acquired permits.
Tauri shutdown stops accepts, revokes capabilities through application
shutdown, joins response work, and joins the listener task.

## Hosting And Authentication

The existing gateway mounts the shared route on its current listener and
publishes that configured origin to the application service. Loopback
unauthenticated, bearer, or web-session gateways permit the exact media route
to use the capability as its request credential because external players do
not reliably attach application headers. Existing exact Host/origin policy
still applies. A hosted/private Basic gateway requires both Basic
authentication and the capability and continues to rely on its existing HTTPS
deployment boundary.

Tauri binds only `127.0.0.1:0`, records the OS-selected HTTP origin in the
service, and mounts only the media route. It does not expose WebSocket,
application calls, health, assets, storage paths, discovery, or control. The
port and all capabilities die with that process generation. No media listener
is mapped through UPnP or advertised to peers.

## Client And Platform Contract

The semantic application call takes a torrent ID and file index and returns a
complete ephemeral URL plus bounded expiry metadata. It is not a durable
command, request envelope, receipt, event, or saved view value.

The shared React Files table shows `Open` only for a typed eligible file. A
browser calls the semantic operation and opens the URL in a new tab with
opener isolation. Tauri calls the same operation, then invokes a native command
that accepts only the exact current loopback media origin and capability-route
shape before using the system URL opener. There is no copy-link UI, embedded
player, stable share link, or MIME-specific presentation.

Android retains `ProductEngineService.openCompletedFile`: a fully verified
file is shared by its existing `content://` platform mechanism. Android does
not receive or bind an HTTP listener in this tactical. Generated boundary and
native builds must remain compatible with the new read-only eligibility fact.

## Source-First Record

No reference source, fixture, or test data is imported.

### Normative HTTP source

[RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html) was inspected,
especially Sections 14.1, 14.2, 14.4, 15.3.7, and 15.5.17. RSTorrent adopts
inclusive byte ranges, suffix/open-ended forms, `206` representation metadata,
and `416` unsatisfied-range reporting. It intentionally rejects multiple
ranges rather than generating multipart bodies and adds the bounded HEAD-range
compatibility behavior described above.

### Pinned libtorrent oracle

Rasterbar libtorrent `2.0.13.0` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected:

- `include/libtorrent/torrent_handle.hpp::{read_piece,have_piece}` defines
  asynchronous completed-piece reads and verified availability;
- `src/torrent.cpp::read_piece` rejects missing metadata and unavailable or
  out-of-range pieces, allocates a full piece, and schedules its block reads;
- `test/test_read_piece.cpp::{read_piece,seed_mode,time_critical}` exercises
  completed reads and the shorter terminal piece;
- `test/test_torrent.cpp::{test_have_piece_no_metadata,
  test_have_piece_out_of_range,test_read_piece_no_metadata,
  test_read_piece_out_of_range}` records strict invalid-state behavior;
- `test/test_file_progress.cpp::{init,init2,update_simple_sequential,
  pad_file_completion_callback}` covers multi-file boundaries, zero-length and
  padding behavior, and file completion; and
- `test/test_storage.cpp` plus
  `simulation/test_file_pool.cpp::file_pool_size` cover positional storage
  failures and bounded shared handles.

RSTorrent adopts completed-piece authority and strict boundary/failure cases.
It intentionally reads one logical file in bounded chunks rather than
allocating whole pieces, and it preserves the existing path/SAF storage owner
rather than adopting libtorrent's storage manager architecture.

### JSTorrent product oracle

The local JSTorrent reference at
`9895410beeed6aff554053769bd006a3fbd373ef` was inspected:

- `desktop/io-daemon/src/media.rs` owns tokens, GET/HEAD, single ranges,
  256-KiB chunks, idle expiry, cancellation, and revoke-on-removal;
- `packages/engine/src/node-io-daemon/engine-http-stream-bridge.ts` binds one
  session to a torrent/file and reserves future verified-range waiting;
- `packages/engine/test/node-rpc/http-rpc-server-content.test.ts` covers full,
  partial, HEAD-without-read, invalid range, and incomplete rejection;
- `packages/engine/test/node-rpc/http-rpc-server-streaming.test.ts` covers
  bounded chunks, disconnect cancellation, blocking ranges, and no-read HEAD;
  and
- `packages/engine/test/node-io-daemon/server.test.ts` covers registration,
  range, disconnect, HEAD, and owner revocation.

RSTorrent adopts the file-scoped session, player-compatible HEAD/range shape,
bounded chunks, and explicit cancellation. It does not adopt the companion IO
daemon, wildcard bind, caller-supplied storage path, incomplete waiting, or
JSTorrent's topology and constants.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure engine/session | File-to-piece geometry including shared boundary pieces, padding neighbor, zero length, partial torrent, Skip, paused/archive, stale view, unavailable root, checking/removal, token bounds/expiry/reuse/revocation, and generation cancellation. |
| HTTP | Full/open/suffix/bounded ranges, overflow/malformed/multiple/zero/unsatisfied `416`, exact HEAD/no read, methods, MIME fallback, headers, 404 indistinguishability, host/auth, slow body, disconnect, concurrency, and shutdown. |
| Storage | Exact path and fake platform reads, replacement/kind/length/root loss, cross-chunk and large offsets, shared handle/read high waters, and no neighboring-file bytes. |
| Client | Generated contract, browser URL open, Tauri exact-origin native opener, disabled/error presentation, and unchanged Android complete-file open. |
| Controlled interoperability | Ordinary HTTP client and browser-shaped range/HEAD sequence against gateway and Tauri media-only hosting with exact fixture bytes. |
| Platform/repository | macOS Tauri build/check where available, Android x86_64 and arm64-v8a native builds, `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, web generation/typecheck/tests, and applicable Playwright checks. |

No public network, swarm, remote host, physical device, visible desktop client,
or downloadable fixture is required or authorized.

## Staged Execution And Commit Plan

1. Commit this source-first tactical, exact bounds, eligibility, ownership, and
   readiness reprioritization with no behavior change.
2. Extract the verified logical-file reader and implement task-free eligibility
   plus the volatile capability registry and semantic application call.
3. Add the shared HTTP router, range/response tests, gateway mount, and Tauri
   media-only listener lifecycle.
4. Regenerate the application contract and land the React/browser/Tauri Open
   flow while retaining Android's platform-native behavior.
5. Run proportional controlled, web, desktop, Android, and repository gates;
   record actual evidence, reconcile living topics, and commit closure.

Within this authorized tactical, ordinary owner-local refactors, adversarial
cases implied by these invariants, generated adapters, test fixtures created
from independent bytes, bounded bug fixes, documentation updates, and logical
commits proceed autonomously.

Stop for human direction before adding an external dependency, persistence or
migration, a nonloopback listener, remote authorization, incomplete-file
waiting or scheduling, a visible player/copy-link surface, an Android HTTP
listener, a public/physical run, or behavior that serves bytes without the
declared verification and storage authorities.

## Completion Record

The tactical landed in five logical commits:

- `375b46b` records the source-first design, exact bounds, ownership map, and
  queue reprioritization without behavior change;
- `ecc52bf` adds the verified logical-file reader, typed eligibility,
  volatile capability registry, semantic application call, and cancellation
  authority;
- `efe75cc` adds the shared bounded Axum media router, existing-gateway mount,
  and joined exact-loopback Tauri media listener;
- `6154011` regenerates the application contract and implements authenticated
  HTTP, WebSocket, Tauri, browser, and React Files `Open` paths; and
- the closure commit fixes the additive Android reducer fixture exposed by
  full cross-build validation and records the completed evidence.

The completed boundary reads only one published non-padding logical file.
Construction requires all intersecting pieces to be durably verified and the
current path or platform observation to match the expected file kind and
length. Each read re-observes the representation before opening it. Force
recheck, torrent removal, storage transition, profile replacement, and
shutdown revoke the applicable entries and cancel already admitted bodies.
Repeated requests for the same file reuse one memory-only 256-bit capability;
no token, port, or URL enters durable state or a view.

`rstorrent-media` owns the one exact route and accepts only exact Host plus
`GET` or `HEAD`. Full, bounded, open-ended, and suffix requests produce exact
`200`/`206` responses; all rejected range shapes produce the selected `416`
without a read. Successful bodies prepare at most one 64-KiB chunk. Fixed
admission ceilings are 128 live capabilities, 16 bodies application-wide,
four bodies per capability, eight logical read jobs, and the existing 40-file
pool. The verified reader's deterministic path and platform tests observed a
high water of one read and one file lease under their one-permit harness and
returned both to zero. Local reads remain separate from peer-transfer rate
accounting.

The gateway mounts the same router while retaining its existing host and
authentication boundary; the byte route needs no application bearer in its
local capability mode. Tauri binds only `127.0.0.1:0`, joins the listener at
shutdown, and accepts only an exact current-origin capability URL before
invoking the system opener. Browser presentation reserves an opener-isolated
tab synchronously and fills it only after the semantic call succeeds. Android
does not bind HTTP and retains its existing complete-file `content://` open.

## Completed Evidence

Deterministic engine, session, media, gateway, desktop, and client tests prove:

- partial-torrent logical reads stay inside one file across shared piece
  geometry, reject padding or unverified files, fail after representation
  replacement, and share the path/platform file-pool contract;
- capability URL reuse, exact token shape, per-capability admission, force-
  recheck revocation, active-body cancellation, origin validation, and
  shutdown are bounded and observable;
- complete, bounded, open-ended, suffix, malformed, multiple, overflowed,
  empty, and unsatisfied ranges have the selected status, length, range,
  security-header, MIME, and no-read `HEAD` behavior;
- wrong Host, method, route, and capability requests do not disclose content,
  and a real ephemeral loopback server exposes only the media route;
- the existing gateway serves exact bytes after an authenticated capability-
  creation call without requiring an application bearer on the byte request;
  and
- browser, WebSocket, Tauri, generated-validator, React availability/action,
  opener isolation, failure cleanup, and exact native URL validation paths
  converge on the same semantic result.

The final tree passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace -- --test-threads=1`;
- focused `rstorrent-media`, gateway, desktop, engine, and session tests;
- `npm run generate --prefix clients/web` with no generated drift;
- `npm run typecheck --prefix clients/web`;
- `npm run test --prefix clients/web` with 247 tests passing and two skipped;
- `npm run test:e2e --prefix clients/web` with 33 tests passing and 11
  explicitly opt-in live cases skipped; and
- `clients/android/build.sh`, including release
  `x86_64-linux-android` and `aarch64-linux-android`, both UniFFI generations,
  Android unit tests, and the debug APK.

Parallel workspace attempts separately exposed timing-sensitive failures in
`bandwidth::tests::live_unlimited_change_wakes_waiter`,
`application::tests::application_incoming_bootstrap_is_disabled_or_exactly_fixed`,
and
`driver::tests::content::disconnect_and_choke_reassign_only_their_outstanding_blocks`.
Each passed immediately in isolation and all three passed in the final serial
workspace run; no media-serving failure was observed. No public network,
swarm, remote host, physical device, or visible client was used. Every
stopping condition is met for verified serving; incomplete-file waiting and
playhead-driven scheduling remain outside this result.

## Non-Goals And Next Boundary

- Incomplete-file streaming, range waits, playhead demand, time-critical piece
  scheduling, durable priority changes, or automatic lifecycle mutation.
- Arbitrary filesystem paths, directory browsing, static root mounts, WebDAV,
  uploads, writes, archives, multipart ranges, or general HTTP serving.
- Stable share links, LAN/public exposure, remote access, relay, accounts,
  TLS termination changes, Basic credentials in URLs, or port mapping.
- Transcoding, remuxing, content sniffing, thumbnails, embedded playback,
  subtitle policy, media metadata, or codec decisions.
- A companion daemon, native host, application REST API, socket proxy, or
  multiplexing with the BitTorrent peer listener.

The next streaming slice may add bounded transient demand and verified-range
waits only after this capability, logical reader, cancellation, and HTTP
contract are stable. Tactical `137` was subsequently reactivated as the next
engine implementation.
