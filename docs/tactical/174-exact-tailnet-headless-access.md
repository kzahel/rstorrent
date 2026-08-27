# Tactical 174: Exact Tailnet Headless Access

Status: **Complete.** Explicit maintainer direction on 2026-08-27 temporarily
yielded desktop release Tactical `158` to this bounded headless deployment
slice. Tactical `158` has resumed as the sole **Now**.

Topics: `runtime-configurations-and-headless-deployment`,
`remote-access-authentication`, `client-surfaces`, `capability-readiness`

Dependencies: completed configured-service Tactical
[`170`](170-configured-linux-headless-service.md), completed signed/LAN
Tactical [`171`](171-signed-headless-release-and-lan-service.md), the current
machine's existing Tailscale installation and tailnet membership, and the
existing shared React hosted product.

## Motivation And Desired Outcome

The installed headless service is deliberately bound only to
`192.168.1.129:3030`. That is correct on the trusted home LAN but unreachable
when the operator's phone leaves that LAN. The host already participates in a
Tailscale tailnet and already uses Tailscale Serve for unrelated services.

Make the same application owner available through both its exact LAN authority
and one exact tailnet-only HTTPS authority. Do not bind `0.0.0.0`, bind the
application directly to the Tailscale `100.64.0.0/10` address, replace an
existing Serve route, enable Funnel, or start a second application process.

The accepted topology is:

```text
LAN browser ---- HTTP ----> 192.168.1.129:3030 --+
                                                    |-- one RSTorrent owner
tailnet browser -- HTTPS --> Tailscale Serve ------+
                                |
                                +--> 127.0.0.1:3031
```

Tailscale owns WireGuard transport, HTTPS termination, MagicDNS, and tailnet
access policy. RSTorrent owns an exact loopback proxy endpoint, exact Host and
Origin admission for the configured Serve authority, bounded gateway state,
and joined shutdown. Both endpoints expose full owner control without an
additional RSTorrent credential; every identity admitted by the applicable LAN
or tailnet policy is therefore an owner.

## Stable Scenarios

1. **TNA-001 versioned endpoint configuration.** Existing version-1 single-
   endpoint configuration remains accepted without semantic drift. Version 2
   accepts one to four explicit endpoints and rejects duplicate sockets or
   origins, wildcard/multicast/public direct binds, non-loopback proxy binds,
   insecure proxy origins, credential fields, and unknown endpoint kinds.
2. **TNA-002 exact direct LAN endpoint.** A `direct-lan` endpoint remains one
   exact non-loopback RFC 1918 IPv4 socket whose plain HTTP origin is exactly
   that socket.
3. **TNA-003 exact Tailscale Serve endpoint.** A `tailscale-serve` endpoint
   binds one exact loopback socket and accepts one exact HTTPS `*.ts.net`
   origin. It cannot be reached directly from the LAN and does not trust
   forwarding headers for authority selection.
4. **TNA-004 one application owner.** All listeners attach to one application
   service, storage/profile owner, updater, and cancellation tree. Failure to
   bind any configured endpoint prevents application startup; shutdown joins
   every gateway before the application owner terminates.
5. **TNA-005 endpoint-local admission.** Each gateway accepts only its own
   configured Host and Origin for HTTP, WebSocket, update, and media requests.
   A Host or Origin valid on the other endpoint is rejected.
6. **TNA-006 endpoint-correct media.** A capability created through either
   endpoint returns a URL at that request's exact origin while retaining one
   bounded capability registry and the existing Host-checked media route.
7. **TNA-007 truthful presentation.** The hosted application reports
   credential-free trusted-network access, explains once per browser origin
   that every reachable device has full owner control, and retains the compact
   `No auth` status after dismissal.
8. **TNA-008 installed tailnet service.** A new local headless package installs
   transactionally before configuration migration. The old version-1 config is
   retained as an owner-only recovery copy, both exact endpoint health probes
   pass, existing LAN access remains healthy, and an unused dedicated
   Tailscale Serve HTTPS port reaches the loopback endpoint. Existing Serve
   routes remain unchanged.

## Owner, Task, And Dependency Map

- The systemd user unit remains the sole process owner and starts one
  `rstorrent-headless` process.
- `run_installed_service` validates and binds every endpoint before creating
  the profile or opening the application service.
- Each prepared gateway owns one exact TCP listener, its endpoint-local
  Host/Origin/CORS state, connection limit, WebSocket registry, and gateway
  cancellation token.
- All gateways share one `ApplicationService`, media capability registry,
  storage roots, session network, and updater provider.
- The service shutdown token is observed by every gateway. Any gateway serve
  failure ends the aggregate serve future; all remaining listener futures are
  dropped before the shared application receives joined shutdown.
- Tailscale Serve is an operator-owned host integration outside the package.
  It proxies only to the exact loopback endpoint and obeys existing tailnet
  policy; the package does not install Tailscale or edit ACLs.

Dependency direction remains configuration -> gateway endpoint adapters ->
shared application service. No Tailscale type, daemon API, identity header, or
process handle enters the application contract or engine.

## Resource And Security Bounds

- At most four configured endpoints and the existing per-gateway connection
  cap are allowed. Endpoint strings remain within the existing configuration
  and origin limits.
- No wildcard listener, dynamic interface enumeration, forwarded Host,
  forwarded Origin, `X-Forwarded-*` trust, Funnel, public route, or port
  forwarding is accepted.
- Tailscale Serve identity headers are not interpreted as application
  credentials in this slice. Tailnet ACL admission is the sole tailnet caller
  boundary, and every admitted caller receives full owner authority.
- The loopback endpoint is not a generic reverse-proxy trust contract. Its
  accepted public authority is exact and its endpoint kind is explicitly
  Tailscale-specific.
- This changes application hosting and product presentation, not BitTorrent
  protocol, scheduling, storage, discovery, or hot-path data movement. The
  libtorrent feature oracle is inapplicable.

## Validation

- Deterministic version-1 compatibility and hostile version-2 endpoint parser
  tests.
- Gateway validation and runtime tests for exact direct/proxy Host and Origin,
  cross-endpoint rejection, endpoint-correct media URLs, all-or-nothing bind,
  and joined multi-listener shutdown.
- Focused React integration tests for the trusted-network access mode and
  per-origin notice persistence.
- Standard Rust format, strict Clippy, workspace tests, web typecheck/tests,
  production build/CSP, and package validation in proportion to touched code.
- Two byte-identical native x86_64 packages and transactional installation.
- Installed health/static/WebSocket checks through both exact authorities,
  retained LAN admission, unchanged pre-existing Serve routes, and a
  phone-sized hosted-browser smoke through the tailnet HTTPS URL.

## Non-Goals

- Binding all interfaces or binding RSTorrent directly to the Tailscale IPv4
  or IPv6 address.
- Tailscale Funnel, public Internet access, router changes, subnet routing,
  exit-node policy, tailnet ACL edits, or Tailscale installation/upgrades.
- Treating a tailnet as end-to-end RSTorrent owner authentication, adding
  roles, consuming Tailscale identity headers, or implementing the future
  owner-remote protocol.
- Multiple application processes, profile sharing between processes, per-
  endpoint application state, per-endpoint roles, or multiple storage owners.
- Generic reverse-proxy discovery, forwarded headers, arbitrary proxy brands,
  custom domains, built-in TLS, or automatic certificate management.
- Publishing a tag/release/stable manifest, deploying a website update, or
  installing on the Raspberry Pi.

## References

- [Tailscale Serve](https://tailscale.com/docs/features/tailscale-serve)
- [Tailscale `serve` command](https://tailscale.com/docs/reference/tailscale-cli/serve)
- [`../topics/runtime-configurations-and-headless-deployment.md`](../topics/runtime-configurations-and-headless-deployment.md)
- [`../topics/remote-access-authentication.md`](../topics/remote-access-authentication.md)
- [`171-signed-headless-release-and-lan-service.md`](171-signed-headless-release-and-lan-service.md)

## Implemented Result

Commits `41a303c` and `3d0d97c` add package/configuration version `0.1.1`,
strict version-2 multi-endpoint configuration, one shared application owner,
endpoint-local gateway policy, endpoint-correct media capabilities, and the
trusted-network React presentation. Commit `726c18b` also repairs a boundary
bug exposed by the live campaign: WebSocket media calls now accept the
application contract's canonical `t1-` plus 32 lowercase hexadecimal torrent
ID rather than a retired 40-hex shape.

Version-1 configurations retain their existing single-endpoint semantics.
Version 2 accepts one through four explicit endpoints only for the
credential-free trusted-network posture. `direct-lan` remains an exact RFC
1918 listener/origin pair. `tailscale-serve` requires an exact IPv4 loopback
listener plus an exact HTTPS `*.ts.net` public origin. All configured sockets
bind before the profile/application opens, and every prepared gateway then
attaches to the same application and media registry. Dropping or failing the
aggregate serve future cancels every gateway before shared application
shutdown.

The installed current-machine configuration now has exactly these paths:

```text
192.168.1.129:3030                 direct LAN HTTP/WS
127.0.0.1:3031                     loopback proxy backend only
zblinux.<tailnet>.ts.net:8445      Tailscale Serve HTTPS/WSS
```

The exact tailnet suffix remains only in the owner-protected machine
configuration rather than becoming repository inventory. That configuration
and its version-1 recovery copy
`~/.config/rstorrent/headless.toml.v1-before-tailnet` are both mode `0600`.
One systemd user process owns both sockets. Tailscale Serve proxies only the
dedicated HTTPS port to `127.0.0.1:3031`; the three pre-existing Serve routes
remain unchanged, Funnel remains off, and no ACL, firewall, router, DNS, or
unrelated unit was changed.

The React client reports `network_none`, shows the full-owner network warning
once per browser origin, persists dismissal under a distinct versioned key,
and retains the compact `No auth` status. This is intentionally not product
owner authentication: the tailnet access policy decides who can reach the
route, and every admitted identity receives full RSTorrent owner control.

## Validation Evidence

- `cargo fmt --all -- --check`, strict workspace Clippy, and
  `cargo test --workspace` pass. Focused gateway tests pass 43 tests and
  focused headless tests pass 23 tests, including hostile endpoint parsing,
  all-or-nothing bind, one shared owner, endpoint-local authority, and media
  origin rewriting.
- Web typecheck and the production same-origin Vite/CSP build pass. With the
  repository's documented Node 25 web-storage compatibility flag, 44 files
  and 292 tests pass with two opt-in tests skipped. Focused updater and
  trusted-network notice/storage suites pass.
- The native x86_64 archive constructs twice with exact SHA-256
  `5adb37cad0af939c158a733880f97b5f13aba39f63bfccbc29dcc508aa69395f`.
  It is 23,578,636 bytes and validates as 20 files with 69,405,239
  uncompressed bytes. Installer/bootstrap shell tests and syntax checks pass.
- Transactional `0.1.1` installation first preserved the live version-1 LAN
  service. Migration and restart then admitted both endpoints. A final same-
  version repair probed every configured endpoint and restored the service
  only after both health checks passed.
- Status and health agree on product `rstorrent-headless`, version `0.1.1`,
  and trusted-network/no-auth exposure. The unit is enabled and active with
  zero restarts; one PID owns only `192.168.1.129:3030` and
  `127.0.0.1:3031`.
- LAN and tailnet HTTPS static, health, API, and application-WebSocket paths
  pass. A real WebSocket media call through each authority returns the same
  bounded capability at that authority's own exact origin. Wrong Host and
  cross-origin proxy requests return `403`.
- A 456-by-1024 headless Chrome run through the tailnet HTTPS URL opens WSS
  without page errors, renders the installed product, dismisses the network
  notice, and retains that dismissal after reload while keeping `No auth`.
  A physical off-LAN phone retry remains operator acceptance, not claimed
  evidence.

## Stopping Condition

This tactical is complete when the current machine runs one packaged
application owner behind the unchanged exact LAN listener and one exact
loopback Tailscale Serve listener, both authorities pass endpoint-local
Host/Origin/application/media checks, the service and both gateways terminate
as one joined lifetime, deterministic and package gates pass, the old
configuration remains recoverable, no existing Tailscale route is disturbed,
and the living topics record the exact evidence and remaining security limits.
