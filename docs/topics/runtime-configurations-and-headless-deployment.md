# Runtime Configurations And Headless Deployment

Topic: `runtime-configurations-and-headless-deployment`

Status: Configured Linux headless-service Tactical
[`170`](../tactical/170-configured-linux-headless-service.md) completed on
2026-08-26. The ordinary-user Linux package, strict versioned configuration,
disabled-by-default systemd user unit, one-process application owner,
data-preserving repair/removal, isolated HTTPS/WSS proxy matrix, and real
x86_64 Linux lifecycle/transfer campaign pass. ARM64 package construction and
binary identity pass without a native ARM64 systemd claim. Production
owner-remote authentication, relay delivery, signed public distribution, and
system-wide service ownership do not exist yet.

## Purpose And Scope

RSTorrent should support several useful runtime compositions without turning
them into different torrent engines or allowing a presentation to own durable
download state accidentally. This topic owns the continuing product direction
for:

- visible, hidden, and eventually absent desktop webview operation;
- a first-class headless Linux service with no desktop or browser process
  requirement;
- backend lifetime while zero, one, or several presentations are attached;
- explicit listener, public-origin, authentication, and TLS-termination
  deployment choices;
- the distinction among process availability, UI lifetime, service startup,
  torrent seeding intent, and remote access;
- the relationship between local extension control and a later configured
  remote-host target; and
- packaging and evidence expected before calling a headless configuration a
  supported product surface.

[`product-surfaces-and-migration.md`](product-surfaces-and-migration.md) owns
the broader backend/presentation model and later JSTorrent graduation.
[`application-connection-architecture.md`](application-connection-architecture.md)
owns the typed application protocol and its HTTP, WebSocket, Tauri, and future
relay delivery adapters.
[`remote-access-authentication.md`](remote-access-authentication.md) owns
future owner passphrase login, host and device identity, remembered access,
end-to-end record protection, and relay threat models.
[`incoming-reachability-and-seeding.md`](incoming-reachability-and-seeding.md)
owns peer-listener and upload behavior; durable ratio/time seeding goals remain
separate application policy.

This topic does not select a password protocol, authorize a public relay,
define a stable third-party daemon API, or make a maintainer-configured Basic
deployment equivalent to the future owner-remote product.

## Product Outcome

One native backend owns one application-service instance, profile database,
torrent engine, network owners, storage roots, and background lifecycle. A
presentation attaches to that backend through the shared semantic application
API and may detach without stopping it.

```text
desktop installation                 headless Linux installation
  Tauri shell + tray                    service manager
            \                           /
             application-service owner
               +-- one profile database
               +-- one torrent engine
               +-- peer and discovery networking
               +-- storage roots and payload IO
                          |
          +---------------+----------------+
          |               |                |
     Tauri webview   browser/extension   remote client
```

The two installation columns are separate backend instances with separate
profiles. The diagram shows shared architecture, not a shared database across
machines. Multiple presentations may attach to one backend only through its
bounded application connection. They never share peer sockets, filesystem
handles, piece payloads, or SQLite pages.

Headless Linux is a first-party product host, not a compatibility IO daemon.
It runs the same Rust application service and engine used by the desktop
product, serves or accepts the same mature React presentation, and keeps peer
and file hot paths in-process. It does not require Tauri, a display server, an
open browser, or an extension.

## Intended Runtime Configurations

| Configuration | Backend owner | Presentation | Lifecycle authority |
| --- | --- | --- | --- |
| Desktop, visible | Tauri process | Embedded React webview | Desktop shell and explicit Quit |
| Desktop, background | Same Tauri process | Hidden or detached webview, tray, and later extension | Persisted **Run in Background** policy and tray Quit |
| Desktop, windowless | Same single desktop backend | Extension or remote client; webview created only on demand | Future desktop/extension tactical |
| Linux, headless | Native service process | Backend-served browser UI or remote first-party client | Explicit service-manager configuration |
| ChromeOS Linux | Crostini user service | Backend-served React UI launched through the extension | Implemented on-demand Crostini service policy |
| Remote controller | The selected desktop or headless backend | Browser, installed client, or later extension | Authenticated connection and backend service policy |

Desktop and mobile clients remain in-process by default. Accepting headless
Linux does not require desktop to become a client of a separate daemon, and it
does not move Android or iOS networking and storage out of their first-party
processes.

## Independent Policies

Several choices that are easy to describe as “run in the background” are
independent and must remain separately configurable.

### Backend availability

The backend may remain available even when no presentation is connected and
no torrent is actively downloading. This is useful for remote intake,
inspection, seeding, scheduled discovery, and accepting a later browser or
extension connection. An idle backend does not need to manufacture UI leases
or keep an invisible high-frequency observer active.

### Desktop close behavior

The existing desktop **Run in Background** preference defaults on. Closing the
window hides it while retaining the same process, application service, profile,
engine, webview, subscriptions, and tray. Turning the preference off makes the
next close perform joined shutdown.

That implementation satisfies “the visible window need not remain open,” but
not the complete windowless target. A later desktop tactical must allow startup
without creating a visible webview, attach an extension to the incumbent
backend, and create or restore the Tauri presentation on demand without
starting another application service.

### Service startup and restart

A headless installation separately chooses whether the service starts:

- manually for one foreground or supervised run;
- when the selected user logs in;
- at boot under an explicitly selected service account; or
- under another operator-owned supervisor.

RSTorrent must not silently enable boot startup, user lingering, or crash
restart as a side effect of installing a binary. The installer or deployment
command reports the selected policy, and the service manager owns automatic
restart. RSTorrent still owns graceful signal handling, cancellation, joined
engine shutdown, and durable reopen.

### Seeding intent

Process lifetime is not torrent policy. A backend may stay reachable while all
torrents are paused, and a completed torrent may remain eligible to seed while
no UI exists. Eventually each torrent or the profile may express such goals as
indefinite seeding, a share ratio, elapsed seeding time, or explicit stop.

RSTorrent currently supports durable completed-torrent seeding and manual
pause/removal, but not ratio/time seeding goals. A Linux service baseline must
not hide that gap behind an automatic process-exit rule. Seeding goals belong
to their own bounded application/engine tactical.

### Remote exposure

Listening, transport protection, application authentication, and relay
routing are distinct. Binding a private address does not authenticate a
caller. TLS termination does not decide application authority. A password
challenge does not provide relay-blind end-to-end protection. Product UI and
diagnostics must describe the effective combination rather than compress it
into one “remote enabled” boolean.

## Headless Linux Product Direction

The leading concrete use case is an operator installing RSTorrent on a Linux
media server, choosing its durable profile and download roots, running it
without a graphical session, and opening the complete React UI from another
machine to paste a magnet and manage torrents.

The first supported Linux headless package should provide:

- one versioned native executable plus the exactly matching production React
  assets;
- explicit durable profile and payload-root configuration outside the release
  directory;
- no dependency on Tauri, X11, Wayland, `xdg-open`, or a browser when launched
  with no-open service behavior;
- one documented, inspectable service-manager installation mode with clear
  enable, disable, start, stop, restart, update, uninstall, and data-preserving
  behavior;
- graceful `SIGINT` and `SIGTERM` handling with joined application shutdown;
- bounded logs and useful health/version facts without logging credentials,
  magnets, torrent names, paths, or arbitrary remote input;
- a safe upgrade path that preserves the profile and refuses incompatible
  configuration rather than silently starting a fresh authority; and
- real validation on a representative headless Linux machine, including
  restart, UI detachment, active transfer, completed seeding, and exact
  cleanup evidence.

A system-wide service may run as a dedicated least-privilege account. A
per-user service may use that user’s roots and permissions. The first tactical
must choose and document its supported ownership mode rather than installing
both implicitly. Service-account identity, filesystem permissions, removable
storage, mounts, and boot ordering are deployment correctness, not incidental
systemd details.

## Listener And Origin Contract

Every network listener is an authority boundary and must be explicit. The
initial headless product should preserve these rules:

- default to one IPv4 loopback address and a documented port;
- accept an exact configured IP address and port for a non-loopback listener;
- do not select a LAN, VPN, container, or public interface heuristically;
- treat IPv4 and IPv6 wildcard addresses as broad exposure requiring a
  distinct explicit opt-in and an authentication mode that permits them, if a
  future tactical supports that combination;
- report the actual bound address and effective public origin after startup;
- fail when the requested address cannot bind rather than falling back to a
  broader or different interface;
- keep the browser’s exact public origin separate from the backend bind
  address so a reverse proxy can terminate HTTPS; and
- reject Host or Origin values outside the selected deployment contract.

An interface-name selector is useful only if it resolves deterministically,
reports every admitted address, and fails closed when the interface changes.
The existing exact socket-address configuration is sufficient for the first
headless slice; automatic multi-interface policy and BEP 45 peer-listener
selection remain separate network work.

## Deployment Security Modes

The product should expose a small validated matrix rather than arbitrary flag
combinations that look secure but are not.

| Mode | Intended exposure | Application authentication | Transport posture | Current status |
| --- | --- | --- | --- | --- |
| Local browser | Exact loopback origin | Local-open or remembered browser sessions | Plain loopback HTTP/WS | Implemented |
| Private reverse-proxy host | One explicit unicast backend address | Bounded Basic credential, with password from a secret file | Public HTTPS/WSS terminated by an operator-owned proxy | Implemented maintainer preview |
| Trusted private overlay | Explicit VPN/tailnet address | Still requires an explicit accepted application-auth policy | Overlay encryption or HTTPS | Desired; not productized |
| Owner remote access | Direct or relay-mediated host | Passphrase bootstrap plus eventual remembered-device identity | Authenticated end-to-end records; relay remains opaque | Direction recorded, not implemented |
| Development-none | Ephemeral loopback only | None | Local test traffic only | Implemented development mode |

The immediate operator deployment may place RSTorrent behind Caddy, nginx, or
another HTTPS terminator and retain application-level Basic authentication.
The expected shape is:

```text
browser -- HTTPS/WSS --> reverse proxy -- private HTTP/WS --> RSTorrent
                              |                                |
                         TLS policy                 exact Origin + Basic
```

The RSTorrent listener should normally bind loopback when the proxy runs on
the same machine. If the proxy is separate, the backend binds one explicit
private address and the operator firewall limits that hop. The configured
external `https://` origin remains the browser authority; it is not inferred
from untrusted forwarding headers.

Basic authentication is a full-owner credential and is acceptable only as an
explicit operator-managed private deployment mode. The password stays in a
bounded secret file, not a command argument, URL, generated asset, log, or
application frame. Reverse-proxy authentication may add defense in depth, but
TLS termination or an upstream login does not automatically authorize a
request to RSTorrent unless a later tactical defines an authenticated proxy
identity contract.

RSTorrent does not currently terminate TLS itself. In-process TLS may be added
later if it materially simplifies supported deployments, but it is not
required for the first headless package because explicit bind/origin
separation already supports a small local TLS terminator. Any direct public
listener still requires the future owner-remote threat model; “it uses HTTPS”
is not by itself a production remote-control claim.

## Configuration Contract

The durable configuration surface should make these facts independently
inspectable, whether the first implementation uses a file, environment, CLI,
or a deliberately narrow combination:

- profile root and named storage roots;
- service/runtime mode and whether browser launch is disabled;
- exact listen address and port;
- exact external browser origin;
- authentication mode and secret-file references;
- production web-asset root and build identity;
- network egress policy and existing engine settings; and
- logging destination and bounded verbosity.

Secrets do not belong in process arguments, unit files, checked-in examples,
or generated frontend configuration. Startup validates the complete effective
configuration before opening application or peer listeners, prints a redacted
summary, and fails closed on incompatible bind, origin, authentication, asset,
profile, or storage-root combinations. A service restart must consume the
same durable configuration; an interactive shell environment must not be a
hidden requirement.

Exact option names and precedence belong to the implementing tactical. Avoid
growing parallel CLI, environment, and file vocabularies whose conflict rules
cannot be explained. The current gateway CLI/environment behavior is
substrate, not automatically the final stable operator contract.

Tactical `170` implements that first contract as one strict
`rstorrent-headless-v1` TOML file. It requires an explicit profile, one through
32 named path roots, exact IP address and nonzero port, exact public origin,
and either local-browser or Basic authentication. Basic secrets are read from
a protected owner-only regular file. Unknown or duplicate keys, unsafe
ownership or modes, overlapping protected paths, symlink roots, invalid
listener/origin combinations, and incomplete package identity fail before the
application opens. A missing configured payload mount remains absent and is
reported unavailable rather than being recreated.

## Presentation And Extension Routing

The simplest remote presentation remains a browser tab opened at the selected
headless host and authenticated there. It provides the complete React control
surface without making extension installation a prerequisite for a server.

The desktop extension path initially attaches to the local installed backend
through native bootstrap plus a future persistent local application channel.
A later extension may retain named remote-host configurations and route a
captured magnet to one user-selected default host. That convenience must reuse
the same authenticated application operation as the web UI and preserve these
rules:

- local native messaging cannot silently become remote authority;
- each remote host has its own endpoint or routing identity, authentication,
  compatibility, and revocation state;
- choosing a default host does not merge profiles or torrent identity across
  machines;
- a failed or ambiguous target is shown before submitting the magnet to a
  different backend; and
- magnets, credentials, and resume secrets are not placed in query strings,
  extension logs, or relay-readable routing metadata.

Multiple-host extension UX, host switching, context menus, and magnet-handler
policy remain future product work. They should follow a proven browser remote
connection rather than define its security model.

## Current Substrate And Gaps

The repository now proves these parts of this direction:

- `rstorrent-gateway serve` runs the application service and production React
  UI without Tauri, accepts an exact socket address and browser origin, and can
  suppress browser opening;
- its current authentication choices include loopback browser sessions,
  bounded Basic, bearer automation, and loopback-only development-none;
- credentials can come from bounded secret files, and hosted Basic mode
  requires one exact unicast bind plus an exact HTTPS browser origin;
- the maintainer-operated private host passes authenticated public
  HTTPS/WebSocket, supervised restart, rollback, and durable profile/root
  evidence;
- ChromeOS Linux packages the same gateway and React assets behind an
  on-demand static systemd user service; and
- desktop already defaults to one retained background process behind the tray
  when its visible window closes.
- Tactical `170` packages `rstorrent-headless`, `rstorrent-gateway`, and the
  exact production React assets for one ordinary user. Installation remains
  disabled, never changes lingering, and exposes explicit enable/start/status/
  restart/uninstall commands. Same-version repair preserves running/enabled
  intent and rolls back on failed authenticated readiness; uninstall preserves
  configuration, secrets, profile, and every payload root.
- One x86_64 Ubuntu systemd-user campaign proves exact bind failure, local
  pairing persistence, private HTTPS/WSS proxy control, an 8-MiB 128-piece
  transfer with all views detached, completed re-seeding to pinned
  libtorrent, idle reachability, missing-root reachability across restart,
  joined restart, repair, preservation-safe uninstall, and exact target
  cleanup. x86_64 and ARM64 archives construct twice byte-identically; ARM64
  binaries report their identities under QEMU, without a native ARM64 service-
  lifecycle claim.

The important remaining gaps are:

- signed public headless artifacts, a hosted installer/update route, and
  native ARM64 service-lifecycle evidence;
- representative removable/media-server mount, reboot, suspend, and long-run
  unattended evidence;
- true desktop startup with no created webview and later on-demand recreation;
- persistent extension application control rather than one-shot launch;
- productized private-overlay behavior;
- owner remote authentication, remembered devices, host identity, and direct
  versus relay delivery;
- ratio/time seeding goals and seed admission/ranking policy; and
- release, update, compatibility, and recovery evidence for an unattended
  headless installation.

## Recommended Next Work

Completed Tactical
[`170`](../tactical/170-configured-linux-headless-service.md) supplies the
first bounded Linux headless deployment. A later headless tactical may add a
signed public distribution/update lane or a system-wide dedicated-service-
account mode after representative deployment evidence. It must retain the
strict configuration, bind-before-application, one-process ownership,
data-preserving removal, and explicit startup-policy boundaries established
here.

The desktop windowless/extension-attachment tactical can proceed independently
because it uses the same application-service and presentation-lifecycle
invariants but a different OS shell and local authentication boundary. The
remote-authentication research tactical remains separate from both and may
reuse the headless package once its direct application connection is stable.

## Non-Goals

- A multi-user torrent service, roles, tenant isolation, or arbitrary
  third-party daemon API.
- Sharing one live SQLite profile or storage capability across machines.
- Moving peer networking, hashing, scheduling, or payload IO into the browser,
  extension, reverse proxy, or relay.
- Automatically exposing every interface or treating a random port as
  authentication.
- Treating Basic authentication as relay-blind owner authentication.
- Bundling Caddy, nginx, Tailscale, DNS, certificates, or a hosted relay into
  the first Linux package.
- Solving multi-host extension UX, friend sharing, remote media streaming,
  public accounts, wake-up delivery, or NAT traversal in the headless baseline.
- Coupling backend availability to an open view, active download, or one
  particular seeding-goal policy.
