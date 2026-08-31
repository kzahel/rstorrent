# Tactical 202: Android Progressive Media Playback

Status: **Active as of 2026-08-31.** User direction authorizes end-to-end
implementation and commits. This tactical selects lazy first-use startup of
the existing shared media listener, retained until joined Android application
shutdown. Idle listener teardown, subtitles, and production release remain
deferred.

Topics: `android-jstorrent-replacement`, `android-saf-storage`,
`http-file-serving-and-streaming`, `application-view-api`, `client-surfaces`,
and `capability-readiness`

Dependencies: completed verified HTTP serving Tactical
[`138`](138-verified-http-file-serving.md), incomplete-file demand Tactical
[`139`](139-incomplete-file-streaming-demand.md), Library playback Tactical
[`189`](189-library-playback-and-torrent-size.md), direct-storage Tactical
[`191`](191-direct-filesystem-storage.md), and Android background-lifecycle
Tactical [`200`](200-android-product-background-lifecycle.md).

## Decision And Desired Outcome

Give the first-party Android product native playback of recognized completed
and eligible incomplete video through the same bounded HTTP capability used by
browser and Tauri clients:

```text
Compose Play
  -> AndroidApplicationClient.create_media_url(torrent, file)
       -> ensure one shared LoopbackMediaServer is running
       -> existing ApplicationService media capability
  -> private PlayerActivity / Media3
       -> GET or single-range GET on 127.0.0.1:<ephemeral>
       -> existing verified logical-file reader
       -> existing streaming-demand lease and SAF storage owner
```

This is Android host and presentation integration, not another streaming
implementation. `rstorrent-session` remains authoritative for eligibility,
capabilities, verification waits, demand, publication handoff, and path/SAF
storage. `rstorrent-media` remains the only HTTP router and loopback-server
implementation. Kotlin and Media3 never read piece payload through UniFFI,
construct a storage path, or implement range scheduling.

The Android application does not bind the media listener during ordinary
profile startup. The first accepted media-URL request serializes startup,
binds exact IPv4 loopback on port zero, configures the application origin, and
retains that one listener for the remaining `AndroidApplicationClient`
generation. Concurrent first requests converge on the same server. A bind
failure rejects playback without failing profile or torrent startup.

Do not stop the listener after capability expiry or an idle interval in this
tactical. Media3 may reconnect after probing, seeking, buffering, pause,
resume, track discovery, or process scheduling. Idle teardown would change
the ephemeral port, revoke every URL, and introduce a second timer/race owner
without measured benefit. One idle listener file descriptor and accept task
remain charged until ordinary application shutdown. A later cross-platform
optimization may add evidence-driven idle closure.

## Scope And Stopping Condition

This tactical owns:

1. a direct `rstorrent-media` dependency in the Android Rust host and one
   `AsyncMutex<Option<LoopbackMediaServer>>` owned by
   `AndroidApplicationClient`;
2. a generated asynchronous `create_media_url` Android boundary that starts
   the listener at most once, calls the existing semantic application method,
   returns the existing typed outcome, and never exposes a token in logs;
3. race-safe shutdown that rejects new creation, captures any completed lazy
   startup, closes the application generation to revoke capabilities and
   streaming demand, gracefully joins the listener, and reports either
   failure without abandoning the other owner;
4. exact Android cleartext permission for `127.0.0.1` while retaining the
   platform default denial for nonloopback Java/Kotlin HTTP;
5. one non-exported Media3 `PlayerActivity` that accepts only an exact current
   loopback capability URL supplied internally, uses the platform decoder and
   `PlayerView`, requests audio focus, exposes ordinary transport controls,
   and owns player release on every terminal activity path;
6. native picture-in-picture on supported Android versions, including current
   video aspect ratio and automatic entry where the platform supports it;
7. one playback interaction lease in the existing Tactical `200` lifetime
   owner so leaving the main Compose activity, buffering, screen-off, Home,
   and picture-in-picture cannot tear down the service mid-stream;
8. a focused Compose **Play** action for classifier-v1 recognized video whose
   authoritative availability is `available` or `streamable`, while retaining
   the existing external **Open** action for completed files;
9. deterministic Rust lazy-start/single-flight/shutdown tests, Kotlin URL and
   playback-policy tests, Compose action tests, merged-manifest and network-
   security checks, and both native ABI builds; and
10. an owned Android campaign that plays a controlled completed SAF video and
    an incomplete SAF video through genuine HTTP range requests, seeks while
    incomplete, exercises Home/picture-in-picture or the exact supported
    fallback, observes completion handoff, and verifies resource cleanup.

The tactical stops only when:

- Android uses `rstorrent_media::LoopbackMediaServer`; no second HTTP server,
  range parser, byte bridge, streaming scheduler, or storage reader exists;
- no media listener exists before the first media request, concurrent first
  requests produce one bound port, subsequent requests reuse it, and service
  shutdown leaves no listener, HTTP body, capability, streaming demand, SAF
  request, storage handle, or player owner;
- a completed recognized video plays and seeks through Media3 from its
  capability URL;
- an eligible incomplete recognized video begins playback from verified
  ranges, a later seek replaces bounded demand, unverified bytes are never
  emitted, and the same URL continues across final publication;
- main-activity departure and picture-in-picture do not stop active playback,
  while closing playback releases its lifetime lease and permits the existing
  background policy to stop an otherwise ineligible service; and
- the focused repository, generated-boundary, Android build/test, and owned
  platform gates pass with exact evidence recorded below.

## Non-Goals

- A second torrent engine, Android scheduler, Kotlin/Java range server,
  callback payload source, custom Media3 `DataSource`, native host, daemon, or
  companion media route.
- Exposing `torrent_media` or capability URLs through the ChromeOS companion,
  remote relay, LAN, wildcard, peer, mapped, or public listener.
- Listener idle timeout, port persistence, port probing, fixed media port,
  service restart solely for a stale URL, or durable playback URL.
- Embedded desktop playback or changing browser/Tauri startup, capability,
  opening, or shutdown behavior.
- Audio-library presentation, codec packs, transcoding, remuxing, thumbnails,
  artwork, playlists, watched state, resume position, playback history, Cast,
  or background audio service controls.
- External subtitle selection, subtitle discovery, or sidecar manifest
  integration. Subtitle support remains a bounded presentation follow-up.
- Claiming every recognized container/codec decodes on every Android device.
  Recognition controls product eligibility; Media3/platform decoder failures
  remain visible and bounded.
- Production `com.jstorrent.app` identity, migration, signing, Play upload,
  extension publication, release, or public-support claims.

## Stable Scenarios And Invariants

### Lazy listener ownership

- `AndroidApplicationClient::open` creates no `LoopbackMediaServer` and does
  not configure a media origin.
- `create_media_url` first checks the live client generation, serializes on
  the media-server owner, rechecks shutdown after acquiring it, binds only
  `127.0.0.1:0` when empty, and retains the successful server before asking
  the application to create a capability.
- A typed unavailable file still causes first-use listener startup: transport
  readiness and file eligibility are separate facts. Bind/configuration
  failure leaves the slot empty so a later direct user action may retry.
- Repeated and concurrent requests share the same port. Capability registry
  reuse, 30-minute idle expiry, 24-hour absolute expiry, 128-entry ceiling,
  per-capability request bounds, and token format remain unchanged.
- Shutdown sets the terminal fence first, stops the companion, cancels
  platform requests, takes the optional media server, closes the application
  generation so bodies and demand are revoked, then joins the listener. Every
  captured owner is attempted even when another shutdown step fails.

### Android URL and network authority

- Rust returns the complete URL. Kotlin never derives the port or token.
- The player accepts only `http`, host `127.0.0.1`, an explicit valid port,
  no user info, query, or fragment, and exact
  `/media/v1/<43-character URL-safe capability>` shape.
- The player activity is not exported. The complete URL is not placed in
  logs, notifications, product state, saved preferences, diagnostics, or
  durable storage.
- Android Network Security Configuration permits cleartext only for exact
  loopback and keeps its nonloopback base policy denied. The Rust BitTorrent
  and tracker network stack is unaffected by this Java/Kotlin policy.
- The existing HTTP server still validates exact Host, capability, methods,
  ranges, and response bounds. Android adds no CORS or authentication bypass.

### Playback eligibility and presentation

- Android classifier-v1 recognized video extensions exactly mirror the
  shared catalog's current case-insensitive set:
  `mp4`, `mkv`, `avi`, `webm`, `mov`, `m4v`, `ts`, `mts`, `m2ts`, `flv`,
  `wmv`, `ogv`, and `3gp`.
- **Play** is enabled only for a recognized non-padding video with typed
  `available` or `streamable` authority and while no launch is pending.
- Completed files retain **Open** through the existing SAF `content://`
  system action. Playback does not remove the general completed-file outcome.
- Presentation never infers streamability from percentage, Done bytes,
  filesystem existence, extension, or current download rate. The semantic
  media call rechecks authority after the displayed snapshot.
- Unavailable outcomes and bind/player errors are shown through bounded
  product/player state without launching a stale or empty activity.

### Playback and service lifecycle

- `PlayerActivity` obtains the capability before launch; it owns only the
  Media3 player and one existing Android interaction lease, not the Rust
  application or listener.
- The lease begins before playback preparation and remains through visible,
  buffering, paused, Home, and picture-in-picture states. It releases exactly
  once on activity destruction or invalid launch.
- ExoPlayer/Media3 owns decoder, renderer, audio focus, buffering, seek, and
  surface lifecycle. Its HTTP requests remain ordinary clients of the shared
  server.
- Player failure or closure releases Media3 resources. Capability expiry,
  torrent pause/removal, root loss, application shutdown, and server closure
  fail the HTTP source rather than serving stale or unverified data.
- Closing the player does not pause, stop, unselect, reprioritize, archive, or
  remove the torrent. Existing Tactical `200` policy alone decides whether an
  otherwise backgrounded service remains.

## Owner, Task, And Dependency Map

| Owner | Responsibility | Termination |
| --- | --- | --- |
| `AndroidApplicationClient` | One optional lazy shared media listener beside the existing application and companion owners | Atomic shutdown fence, application close, listener cancellation and joined task |
| `LoopbackMediaServer` | Exact-loopback accept task and shared media router | Explicit `shutdown`; `Drop` remains abort safety only |
| `ApplicationService` media registry | Origin, capability, exact file reader, request/demand admission and revocation | Torrent/profile/application lifecycle and existing expiry |
| `ProductEngineService` | Asynchronous direct-user playback request, typed outcome, private activity launch, product error state | Existing service scope and joined application shutdown |
| `PlayerActivity` | Media3/PlayerView, audio focus, PiP parameters and one playback interaction lease | Activity destroy/finish or invalid request |
| Existing SAF broker/file pool | Dynamic exact-document acquisition and Rust-owned positional I/O | Existing generation cancellation, pool eviction, repair or shutdown |

Dependency direction remains:

```text
Android Compose / Media3
  -> generated Android application adapter
  -> rstorrent-media HTTP adapter
  -> rstorrent-session media authority
  -> rstorrent-engine verified path/SAF storage and torrent scheduling
  -> pure protocol/domain layers
```

No lower layer depends on Android, Media3, Axum presentation state, an
activity, or a player.

## Existing Bounds Retained

| Resource | Bound |
| --- | --- |
| Android media listeners | 0 before first use; then at most 1 per application generation |
| Listener bind | exact `127.0.0.1:0`; one OS-selected ephemeral port |
| Capabilities | existing 128 memory-only entries |
| Capability lifetime | existing 30-minute idle / 24-hour absolute |
| HTTP bodies | existing 16 application-wide / 4 per capability |
| Logical media reads | existing 8 |
| Response preparation | existing 64 KiB chunks |
| Streaming demand | existing current interval plus at most 4 MiB / 16 pieces ahead |
| Streaming demand per torrent | existing 8 |
| Android playback activities | platform task may recreate one private activity; each instance owns at most one player and one lease |

## Source And Reference Record

This tactical changes no BitTorrent protocol, media eligibility, range
fulfillment, verified-read, demand, picker, publication, or storage semantics.
The source-first libtorrent and HTTP findings in Tacticals `138` and `139`
therefore remain authoritative; no new libtorrent behavior is adopted and no
reference source or fixture is imported.

The RSTorrent seams reviewed before implementation are:

- `crates/rstorrent-media/src/lib.rs::LoopbackMediaServer::{bind,shutdown}`
  and `media_router`;
- `crates/rstorrent-session/src/application.rs::{configure_media_origin,
  create_media_url,resolve_media_capability}`;
- `crates/rstorrent-session/src/media.rs` capability, verified/active reader,
  streaming-demand, handoff, expiry, and resource owners;
- `crates/rstorrent-android/src/lib.rs::AndroidApplicationClient` open,
  companion, platform-storage, and shutdown owners;
- `clients/android/.../ProductEngineService.kt`,
  `ProductInteractionRegistry`, `ProductLifecycleCoordinator`, and Files
  presentation; and
- the exact-loopback Tauri composition in
  `clients/desktop/src-tauri/src/lib.rs`.

The local JSTorrent Android reference was inspected at
`25e4b701433fd815398ba89526546f5e4f072e3f`:

- `android/app/.../player/PlayerActivity.kt` owns Media3, player release,
  buffering/error presentation, audio/video surface behavior, PiP, and a
  playback lifetime registration;
- `android/app/.../player/PlayerLaunchRequest.kt` uses a private typed launch
  shape and rejects invalid input;
- `android/app/.../service/ServiceLifecycleManager.kt` keeps streaming
  playback alive independently from the background-download preference; and
- `android/gradle/libs.versions.toml` pins Media3 `1.9.2`.

RSTorrent adopts the proven product requirements—native player ownership,
service survival through playback, prompt release, PiP, and visible errors.
It deliberately does not adopt JSTorrent's QuickJS-backed custom Media3
`DataSource`, companion server, daemon topology, stream callback bridge, or
payload path. RSTorrent's existing HTTP capability is the data source.

Android's official Network Security Configuration documentation states that
apps targeting API 28+ deny cleartext by default and supports destination-
specific opt-in; its localhost rules recognize numerical loopback such as
`127.0.0.1`. Media3's official troubleshooting guidance confirms that HTTP
playback requires this policy opt-in. The implementation uses a specific
loopback `domain-config`, not app-wide cleartext enablement.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Rust ownership | no server before use; invalid-file first use; concurrent single-flight; same-port reuse; bind retry shape; shutdown during/after startup; no listener/capability/request/demand residue |
| Existing HTTP authority | focused `rstorrent-media` and session media suites remain passing unchanged |
| Generated boundary | UniFFI derives/build, Kotlin generation, typed created/unavailable outcomes and no stringly URL/error substitution |
| Kotlin policy | exact recognized extensions, typed availability, exact-loopback URL validation, invalid URL rejection and one pending launch |
| Compose/player | Play enabled/disabled truth, completed Open retained, private Player activity, visible preparing/error/player states, audio focus and release |
| Android security | merged manifest has private PiP player and network-security config; device policy permits `127.0.0.1` cleartext and denies a nonloopback control host |
| Android runtime | completed SAF video full/range playback and seek; incomplete SAF video progressive start/seek/publication handoff; pause/removal failure; Home/PiP retention; close/lifetime shutdown |
| Repository | format, focused Clippy/tests, web generated-contract tests if artifacts change, dual-ABI Android build, `git diff --check` |

Physical ChromeOS playback is preferred when the currently authorized device
is available because Media3, PiP, ARCVM networking, and SAF are directly in
scope. An owned API 35 AVD may supply the deterministic platform gate if the
physical device is unavailable, but the tactical must state the evidence
level honestly and must not claim hardware codec breadth from an emulator.

## Execution And Commit Plan

1. Commit this decision-complete tactical before code.
2. Add the lazy shared Rust host, typed boundary, shutdown ownership, and
   focused Rust tests; commit after those gates pass.
3. Add exact-loopback Android security, playback policy, Media3 activity,
   Compose action, lifetime lease, and focused JVM/instrumentation tests;
   commit after Android compilation and focused tests pass.
4. Run the owned end-to-end Android campaign, reconcile defects, update the
   focused topics/readiness and this evidence record, then commit completion.

## Evidence Record

Pending implementation.
