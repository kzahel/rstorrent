# Tactical 196: Remote Direct-File Product Integration

Status: **Implementation complete; qualification remains active.** The product
path, operator controls, audit, default build graph, direct-only signaling,
and completed-file Save are implemented and pass the current-host product
matrix. Native Windows and Linux ARM64 compilation also pass. An
independent-network direct path and a real browser streaming save picker
remain bounded evidence gaps, so this tactical does not yet claim completion
or supported availability. This tactical turns completed Tactical
[`195`](195-webrtc-direct-file-feasibility-spike.md)'s retained lower-`rtc`
endpoint into one default-compiled, lazy, authenticated remote-file product
slice. It adds completed-file **Save file...** from the remote React UI,
direct-only ICE with one documented public STUN service, explicit operator
control and audit, and zero payload bytes through the application relay. TURN
is not implemented, configured, provisioned, or planned by this slice.

Topics:
[`direct-remote-file-streaming`](../topics/direct-remote-file-streaming.md),
[`http-file-serving-and-streaming`](../topics/http-file-serving-and-streaming.md),
[`remote-access-authentication`](../topics/remote-access-authentication.md),
[`application-connection-architecture`](../topics/application-connection-architecture.md),
[`application-view-api`](../topics/application-view-api.md),
[`web-ui-design`](../topics/web-ui-design.md),
[`client-surfaces`](../topics/client-surfaces.md), and
[`capability-readiness`](../topics/capability-readiness.md).

Dependencies: completed Tacticals
[`138`](138-verified-http-file-serving.md) and
[`139`](139-incomplete-file-streaming-demand.md) supply the bounded logical
file, capability, verification, and range-read authority. Completed Tacticals
[`190`](190-opaque-wasm-relay-foundation.md) and
[`192`](192-production-owner-relay-access.md) supply the host identity,
password login, remembered-browser resume, encrypted circuit, revocation,
security ledger, production-shaped relay, and mature remote React surface.
Completed Tactical [`195`](195-webrtc-direct-file-feasibility-spike.md) retains
the default-off lower `rtc` 0.20.4 endpoint, versioned range codec, browser
harness, exact size/resource measurements, and feature-on CI gate that this
slice must promote rather than replace.

## Decision And Desired Outcome

An authenticated remote user viewing one completed eligible torrent file can
choose **Save file...**. RSTorrent obtains the destination during that user
gesture, creates a circuit-bound logical-file authority on the native host,
negotiates a direct WebRTC DataChannel through encrypted sideband records, and
streams exact verified chunks into a bounded browser sink. The UI reports
connecting, direct, unavailable, cancelled, and complete truthfully. Failure
to establish or consume the direct path never closes or degrades the ordinary
remote application connection.

Desktop and configured-headless products compile the retained endpoint by
default. Compilation still creates no certificate, task, socket, candidate,
DNS request, STUN request, or background traffic. Runtime direct transfer is
enabled by default only after the operator has enabled remote access, and the
first network work begins only after an authenticated browser explicitly
requests one eligible file. The Remote Access settings surface exposes one
default-on **Direct file transfers** kill switch and live bounded status.

The first product path is deliberately a save operation, not a universal URL
adapter. It does not use OPFS. Browsers with a user-visible streaming save
picker write directly to the selected file. Other browsers may use an exact
small-file Blob fallback capped at 32 MiB; larger files receive a truthful
unsupported-browser result. General Open, PDF/image viewing, audio/video Play,
seekable media, and service-worker synthesis remain later tacticals.

## Accepted Product Direction

- **Compiled by default, activated lazily.** Desktop and headless leaf
  manifests enable `direct-file-webrtc` by default. The library feature stays
  optional so Android, iOS, relay, Wasm, engine, session, and unrelated CLI
  artifacts do not acquire RTC/DTLS/SCTP/Ring merely through workspace feature
  unification. Feature-off CI uses explicit no-default-feature builds.
- **Direct transfer defaults on with a kill switch.** Enabling remote access
  permits an explicit remote Save action to attempt a direct connection.
  Turning direct transfer off cancels and joins active endpoint owners and
  rejects new generations without disabling ordinary remote control.
- **One authenticated signaling authority.** SDP, ICE candidates, DTLS
  fingerprints, peer generations, close, and typed failures travel only in
  the existing end-to-end encrypted host/browser circuit. The opaque relay
  forwards bounded ciphertext and never parses candidates or file authority.
- **No file payload through the relay.** Range frames exist only on the
  DataChannel. There is no automatic or manual payload fallback to the relay,
  application WebSocket, HTTP control gateway, or remote-security records.
- **One public STUN service initially.** The exact initial service is
  `stun:stun.cloudflare.com:3478`, which Cloudflare documents as free and
  unlimited. Both browser and host may contact it only for an authorized
  direct-file generation. An unavailable STUN service degrades to host/public
  IPv6 candidates or a typed direct-unavailable outcome; it never affects
  login or control.
- **No TURN.** The product supplies no TURN URL or credential and rejects
  relayed ICE candidates. RSTorrent does not host, buy, provision, or silently
  discover a TURN service. A future reversal requires an explicit product,
  privacy, abuse, availability, and operating-cost decision; it is not the
  presumed next step after a failed direct path.
- **Completed verified files first.** The host issues authority only when the
  incumbent application/media state says that the requested logical file is
  complete and available. This slice neither waits for incomplete pieces nor
  changes streaming-demand selection.
- **A circuit owns a peer.** The first shape permits one direct peer generation
  per authenticated circuit and at most four direct peers per host process,
  although the current relay admits less concurrency. Reload/resume creates a
  fresh WebRTC generation; WebRTC state itself is not persisted or resumed.
- **Circuit loss closes the peer.** The first product does not let a file
  transfer outlive its authenticated control circuit. This makes revocation,
  sign-out, route replacement, and shutdown prompt and auditable.

## User And Operator Scenarios

### Successful streaming save

1. A password-authenticated or resumed browser selects one completed file and
   chooses **Save file...**.
2. When available, the browser invokes its user-visible save picker during
   that click, before asynchronous signaling consumes transient activation.
3. The browser opens a bounded direct generation through the authenticated
   circuit. The host creates an internal opaque media capability; the token is
   never exposed as a path or public HTTP URL.
4. Host and browser gather bounded host and server-reflexive candidates. They
   authenticate the negotiated DTLS fingerprint because the exact SDP and
   candidates came through the encrypted circuit.
5. The browser writes sequential chunks with backpressure, verifies exact
   length and terminal success, closes the destination, and reports completion.
6. The endpoint closes after the bounded idle period or explicit close and
   every task, socket, request, capability lease, and queued byte returns to
   zero.

### Direct path unavailable

STUN DNS/response failure, restrictive or symmetric NAT, blocked UDP,
candidate exhaustion, negotiation timeout, unsupported browser saving, and
provider outage produce distinct bounded local classifications but one simple
user result: **Direct connection unavailable** with a retry action. Ordinary
views and controls remain live. The UI never promises that direct WebRTC works
on every network and never suggests that bytes were relayed.

### Revocation and operator stop

Authorization revocation, circuit close, sign-out, remote-access disable,
direct-transfer disable, profile replacement, or application shutdown cancels
the direct peer and its file request before returning. Late candidates,
chunks, acknowledgements, and terminal records from the old generation cannot
reopen authority or affect a replacement peer.

### Unsupported large-file sink

A browser without a user-visible streaming file destination may save a file
only through the bounded 32-MiB Blob fallback. A larger file is rejected before
opening WebRTC or allocating payload storage. OPFS is not used as a product
destination or hidden staging area.

## Stopping Condition

This tactical is complete only when all of the following are committed with
reproducible evidence:

1. Desktop and configured-headless default release graphs include the retained
   lower-`rtc` start path. Startup with no Save request still proves zero RTC
   tasks, sockets, certificates, candidate enumeration, DNS, STUN, and queued
   bytes. Explicit feature-off builds prove the unique graph remains absent
   from Android, iOS, relay, Wasm, engine/session, and unrelated CLI targets.
2. A versioned bounded signaling protocol advertises `direct_file_v1`, binds
   every request to the authenticated circuit and fresh peer generation,
   exchanges offer/answer and trickle candidates in both directions, rejects
   stale/conflicting/oversized messages, and never puts file chunks in relay
   records.
3. The real remote runtime owns a lazy direct-file supervisor rather than an
   unused factory. One completed logical file flows from the incumbent
   `ApplicationService` and `MediaCapabilityLease` through the retained range
   codec without accepting a host path, arbitrary capability token, URL,
   header map, or unverified content from the browser.
4. The mature remote React Files surface advertises the capability only when
   the host compiled and enabled it, distinguishes **Save file...** from the
   existing torrent **Download now** action, and renders connecting, direct,
   progress, unavailable, cancellation, and completion without disrupting
   ordinary remote controls.
5. A real supported streaming-save browser writes an exact completed fixture
   incrementally into a user-selected or automation-equivalent bounded sink.
   Browser-independent tests prove the sink seam and cancellation. Firefox and
   Playwright WebKit prove the transport separately from OPFS; their product
   behavior is either the exact 32-MiB fallback or a truthful large-file
   unavailable result. Actual Safari evidence is recorded when available and
   never inferred from Playwright WebKit.
6. Host and browser each gather at least one controlled server-reflexive
   candidate through `stun:stun.cloudflare.com:3478`, and one controlled
   different-network direct transfer passes when the available test topology
   permits it. STUN outage, blocked UDP, and a topology requiring relay produce
   clean direct-only failure with zero payload bytes and zero residue. No TURN
   URI, credential, allocation, or relay candidate appears in code, config,
   logs, captures, or evidence.
7. The operator setting defaults on, disables new work, promptly joins active
   work, and reports bounded live state. The durable security audit records
   peer start and terminal outcome with authorization/circuit identity,
   torrent ID, file index, byte count, candidate class, and bounded failure
   class, never a filesystem path, SDP, candidate address, STUN response,
   capability token, passphrase, key, or file content.
8. Password login, automatic authorized-browser resume, browser reload,
   circuit replacement, exact authorization revocation, direct disable,
   application shutdown, malformed signaling, slow sink, cancel, and provider
   failure all leave zero direct tasks, sockets, range requests, leases, and
   queued bytes. The existing no-bulk relay and remote-security matrices remain
   passing.
9. Current Chromium, Firefox, and Playwright WebKit pass their applicable
   deterministic and real-browser matrices on macOS. Representative native
   Windows and Linux host builds pass, and at least one available native host
   completes a production-shaped relay-to-browser direct save. Public website,
   relay, package, tag, release, or signed-candidate deployment remains
   separately authorized.
10. The tactical execution record, owning topics, capability scoreboard,
    protocol claim, public-STUN privacy statement, release/package delta, and
    exact validation commands reflect what actually passed and what remains.

## Normative And Reference Survey

Before finalizing signaling and candidate state transitions, re-read and
record the relevant sections and hostile/lifecycle cases from:

- [RFC 8831](https://www.rfc-editor.org/rfc/rfc8831.html), reliable WebRTC
  DataChannels, message sizing, congestion, closure, and file transfer;
- [RFC 8832](https://www.rfc-editor.org/rfc/rfc8832.html), DataChannel
  establishment and label/subprotocol behavior;
- [RFC 8445](https://www.rfc-editor.org/rfc/rfc8445.html), full ICE roles,
  candidate pairs, connectivity checks, nomination, peer-reflexive discovery,
  consent, restart, and failure;
- [RFC 8489](https://www.rfc-editor.org/rfc/rfc8489.html), STUN transaction,
  authentication-independent binding behavior, retransmission, response
  validation, DNS, and resource limits;
- [RFC 8827](https://www.rfc-editor.org/rfc/rfc8827.html), WebRTC security and
  authenticated DTLS fingerprints;
- the [W3C WebRTC Recommendation](https://www.w3.org/TR/webrtc/), browser
  signaling, ICE gathering/connection state, candidates, DataChannels,
  `bufferedAmount`, and statistics;
- the [File System Access specification](https://wicg.github.io/file-system-access/),
  user activation, save picker, writable stream, abort, and error behavior;
- the exact retained crates.io `rtc` 0.20.4 source corresponding to reviewed
  revision `bbc18664cf2dcb690e023c6a1a436eb15253ca7f`, especially its ICE,
  STUN, mDNS, candidate, peer-connection, DataChannel, timeout, and Sans-I/O
  tests/examples; and
- Cloudflare's official
  [Realtime TURN FAQ](https://developers.cloudflare.com/realtime/turn/faq/),
  which documents `stun.cloudflare.com` as a free unlimited STUN service. Only
  the unauthenticated STUN endpoint is selected; Cloudflare TURN is not.

Inspect the matching current browser Web Platform Tests and the product's
existing remote-host/runtime/browser tests. Libtorrent and JSTorrent do not
implement this remote WebRTC transport, so no libtorrent transport behavior is
adopted. Inspect JSTorrent only for file-action naming and product history; the
RSTorrent application/media authority remains the implementation source.

No external source, fixture, SDP, candidate, or test vector is copied into the
repository without a separate provenance and license decision.

## Wire And Compatibility Contract

Direct signaling is a new typed encrypted sideband beside, not inside, the
generic application JSON gateway:

```text
remote browser
  -> opaque relay WebSocket
       -> authenticated secure-record channel
            -> application JSON frames (unchanged)
            -> remote-security records (unchanged)
            -> direct-file signaling records (new, bounded)

browser RTCDataChannel <====== verified file frames ======> native RTC peer
```

The sideband must carry only:

- browser capability/version advertisement;
- open request with request ID, torrent ID, file index, offer, protocol limits,
  and fresh browser peer generation;
- answer with host peer generation, negotiated limits, and initial candidates;
- bounded trickle candidates in either direction and explicit end-of-candidates;
- connected/path-class status, typed rejection/failure, close, and cancel.

It must not carry range chunks, filesystem paths, media capability tokens,
arbitrary application calls, arbitrary HTTP fields, provider credentials, or
generic proxy data. Every message has a closed tagged shape, exact maximum
encoded size, circuit generation, peer generation, and request correlation.
Candidates with `typ relay`, unsupported transports, invalid mDNS names,
unbounded extensions, or conflicting ICE credentials/fingerprints fail closed.

The hosted browser must connect safely to both current protocol-1 hosts and
new direct-file-capable hosts during rollout. The implementation may introduce
a protocol-2 greeting/authorization outcome, but the browser is deployed
first with explicit protocol-1 fallback and never sends new fields to a host
that did not advertise them. There is no stable public wire promise beyond
this bounded rollout contract.

The Stage 0 survey retains protocol 1 unchanged while introducing closed
`RDF1` browser-to-host and `RDF2` host-to-browser records for the negotiated
capability. Each record carries request, circuit and peer generations; the
64-KiB encoded ceiling is independent from file frames. The browser will be
made protocol-2-aware before the host advertises `direct_file_v1`, so current
protocol-1 hosts remain controllable during rollout.

## Authority And Security Invariants

- The authenticated circuit, not STUN reachability or a DataChannel message,
  authorizes peer creation.
- The browser supplies only a torrent ID and file index already visible in its
  authorized application view. The host independently resolves exact
  completed-file eligibility and creates an internal opaque capability.
- The DTLS certificate is ephemeral per peer generation. The exact SDP
  fingerprint and ICE credentials are integrity-protected by the existing
  secure record channel before any file request is honored.
- STUN observes the public source address and request timing of each endpoint.
  It receives no username, passphrase, host identity, authorization ID,
  torrent/file identity, capability, SDP, DTLS key, or file content. This
  disclosure appears in Settings/privacy copy.
- Relay records remain E2E encrypted and bounded. The relay can observe timing
  and ciphertext sizes but no candidate address or file identity.
- Candidate addresses, SDP, STUN responses, capability tokens, DTLS keys, and
  file content are transient and excluded from persistent logs and audit.
- A successful DataChannel can read only the one internally bound logical file
  through existing verified range authority. Request IDs cannot change the
  capability or escape its length.
- Disabling direct transfer or revoking the circuit is synchronous at the
  authority boundary: no success response returns before the peer and file
  owners have been cancelled and joined.

## Owner, Task, And Cancellation Map

```text
ApplicationService/profile owner
  -> RemoteApplicationRuntime
       -> RemoteAccessOwner
            -> authenticated circuit owner
                 -> DirectFileSupervisor (lazy; max one peer for this circuit)
                      -> endpoint generation
                           -> ephemeral certificate + ICE/DTLS/SCTP state
                           -> bounded DNS/STUN work
                           -> UDP socket and optional mDNS socket
                           -> range request tasks
                                -> internal MediaCapabilityLease
                                -> verified logical range reads
```

The runtime-independent range codec remains inward of RTC, Tokio, sockets,
remote authentication, and platform code. The direct-file endpoint may depend
on session/application media authority; neither engine protocol nor durable
domain state depends outward on WebRTC or remote-host types. The browser sink
adapter depends on a small direct-file client interface, not on the remote
authentication implementation.

Cancellation sources are sink abort, range replacement, browser peer close,
direct-file close record, circuit loss, circuit replacement, sign-out,
authorization revocation, direct-transfer disable, remote-access disable,
profile replacement, application shutdown, negotiation timeout, request idle
timeout, peer idle timeout, and hard circuit lifetime. Every spawned task is
named by its owner, observes one of those tokens, and is joined before its
owner reports terminal cleanup.

## Initial Resource Bounds

Retain or tighten the proven Tactical 195 limits:

| Resource | Initial bound |
| --- | ---: |
| Direct peers per authenticated circuit | 1 |
| Direct peers per host process | 4 |
| Simultaneous range requests per peer | 4 |
| Simultaneous Save operations in first browser UI | 1 |
| Control frame | 4 KiB |
| Data frame | 64 KiB |
| File payload per chunk | 16 KiB |
| Application outbound queue per peer | 512 KiB |
| SCTP buffered data per peer | 512 KiB |
| Candidates per direction and peer generation | 32 |
| Aggregate accepted SDP/candidate text per direction | 64 KiB |
| Pending signaling commands per circuit | 16 |
| Negotiation deadline | 20 seconds |
| Chunk acknowledgement/request inactivity | 60 seconds |
| Connected peer with no active request | 60 seconds |
| Hard peer lifetime | circuit lifetime, at most 24 hours |
| Blob fallback file size | 32 MiB |
| Persistent audit additions per transfer | start plus one terminal event |

The payload row is intentionally tighter than Tactical 195's experiment.
RFC 8831 section 6.6 recommends messages no larger than 16 KiB when SCTP
message interleaving is unavailable, and the reviewed `rtc`/`rtc-sctp` 0.20.4
surface does not expose RFC 8260 I-DATA interleaving. The retained 64-KiB data
frame decoder ceiling remains a hostile-input bound, not a sender target.

Candidate gathering resolves at most eight STUN addresses per peer generation,
retains no more than two usable addresses per IP family, applies bounded DNS
and STUN retransmission deadlines from the reviewed RFC/library behavior, and
does not contact a second provider automatically. An oversized file is not a
reason to allocate proportional memory; streaming-save size is limited by
logical file authority and destination/runtime errors, with counters using
checked `u64`/`BigInt` representations.

## Product UX Contract

Remote Files gains **Save file...** only for one selected completed non-padding
file when `direct_file_v1` is compiled and enabled. Existing **Download now**
continues to mean torrent selection/priority and is neither renamed nor reused.
Local Tauri **Open** and **Play** continue on their incumbent media URL path.

The Save action:

1. detects a supported streaming destination and obtains it during the click;
2. otherwise admits the Blob fallback only after the exact file length proves
   it is at most 32 MiB;
3. shows negotiated bytes and a cancel action without claiming disk completion
   before the writable sink closes;
4. removes a partial fallback object and aborts/rolls back the browser writable
   where its API permits after failure or cancellation; and
5. says **Direct connection unavailable** rather than **Download failed** when
   ICE cannot produce a direct path.

Remote Access settings shows whether direct transfer is compiled, enabled,
idle, negotiating, connected, or unavailable; active circuit/client, bytes,
candidate class, and a Stop action are visible without exposing addresses.
The default-on setting explains that a public STUN service learns the device's
public address during an explicit attempt and that file content remains direct
and end-to-end encrypted.

## Staged Implementation And Gates

### Stage 0: Correct evidence and freeze product contracts

1. Amend Tactical 195 and the owning topic to record the post-completion
   Playwright WebKit result accurately: current reruns complete ICE, DTLS,
   DataChannel, and range traffic; the reproducible failure is OPFS in an
   ordinary non-persistent Playwright context, while a persistent context
   completes OPFS primitives.
2. Separate transport verdicts from sink verdicts in the browser harness and
   repeat WebKit negotiation enough times to classify any remaining ICE
   intermittency.
3. Inspect the exact specs, lower-rtc source/tests, current remote wire, and
   browser save behavior. Record edge cases and settle the version-1/2 rollout.

Gate: the new wire, authority, sink, candidate, cancellation, audit, and
compatibility contracts are deterministic and do not depend on OPFS.

### Stage 1: Default build and signaling state

1. Enable the leaf desktop/headless feature defaults and strengthen feature-off
   CI/package dependency assertions.
2. Add runtime-independent signaling values/codecs and adversarial tests.
3. Add browser/host capability negotiation and the encrypted direct-file
   sideband without weakening generic bulk/media rejection.

Gate: old hosts remain controllable, new peers negotiate only after capability
agreement, malformed/stale signaling fails locally, and relay payload counters
prove that a simulated file chunk cannot enter the control circuit.

### Stage 2: Product supervisor and direct candidates

1. Replace the unused product factory accessor with the circuit-owned
   supervisor and internal completed-file capability issuance.
2. Integrate bounded host/mDNS/server-reflexive candidate gathering using one
   exact public STUN endpoint, full ICE state, path classification, timeouts,
   and zero-owner teardown.
3. Bind circuit revocation, settings disable, and shutdown to joined endpoint
   cancellation and bounded audit.

Gate: deterministic host-only and scripted STUN cases plus controlled public
STUN gathering pass; unavailable STUN and no-direct-pair cases are terminal,
bounded, and leave remote control live.

### Stage 3: Browser sink and Files UX

1. Add the browser direct-file client, streaming sink abstraction, exact Save
   action, progress/cancel states, and 32-MiB fallback.
2. Preserve user activation by choosing the destination before negotiation.
3. Add Remote Access setting/live state and operator stop/audit presentation.

Gate: component and real-browser tests prove exact action naming, eligibility,
streaming writes, fallback cap, cancellation, failure copy, and continued UI
control.

### Stage 4: End-to-end and platform evidence

1. Exercise full password and resume paths through the production-shaped relay
   with exact verified save, revocation, route/circuit replacement, and cleanup.
2. Run current Chromium, Firefox, Playwright WebKit, native macOS, available
   Windows/Linux builds, controlled LAN, public-STUN, and available
   different-network direct cases.
3. Re-measure default release package/link and idle/active resources, update
   docs/readiness, and remove temporary profiles, captures, fixtures, and
   partial downloads.

Gate: every stopping condition passes or the execution record truthfully
leaves a specific bounded blocker without claiming the capability complete.

## Validation Matrix

### Deterministic

- signaling codec round trips and exact-key rejection;
- version/capability negotiation and protocol-1 fallback;
- peer/circuit/request generation fencing and wrap/overflow behavior;
- host-only, server-reflexive, peer-reflexive, mDNS, relay-candidate rejection,
  duplicate/end-of-candidates, STUN timeout/error, and no-pair transitions;
- completed/padding/incomplete/missing/revoked logical-file eligibility;
- exact range, ACK, cancel, slow/closed sink, queue/backpressure, and size caps;
- settings convergence, audit bounds/redaction, and joined cancellation.

### Scripted runtime

- lazy startup and first-request resource creation;
- DNS/STUN loss, delay, malformed/spoofed response, provider address rotation,
  blocked UDP, negotiation timeout, circuit loss, late candidates, stale
  chunks, repeated retry, sink failure, and shutdown;
- relay/application fairness while signaling is active and proof that payload
  frames are rejected from the relay sideband;
- high-water counters for tasks, sockets, peers, requests, signaling, queues,
  bytes, DNS/STUN work, and cleanup.

### Browser and web

- generated/static TypeScript contracts and validators as applicable;
- React Files/Settings component tests at desktop and phone widths;
- Chromium streaming-save adapter and injected sink failure/cancel;
- Firefox/WebKit transport independent of OPFS, fallback cap, and unavailable
  behavior;
- password login, private/shared choice, resume, reload, reconnect, revocation,
  host-version skew, and continued view/action use after direct failure.

### Interoperability and live opt-in

- exact Cloudflare STUN binding/gathering from browser and native host;
- controlled same-LAN and available independent-network srflx candidate path;
- current Chromium/Firefox/Playwright WebKit DataChannel exact-range trace;
- actual Safari and physical mobile browser when available, reported as
  evidence rather than a hard implementation prerequisite;
- zero TURN allocation and no relay candidate in captured ICE statistics.

### Repository and package

Run the proportional Rust/workspace and web baseline from `DEVELOPMENT.md`,
the exact remote-host/crypto/relay/direct-file suites, desktop/headless
feature-default and explicit feature-off clippy/tests, dependency-tree
assertions for excluded targets, desktop/headless packages on available native
platforms, actionlint, generated-contract checks when applicable, and
`git diff --check`. Record exact commands and all exceptions.

## Deliberate Non-Goals

- TURN URLs, credentials, allocations, hosting, accounts, payment, deployment,
  fallback, or support claims.
- Sending file content through the opaque relay, application WebSocket,
  remote-security control, HTTP gateway, or a new proxy service.
- OPFS as a product destination, hidden staging model, required browser
  capability, or support boundary.
- General Open, Play, seekable media, PDF/image viewer, service worker, native
  download URL, MSE/WebCodecs, transcoding, or container parsing.
- Active incomplete-file streaming, streaming-demand priority, piece waiting,
  or speculative reads before verification.
- Remote `.torrent` byte upload, directory browsing, arbitrary filesystem
  access, generic HTTP/range tunneling, or stable public wire compatibility.
- UPnP, NAT-PMP, PCP, firewall mutation, public HTTP listeners, wildcard TLS,
  public DNS/certificate automation, or interface pinhole management.
- Android/iOS endpoint hosting, background transfer, Compose/SwiftUI changes,
  extension transport, or remote control of a mobile host.
- Public website/relay deployment, package publication, tag, release, signed
  candidate, telemetry backend, or supported public capability claim.

## External Actions And Cleanup

The user explicitly authorizes bounded unauthenticated STUN binding/gathering
against `stun.cloudflare.com:3478` during implementation and validation. Each
probe starts only from an isolated test or authenticated file action, sends no
RSTorrent/user/file identifiers, uses no provider account or credential, and
ends with the owning peer generation. Public swarm traffic is not required.

Controlled LAN and available independent-network tests may open only transient
ephemeral UDP sockets owned by the test/product process. This tactical does not
authorize firewall changes, router mappings, public listeners, service
deployment, DNS changes, external messages, release publication, or edits to
private deployment repositories. Browser profiles, selected test downloads,
temporary payloads, logs, captures, and build probes stay isolated and are
removed after summarized non-sensitive evidence is recorded.

## Execution Record: 2026-08-30

The end-to-end product implementation landed in these commits:

- `1b5fcc2` defines capability-negotiated, circuit-bound direct-file
  signaling while retaining exact protocol-1 compatibility;
- `fe05d74` adds internal completed-file capability issuance;
- `33046f1` makes remote circuits own and cancel direct peer generations;
- `477b831` streams completed verified ranges over the retained WebRTC
  endpoint;
- `7c865f8` exposes direct saves, operator controls, live resources, and
  bounded security audit events;
- `d520c6c` retains usable host candidates when public STUN is unavailable;
- `3b4b472` completes the Files/Settings product path, browser sink,
  production-shaped verifier, mDNS routing, and cleanup fixes; and
- `6cae2e2` selects the incumbent Rustls provider explicitly in the unified
  product graph without changing RTC's Ring-owned DTLS path.

The implemented browser action is **Save file...** for one selected verified
completed file. The destination picker is requested synchronously from the
user gesture when available. The browser then negotiates one direct-only
DataChannel through authenticated encrypted sideband records and writes
sequential 16-KiB chunks with write-before-ack backpressure. Browsers without
the picker use the exact Blob fallback only up to 32 MiB. The product does not
use OPFS, does not expose file paths or bearer URLs, and does not send file
payload through the relay.

The host gathers bounded UDP host, strict UUIDv4 `.local` mDNS, and
Cloudflare server-reflexive candidates. STUN failure retains usable host
candidates. TCP, relay, multicast, unspecified, and arbitrary DNS candidates
fail closed. Remote disable, the direct-transfer kill switch, circuit loss,
revocation, cancellation, and shutdown close and join the peer, request,
capability, sockets, and mDNS driver. The Remote Access settings surface shows
the setting, stop action, bounded live resources, and redacted per-transfer
audit outcome.

Passing evidence:

- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`,
  and `cargo test --workspace --quiet`; the workspace includes 592 engine,
  41 desktop, 45 remote-host, 290 session, and 9 direct-file passing tests,
  with only their recorded ignored cases;
- `cargo test -p rstorrent-direct-file --features experiment`, the exact
  tracker runtime rerun, and direct-file feature clippy;
- `npm run typecheck --prefix clients/web` and
  `npm run test --prefix clients/web` with 364 passing and 2 skipped tests;
- the production-shaped remote verifier through real TLS and the opaque
  relay: password login, reload/browser/phone resume, relay restart, package
  rollback, revocation, tombstone, changed-host refusal, 256 invalid-circuit
  attempts, rendered audit, and an exact 1,048,576-byte direct save with
  SHA-256
  `52157c7a6432d236380ce45de056573cf27dd0e1dfcca37aac92d09530ea67a9`;
- zero relay payload retention and terminal zero direct peers, tasks, sockets,
  requests, and queued bytes in that product verifier;
- public Cloudflare STUN gathering from Chromium, Firefox, Playwright WebKit,
  and the native endpoint in isolated probes;
- earlier retained endpoint traces for exact 8-MiB Chromium and Firefox
  saves, cancellation, bounded 16-KiB messages, and zero-owner cleanup, plus
  WebKit DTLS/DataChannel/range success independent of OPFS;
- a native Ubuntu 24.04 ARM64 `cargo check -p rstorrent-headless` of the
  default RTC graph in an isolated VM; and
- native Windows 11 ARM64
  `cargo check -p rstorrent-direct-file --features webrtc` and
  `cargo check -p rstorrent-desktop` with the CI-equivalent web-dist
  prerequisite. The checks compile Ring, AWS-LC, and the complete retained RTC
  graph through the default desktop leaf using Rust 1.97 and the Visual Studio
  C++/Windows SDK plus LLVM toolchain.

Qualification remains open for two bounded reasons:

1. Browser and native public-STUN gathering pass separately and the complete
   product payload path passes on the current host, but no available
   independent-network topology selected a server-reflexive pair.
2. The product verifier deliberately exercises the bounded Blob fallback.
   Unit sink seams prove incremental write-before-ack behavior, but a real
   browser exposing `showSaveFilePicker` has not yet supplied the large-file
   streaming-save evidence. Actual Safari and physical mobile remain
   evidence, not inferred support.

These gaps do not reopen the implementation architecture. They keep Tactical
`196` active and the capability unadvertised until the exact stopping
condition is met. All temporary browser profiles, fixtures, downloads, VM
sources, archives, and testbed claims were removed or released. The standard
Windows native development toolchain remains installed in its dedicated
appliance.

## Escalation Contract

Within this tactical, implementation may refactor the remote runtime and web
file-action seam, add new bounded wire/codecs, change the remote protocol with
the stated rollout, add durable direct-transfer enablement/audit values, use
the selected credential-free public STUN endpoint, tighten limits, and fix
same-boundary defects without routine approval.

Stop for direction before:

- adding or contacting TURN, another byte relay, a paid/account-bound service,
  provider credential, or a second public STUN operator without a recorded
  primary-source privacy/availability reason;
- sending any file byte through the existing relay or changing its bandwidth
  role;
- adding UPnP/NAT-PMP/PCP/firewall mutation or a public file listener;
- making incomplete files, Open/Play/media, service workers, Android/iOS
  hosting, or arbitrary filesystem access part of this slice;
- deploying the website/relay, publishing a package/release, or making a
  supported compatibility claim; or
- accepting a new non-permissive dependency, persistent candidate/address
  telemetry, or architecture that lets direct work outlive revocation.

An ordinary browser difference, STUN timeout, NAT failure, internal module
split, conservative limit change, or unavailable optional testbed is not an
escalation. Record truthful applicability and continue through the bounded
matrix.

## Next-Slice Boundary

After this tactical, separate work may add native Open/Play and seekable media
adapters, broaden cross-browser large-file saving, measure broader NAT success,
or reconsider UPnP/public IPv6 policy. TURN is not the default next slice and
is not planned merely because a network fails direct ICE. A direct-unavailable
result is an accepted product outcome.
