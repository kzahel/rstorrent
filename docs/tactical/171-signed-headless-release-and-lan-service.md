# Tactical 171: Signed Headless Release And LAN Service

Status: **Accepted and in progress on 2026-08-26.** Explicit maintainer
direction temporarily yields desktop release Tactical
[`158`](158-desktop-signed-packaging-and-updater.md) to this bounded headless
distribution, operator-approved update, and trusted-LAN deployment slice.

Topics: `runtime-configurations-and-headless-deployment`,
`application-connection-architecture`, `remote-access-authentication`,
`client-surfaces`, `product-surfaces-and-migration`,
`beta-release-readiness`

Dependencies: completed configured-service Tactical
[`170`](170-configured-linux-headless-service.md); completed Crostini signed
distribution Tactical
[`169`](169-hosted-crostini-bootstrap-and-release.md); the existing RSTorrent
incubation updater trust root and GitHub Release repository; and the shared
React updater presentation implemented by desktop release Tactical `158`.

## Decision And Desired Outcome

Turn the existing deterministic headless packages into one reviewable signed
release channel without adopting apt, RPM repositories, containers, a
system-wide `/opt` installation, or unattended replacement. One ordinary
Linux user can install an architecture-matched signed package, ask the
installed command or browser UI whether a newer accepted package exists, and
explicitly apply it through Tactical `170`'s health-checked immutable-version
installer and rollback path.

This tactical also accepts one deliberately narrow no-credential deployment
for an operator-controlled trusted IPv4 LAN. It is not called generic `none`:
`lan-none` means every client on the selected LAN can exercise full owner
control. It requires one exact RFC 1918 listener and the matching exact plain
HTTP origin, retains exact Host and Origin enforcement, and reports the
effective exposure in health, logs, status, and the browser UI. Wildcard,
public, loopback, multicast, IPv6, HTTPS-proxy, and forwarded-host
combinations are invalid in this mode.

The first real deployment is the maintainer's current Ubuntu x86_64 machine at
its exact selected LAN address. The existing ordinary-user installation,
profile, configured payload root, user service, and separately enabled user
lingering remain the owners. No firewall, router, DNS, TLS proxy, public
route, or system-wide service is changed implicitly.

## Stable Scenarios

1. **HLU-001 canonical release.** `headless-vX.Y.Z` source produces native
   x86_64 and ARM64 packages plus one strict signed
   `rstorrent-headless-release-v1` manifest. Tag, source commit, product,
   runtime, architecture assets, sizes, hashes, metadata names, and package
   identities agree before one complete draft can publish as non-latest.
2. **HLU-002 verified bootstrap.** The public bootstrap carries only the
   RSTorrent updater public key, downloads bounded HTTPS metadata and the
   selected architecture, verifies signature/size/hash/archive/package
   identity before execution, refuses downgrades, and delegates ownership to
   the package installer without sudo.
3. **HLU-003 operator update check.** The installed CLI and packaged browser
   UI fetch the same bounded signed stable-channel manifest. Automatic browser
   checks are quiet on current/network failure, manual results are visible,
   and an available version exposes its exact release and copyable local
   apply command without installing or restarting automatically.
4. **HLU-004 transactional apply.** A shell-invoked
   `rstorrent-headless update --apply` verifies and safely extracts the
   selected package, rejects wrong architecture/version/identity and unsafe
   archive entries, then invokes the existing installer. Running/enabled
   intent is restored only after authenticated health; failure leaves the
   previous version selected and healthy.
5. **HLU-005 trusted LAN admission.** `lan-none` accepts only an exact
   non-loopback RFC 1918 IPv4 socket and exactly
   `http://<configured-address>:<configured-port>`, with no username or secret
   fields. Wrong Host and HTTP/WebSocket Origin fail; wildcard, public,
   multicast, loopback, IPv6, mismatched origin, and extra credential fields
   fail before profile creation or peer networking.
6. **HLU-006 truthful presentation.** Health and the production React UI say
   that authentication is absent and every LAN client has full control. The
   update UI identifies the headless package, performs the signed check from
   the backend, and directs apply through the stable installed command.
7. **HLU-007 installed host.** The exact locally built x86_64 package installs
   on the current machine, uses a protected configuration and explicit payload
   root, binds the chosen LAN address, survives user-service restart, and is
   reachable through its exact LAN URL with no credential prompt. Existing
   unrelated user units and operator data remain untouched.
8. **HLU-008 architecture honesty.** Both packages build and validate. Native
   x86_64 service evidence is recorded here; ARM64 remains artifact evidence
   until a physical Raspberry Pi or other native ARM64 service passes install,
   reboot, update, storage, transfer, and cleanup.

## Signed Channel Contract

- Headless tags are `headless-v<MAJOR.MINOR.PATCH>` and releases are not the
  repository-wide GitHub `latest`; desktop and Crostini tag families remain
  independent.
- The canonical manifest is the release and update authority. It binds the
  exact repository, tag, version, source commit, runtime, manifest/signature
  names, and x86_64/aarch64 package names, byte lengths, and SHA-256 values.
- The default stable-channel URLs are fixed HTTPS resources below
  `https://rstorrent.com/releases/headless/`. Promoting them is a separate
  reviewed operation after exact public-artifact acceptance. An unsigned
  GitHub API response or redirect never supplies release identity.
- Reuse the existing RSTorrent incubation updater key with strict headless
  product/runtime/tag binding. The private key remains only in the existing
  release-signing secret. No new key or credential is introduced.
- The website bootstrap may pin an exact accepted `headless-v*` release for a
  reproducible first install. Installed update discovery consumes the signed
  stable manifest, not GitHub's repository-wide latest selection.
- Signed metadata can be withheld by a network attacker but cannot authorize
  different package bytes. Versions older than the installed semantic version
  are rejected; same-version repair remains an explicit local-package action.
- A broken promoted release is repaired by a newer release. Published assets
  and tags are not mutated, and stable-channel rollback does not silently
  override client downgrade refusal.

## Resource And Hostile-Input Bounds

- Manifest and detached signature: at most 64 KiB each, strict line/key order,
  ASCII identifiers, numeric final semantic versions, and no unrecognized
  fields.
- Package: at most 128 MiB compressed, 256 MiB expanded, 4,096 archive
  entries, regular files/directories only, bounded paths, no links, devices,
  FIFOs, sockets, traversal, backslashes, duplicate destinations, or writes
  outside one fresh temporary directory.
- Network: HTTPS initial and redirected production URLs only, finite connect
  and request deadlines, bounded streaming reads, at most one in-process
  release check at a time, and no response body or redirect as executable
  authority before signature verification.
- UI: five-second startup check, one 24-hour interval while a browser remains
  open, deduplicated concurrent checks, manual visibility, bounded release
  facts, and no installation identifier for the headless channel.
- LAN: one exact RFC 1918 IPv4 socket, one exact matching HTTP authority, the
  existing gateway connection/message/view/upload bounds, and no service
  discovery or interface following.

## Owner, Task, Cancellation, And Dependency Map

```text
Git tag
  -> native CI package jobs
  -> one manifest/signature finalizer
  -> exact draft asset validation
  -> non-latest publication only after every required job

browser tab
  -> browser-local bounded schedule/state
  -> same-origin gateway update-check route
  -> one headless release checker (serialized network work)
  -> signed stable manifest result only

operator shell
  -> installed update command
  -> bounded metadata/package downloader
  -> fresh safe extraction directory
  -> Tactical 170 installer/systemd owner
  -> existing service health and rollback transaction

systemd user service
  -> rstorrent-headless
  -> prepared exact LAN listener
  -> gateway + one application/profile/engine owner
  -> joined signal shutdown
```

Manifest parsing, version comparison, archive admission, and LAN
configuration validation remain deterministic and independent from HTTP,
systemd, sockets, and the browser. Network checks have no detached task and
are cancelled when their request/CLI runtime ends. The service never spawns an
updater child that must survive its own systemd cgroup shutdown.

## Reference Dossier

- RSTorrent's exact signed-release reference is
  `.github/workflows/crostini-release.yml`,
  `.github/scripts/write-crostini-release-manifest.mjs`,
  `website/public/install-crostini.sh`, and their tests. Reuse their
  pinned-key, strict-manifest, bounded-download, draft-finalization, and
  two-native-architecture lessons under independent headless identities.
- Tactical `170`'s `crates/rstorrent-headless/src/installer.rs`, package
  builder/validator, and fake-manager tests remain the only install/update/
  rollback ownership authority.
- The sibling 200 OK implementation was inspected at
  `~/code/web-server-chrome/docs/tactical/005-in-app-desktop-updater.md` and
  `008-appimage-first-linux-distribution.md`. Adopt its quiet check/manual
  result/explicit-apply presentation, not its writable AppImage replacement
  mechanism.
- GitHub's current immutable-release documentation recommends creating a
  draft, attaching every asset, and then publishing; future repository
  immutability enablement remains a maintainer setting rather than source
  mutation in this tactical.
- `minisign-verify` `0.2.5` is already locked through Tauri, is MIT licensed,
  has no dependencies, and verifies both legacy and prehashed Minisign
  signatures. Direct headless use avoids an external verifier process while
  preserving the existing trust format. `reqwest`, `flate2`, `tar`, `sha2`,
  and `tempfile` are also already present in the lockfile; direct use adds no
  second HTTP, archive, hash, or temporary-file implementation.
- This slice changes product distribution, application hosting, and browser
  presentation rather than BitTorrent protocol, discovery, scheduling,
  storage semantics, or hot-path performance. The pinned libtorrent feature
  oracle is therefore inapplicable.

No reference source, fixture, or test data is copied.

## Staged Implementation

1. Land this decision record and temporarily make Tactical `171` the sole
   **Now** without changing Tactical `158`'s open gates.
2. Add deterministic `lan-none` configuration/gateway admission, exact Host/
   Origin behavior, truthful health facts, and focused runtime tests.
3. Add the strict headless manifest writer, verifier/parser/downloader/archive
   owner, CLI check/apply behavior, website bootstrap, native release workflow,
   changelog, and adversarial tests.
4. Reuse the injected updater presentation for browser-hosted headless builds,
   including the LAN warning and manual-command update action, without adding
   headless facts to the torrent application contract.
5. Run focused Rust, shell, Node, generated-web, package, and local signed-
   fixture gates; then run the proportional workspace baseline.
6. Build and install the exact x86_64 package on the current machine, create
   only the authorized configuration/profile/payload paths, explicitly enable
   the service, and verify exact LAN static/health/HTTP/WebSocket behavior and
   restart.
7. Record actual evidence, restore Tactical `158` as the sole **Now**, and
   leave public tag/release/channel promotion plus native ARM64 service proof
   unclaimed.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure | Manifest/version/LAN matrix, wrong product/tag/runtime/architecture, metadata/order/size/path/archive hostile cases, update UI state/policy tests |
| Scripted runtime | Signed local metadata/package download, signature/package tamper, safe extraction, current/older/newer selection, successful update and rollback through a fake user manager |
| Gateway/web | Exact LAN Host/Origin and no-credential success/rejection, truthful health/access warning, automatic/manual available/current/error update presentation |
| Package | Two byte-identical x86_64 builds, ARM64 construction/ELF validation, exact release allowlist, no enable/linger/sudo drift |
| Repository | Rust format, strict workspace Clippy, workspace tests, generated TypeScript drift, web typecheck/tests/build, shell syntax, Node manifest tests, `git diff --check` |
| Installed host | Exact package/config/root ownership, enabled/active/healthy service, LAN URL static/health/application connection, restart, logs without secrets, retained linger policy |
| Public/ARM64 | Explicitly deferred: no tag, push, release, website deployment, stable-channel promotion, Raspberry Pi mutation, or physical ARM64 support claim |

## Non-Goals And Next Boundary

- apt/RPM repositories, containers, AppImage, `/opt`, system-wide units,
  dedicated service accounts, multi-user roles, or root-owned configuration;
- automatic/unattended installation, maintenance windows, web-triggered
  service replacement, arbitrary rollback selection, or background updater
  daemons;
- wildcard, public, IPv6, VPN/overlay, service-discovered, hostname-selected,
  forwarded-header, or unauthenticated HTTPS-proxy listeners;
- treating a private address as authentication, encryption, host identity, or
  safety from other devices on that network;
- built-in TLS, Basic removal, owner passphrase/E2E authentication, relay,
  extension control, or Internet exposure;
- publishing a tag/release, deploying the website/stable channel, changing
  repository settings, opening a firewall/router, or installing on the
  Raspberry Pi; and
- release notes ingestion from unsigned GitHub metadata, telemetry, accounts,
  or an installation identifier for headless checks.

The next headless slice may add physical Raspberry Pi/native ARM64 service,
reboot/mount/update evidence or a separately authorized system-wide appliance
mode. Owner-authenticated remote access remains with its security topic.

## Escalation Contract

Ordinary naming, module extraction, direct use of the already locked
dependencies above, conservative bound tightening, source-only update URL
plumbing, generated client changes, local fixture networking, current-host
per-user installation/service enablement, and same-boundary bug fixes are
authorized. Stop before changing the updater trust root, tag/release or website
publication, repository settings, public/IPv6/wildcard exposure, system-wide
or root installation, firewall/router mutation, Raspberry Pi/device mutation,
an unattended update policy, a new dependency/credential, or any destructive
operator-data action.
