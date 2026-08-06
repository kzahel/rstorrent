# Tactical 098: Authenticated HTTPS Tracker Platform Trust

Status: Planned on 2026-08-06 after the maintainer accepted platform trust as
the default, one hidden compatibility/debug override, and live replacement of
the small tracker HTTP client pool. Implementation has not started.

This tactical is sequenced strictly after Tactical
[`097`](097-live-client-settings-and-replaceable-session-generations.md).
Tactical `097` is a hard architectural prerequisite: its final
`SessionNetworkRuntime`, settings reconciliation, and configured/effective
view shape own the insertion point. Do not implement this tactical against the
pre-`097` application-lifetime machinery or preserve an interim parallel
settings path if `097` changes the exact private types described below.

Topics: `tracker-discovery`, `client-persistence`, `application-control`,
`application-view-api`, `client-surfaces`, `code-organization-and-refactoring`,
`capability-readiness`

Dependencies: completed Tacticals
[`084`](084-persisted-client-connection-and-seeding-settings.md),
[`095`](095-bounded-http-https-tracker-transport.md), and
[`096`](096-metadata-tracker-activation-and-family-observability.md) establish
the typed settings waist, bounded HTTP/HTTPS tracker transport, long-lived
tracker owner, metadata-only activation, and truthful tracker projection.
Tactical [`097`](097-live-client-settings-and-replaceable-session-generations.md)
must be complete before implementation begins because it replaces the current
session-network and live-settings machinery this slice will extend.

## Decision And Motivation

Tactical `095` deliberately configured both reqwest/rustls certificate and
hostname checks off. HTTPS tracker traffic is encrypted against passive
observers but an active intermediary can impersonate the tracker. That is a
security boundary, not a cosmetic support gap. Tracker announce URLs may
contain private-site passkeys in their path or query and may carry Basic
credentials; an intermediary can also observe the info hash, return attacker-
selected peers, suppress discovery, or consume repeated client bandwidth.

Make platform certificate-chain and server-name validation the production
default on desktop and Android. Continue using the existing reqwest 0.13.4
rustls stack and its locked `rustls-platform-verifier` 0.7.0 backend. Do not
introduce native-tls, OpenSSL as RSTorrent's TLS runtime, a bundled CA store,
certificate pinning, or browser/Java proxy transport.

Add one explicit persisted compatibility/debug policy:

```text
HttpsServerAuthenticationPolicy
  system_trust
  disabled

ClientSettings
  ...post-Tactical-097 fields...
  tracker_https_server_authentication: HttpsServerAuthenticationPolicy
```

`system_trust` is the default and requires both a valid certificate chain
under the platform trust policy and a name valid for the requested tracker
host. `disabled` retains TLS encryption while deliberately authenticating no
server identity. It is not called insecure plaintext and it must never be
projected as authenticated.

This setting is part of the typed application API, persisted settings,
headless tools, generated contracts, and the advanced/debug API surface. It is
omitted from the ordinary React Settings controls and from Compose. The
ordinary React form must nevertheless preserve the hidden field when saving
the rest of the settings group; it must never reset it implicitly.

Web seeds will eventually need a distinct
`web_seed_https_server_authentication` field using the same enum. Content
hashes protect downloaded bytes from silent publication, but do not protect
web-seed URL credentials, query tokens, request privacy, availability, or
bandwidth from impersonation. No web-seed setting is added in this tactical
because RSTorrent has no web-seed owner to enforce or report it. Tracker and
web-seed policy must not be coupled later merely because pinned libtorrent
uses one setting for both.

## Stopping Condition

This tactical is complete when all of the following hold:

1. a fresh or migrated profile defaults tracker HTTPS server authentication
   to `system_trust`, and the setting passes through the existing atomic
   `ClientSettings` command, persistence, receipt/replay, snapshot, generated
   TypeScript/JSON Schema, and UniFFI contracts;
2. every HTTPS tracker operation started under `system_trust` requires both
   platform-trusted chain validation and requested-host validation, while HTTP
   and UDP behavior is unchanged;
3. macOS uses Security.framework/keychain policy, Windows uses the Windows
   certificate store and verification APIs, Linux uses the discovered system
   CA bundle with WebPKI verification, and Android uses its system
   `X509TrustManager` through the packaged verifier component;
4. Android packages the version-matched
   `rustls-platform-verifier-android` component and initializes verifier JVM,
   application-context, and class-loader references once before any native
   network owner or reqwest client can be constructed;
5. a live policy change reconciles through Tactical `097` and atomically
   replaces the current IPv4/IPv6 tracker client pair; operations started
   after the replacement use the new policy while already-running operations
   finish under their captured old policy;
6. replacement adds no client per request, mutable verifier, separate settings
   task, discovery restart, torrent restart, application restart, permanent
   second pool, or retained client-generation history;
7. failure to construct the requested `system_trust` pair never falls back to
   `disabled`; it leaves new HTTPS tracker work unavailable/degraded while
   HTTP, UDP, peers, DHT, listener, reachability, and existing tracker schedule
   state remain alive;
8. disabling verification is possible only through the typed advanced/debug
   contract, emits a bounded structured warning on each transition that
   becomes effective, survives reopen, and is truthfully visible as
   `encrypted_unauthenticated` without leaking a tracker URL or credential;
9. tracker rows distinguish `unencrypted`, `encrypted_system_trust`, and
   `encrypted_unauthenticated`, and an active or completed announce reports the
   exact policy captured for that operation even if the configured policy
   changed while it was running;
10. controlled tests accept a valid chain/name and reject an unknown issuer,
    expired and not-yet-valid leaf, wrong DNS name, wrong IP SAN, missing
    intermediate, and invalid server purpose under `system_trust`; the same
    certificate failures reach the server only when policy is explicitly
    `disabled`;
11. redirect, credential, URL redaction, address-family, network-policy,
    timeout, response-size, decompression, operation-ceiling, and cancellation
    invariants from Tactical `095` remain exact on every TLS policy; and
12. desktop platform, Android AVD product, controlled tracker-to-peer, resource
    high-water, generated-contract, workspace, and terminal-cleanup gates pass
    with exact evidence recorded here and in the owning topics.

This proves authenticated HTTPS tracker transport under platform trust plus
one explicit compatibility escape hatch. It does not implement web seeds,
client certificates, custom CA import, certificate pinning, revocation policy
configuration, proxies, tracker login UI, or a general TLS subsystem.

## Product And Persistence Contract

### One shared policy enum, one enforced field

`HttpsServerAuthenticationPolicy` is a portable closed enum owned with the
other settings contract values. Its initial variants and meanings are exact:

| Value | Meaning | Default |
| --- | --- | --- |
| `system_trust` | Require the platform trust decision and requested DNS/IP name. Failure ends that HTTPS tracker attempt. | Yes |
| `disabled` | Encrypt with TLS but accept any certificate chain and name. This is an advanced compatibility/debug override. | No |

Do not split certificate and hostname validation into separate controls. A
certificate-only mode is easy to misunderstand, does not authenticate the
requested tracker, and has no accepted product need. Do not add an `automatic`
value whose result cannot be inspected. Absence of a usable system store is a
degraded secure configuration, not implicit authorization to disable checks.

The field is a required member of the post-`097` `ClientSettings` group. The
schema advances once from the version that Tactical `097` actually lands;
do not preselect a numeric version in this document. Every supported prior
schema migrates to `system_trust`. Existing profiles therefore become secure
by default rather than silently preserving Tactical `095`'s temporary
unauthenticated behavior. Malformed values fail closed during profile open and
are never coerced to `disabled`.

Atomic mutation, durable revision, exact receipt replay, conflict, no-op, and
rollback semantics remain those of the existing complete settings group. An
ephemeral profile accepts and applies the same value for its process lifetime
under Tactical `097`; it makes no durability claim.

### Hidden ordinary UI, explicit advanced control

Generated TypeScript, JSON Schema, validators, Rust, and Kotlin types expose
the setting. The existing semantic `SetClientSettings` command remains the
only mutation path; do not add a tracker-specific REST route, environment
variable, CLI-only side channel, local-storage mirror, or untyped string map.

The ordinary connection/seeding Settings section does not render the field.
Its draft initialization, equality checks, validation, and save operation must
round-trip the authoritative configured value unchanged. Focused tests must
prove that saving a visible listener, peer, or slot field while the hidden
tracker policy is `disabled` preserves `disabled`, and that a refresh does not
replace a local in-flight draft with a default.

An advanced/debug consumer may send the generated typed command directly.
If an advanced UI is added during implementation, it must be opt-in, clearly
label `disabled` as unauthenticated, and require an explicit choice; building
such a UI is not required by this tactical. Compose receives compile-checked
types but no control.

### Configured, effective, and operation-captured truth

Extend the post-`097` runtime settings view with an independent
`tracker_https_authentication` application domain. It reports:

- configured policy from the authoritative settings group;
- optional effective policy owned by the installed client pair;
- `applying`, `applied`, or `degraded` state using Tactical `097`'s common
  bounded status shape; and
- a stable coarse construction failure plus at most 512 UTF-8 bytes of
  redacted detail when no requested pair could become effective.

At startup the domain is not `applied` until the required client pair is
successfully constructed. A client pair is usable for both HTTP and HTTPS,
but a platform-verifier construction failure affects only new HTTPS tracker
operations. HTTP trackers continue with the existing family-specific client
behavior; implementation may retain a verifier-independent HTTP path or fail
HTTPS before dispatch, but it must not disable HTTP discovery merely because
secure trust initialization failed.

`TrackerSecurityView` becomes:

```text
unencrypted
encrypted_system_trust
encrypted_unauthenticated
```

For UDP/HTTP it is always `unencrypted`. For an HTTPS row that has never
started, it reflects the currently effective tracker policy, or the configured
policy it would be required to use when no HTTPS-capable pair has ever become
effective. When an announce starts, its record captures and immediately
publishes the exact client-pair policy used. The row retains that captured
policy with its last outcome until the next operation starts. Consequently,
an old operation that finishes after a live change may truthfully remain
`encrypted_unauthenticated` while the global settings view already says future
operations use `system_trust`. This is intentional; configured/effective
policy and historical operation truth must not be conflated.

`encrypted_system_trust` means the operation required chain and name
validation. A successful status under that policy proves the connection
passed. A failure can still carry the same policy and a classified TLS error;
the enum itself does not falsely claim that a failed handshake authenticated a
server.

## Live Application And Client-Pair Ownership

Tactical `097` owns one coalescing desired-settings channel, one settings
reconciler task, generation fencing, configured/effective state, same-value
retry, and joined shutdown. This tactical adds one reconciliation domain to
that owner. It must not add another channel or task.

The HTTP tracker runtime retains one current `Arc<HttpTrackerClients>`-like
value containing the IPv4 and IPv6 reqwest clients. Exact type placement may
change under Tactical `097`; the invariant is one atomically replaceable
current pair shared by the long-lived discovery service.

Reconciliation is:

1. persist and publish configured intent under Tactical `097` rules;
2. mark the tracker HTTPS authentication domain applying;
3. construct one candidate IPv4/IPv6 pair for the requested policy, including
   loading or binding platform trust state;
4. if construction succeeds and the attempt generation is still current,
   atomically install the candidate and publish it effective/applied;
5. if construction fails, drop the candidate and publish degraded without
   installing a weaker policy; and
6. let each retired pair drop when its last already-running operation releases
   its `Arc`.

Because the current reqwest pair serves HTTP and HTTPS, a secure verifier
construction failure must not remove ordinary HTTP service. The installed
pair therefore carries explicit HTTPS eligibility in addition to its clients:
it may be `system_trust`, `disabled`, or `unavailable`. On first-start secure
construction failure, a replacement pair built only for HTTP use may become
current with HTTPS eligibility `unavailable`; dispatch must reject HTTPS
before issuing a request through it. During a failed upgrade, the retained
old pair can continue HTTP while its HTTPS eligibility is fenced unavailable.
This is still one current family pair, not a parallel HTTP pool, and the
`unavailable` value is runtime failure state rather than a persisted policy.

New operations load the current pair once at operation start and retain that
exact `Arc` through DNS, redirects, TLS, body handling, and completion. They do
not reload policy between redirects. An already-running operation is neither
cancelled nor restarted when the setting changes. Tactical `095`'s aggregate
30-second operation deadline bounds how long an old pair can remain solely
because of network work.

When moving from `disabled` to `system_trust`, no new HTTPS operation may start
through the old unauthenticated pair after configured intent has been accepted.
The HTTPS dispatcher pauses or rejects new HTTPS work as temporarily
unavailable until the secure candidate commits; already-started operations may
finish. When moving from `system_trust` to `disabled`, continuing to use the
stricter old pair until the candidate commits is safe. On candidate failure,
the effective field remains the last actually installed policy when one
exists, while the domain is degraded against configured intent. For a failed
upgrade from disabled to system trust, that retained pair is fenced
`unavailable` for new HTTPS work while remaining usable for HTTP.

Every accepted save, including the same value and exact request replay,
creates a fresh runtime attempt under Tactical `097`. Rebuilding
`system_trust` on a same-value save intentionally retries Android
initialization/client construction and reloads Linux system roots, whose
current backend snapshots the CA bundle when the verifier is constructed.

No generation owns a background client task. reqwest clients and TLS verifier
state are passive shared resources; existing tracker operations remain the
only task owners. Shutdown closes new operation admission, cancels/joins the
existing discovery operations, drops the current pair, and then completes
through Tactical `097`'s session-network shutdown.

## Platform Trust And Packaging Contract

### Desktop

Keep reqwest configured with `default-features = false` and `rustls` plus
`stream`. With certificate and hostname bypasses removed for `system_trust`,
reqwest 0.13.4 constructs `rustls-platform-verifier` 0.7.0 using the active
rustls crypto provider. RSTorrent does not load OpenSSL for TLS and does not
enable reqwest's native-tls backend.

The accepted backend semantics are:

- macOS 10.14 and later: Security.framework evaluates platform roots,
  keychain trust decisions, name policy, and supported revocation data;
- Windows: Windows certificate-store and chain-policy APIs evaluate platform
  trust, name policy, and supported revocation data;
- Linux/BSD: `rustls-native-certs` plus `openssl-probe` discovers the system
  CA bundle, then WebPKI evaluates the chain and name; this backend does not
  provide revocation checking and loads roots when the verifier is built.

These platform differences are truthful behavior, not reasons to bundle a
second Mozilla root store silently. A Linux installation with no discoverable
usable CA bundle becomes degraded for `system_trust`. The compatibility escape
hatch remains an explicit user decision.

The production crypto provider remains the one already selected by the
reqwest/rustls dependency graph. This tactical does not change AWS-LC versus
ring provider policy.

### Android

The locked Rust dependency pulls
`rustls-platform-verifier-android` 0.1.1. The Android Gradle app must locate its
version-matched on-disk Maven repository through `cargo metadata` and package
`rustls:rustls-platform-verifier` from that repository. Do not copy the AAR
into this repository, use `latest.release` from a network repository, or let
the Gradle artifact version drift from Cargo.lock. Add the documented keep
rule if shrinking is enabled now or later.

Add one Android-only native initialization boundary in `rstorrent-android`
using the locked 0.7.0 API (`android::init_with_env` or its equivalent if the
locked version changes deliberately). A small application-owned Kotlin
bootstrap supplies the process `Application` context before either
`EngineService` calls `interfaceVersion`/constructs `EngineSession` or
`ProductEngineService` calls `AndroidApplicationClient.open`. The
initialization is idempotent for one process, retains only JVM/context/class-
loader global references required by the verifier, and fails application
network startup explicitly if it cannot initialize.

Do not depend on the stale function names in prose examples when the locked
crate source differs. Do not let lazy first use occur on an arbitrary Tokio
thread before the Android context exists. A unit/startup test must prove
ordering, and an AVD run must exercise an actual HTTPS tracker through the
ordinary foreground-service product path.

Android's platform verifier delegates chain trust to the system
`X509TrustManager` and performs requested-host verification in rustls after a
successful platform trust result. This explicit name check is required;
JSTorrent's current raw `SSLSocket` Android path does not set HTTPS endpoint
identification and is not the behavior to copy.

## Tracker Security And Failure Invariants

- Preserve Tactical `095`'s five-redirect maximum and accept only HTTP to
  HTTP, HTTP to HTTPS, or HTTPS to HTTPS. HTTPS to HTTP remains rejected under
  both authentication policies.
- Each HTTPS redirect authenticates the requested host for that hop under
  `system_trust`. The TLS client must receive the URL hostname, not a resolved
  IP substituted as the server name. IP-literal URLs require the matching IP
  subject alternative name.
- Same-origin Basic credentials may remain; cross-origin credentials are
  removed. Certificate failures, redirect failures, and diagnostics never
  echo userinfo, path passkeys, query values, headers, certificate bytes, or
  platform exception strings that may contain the full URL.
- Preserve policy-filtered DNS before connect, selected-family consistency,
  literal-address checks, no proxy, HTTP/1.1 only, explicit decompression, body
  ceilings, address ceilings, aggregate timeout, cancellation, and the shared
  eight-operation tracker ceiling.
- `disabled` changes only certificate-chain and server-name authentication.
  It does not weaken TLS protocol parsing, redirect downgrade prevention,
  URL validation, DNS/address policy, origin credential rules, timeouts,
  response parsing, or peer endpoint validation.
- Emit one structured warning when a `disabled` candidate becomes effective
  and once on startup when persisted `disabled` becomes effective. Do not warn
  for every announce. The event carries the policy, settings/runtime
  generation, and coarse category, but no tracker URL or credential.
- Classify handshake failures into stable bounded categories sufficient for
  support and tests: `unknown_issuer`, `expired_or_not_yet_valid`,
  `name_mismatch`, `invalid_server_purpose`, `certificate_rejected`,
  `verifier_unavailable`, and `tls_protocol`. Do not promise identical native
  error strings across platforms. If the dependency does not expose a stable
  finer distinction, use the conservative parent category rather than parse
  display text.

## Owner, Task, Cancellation, And Dependency Map

```text
SessionStore
  -> authoritative configured ClientSettings
ApplicationService
  -> accepted command publication
post-097 SessionNetworkRuntime
  -> existing desired-settings cell and reconcile task
     -> tracker HTTPS authentication domain
        -> current Arc<HTTP tracker IPv4/IPv6 client pair>
DiscoveryAdvertisementService
  -> existing bounded tracker operation tasks
     -> capture current client-pair Arc and security policy
     -> DNS / HTTP redirect / TLS / response pipeline
Android Application bootstrap
  -> one process platform-verifier JNI initialization before network owners
```

Portable policy enum, persistence validation, and deterministic transition
state do not depend on reqwest, rustls, Tokio, sockets, Android, JNI, SQLite,
or product views. The engine HTTP tracker runtime depends inward on the policy
value and owns client construction/operation use. The session owner translates
configured policy into live reconciliation and views. The Android adapter
owns JVM/platform initialization; the engine must not import Android types.

Use concrete structs and functions. Do not add a generic TLS provider trait,
settings callback registry, dependency-injection container, new crate,
process-global mutable policy, native host, or transport daemon. A narrow
test-only client/verifier constructor is allowed to supply deterministic trust
roots without weakening production construction.

## Resource And Lifetime Bounds

- Steady state owns exactly two reqwest clients: one IPv4 and one IPv6 client
  in the current pair. There is no always-resident on/off pool.
- One serialized reconciliation may hold the current pair and one candidate,
  for at most four clients before commit or candidate failure.
- Retired pairs are retained only by already-running operations. Under the
  existing session-wide eight-operation ceiling, rapid successful policy
  changes can retain at most one distinct old pair per in-flight operation
  plus the current pair: nine pairs/eighteen clients. Coalescing and serialized
  reconciliation add no unbounded candidate series.
- Each operation still has the aggregate 30-second deadline, so network work
  cannot retain an old pair indefinitely. A stalled settings consumer has one
  latest desired value, not a queue of client pairs.
- Platform trust objects and CA material are owned inside their client pair.
  No certificate, chain, trust store, OCSP/CRL result, or native error history
  enters an application view or diagnostic retention buffer.
- Android owns one process-lifetime set of JVM/context/class-loader global
  references required by the verifier. Repeated settings changes do not add
  JNI global references.
- Existing tracker bounds remain exact: eight simultaneous operations, five
  redirects, 30-second total deadline, 4-MiB wire and decoded body ceilings,
  bounded URLs/credentials/headers, 200 response peers, and existing DNS and
  address-candidate ceilings.

Validation records current/candidate/retired pair high water, clients, tracker
operations by captured policy, Android initializer count, warnings, and
terminal owner counts. The expected steady high water is two clients; a test
that reaches the declared transient maximum must show the corresponding
in-flight operations and bounded release.

## Normative And Reference Dossier

TLS server authentication follows the WebPKI/platform contract provided by
rustls and the operating system rather than a BitTorrent BEP. BEP 3 and
tracker conventions remain relevant to announce fields and credentials;
Tactical `095` remains the owner of HTTP wire, redirect, body, peer-response,
and resource behavior.

### Pinned libtorrent 2.0.13

The required checkout is `reference/libtorrent` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` (`v2.0.13`). Inspected paths and
cases are:

- `include/libtorrent/settings_pack.hpp::validate_https_trackers` documents
  validation against the system certificate store and notes the compatibility
  reason for disabling it;
- `src/settings_pack.cpp` defaults `validate_https_trackers` to true and binds
  its live update callback;
- `src/session_impl.cpp::{init_ssl,update_validate_https}` loads platform
  trust and switches the live SSL context between peer verification and
  `verify_none`;
- `src/socket_type.cpp::ssl_stream::set_host_name` supplies SNI and a hostname
  verification callback;
- `test/web_seed_suite.cpp` disables validation only for a controlled
  self-signed HTTPS fixture; and
- `docs/security-audit.rst` records default HTTPS validation as the security
  posture.

RSTorrent adopts default-on validation, system trust, requested-host checking,
a compatibility escape hatch, and live update as completeness behavior. It
does not copy libtorrent's OpenSSL context architecture or couple trackers and
web seeds in one global boolean. The RSTorrent enum is deliberately explicit,
and separate future tracker/web-seed fields avoid libtorrent's historically
misleading setting breadth.

### Locked reqwest/rustls platform verifier

Cargo.lock currently resolves reqwest 0.13.4,
`rustls-platform-verifier` 0.7.0, and its Android support crate 0.1.1. Before
implementation, re-check the lockfile after Tactical `097`; if versions moved,
repeat this source audit and record the new exact paths and behavior here.

Required inspected upstream paths are:

- reqwest `src/async_impl/client.rs`, where enabled rustls certificate and
  hostname verification selects `rustls_platform_verifier::Verifier`, while
  disabled certificate validation selects `NoVerifier`;
- verifier `README.md` for the platform matrix, Linux root-load timing,
  Android Gradle packaging, keep rule, and initialization requirement;
- verifier `src/verification/{apple,windows,others,android}.rs` for native
  trust, fallback, error, and hostname behavior;
- verifier `src/android.rs` for the exact locked initialization APIs and
  process-global reference ownership; and
- verifier `src/tests/{verification_mock,verification_real_world,ffi}.rs` for
  wrong-name, trust-chain, time, purpose, and Android native-context cases.

The verifier is dual MIT/Apache-2.0 and already enters the locked dependency
graph through reqwest. The Android AAR is its version-matched support artifact,
not copied RSTorrent source. Adding direct dependency declarations needed to
call the Android initialization API does not authorize a TLS-stack change.

### JSTorrent product history

The inspected local checkout is `../jstorrent` at
`9895410beeed6aff554053769bd006a3fbd373ef`. Relevant paths are:

- `desktop/io-daemon/src/ws.rs` uses `native-tls::TlsConnector` with normal
  verification by default and exposes a transport-level `skipValidation`
  capability that tracker requests do not select;
- desktop's native-tls backend uses Security.framework on macOS, Schannel on
  Windows, and system OpenSSL on Linux rather than a bundled OpenSSL build;
- `android/io-core/.../TcpSocketService.kt` uses
  `SSLSocketFactory.getDefault()` and starts a handshake;
- `android/quickjs-engine/.../TcpBindings.kt` hard-codes tracker-facing
  `skipValidation` false; and
- the legacy Chrome application ordinarily used browser XHR for HTTPS, while
  its raw Chrome socket HTTP path did not support HTTPS.

JSTorrent corroborates default platform trust and a nonordinary debug bypass.
RSTorrent does not adopt desktop native-tls/OpenSSL or Android's raw
`SSLSocket` implementation. No Android endpoint-identification configuration
or wrong-host regression test was found in that raw socket path, so it is not
sufficient evidence for RSTorrent hostname validation. No source or fixture is
copied.

### Current RSTorrent pressure points

The pre-implementation survey found:

- `crates/rstorrent-engine/src/http_tracker.rs::{HttpTrackerClients,
  build_client}` owns the family pair and currently sets both reqwest danger
  flags true;
- `crates/rstorrent-engine/src/advertisement.rs` constructs one pair for the
  long-lived discovery service and passes borrowed access into operations;
- `crates/rstorrent-session/src/settings/contract.rs` owns the typed complete
  settings group and generated values;
- `crates/rstorrent-session/src/tracker_views.rs` currently derives
  `encrypted_unauthenticated` only from the URL scheme rather than an operation
  policy;
- `crates/rstorrent-android/src/lib.rs` has the in-process cdylib/UniFFI
  boundary but no platform-verifier initialization;
- `experiments/android-engine-bootstrap/app/build.gradle.kts` does not package
  the verifier Android component; and
- both Android services can construct native network owners without a prior
  application-level TLS initializer.

Tactical `097` is expected to change the session and client-pair insertion
point. Reinspect these paths after it lands and update this dossier rather
than forcing its planned private names onto the resulting code.

## Shape-Changing Edge Cases

The common implementation and tests must include:

- fresh, migrated, durable-reopen, and ephemeral defaults; explicit disable;
  malformed durable enum; no-op save; exact replay; and rollback before live
  application;
- ordinary visible Settings save while the hidden field is disabled, stale
  snapshot arrival during a local draft, generated old-snapshot defaults if
  still supported, and advanced typed mutation without an added API route;
- valid DNS SAN and IP SAN; wrong DNS name; DNS certificate requested by IP;
  wrong IP SAN; wildcard boundaries; unknown/self-signed issuer; missing and
  wrong intermediate; expired and not-yet-valid leaf; invalid server purpose;
  malformed certificate/TLS; and absent system trust;
- direct HTTPS and every permitted redirect combination, including a valid
  first hop followed by an invalid second certificate/name, changed origin
  with stripped Basic auth, same-origin retained auth, family mismatch, loop,
  and forbidden downgrade under both policies;
- `system_trust -> disabled`, `disabled -> system_trust`, same-value rebuild,
  A-to-B-to-A coalescing, candidate construction failure, stale candidate
  completion, and shutdown during construction;
- old unauthenticated operation crossing a successful secure replacement,
  old secure operation crossing disablement, no new insecure dispatch during
  a secure upgrade, exact operation-captured tracker projection, and release
  at the 30-second deadline;
- eight simultaneous operations spread across rapid replacements without more
  than nine retained pairs/eighteen clients, followed by steady two clients
  and terminal zero operation ownership;
- Android initializer called once before both service paths, repeated
  initialization, missing packaged component, Java exception/class lookup
  failure, service restart in one process, and process restart;
- macOS keychain/platform root acceptance, Windows system-store acceptance,
  Linux discovered bundle acceptance/reload and missing-bundle failure, with
  platform-specific revocation limitations reported truthfully; and
- disabled-policy warning cardinality and every error/log/view redaction case
  using synthetic passkeys and Basic credentials only.

## Staged Implementation And Intermediate Gates

### Gate 1: contract, persistence, and pure live state

After Tactical `097` is complete, add the policy enum, tracker field, secure
default migration, independent application domain, operation-captured
security enum, and pure reconciliation transitions. Regenerate every contract
consumer. Prove that ordinary hidden-field round trips and configured/
effective/operation truth are deterministic before changing TLS behavior.

### Gate 2: secure desktop client construction

Parameterize client-pair construction by the policy, remove both danger
bypasses from `system_trust`, retain them together only for explicit
`disabled`, and add a narrow deterministic trust-root fixture seam. Pass the
complete valid/invalid chain, name, IP, redirect, credential, timeout, and
redaction matrix on the host platform while preserving all Tactical `095`
tests.

### Gate 3: post-097 live reconciliation

Install atomic current-pair replacement through the existing settings
reconciler, gate new work during an insecure-to-secure transition, capture one
pair per operation, classify candidate failures, implement same-value reload,
and measure current/candidate/retired resource bounds. Do not restart
discovery or torrents. A controlled in-flight announce must cross each policy
transition with truthful projection.

### Gate 4: Android platform integration

Package the version-matched AAR from Cargo metadata, add the process bootstrap
and native initialization boundary, prove initialization ordering/failure,
cross-build both established ABIs, and run controlled valid/invalid HTTPS
tracker cases plus the ordinary product path on an AVD. Do not accept a build-
only Android gate for the platform trust claim.

### Gate 5: interoperability and closure

Run a controlled authenticated HTTPS tracker that introduces RSTorrent to a
pinned libtorrent peer for an exact hash-verified transfer. Run one opt-in
public HTTPS metadata-only smoke with no private credential on desktop and AVD
as corroborating platform evidence, not deterministic correctness. Update this
tactical, tracker/protocol/readiness topics, API presentation, and claims with
exact platform results, resource high water, commands, and remaining gaps.

Each gate must leave the workspace buildable and its selected tests green.
Do not temporarily make `system_trust` mean reqwest defaults on desktop but
the old bypass on Android; the secure default is not complete until every
shipping platform path enforces it.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure contract | Enum/default serialization; post-`097` schema migration; malformed fail-closed state; atomic command/no-op/replay/rollback; configured/effective/applying/degraded transitions; stale attempts; hidden-field preservation; operation-captured security projection. |
| Scripted TLS/runtime | Valid and all named invalid certificate/name cases; redirect-hop validation; credential stripping/redaction; policy transition during in-flight work; candidate failure; same-value root reload; cancellation; timeouts; exact pair/client/operation high water. |
| Session/application | Startup and reopen for both policies; secure construction unavailable without insecure fallback; HTTP/UDP continuity; long-lived tracker schedule/metadata activation retained; warning cardinality; shutdown during replacement; terminal owner cleanup. |
| Controlled interoperability | Authenticated HTTPS tracker introduces RSTorrent to pinned libtorrent for exact hash-verified content; an invalid controlled certificate/name never receives a valid announce under system trust; explicit disabled mode reaches the same synthetic fixture and remains labelled unauthenticated. |
| Desktop platforms | Runtime system-trust success and controlled rejection on macOS, Windows, and Linux, plus no-window Tauri builds. If a platform cannot run in the available environment, the tactical remains incomplete for that platform claim rather than treating cross-compilation as runtime proof. |
| Android | Version-matched AAR packaging; initializer-before-network test; x86_64 and arm64-v8a release cross-builds; debug JVM suite; no-window AVD valid-chain/name success, invalid-chain/name rejection, explicit disabled compatibility, ordinary product tracker state, and clean process teardown. |
| Product contracts | Rust/JSON Schema/TypeScript/validators/UniFFI/Kotlin values and defaults agree; React Settings does not render the field but preserves it; advanced typed commands can read/write it; tracker views render all three security values without false authentication. |
| Opt-in live | One credential-free public HTTPS metadata-only attempt on desktop and AVD records system-trust policy and platform, reaches tracker rows, and creates no payload artifacts. Public availability does not gate deterministic correctness or become a reliability claim. |
| Workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, generated-artifact drift checks, web tests/typecheck/build/CSP scan, Android builds/tests, dependency/license audit, `git diff --check`, and terminal-zero harness checks. |

Controlled fixtures use synthetic credentials, locally generated test
certificates, bounded content, and temporary roots. Production code never
trusts the fixture root. Any temporary host/AVD trust state and TLS artifacts
must be scoped to the harness and removed at teardown. Physical devices,
private tracker credentials, and external account state are not authorized by
this tactical.

## Non-Goals And Deliberate Deferrals

- No web-seed BEP 17/19 transport or
  `web_seed_https_server_authentication` field before that owner exists.
- No separate certificate-only or hostname-only setting.
- No custom CA file/directory setting, user certificate importer, per-tracker
  exception, certificate pin, TOFU, certificate viewer, or OS trust-store
  mutation by the product.
- No client TLS certificate, OAuth, cookie jar, arbitrary request header,
  private tracker account UI, passkey store, or credential migration.
- No proxy, PAC, VPN/interface selection, DNS-over-HTTPS, TLS interception
  accommodation, or ambient platform proxy credentials.
- No change to redirect, downgrade, Basic-origin, URL, DNS/address, tracker
  response, peer-intake, timeout, operation, decompression, or memory bounds.
- No cancellation/restart of already-running tracker operations on a policy
  change, and no discovery, DHT, listener, reachability, peer, torrent, or
  application restart.
- No permanent secure/insecure pool pair, client per operation, unbounded
  client cache, mutable verifier, generic TLS abstraction, generic settings
  callback system, new crate, daemon, native host, or IPC boundary.
- No promise of identical revocation behavior across platforms. Linux's
  current verifier backend does not perform revocation checking.
- No physical Android/ChromeOS test, private/public tracker reliability claim,
  or public-swarm performance claim.

## Next-Slice Boundary

After this tactical, HTTPS trackers can make a truthful authenticated platform-
trust claim and retain one explicit advanced compatibility override. A later
web-seed tactical may reuse `HttpsServerAuthenticationPolicy` while adding its
own separately persisted field, owner, integrity/credential policy, range and
resource bounds, views, and evidence. It must not silently inherit the tracker
choice.

Other tracker follow-ups such as proxies, scrape, additional authentication,
durable tracker IDs, or reliability policy remain independently selected work.
This slice does not promote them merely because TLS is authenticated.

## Escalation Contract

Implementation may proceed without routine maintainer input for private
module/file names, exact atomic-swap primitive, a narrow test-only root seam,
same-boundary refactoring after Tactical `097`, direct declarations of the
already locked verifier/JNI dependencies required for Android initialization,
generated-contract updates, conservative error coarsening, and adversarial
cases implied by these invariants.

Stop for direction if evidence requires a different persisted/default policy,
showing the control in ordinary UI, coupling tracker and web-seed settings,
changing the TLS backend or crypto provider, bundling roots/OpenSSL, mutating a
user trust store, keeping more than the declared bounded client generations,
weakening redirect/credential/network policy, adding a new crate/process/IPC
boundary, using a private credential or physical device, changing supported
profile compatibility, or expanding into a deferred TLS/tracker feature.
