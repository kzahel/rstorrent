# Tactical 095: Bounded HTTP And HTTPS Tracker Transport

Status: Planned on 2026-08-05. This is a decision-complete candidate tactical.
Creating and committing this planning record is authorized; implementation has
not started. It becomes executable only after Tactical
[`092`](092-truthful-tracker-and-dht-peer-advertisement.md) is complete and the
living capability queue explicitly selects it. It may then be prioritized
ahead of planned Tacticals `093` and `094`, which are not wire dependencies.

Topics: `tracker-discovery`, `protocol-support`, `capability-readiness`,
`incoming-reachability-and-seeding`, `application-control`,
`code-organization-and-refactoring`

Dependencies: completed Tacticals
[`081`](081-v1-torrent-byte-intake.md),
[`086`](086-long-lived-torrent-peer-runtime.md), and
[`089`](089-coordinated-session-listen-sockets.md) provide the retained
multi-transport tracker catalog, long-lived torrent peer owner, and session
socket facts. Tactical `092` is the hard prerequisite: it must first establish
one session-long discovery-advertisement service, truthful counters and
explicit outbound-only semantics, completed/stopped lifecycle, stale-result
fencing, and one session-wide eight-tracker-operation ceiling. HTTP must extend
that owner rather than recreate the superseded active-download tracker manager.

## Decision And Motivation

Implement bounded HTTP and HTTPS announce transport through the retained
tracker catalog and the common tracker schedule. The slice includes HTTP/1.1
request and response handling, redirects, binary query encoding, gzip,
bencoded tracker failures and successes, compact and noncompact peers, IPv4
and IPv6 tracker connectivity, `peers6`, tracker IDs, bounded Basic
authentication, and mixed UDP/HTTP/HTTPS tier scheduling.

HTTP and HTTPS are tracker transports whose network exchange uses TCP. There
is no separate BitTorrent "TCP tracker" protocol. The shared abstraction is
the announce lifecycle and result, not the wire protocol.

RSTorrent currently parses, persists, redacts, and displays HTTP/HTTPS
metainfo rows as unsupported while feeding only UDP rows into runtime.
Magnet intake is narrower still: it retains only valid UDP `tr` parameters and
silently omits valid HTTP/HTTPS trackers. The pure schedule, operation action,
and new Tactical `092` owner also carry UDP-specific URL and response types.
Adding an isolated HTTP manager would duplicate tier policy, counters,
shutdown, operation budgets, snapshots, and peer intake and would restore the
lifetime split that Tactical `092` removes.

The first HTTPS slice deliberately disables server-certificate and hostname
validation for the tracker-only reqwest clients. This is an explicit
maintainer decision intended to avoid Android platform-verifier integration in
the initial transport slice. TLS still encrypts bytes on the wire, but without
server authentication it does not establish that RSTorrent is talking to the
configured tracker. An active intermediary can read private tracker
credentials, change announce responses, redirect traffic within the permitted
policy, and inject attacker-selected peer endpoints. The product and protocol
matrix must therefore call this state **encrypted but unauthenticated**, never
secure, authenticated, or verified HTTPS.

This temporary TLS policy is scoped to tracker clients. It must not change the
gateway, UPnP, web, update, or any other HTTP/TLS owner. It is not a user-facing
global "accept invalid certificates" switch. Authenticated platform trust is
the immediate next-slice boundary and remains required before an HTTPS tracker
security or full-support claim.

## Stopping Condition

This tactical is complete when all of the following hold:

1. retained metainfo and magnet tracker rows for UDP, HTTP, and HTTPS feed one
   transport-neutral pure tracker schedule without losing exact tier,
   position, source, passkey-bearing path/query, or redacted presentation;
2. mixed UDP/HTTP/HTTPS rows share Tactical `092`'s attempts, fallback,
   promotion, retries, reannounce, counters, started/update/completed/stopped
   lifecycle, stale-result fencing, and exact session-wide eight-operation
   ceiling;
3. one session-owned reqwest client set performs bounded HTTP/1.1 and HTTPS
   announces with policy-filtered DNS, address-family selection, redirects,
   Basic authentication, credential redaction, cancellation, and connection
   reuse, while HTTPS explicitly disables certificate and hostname validation;
4. request construction preserves an existing tracker query and percent-
   encodes all 20 info-hash and peer-ID bytes exactly, sends truthful counters
   and the family-correct advertised or outbound-only port, and carries the
   returned bounded tracker ID on later announces;
5. bounded response handling accepts `gzip` and `x-gzip`, chunked or length-
   framed bodies, tracker failure/warning fields, optional swarm counts and
   intervals, compact IPv4 `peers`, compact IPv6 `peers6`, and BEP 3
   noncompact peers without letting encoded body, decompression, bencode, DNS,
   entry, or diagnostic work escape its declared limits;
6. an HTTP response containing only `peers6` is a success and its valid IPv6
   endpoints pass through the ordinary `PeerObservation` and `PeerEndpoint`
   owners into outbound IPv6 dialing;
7. a controlled AAAA-only HTTP tracker and a controlled HTTPS tracker with an
   untrusted or hostname-mismatched certificate each provide peers to a
   hash-verified transfer, while the latter is observably classified as
   encrypted and unauthenticated;
8. a tracker reached over IPv6 receives port `1` while RSTorrent has no
   corresponding IPv6 listener, even when a truthful IPv4 listener exists;
   the slice makes no full BEP 7 multi-address publication claim;
9. pause, removal, replacement, stopped timeout, and session shutdown cancel
   and join every request, resolver, body, and peer-hostname operation with no
   late schedule mutation or credential-bearing error; and
10. deterministic hostile-input, scripted HTTP/HTTPS, controlled
    RSTorrent/libtorrent, desktop, Android cross-build/AVD, and exact resource
    high-water evidence pass at the support levels claimed below.

## Scope

### Transport-neutral tracker catalog and schedule

- Replace the runtime-only `UdpTrackerConfig`/`UdpTrackerUrl` schedule boundary
  with a bounded `TrackerConfig` carrying a parsed `TrackerEndpoint` enum plus
  the exact retained display/wire URL, tier, position, and source. The initial
  endpoint variants are UDP and HTTP(S); HTTP and HTTPS may share one parsed
  HTTP target whose scheme remains explicit.
- Preserve the exact retained URL for persistence and request semantics. A
  parsed operational URL may normalize syntax needed by the HTTP stack, but it
  must not silently rewrite, drop, decode, or re-encode a private tracker
  passkey in userinfo, path, or an existing query.
- Generalize magnet `tr` intake from `udp_trackers` to one bounded tracker
  catalog. Valid UDP, HTTP, and HTTPS values form the existing single
  synthetic magnet tier, retain input order, deduplicate by the established
  normalized tracker identity, and share the existing limit of 32 valid
  magnet trackers. Rename UDP-specific constants and errors without changing
  the outer magnet byte or parameter ceilings.
- Keep metainfo's retained BEP 12 tiers unchanged. An unsupported or
  operationally invalid row remains inspectable and does not invalidate the
  torrent or collapse its surrounding tier.
- Make `TrackerAction::Announce` transport-neutral. The schedule owns tracker
  selection and lifecycle but no socket, reqwest client, UDP token, redirect
  history, or response body.
- Introduce one common accepted outcome containing the accepted interval,
  optional minimum interval and swarm counts, bounded peers, warning context,
  and optional tracker ID. UDP responses convert into this outcome. HTTP-only
  optional fields must not force invented zero seed/leech counts into views.
- Keep transport continuation state keyed by stable tracker ID outside the
  pure schedule: UDP connection-token caches remain UDP-only, while a bounded
  HTTP tracker ID is retained for the current torrent generation and sent on
  subsequent requests. Neither is persisted across restart.
- Use explicit enum dispatch in the Tactical `092` owner. Do not create a
  general tracker plugin framework or trait hierarchy merely to make two
  functions look alike. A narrow test seam is permitted if scripted transport
  injection demonstrates a concrete need.

### HTTP and HTTPS dependency policy

- Use the existing workspace `reqwest` dependency for status lines, headers,
  HTTP/1.1 framing, chunked transfer, DNS integration, redirects, connection
  pooling, TLS, and cancellation. Do not hand-roll an HTTP parser or add a
  separate typed-header library; `reqwest::header` is sufficient.
- Retain `default-features = false` and enable only the focused `rustls` and
  `stream` features initially. Do not enable reqwest's automatic `gzip`
  feature because the operation must count compressed bytes before decoding.
  Do not enable default TLS, system proxy, HTTP/2, charset, cookies, JSON,
  multipart, brotli, deflate, zstd, SOCKS, or HTTP/3.
- Add `async-compression` `0.4` with only its Tokio and gzip features for one
  cancellation-safe streaming decoder after the encoded-byte counter. This
  focused dependency is authorized by the tactical, must retain its upstream
  Apache-2.0/MIT-compatible license record, and may not become a general
  compression framework.
- Build the tracker clients with HTTP/1 only, no proxy, bounded connect and
  total timeouts, a bounded redirect policy, explicit `Accept-Encoding: gzip`,
  and strict default HTTP response parsing. Do not enable reqwest's invalid-
  header, obsolete-multiline-header, or spaces-after-header-name compatibility
  modes without a later real-world fixture and a separate security decision.
- Set both reqwest rustls danger flags needed to accept invalid certificates
  and hostnames. Keep SNI enabled and otherwise use rustls's supported TLS
  versions and cipher configuration. A TLS handshake/protocol failure remains
  an operation failure; this policy bypasses identity validation, not the TLS
  protocol itself.
- Reqwest `0.13.4` selects its `NoVerifier` path before constructing
  `rustls-platform-verifier` when certificate validation is disabled. Prove
  that behavior in the Android controlled gate rather than assuming that
  enabling the Rust feature alone establishes a working Android TLS runtime.
- Recognize exactly one absent/identity, `gzip`, or obsolete `x-gzip` content
  coding after reqwest has decoded HTTP framing but before body bencode. Feed
  gzip and x-gzip through the same bounded `async-compression` decoder. Reject
  stacked, unknown, corrupt, or truncated encodings; do not enable unrelated
  compression formats to solve them.

### Request construction

- Parse and validate the base URL before allocating an operation. Accept only
  `http` and `https`, require a host, reject fragments and control characters,
  and enforce the existing 2-KiB tracker-URL ceiling for an operational row.
  The final request target, including announce parameters, is capped at 4 KiB.
- Preserve any existing query and append with the correct separator. Empty,
  trailing-`?`, trailing-`&`, userinfo, explicit port, IPv6-literal authority,
  path passkey, and query passkey cases receive pure tests.
- Percent-encode `info_hash` and `peer_id` byte-for-byte rather than passing
  them through a UTF-8 or generic form serializer. Every byte may be encoded
  as uppercase `%XX`; no `+`/space or Unicode transformation is permitted.
- Send `port`, `uploaded`, `downloaded`, `left`, `compact=1`, `no_peer_id=1`,
  the stable per-torrent 32-bit tracker key, and `numwant=200`. Send
  `numwant=0` for stopped. Send `event=started`, `completed`, or `stopped` only
  for those events; omit `event` for an ordinary update.
- If a response supplies a bounded tracker ID, percent-encode its raw bytes in
  a later `trackerid` query parameter. Replacing a tracker ID replaces only
  that row's volatile value.
- Send a bounded RSTorrent user agent and `Accept-Encoding: gzip`. Do not send
  cookies, referrers, ambient authorization, proxy credentials, client
  certificates, or caller-provided arbitrary headers.
- Continue supporting private-tracker passkeys already present in the path or
  query. For URL userinfo, remove credentials from the request URL and set one
  Basic `Authorization` header explicitly. Enforce the URL limit before
  decoding, bound decoded username and password to 256 bytes each, and reject
  control characters.
- Never include a full request URL in diagnostics. Strip reqwest error URLs
  with `without_url()` and use the established authority-only tracker label.
  Failure reasons, redirect errors, and TLS errors must not echo query, path,
  userinfo, `info_hash`, peer ID, or Authorization content.

### DNS, address family, redirects, and network policy

- Check `Offline` before DNS. Use one policy-aware resolver boundary for the
  initial origin, every redirect origin, and noncompact peer hostnames. Filter
  every returned `SocketAddr` through `NetworkPolicy` before connection or
  observation and retain at most 16 resolved addresses per hostname/family.
- Validate literal-IP origins and redirects directly because a DNS resolver is
  not necessarily invoked for them. Loopback-only runs may contact only
  loopback addresses. Online retains the project's existing valid-address
  semantics rather than inventing a separate tracker SSRF policy in this
  slice.
- Select an address family before building the announce query. Use
  family-filtered resolution/source binding for that physical request so a
  redirect cannot silently cross families after the advertised port is
  chosen. Try alternate allowed addresses and then an alternate available
  family sequentially within the same logical operation and total deadline;
  never fan one row into uncounted concurrent sockets.
- Prefer a family with a truthful advertisable listener. Under the current
  IPv4-only incoming owner, an IPv4 request receives Tactical `092`'s selected
  IPv4 port and an IPv6 request receives the outbound-only sentinel port `1`.
  An AAAA-only tracker must therefore remain usable for discovery without
  falsely advertising the IPv4 listener on an IPv6 source address.
- Follow at most five redirects. Accept only HTTP-to-HTTP,
  HTTP-to-HTTPS, or HTTPS-to-HTTPS. Reject HTTPS-to-HTTP downgrade,
  unsupported schemes, a policy-ineligible literal, excess hops, loops, and a
  redirect that cannot remain in the operation's selected address family.
- Let reqwest remove Authorization on a cross-origin redirect and verify this
  with a scripted two-origin server. Do not copy the original path/query
  passkey to a new origin; only parameters explicitly present in `Location`
  survive URL resolution. Same-origin redirect behavior follows URL semantics
  and the bounded hop/deadline rules.
- Count each concurrently live logical request against Tactical `092`'s
  session-wide eight-operation ceiling. Sequential address/family and redirect
  attempts do not increase simultaneous operation count, but their attempt and
  failure facts remain observable.

### HTTP response and compression policy

- Require a complete HTTP response with status `200`. A redirect is handled
  only by the bounded redirect policy. Other status codes fail the operation
  using status and bounded reason context; do not parse an arbitrary HTML or
  text error body as a tracker response or retain it in diagnostics.
- Ignore absent or incorrect `Content-Type` on a `200` response and parse the
  bytes as bencode. Old trackers commonly return `text/plain` or omit the
  field.
- Count the body stream exposed after HTTP transfer framing and before content
  decoding. Apply a 1-MiB encoded-body limit and stop at limit plus one byte.
  Reject a known encoded `Content-Length` above that limit early, but retain
  the streaming counter because the header is untrusted and chunked bodies
  omit it.
- Apply a separate 1-MiB decompressed-body limit and read at most limit plus
  one byte before failing. Identity bodies pass through both counters without
  a second retained copy where practical. Gzip decoding is streamed under the
  same aggregate deadline and operation cancellation rather than placed in an
  unjoined blocking task.
- Accept no stacked or unrequested content encoding. Standard gzip and the
  explicit `x-gzip` compatibility behavior above are the only compressed
  forms in scope. Corrupt, truncated, concatenated-bomb, or over-limit gzip is
  a bounded tracker failure.
- Parse the whole decompressed body with
  `parse_with_limits_permissive_dictionaries`: out-of-order dictionary keys
  and unknown fields are accepted, duplicate keys and trailing bytes are
  rejected, and metainfo's canonical dictionary policy is unchanged.
- Use tracker-specific limits: depth 8, at most 4,096 lexical tokens, at most
  512 entries in any collection, and the 1-MiB byte/string ceiling. Tightening
  these values from deterministic/reference evidence without excluding the
  declared 200-peer noncompact case is authorized.

### Bencoded tracker outcome

- Require a root dictionary. A bounded `failure reason` byte string makes the
  operation a tracker-declared failure and does not require interval or peers.
  Retain at most 256 lossy-UTF-8 diagnostic bytes.
- Support BEP 31 `retry in` only alongside `failure reason`. Accept the
  normative positive decimal minute string or `never`; accepting a positive
  bencoded integer as a bounded compatibility form is allowed, but it is still
  minutes rather than JSTorrent's current seconds interpretation. Clamp a
  numeric delay to 24 hours. `never` disables that row for the current torrent
  generation while allowing other rows/tiers to operate; restart makes the
  volatile row eligible again.
- Retain at most 256 bytes of `warning message` as diagnostic context without
  turning an otherwise valid response into failure.
- If `interval` is absent, use the pinned libtorrent compatibility default of
  1,800 seconds. Treat `min interval` as a lower bound by selecting the greater
  value, then apply the existing schedule clamp of five minutes through 24
  hours. Reject negative/non-integer values; oversized positive values
  saturate at the schedule maximum without arithmetic overflow.
- Treat `complete` and `incomplete` as optional non-negative values. Range-
  checked values project to optional `u32`; absent or invalid values remain
  unknown instead of becoming zero.
- Retain `tracker id` as bounded raw bytes and return it to the request owner.
  A value over 256 bytes is ignored and diagnosed rather than becoming an
  allocation or request-URL authority.
- Missing `peers` is allowed. A zero-peer response and a response containing
  only valid `peers6` are successful responses and schedule a reannounce.
- Cap accepted peer entries at 200 combined across `peers` and `peers6` before
  registry insertion. Preserve response order for deterministic intake and
  deduplicate exact `SocketAddr` values within the response before observation.

### Compact and noncompact peers

- Accept BEP 23 compact IPv4 `peers` as six-byte address/port entries and BEP 7
  compact IPv6 `peers6` as eighteen-byte entries. A correct empty byte string
  is an empty peer list.
- For old-tracker compatibility, accept every complete compact stride and
  discard one short trailing suffix with a bounded diagnostic. A nonempty
  compact field containing no complete entry is malformed and fails the
  response. This tolerance never shifts alignment or manufactures a partial
  endpoint.
- Continue accepting BEP 3 noncompact `peers` even though the request asks for
  `compact=1`; BEP 23 makes the preference advisory. Each list item must be a
  dictionary with a bounded byte-string `ip` and integer port. `peer id` is
  optional and ignored because endpoint observations do not establish peer
  identity.
- Accept numeric IPv4 and IPv6 strings immediately. Retain at most 16 valid
  hostname entries, each no longer than the existing host limit, for runtime
  resolution with at most four hostname lookups in flight and the announce's
  existing total deadline. More hostname entries are truncated and diagnosed,
  not allocated or resolved.
- Skip malformed individual noncompact entries. If a nonempty list contains no
  structurally valid entry, fail it as a malformed tracker response. A valid
  entry whose address later fails DNS or network policy does not invalidate
  otherwise valid entries or the tracker interval.
- Pass all numeric and resolved endpoints through `PeerEndpoint` and current
  network policy. Drop port zero, unspecified, multicast, broadcast, and
  unscoped link-local endpoints. Tracker output remains an untrusted discovery
  hint, never proof of reachability, peer identity, seeding state, or content
  integrity.

### IPv6 support level

- Support an IPv6-literal HTTP/HTTPS tracker authority, an AAAA-only tracker
  hostname, and IPv6 addresses reached after a redirect under the same family
  and policy rules.
- Accept compact `peers6` whether the tracker request itself used IPv4 or IPv6.
  Feed those endpoints through the existing address-neutral peer registry and
  TCP dial path and prove an outbound IPv6 content transfer.
- Do not send the discouraged `ipv4` or `ipv6` query parameters from an
  unrelated source address. BEP 7 identifies them as a reflection/DDoS risk.
- Do not claim full BEP 7 announcing. The current incoming listener,
  advertised-endpoint selector, UPnP mapping, and product reachability state
  are IPv4-only. This slice makes at most one successful family request per
  logical announce and does not enumerate every local address, publish an
  IPv6 incoming port, create an IPv6 pinhole, or retain per-interface tracker
  records.
- Full BEP 7 requires a later tactical for dual-stack/multi-interface listen
  sockets, per-family advertised endpoints, source-address selection,
  interface/rebinding lifecycle, and platform reachability evidence. Until
  then IPv6 tracker use is discovery/outbound-transfer support and HTTPS over
  IPv6 is still unauthenticated under the initial TLS policy.

### Truthful presentation and observability

- Preserve `Http` and `Https` as URL/transport facts. Add a tracker security
  projection that distinguishes plaintext HTTP from encrypted-but-
  unauthenticated HTTPS. Do not overload success, tracker status, listener
  reachability, or the URL scheme to imply certificate verification.
- Existing configured HTTP/HTTPS rows transition from `unsupported` into the
  ordinary inactive/announcing/waiting/success/failure lifecycle only when the
  runtime has admitted their operational URL. Invalid rows remain configured
  and truthfully unsupported/unusable with bounded redacted context.
- Emit typed attempt, redirect, address-family, response, compatibility-drop,
  gzip, failure, warning, and peer-intake facts through existing diagnostics.
  Logs remain non-authoritative and never carry full URLs, credentials, or raw
  bodies.
- Track HTTP operations, active sockets/requests, redirects, resolved
  addresses, exact encoded/decompressed bytes, bencode tokens,
  response peers, hostname resolutions, dropped entries, queue high-water,
  cancellation, and terminal counts. A reqwest connection pool may retain
  idle connections only within its explicit session owner and bounded idle
  policy.

## Owner, Task, Cancellation, And Data-Flow Map

```text
retained tracker catalog + counters + advertised endpoint
                         |
                pure TrackerSchedule
                         |
             transport-neutral Announce action
                         |
              +----------+-----------+
              |                      |
       UDP announce operation   HTTP(S) announce operation
       token cache/datagrams    reqwest/DNS/TCP/TLS/body
              |                      |
              +----------+-----------+
                         |
              common accepted outcome
                         |
       schedule transition + tracker snapshot/diagnostics
                         |
             ordinary PeerObservation(Tracker)
                         |
          peer registry -> existing bounded dial owner
```

- Runtime-independent tracker endpoint values, request query encoding,
  bencoded response parsing, compact peer decoding, schedule transitions, and
  accepted-outcome normalization contain no Tokio, DNS, reqwest, socket,
  random, or wall-clock type.
- The session `DiscoveryAdvertisementService` created by Tactical `092`
  remains the sole tracker scheduler and operation-budget owner. It holds the
  session HTTP client set and per-row transport continuation state and starts
  at most eight tracker operations across all torrents and transports.
- One HTTP operation owns its address selection, redirect chain, request,
  response stream, decompression, bounded hostname resolution, and result. It
  does not spawn an unjoined retry loop; the common schedule owns later retry.
- Reqwest clients are constructed once per session/family policy so successful
  trackers may reuse connections. They are dropped only after all torrent
  operations have been cancelled/joined during session shutdown.
- Cancellation drops the in-flight request/body future. Any auxiliary
  resolver or compatibility decoder must be cancellation-safe or retain a
  handle that the operation joins before returning. Stale generation,
  schedule-epoch, and advertised-endpoint-generation results are discarded by
  Tactical `092`'s existing fences.
- The peer registry remains the only accumulated peer-record and source-merger
  owner. HTTP does not maintain its own known-peer set or dial directly.

## Resource And Security Invariants

| Resource | Initial bound |
| --- | --- |
| Concurrent tracker operations | 8 session-wide across UDP/HTTP/HTTPS |
| Concurrent requests within one logical operation | 1 |
| Redirects | 5 |
| Operational base URL | 2 KiB |
| Final announce target | 4 KiB |
| Resolved tracker addresses | 16 per hostname/family |
| Tracker connect timeout | 10 seconds |
| Ordinary aggregate HTTP operation | 30 seconds |
| Stopped aggregate HTTP operation | Tactical `092`'s 5-second shutdown bound |
| Encoded response body | 1 MiB |
| Decompressed response body/string | 1 MiB |
| Bencode depth | 8 |
| Bencode lexical tokens | 4,096 |
| Entries in one decoded collection | 512 |
| Accepted response peers | 200 combined IPv4/IPv6 |
| Retained noncompact peer hostnames | 16 |
| Concurrent peer-hostname resolutions | 4 per operation |
| Failure/warning/tracker-ID value | 256 bytes each |
| Decoded Basic username/password | 256 bytes each |
| Tracker diagnostic/error detail | existing 256-byte ceiling |
| TLS certificate/hostname validation | deliberately disabled, tracker clients only |

- No response field, redirect, header, URL, hostname, certificate, compressed
  stream, or tracker count may choose an allocation, task count, retry loop,
  request count, or log size outside these bounds.
- A successful TLS handshake is not authenticated identity evidence in this
  slice. The security projection and capability claim must remain explicit
  even when a certificate would have validated under a later policy.
- HTTP plaintext and unauthenticated HTTPS may expose private tracker
  credentials to the network or an active intermediary. Controlled fixtures
  use synthetic credentials. Public live evidence uses only public trackers
  with no account/passkey secret.
- Redirect policy, DNS policy, and peer endpoint policy are applied
  independently. Accepting a response from a permitted tracker does not make
  its returned peer endpoints policy-eligible.
- Status success, valid bencode, peer receipt, and eventual hash verification
  are distinct facts. No layer presents unverified tracker data as verified
  peer or content state.

## Failure, Retry, And Shutdown Semantics

- URL/build, DNS, policy, connect, TLS, redirect, timeout, status, body,
  decompression, bencode, declared failure, and peer-shape failures are typed
  operation outcomes mapped into the common tracker failure transition with
  bounded redacted context.
- HTTP transport failure uses the existing quadratic tracker retry and tier
  fallback unless a valid BEP 31 failure supplies a bounded later retry or
  disables only that row for the current generation.
- A structurally valid `200` response with zero peers, only unusable endpoints,
  or only a warning is a tracker success. It accepts the bounded interval and
  does not spin through fallback merely because no dialable peer arrived.
- A redirect, alternate IP, or alternate family remains part of one logical
  operation. It may emit attempt facts but does not independently mutate the
  schedule.
- Started remains pending until one successful response for that row.
  Completed and stopped follow the shared Tactical `092` ordering and endpoint
  correction rules. A best-effort stopped request does not outlive the
  five-second owner shutdown deadline or delay listener/mapping teardown past
  that contract.
- A dropped/cancelled request produces no late peer observations, tracker ID,
  interval, warning, or success. Session terminal counts are emitted only
  after active requests and owned auxiliaries have ended.

## Normative And Reference Dossier

Pinned BEP revision `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`
was inspected:

- `reference/bittorrent.org/beps/bep_0003.rst` defines HTTP announce query
  fields, tracker events, failure responses, interval, and noncompact peers;
- `reference/bittorrent.org/beps/bep_0007.rst` requires source-family-aware
  announces, discourages cross-family IP query parameters, permits expanded
  IPv6 peers, and defines eighteen-byte compact `peers6`;
- `reference/bittorrent.org/beps/bep_0012.rst` defines tracker tier retention
  and ordered fallback semantics;
- `reference/bittorrent.org/beps/bep_0023.rst` defines six-byte compact IPv4
  peers, makes `compact=1` advisory, and requires clients to continue accepting
  noncompact peer lists; and
- `reference/bittorrent.org/beps/bep_0031.rst` defines positive-minute or
  `never` retry advice attached to a tracker failure.

Pinned libtorrent revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected:

- `reference/libtorrent/src/http_tracker_connection.cpp`, especially
  `http_tracker_connection` and `parse_tracker_response`, manually constructs
  binary query fields, carries tracker ID, selects a bound source, defaults a
  missing interval to 1,800 seconds, accepts optional swarm counts, skips bad
  noncompact entries, parses compact IPv4 and IPv6 lists, and accepts missing
  peer fields;
- `reference/libtorrent/src/http_connection.cpp` owns HTTP/TLS exchange, five
  redirects, gzip/`x-gzip`, bounded receive/decompression, Basic
  authentication, alternate endpoints, and cross-origin Authorization
  stripping;
- `reference/libtorrent/src/tracker_manager.cpp` dispatches HTTP(S) and UDP
  behind shared manager policy and a bounded HTTP announce count without
  imposing one common wire-transport class on their mechanics;
- `reference/libtorrent/test/test_tracker.cpp` covers hostname peers, compact
  IPv4, intervals, warnings, failures, and peer extraction, while explicitly
  recording missing `peers6`, tracker-ID, and uneven-stride coverage that this
  tactical must author independently; and
- `reference/libtorrent/test/test_http_connection.cpp` covers HTTP and HTTPS,
  finite and infinite redirects, Basic authentication, same-origin credential
  retention, cross-origin credential removal, and proxy variants. RSTorrent
  adopts the relevant direct-connection cases and deliberately defers proxying.

Pinned rqbit revision `4e5f94cbcf1d57ec500885c77cf1e24d70232d89`
was inspected:

- `reference/rqbit/crates/tracker_comms/src/tracker_comms_http.rs` uses binary
  query escaping, parses compact/noncompact peers and `peers6`, and includes
  basic request/response tests; and
- `reference/rqbit/crates/tracker_comms/src/tracker_comms.rs` dispatches UDP
  and HTTP monitor paths, reuses reqwest, preserves an existing tracker query,
  retries transport errors, and feeds returned `SocketAddr` values to one peer
  channel.

Rqbit is useful Rust evidence but not the completeness oracle: its current
HTTP path buffers the whole body without this tactical's explicit cap, requires
an interval, accepts only numeric noncompact IPs, does not carry returned
tracker IDs, and has no equivalent redirect, hostile-body, cancellation, or
IPv6-only vertical evidence.

The first-party JSTorrent checkout was inspected:

- `packages/engine/src/tracker/http-tracker.ts` supplies useful binary query,
  30-second deadline, failure/warning vocabulary, compact/noncompact peer, and
  `peers6` behavior;
- `packages/engine/src/utils/minimal-http-client.ts` has a 1-MiB response cap,
  optional socket TLS upgrade, and connection metadata; and
- `packages/engine/src/tracker/tracker-manager.ts` dispatches HTTP and UDP
  trackers and provides practical product vocabulary.

JSTorrent also records failure modes this tactical must not inherit: its HTTP
URL construction always inserts `?`, its minimal client rejects chunked
transfer and requests identity encoding, its manager flattens tiers and uses
separate transport queues, its response ownership is not the long-lived
session owner, and its BEP 31 interpretation treats a numeric value as seconds
rather than normative minutes. Its source and fixtures are not copied.

Workspace reqwest `0.13.4` source and official API documentation were
inspected. `ClientBuilder` provides HTTP/1 restriction, redirect policy,
timeouts, no-proxy behavior, gzip, local-address and resolver hooks, invalid-
certificate/hostname controls, and URL removal from errors. The rustls builder
selects `NoVerifier` before platform verifier construction when certificate
validation is disabled. This implementation fact must be re-audited if the
reqwest version changes.

The official `async-compression` `0.4` documentation and source were inspected
for its Tokio `GzipDecoder`, streaming behavior, focused feature selection,
and dual MIT/Apache-2.0 license. Re-audit the exact Cargo-resolved release and
license before landing the dependency.

No reference source, response, certificate, or fixture is imported. RSTorrent
independently implements the public protocol behavior and authors its own
bounded deterministic and controlled evidence.

## Edge-Case Checklist

The common path and these shape-changing edges land together:

- base URLs with no query, an existing query/passkey, empty query, trailing
  separators, explicit/default ports, Basic userinfo, and IPv6 literals;
- arbitrary binary zeros, `%`, `&`, `+`, `/`, high bytes, and all-byte vectors
  in info hash, peer ID, and tracker ID percent encoding;
- mixed schemes in one tier and across tiers; success promotion, fallback,
  zero-peer success, retry, reannounce, completed, stopped, removal, and stale
  endpoint/result generations;
- A, AAAA, dual-stack, IPv6 literal, filtered, duplicate, excess, empty, slow,
  and failing resolution results; family failover never changes the already
  selected port without rebuilding the request;
- relative, absolute, same-origin, cross-origin, looping, excess-hop,
  cross-family, unsupported-scheme, and downgrade redirects;
- valid, self-signed, expired, and hostname-mismatched certificates all
  accepted under the initial unauthenticated policy; malformed TLS handshakes
  and protocol failures remain failures;
- length-framed, connection-close, chunked, gzip, `x-gzip`, corrupt
  gzip, gzip bomb, empty, slow, oversized, wrong-content-type, and non-200
  responses;
- out-of-order and unknown bencode keys, duplicates, trailing bytes, excessive
  depth/tokens/collections, missing interval, min interval, negative/huge
  integers, failure, warning, tracker ID, BEP 31 delay/never, and missing peers;
- empty, exact, over-limit, mixed, duplicate, zero-port, invalid-address, short-
  suffix, and only-`peers6` compact fields;
- empty, partially malformed, all-malformed, numeric IPv4/IPv6, hostname,
  excess-hostname, failed-resolution, and policy-rejected noncompact lists;
- URLs and Basic credentials absent from build, request, redirect, TLS,
  timeout, body, bencode, declared-failure, diagnostics, snapshots, and terminal
  errors; and
- cancellation during DNS, connect, TLS, redirect, headers, body,
  decompression, hostname resolution, and stopped delivery.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure URL/request | Exact base-query preservation, binary percent encoding, event/counter/port/key/numwant/tracker-ID fields, Basic extraction, URL/final-target bounds, and redacted display/error cases. |
| Pure response | All fields and limits above; compact IPv4/IPv6, only-`peers6`, noncompact numeric/hostname entries, tolerant partial lists, malformed structures, BEP 31, optional counts/intervals, and response deduplication. |
| Pure schedule | Mixed UDP/HTTP/HTTPS tiers share fallback, promotion, retry, reannounce, lifecycle, tracker-ID continuation, optional swarm values, BEP 31 row disable, stale-result fencing, and inactive terminal state. |
| Scripted HTTP | HTTP/1 status/framing, chunked and wrong content type, gzip/over-limit/corrupt body, redirects and loops, existing passkeys, same/cross-origin Basic behavior, strict malformed-header rejection, DNS/address/family policy, slow stages, cancellation, and exact high-water counts. |
| Scripted HTTPS | Valid, self-signed, expired, and wrong-host certificates prove the declared unauthenticated policy; malformed TLS fails; downgrade is rejected; diagnostics and views say encrypted/unauthenticated and contain no credentials. |
| IPv6 | IPv6 literal and AAAA-only tracker operations use an IPv6 source/family and port `1`, accept only-`peers6`, feed ordinary observations, and complete a hash-verified outbound IPv6 peer transfer without an IPv6 incoming-support claim. |
| Controlled interoperability | A controlled HTTP tracker introduces RSTorrent to a pinned libtorrent seed for exact verified content; HTTPS repeats through an untrusted controlled certificate. Mixed-tier fallback and stopped observation are retained. No external tracker credential is used. |
| Session/lifecycle | Discovery survives metadata-to-content-to-seed transition, counters and endpoint corrections are current, eight operations remain the session high-water, replacement/pause/remove/shutdown join cleanly, and no superseded manager remains. |
| Product/platform | Generated schema/TypeScript/UniFFI projections retain scheme, lifecycle, redacted URL, and plaintext versus encrypted-unauthenticated security. Rust desktop tests, Android x86_64/arm64 cross-builds, and an owned no-window AVD controlled HTTPS announce pass. |
| Baseline/resources | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, established generated-contract/web/Android gates affected by changed views, and exact operation/request/body/resolver high-water and terminal counts. |
| Opt-in live | After deterministic closure, the current official Ubuntu torrent may exercise its public HTTPS tracker tiers and IPv6-family observations with no secret and bounded duration. This is supporting evidence only and requires the repository's explicit live-smoke opt-in. |

An environment without usable routed IPv6 may mark a live public IPv6 row
unavailable, but the deterministic loopback/controlled AAAA-only and outbound
IPv6 vertical are stopping-condition evidence and may not be replaced by URL
parsing alone. Android AVD network limitations must be recorded precisely;
cross-build success is not on-device HTTPS evidence.

## Implementation Slices

1. **Pure catalog and schedule boundary.** Generalize magnet/catalog/runtime
   endpoint types, preserve stored URL/tier/source behavior, make schedule
   actions/outcomes transport-neutral, add optional swarm/tracker-ID/BEP 31
   state, and pass pure mixed-transport tests. Do not open HTTP sockets yet.
2. **Pure HTTP tracker protocol.** Add exact query construction, Basic
   extraction/redaction, bounded permissive response parsing, compact and
   noncompact IPv4/IPv6 peers, tracker ID, interval/failure policy, and hostile
   parser tests in the runtime-independent protocol layer.
3. **Bounded reqwest runtime.** Add narrowly featured reqwest clients, explicit
   unauthenticated rustls configuration, policy/family-aware DNS, redirects,
   gzip, body limits, timeouts, cancellation, per-row continuation state, and
   scripted HTTP/HTTPS tests before integrating live scheduling.
4. **Long-lived integration and truthfulness.** Dispatch HTTP(S) actions from
   the Tactical `092` owner under the common operation budget, ingest common
   outcomes and peers, preserve endpoint generation/counters/lifecycle, and
   project active rows plus encrypted-unauthenticated security through existing
   application contracts.
5. **Vertical evidence and closure.** Prove controlled HTTP, untrusted-
   certificate HTTPS, AAAA-only/only-`peers6`, IPv6 outbound transfer,
   RSTorrent/libtorrent interoperability, Android runtime, cleanup, and
   resource rows; update all owning topics, readiness/protocol claims, and this
   tactical with exact landed evidence.

Each slice must leave the workspace formatted and its focused tests passing.
Logical commits are allowed after each gate. A partial implementation must
continue showing unlanded HTTP/HTTPS rows as unsupported and may not infer
support from a parser, reqwest client, or one successful public announce.

## Deliberate Deferrals And Next Boundary

- **Authenticated HTTPS certificates.** The immediate follow-up tactical must
  enable certificate-chain and hostname validation, integrate and initialize
  `rustls-platform-verifier` with its Android Kotlin/Gradle component, prove
  desktop and Android system trust, reject self-signed/expired/wrong-host
  certificates, and change the product security projection before any secure
  HTTPS claim. Enterprise/user roots, custom CAs, pinning, and trust overrides
  require explicit product policy.
- Full BEP 7 multi-address announcing, simultaneous per-family announces,
  routable IPv6 incoming listeners, per-family advertised endpoints, IPv6
  firewall/pinhole ownership, multiple interfaces, scoped link-local sources,
  rebinding, VPN/metered policy, Android local-network permission, and physical
  IPv6 reachability evidence.
- Proxy configuration and authentication, system-proxy discovery, proxy DNS
  semantics, SOCKS, cookies, digest/bearer authentication, client certificates,
  arbitrary request headers, and a general credential store.
- Tracker scrape/BEP 48, BEP 41 UDP URL data, WebSocket trackers, web seeds,
  HTTP tracker server behavior, HTTP/2, HTTP/3, HSTS, brotli, deflate, zstd,
  content sniffing, and arbitrary malformed-header tolerance.
- `external ip` tracker-response policy. It cannot become advertised-endpoint
  or reachability authority without a separate corroboration and ownership
  design.
- Durable tracker ID, cookies, redirect cache, DNS cache, response peers, or
  volatile tracker outcome history across restart.
- A general tracker transport framework, plugin interface, separate HTTP
  manager, companion server, native host, or socket proxy.
- A full BEP 7, authenticated HTTPS, private-tracker interoperability, general
  public-tracker reliability, or stable external compatibility claim from this
  initial transport slice.

The recommended next slice is authenticated platform certificate validation.
If evidence instead shows that proper IPv6 listener/advertisement ownership is
required for the target trackers to return useful peers, stop at the honest
outbound-only support level and create the dual-stack reachability tactical
before broadening BEP 7 claims.

## Escalation Contract

Implementation may proceed within this tactical without asking about internal
names, module placement, exact test fixture syntax, ordinary refactoring,
adding the focused reqwest features and declared `async-compression`
dependency, implementing the bounded gzip/`x-gzip` decoder under the declared
constraints, or tightening declared limits from deterministic evidence. It
may update generated contracts and existing tracker presentation to state the
accepted security semantics.

Stop for direction if evidence requires enabling certificate validation in
this slice, weakening HTTP framing or network policy, forwarding credentials
across origins, advertising a nontruthful family port, adding a proxy or
general TLS setting, changing the accepted private-tracker policy, adding a
dependency with broader license/platform tradeoffs than the declared reqwest
and `async-compression` features, modifying persistence compatibility beyond
tracker-catalog generalization, launching a public/live smoke without explicit
opt-in, using a real private tracker credential, or expanding into IPv6
listener/reachability ownership.
