# Tactical 158: Desktop Signed Packaging And Updater

Status: **Implementation in progress and resumed as the sole Now on
2026-08-25.** Desktop-notification Tactical
[`164`](164-desktop-completion-and-attention-notifications.md) is complete.
Maintainer direction selected RSTorrent as the foreseeable public product name
and froze `com.jstorrent.rstorrent` as the desktop beta identifier on
2026-08-23. Cross-platform presubmit Tactical `159` is complete. The distinct
per-app updater key and required repository secrets are provisioned. Native
and React updater behavior, public configuration, release validators, and the
five-leg signed workflow pass their local and hosted gates. The production
route is deployed and a credentialed rehearsal produced all five signed
package sets. Tagged `desktop-v0.1.0` then passed draft finalization and became
the first public release. Tagged `desktop-v0.1.1` passed the same complete
release workflow. Exact installed macOS arm64 and Linux arm64
`0.1.0`-to-`0.1.1` production updates pass. Windows x86_64 NSIS replacement,
relaunch, signing, current-version checking, and private installation-ID
continuity pass under the supported automatic-loopback profile, but a fresh
default profile exposed a local-network listener-selection startup blocker.
Completed Tactical `160` repairs that defect on `main` and adds a passing
native Windows x86_64 regression, but no public signed package contains the
repair yet. A newer signed package plus clean default-profile update proof and
Linux x86_64 remain open. Completed Tactical `161` closes the native Windows
folder-picker blocker, makes the packaged Linux picker self-contained, passes
the hosted desktop matrix, and proves installed Windows
choose/cancel/repair/restart behavior in an unsigned package. The next signed
candidate can now exercise a complete fresh-profile setup. Maintainer
direction deliberately omits installed Intel macOS testing while retaining
its automated signed package and route. That unsigned Windows campaign also
exposed Windows Security listener consent; the next signed candidate must
characterize and document it without automatically granting a firewall rule.
Completed Tactical `162` closes single-instance, tray/background, joined
Quit, release-only Windows GUI launch, and installed Windows x86_64/Linux
arm64 lifecycle behavior. The next signed candidate therefore carries a
reachable and cleanly terminating desktop shell. Completed Tactical `163`
also adds installed external magnet and local `.torrent` activation. Completed
Tactical `164` adds native completion/attention notifications before this
tactical produces the next signed candidate.

Topics: `beta-release-readiness`, `client-surfaces`,
`product-state-and-feedback`, `product-surfaces-and-migration`,
`client-persistence`

Dependencies: completed Tactical
[`157`](157-beta-release-foundation.md); completed presubmit Tactical
[`159`](159-cross-platform-presubmit-ci.md); the maintained Tauri/React product;
the accepted external
[`desktop-update-v1`](https://github.com/kzahel/desktop-release-kit/blob/main/contract/desktop-update-v1.md)
contract; the completed signed canary campaign in `kzahel/desktop-release-kit`;
the shared `simple-app-update-server`; and the existing publisher signing and
notarization operations described outside this public repository.

## Decision And Desired Outcome

Adopt the already-proven shared desktop release contract without inventing a
second updater protocol or copying the canary's product presentation. RSTorrent
owns its route, per-app key, versions, release tags, packages, UI, lifecycle,
release workflow, and acceptance evidence. The shared server owns compatible
multi-product routing and GitHub release aggregation.

The finished slice produces signed installable desktop beta candidates for
macOS, Windows, and Linux plus an explicit user-approved in-app update path for
self-replacing packages. It proves a real older signed build updating through
the production route and relaunching into an exact newer build on every
supported updater target. Configuration, source tests, signatures, and
generated `latest.json` are necessary but do not alone satisfy the stopping
condition.

## Product Contract

### Stable identity and routing

- Stable versions are `MAJOR.MINOR.PATCH`; the desktop tag prefix is
  `desktop-v`.
- The foreseeable public product name is RSTorrent. Its desktop beta
  identifier is `com.jstorrent.rstorrent`, replacing the unreleased provisional
  `org.jstorrent.rstorrent` before the first package.
- The general later goal is to graduate the proven implementation as a normal
  JSTorrent update retaining JSTorrent branding, `com.jstorrent.desktop`, and
  the existing JSTorrent updater trust root. That best-effort transition is
  later scope. This tactical neither uses the JSTorrent updater private key nor
  changes the meaning of released RSTorrent routes or clients.
- The intended product route is
  `https://updates.graehlarts.com/rstorrent/tauri/{{target}}/{{arch}}/{{current_version}}`
  and the server product config uses a stable RSTorrent ID, `/rstorrent` path
  prefix, `kzahel/rstorrent` repository, and `desktop-v` tags.
- One unique Tauri updater key was generated for this application and the
  private key/passphrase plus shared publisher credentials were confirmed
  present as repository Actions secrets on 2026-08-23. Commit only the
  RSTorrent public key. Private values never enter source, logs, or shell
  arguments.
- The app fails closed: no unsigned, wrongly signed, incompatible, draft,
  missing-target, or malformed response can be installed.

### Check lifecycle and state

- Store one random UUID in the platform application-config directory using
  bounded atomic create/repair. Send it only as `X-CFU-Id` for update checks.
  It is resettable installation counting, not an account, authorization token,
  profile ID, device fingerprint, or general analytics ID.
- A future installation-wide `product.db` adopts or deliberately migrates this
  same identity rather than silently creating a second installation ID.
- Schedule one silent startup check five seconds after Tauri presentation
  initialization and one silent check every 24 hours while running. Expose a
  manual check. Deduplicate concurrent checks and use a 20-second timeout.
- Send exactly one `X-Check-Reason`: `startup`, `periodic`, or `manual`.
- Automatic failures are bounded diagnostics and do not interrupt torrent
  work. Manual results/failures are visible. A previously discovered update
  survives later silent checks.
- Represent idle, checking, up-to-date, available, manual-install,
  downloading, installing, and check/install error states. Available includes
  version and release notes; downloads show byte progress when known.
- Installation is always an explicit user action. Relaunch only after Tauri
  reports a successful install. Failure retains an appropriate retry or manual
  download path.

### Product presentation boundary

The shared React application remains browser/Tauri reusable. Tauri-only
plugins must not leak into demo or browser-hosted bundles.

- Define a small update-client/state boundary injected only by
  `startTauriInspection`; browser/demo/live gateway entry points omit it.
- Add an **About & updates** Settings category only when the update client is
  present. Show product version, build ID, target/architecture, bundle type,
  check/install state, release notes, manual check, install/relaunch, and
  manual package-channel guidance.
- Surface an automatically discovered update outside a closed Settings dialog
  through one accessible non-modal notice that opens the same state. Do not
  install, relaunch, or steal focus automatically.
- Keep release/update state outside the generated torrent application
  contract. It belongs to the desktop product lifetime, not a profile or
  torrent owner.
- Add bounded update diagnostics without logging the full installation ID,
  private filesystem locations, response bodies beyond reviewed release notes,
  or credential material.

### Package ownership

- macOS initial install is a signed/notarized DMG containing a self-contained
  `.app`; in-app updates replace its signed `.app.tar.gz` artifact.
- Windows in-app updates apply only to the signed per-user NSIS installation.
  MSI stays with its package/admin channel and presents a manual path.
- Linux in-app updates apply only to an AppImage in a stable user-writable
  location. DEB and RPM stay with their package manager and present a manual
  path.
- RSTorrent currently has no external helper/native host to justify an
  integrated macOS PKG. Adding one would require a separate ownership and
  compatibility decision.

## Scope And Stopping Condition

This tactical owns:

1. exact compatible Tauri updater/process dependencies, permissions, embedded
   endpoint/public key, installation-ID plugin setup, and tests;
2. the Tauri-only injected React updater model, schedule/policy/state owners,
   About & updates UI, accessible available-update notice, and unit/component
   tests;
3. version/build identity, changelog, product/server configuration, and a
   repository validator rejecting version, identity, route, key, tag, and
   package-policy drift;
4. an untagged source/build path that needs no release credentials and a
   `desktop-v*` tagged path that fails early when applicable updater, macOS, or
   Windows credentials are incomplete;
5. draft-only multi-platform packaging for macOS arm64/x86_64, Windows x86_64,
   and Linux x86_64/arm64, including updater artifacts, ordinary installers,
   signature/notarization checks, exact URLs, release checksums, and one final
   validation owner allowed to publish;
6. product-owned update-server configuration compatible with the shared
   service and a production-route preflight that does not change route meaning
   for any existing client;
7. least-privileged install/uninstall and package-channel behavior; and
8. exact older-to-newer installed testbed evidence on macOS arm64, Windows
   x86_64, and Linux x86_64/arm64, with version/build/relaunch and cleanup
   recorded; macOS x86_64 remains an explicit maintained package/route whose
   installed test is deliberately omitted by maintainer direction.

The tactical stops only when:

- versions, identifier, changelog, tag, endpoint, public key, product config,
  draft release, assets, updater JSON, and checksums agree;
- missing tagged credentials fail before a release can become public while
  ordinary pull-request/source checks remain credential-free;
- both macOS architecture apps/DMGs are Developer ID signed, notarized, and
  stapled; Windows NSIS/MSI are Authenticode-signed by the expected publisher;
  and Linux AppImage/DEB/RPM artifacts exist for both intended architectures;
- `latest.json` contains valid signatures and exact same-release URLs for
  `darwin-aarch64`, `darwin-x86_64`, `windows-x86_64`, `linux-x86_64`, and
  `linux-aarch64`;
- browser/demo builds contain no updater behavior, while the Tauri product
  passes the full visible state, schedule, timeout, deduplication, package
  policy, error, progress, install, and relaunch tests;
- an older public signed installer on each planned installed-testbed target
  checks through the production route, installs, relaunches, and reports the
  exact new version and frontend/native build identity, with the deliberate
  macOS x86_64 omission recorded; and
- privacy/support docs, release checklist, focused topics, and a versioned
  acceptance record contain the actual evidence and known limits.

## Non-Goals

- automatic installation or relaunch without an explicit user action;
- delta updates, background service updates, privileged helpers, or a generic
  reusable RSTorrent release library;
- Android Play or iOS TestFlight/App Store update behavior;
- MSI/DEB/RPM self-replacement, Linux repository hosting, Homebrew, Flatpak,
  Snap, or other package channels;
- browser extension/native-host packaging, legacy JSTorrent migration, or
  production remote access;
- analytics beyond the disclosed random updater installation ID;
- publishing a release merely because automation generated artifacts; or
- claiming an update works from same-version, local-server, source-only, or
  metadata-only evidence.

## Owner, Task, Cancellation, And Data Flow

```text
desktop product lifetime
  -> installation-ID file owner
  -> updater plugin with embedded endpoint/public key
  -> one deduplicated check/download/install operation
  -> React update state injected only into Tauri
  -> explicit install -> successful replacement -> relaunch

desktop-v* draft workflow
  -> five platform build legs (serialized latest.json writers)
  -> signing/notarization and artifact checks
  -> one finalizer validates the complete draft + checksums
  -> explicit publication boundary

installed old client
  -> product HTTPS route
  -> shared update server + product config
  -> immutable public GitHub release asset
  -> signature verification -> replacement -> new installed client
```

The updater lifetime ends with the desktop application. Scheduled timers are
disposed with React presentation teardown, concurrent checks share one promise,
download/update handles close on dismissal/unmount, and application shutdown
must not leave an updater task or partial product-state write owner alive.

## Resource And Security Bounds

- one UUID file, at most one update handle, at most one active check/install,
  one startup timer, and one periodic timer;
- 20-second check timeout; download size is obtained from signed release
  metadata/artifacts and streamed by Tauri rather than buffered in React;
- release notes are bounded before display/log retention and rendered as text,
  not trusted HTML;
- updater checks never block application-service startup or torrent work;
- automatic error presentation is non-modal and rate-bounded by the schedule;
- secrets enter CI only through environment/secret mechanisms and are removed
  with temporary keychains/files;
- draft release failure cannot fall through into publication; and
- a clean Windows campaign never grants a firewall exception implicitly;
  record the signed candidate's prompt and supported network-scope guidance.

## Implementation Order And Gates

1. **Pure product model.** Port/adapt state, progress, scheduling, package
   policy, and injected-boundary tests from the accepted canary behavior.
2. **Native identity and plugin.** Add atomic installation ID, headers,
   capabilities, process/updater plugins, build identity, and Rust tests while
   keeping browser builds isolated.
3. **Identity/provisioning gate.** Obtain explicit brand/identifier/route
   direction, generate/store the per-app key, register product configuration,
   and embed only reviewed public values.
4. **Presentation.** Add About & updates plus accessible availability/error
   behavior and component tests with a fake client.
5. **Release validation.** Add changelog/version/config checks, credential
   boundaries, draft artifact matrix, signing, notarization, checksums, and
   fail-closed finalizer.
6. **Installed evidence.** Run exact old-to-new production-route campaigns on
   macOS arm64, Windows x86_64, and Linux x86_64/arm64 and record the results
   before any beta readiness claim. Retain macOS x86_64 package/route checks
   while recording its installed campaign as deliberately omitted.

## Reference Record

Planning inspected the accepted canary's exact:

- `contract/desktop-update-v1.md` ownership, scheduling, state, package,
  signing, artifact, and acceptance rules;
- `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, capability permissions,
  and `src-tauri/src/lib.rs` installation-ID/plugin setup;
- `src/updater/{state,policy,schedule,useDesktopUpdater}.ts` and their focused
  tests for state, package policy, timers, deduplication, progress, and
  relaunch;
- `update-server/desktop-canary.json` and `scripts/validate-config.mjs` for
  product-owned server config and release-identity drift checks;
- `.github/workflows/desktop.yml` for credential-free source checks, five build
  legs, draft-only assembly, platform signature validation, exact finalization,
  checksums, and publication ownership; and
- the canary's `docs/evidence/desktop-v0.1.0-to-v0.1.1.md` for its completed
  macOS, Windows, and Linux installed update campaign and explicit untested
  boundaries.

RSTorrent adopts contract behavior, not the canary's UI, sidecar, product ID,
key, endpoint, or release artifacts. AtPiano remains an updater-pending
consumer and is not used as compatibility evidence.

## Validation Matrix

- pure TypeScript state, schedule, package policy, adapter-absence, component,
  progress, error, and concurrency tests;
- Rust installation-ID create/reopen/malformed/atomic-failure tests and Tauri
  plugin/capability build checks;
- browser typecheck/unit/build/Playwright regression with no updater client;
- untagged Tauri build/package matrix without credentials;
- tagged credential-negative controls and protected signed draft workflow;
- config/latest.json/signature/checksum/release-asset validators;
- native package install/uninstall and manual-channel behavior; and
- exact public signed older-to-newer production-route testbed campaigns for
  the four planned installed targets plus the explicit macOS x86_64 omission.

Record exact commands, workflow runs, artifact hashes, package types, target
architectures, signing subjects, notarization/stapling outcomes, update route,
old/new versions and build IDs, visible states, relaunch, cleanup, and every
deliberate omission.

## Implementation Evidence

The 2026-08-23 source slice now includes:

- `com.jstorrent.rstorrent`, the reviewed RSTorrent updater public key and
  `/rstorrent` endpoint, native process/updater plugins, a private atomically
  repaired `cfu-id`, exact build/version/target facts, and focused Rust tests;
- an injected Tauri-only updater controller with five-second startup and
  24-hour checks, timeout, check deduplication, package policy, progress,
  explicit install/relaunch, About & updates presentation, and an accessible
  non-modal availability notice; browser/demo entry points omit the adapter;
- one root changelog, product-owned update-server descriptor, version/identity/
  route/key drift validator, complete-release and checksum validators, and
  focused negative tests;
- a five-leg `desktop-v*` release workflow for notarized macOS arm64/x86_64,
  Authenticode Windows x86_64, and Linux x86_64/arm64 packages. Manual runs
  retain private signed rehearsal artifacts; tagged runs assemble one draft
  and publish only through the complete-release validator; and
- an explicit TLS-backend boundary: the updater uses native TLS while the
  engine pins HTTP trackers to its existing Rustls/platform-verifier client,
  preventing Cargo feature unification from changing tracker authentication.

Local validation passed `cargo fmt --all -- --check`, workspace clippy with
warnings denied, the full workspace test suite, web typecheck, 262 web unit
tests with 2 skips, production build/CSP validation, 33 deterministic
Playwright tests with 12 live tests skipped, release-tool tests, actionlint,
and an unsigned optimized macOS `.app` whose bundle identifier/version are
`com.jstorrent.rstorrent`/`0.1.0`.

Hosted ordinary CI run
[`32627431920`](https://github.com/kzahel/rstorrent/actions/runs/32627431920)
passes all seven Rust/interop, web/E2E, desktop, Android, and iOS jobs at commit
`f34961c1cbd34508e2f62edc68d1c2a321d78767`. Credentialed Desktop Release run
[`32627436936`](https://github.com/kzahel/rstorrent/actions/runs/32627436936)
passes the source gate and all five serialized release legs at that commit:

- macOS arm64 and x86_64 app/DMG output passed Developer ID code-signing,
  Gatekeeper assessment, Apple notarization/stapling, and emitted signed
  `.app.tar.gz` updater archives;
- Windows x86_64 emitted one NSIS and one MSI installer, and both passed
  Authenticode validation against the expected publisher subject;
- native Linux x86_64 and arm64 runners each emitted exactly one AppImage, DEB,
  and RPM plus signed AppImage updater archives; and
- five private Actions artifacts were retained for 14 days, ranging from
  23,643,046 bytes for Windows to 259,452,543 bytes for Linux x86_64.

The product-owned `/rstorrent` descriptor is also deployed through the shared
production update service, whose health and product registration passed on
2026-08-23.

Tagged run
[`32656926123`](https://github.com/kzahel/rstorrent/actions/runs/32656926123)
published `desktop-v0.1.0` from exact green commit
`768d7de3f5fabcdea4bc1619b127247d61df9ef9` only after the source gate, all five
signed legs, complete-draft validator, and checksum finalizer passed. All 13
files named by the public `SHA256SUMS` passed independent download verification;
the public manifest has the five required updater keys, nonempty signatures,
and immutable exact-release URLs. Every production target returned HTTP 204
for current `0.1.0` and HTTP 200 signed metadata for older `0.0.0`.

The public arm64 DMG then passed one bounded installed smoke outside the source
checkout: checksum, Developer ID/Gatekeeper/notarization/stapling, exact bundle
identity/version, `/Applications` launch, 12-second healthy lifetime including
updater initialization, private UUIDv4 creation, and graceful quit. Smoke-only
app/state were moved to Trash. Exact evidence and deliberate limits are in
[`desktop-v0.1.0`](../evidence/desktop-v0.1.0.md).

Credential-free run
[`32660657596`](https://github.com/kzahel/rstorrent/actions/runs/32660657596)
then passed all seven presubmit jobs for the `0.1.1` release commit. Tagged run
[`32661616090`](https://github.com/kzahel/rstorrent/actions/runs/32661616090)
passed the source gate, all five signed legs, and finalizer before publishing
`desktop-v0.1.1` at exact commit
`2a9ab871847893ed809bf042406ab95487b9d645`. All 13 files named by public
`SHA256SUMS`, the 15-key updater manifest, and current/older route probes for
all five default targets passed independently.

An isolated Machine Control/Tart macOS arm64 guest then installed the exact
public `0.1.0` DMG outside the checkout. Its automatic check offered `0.1.1`,
the explicit install action replaced and relaunched the app, and About &
updates reported version `0.1.1` and the exact new commit. Developer ID,
Gatekeeper, stapled notarization, one running relaunched process, a manual
newest-compatible-release check, and byte-equal private mode-`0600`
installation-ID continuity passed. The app quit normally and the receipt-
bound isolated workspace was discarded. Exact evidence and remaining target
gaps are in
[`desktop-v0.1.0-to-v0.1.1`](../evidence/desktop-v0.1.0-to-v0.1.1.md).
Publisher signatures, package breadth, tagged finalization, public lookup, one
installed launch, and the macOS arm64 cross-version update are therefore
proven. A later isolated campaign also proves Linux arm64 replacement and
relaunch against the exact public AppImages. Windows x86_64 proves the NSIS
updater under an automatic-loopback profile but exposes a fresh-default
listener-selection startup blocker. Linux x86_64 remains an external gate;
installed macOS x86_64 testing is deliberately omitted.

Completed Tactical `161` subsequently passes the complete credential-free
matrix and an unsigned installed Windows fresh-profile campaign. Native picker
cancel/select/default, controlled restart, unavailable-root repair under the
same stable ID, and repaired restart pass. First launch displayed Windows
Security listener consent; selecting Cancel left the app and picker usable but
proved no incoming reachability. The first signed package carrying Tacticals
`160`, `161`, and `162`, its exact clean-profile update, firewall-consent
guidance, and installed Linux x86_64 campaign remain this tactical's next
boundary. Tactical `162` adds the selected single-instance/tray lifecycle,
joined shutdown/restart, corrected Windows GUI launch, native Linux arm64
package gate, and installed Windows x86_64/Linux arm64 evidence.

## Escalation Contract

Pure model/UI/tests, product-state file implementation, plugin integration,
config validators, draft-workflow construction, and build-only fixes within
these contracts are authorized. The application identifier and distinct
RSTorrent updater key are frozen/provisioned; production route deployment and
credentialed private rehearsal plus initial `desktop-v0.1.0` publication are
complete. Stop for maintainer direction before changing route meaning,
rotating or recovering a long-lived updater private key, creating or
publishing another release, or mutating external testbeds. Those actions are
required gates, not implied by this tactical.

The next beta-readiness slice after this tactical freezes the remaining
application identities, persistence baseline, changelog, privacy/support
presentation, diagnostics export, and common beta torrent cohort. Mobile store
distribution remains an independent later campaign in
`beta-release-readiness`.
