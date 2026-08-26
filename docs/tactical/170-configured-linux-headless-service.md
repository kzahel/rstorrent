# Tactical 170: Configured Linux Headless Service

Status: **In progress and selected as the sole Now on 2026-08-26.** The strict
configuration and missing-root safety gate is implemented. Desktop
signed-package Tactical
[`158`](158-desktop-signed-packaging-and-updater.md) is paused with its open
Windows and Linux x86_64 evidence preserved and resumes after this bounded
slice.

Topics: `runtime-configurations-and-headless-deployment`,
`product-surfaces-and-migration`, `application-connection-architecture`,
`remote-access-authentication`, `client-surfaces`, `beta-release-readiness`

Dependencies: the existing `rstorrent-gateway serve` application owner and
matching production React bundle; completed authenticated private-host
Tactical [`076`](076-authenticated-private-web-host.md); completed local web
authentication Tacticals [`101`](101-first-run-web-authentication.md) and
[`109`](109-stable-same-origin-web-launch.md); and the per-user owned package,
systemd, and validation techniques proven by completed Crostini Tacticals
[`167`](167-chromeos-crostini-bundled-web-launcher.md) and
[`169`](169-hosted-crostini-bootstrap-and-release.md).

## Decision And Desired Outcome

Make headless Linux a deployable first-party RSTorrent product configuration,
not merely a developer command or a maintainer-specific external deployment.
One ordinary Linux user installs an architecture-matched package, supplies one
strict versioned configuration plus secret-file references, explicitly enables
a systemd user service, and controls the same mature React application from a
browser without Tauri, a graphical session, an extension, or an open browser
on the server.

The first operator-facing remote topology is deliberately the already-proven
private-host boundary:

```text
browser -- HTTPS/WSS --> operator TLS proxy -- private HTTP/WS --> RSTorrent
                              |                                  |
                      certificate and route          exact Origin + Basic
```

The ordinary same-machine form binds loopback behind Caddy, nginx, or another
operator-owned HTTPS terminator. The package does not install or configure the
proxy. It also supports the existing loopback browser-session mode for SSH
forwarding or truly local use. It does not claim passphrase-based owner remote
access, remembered devices, end-to-end record encryption, or relay blindness.

## Stable Scenarios

This tactical makes these product scenarios pass:

1. **HLS-001 clean configured install.** An ordinary Linux user installs one
   package into owned XDG/home paths. Installation neither starts nor enables
   the service, changes lingering, invokes a browser, nor mutates a reverse
   proxy. The command reports the exact next configuration and service steps.
2. **HLS-002 exact listener.** A valid configuration binds exactly its stated
   loopback or explicit unicast address and port. An unavailable address fails
   without falling back to wildcard, another interface, or another port.
3. **HLS-003 local authentication.** Loopback browser-session mode preserves
   the implemented first-run/recovery behavior, exact HTTP loopback origin,
   remembered-browser revocation, and WebSocket termination semantics.
4. **HLS-004 private HTTPS host.** Basic mode accepts one exact external HTTPS
   origin, reads the password from a protected file, serves the matching React
   assets plus application WebSocket through a TLS terminator, and rejects
   missing/wrong credentials, Host, and Origin values.
5. **HLS-005 zero-presentation lifetime.** Closing every browser connection
   destroys or expires only its bounded application views. The backend,
   profile, active download, completed seed registration, and service remain
   alive and later accept a fresh presentation.
6. **HLS-006 idle availability.** With no active download and no presentation,
   the enabled service remains reachable without retaining a synthetic UI
   view or a high-frequency observer.
7. **HLS-007 joined restart.** `systemctl --user restart` delivers graceful
   termination, joins application/network/storage tasks, reopens the exact
   profile and roots, and returns the UI to truthful durable state without a
   second owner or a fresh profile.
8. **HLS-008 missing root safety.** An absent or unavailable configured payload
   root is never recreated accidentally at a missing mount point. The backend
   remains controllable when the existing application availability model can
   represent the root as unavailable; affected torrents do not write to a
   substitute local directory.
9. **HLS-009 repair and update.** Same-version repair and a newer local package
   install atomically replace only owned application files, retain
   configuration/profile/downloads, and never record success with an
   incomplete package or failed service restart.
10. **HLS-010 uninstall preservation.** Uninstall stops and disables the exact
    owned unit and removes application versions, command link, and ownership
    record while preserving the operator configuration, profile, secret file,
    and every download root. This first slice exposes no destructive purge.

## Scope And Stopping Condition

This tactical owns:

1. a Linux-only `rstorrent-headless` adapter and per-user package containing
   that adapter, `rstorrent-gateway`, and the exactly matching production
   `clients/web` asset tree;
2. strict `rstorrent-headless-v1` configuration with an explicit profile,
   one through 32 named path-backed storage roots, exact listener, exact public
   origin, and either local-browser or Basic authentication;
3. fail-closed configuration, path, ownership, permission, bind/origin/auth,
   installed-asset, and build-identity validation before application or peer
   networking starts;
4. one generated systemd user unit, installed disabled, with an explicit
   `[Install]` target, bounded failure restart, no-new-privileges/private-temp
   baseline, and a graceful stop allowance selected in the 30--60 second range
   from measured shutdown evidence;
5. bounded `install`, `status`, and `uninstall` behavior plus explicit printed
   `systemctl --user enable --now`, disable, restart, and logging commands;
6. deterministic host-package construction and validation for native Linux
   x86_64 and ARM64 without a public tag, release, website installer, or
   updater route;
7. an isolated TLS-terminator integration fixture proving authenticated
   static, health, HTTP, and WebSocket behavior through the configured public
   origin without checking credentials into source or arguments; and
8. a real Linux target campaign through `~/code/machine-control` proving
   clean install, service enable/start, exact listener, authenticated remote
   browser control, transfer with every view detached, completed seeding,
   idle reachability, restart, repair/update, uninstall preservation, and
   exact cleanup.

The slice stops when all deterministic source/package gates and the real Linux
campaign pass, the installed service has one process/application/profile
owner, and the owning topics truthfully record which architectures and
deployment topology were actually exercised. A published package is not part
of this stopping condition.

## Fixed Product Contract

The initial package uses:

- adapter command `rstorrent-headless`;
- package/application identity `com.jstorrent.rstorrent.headless`;
- systemd user unit `com.jstorrent.rstorrent.headless.service`;
- default configuration path
  `${XDG_CONFIG_HOME:-$HOME/.config}/rstorrent/headless.toml`;
- default application root
  `${XDG_DATA_HOME:-$HOME/.local/share}/rstorrent-headless`;
- immutable versions beneath `versions/<version>` plus an atomic relative
  `current` link;
- stable command link `$HOME/.local/bin/rstorrent-headless`;
- default profile path below the application root only when the generated
  example is explicitly accepted; and
- no default payload root, listener broadening, service enablement, lingering,
  TLS proxy, DNS, firewall, or public route.

The installed service invokes the stable adapter with only the configuration
path. The adapter locates and validates its immutable sibling gateway and web
asset tree, loads the complete configuration, and becomes the gateway owner
without retaining a supervising child process. Refactoring the existing
gateway executable into a reusable run/configuration boundary or using a
Linux `exec` handoff are both in scope; two long-lived processes are not.

Installation copies one completely validated staging tree before advancing
`current`. It reuses the Crostini installer's real-directory, symlink,
ownership-manifest, bounded-web-tree, and atomic-link lessons under independent
headless names. Shared helpers may be extracted only when doing so leaves
Crostini policy and tests explicit; the new adapter must not acquire X11,
ChromeOS Launcher, extension, `penguin.linux.test`, or static-service behavior.

Normal install and repair do not enable or start the unit. The user explicitly
runs the printed `systemctl --user enable --now` command. Running before login
requires the operator's separately visible lingering or service-manager
policy; the installer never invokes `loginctl enable-linger`. A system-wide
unit and dedicated service account are deferred.

## Configuration V1

The initial operator file is strict TOML with unknown and duplicate fields
rejected. The implementation may add one exact maintained permissively
licensed TOML parser after recording its version/license and focused parsing
tests; it must not write a bespoke parser for quoted strings, arrays, or path
syntax.

The conceptual v1 shape is fixed as:

```toml
version = 1
profile_root = "/home/alice/.local/share/rstorrent-headless/profile"
listen = "127.0.0.1:3030"
public_origin = "https://torrent.example.test"

[[storage_roots]]
id = "downloads"
label = "Downloads"
path = "/srv/media/torrents"

[authentication]
mode = "basic"
username = "owner"
password_file = "/home/alice/.config/rstorrent/basic-password"
```

Exact field invariants are:

- `version` is exactly integer `1`;
- `profile_root`, storage-root paths, and `password_file` are absolute UTF-8
  Linux paths without NUL or line endings;
- one through 32 storage roots have unique valid application root IDs,
  nonempty bounded labels, and distinct paths under the existing application
  storage-root limits;
- the service never creates a missing configured storage root, follows it to
  a different fallback, or treats the profile/release/config directory as a
  payload root;
- `listen` is one concrete `SocketAddr`; wildcard, multicast, port zero,
  hostname, interface-name, and multi-listener forms are rejected in v1;
- `public_origin` is one exact origin with no path, query, fragment, userinfo,
  or wildcard;
- `local-browser` mode requires a loopback listener and the matching exact
  HTTP loopback origin and uses the existing profile-local web-auth store;
- `basic` mode requires one exact HTTPS public origin, the existing bounded
  username, and a password file; it may bind loopback or one explicit unicast
  private address for a separately controlled proxy hop;
- Basic password bytes never enter the TOML file, command arguments,
  environment, unit, generated assets, logs, health output, or application
  frames;
- the configuration and secret are regular nonsymlink files owned by the
  service user and not group/world writable; the password file is not
  group/world readable;
- configuration is read once per process generation; edits take effect only
  after an explicit restart; and
- startup emits one bounded redacted effective-configuration summary with
  paths and listener/origin facts but no credential or torrent content.

The generated example uses placeholders and mode `0600`; it does not invent a
working password, domain, download root, or public listener. The config file is
operator-owned durable state and is preserved by uninstall.

## Listener, Proxy, And Authentication Invariants

- The application gateway remains the only HTTP/WebSocket listener owned by
  this package. The adapter does not add a proxy socket or second control API.
- Listener validation happens before constructing `ApplicationService`, so a
  bad interface/origin/auth combination does not briefly open peer, DHT,
  tracker, media, or application listeners.
- The exact bound address and configured public origin are distinct and
  observable. RSTorrent does not infer authority from `Host`,
  `X-Forwarded-Host`, `X-Forwarded-Proto`, or a reverse proxy's source address.
- Basic authentication remains ahead of static, health, HTTP application, and
  WebSocket routes. Existing constant-time comparison, bounds, and browser
  challenge behavior remain intact.
- The proxy fixture terminates TLS on a temporary exact origin and forwards to
  an exact loopback RSTorrent address. It proves the HTTP and WebSocket upgrade
  paths, not merely `/healthz`.
- Direct plaintext Basic over an ordinary LAN or public network is not a
  documented product topology. A private overlay does not silently bypass
  application authentication.
- `development-none`, bearer automation, Crostini wildcard hosting, built-in
  TLS, multiple listeners, automatic interface selection, proxy-header trust,
  and public Internet exposure remain outside the installed v1 configuration.

## State, Ownership, And Cancellation Map

```text
operator
  -> immutable config + external password file
  -> explicit systemctl enable/start/stop/restart

systemd --user
  -> one rstorrent-headless service generation
       -> validate installed version/assets/config/security
       -> become the gateway/application process
            -> ApplicationService
                 -> one profile + configured roots
                 -> torrent runtimes, listeners, discovery, storage
                 -> completed seed registrations
            -> one GatewayServer
                 -> authenticated HTTP/static routes
                 -> bounded application WebSocket connections
                      -> detachable view sets and leases

SIGINT/SIGTERM or service stop
  -> stop accepting application connections
  -> cancel and join gateway connections/views
  -> cancel and join ApplicationService children
  -> close durable stores
  -> exit before the measured systemd stop bound
```

Systemd owns boot activation and bounded crash restart. The application owns
all engine and durable state. A browser owns only its connection and leased
views. The TLS proxy owns certificates and external routing but no RSTorrent
principal or application state. Package installation owns immutable program
files and links, never profile or payload bytes.

An invalid immutable configuration/package exits with a stable non-restarting
configuration classification used by `RestartPreventExitStatus`. Unexpected
runtime failures use bounded `Restart=on-failure` plus systemd start-rate
limiting. Implementation may select exact exit values and retry
intervals within standard Linux conventions, but configuration errors must not
produce an unbounded restart loop.

## Resource And Hostile-Input Bounds

- Existing gateway connection, frame, upload, response, view, queue, lease,
  credential, origin, and build-ID limits remain unchanged.
- The configuration file is at most 64 KiB and is read once before service
  construction.
- There are 1 through 32 storage-root records under existing ID, label, and
  locator bounds; duplicate IDs and paths fail before mutation.
- The installed web tree retains the Crostini package's 4,096-file and
  128-MiB ceilings, regular-file-only behavior, and bounded recursive copy.
- The version, ownership manifest, unit template, path fields, and package
  archive shape are bounded and reject symlinks, traversal, devices, FIFOs,
  sockets, and unexpected executable entries.
- Status and health output is bounded and never returns the effective Basic
  header, password file contents, profile database contents, torrent names,
  magnets, or storage paths beyond the locally invoked redacted operator
  summary.
- One UI or stalled proxy client cannot retain unbounded calls, views, queued
  output, upload bytes, or shutdown time; existing application-connection
  fairness and cancellation remain authoritative.

## Package, Update, And Removal Contract

The native package builder creates an architecture-specific archive from:

- `rstorrent-headless`;
- `rstorrent-gateway`;
- the production `clients/web/dist` tree built for same-origin live mode;
- the unit template, configuration example, package identity, and `VERSION`;
  and
- a small installer entry point that delegates to the adapter's checked
  ownership implementation.

The validator checks architecture, version equality, exact allowlist,
executable modes, regular-file shape, asset bounds, example configuration, and
unit placeholders. Two builds from one clean source on the same native Linux
architecture must be byte-identical after normalized archive metadata.

A newer package install stops the existing service if present, validates and
stages the complete version, advances `current`, rewrites only the owned unit
and command link, reloads the user service manager, and restores the prior
enabled/running state only after the new generation passes status. A failed
new generation leaves the previous version available and does not print or
record success; automatic network retrieval and rollback UI are deferred.

Uninstall stops and disables the exact owned unit, removes the unit, command
link, immutable versions, current link, and ownership record, and reloads the
user manager. It never removes the TOML file, secret, profile, database,
download roots, or payload files. It refuses paths not proven by its ownership
record. No `--purge` exists in this slice.

## Reference Dossier

The implementation begins by refreshing these exact local references:

- `crates/rstorrent-gateway/src/main.rs` and `src/lib.rs` for the current
  listener/auth/origin/static/application owner, signal handling, and bounds;
- Tactical `076` plus its hosted verification script for Basic-before-all-
  routes and exact HTTPS Origin behavior;
- `crates/rstorrent-crostini/src/main.rs`, `src/installer.rs`, and
  `resources/com.jstorrent.rstorrent.crostini.service.in` for one-process
  `exec`, owned XDG paths, atomic version links, package bounds, service
  restart, preservation, and no-linger behavior;
- `scripts/build-crostini-package.sh` and
  `scripts/validate-crostini-package.sh` for deterministic architecture package
  construction and allowlisting; and
- `~/code/machine-control/README.md`, its target-use claim contract, and the
  selected Linux platform guide before using a real target.

The sibling `web-server-chrome` and YepAnywhere are not source dependencies.
Their already-recorded lessons may inform deployment and remote layering, but
this slice changes no BitTorrent protocol or engine state machine, so the
pinned libtorrent feature oracle is inapplicable.

Before adding a direct TOML dependency, record its exact version, license,
maintenance status, existing lockfile presence, and why using a standard parser
is safer than handwritten configuration syntax. No source or fixture is
copied from a reference.

### Implementation Refresh (2026-08-26)

- The selected parser is `toml 1.1.4+spec-1.1.0`, pinned directly as
  `=1.1.4` with only `std`, `serde`, and `parse`. Its published metadata names
  Rust 1.85, `MIT OR Apache-2.0`, and the active
  `https://github.com/toml-rs/toml` repository. The repository has continuing
  2026 releases and fixes, including parser hardening. This exact crate was
  already present in `Cargo.lock` through the desktop Tauri build graph, so
  making it a headless dependency introduces no second TOML implementation.
- Serde's `deny_unknown_fields` plus the maintained TOML decoder rejects
  unknown and duplicate keys while retaining correct quoted-string, array,
  Unicode, number, and TOML 1.1 syntax behavior. A handwritten parser would
  duplicate that hostile-input surface without product value.
- The focused edge-case checklist is: 64-KiB pre-parse bound; UTF-8 and exact
  version; missing, unknown, duplicate, and wrongly typed fields; one through
  32 roots with bounded application IDs, labels, and locators; duplicate and
  protected paths; wildcard, multicast, port-zero, hostname, and public-IP
  listeners; canonical origin-only URLs; the local/Basic authentication
  matrix; protected regular files with owner/mode/no-follow checks; bounded
  one-line secrets; and summaries that cannot expose username, password-file,
  or password bytes.
- `ApplicationConfig` now has an explicit path-root startup policy. Existing
  clients retain `CreateMissing`; headless selects `PreserveUnavailable`,
  which leaves an absent mount absent and exposes it through the existing
  unavailable-root state. Runtime root availability also refuses a symlink.
- The gateway now exposes a prepared-listener boundary. Gateway and hosted
  authentication validation, web-auth initialization, asset identity, and the
  exact socket bind complete before an `ApplicationService` may attach. The
  gateway CLI uses that sequence too, while its existing bind entry points
  remain source-compatible for tests and other clients.
- Basic hosted mode now requires the exact external-origin authority in
  `Host` before every static, health, HTTP application, media, or WebSocket
  route. It does not accept the private bind address or forwarded headers as
  an alternative. Hosted health may carry an explicit bounded product
  identity; the existing Crostini identity and handoff fields are unchanged.
- `rstorrent-headless` now validates that it is running from the selected
  immutable relative-current release, including the exact package identity,
  version, sibling gateway, regular-file/mode/owner rules, and a bounded
  nonsymlink web tree. It then loads the protected configuration and secret,
  binds the prepared gateway, opens one online application with unavailable
  roots preserved, and joins gateway/application shutdown in the same process.
  Immutable startup failures use configuration exit 78; post-start serving or
  shutdown failures use runtime exit 1.
- Focused runtime evidence covers an occupied exact listener failing before
  profile or payload creation, Basic-authenticated product/build health from a
  complete installed fixture, a missing payload root remaining absent, and a
  cancellation-driven joined shutdown. The isolated proxy and real-systemd
  gates remain open.
- The owned installer now resolves the fixed XDG/home paths, validates an
  architecture-matched package and exact embedded binary identities, stages a
  complete immutable release, advances relative `current` and the stable
  command atomically, writes the generated disabled unit and mode-0600 example,
  and records exact ownership. It preserves prior enabled/running state only
  for an existing installation. A failed running-generation authenticated
  health check restores the former links, files, version, and service state.
  Uninstall requires the matching manifest, stops/disables the exact unit, and
  removes only owned application files while preserving config, example,
  profile, secret, and payload paths. Fake-manager tests cover fresh install,
  same-version running repair, failed-update rollback, and preservation-safe
  removal.
- Native Linux x86_64 package `0.1.0` was built twice from the production
  same-origin web bundle. Both archives were byte-identical at SHA-256
  `a8e3092f9d299e33a5d77a00d02c6407231ad5dc8b3cfe580593d44f984918f8`.
  The validator reports 19 files and 69,018,601 bytes and checks the archive
  allowlist, paths/types/modes, x86_64 ELF identity, exact adapter/gateway
  versions, fixed templates, web bounds, and no enable/linger command. Native
  ARM64 package construction remains open for the real-target gate.
- The isolated proxy fixture extracts that package into an immutable
  versions/current layout, creates an ephemeral random password and protected
  secret/config files, generates a one-day self-signed IP certificate, and
  terminates HTTPS/WSS in a temporary Node proxy that preserves the external
  `Host`. It passes authenticated production static assets, product/build
  health, HTTP API hello, and application WebSocket negotiation; rejects
  missing/wrong Basic, wrong Host, and wrong HTTP/WebSocket Origin; observes
  zero child processes; and joins SIGTERM twice around a restart of the same
  durable profile. The configured missing payload root remains absent in both
  generations, and captured process output contains no password. All temporary
  certificates, secrets, profile, package extraction, sockets, and logs are
  removed by the fixture.

## Staged Implementation

1. **Decision gate:** land this tactical, reconcile the single Now, and retain
   Tactical `158`'s open evidence without running release work concurrently.
2. **Configuration gate:** add pure strict-v1 parsing, bounds, path/security,
   auth/listener/origin matrix, redaction, and invalid-combination tests before
   constructing an application service.
3. **Runtime gate:** introduce the Linux adapter and reusable one-process
   gateway launch boundary; preserve existing CLI, Crostini, development, and
   hosted-private behavior with focused integration tests.
4. **Package gate:** implement owned XDG paths, immutable versions, atomic
   current/command links, unit/config templates, disabled install, update,
   status, uninstall preservation, builder, validator, and temporary-root
   adversarial tests.
5. **Proxy gate:** build the same-origin React assets and exercise exact TLS
   proxy, Basic/static/health/HTTP/WebSocket, wrong auth/origin, signal, and
   restart cases in isolated temporary directories.
6. **Source gate:** run formatting, warning-denied clippy, focused and workspace
   Rust tests, web typecheck/tests/build, package repeatability/validation, and
   Git whitespace checks.
7. **Real Linux gate:** use machine-control discovery, doctor, and an expiring
   claim; install on the selected target without touching unrelated services,
   configure an isolated address/origin/root/secret, and prove HLS-001 through
   HLS-010 with process/socket/service/database/payload evidence.
8. **Closeout gate:** remove temporary proxy, cert, package, logs, fixture
   torrents, and claim; retain only deliberately selected installed state;
   record exact architecture/version/hashes and evidence; reconcile topics and
   return **Now** to Tactical `158`.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure configuration | Valid local and Basic examples; unknown/duplicate/type/size/version errors; path/root/auth/origin/listener combinations; permission and redaction behavior |
| Gateway runtime | Exact bind/failure, local browser auth, Basic before all routes, wrong Host/Origin, matching assets/build, WebSocket semantic call, signal join, missing-root behavior |
| Installer/package | Temporary XDG homes, symlink/traversal/device/unowned-path rejection, disabled fresh install, enable-state preservation, same/new-version repair, failed health rollback, uninstall preservation, two deterministic archives |
| Web | Same-origin production build, typecheck/tests, authenticated browser connection, magnet intake, detach/reattach, truthful restart recovery |
| Real Linux service | One user/unit/process/profile/listener, no GUI/browser dependency, exact configured socket, proxy authentication, active transfer detached, completed seed, idle reachability, joined restart, update, uninstall, cleanup |

The actual target campaign records what the control route proves. A VM can
close the source/package/service contract; a maintainer media server may add
representative mount and daily-use evidence only when its exact target and
deployment mutation are explicitly in scope for the implementation run.

## Non-Goals And Next Boundary

- Tauri windowless startup, tray changes, desktop autostart, or persistent
  extension control.
- Extension remote-host configuration, magnet routing, or multiple-host UX.
- SRP, OPAQUE, device enrollment, resume credentials, host identity, relay,
  rendezvous, public accounts, or a stable public remote protocol.
- Built-in TLS, ACME, bundled proxy, DNS, certificate installation, firewall
  mutation, Tailscale installation, NAT traversal, or public exposure.
- Wildcard, hostname, interface-name, multi-address, automatic LAN/VPN, or
  BEP 45 listener selection.
- System-wide units, dedicated-user creation, containers, Docker, Kubernetes,
  NAS-vendor packages, or multi-user authorization.
- Automatic update downloads, signed public headless releases, website
  bootstrap, or a headless updater channel.
- Ratio/time seeding goals, seed ranking, bandwidth-policy changes, or any
  BitTorrent protocol/engine behavior.
- Remote filesystem browsing, arbitrary path selection, remote media serving,
  or access outside explicitly configured storage roots.
- Destructive profile, configuration, secret, or payload purge.

The next headless slice may add signed distribution/update mechanics or a
system-wide dedicated-service-account mode from deployment evidence. The
separate desktop extension tactical may make the Tauri backend genuinely
windowless. Owner remote authentication remains its own research and security
implementation campaign around the same application connection.

## Escalation Contract

Implementation may choose internal module names, extract bounded installer or
gateway helpers, choose one exact TOML parser after the required review,
select the systemd stop bound within 30--60 seconds from evidence, tighten any
declared limit, and repair defects at the same config/package/lifecycle
boundary without further direction.

Stop for maintainer direction if the selected real target requires a
system-wide service, root-owned installation, container, proxy/DNS/firewall
mutation, public route, credential provisioning outside an isolated fixture,
destructive data action, incompatible persistence change, new public release,
or a materially different authentication/listener product contract. Ordinary
test failures, missing packages in a guest, internal refactoring, and a
temporary target outage do not change the authorized product shape.
