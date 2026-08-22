# Tactical 158: Desktop Signed Packaging And Updater

Status: **Decision-complete and Later (2026-08-22); implementation not
started.** Maintainer direction selected RSTorrent as the foreseeable public
product name and temporarily prioritized cross-platform presubmit Tactical
`159`. Pure product integration can proceed after that slice; identifier,
route, and per-app key provisioning retain the gates below. No release or
production mutation is implied.

Topics: `beta-release-readiness`, `client-surfaces`,
`product-state-and-feedback`, `product-surfaces-and-migration`,
`client-persistence`

Dependencies: completed Tactical
[`157`](157-beta-release-foundation.md); the maintained Tauri/React product;
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
- The foreseeable public product name is RSTorrent. A later merger into
  JSTorrent is a separate migration campaign and must not change the meaning
  of already released RSTorrent routes or clients silently.
- Before route/key provisioning, maintainer direction must still freeze whether
  the current `org.jstorrent.rstorrent` identifier is retained or replaced by
  a clean RSTorrent identifier.
- The intended product route is
  `https://updates.graehlarts.com/rstorrent/tauri/{{target}}/{{arch}}/{{current_version}}`
  and the server product config uses a stable RSTorrent ID, `/rstorrent` path
  prefix, `kzahel/rstorrent` repository, and `desktop-v` tags.
- Generate one unique Tauri updater key for this application. Commit only the
  public key. The private key/passphrase live in maintainer secret storage and
  repository Actions secrets; they never enter source, logs, or shell
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
8. exact older-to-newer installed testbed evidence on all five updater target
   keys, with version/build/relaunch and cleanup recorded.

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
- an older public signed installer on each supported testbed checks through
  the production route, installs, relaunches, and reports the exact new version
  and frontend/native build identity; and
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
  with temporary keychains/files; and
- draft release failure cannot fall through into publication.

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
   macOS arm64/x86_64, Windows x86_64, and Linux x86_64/arm64 and record the
   results before any beta readiness claim.

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
- `docs/evidence/desktop-v0.1.0-to-v0.1.1.md` for the completed macOS, Windows,
  and Linux installed update campaign and its explicit untested boundaries.

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
  all five updater keys.

Record exact commands, workflow runs, artifact hashes, package types, target
architectures, signing subjects, notarization/stapling outcomes, update route,
old/new versions and build IDs, visible states, relaunch, cleanup, and every
deliberate omission.

## Escalation Contract

Pure model/UI/tests, product-state file implementation, plugin integration,
config validators, draft-workflow construction, and build-only fixes within
these contracts are authorized. Stop for maintainer direction before freezing
the application identifier/route, generating or storing the long-lived updater
private key, changing the production server configuration, using publisher
credentials, creating a tag/release, publishing, or mutating external
testbeds. Those actions are required gates, not implied by this tactical.

The next separate release slice is cross-platform presubmit CI for the whole
workspace and maintained product clients. Mobile store distribution, release
identity/migration, and the common beta torrent cohort remain independent
tacticals in `beta-release-readiness`.
