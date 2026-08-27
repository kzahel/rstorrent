# Tactical 174: Exact Tailnet Headless Access

Status: **In progress.** Explicit maintainer direction on 2026-08-27
temporarily yields desktop release Tactical `158` to this bounded headless
deployment slice.

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

## Stopping Condition

This tactical is complete when the current machine runs one packaged
application owner behind the unchanged exact LAN listener and one exact
loopback Tailscale Serve listener, both authorities pass endpoint-local
Host/Origin/application/media checks, the service and both gateways terminate
as one joined lifetime, deterministic and package gates pass, the old
configuration remains recoverable, no existing Tailscale route is disturbed,
and the living topics record the exact evidence and remaining security limits.
