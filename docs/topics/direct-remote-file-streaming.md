# Direct Remote File Streaming

Topic: `direct-remote-file-streaming`

Status: Direction accepted from maintainer discussion on 2026-08-30. No
implementation, dependency, Cargo feature, network service, or product support
claim is authorized by this topic. The leading investigation is an optional
native Rust WebRTC DataChannel endpoint that uses the existing end-to-end
encrypted remote application circuit only for signaling, attempts a direct
browser-to-host path, and starts no socket or task until an authorized remote
browser requests file bytes. The first experimental build should keep the
feature off by default. Its eventual release default remains deliberately
undecided until package, binary, idle, active, and interoperability costs are
measured. Ready Tactical
[`195`](../tactical/195-webrtc-direct-file-feasibility-spike.md) separately
authorizes the local-only endpoint bake-off, real-browser/file-range prototype,
and exact feature-off/on cost measurement; it does not authorize product
integration or deployment.

## Purpose And Ownership

The remote React UI can control and inspect the real RSTorrent application,
but its authenticated relay circuit intentionally carries no torrent payload
or file content. The local media service already supplies bounded complete and
incomplete-file range reads. This topic owns the missing transport and browser
presentation seam between those two implemented capabilities.

Specifically, it owns:

- direct remote file-byte transport from a native desktop or configured
  headless RSTorrent host to an authenticated browser;
- direct-path discovery, selection, fallback posture, and privacy;
- optional compilation, product packaging, and lazy runtime ownership;
- the bounded file-range protocol carried over a non-HTTP transport;
- browser-side download, viewing, seeking, and playback adapters;
- direct-transfer audit, cancellation, and operator visibility; and
- the investigations required before choosing a Rust WebRTC dependency or
  enabling the feature by default.

[`http-file-serving-and-streaming.md`](http-file-serving-and-streaming.md)
continues to own media eligibility, opaque per-file capabilities, verified
logical reads, incomplete-file demand, resource ceilings, and storage
generation fencing. This topic must reuse those authorities rather than create
a second file reader or infer verification from bytes on disk.
[`remote-access-authentication.md`](remote-access-authentication.md) owns the
owner passphrase, host identity, remembered-browser authorization, revocation,
security history, and encrypted control circuit.
[`application-connection-architecture.md`](application-connection-architecture.md)
owns the typed application frames and the rule that bulk file content does not
enter the application control connection.
[`incoming-reachability-and-seeding.md`](incoming-reachability-and-seeding.md)
owns BitTorrent peer-listener reachability. Its mappings and sockets are not
remote-file authority merely because some discovery mechanisms overlap.

## Desired Product Outcome

From the remote UI, an authorized owner should be able to select an eligible
torrent file and choose **Open**, **Play**, or **Download**. RSTorrent then:

1. creates a short-lived file-scoped capability inside the authenticated
   application circuit;
2. starts a direct endpoint lazily if one is not already suitable for that
   browser;
3. exchanges bounded connectivity and endpoint-authentication material through
   the existing encrypted control circuit;
4. attempts the best direct path available between browser and host;
5. serves only requested verified ranges with explicit backpressure and
   cancellation; and
6. tears down the peer, socket, mapping, reads, and transient download demand
   after revocation, terminal failure, shutdown, or a bounded idle period.

The UI should say whether it is connecting, direct, unavailable, or stopped.
It should not ask the user for an IP address, port, certificate exception, or
second password during the ordinary path. A failed direct-file attempt must
not disconnect or degrade the remote control UI. The first implementation must
not silently send file bytes through the control relay as a fallback.

## Decisions And Invariants In Force

- **Direct first.** The initial product investigation sends file bytes directly
  between browser and host. The deployed opaque relay remains signaling and
  control infrastructure, not a payload proxy.
- **WebRTC DataChannel leads.** It combines a browser-native endpoint, ICE path
  checks, DTLS authentication/encryption, SCTP reliability, congestion control,
  and bidirectional binary messages without requiring audio or video tracks.
- **Signaling stays inside the authenticated circuit.** SDP, ICE candidates,
  DTLS fingerprints, protocol negotiation, and transfer grants are typed and
  bounded messages protected by the existing owner-authenticated channel. No
  new unauthenticated signaling endpoint is introduced.
- **The native host embeds the other peer.** Desktop and headless builds would
  contain a Rust WebRTC endpoint when compiled with the feature. A hidden
  desktop WebView, browser extension, companion daemon, and Google libwebrtc
  process are not the selected architecture.
- **Compilation and activation are separate.** Including the optional code in
  a package must not bind UDP, enumerate interfaces, contact STUN, create a
  mapping, generate continuing traffic, or retain a background task. Runtime
  work begins only for an authorized file request and ends observably.
- **The release default is evidence-driven.** The first experimental feature is
  default-off. No permanent enabled-by-default or disabled-by-default product
  decision exists until exact binary and package deltas, idle behavior, active
  resource use, compatibility, and maintenance costs are recorded.
- **No unencrypted hole.** UPnP or an IPv6 firewall pinhole may make a UDP
  candidate reachable, but every accepted file byte still travels through the
  endpoint-authenticated DTLS transport. RSTorrent must never publish a plain
  HTTP media listener through a gateway mapping.
- **Existing file authority remains final.** A transport grant names an opaque
  registry entry, never a path. It cannot browse directories, read neighboring
  files, widen a range beyond one logical file, or serve unverified content.
- **No TURN in the first direct-only slice.** NATs that cannot establish a
  direct candidate pair produce a clear unavailable result. TURN or another
  byte-relay fallback remains a separate cost, privacy, and product decision.
- **No new mobile-host claim.** The first investigation targets the already
  implemented desktop and configured headless remote hosts. Android and iOS
  hosting, background policy, and platform file presentation remain separate.

## Leading WebRTC Shape

WebRTC DataChannels carry SCTP over DTLS over ICE/UDP. That supplies NAT
traversal, confidentiality, endpoint authentication, integrity, reliable or
partially reliable messages, and multiple logical streams independently of
audio/video media. RSTorrent needs the data transport stack, not capture,
rendering, codecs, RTP media tracks, transcoding, or an SFU.

```text
remote React page                     native RSTorrent host
-----------------                     ----------------------
authenticated remote application circuit through opaque relay
    |                                        |
    +-- offer/answer, ICE, fingerprint ------+
    +-- file capability and cancellation ----+

browser RTCPeerConnection             optional Rust WebRTC peer
    |                                        |
    +======= direct ICE/DTLS/SCTP ===========+
                 DataChannel
                                               |
                                    bounded range protocol
                                               |
                                    media capability lease
                                               |
                             verified/active logical-file reader
```

The DTLS certificate should be ephemeral to the direct-peer generation. Its
fingerprint is authenticated by the existing encrypted host/browser circuit;
it is not a second user-entered pin and does not replace the durable RSTorrent
host identity. A new or resumed authorized browser already knows which host it
is controlling before direct negotiation begins.

### Candidate paths

The host and browser should let ICE validate and select among a bounded set of
eligible paths:

1. same-LAN interface candidates;
2. directly reachable public IPv6 candidates;
3. STUN-derived server-reflexive IPv4 or IPv6 candidates and ordinary UDP hole
   punching;
4. a short-lived explicitly advertised UDP mapping created through UPnP IGD v2
   when the library permits it; and
5. a short-lived IPv6 firewall pinhole where the platform, gateway, and
   selected candidate require one.

The existing session reachability code already implements supervised UPnP IGD
v2 TCP/UDP mappings and IPv6 firewall pinholes for the BitTorrent peer
listeners. A future tactical may extract and reuse its deterministic discovery
and gateway protocol components. The direct-file owner must still have its own
ephemeral UDP socket, lease, renewal, generation, cleanup, and audit state. It
must not borrow the torrent peer port or mutate the peer listener's advertised
endpoint.

STUN is rendezvous metadata, not a payload proxy. Its operator can observe the
source address and timing of binding traffic. The authenticated remote browser
also necessarily learns the selected host candidate address. Candidate
disclosure therefore happens only after owner authentication, stays bounded,
and is never copied into ordinary persistent logs. Without TURN, symmetric
NAT, restrictive enterprise filtering, or incompatible UDP policy will remain
honest expected failures.

## Transport Options

| Option | Useful property | Principal limitation | Current posture |
| --- | --- | --- | --- |
| Existing direct HTTPS media route | Preserves native browser `GET`, `HEAD`, Range, download, and media behavior with no new file protocol. | Requires the browser to reach a correctly authenticated HTTPS host origin. It does not discover or traverse arbitrary NAT by itself. | Keep for LAN, private-host, and accepted overlay deployments; use immediately when already reachable. |
| WebRTC DataChannel | Browser-native direct path with ICE checks and fingerprint-authenticated DTLS; needs no media codecs. | Native host must embed ICE, DTLS, SCTP, SDP, UDP, timers, and congestion/backpressure machinery. Some NATs still require TURN. | Leading general investigation. |
| WebTransport over HTTP/3 | Native bidirectional streams and browser-supported certificate hashes could fit the range protocol cleanly. | Supplies no ICE candidate discovery or connectivity checks itself; still needs a reachable UDP endpoint, mapping/public address, and uneven browser support must be proved. | Retain as a focused comparison, especially for public IPv6 or mapped UDP. |
| Per-host DNS plus WebPKI HTTPS | Gives the browser a real URL and keeps the existing HTTP range path. | Requires dynamic DNS, certificate issuance/renewal, inbound reachability, key recovery, and a safely exposed HTTPS listener. A shared wildcard private key must never be distributed to hosts. | Possible later operator/product service; not the first zero-configuration path. |
| User-managed overlay or reverse proxy | Mature connectivity, DNS, TLS, and ordinary HTTP semantics. | Requires installation or network administration and may route through another provider. It is not universal owner UX. | Existing explicit operator mode, not a replacement for direct discovery. |
| TURN or an E2E file relay | Reaches many networks that direct ICE cannot. TURN retains WebRTC endpoint encryption. | Every byte is proxied, creating bandwidth, abuse, availability, and operating-cost obligations. The current control relay is not sized or authorized for it. | Separate opt-in fallback investigation after direct evidence. |
| Binary file records over the current opaque relay | Reuses the established authenticated circuit and works wherever remote control works. | Couples bulk data to control fairness, proxies all bytes, expands relay abuse/cost, and violates the direct-first goal. | Explicit non-goal for the first implementation. |

Plain mapped HTTP is not an option. An unencrypted UPnP opening would expose a
bearer URL and file traffic to active network attackers, conflict with the
HTTPS remote page, and make router state part of file authority.

## Rust Dependency Investigation

The browser supplies its WebRTC peer. The native host still needs a conforming
implementation of ICE/STUN, UDP connectivity checks, SDP negotiation, DTLS,
SCTP/DataChannel, retransmission, timers, flow control, and shutdown. This is
substantial protocol code even when no audio/video feature is used; RSTorrent
must not implement that stack ad hoc.

The first bake-off should compare at least:

- **`webrtc-rs/webrtc`.** The current `0.20.x` line offers an async
  `PeerConnection` API over a Sans-I/O core, includes a Tokio runtime backend,
  and has direct DataChannel examples. It is likely the shortest route to a
  browser interoperability proof, but its resolved and linked media-related
  graph must be measured rather than assuming dead-code elimination makes it
  small.
- **`webrtc-rs/rtc`.** The lower Sans-I/O core may offer a narrower and more
  explicitly owned integration if the high-level driver is too large or
  difficult to supervise. It creates more socket, timer, and event-loop work
  for RSTorrent.
- **`str0m`.** Its `Rtc` value performs no networking, starts no threads or
  async tasks, and exposes network output, events, and timeouts to the caller.
  It supports DataChannels and fits RSTorrent's explicit-owner style, while its
  own documentation notes that peer-to-peer use receives less testing than its
  server/SFU use and that interface enumeration and TURN are caller concerns.

Google's C++ libwebrtc through FFI is not a baseline candidate. Its build,
binary, platform, unsafe-boundary, update, and codec/media surface are
disproportionate for a data-only first slice. Reconsider it only if pure-Rust
options fail recorded browser interoperability or correctness gates.

Before choosing any crate, record its exact version/revision, transitive graph,
platform crypto providers, unsafe code and native-build requirements, license
and notice obligations, maintenance activity, published security posture, and
known browser interop issues. No source or test fixture should be imported
during a dependency spike.

## Optional Compilation And Packaging

The intended Cargo shape is an additive, leaf-level feature provisionally
called `direct-file-webrtc`; the implementing tactical may refine the name.

- The WebRTC dependency graph is optional and must not enter engine, protocol,
  session, Android, iOS, relay, Wasm, or ordinary CLI artifacts merely because
  those crates share the workspace.
- A small runtime-independent direct-file frame codec may remain separate from
  the optional endpoint, but feature unification must not accidentally pull
  the endpoint into mobile or unrelated binaries.
- Desktop and headless package manifests propagate the feature deliberately.
  Release metadata and the application capability handshake report whether it
  was compiled.
- When absent, the remote UI receives a typed unsupported capability and does
  not offer a path that can fail only at click time.
- When present but runtime-disabled, the setting is visible and no endpoint is
  started. Compilation does not imply operator consent to interface discovery,
  STUN, or UPnP.
- CI must build and test both feature-off and feature-on graphs. The off build
  must prove that WebRTC and its unique crypto/SCTP dependencies are absent.

The first experimental feature-on artifact remains opt-in. Whether ordinary
desktop or headless releases enable it later depends on measured installed and
compressed package deltas, platform reliability, security maintenance burden,
and the percentage of real connections that become direct. No arbitrary size
budget is accepted yet.

## Runtime Ownership And Cancellation

The expected ownership shape is:

```text
application/profile owner
  -> remote-access owner
       -> optional direct-file supervisor (created on first request)
            -> bounded browser-peer generations
                 -> UDP socket + ICE/DTLS/SCTP driver
                 -> optional mapping/pinhole lease
                 -> bounded file-request owners
                      -> media capability lease
                      -> verified-range wait/read
                      -> DataChannel backpressure
```

One browser peer should be able to multiplex a bounded number of range
requests. Whether to use one reliable ordered channel, several SCTP streams,
or a small control channel plus data channels is a measurement question;
large ordered messages must not create head-of-line stalls for seek or cancel.
Messages remain far below negotiated browser maxima, and sender
`bufferedAmount`/library backpressure must bound queued data.

Cancellation must flow from file-request cancel, seek replacement, capability
revocation, authorization revocation, direct-peer replacement, timeout,
profile replacement, remote-access disable, and application shutdown. Every
task, read, range wait, transient streaming-demand lease, UDP socket, mapping,
and pinhole must terminate and join. Uncertain gateway cleanup is an operator-
visible bounded state, never silently reported as removed.

Whether an established direct transfer may briefly survive loss of the relay
control circuit remains open. The conservative first behavior is to stop
admitting new ranges and close after a short bounded reconnection grace, while
local revocation and shutdown always take effect immediately.

## File-Range Protocol

DataChannel is message-oriented, not an HTTP URL. A narrow typed binary
protocol should preserve the useful media contract without tunneling arbitrary
HTTP:

- negotiate one protocol version, chunk limit, request ceiling, and supported
  operations;
- open an opaque capability bound to profile generation, authorization,
  direct-peer generation, torrent, and file index;
- request metadata, a complete representation, or one byte range with a
  caller-generated bounded request ID;
- return exact length, MIME hint, availability, accepted range, binary chunks
  with exact offsets, terminal success, or a closed typed error;
- cancel or replace a request promptly after seek or consumer abandonment;
- enforce global, per-peer, per-capability, request, read, queued-byte, and idle
  limits before allocating or touching storage; and
- preserve the existing wait-for-verification, publication handoff, storage
  generation, and no-progress behavior for active files.

The protocol never accepts a filesystem path, URL, HTTP header map, method
string, storage root, SAF identifier, or arbitrary application command. DTLS
protects transfer integrity in flight; only the existing torrent verification
authority decides whether source bytes may be emitted.

## Browser Consumption Is A Separate Gate

A DataChannel cannot be assigned directly to `<video src>`, an `<a download>`
navigation, the built-in PDF viewer, or another URL consumer. A transport proof
alone therefore does not complete remote file viewing. The browser adapter
must be selected and tested explicitly.

Candidate adapters are:

1. **Bounded Blob/object URL.** Simple for small complete images, documents,
   and downloads, but it buffers the entire file and is unacceptable as a
   general large-file path.
2. **Stream-to-file API.** Write a `ReadableStream` to a user-selected file
   where the browser exposes a suitable save-file API. This can bound memory
   but is not a portable assumption across every supported browser.
3. **Service-worker synthetic media route.** Intercept a same-origin ephemeral
   URL, translate `GET`/`HEAD`/Range fetches into page-owned DataChannel range
   requests, and return streaming responses. This most closely preserves the
   existing media-element contract, but worker/page lifetime, navigation,
   reload, multiple ranges, backpressure, cancellation, and browser-specific
   behavior are substantial gates.
4. **Media Source Extensions.** Feed selected audio/video containers to an
   embedded player. This may provide good playback control but is not a general
   file viewer and can require container parsing or segmentation that
   RSTorrent does not otherwise own.
5. **A real direct HTTPS URL.** When an already trusted direct gateway, overlay,
   or future DNS/TLS route is reachable, retain the existing media URL and let
   the browser perform native ranges, download, PDF, audio, and video handling.

The first browser investigation should prove bounded download and one seeking
media-element path without buffering the whole file. It must run in real
Chromium, Firefox, and WebKit/Safari-shaped environments before general support
is claimed. Small-file success in one browser is not sufficient evidence for
remote streaming.

## Security, Privacy, And Audit

- Authenticate the ephemeral DTLS fingerprint, ICE credentials, protocol
  version, authorization ID, host ID, and direct-peer generation inside the
  existing encrypted control transcript. A relay-injected candidate or
  fingerprint must fail closed.
- Admit signaling only after full password login or successful authorized-
  browser resume. A routing name, browser label, candidate, source address, or
  DataChannel message never creates authority.
- Bound hostile SDP/candidate text, candidate count, addresses, mDNS names,
  connectivity checks, retransmission, SCTP streams, message sizes, queued
  bytes, and negotiation time before state growth.
- Expose no file capability, ICE password, DTLS key, exact private address, or
  file path in logs, diagnostics, URLs, support exports, or durable history.
- Record the selected route only as a useful class such as LAN, public IPv6,
  STUN-reflexive, or mapped UDP unless a transient local diagnostic view
  explicitly needs the exact endpoint.
- Surface each live direct peer in the existing remote security view with its
  verified browser authorization, start/last-active time, state, path class,
  active request count, bytes sent, and close action.
- Add bounded security events for negotiation started, connected, rejected,
  failed, revoked, idle-closed, mapping-cleanup uncertainty, and terminal
  shutdown. Persistent events should not include file names or exact ranges by
  default.
- Local individual/global authorization revocation, password-everywhere,
  passphrase change, remote disable, and profile recovery must close matching
  direct peers and revoke their file capabilities before reporting success.
- Do not add a second application-layer content cipher merely by habit. DTLS
  is end-to-end once its fingerprint is authenticated by the existing circuit;
  add another layer only if a concrete threat or implementation boundary
  requires it.

## Required Measurements Before A Dependency Or Default Decision

### Build and distribution

- exact `cargo tree -e features` delta for every candidate and crypto backend;
- stripped executable and compressed installer/archive deltas for macOS arm64,
  Windows x86_64, Linux x86_64, and Linux arm64 desktop/headless targets;
- duplicate TLS, crypto, SCTP, RTP/SRTP, and native-library contribution;
- confirmation that feature-off Android, iOS, relay, Wasm, CLI, and engine
  artifacts are unchanged; and
- license/notice and vulnerability-audit delta for each packaged graph.

### Idle and active runtime

- zero WebRTC tasks, UDP sockets, candidate enumeration, STUN requests, UPnP
  leases, and continuing memory after startup when no direct request exists;
- first-start latency and peak/steady RSS, allocations, task count, socket
  count, queue high water, mapping lifetime, and complete teardown residue;
- large-file throughput, CPU, retransmission, loss behavior, cancellation
  latency, fairness beside torrent traffic, and several concurrent range
  requests; and
- application control responsiveness while a direct transfer is saturated.

### Connectivity and browser behavior

- same-host and same-LAN paths;
- direct public IPv6, ordinary STUN hole punching, UPnP-mapped UDP, restrictive
  NAT failure, firewall denial, interface change, sleep/wake, and route change;
- no-TURN failure classification and success rate across representative home,
  cellular, guest, VPN, CGNAT, and enterprise networks;
- current Chromium, Firefox, and WebKit/Safari offer/answer, trickle ICE,
  DataChannel, message-size, backpressure, restart, and close behavior; and
- complete and active-file head/tail probe, seek, overlap, abandonment,
  publication handoff, revocation, and shutdown through the chosen browser
  adapter.

The release-default decision should be made from this recorded evidence. A
small dependency alone does not justify enabling a fragile or low-success
feature, and a larger dependency may still be justified if it is reliable,
lazy, maintainable, and materially improves remote use.

## Recommended Investigation Sequence

When implementation is authorized, create one bounded tactical before changing
dependencies or code. It should proceed in these stages with explicit stop/go
evidence:

1. Re-read the current WebRTC, ICE, DataChannel, security, and browser contracts;
   pin candidate library sources and licenses; inspect their exact DataChannel
   tests and examples.
2. Build isolated release examples for `webrtc-rs/webrtc`, its lower Sans-I/O
   core if practical, and `str0m`; connect a real browser over loopback and LAN;
   measure stripped/package graph deltas before integrating any candidate.
3. Select one candidate provisionally and prove a lazy supervised endpoint,
   authenticated fingerprint signaling, direct DataChannel echo, cancellation,
   and zero idle residue through the real remote circuit.
4. Define and fuzz the bounded range codec, then reuse the existing media
   capability lease for completed and active verified files.
5. Compare browser consumption adapters and prove bounded download plus seeking
   playback without whole-file buffering.
6. Add STUN, public IPv6, and optionally separate short-lived UPnP UDP/pinhole
   candidates; retain direct-only truthful failure when no pair succeeds.
7. Run packaged platform, browser, network, audit, revocation, resource, and
   shutdown evidence before deciding whether any release enables the Cargo
   feature or runtime setting by default.

Do not begin with TURN, public DNS automation, wildcard certificates, media
relay capacity, Android hosting, transcoding, or a stable public wire promise.
Those choices can be evaluated independently after the direct path has real
cost and success-rate evidence.

## Open Decisions

- Which pure-Rust endpoint provides the best correctness/size/ownership tradeoff?
- What binary and package deltas are acceptable for default desktop and
  headless builds?
- Should runtime direct streaming default on when compiled, or require a host
  setting before any interface/STUN/UPnP work?
- Which STUN service, privacy statement, abuse policy, and availability model
  are acceptable?
- Is UPnP mapping attempted automatically after an authenticated request,
  separately opted in, or omitted from the first product slice?
- Should one direct peer live per browser authorization, live control circuit,
  or file-transfer generation?
- How long may an active direct transfer survive temporary control-relay loss?
- Which browser adapter gives portable range and save behavior without
  unbounded memory or container-specific code?
- Is an explicit user-selected TURN/file-relay fallback ever desirable, and
  who operates and pays for it?
- When should Android or iOS act as a remote controller or host, if ever?

## Deliberate Non-Goals

- Replacing the authenticated application WebSocket or sending control frames
  through WebRTC.
- Serving arbitrary host files, directories, storage roots, or unverified
  torrent bytes.
- Publishing the existing loopback HTTP listener through UPnP or exposing an
  unauthenticated LAN/public file server.
- Bundling camera, microphone, capture, render, codec, transcoding, SFU, RTP
  media-track, or screen-control functionality.
- Sharing a wildcard TLS private key among RSTorrent hosts.
- Claiming that WebRTC always establishes a direct path without TURN.
- Enabling, deploying, or supporting a new feature merely because this design
  topic exists.

## Primary External References

- [RFC 8831, WebRTC Data Channels](https://www.rfc-editor.org/rfc/rfc8831.html)
  defines SCTP over DTLS over ICE/UDP, reliable and partially reliable data
  transfer, multiplexed streams, congestion control, and the file-transfer use
  case.
- [RFC 8445, Interactive Connectivity Establishment](https://www.rfc-editor.org/rfc/rfc8445.html)
  defines host, server-reflexive, peer-reflexive, and relayed candidates plus
  connectivity checks and candidate lifecycle.
- The [W3C WebRTC Recommendation](https://www.w3.org/TR/webrtc/) defines the
  browser `RTCPeerConnection` and `RTCDataChannel` API surface.
- The [W3C WebTransport specification](https://www.w3.org/TR/webtransport/)
  defines browser streams/datagrams and optional server certificate hashes;
  it remains an alternative investigation rather than an accepted transport.
- The [W3C Service Workers specification](https://www.w3.org/TR/service-workers/)
  defines intercepted fetch and synthetic response behavior relevant to a
  possible same-origin range adapter.
- [`webrtc-rs/webrtc`](https://github.com/webrtc-rs/webrtc) and
  [`webrtc-rs/rtc`](https://github.com/webrtc-rs/rtc) are the async and Sans-I/O
  pure-Rust candidates observed during the 2026-08-30 survey.
- [`str0m`](https://github.com/algesten/str0m) is the alternative Sans-I/O Rust
  candidate observed during the same survey.

No external source, fixture, or test data has been copied into RSTorrent.
