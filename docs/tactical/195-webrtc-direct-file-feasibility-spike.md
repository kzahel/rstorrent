# Tactical 195: WebRTC Direct-File Feasibility Spike

Status: **Complete as of 2026-08-30; Proceed after post-completion
correction.** The local experiment proves that lower `webrtc-rs/rtc` can
provide a bounded, lazy, real-file DataChannel path in Chromium, Firefox, and
Playwright WebKit at a measured optional package cost. The original WebKit
run timed out during ICE, but two subsequent runs completed ICE, authenticated
DTLS, opened the DataChannel, and transferred the exact range corpus. Their
repeatable failure was instead OPFS startup in Playwright's non-persistent
context; a focused persistent-context probe completed OPFS create/write/close.
The retained Cargo feature remains off by default and has no production
signaling, UI, deployment, TURN, UPnP, or supported remote-file claim.

Topics:
[`direct-remote-file-streaming`](../topics/direct-remote-file-streaming.md),
[`http-file-serving-and-streaming`](../topics/http-file-serving-and-streaming.md),
[`remote-access-authentication`](../topics/remote-access-authentication.md),
[`application-connection-architecture`](../topics/application-connection-architecture.md),
[`client-surfaces`](../topics/client-surfaces.md), and
[`performance-and-live-evidence`](../topics/performance-and-live-evidence.md).

Dependencies: completed Tacticals
[`138`](138-verified-http-file-serving.md) and
[`139`](139-incomplete-file-streaming-demand.md) supply the bounded verified
logical-file capability and range-read authority. Completed Tacticals
[`190`](190-opaque-wasm-relay-foundation.md) and
[`192`](192-production-owner-relay-access.md) supply the authenticated opaque
remote circuit and browser/host identity model that a later product slice will
use for signaling. This spike may model that signaling locally but must not
change the production relay wire contract.

## Decision And Desired Outcome

Answer four questions with reproducible evidence:

1. Can a pure-Rust WebRTC endpoint establish a reliable DataChannel with the
   supported browser engines without bundling Google libwebrtc, media codecs,
   capture, rendering, or an SFU?
2. Can the endpoint start only on demand, remain bounded under slow or hostile
   consumers, stream exact verified RSTorrent file ranges, and terminate with
   zero owned tasks, sockets, reads, and queued bytes?
3. What are the exact stripped binary, compressed artifact, transitive
   dependency, idle-memory, active-memory, and CPU costs in representative
   RSTorrent desktop and headless release links?
4. Is there a practical browser adapter for bounded download and seekable file
   consumption, or does the fact that a DataChannel is not a URL make the
   product solution unattractive despite transport success?

The final recommendation must be one of:

- **Proceed:** retain one optional dependency and prototype seam, then propose
  a separate product-integration tactical.
- **Continue narrowly:** retain only the evidence and the smallest useful
  harness while naming one specific unresolved feasibility question.
- **Reject WebRTC for this role:** remove experimental dependencies and product
  link hooks, retain the measurements, and recommend the best alternative.

Convenient API shape or a successful echo message alone is not enough to
recommend proceeding.

## Stopping Condition

This spike stops when all of the following are committed to its execution
record and reproducible from tracked commands:

1. The exact reviewed versions/revisions, source paths, DataChannel/ICE tests,
   features, crypto backends, licenses, unsafe/native-build surfaces,
   maintenance posture, and known limitations of `webrtc-rs/webrtc` and
   `str0m` are recorded. The lower `webrtc-rs/rtc` API is evaluated when it
   could materially reduce size or improve ownership.
2. At least the high-level `webrtc-rs` endpoint and one Sans-I/O candidate are
   built in isolated release probes. If a candidate cannot reach a browser,
   the exact blocker and cost measured before rejection are recorded.
3. One selected candidate establishes a data-only connection from real
   Chromium and Firefox to a native Rust endpoint through bounded local
   signaling, transfers binary messages in both directions, applies sender
   backpressure, handles cancel/close, and leaves no owner residue. WebKit or
   Safari-shaped evidence is recorded when available and never inferred.
4. The selected prototype serves exact head, tail, overlapping, seek-shaped,
   and cancelled ranges from a real `MediaCapabilityLease` backed by a
   controlled verified torrent file. The browser independently verifies exact
   lengths and content hashes. One active incomplete-file range demonstrates
   that bytes are not emitted before hash verification, or the implementation
   recommendation explicitly scopes the first product slice to completed
   files and explains why.
5. The browser proves one bounded practical consumption route beyond an echo:
   a large-file stream-to-save path, a same-origin synthetic Range route, or a
   seekable media path. Whole-file Blob buffering is recorded only as a small-
   file baseline and cannot satisfy this condition by itself.
6. Feature-off and feature-on release measurements report exact deltas for the
   isolated endpoint probe, `rstorrent-headless`, and the current-host desktop
   executable/package. The feature-on product link demonstrably retains the
   endpoint startup path; a dependency optimized out as unreachable is not a
   valid measurement. Other release targets receive compile/link evidence in
   proportion to available local or CI infrastructure.
7. Startup with the feature compiled but unused proves zero endpoint tasks,
   UDP sockets, candidate enumeration, STUN traffic, mappings, and continuing
   allocations. A completed or failed experiment joins every owner and returns
   to the recorded idle baseline.
8. The owning topic records the measured comparison, selected or rejected
   dependency, browser result, binary/package table, known risks, and exact
   recommendation. Rejected candidate code and dependencies are removed.

## Normative And Candidate Sources

The source survey starts from:

- [RFC 8831](https://www.rfc-editor.org/rfc/rfc8831.html), WebRTC
  DataChannels, including SCTP over DTLS over ICE/UDP, message reliability,
  interleaving, congestion control, security, and file-transfer use cases;
- [RFC 8832](https://www.rfc-editor.org/rfc/rfc8832.html), the DataChannel
  Establishment Protocol;
- [RFC 8445](https://www.rfc-editor.org/rfc/rfc8445.html), ICE candidates,
  connectivity checks, nomination, consent, restart, and lifecycle;
- [RFC 8827](https://www.rfc-editor.org/rfc/rfc8827.html), WebRTC security
  architecture and fingerprint-authenticated DTLS;
- the [W3C WebRTC Recommendation](https://www.w3.org/TR/webrtc/), especially
  `RTCPeerConnection`, `RTCDataChannel`, offer/answer, ICE events,
  `bufferedAmount`, closure, and statistics;
- [`webrtc-rs/webrtc`](https://github.com/webrtc-rs/webrtc), its current
  async/runtime architecture, data-only examples, tests, and dependency graph;
- [`webrtc-rs/rtc`](https://github.com/webrtc-rs/rtc), its Sans-I/O core and
  independently packaged protocol components; and
- [`str0m`](https://github.com/algesten/str0m), its Sans-I/O driver, candidate
  ownership, DataChannel examples/tests, crypto-provider graph, and explicit
  caller ownership of interface enumeration and TURN sockets.

The execution record pins exact crate versions and source commits before
comparison. No project is a source donor. Source, examples, fixtures, or tests
must not be copied without a separate provenance and license decision.

Google C++ libwebrtc and its Rust FFI wrappers are not comparison candidates
in this tactical. Reaching them would materially expand toolchains, platform
build work, unsafe boundaries, and binary surface and requires new authority.

## Experiment Boundary

### Authorized repository changes

This tactical may add:

- an isolated local WebRTC experiment crate or binary and a browser probe page;
- exact optional Rust dependencies and resulting lockfile changes;
- a small runtime-independent typed range-frame codec;
- a feature-gated lazy endpoint owner reusable by a later product slice;
- the provisional additive Cargo feature `direct-file-webrtc`, propagated to
  desktop and headless manifests only as required for honest link measurement;
- deterministic local browser, range, lifecycle, and size-measurement scripts;
  and
- documentation, notices, and dependency-audit updates required by the
  selected retained candidate.

The final repository retains at most one candidate dependency. If none earns a
Proceed recommendation, remove all experimental product dependencies, feature
propagation, and endpoint code before completing the tactical. A useful
standalone evidence harness may remain only when it has no release dependency
effect and the tactical explains its continuing purpose.

### Product boundaries not crossed

- No visible Settings switch, Files action, Open/Play/Download behavior,
  generated application call, persistent setting, support claim, or release
  default changes.
- No production remote signaling-frame change. The harness uses an isolated
  loopback HTTPS/signaling path or a test-only adapter around existing secure
  records.
- No public listener, public DNS, certificate issuance, relay deployment,
  account, telemetry, or external message.
- No TURN, file relay, public STUN dependency, UPnP mapping, NAT-PMP, PCP, IPv6
  pinhole, or firewall mutation.
- No Android or iOS endpoint integration, mobile package dependency, or mobile
  background claim.
- No arbitrary HTTP proxy, directory server, filesystem path input, torrent
  payload over the application control relay, or unverified file read.

### External-action and cleanup bounds

- Required transport evidence is loopback and exact local-LAN only. A LAN test
  binds one explicit interface for one supervised run, never wildcard or a
  mapped/public endpoint, and removes its temporary trust and process state.
- Browser automation uses repository-managed Playwright/Chrome facilities
  before interactive control. No physical device or VM is required for the
  stopping condition.
- Candidate source may be fetched through Cargo and official source hosts.
  Experimental build targets, profiles, generated payloads, certificates,
  browser profiles, logs, and captures are ignored or temporary and removed
  after summarized evidence is retained.
- No committed media binary is required. A deterministic temporary fixture may
  be generated locally; its generation recipe and exact digest are recorded.

## Candidate Comparison Contract

Compare `webrtc-rs/webrtc` and `str0m` against the same data-only task and
build conditions. Evaluate the lower `webrtc-rs/rtc` API only if the first
comparison shows a credible size or lifecycle reason to bypass the async
layer.

For each candidate, record:

- offer/answer and trickle-ICE API shape;
- DataChannel creation, polling/events, maximum-message negotiation,
  `bufferedAmount` or equivalent sender backpressure, and prompt cancel/close;
- socket, interface, timer, task, certificate, DTLS, SCTP, ICE, STUN, and TURN
  ownership;
- ability to supply an ephemeral DTLS certificate and verify the authenticated
  fingerprint from signaling;
- runtime integration and whether hidden internal tasks can be joined;
- deterministic testability without sockets or wall-clock time;
- candidate/SDP input bounds and hostile-input behavior;
- panic, unsafe, native crypto, platform, and security-update surface;
- release binary contribution and whether unused RTP/SRTP/media modules remain
  linked; and
- upstream tests for Chrome/Firefox interop, loss, reordering, close, ICE
  restart, DataChannel fragmentation/interleaving, and large/slow sends.

The selection must be based on observed correctness and ownership as well as
size. A smaller crate that cannot expose clean cancellation or interoperable
backpressure is not the preferred solution.

## Optional Feature And Size Measurement

The experimental feature is additive and default-off. Its unique WebRTC,
DTLS/SCTP, crypto, RTP/SRTP, and native dependencies must be absent from
feature-off product dependency graphs even though the workspace lockfile may
record them for the experiment.

Measure from clean, identically configured source and toolchain inputs:

1. a no-WebRTC probe versus the candidate's minimal data-only endpoint probe;
2. `rstorrent-headless` feature-off versus feature-on release executables;
3. the current-host desktop feature-off versus feature-on release executable
   and unsigned package/archive; and
4. any readily available Linux/Windows release link needed to expose a
   platform-specific crypto or native-library surprise.

For every pair, record:

- exact command, target triple, toolchain, linker, profile, feature set, and
  git commit;
- unstripped and stripped executable size;
- compressed executable and complete unsigned package/archive size;
- absolute byte and percentage delta;
- `cargo tree -e features` unique dependency graph;
- symbol/section or `cargo bloat` attribution where supported;
- duplicated TLS/crypto/native libraries and platform runtime dependencies;
  and
- build time and peak build artifact size when notably different.

The feature-on binary must contain a reachable endpoint construction/start
path matching the prototype. Merely mentioning a crate type, compiling tests,
or adding an unused optional dependency is not representative. Conversely,
the endpoint must remain runtime-lazy: link reachability cannot be achieved by
starting it during ordinary application startup.

Installer signatures, notarization, updater publication, and CDN compression
are unnecessary. If nondeterministic package metadata changes compressed size,
repeat the build or isolate the payload contribution and state the limitation.

No binary-size acceptance threshold is invented in advance. The report must
make the decision possible by separating fixed WebRTC cost from RSTorrent's
existing binary/package size and by showing whether platform-specific costs
differ materially.

## Prototype Architecture

The preferred retained boundary is:

```text
test browser page
  -> bounded local HTTPS/signaling owner
       -> lazy direct-file supervisor
            -> one browser peer generation
                 -> UDP + ICE/DTLS/SCTP driver
                 -> one bounded DataChannel range session
                      -> MediaCapabilityLease
                           -> verified/active logical-file reader
```

The experiment must preserve these dependency directions:

- a pure range-frame codec contains no Tokio, socket, WebRTC, filesystem,
  application-service, or browser types;
- the endpoint runtime depends inward on that codec and adapts the selected
  WebRTC library;
- the file adapter borrows the existing media capability lease rather than
  duplicating storage or verification policy; and
- experiment hosting depends on the endpoint, never the other way around.

The harness may use simple loopback signaling because production signaling is
out of scope, but it must authenticate the exact DTLS fingerprint conveyed in
the accepted offer/answer transcript. It must never treat ICE reachability as
host or file authorization.

## Initial Owner, Task, And Cancellation Map

```text
experiment process owner
  -> controlled ApplicationService + temporary verified fixture
  -> local HTTPS/signaling task
  -> lazy direct supervisor (absent until Start)
       -> one peer-generation driver
       -> one UDP socket
       -> bounded signaling/candidate state
       -> one DataChannel pump
            -> bounded request registry
            -> bounded outbound queue
            -> media capability lease per admitted file
            -> verified wait/read per active request
  -> shutdown
       -> stop accepts and negotiation
       -> reject new requests
       -> cancel range waits and reads
       -> close DataChannel/peer
       -> join driver and UDP task
       -> revoke capabilities and close ApplicationService
       -> join HTTPS task and remove temporary state
```

Cancellation sources include browser request cancel, seek replacement,
DataChannel close, peer failure, negotiation deadline, request no-progress
deadline, browser page close, fixture/torrent removal, profile replacement,
and process shutdown. Every terminal test reports owner counts rather than
assuming library drop is sufficient.

If a candidate hides a driver task that cannot be awaited, reports successful
close while retaining sockets, or requires abort as the normal shutdown path,
that is a material recommendation risk.

## Initial Protocol And Resource Bounds

The experiment uses a closed binary protocol with no arbitrary HTTP method,
header, URL, path, or application-command tunneling. Conservative initial
bounds are:

- one browser peer and one DataChannel association per process;
- one file capability and at most four simultaneous range requests;
- request/control messages no larger than 4 KiB;
- binary file chunks no larger than 64 KiB;
- at most 1 MiB of queued outbound application data;
- at most 32 local and 32 remote ICE candidates, with at most 64 KiB total
  accepted SDP/candidate text per negotiation;
- a 20-second negotiation deadline, the existing 120-second media no-progress
  bound, a 60-second inactive-request deadline, and a ten-minute experiment
  lifetime; and
- one temporary controlled file no larger than 256 MiB.

The implementation may tighten a limit from source and browser evidence. It
may raise only the SDP/candidate or test-fixture ceiling within twice the
declared value when a conforming real-browser trace proves the initial bound
insufficient and the execution record explains the measured high water.

The DataChannel starts reliable and ordered for the simplest correct baseline.
The experiment must measure whether a large or stalled ordered request blocks
cancel or an independent seek. It may compare multiple channels or unordered
chunk messages, but the tactical does not select a stable product wire format.

## Browser Consumption Investigation

Transport correctness is proved first with direct typed range requests and an
independent browser hash. Then compare the smallest viable browser adapters:

1. bounded Blob/object URL for a deliberately small complete-file baseline;
2. stream-to-save using a browser-supported writable-file path without
   buffering the complete fixture;
3. a service-worker synthetic same-origin `GET`/`HEAD`/Range route bridged to
   page-owned DataChannel requests; and
4. Media Source Extensions only if a seekable ordinary media element cannot be
   proved without container-specific processing.

The experiment need not ship a polished React UI. It must nevertheless prove
one operator-visible Start/Cancel/Close flow and expose bytes, active request
count, queued bytes, selected candidate class, RTT where available, and
terminal reason so failures are diagnosable.

Record browser support and memory behavior rather than hiding incompatibility
behind a small Blob. A browser API available only in Chromium may be a useful
path but cannot establish portable remote-file support by itself.

## Staged Execution

### Stage 1: source, build, and cost preflight

1. Pin exact candidate sources and inspect their DataChannel, ICE, lifecycle,
   hostile-input, and browser interoperability tests.
2. Build minimal data-only release probes for `webrtc-rs/webrtc` and `str0m`
   with the narrowest honest crypto/runtime features.
3. Record dependency, license, native-build, stripped-size, and simple
   loopback handshake results. Reject candidates with a concrete reason.

Gate: select one provisional candidate for the integrated prototype. This
selection is ordinary execution within this tactical unless it introduces a
new non-permissive license, mandatory external service, FFI/native toolchain,
or other escalation condition below.

### Stage 2: supervised DataChannel prototype

1. Establish browser offer/answer and trickle ICE through the bounded local
   signaling owner.
2. Bind the ephemeral DTLS fingerprint to the signaling transcript.
3. Send exact binary data bidirectionally under explicit queued-byte
   backpressure.
4. Prove connect, slow consumer, cancellation, peer close, malformed messages,
   timeout, page disappearance, and joined shutdown in Chromium and Firefox.

Gate: no file integration proceeds until owner counts return to zero after
every terminal path.

### Stage 3: real RSTorrent file ranges

1. Generate a bounded torrent fixture and acquire a normal
   `MediaCapabilityLease` through the controlled application owner.
2. Implement the experimental typed range adapter with head, tail, overlap,
   seek, cancellation, and slow-consumer cases.
3. Prove exact browser hashes, no cross-file reads, revocation, and no
   unverified emission. Exercise active incomplete streaming if the adapter
   can do so without expanding the tactical materially.
4. Prove one bounded practical browser consumption path.

### Stage 4: representative release-link measurement

1. Make the selected endpoint startup path reachable only behind the optional
   feature while keeping it unstarted at ordinary runtime.
2. Build controlled feature-off and feature-on endpoint, headless, and desktop
   release pairs.
3. Attribute binary/package deltas and prove feature-off product graphs omit
   candidate-only dependencies.
4. Measure idle and active owner/memory/task/socket/CPU/throughput high waters.

### Stage 5: decision and cleanup

1. Remove rejected candidates, stale probes, temporary artifacts, and any
   feature/link hooks not justified by the recommendation.
2. Update this tactical, the owning topic, references/notices, and relevant
   readiness wording with exact evidence and gaps.
3. Present Proceed, Continue narrowly, or Reject with a bounded next tactical
   shape. Do not begin product integration in this slice.

## Validation Matrix

### Deterministic and codec

- round-trip every frame family and maximum-length boundary;
- reject truncated, oversized, unknown-version, duplicate-ID, stale,
  out-of-range, overflow, cross-file, and post-cancel frames;
- preserve exact request ID, offset, chunk order, terminal result, and
  cancellation state; and
- fuzz or property-test the runtime-independent decoder with bounded input.

### Scripted runtime

- lazy-zero baseline before Start;
- success, wrong fingerprint, malformed SDP/candidate, negotiation timeout,
  slow sender/receiver, queue saturation, DataChannel error, abrupt browser
  close, request cancel, capability revoke, torrent removal, and application
  shutdown;
- exact head/tail/seek/overlap and controlled incomplete-file verification;
- no request starvation while another range is slow; and
- zero endpoint task, socket, request, read, capability, and queued-byte counts
  after every terminal case.

### Real browser

- current repository-supported Chromium and Firefox against the same Rust
  endpoint and frame trace;
- available WebKit/Safari-shaped DataChannel and consumption-adapter result;
- exact 256-MiB or largest successfully generated bounded-file hash without
  whole-file browser memory growth;
- cancel and seek responsiveness under a throttled consumer; and
- browser page reload/close followed by complete native cleanup.

### Performance and size

- exact feature-off/on link and package table described above;
- loopback and local-LAN throughput, CPU, RSS, queued-byte and task/socket high
  water versus the existing local HTTP range path;
- first-peer startup latency and peer teardown latency; and
- no measurable continuing runtime resource after returning idle.

### Repository and platform

- feature-off and feature-on formatting, Clippy, and relevant Rust tests;
- web typecheck/tests for tracked browser harness code;
- current-host desktop/headless release builds and proportional Linux/Windows
  compile/link evidence when available;
- feature-off Android and iOS dependency trees remain free of WebRTC endpoint
  dependencies; and
- documentation links, dependency/license audit, artifact cleanup, and
  `git diff --check` pass.

The execution record reports exact commands and distinguishes passed, failed,
unavailable, and not run. No public-swarm, public-STUN, or relay traffic is
needed for this tactical.

## Recommendation Evidence Table

The completed tactical includes at least these tables:

| Candidate | Version/revision | Runtime/crypto features | Browser result | Stripped probe delta | Ownership result | Disposition |
| --- | --- | --- | --- | --- | --- | --- |

| Artifact/target | Feature off | Feature on | Byte delta | Percent delta | Compressed/package delta | Notes |
| --- | --- | --- | --- | --- | --- | --- |

| Scenario/browser | Direct path | Exact bytes/ranges | Throughput | Peak RSS/queue | Cleanup | Result |
| --- | --- | --- | --- | --- | --- | --- |

The recommendation explicitly separates:

- whether WebRTC transport is technically sound;
- whether the selected Rust implementation is maintainable;
- whether its binary/package cost is acceptable enough to consider default
  enablement later; and
- whether browser-side file consumption is good enough to justify product
  integration.

## Deliberate Non-Goals

- Production remote signaling, relay-frame negotiation, remembered-browser
  integration, security-surface presentation, or deployment.
- STUN service selection, NAT traversal success-rate claims, UPnP, public IPv6
  pinholes, TURN, relay fallback, or Internet/cellular evidence.
- Stable direct-file protocol/version compatibility or third-party clients.
- A polished React product flow, Settings UI, file action, stable service
  worker, media-library experience, or release note.
- Transcoding, remuxing, codecs, camera/microphone, screen control, RTP media,
  SFU behavior, or Google libwebrtc.
- Android/iOS endpoint hosting or physical-device validation.
- Default-on compilation, runtime activation, package publication, release,
  push, public service, or supported capability claim.

## Escalation Contract

Ordinary candidate source inspection, permissively licensed optional Rust
dependencies, local source fetches, temporary release builds, local HTTPS,
real-browser automation, feature-gated prototype code, bounded refactoring at
the media-capability seam, measurement, cleanup, and documentation proceed
without another routine implementation choice.

Stop for maintainer direction before:

- accepting a non-permissive or materially surprising license/notice posture;
- adding C/C++ libwebrtc, FFI, a mandatory native build toolchain, or a
  platform crypto dependency with material distribution consequences;
- contacting or operating a public STUN/TURN/relay service, opening a mapped or
  public listener, changing DNS/TLS, or testing through an external network;
- changing the production remote authentication/signaling wire, visible
  product UI, persistence, release defaults, or support claims;
- retaining more than one endpoint dependency after the comparison; or
- proceeding to product integration after the recommendation.

Candidate failure, a larger-than-hoped binary, browser incompatibility, or an
unfavorable recommendation is evidence rather than a blocker. Complete the
comparison, clean the repository according to the selected disposition, and
record the result.

## Execution Record

### 2026-08-30: source and cost preflight

The first-stage comparison used Rust 1.97.0 on
`aarch64-apple-darwin`, macOS 26.6.1, Apple clang 21.0.0, and Apple
`ld` 1267. Exact inspected releases and crate-package source revisions were:

- `webrtc` 0.20.4, revision
  `843d52e3af05c26e6257154e18ddf0caa241d0ad`;
- its lower `rtc` 0.20.4 API, revision
  `bbc18664cf2dcb690e023c6a1a436eb15253ca7f`; and
- `str0m` 0.23.1, revision
  `120401c9affd97fd4246d9e7faf0ad4ca099c1bc`.

The inspected `webrtc-rs` paths were `webrtc/src/peer_connection/mod.rs`,
`webrtc/src/data_channel/mod.rs`, `webrtc/src/peer_connection/driver.rs`,
`webrtc/tests/custom_runtime_interop.rs`,
`webrtc/tests/data_channel_send_backpressure.rs`, the
`data-channels-simple`, `data-channels-close`, and `data-channels-flow-control`
examples, plus `rtc/src/peer_connection/mod.rs`, its `sansio::Protocol`
handler stack, and the lower-level `data-channels-flow-control` example. The
inspected `str0m` paths were `src/lib.rs`, `src/channel.rs`, `src/config.rs`,
`src/change/{sdp,direct}.rs`, `src/sctp/mod.rs`, `src/sdp/parser.rs`, and the
`data-channel`, `data-channel-direct`, `dtls-close`, `dtls-security`,
`ice-candidates`, `handshake-direct`, and `mtu-compliance` tests.

The source comparison found:

- high-level `webrtc` owns UDP reactors and a background peer driver. It
  exposes awaited peer/channel close and an explicit bounded DataChannel send
  buffer, but application callback tasks remain the caller's responsibility;
- lower `rtc` leaves sockets, time, task creation, polling, and joining with
  the caller through `sansio::Protocol`. It retains the same ICE/DTLS/SCTP and
  SDP implementation while making the direct supervisor the sole runtime
  owner;
- `str0m` is also Sans-I/O and has the clearest documented mutate-then-drain
  rule. Its channel API exposes queued bytes and low-water notification, and
  its deterministic test suite covers channel churn, direct setup, MTU
  fragmentation, DTLS security, and graceful close;
- all candidates expose the authenticated DTLS fingerprint through the
  offer/answer or direct API. None substitutes transport reachability for
  authorization, and the experiment must compare the accepted fingerprint to
  the bounded signaling transcript before admitting a file request; and
- none of the APIs replaces the experiment's aggregate SDP, candidate-count,
  control-frame, request, or queue limits. Those bounds remain outside the
  dependency and are enforced before parsing or mutation.

All three releases are MIT/Apache-2.0 dual licensed and dynamically require
only macOS system libraries in these probes. `rtc` uses `ring`. A notable
`str0m` packaging surprise is that its nominal `rust-crypto` feature still
links AWS-LC: `str0m-rust-crypto` enables `dimpl/rcgen`, and that `dimpl`
feature enables `aws-lc-rs`. It therefore invokes a CMake/native crypto build
on this host. No OpenSSL dynamic dependency appeared.

#### Isolated release probes

Each probe had one reachable endpoint-construction path: bind loopback UDP,
generate an ephemeral certificate/fingerprint, create a reliable ordered
DataChannel, and create local signaling state. The high-level probe also
awaited peer close. The profile used `codegen-units = 1`, LTO, aborting panics,
and no Cargo stripping. Post-link stripping used `strip -S -x`; compression
used `gzip -9`. Each feature was linked in a separate Cargo invocation so
crypto features could not unify across candidates.

| Probe | Cargo packages | Unstripped | Stripped | Stripped gzip | Stripped delta |
| --- | ---: | ---: | ---: | ---: | ---: |
| loopback UDP baseline | 1 | 403,080 B | 302,992 B | 144,462 B | - |
| lower `rtc` 0.20.4 + `ring` | 180 | 1,462,552 B | 1,229,520 B | 669,134 B | +926,528 B |
| `str0m` 0.23.1 + `rust-crypto` | 111 | 3,102,448 B | 2,802,856 B | 1,388,030 B | +2,499,864 B |
| high-level `webrtc` 0.20.4 + Tokio | 202 | 3,724,096 B | 2,954,200 B | 1,538,334 B | +2,651,208 B |

Package count is the unique normal/build package count from `cargo tree` and
is descriptive rather than a size proxy. The lower `rtc` probe's stripped
increment was 0.93 MB, 1.57 MB below `str0m` and 1.72 MB below high-level
`webrtc`. This is a credible size and lifecycle reason to use the lower API,
not evidence that an unused crate will have the same product-link cost.

Upstream checks passed without public STUN/TURN:

- `webrtc` 0.20.4 `custom_runtime_interop` (2 tests) and
  `data_channel_send_backpressure` (1 test);
- lower `rtc` 0.20.4 focused DataChannel tests (14) and candidate tests (27);
  and
- `str0m` 0.23.1 `data-channel`, `data-channel-direct`, `dtls-close`,
  `dtls-security`, and `mtu-compliance` (20 tests total). The crate's own
  `_internal_test_exports` feature was required to compile those integration
  tests with default features disabled.

The Stage 1 provisional selection is **lower `webrtc-rs/rtc` 0.20.4 with
`ring`**. It combines the smallest measured fixed cost with explicit runtime
ownership and deterministic protocol driving. `str0m` remains the strongest
alternative if browser interoperability exposes an `rtc` defect; high-level
`webrtc` remains the fallback if writing a correct driver proves materially
riskier than its 1.72 MB probe delta. No rejected candidate dependency will be
added to the repository.

### 2026-08-30: bounded endpoint and verified-file integration

The retained `rstorrent-direct-file` crate has two layers:

- a runtime-independent version-1 binary codec with exact request IDs,
  offsets, lengths, acknowledgement offsets, completion and typed errors; and
- a feature-gated lower-`rtc` adapter driven by one RSTorrent-owned Tokio task
  and one explicitly bound UDP socket.

The adapter admits one peer and DataChannel, at most four range requests, a
4-KiB control ceiling, 60-KiB payload chunks, a 512-KiB application queue and
512-KiB SCTP queue, 32 remote candidates, 64 KiB of accepted signaling, a
20-second negotiation deadline, 60-second request-inactivity deadline, and a
ten-minute experiment lifetime. Each chunk requires an exact next-offset ACK,
so a slow browser cannot cause unbounded application reads or sends. The
endpoint parses one consistent SHA-256 fingerprint from the bounded offer and
reports it verified only after lower `rtc` authenticates the DTLS peer and the
connection reaches connected state.

The local harness creates a deterministic torrent through the ordinary
`ApplicationService`, records its pieces verified, obtains a normal
`MediaCapabilityLease`, and reads through that lease for every request. It
never accepts a path, storage root, HTTP method/header, or application command.
The browser independently verifies concurrent head, tail, seek-shaped, and
overlapping ranges, an out-of-range rejection, cancellation after the first
chunk, and the complete-file digest. An oversized hostile control frame and a
stale acknowledgement leave the session usable.

The practical browser adapter streams directly into Origin Private File
System through incremental writes and an incremental browser SHA-256. It does
not construct a whole-file Blob. This is sufficient to prove bounded browser
consumption, but it is not yet the product's native Download/Open/Play route.
No active incomplete-file fixture was run, so the first product proposal must
remain completed-file-only until a separate slice proves active verified
waiting, publication handoff, revocation, and removal over this transport.

The lazy product seam is propagated as `direct-file-webrtc` through
`rstorrent-remote-host`, `rstorrent-headless`, and `rstorrent-desktop`. A
feature-on application owns only a dynamic lazy starter until signaling calls
it; startup creates no certificate, candidate, task, or socket. The dynamic
boundary also keeps the real endpoint start path link-reachable, avoiding the
initial measurement error where the linker discarded the unused RTC stack.
The ordinary feature-off graph contains neither `rstorrent-direct-file` nor an
`rtc` package.

Feature unification exposed one real integration defect: RTC selects Ring
while the existing remote host selects AWS-LC, leaving rustls unable to infer
one process default. Existing relay TLS client/server builders now select
AWS-LC explicitly; RTC selects its provider explicitly. The combined remote
host end-to-end matrix passes. Feature-on therefore contains both AWS-LC and
Ring, but no OpenSSL dynamic library or additional non-system dylib.

The selected `rtc` family is dual MIT/Apache-2.0, `ring` is Apache-2.0 AND ISC,
and the newly resolved supporting crates are permissively licensed. No
upstream source, fixture, or test asset was copied. The direct RTC family does
contain audited upstream unsafe surfaces in DTLS padding, the otherwise unused
media buffer package, and Windows interface enumeration; Ring and the existing
AWS-LC also retain their native/unsafe crypto surfaces. This is a maintenance
and security-update cost even for a data-only consumer. Upstream was actively
shipping the 0.20 Sans-I/O line during July/August 2026 and still records
pre-1.0 deterministic-time work, so the exact pin and focused upstream tests
remain part of future update review.

### 2026-08-30: representative release-link cost

The product measurements used Rust/Cargo 1.97.0 on macOS 26.6.1,
`aarch64-apple-darwin`, Apple clang 21.0.0 and Apple `ld` 1267. The repository
release profile is Cargo's ordinary optimized profile without LTO or Cargo
stripping. Executable stripping used `strip -S -x`; executable compression
used `gzip -9`. The unsigned Tauri app used the documented package overlay,
`--bundles app --no-sign --ci`, and the complete app contents were archived
under the same relative root with `COPYFILE_DISABLE=1 tar -czf`.

The paired source was the `fa80328` endpoint-link state. The only difference
within each pair was the additive `direct-file-webrtc` feature. The feature-on
executables contain RTC/ICE/DTLS/SCTP error strings and the reachable endpoint
driver; feature-off trees and executables do not contain the direct-file
crate/path. Product normal/build package count rises from 233 to 315, adding
82 packages. Ring is new to the normal feature-on graph alongside the existing
AWS-LC stack. `cargo bloat` was unavailable; Mach-O section attribution shows
the headless stripped delta is principally `__TEXT` (+3,211,264 bytes), then
`__LINKEDIT` (+294,912) and `__DATA_CONST` (+114,688).

| Artifact | Feature off | Feature on | Delta | Percent | Compressed/package delta |
| --- | ---: | ---: | ---: | ---: | ---: |
| Headless executable, unstripped | 36,398,864 B | 40,684,912 B | +4,286,048 B | +11.78% | - |
| Headless executable, stripped | 29,121,000 B | 32,744,504 B | +3,623,504 B | +12.44% | - |
| Headless stripped gzip | 12,098,383 B | 13,702,231 B | +1,603,848 B | +13.26% | +1,603,848 B |
| Desktop executable, unstripped | 48,427,328 B | 52,669,312 B | +4,241,984 B | +8.76% | - |
| Desktop executable, stripped | 37,087,640 B | 40,645,312 B | +3,557,672 B | +9.59% | - |
| Desktop stripped gzip | 15,048,218 B | 16,638,956 B | +1,590,738 B | +10.57% | +1,590,738 B |
| Unsigned app executable | 47,932,400 B | 52,281,008 B | +4,348,608 B | +9.07% | - |
| Complete unsigned app archive | 16,577,462 B | 18,351,083 B | +1,773,621 B | +10.70% | +1,773,621 B |

Observed release build wall times were 64--72 seconds for the first matched
headless/desktop and package switches; subsequent feature-on relinks were
cache-dependent and are not treated as comparative performance evidence.
Only the current macOS ARM64 link/package pair was available and required by
the stopping condition. Linux/Windows sizes, signed installers, notarization,
and update artifacts were not run. Default-off Android, iOS, remote-relay,
remote-Wasm, and engine trees contain no direct-file or RTC dependency. CI now
keeps the ordinary workspace feature-off gate and adds feature-on product
Clippy plus focused direct-file and remote-host tests on Ubuntu.

### 2026-08-30: real-browser and runtime evidence

The test page is served from loopback HTTP, which browsers treat as a secure
context, while the endpoint binds the explicitly discovered private-LAN IPv4
address. No STUN, TURN, mapping, public listener, DNS, relay payload, or
external service was contacted. Both successful browsers selected a host
candidate and transferred bidirectionally over the one UDP socket.

| Scenario/browser | Direct result | Exact content | Timing | RSS and queue high water | Cleanup | Result |
| --- | --- | --- | --- | --- | --- | --- |
| Chromium 151, 8 MiB | host candidate, DTLS fingerprint verified | 4 concurrent ranges, cancel, hostile controls, full SHA-256 | connect 229 ms; OPFS 25.1 MiB/s; close 30 ms | 29,184 -> 34,336 KiB RSS; 245,946 B combined queues | 0 tasks, sockets, requests and queued bytes | Pass |
| Firefox 153, 8 MiB | host candidate, DTLS fingerprint verified | same trace and SHA-256 | connect 224 ms; OPFS 23.5 MiB/s; close 39 ms | 29,200 -> 34,464 KiB RSS; 368,724 B combined queues | 0 tasks, sockets, requests and queued bytes | Pass |
| Chromium 151, 64 MiB | host candidate, DTLS fingerprint verified | exact `8d5c7f5d...7614dc`; no Blob | OPFS 25.8 MiB/s over 2.483 s | 86,768 -> 91,952 KiB RSS; 307,374 B combined queues | 0 tasks, sockets, requests and queued bytes | Pass |
| Playwright WebKit, 8 MiB | one candidate received; no selected pair or DTLS verification | 0 file bytes | 20-second negotiation timeout | 0 request/queue high water | driver joined; 0 tasks/sockets | Fail |

The 64-MiB harness baseline is higher because deterministic fixture creation
precedes the idle sample and the process allocator retains that memory. Across
both 8-MiB browsers the active Rust process RSS increment was about 5.2 MiB.
Sampled `ps` lifetime-average CPU reached 16.4%--17.5% for the 8-MiB runs and
25.2% for the 64-MiB run; this is useful host evidence but not a cross-platform
CPU benchmark. Immediate post-close RSS retained the allocator high water, but
endpoint counters prove no continuing RTC owner, task, socket, request, queued
byte, candidate work, STUN traffic, or mapping. Browser hashes, not the server,
are the content oracle.

### Validation and deliberate gaps

Passed locally:

- `cargo fmt --all -- --check`;
- direct-file codec/fingerprint/queue tests with `webrtc` enabled;
- feature-on remote-host end-to-end tests and remote-relay TLS tests;
- feature-on checks for remote host, headless, and desktop;
- feature-on Clippy for direct-file, remote host, relay, headless, and desktop
  with warnings denied;
- Node syntax checks and real Chromium/Firefox browser runs;
- both feature-off and feature-on current-host release links plus unsigned app
  packages; and
- `actionlint` for the added CI feature gate and `git diff --check`.

The retained evidence does not claim active incomplete-file behavior, service
worker/native media seeking, capability revocation during an active range,
loss/reordering, fairness beside torrent traffic, interface/sleep changes,
public IPv6, STUN, NAT traversal, firewall/mapping behavior, TURN, Safari,
Linux/Windows link size, or production signaling. These are explicit future
gates, not inferred from LAN success.

### Post-completion correction and final recommendation: Proceed

| Candidate | Version/revision | Runtime/crypto | Browser evidence | Stripped probe delta | Ownership | Disposition |
| --- | --- | --- | --- | ---: | --- | --- |
| lower `rtc` | 0.20.4 / `bbc1866` | caller-owned Sans-I/O + Ring | Chromium and Firefox pass; current Playwright WebKit reruns complete transport and exact ranges | +926,528 B | one joinable task/socket | Retain and integrate |
| `str0m` | 0.23.1 / `120401c` | caller-owned Sans-I/O + RustCrypto/AWS-LC | upstream focused tests only | +2,499,864 B | explicit caller ownership | Remove/not added |
| high-level `webrtc` | 0.20.4 / `843d52e` | async driver + Tokio + Ring | upstream focused tests only | +2,651,208 B | library driver plus caller callbacks | Remove/not added |

The transport is technically sound in Chromium, Firefox, and the currently
installed Playwright WebKit build, the lower-`rtc` ownership model fits
RSTorrent, and the incremental sink proves bounded file consumption. The
measured current-host cost is material but accepted for default desktop and
headless compilation: about 3.6 MiB stripped or 1.6--1.8 MiB
compressed/package.

The original completion evidence remains important: one WebKit run supplied
only one remote candidate and timed out before a selected pair. It is now an
ICE reliability observation rather than a demonstrated interoperability
blocker. Two post-completion reruns reached `ready`, verified the DTLS
fingerprint, completed the concurrent exact-range/cancel/hostile-input corpus,
and transferred about 404 KiB before the browser sink failed. A minimal probe
then isolated that failure to `navigator.storage.getDirectory()` in
Playwright's ordinary non-persistent context; the same WebKit build completed
root, file handle, writable, write, and close in a persistent context. OPFS was
only an experiment sink and is not selected for the product.

That corrected evidence supports **Proceed**. Actual branded Safari and
repeated ICE reliability remain proportional product evidence, not reasons to
misclassify the retained Rust transport. Ready Tactical
[`196`](196-remote-direct-file-product-integration.md) begins
completed-file-only, compiles the endpoint by default in desktop/headless,
authenticates signaling inside the existing remote circuit, uses no OPFS or
TURN, and lands **Save file...** before general Open/Play adapters.
