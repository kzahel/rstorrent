# Tactical 199: Android Live Unmetered-Network Enforcement

Status: **Ready as of 2026-08-30.** Maintainer direction selected this as the
next Android replacement plan. No implementation, emulator/device mutation,
or release action has occurred yet.

Topics: `android-jstorrent-replacement`, `beta-release-readiness`,
`application-control`, `application-view-api`, `client-surfaces`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed live session-network Tactical
[`097`](097-live-client-settings-and-replaceable-session-generations.md),
completed Android product Tactical
[`117`](117-jstorrent-shaped-android-product-ui.md), and completed ChromeOS
companion Tactical
[`194`](194-chromeos-android-extension-control.md). Ready external-intake
Tactical [`197`](197-android-external-torrent-intake.md) and notification
Tactical
[`198`](198-android-completion-and-attention-notifications.md) may execute
independently. This tactical must preserve their one-service, one-application
owner and must not depend on either presentation slice landing first.

## Decision And Desired Outcome

Add one default-off Android setting named **Unmetered networks only**. When it
is disabled, the current ordinary Android online behavior is unchanged. When
it is enabled, BitTorrent networking is permitted only while Android reports
that RSTorrent's current default network is usable and unmetered. Unknown,
unavailable, unvalidated, suspended, blocked, or metered state closes the live
BitTorrent network prerequisite.

This is a cost-control feature, not a Wi-Fi transport selector:

- validated unmetered Wi-Fi, Ethernet, cellular, or a virtual default network
  is eligible;
- metered Wi-Fi is ineligible;
- an unmetered transport that is not the application's current default does
  not make the engine eligible;
- the classifier does not infer cost from `TRANSPORT_WIFI` or
  `TRANSPORT_CELLULAR`; and
- VPN presence is neither required nor rejected. The capabilities of the
  application's current default network determine only this cost policy.

The setting defaults off, matching maintained JSTorrent Android and avoiding
a silent behavior change for fresh RSTorrent installations. Production
handoff Tactical `JAR-004` must later decide whether and how a legacy
JSTorrent `wifi_only_enabled` value migrates into the production package.

The application-level network prerequisite remains separate from each
torrent's durable desired state. Closing the prerequisite does not issue
Pause, Pause All, Stop, or a settings command. It prevents new BitTorrent
DNS/socket work, cancels and joins existing BitTorrent network generations,
and leaves every torrent's running/paused intent, queue position, priorities,
limits, metadata, verified pieces, and storage state unchanged. Reopening the
prerequisite admits only torrents whose existing desired intent and queue
policy permit work.

The Android activity, Compose presentation, SAF operations, notification
owner, and same-device ChromeOS companion control connection are not
BitTorrent egress and remain usable while the prerequisite is closed. A user
must be able to inspect, pause, remove, repair, or change a setting while all
peer/tracker/DHT traffic is stopped.

## Scope And Stopping Condition

This tactical owns:

1. one service-lifetime Android default-network observer using
   `ConnectivityManager.registerDefaultNetworkCallback`, with a task-free
   pure reducer for ordered callback facts and exact unregister ownership;
2. the normal `ACCESS_NETWORK_STATE` manifest permission required by that
   callback API;
3. one installation-local default-off **Unmetered networks only** preference,
   directionally ordered with live policy changes and exposed on Android's
   Network & Privacy settings page;
4. a typed initial and live Android-to-Rust network-prerequisite boundary that
   can begin closed before `ApplicationService::open` creates any BitTorrent
   socket, DNS lookup, DHT task, listener, or mapping;
5. one application-owned, latest-generation live network prerequisite layered
   over the existing fixed `Offline`/`LoopbackOnly`/`Online` address policy;
6. immediate close-before-join behavior and bounded asynchronous convergence
   for outgoing TCP/uTP peers, incoming TCP/uTP peers, HTTP and UDP trackers,
   tracker DNS, DHT, PEX carried by peers, coordinated UDP families,
   discovery advertisement, listeners, and port-mapping/reachability owners;
7. automatic reactivation through the existing application admission and
   session-network owners without rewriting torrent intent or creating a
   second engine path;
8. typed waiting/application truth in generated views, Android Library and
   detail presentation, Network settings, structured diagnostics, and the
   foreground notification;
9. deterministic Rust/Kotlin/generated-boundary coverage, controlled TCP/UDP
   traffic accounting, both Android ABI builds, API 28 and API 35 AVD
   campaigns, and a bounded physical-phone handoff campaign; and
10. exact cleanup of callbacks, transition tasks, network generations,
    sockets, DNS work, peer permits, mappings, and test policy mutations.

The tactical stops only when:

- the default-off preference leaves current online behavior unchanged and
  survives a process restart;
- enabling it on a metered, unknown, or unavailable default network closes the
  engine gate before returning success and converges every owned BitTorrent
  network surface to zero without changing durable torrent intent;
- a process started while the persisted setting is enabled and eligibility is
  unknown or metered performs zero BitTorrent DNS, bind, listen, mapping,
  tracker, DHT, peer, or payload network work before the first eligible fact;
- a genuine controlled transfer crosses unmetered to metered to unmetered,
  produces no new BitTorrent packets after bounded close convergence, and
  resumes from retained verified state without a user Resume action;
- a torrent paused by the user before or during restriction remains paused
  after eligibility returns, while desired-running queued/download/seeding
  torrents resume only under their ordinary admission rules;
- outgoing/incoming TCP, outgoing/incoming uTP, HTTP tracker, UDP tracker,
  DHT, listener, and mapping cases all prove closed or absent while blocked;
- Compose and the ChromeOS companion remain controllable and present truthful
  waiting state while BitTorrent egress is blocked;
- rapid callback, setting, process-recovery, and service-shutdown races cannot
  reopen an old eligible generation; and
- full deterministic, controlled runtime, generated contract, Android build,
  AVD, physical-device, resource high-water, and terminal-zero evidence is
  recorded here and in the owning topics.

Passing this tactical closes replacement gate `JAR-008` and the required
unmetered portion of beta gate `AND-010`. It does not implement or qualify
VPN-only behavior, SOCKS proxying, Android background lifecycle, or a
zero-packet privacy guarantee before Android reports a network change.

## Non-Goals

- A VPN-only setting, VPN detection presented as protection, Android
  `Network.bindSocket`, process binding, underlying-network selection, VPN
  handover leak prevention, kill-switch claim, or packet-level privacy claim.
- SOCKS4, SOCKS5, HTTP proxy, proxy DNS, tracker/peer proxy selection,
  credentials, UDP ASSOCIATE, proxy fallback, or proxy bypass prevention.
- Literal Wi-Fi-only behavior, SSID/BSSID inspection, Wi-Fi permissions,
  transport allowlists, roaming policy, Data Saver configuration, background-
  data exemption requests, or changing the user's system network.
- Requesting or bringing up an unmetered network through `requestNetwork`,
  scheduling deferred jobs through WorkManager/JobScheduler, or changing
  Tactical `JAR-009`'s foreground/background/idle lifetime decision.
- Persisting Android capability snapshots or network identities. Only the
  user's preference is durable; platform network facts are transient.
- Per-torrent metered policy, one-time metered overrides, schedules, data
  quotas, byte budgets, speed throttling, selective seeding, or separate
  upload/download cost settings.
- Treating the network prerequisite as Pause All, changing desired-running
  intent, moving queue positions, persisting a temporary stopped state, or
  exposing a generic remote command that bypasses Android policy.
- Closing Compose, SAF, notification, or same-device companion control merely
  because BitTorrent egress is blocked. Tactical `198` separately owns the
  notification-visibility prerequisite for retaining the service itself.
- BEP, tracker, DHT, peer-selection, encryption, port-mapping, or storage
  feature breadth unrelated to making existing network owners dynamically
  ineligible.
- Web-seed coverage. BEP 17/19 remains absent and cannot be claimed by testing
  a path that this product does not implement.
- Production branding, application ID, legacy migration, signing, Play
  declarations, release publication, or store rollout.

## Accepted Android Eligibility Contract

The Android owner reduces one persisted preference and ordered facts for the
application's current default `Network` into:

```text
Unrestricted
WaitingForDefaultNetwork
WaitingForCapabilities
WaitingForValidatedInternet
WaitingForUnmeteredNetwork
WaitingForUsableNetwork
```

Only `Unrestricted` permits BitTorrent networking. The reducer selects it
when either:

- **Unmetered networks only** is disabled; or
- the setting is enabled and the current default network has
  `NET_CAPABILITY_INTERNET`, `NET_CAPABILITY_VALIDATED`,
  `NET_CAPABILITY_NOT_METERED`, and `NET_CAPABILITY_NOT_SUSPENDED`, and is not
  reported blocked. On API 29 and newer, an explicit
  `onBlockedStatusChanged(..., false)` fact is required before eligibility;
  absence of that initial fact remains unknown and closed.

`NET_CAPABILITY_TEMPORARILY_NOT_METERED` alone does not satisfy the first
contract. The stable Android recommendation for large-transfer cost policy is
`NET_CAPABILITY_NOT_METERED`; broadening eligibility later requires an
explicit product decision and physical carrier evidence.

Do not call synchronous `getNetworkCapabilities` from a network callback.
`onAvailable` establishes a new current network in unknown state;
`onCapabilitiesChanged` supplies its ordered capabilities;
`onBlockedStatusChanged` supplies the ordered blocked fact on API 29 and
newer; and `onLost` clears state only if it refers to the current network.
Callbacks for a superseded `Network` cannot alter the active result.

The minimum supported API is 28. API 28 lacks
`onBlockedStatusChanged`; its eligibility uses the required capabilities and
ordinary socket failure behavior. API 29 and newer additionally require an
observed `blocked == false`. Callback registration failure or absence of an
initial capabilities or blocked callback is fail-closed when the setting is
enabled and non-blocking when it is disabled.

Register exactly one default-network callback while `ProductEngineService`
exists. Do not use a broad callback for every Internet-capable network: the
presence of some unmetered secondary network says nothing about where an
unbound RSTorrent socket will route. Unregister before service shutdown
returns. There is no manifest receiver, polling loop, timer, or retained
`Network` across process death.

## Preference And Product Contract

Add one app-private `product_network` preference:

- `unmetered_networks_only`, default `false`.

The preference belongs to the Android installation, not the portable Rust
profile, web local storage, Chrome extension storage, engine settings group,
or generated application command schema. Enabling first closes the native
gate, then persists `true`, then publishes the Compose value and applies the
latest eligibility fact. If persistence fails, keep the gate closed until the
previous durable `false` value is confirmed and its policy is deliberately
restored. Disabling first persists `false`, then relaxes the cost gate and
publishes the Compose value. A failed disable must not reopen it. Present one
bounded inline failure without repeated writes.

The Network & Privacy page replaces **Metered network policy** with:

- **Unmetered networks only**;
- default-off explanatory text such as **Pause network transfers on metered
  connections**; and
- current truth: **Unrestricted**, **Unmetered network**, **Metered network**,
  **Checking network**, **Network temporarily unavailable**, or **No validated
  internet**.

Keep **VPN-only mode** and **Proxy** visibly unavailable. Do not imply that
the unmetered setting supplies their privacy or routing semantics.

While blocked, Android Library presents one non-modal **Waiting for an
unmetered network** banner with a route to Network settings. Desired-running
torrents present a waiting/blocked assessment, zero live transfer rate, and
their normal Pause action; user-paused torrents remain Paused rather than
being relabeled. The foreground service status uses the same generic waiting
text and does not create an attention notification.

The ChromeOS companion receives the generated torrent/application truth from
the same application owner. It may inspect and control torrents while blocked
but cannot change the Android-only preference through a generic application
command. It directs the user to Android settings if an override is needed.

## Application And Engine Contract

### Static address policy versus live prerequisite

Retain `NetworkPolicy::{Offline,LoopbackOnly,Online}` as the fixed address-
scope and DNS policy selected for an application lifetime. Add a separate
live `ApplicationNetworkPrerequisite` with at least:

- `Allowed`; and
- `WaitingForUnmeteredNetwork`.

The effective result is conjunctive. A live prerequisite can reduce an
`Online` application to no BitTorrent egress but can never broaden `Offline`
or `LoopbackOnly`. Engine modules do not import Android `Network`,
`NetworkCapabilities`, preference, Compose, or service types.

The initial prerequisite is part of `ApplicationConfig` and the Android
UniFFI open record. It is evaluated before any session networking is started.
Changing it later uses a focused Android client method and an
application-owned handle, not a durable request envelope. Platform capability
churn is transient environment input rather than replayable user intent.

### Close-before-join and latest-generation convergence

The application owner exposes an O(1), synchronous close operation that:

1. advances a nonzero monotonic prerequisite generation;
2. changes the shared engine permit to closed with release ordering;
3. prevents new DNS, dial, accept, UDP send, tracker, DHT, mapping, and
   advertisement acquisition from that point; and
4. wakes one latest-state convergence owner.

The Android callback path must be able to perform that close without waiting
behind a long application mutex operation, SAF request, tracker timeout, or
network cleanup. Joined convergence is asynchronous and bounded; immediate
gate closure is not.

The convergence owner then cancels and joins every older network generation.
An allow transition starts or re-enables session networking first, publishes
its new effective generation, and wakes ordinary torrent admission. Rapid
`allow -> block -> allow -> block` changes coalesce to the latest requested
state; completion from an older start or stop cannot publish, install a
socket, restore a mapping, or wake torrents.

Generation overflow is a typed terminal refusal rather than wraparound.
Duplicate same-state facts are no-ops. A failed allow remains blocked and
publishes bounded degraded detail; a later capability event, setting save, or
explicit internal retry may create one newer attempt. There is no unbounded
background retry loop.

### Network surfaces that must converge

Closing the prerequisite must cover all currently implemented BitTorrent
network owners:

- pending and established outgoing TCP peer sockets;
- pending and established outgoing uTP peer streams and shared UDP egress;
- incoming TCP acceptors, pending handshakes, and established incoming peers;
- incoming uTP flows and shared UDP ingress/egress;
- HTTP tracker DNS, connect, TLS, request, and response work;
- UDP tracker transactions;
- DHT bootstrap, lookup, announce, request, response, routing maintenance, and
  both address-family UDP traffic;
- peer exchange only insofar as it travels on now-closed peer connections;
- discovery advertisement scheduling and its tracker/DHT registrations;
- TCP/UDP listener generations, advertised endpoints, UPnP work, and retained
  reachability generations; and
- queued connection attempts, peer-budget permits, timers, and wakeups capable
  of recreating any of the above.

Normal application shutdown may attempt bounded stopped announces and mapping
cleanup while its network remains permitted. Cost-policy closure is
different: close the gate first and do not send tracker `stopped`, DHT,
mapping-delete, or other cleanup packets over a network Android has already
classified ineligible. Record uncertain finite mapping state truthfully and
let the lease or old network disappear. A later eligible generation must not
advertise or reuse a stale endpoint.

Existing payload bytes already accepted into storage may finish bounded local
verification/checkpoint work after gate closure. Force recheck, payload hash,
SAF repair/removal, and other network-free operations remain eligible. No new
piece request, upload response, tracker/DHT message, DNS query, or peer write
may begin from a closed generation.

### Intent preservation and automatic restart

Do not call the durable pause/resume path. The application admission owner
must consider the live prerequisite in addition to desired-running, queue,
storage, and resource conditions. While blocked:

- desired-running incomplete torrents remain desired running and waiting;
- complete desired-running torrents remain intended seeds but perform no
  incoming or outgoing upload work;
- a new Add with start intent persists that intent but starts no networking;
- Pause changes durable intent to paused as usual;
- Resume changes durable intent to running but remains waiting; and
- removal, archive, priorities, limits, and local storage commands retain
  their ordinary semantics.

When allowed, the existing active-download cap and queue order select work.
Only still-desired-running torrents resume. A torrent paused, removed,
archived, failed, or made unavailable during restriction cannot be revived by
the network transition.

## Generated View And Observability Contract

Extend the application view contract rather than requiring clients to parse a
diagnostic string:

- `ProgressReason::WaitingForUnmeteredNetwork` is a blocked/waiting reason
  distinct from the fixed-policy `NetworkDisabled` reason;
- desired-running torrent projections use that reason while the prerequisite
  is closed and do not expose `EnableNetwork`, because no portable application
  command can change an Android platform preference;
- fixed `Offline` or `LoopbackOnly` blockage retains `NetworkDisabled`
  precedence; the new reason applies only when fixed `Online` is reduced by
  the live prerequisite;
- the session-network settings/runtime projection distinguishes configured
  listener/mapping intent from temporary prerequisite suspension and never
  reports a retired socket as listening or mapped;
- DHT inspection reports a suspended/ineligible lifecycle while retaining
  static `Online` policy truth and any bounded in-memory routing state; and
- current rate/history naturally reaches zero without clearing durable
  totals.

Generated TypeScript, JSON Schema, UniFFI Kotlin/Swift, validators, fixtures,
React exhaustive reducers, Android reducers, and iOS exhaustive reducers must
all accept the new closed enum values. Desktop, headless, iOS, and ordinary
web products construct `Allowed` and observe no behavior change.

Structured diagnostics record bounded transition facts:

- requested and effective prerequisite generation;
- `allowed`, `blocking`, `blocked`, `starting`, or `degraded` state;
- product reason `unmetered_required`, never Android network identity;
- convergence duration and terminal owner counts; and
- bounded failure class without SSID, BSSID, IP address, interface name,
  carrier, VPN name, endpoint, tracker URL, peer address, or preference file
  content.

Do not log the Android `Network` handle or full `NetworkCapabilities`. Android
callback counts and classification may be exposed only as bounded debug/test
observations without user or network identity.

## Ownership, Tasks, Cancellation, And Dependency Direction

```text
ProductEngineService
  -> ProductNetworkPreference (one app-private boolean)
  -> AndroidDefaultNetworkObserver (one registered callback)
       -> pure ordered eligibility reducer
          -> latest AndroidNetworkEligibility
             -> AndroidApplicationClient initial prerequisite
             -> focused live prerequisite transition

ApplicationService
  -> fixed NetworkPolicy (Offline / LoopbackOnly / Online)
  -> ApplicationNetworkPrerequisite owner
       -> synchronous shared permit closure
       -> one latest-generation convergence owner
          -> SessionNetworkRuntime network-permission domain
             -> listeners + incoming peer generations
             -> coordinated UDP/uTP + DHT
             -> discovery advertisement + reachability/mapping
          -> per-torrent network generations
             -> peer TCP/uTP + tracker HTTP/UDP/DNS
          -> view/diagnostic publication
          -> ordinary admission wake after allowed convergence

ChromeOS companion / Compose / SAF / notifications
  -> same ApplicationService control and view owner
  -> outside the BitTorrent network prerequisite
```

`AndroidDefaultNetworkObserver` and its reducer live in the Android adapter.
The reducer depends only on small value facts, not `Context`, coroutines,
Compose, Rust, or sockets. Android maps its result into the platform-neutral
application prerequisite.

The engine owns a concrete, cloneable permission/generation primitive. It is
separate from `NetworkConfig` so fixed address validation remains task-free
and so Android concepts do not leak inward. Runtime network owners depend on
that primitive; it does not depend on session persistence, view types, JNI,
or Android.

Extend the private `SessionNetworkRuntime` introduced by Tactical `097`.
Do not add a parallel Android engine, global service framework, network daemon,
socket proxy, process-wide Android network bind, second application service,
or generic policy plugin layer.

Successful service shutdown orders:

1. unregister the Android callback and prevent new preference/platform facts;
2. close the engine network permit and cancel network convergence;
3. join per-torrent network owners and session network generations;
4. continue the incumbent application, companion, SAF, presentation, and
   notification shutdown ordering; and
5. return only after callback, transition, DNS, socket, mapping, peer permit,
   wake lock, and Rust task counts are terminal.

## Resource And Timing Bounds

- Exactly one Android default-network callback exists per service lifetime.
- The Android reducer retains one current network token, its latest capability
  bits, optional blocked fact, one preference boolean, and one monotonically
  increasing local revision. It has no event history.
- The Rust owner retains one requested state, one effective state, one
  convergence task, one nonzero generation, and bounded diagnostic counters.
- A state replacement may overlap the retiring generation and one candidate
  generation only while the gate is allowed. A block transition creates no
  candidate socket.
- Gate closure is synchronous and O(1). Controlled tests require zero new
  acquisitions after it returns.
- Joined network convergence has a two-second target and a five-second hard
  test deadline. A deadline failure keeps the gate closed, records degraded
  state, and fails the tactical rather than detaching an owner.
- Android callback bursts are latest-value coalesced. No unbounded channel,
  coroutine launch per callback, retry list, network catalog, or packet log is
  permitted.
- Network identity, capabilities, tracker/peer addresses, and packet payloads
  are not persisted for this feature.

The two-second convergence target begins when the native gate closes, not when
the physical network actually changes. Android callback delivery before that
point is outside RSTorrent's control. This tactical is a strong cost-control
policy after observed state, not a VPN-grade claim of zero packets during an
unobserved handoff.

## Failure And Race Cases

- Service startup with the preference enabled begins Rust closed before
  callback registration or capability delivery. No optimistic `Online`
  window is allowed.
- `onAvailable` without capabilities remains unknown/closed. Only the ordered
  capabilities callback for that current network can allow it.
- `onLost` or capability/block callbacks for a superseded network are ignored.
- A new default network invalidates prior capability and blocked facts before
  awaiting the new ordered callbacks.
- Network loss racing a preference disable is serialized by revision. If the
  durable preference is now disabled, the cost prerequisite is unrestricted;
  ordinary OS connectivity failures still apply.
- Preference enable racing metered traffic closes the native gate before the
  UI reports success. Persistence or native-application failure cannot leave
  a displayed enabled setting with confirmed egress still open; stop the
  application owner if enforcement cannot be confirmed.
- Preference disable persists first. A failed native allow remains safely
  blocked and presents retryable degraded state.
- Block racing an outgoing DNS/dial, tracker request, peer write, incoming
  accept, UDP send, mapping renewal, or DHT response rejects new work and
  cancels/joins the captured older generation.
- Policy closure does not send a final tracker or mapping packet over the now-
  ineligible network.
- Allow racing a second block cannot publish or retain its newly bound socket.
  Candidate generation fencing retires it before the block converges.
- Process death loses transient capability facts. A sticky service restart
  reloads the preference but starts unknown/closed until a fresh callback.
- User Pause/Resume/Add/Remove racing a network transition commits application
  intent under the existing command/revision owner. The network generation
  never rolls that intent back.
- A local Force recheck begun before restriction may continue without network;
  its completion cannot reopen networking or fabricate a completion edge.
- Session network start/bind failure on an eligible network remains blocked
  and degraded. It never falls back to a metered or secondary network.
- Companion connection loss follows its own owner; blocking BitTorrent
  networking neither closes nor recreates the companion listener.
- Service Stop, Tactical `198` notification-denial shutdown, API-35 timeout,
  and unmetered blocking share idempotent cancellation but distinct outcomes:
  the first three stop the application; this tactical normally retains it.

## Current RSTorrent Findings

The implementation starts from a useful but lifetime-fixed policy:

- `crates/rstorrent-engine/src/network.rs` defines task-free
  `NetworkPolicy::{Offline,LoopbackOnly,Online}` and `NetworkConfig`. Policy is
  copied into peer, tracker, DHT, advertisement, and incoming owners.
- `crates/rstorrent-session/src/application.rs::ApplicationService::open`
  starts `SessionNetworkRuntime` unconditionally, then creates per-torrent
  runtimes with the same fixed `NetworkConfig`.
- `ApplicationService` already preserves durable desired-running intent
  separately from runtime activity. Fixed Offline projects
  `ProgressReason::NetworkDisabled` without converting the torrent to Error or
  Paused.
- `crates/rstorrent-session/src/session_network.rs` is the private joined owner
  for stable incoming state, coordinated TCP/UDP generations, uTP, DHT,
  discovery advertisement, reachability, mappings, bandwidth, and live
  settings reconciliation. Tactical `097` makes it the correct extension
  point.
- `SessionUdpService::{replace_socket,remove_family}` already supports stable
  handles around replaceable and removable address-family socket generations.
- live settings can remove/replace acceptors and mapping generations while
  preserving stable registrations and DHT state, but no current domain
  represents temporary whole-network ineligibility.
- `DownloadControl` and application supervision already have cancellation,
  checkpoint, resource-release, and desired-running restart foundations, but
  no distinct live network-generation cancellation.
- `ProgressReason::NetworkDisabled` currently implies an
  `EnableNetwork` action suitable for fixed Offline policy, not an automatic
  Android prerequisite.
- `crates/rstorrent-android/src/lib.rs::AndroidApplicationConfig` accepts only
  the lifetime-fixed `AndroidNetworkPolicy`; `AndroidApplicationClient` has no
  live prerequisite method.
- `ProductEngineService` always opens with `AndroidNetworkPolicy.ONLINE`.
- Android's Network & Privacy page shows unavailable VPN, metered-policy, and
  proxy rows.
- the manifest declares Internet but not `ACCESS_NETWORK_STATE`; and
- Android has no connectivity callback, cost classifier, network preference,
  or blocked-state presentation.

The concrete boundary improvement is a live application network prerequisite
that composes with the incumbent fixed address policy and the existing private
session-network owner. It must not smear Android capability checks through
peer, tracker, DHT, or storage modules.

## Reference Inspection

### Maintained JSTorrent Android product

The maintained sibling JSTorrent checkout at revision
`25e4b701433fd815398ba89526546f5e4f072e3f` was inspected on 2026-08-30:

- `android/app/src/main/java/com/jstorrent/app/settings/SettingsStore.kt`
  stores `wifi_only_enabled` and defaults it to false;
- `android/app/src/main/java/com/jstorrent/app/ui/screens/NetworkSettingsScreen.kt`
  exposes Wi-Fi-only and VPN-only switches;
- `android/app/src/main/java/com/jstorrent/app/network/NetworkMonitor.kt`
  derives unmetered state from the active network's
  `NET_CAPABILITY_NOT_METERED` capability;
- `android/app/src/main/java/com/jstorrent/app/network/NetworkRestrictionEnforcer.kt`
  combines settings and network facts, performs an initial block decision,
  and calls global suspend/resume while retaining its own `didSuspend` latch;
- `android/app/src/main/java/com/jstorrent/app/JSTorrentApplication.kt`
  constructs the enforcer before engine load and passes
  `shouldRemainSuspended` into initial configuration;
- `android/quickjs-engine/src/main/kotlin/com/jstorrent/quickjs/EngineController.kt`
  exports global suspend/resume calls;
- `packages/engine/src/core/bt-engine.ts::{suspend,resume}` preserves torrent
  `userState`, calls `stopNetwork` for torrents, and resumes only active queue
  entries;
- `packages/engine/src/core/torrent.ts::stopNetwork` cancels peer attempts,
  closes peers, stops torrent-local DHT lookup and maintenance, clears active
  request state, and sends a best-effort tracker `stopped` announce; and
- engine tests cover retained queue state and DHT/UPnP enablement on resume,
  while Android instrumentation chiefly covers setting persistence/toggling
  rather than packet-level closure.

RSTorrent adopts the default-off preference, fail-closed initial decision,
global prerequisite, and separation from torrent user intent. It does not
adopt JSTorrent's broad all-network callback followed by synchronous active-
network requery, string status values, detached coroutine per transition,
tracker `stopped` traffic after restriction, or incomplete evidence that
global DHT, incoming listener, and UPnP have stopped.

### Pinned libtorrent oracle

The required pinned libtorrent checkout was inspected at exact revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/session_handle.hpp::reopen_network_sockets` separates
  route/interface change handling from per-torrent commands;
- `src/session_handle.cpp::{pause,resume,reopen_network_sockets}` posts those
  operations to the session owner;
- `src/session_impl.cpp::{pause,resume,on_ip_change,reopen_network_sockets}`
  keeps a session-paused bit distinct from torrent pause flags, aborts tracker
  requests, applies session pause to every torrent, ignores incoming
  connections while paused, and reopens listeners when routing changes;
- `src/torrent.cpp::{set_session_paused,do_pause,do_resume,stop_announcing}`
  preserves the independent torrent pause bit, disconnects peers, stops
  discovery, and resumes only when the combined pause state permits it;
- `test/test_session.cpp::{paused_session,reopen_network_sockets}` covers
  independent torrent/session pause state and listener/mapping reopen calls;
- `simulation/test_session.cpp::ip_notifier_setting` exercises the route-
  notification owner; and
- `test/test_listen_socket.cpp`, including the device-IP-change cases, covers
  deterministic socket-generation partitioning.

The adopted completeness lessons are separate session versus torrent intent,
session-owned incoming gating, prompt peer/tracker cancellation, route-change
generation handling, and deterministic resume. RSTorrent intentionally does
not use libtorrent's public architecture or ordinary session pause as its
cost-policy contract: a pause path may issue tracker `stopped` traffic and is
not by itself proof that every DHT, UDP, mapping, or DNS owner is quiescent.

This feature changes no BitTorrent wire behavior and requires no new BEP.
Pinned libtorrent is an edge-case oracle, not a source donor. No reference
source, test, fixture, string, or asset is imported.

### Android platform contract

Official Android documentation was inspected on 2026-08-30:

- [Read network state](https://developer.android.com/develop/connectivity/network-ops/reading-network-state)
  recommends callbacks for fresh dynamic connectivity rather than polling;
- [`ConnectivityManager`](https://developer.android.com/reference/android/net/ConnectivityManager)
  defines `registerDefaultNetworkCallback`, exact unregister ownership, the
  per-UID callback bound, and its `ACCESS_NETWORK_STATE` requirement;
- [`ConnectivityManager.NetworkCallback`](https://developer.android.com/reference/android/net/ConnectivityManager.NetworkCallback)
  defines ordered current-default, capabilities, loss, and API-29 blocked-
  state callbacks and warns against synchronous capability queries inside a
  callback; and
- [`NetworkCapabilities`](https://developer.android.com/reference/android/net/NetworkCapabilities)
  distinguishes configured Internet, validated Internet, unmetered cost,
  suspension, and transport. It explicitly recommends
  `NET_CAPABILITY_NOT_METERED` rather than Wi-Fi/cellular transport for cost
  decisions.

The Android app observes the default network; it does not request, select,
bind, or preserve one. No new dangerous/runtime permission is introduced.

## Implementation Stages

1. Add the pure Android callback reducer, preference owner, manifest
   permission, default-off Settings row, state labels, and exhaustive callback
   ordering tests. Keep the Rust application closed/unchanged at this gate.
2. Add platform-neutral initial/live application prerequisite types, the
   synchronous shared close handle, latest-generation transition model,
   generated view truth, and deterministic transition tests. Regenerate every
   boundary consumer.
3. Extend session networking with a network-permission domain. Close and
   reopen listener, coordinated UDP/uTP, DHT, discovery, reachability, and
   mapping generations behind stable handles with stale-generation fencing.
4. Extend per-torrent supervision so the same prerequisite cancels peer and
   tracker/DNS network generations without changing desired state, while
   already-admitted bounded local verification/checkpoint work can finish.
5. Wire Android initial state before application open and live transitions
   through the focused client method. Add failure-safe setting ordering,
   Library/detail/foreground-status truth, and companion-visible generated
   state.
6. Run deterministic Rust/Kotlin/generated-contract and scripted runtime
   campaigns for every implemented network surface, including rapid changes,
   failed starts, restart, shutdown, resource bounds, and terminal zero.
7. Build both Android ABIs and run API 28 plus API 35 AVD campaigns with
   controlled TCP/UDP endpoints and explicit system metered-policy cleanup.
8. After explicit target authorization, run the bounded physical current-API
   phone Wi-Fi/cellular/metered-handoff campaign and record packet/UID-byte
   evidence without public swarms or uncontrolled payload volume.
9. Reconcile this tactical, the oracle restart checkpoint, replacement/beta
   gates, client/application truth, and capability matrix. Close only the
   unmetered portion of `AND-010`; leave VPN and proxy absent.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure Android reducer | Preference off/on; no network; available before capabilities; required capability combinations; metered Wi-Fi; unmetered cellular; suspended/blocked; current-network replacement; stale loss/capability/block callbacks; API 28 behavior; callback registration/unregister failure |
| Application transitions | Initial blocked open; close-before-join; latest generation; duplicate state; overflow refusal; failed allow; shutdown race; desired intent, queue, priority, limits, verified state, and storage unchanged |
| Per-torrent runtime | Active/queued/paused/newly added/incomplete/complete seed; outgoing TCP/uTP; incoming TCP/uTP; peer upload/download; HTTP/UDP tracker and DNS; DHT-derived peer; recheck/local checkpoint; automatic eligible restart |
| Session networking | TCP listeners; both UDP families; DHT lifecycle/state; discovery registration; advertised endpoint withdrawal; UPnP/uncertain lease; peer permits; timers/wakes; no stopped or cleanup packet after close |
| Generated product contract | New prerequisite/progress/runtime states in schema, TypeScript, Kotlin, Swift, validators, fixtures, React, Android, and iOS; fixed Offline remains distinct and unchanged |
| Android presentation | Default-off persisted toggle; current cost/validation truth; Library banner; per-torrent waiting; zero rate; foreground status; setting failure; process restart; Compose and companion control while blocked |
| Controlled traffic | Packet/endpoint counters stop after close convergence across TCP/UDP/DNS/tracker/DHT/peer/mapping paths; no new socket acquisition; resume uses only a newer eligible generation |
| AVD and physical | API 28/35 cold start and live unmetered/metered/loss/handoff, metered Wi-Fi, process recovery, exact transfer continuation, tiny bounded physical Wi-Fi/cellular cohort, policy restoration, uninstall, and terminal cleanup |
| Repository | Rust format/Clippy/test, generated-contract check, web typecheck/test, Kotlin format/lint/unit/instrumentation, dual Android ABI packages, no dependency/license/secret/source import |

### Deterministic and controlled runtime cases

At minimum, automated tests prove:

- fixed `Offline`, `LoopbackOnly`, and `Online` address policy remains
  independent from the live prerequisite;
- initial `Online + WaitingForUnmeteredNetwork` binds no listener or UDP
  family, performs no DNS, starts no DHT/bootstrap/mapping, and projects
  waiting rather than Error or Paused;
- blocking an established outgoing TCP download closes the peer, tracker, and
  discovery owners within the deadline and leaves exact desired intent and
  verified pieces;
- the same holds for uTP, incoming upload, HTTP tracker, UDP tracker, and DHT
  traffic;
- policy closure sends no tracker stopped, DHT response, mapping deletion, or
  peer keepalive after the gate-close observation;
- user Pause before/during block remains durable; Resume and Add while blocked
  update intent but create no network; removal prevents later restart;
- a complete desired-running seed accepts and uploads nothing while blocked
  and resumes only after eligibility;
- Force recheck and pending local write/checkpoint work remain bounded and do
  not reopen the network;
- allow starts one fresh session generation and ordinary admission selects the
  correct desired-running torrents under the active-download cap;
- A-to-B-to-C transitions fence late DNS, sockets, DHT responses, endpoint
  publications, mapping results, peer events, and admission wakeups;
- failed listener/UDP/session start remains blocked and degraded with no
  partial active owner;
- service shutdown during blocking or starting joins all children and leaves
  zero callback, task, socket, mapping, permit, timer, and queued-byte counts;
  and
- companion/API traffic and SAF operations remain available while BitTorrent
  TCP/UDP endpoint counters remain flat.

### Installed AVD campaign

Use explicitly owned API 28 and API 35 AVDs. Build and install the debug
product, then use deterministic host-controlled peer/tracker/DHT endpoints and
Android `adb`/emulator interfaces before any manual UI inspection.

The campaign records:

- exact API, ABI, application ID, target SDK, APK digest, preference state,
  active default network, redacted capability classification, and callback
  count;
- fresh-start disabled behavior and enabled-plus-unknown/metered zero-network
  behavior before application readiness;
- one genuine tiny controlled download across eligible, metered, unavailable,
  and eligible transitions with exact payload hash and retained progress;
- separate TCP, uTP/UDP, HTTP/UDP tracker, DHT, listener, and mapping endpoint
  counters plus gate-close/convergence timing;
- metered Wi-Fi, no validated Internet, suspended/blocked where scriptable,
  rapid handoff, preference toggle, process death/restart, and service Stop;
- Compose and same-device companion control while engine egress remains
  blocked; and
- exact restoration of emulator radio/netpolicy state, app preference/data,
  installed package, host endpoints, captures, temporary payload, and AVD
  ownership.

Do not count airplane mode alone as metered-policy evidence. Do not claim VPN
privacy from a VPN-shaped default network. Packet captures contain only
controlled fixture traffic and are removed after aggregate evidence is
recorded.

### Physical phone campaign

Physical validation requires explicit maintainer authorization and a claimed
current-API Android phone. Use only a tiny controlled private fixture and
record the maximum permitted cellular volume before beginning.

The campaign must prove:

1. unmetered Wi-Fi permits the controlled transfer;
2. handoff to cellular or an explicitly marked metered network closes all
   BitTorrent traffic within the native convergence deadline while Compose
   remains usable;
3. a process/service start on that metered state performs zero controlled-
   endpoint traffic before eligibility;
4. return to unmetered Wi-Fi automatically resumes only desired-running work
   and reaches the exact payload hash;
5. a user-paused torrent does not resume, and the foreground notification plus
   Library/detail state stay truthful; and
6. radio/policy state, package data, fixture, capture, process, service,
   listener, mapping, wake lock, and temporary files are restored or removed.

This proves observed cost-policy enforcement. It does not prove carrier
billing behavior, VPN binding, suspend/reboot background lifetime, or public-
swarm interoperability.

### Build and repository baseline

Run from the repository root after sourcing the configured profile:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
(
  cd clients/android
  ./gradlew lintDebug testDebugUnitTest assembleDebug assembleDebugAndroidTest
)
./clients/android/build.sh
```

Run connected instrumentation only on explicitly owned AVDs. Run physical
work only after the required authorization and record exact commands,
captures, byte limits, policy mutations, and cleanup.

## Documentation And Completion Updates

Before marking this tactical complete:

- record exact commits, reference paths, tests, commands, AVD/device classes,
  API/ABI/package digests, network transitions, controlled endpoint/packet
  counts, convergence timings, resource high waters, failures, and cleanup;
- mark `JAR-008` complete in
  [`android-jstorrent-replacement.md`](../topics/android-jstorrent-replacement.md);
- close only the unmetered requirement in `AND-010` and retain VPN as an
  explicit disclosed later feature in
  [`beta-release-readiness.md`](../topics/beta-release-readiness.md);
- update dynamic application-network truth in
  [`application-control.md`](../topics/application-control.md),
  [`application-view-api.md`](../topics/application-view-api.md),
  [`client-surfaces.md`](../topics/client-surfaces.md), and
  [`capability-readiness.md`](../topics/capability-readiness.md);
- update the exact source-first restart checkpoint in
  [`oracle-driven-engine-campaign.md`](../topics/oracle-driven-engine-campaign.md);
  and
- leave VPN-only, proxy, `JAR-009`, production migration/branding/signing,
  Play rollout, and publication open under their existing owners.

## Escalation Contract

Implementation may add the focused engine gate, transient application
prerequisite and generated view states, Android callback/preference owner,
manifest permission, native setting/presentation, deterministic fault hooks,
bounded controlled endpoint counters, and internal refactors required to make
the existing session-network owner converge without further direction.

Stop for maintainer direction if evidence requires:

- changing the default-off preference, accepting a capability other than the
  declared unmetered contract, or implementing a one-time metered override;
- treating cost policy as VPN privacy, binding sockets/processes to an Android
  `Network`, selecting underlying networks, or adding VPN/proxy behavior;
- persisting temporary restriction as torrent pause/stop state, changing queue
  semantics, or adding a portable command that can bypass Android policy;
- adding WorkManager, JobScheduler, another foreground service, process,
  daemon, socket proxy, system-settings mutation, or external dependency;
- changing the production package, legacy migration, production extension,
  signing/store state, or public release artifact;
- using a physical device, cellular data, public swarm, uncontrolled endpoint,
  or external publication without the stated authorization; or
- accepting an implementation that cannot prove one of the existing peer,
  tracker, DHT, UDP, listener, or mapping owners quiesces.

An ordinary Rust, Kotlin, generated-contract, build, AVD, callback, bind,
controlled-transfer, or cleanup failure is not an escalation. Diagnose it
within the declared owner and bounds.

## Oracle Restart Checkpoint And Next Action

The exact pre-implementation checkpoint is:

- fixed `NetworkPolicy` already blocks addresses and DNS but is copied at
  application open;
- `ApplicationService` already separates desired torrent intent from runtime
  activity and projects fixed Offline as non-error blockage;
- Tactical `097` already centralizes stable/replaceable session networking,
  and session UDP already removes/replaces socket families behind stable
  handles;
- Android always opens `Online`, lacks `ACCESS_NETWORK_STATE`, and has no
  callback, preference, or live native boundary;
- maintained JSTorrent proves the desired default/intent behavior but does not
  prove complete DHT/listener/mapping quiescence; and
- pinned libtorrent supplies the independent session-pause and network-
  generation edge checklist, not the architecture.

The next executable action is Stage 1: add the task-free Android eligibility
reducer and default-off preference with exhaustive ordered-callback tests,
then land the initial/live platform-neutral prerequisite contract before any
runtime socket behavior changes. Do not begin with Compose wiring alone or a
Pause All shortcut.

After this tactical closes, plan `JAR-009` background lifecycle or make an
explicit retain/defer decision for VPN-only mode. SOCKS proxy remains a
separate source-first engine campaign rather than the presumed continuation
of Android connectivity work.
