# Tactical 205: Durable DHT And PEX Controls

Status: **Completed 2026-09-01.** Durable default-on DHT and PEX controls,
live engine convergence, generated client boundaries, React and Android
controls, and proportional validation are complete.

Topics: `settings-mutation-and-draft-consistency`, `dht-discovery`,
`peer-lifecycle`, `client-surfaces`, `android-jstorrent-replacement`,
`application-view-api`, `client-persistence`, `protocol-support`,
`capability-readiness`, and `oracle-driven-engine-campaign`

Dependencies: completed DHT Tactical [`016`](016-dht-discovery-foundation.md),
bounded PEX Tactical [`094`](094-bounded-bep11-peer-exchange.md), live-settings Tactical
[`097`](097-live-client-settings-and-replaceable-session-generations.md), and
disposable schema Tactical
[`179`](179-disposable-incubation-state-epoch.md).

## Decision And Desired Outcome

Add durable, default-on session controls for DHT and peer exchange to the
shared settings contract. Apply them live through the existing typed settings
reconciler, expose configured/effective/application truth, and present the
same backed controls in Android Compose and the shared React product UI.

The switches control discovery participation, not privacy policy. A private
torrent must remain unable to announce through DHT or negotiate, receive,
retain, or emit PEX even when both session settings are enabled.

## Scope

- Persist `dht_enabled` and `peer_exchange_enabled` in the singleton client
  settings row and sparse patch contract, defaulting both to `true`.
- Add independently converging DHT and PEX domains to the live settings view.
- Let the long-lived DHT owner disable all wire participation without replacing
  its UDP route or node identity, cancel in-flight lookups, retain only its
  already bounded warm routing snapshot, and bootstrap again on re-enable.
- Give every current and future torrent/incoming peer path one shared live PEX
  policy. Disable advertisement, receive, and send immediately; purge PEX-only
  candidates; and send a repeated BEP 10 update to established public peers on
  disable or re-enable.
- Generate TypeScript, Kotlin, and Swift boundary bindings; add React and
  Compose controls, draft/rebase behavior, localization source strings, and
  focused contract/UI tests.
- Advance the disposable incubation store schema once and preserve the
  bounded reset/payload-sentinel contract.

## Non-Goals

- Per-torrent DHT or PEX overrides, schedules, battery/network heuristics, or
  changing the existing Android background-lifetime policy.
- Closing the shared UDP socket when DHT is disabled; trackers and uTP share
  the session transport.
- Persisting arbitrary DHT packets, PEX contacts, or a second settings store.
- Adding LSD, peer caches, proxy controls, a daemon, or a remote API.
- Changing BEP 5, BEP 10, BEP 11, or BEP 27 support claims beyond recording
  the new live policy evidence.

## Stable Scenarios And Invariants

1. Fresh and upgraded disposable profiles start with DHT and PEX enabled; an
   accepted patch survives reopen and is reflected in configured/effective
   runtime truth.
2. Disabling DHT cancels every pending lookup, sends no query or response,
   stops maintenance/bootstrap work, and reports disabled. It keeps at most the
   existing bounded warm routing state and can re-bootstrap without replacing
   the session UDP generation or node identity.
3. Disabling PEX stops advertisement, receive, and outbound diffs for existing
   and future connections, purges all PEX-only registry provenance, and cannot
   remove tracker-, DHT-, manual-, or incoming-backed observations.
4. Re-enabling PEX sends a repeated BEP 10 advertisement to established public
   extension-capable peers and resumes bounded PEX state without replacing
   torrent or peer task generations.
5. Private or privacy-unknown torrents remain DHT/PEX-blocked independently of
   either switch. Re-enabling a global setting can never weaken that gate.
6. Rapid patches converge to the newest nonzero attempt; an unchanged save is
   a supported retry; shutdown joins the existing owners with no added task.
7. PEX adds one shared atomic/watch policy cell and one receiver per live
   torrent/connection, not a queue or task. DHT command capacity remains
   bounded and disabling clears its already bounded transaction/lookup state.

## Source-First Record

### Normative specifications

Pinned BEP revision `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`
was inspected:

- `reference/bittorrent.org/beps/bep_0005.rst` defines DHT participation,
  routing, queries, replies, tokens, and peer announcement;
- `reference/bittorrent.org/beps/bep_0010.rst` defines connection-local
  extension IDs and repeated handshakes, including disable-by-zero;
- `reference/bittorrent.org/beps/bep_0011.rst` defines bounded `ut_pex`
  exchange and minute cadence; and
- `reference/bittorrent.org/beps/bep_0027.rst` requires private torrents to use
  only their private tracker, excluding both DHT and PEX.

### Pinned libtorrent 2.0.13

Exact commit `7d7fc38fac61177fa5e02148f791b2f65250b09d` was
inspected:

- `include/libtorrent/settings_pack.hpp::enable_dht` and
  `src/session_impl.cpp::{update_dht,start_dht}` own live session DHT start and
  stop;
- `include/libtorrent/torrent_flags.hpp::{disable_dht,disable_pex}` and
  `src/torrent.cpp::{set_flags,should_announce_dht}` own live per-torrent
  discovery gates while preserving private-torrent exclusion;
- `src/ut_pex.cpp` checks `disable_pex` before advertising, receiving, and
  emitting diffs;
- `src/{read_resume_data,write_resume_data}.cpp` preserve disable flags; and
- `test/test_flags.cpp` exercises dynamic DHT/PEX disable and re-enable,
  `test/test_dht_storage.cpp` exercises repeated session DHT stop/start, and
  `test/test_privacy.cpp` covers private-session behavior.

RSTorrent adopts live, reversible policy and independent private gating. It
differs by exposing two session-wide product settings and by retaining the
existing bounded DHT routing snapshot while disabled rather than destroying
the session UDP/DHT owner.

### JSTorrent product behavior

JSTorrent commit `25e4b701433fd815398ba89526546f5e4f072e3f` was
inspected:

- `android/app/src/main/java/com/jstorrent/app/ui/screens/settings/NetworkSettingsScreen.kt`
  exposes default-on DHT and PEX switches;
- `android/app/src/main/java/com/jstorrent/app/settings/AndroidConfigHub.kt`,
  `SettingsViewModel.kt`, and `ConfigBridge.kt` persist and apply them;
- `packages/engine/src/config/schema.ts` defines both default-on fields;
- `packages/engine/src/core/bt-engine.ts::{enableDHT,disableDHT}` starts,
  stops, restores, and persists DHT routing state; and
- `packages/engine/src/core/torrent-peer-handler.ts` installs PEX only for
  public torrents when enabled and rechecks policy before accepting contacts.

RSTorrent follows the visible product shape but strengthens live PEX disable:
existing negotiated connections stop both directions and PEX-only candidates
are purged immediately. No reference source or fixture is imported.

## Owner, Task, Cancellation, And Dependency Map

```text
ClientSettings patch -> SessionStore singleton -> settings reconciler task
                                             |-> DhtService actor command
                                             |-> PeerExchangePolicyHandle
                                                    |-> torrent peer owners
                                                    `-> incoming peer loops
```

- `rstorrent-session` owns durable intent, attempt generations, application
  state, and the one existing settings reconciliation task.
- `rstorrent-engine::dht` owns DHT wire state, bounded tables, command
  cancellation, observation lifecycle, and shutdown.
- `rstorrent-engine::{network,driver,incoming,pex}` owns the task-free shared
  PEX policy and applies it through existing torrent/connection owners.
- Protocol codecs remain runtime-free and inward of engine/session clients.
- React and Compose mutate only the generated sparse application patch; they
  do not own runtime policy or persistence.

No new background task, socket, daemon, or unbounded channel is introduced.
Application shutdown still cancels reconciliation, joins discovery/peer
owners, then stops DHT and the shared UDP generation through the established
dependency order.

## Validation

- deterministic DHT disabled/start/lookup-cancel/re-enable/shutdown tests;
- deterministic public/private PEX live-toggle, repeated-handshake, purge, and
  mixed-provenance tests for outgoing and incoming paths;
- schema 25 fresh/reopen/reset/payload-sentinel and settings patch tests;
- generated contract drift, React draft/UI, Compose draft/UI, localization,
  desktop build, Android cross-build, and Apple generated-boundary checks;
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, and
  `cargo test --workspace`; and
- existing controlled DHT and PEX interoperability suites remain regression
  evidence; no new wire behavior requires another public-swarm claim.

## Implementation And Evidence

- Disposable schema 25 stores both booleans with constrained default-true
  columns. Fresh, sparse-patch, reopen, prior-schema reset, and payload
  sentinel tests passed.
- The existing DHT actor now owns a reversible disabled lifecycle. Disable
  cancels bounded transactions and lookups, clears learned peers, suppresses
  wire and maintenance work, and preserves the route and node identity;
  enable bootstraps the same owner. The advertisement scheduler observes that
  lifecycle and wakes registered public torrents immediately after re-enable.
- One task-free `PeerExchangePolicyHandle` feeds current and future outgoing
  and incoming peer owners. Disable-by-zero and re-enable BEP 10 updates,
  PEX-only provenance purge, retained non-PEX provenance, idempotent policy
  changes, and private-torrent exclusion are covered by deterministic tests.
- The application view reports configured, effective, and independently
  converging DHT and PEX application truth. TypeScript, Kotlin, and Swift
  generated boundaries consume the expanded contract.
- React and Compose expose localized default-on controls and send sparse
  patches. The web interaction suite exercised both controls; an API 34 AVD
  Compose test exercised each switch and asserted the two independent patch
  fields. Switches also have explicit accessibility descriptions.

Validation completed on 2026-09-01:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings`;
- `cargo test --workspace`;
- focused DHT actor, discovery-scheduler, PEX policy/state, and repeated BEP 10
  handshake-update tests;
- `npm run generate --prefix clients/web`, localization drift checks,
  `npm run typecheck --prefix clients/web`, `npm run test --prefix clients/web`
  (377 passed, 2 skipped), and the production web build/CSP check;
- `clients/android/build.sh` for x86_64 and arm64-v8a, followed by
  `assembleDebugAndroidTest` and the focused Compose test on the API 34
  `jstorrent-dev` AVD; and
- `clients/ios/scripts/test.sh` (30 unit and 4 UI tests).

The same focused Android test could not enter its body on the attached Android
17 preview device because the installed Espresso stack still reflects on the
removed `InputManager.getInstance` method. The API 34 AVD run passed, so this
device/tooling incompatibility does not reduce the product evidence above.

## Stopping Condition

This tactical is complete when both settings round-trip durably, configured
and effective application truth converges live, established public torrents
obey reversible DHT/PEX policy without generation replacement, private
torrents remain unconditionally excluded, React and Android expose backed
localized controls, generated clients are current, the proportional validation
matrix passes, owning topics record exact evidence, and the completed slice is
committed.
