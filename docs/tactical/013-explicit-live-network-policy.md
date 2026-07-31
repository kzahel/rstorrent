# Tactical 013: Explicit Live Network Policy

Status: completed 2026-07-31.

## Motivation And Outcome

The first desktop run with the public Big Buck Bunny magnet exposed a
bring-up restriction as product behavior. RSTorrent's current peer runtime
silently discards every non-loopback peer hint, tracker address, and compact
tracker result. That restriction was deliberate while the engine was only a
controlled interoperability diagnostic, but desktop and Android now construct
the same runtime without any way to select product networking.

The restriction is also encoded as scattered `is_loopback` branches and
loopback-specific errors rather than one explicit policy. Removing those
branches without replacing the safety boundary would allow deterministic
harnesses and diagnostic tools to contact arbitrary addresses accidentally.

Complete one bounded product-network slice:

- outbound network access is an explicit runtime policy with no implicit
  default;
- desktop and the Android product service select online networking;
- repository diagnostics, the browser gateway, and controlled interoperability
  harnesses explicitly retain loopback-only networking;
- an offline policy performs no DNS or socket work and produces a deliberate
  blocked-progress reason;
- policy is enforced after name resolution, when discovered peers enter the
  registry, and immediately before a peer socket is opened;
- the diagnostic whole-download deadline is replaced by bounded peer connect,
  handshake, message, and write deadlines; and
- ordinary shutdown still cancels and joins runtime work rather than using
  network policy as a lifecycle shortcut.

This makes public UDP trackers and peers reachable from the actual clients. It
does not make the current one-peer, one-shot tracker implementation a mature
torrent client.

## Dependencies And References

- [`../engineering-principles.md`](../engineering-principles.md)
- [`../topics/product-direction.md`](../topics/product-direction.md)
- [`../topics/application-control.md`](../topics/application-control.md)
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md)
- [`../topics/peer-lifecycle.md`](../topics/peer-lifecycle.md)
- [`../topics/tracker-discovery.md`](../topics/tracker-discovery.md)
- [`010-peer-registry-magnet-failover.md`](010-peer-registry-magnet-failover.md)
- [`011-one-shot-udp-tracker.md`](011-one-shot-udp-tracker.md)
- [`012-bounded-diagnostics-progress.md`](012-bounded-diagnostics-progress.md)
- [`../test-torrents.md`](../test-torrents.md)
- JSTorrent Android network monitoring and restriction behavior at sibling
  revision `9895410beeed6aff554053769bd006a3fbd373ef`

JSTorrent observes Android connectivity, unmetered capability, and VPN
transport separately from user Wi-Fi-only and VPN-only settings. A product
owner derives `waiting_wifi` or `waiting_vpn`, suspends all engine networking
without changing each torrent's user intent, and resumes only work that was
otherwise active when the prerequisite returns.

RSTorrent retains that useful semantic separation without copying source or
preserving JSTorrent's controller topology:

- the operating-system adapter observes platform facts;
- application policy combines facts with user settings;
- the engine receives an explicit permission to perform network work; and
- torrent desired state remains separate from a temporary session-wide
  network restriction.

This tactical establishes the destination-policy and offline foundations only.
Dynamic Android monitoring and user settings remain a later product slice.

## Scope

### Explicit engine policy

Add one plain engine-owned outbound policy:

- `Offline` denies all outbound DNS and socket work;
- `LoopbackOnly` permits valid IPv4 and IPv6 loopback destinations; and
- `Online` permits every otherwise valid unicast destination the operating
  system can route, including loopback and private/LAN addresses.

The enum has no `Default` implementation. Every engine and application entry
point must make the choice visible. `Online` does not bypass endpoint
validation: zero ports, unspecified addresses, multicast, IPv4 broadcast, and
unscoped IPv6 link-local destinations remain invalid.

The policy belongs to runtime infrastructure, not magnet, tracker-codec, peer
registry, or durable torrent state. Magnets and compact tracker responses
describe untrusted addresses; they do not decide whether this process may use
them.

### Enforcement points

Apply the same policy:

1. before resolving a magnet peer hint or tracker while offline;
2. to every resolved peer-hint and tracker address;
3. to every compact peer returned by a tracker before registry observation;
4. to manual diagnostic peers; and
5. immediately before `TcpStream::connect`, so a future alternate observation
   path cannot bypass the policy.

Rejected addresses produce bounded policy-neutral errors and diagnostics.
Do not log every member of a hostile tracker response; retain only bounded
aggregate or terminal context.

### Product and diagnostic selection

- Tauri desktop selects `Online`.
- The Android product foreground service selects `Online`.
- The loopback WebSocket gateway remains bound to loopback and constructs its
  application service with `LoopbackOnly` unless an explicit test-only
  environment selection requests another policy.
- Engine and session diagnostic CLIs select `LoopbackOnly`.
- Unit, integration, browser, and controlled libtorrent harnesses select
  `LoopbackOnly` explicitly.

The gateway's bind policy and torrent egress policy are independent. Enabling
online torrent sockets must never expose the browser control listener.

### Offline progress foundation

`Offline` prevents work before DNS and yields blocked discovery with a stable
`network_disabled` reason and an `enable_network` action. It is not a torrent
error and does not change the torrent's desired running state.

There is no user-facing toggle in this tactical. The semantic value exists so
a later application command and Android platform-policy owner can suspend and
resume network activity without overloading torrent pause.

### Operation deadlines

Remove the timeout around an entire metadata or content download. A product
torrent may legitimately run for hours or days.

Retain bounded network waits:

- TCP connect has a configurable nonzero deadline;
- sending and receiving the peer handshake each have a configurable nonzero
  deadline;
- decoding the next complete peer message has a configurable nonzero deadline
  across fragmented reads;
- writing one complete peer frame has a configurable nonzero deadline; and
- the existing bounded UDP tracker operation deadline remains.

A successful protocol message begins the next message deadline. Storage,
hashing, and the lifetime of a whole torrent are governed by ownership and
cancellation, not a network timeout.

Timeout failures retain the operation and configured duration. They participate
in the existing peer failure and discovery-exhaustion handling without
becoming durable corruption.

### Lifecycle

Pause retains safe cancellation around restart-critical storage work. Shutdown
retains immediate cancellation, observes task termination, and drops all
owned sockets before returning.

A future session-wide kill switch will need a mutable network owner that:

- prevents new DNS and socket work;
- closes active network resources promptly;
- preserves torrent desired state;
- reports a network prerequisite rather than pause or error; and
- restarts eligible work after policy permits it.

That application-level behavior is not a VPN-grade leak-prevention guarantee.
Interface binding, VPN loss races, already-buffered kernel traffic, proxying,
and platform-specific network selection require their own threat model and
evidence.

## Non-Goals

- Android Wi-Fi-only, metered-network, VPN-only, or connectivity UI.
- Dynamic policy mutation or a user-visible kill-switch command.
- Binding sockets to a selected Android `Network`, VPN, interface, or source
  address.
- DHT, PEX, local discovery, incoming peer listening, NAT traversal, or proxy
  support.
- HTTP, HTTPS, or WebSocket trackers and WebSeeds.
- Tracker scheduling, retransmission, tiers, stopped/completed announces, or a
  nonzero listening port.
- Multiple simultaneous peers, content failover, mature choking, or bandwidth
  policy.
- Public exposure of the diagnostic WebSocket gateway.
- Claiming that Big Buck Bunny or another changing public swarm will always be
  reachable.

## Validation

### Deterministic policy and lifecycle evidence

- policy-table tests cover IPv4 and IPv6 loopback, public, private, scoped and
  unscoped link-local, unspecified, multicast, broadcast, and zero-port
  destinations;
- offline magnet execution performs no resolver or socket operation;
- loopback-only mode rejects documentation-range tracker and peer addresses
  before socket creation;
- online mode accepts the same otherwise valid non-loopback addresses into the
  runtime path;
- a second policy check immediately before peer dialing rejects a prohibited
  record;
- fragmented peer input cannot extend one complete-message deadline
  indefinitely;
- a sequence of timely complete messages may outlive the former whole-download
  timeout;
- connect, handshake read/write, peer-message, and peer-frame timeouts release
  sockets and terminate through the ordinary owner;
- cancellation remains prompt during tracker and peer waits; and
- shutdown joins the active task.

### Existing controlled evidence

Run the workspace format, Clippy, and tests. Retain the loopback scripted and
libtorrent tracker/magnet scenarios, session resume scenarios, generated
contract checks, web tests and build, Tauri build without launch, and Android
native/JVM tests.

Drive the shared web UI through the loopback headless gateway and Android
Compose through an explicitly owned no-window AVD. Do not launch Tauri or
attach to a visible or user-owned emulator.

### Live evidence

Provide an opt-in bounded metadata-only public-swarm probe using `Online`.
Attempt the recorded Big Buck Bunny magnet without writing full content.
Success requires verified metadata with the expected info hash, cancellation
or metadata-only completion, and cleanup of every temporary artifact and
socket owner.

Public tracker or swarm failure is recorded honestly and does not fail the
deterministic suite. A live success is current evidence only; it is not a
permanent support claim.

## Stopping Condition

Stop when desktop and Android product configurations explicitly use `Online`,
controlled tools explicitly use `LoopbackOnly`, offline mode is observable
without network work, no whole-download deadline remains, all network waits
and owners remain bounded, deterministic and headless gates pass, and the
living topics record both the new public-egress capability and its remaining
one-peer/one-shot limits.

The next peer-focused tactical remains a bounded live-peer set with explicit
request ownership and content failover. Android dynamic network prerequisites
can proceed separately on top of the policy and progress seams established
here.

## Execution Record

### Implemented policy and ownership

`rstorrent-engine` now owns `NetworkPolicy` and `NetworkConfig`. The enum has
no default. Every engine download configuration and application service
constructor must supply `Offline`, `LoopbackOnly`, or `Online` plus nonzero
peer connect and I/O deadlines.

One endpoint validator rejects zero ports, unspecified and multicast
addresses, IPv4 broadcast, and unscoped IPv6 link-local addresses. Policy is
then enforced independently:

- before magnet or tracker DNS when offline;
- on resolved peer hints and tracker addresses;
- on compact tracker observations before they enter the peer registry;
- on manual peer observations; and
- immediately before each TCP dial.

Desktop and the Android foreground product service select `Online`, with a
15-second peer-connect and 60-second peer-I/O deadline. Engine and session
diagnostic CLIs select `LoopbackOnly`. The authenticated browser gateway
defaults to `LoopbackOnly`, its harness sets that value explicitly, and a
bounded diagnostic environment setting can choose another policy without
changing the gateway's loopback bind restriction.

`Offline` terminates before resolver or socket work. The application keeps the
torrent in `awaiting_metadata`, leaves its desired-running intent intact, and
publishes blocked discovery with reason `network_disabled` and action
`enable_network`. This tactical deliberately adds no command or platform
monitor that can change the service policy after construction.

The former timeout around a whole metadata or content download is gone. The
peer owner now bounds TCP connect, handshake read and write, one complete
message read across fragmented input, and one complete frame write. Each
successfully decoded message starts a fresh deadline, so a healthy torrent may
run indefinitely. DNS is bounded to 10 seconds and at most 32 returned
addresses are processed. Existing UDP tracker packet and response operations
remain bounded to five seconds. Cancellation and owner teardown remain the
torrent lifetime controls.

### Client and harness changes

Generated TypeScript and UniFFI Kotlin contracts include the new offline
progress reason and action. Both product adapters have tests asserting their
online policy. The no-public-traffic Android diagnostic magnet now uses an
invalid unspecified tracker address, which remains rejected under every
policy without relying on the product client being loopback-only.

The Android diagnostics harness previously issued vertical swipes at
horizontal coordinate 2200. The API 34 AVD is 1080 pixels wide, so the system
ignored every gesture. It now derives both axes from `adb shell wm size` and
passed the profile and category interaction on the owned no-window AVD.

The forced-restart fixture could complete its original 40,000-byte torrent
between two SQLite polls on this Mac. It now uses eight 1 MiB pieces and proves
restart behavior from exact payload accounting instead of requiring Python to
observe a transient database state: every valid claim is retained, the
corrupted claim and missing pieces are redownloaded, and the final SHA-1
matches.

macOS may report a scripted peer close as `ECONNRESET` where Linux commonly
reports EOF. The disconnect-before-storage test now accepts either operating
system representation while retaining the same remote-disconnect and cleanup
assertions.

### Deterministic validation

The following gates passed:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

npm ci --prefix clients/web
npm run generate --prefix clients/web
git diff --exit-code -- clients/web/src/fixtures/reactive-trace.json
npm run typecheck --prefix clients/web
npm test --prefix clients/web
npm run build --prefix clients/web

clients/web/node_modules/.bin/tauri build \
  --config clients/desktop/src-tauri/tauri.conf.json --no-bundle

cargo clippy -p rstorrent-android --all-features -- -D warnings
cargo test -p rstorrent-android --all-features
experiments/android-engine-bootstrap/build.sh
```

The normal engine suite passed 46 tests with the public probe ignored. The
workspace additionally passed 50 protocol, 28 session, six Android, two
gateway, three engine-CLI, two session-CLI, one desktop, one metadata-seed,
and one architecture test plus doc tests. Web validation passed six tests with
one fixture-dependent test skipped; type checking and the production Vite
build passed. The Tauri command produced
`target/release/rstorrent-desktop` without launching it.

Android validation rebuilt API-28 release libraries for x86_64 and
arm64-v8a, regenerated both UniFFI Kotlin packages, passed six Rust tests and
the Compose/JVM unit tests, and assembled:

```text
experiments/android-engine-bootstrap/app/build/outputs/apk/debug/app-debug.apk
```

The Kotlin compiler repeated existing deprecated Android API warnings; it
reported no new error or warning caused by the network configuration.

`python3 scripts/references.py status` was also run. It reported that this Mac
does not have the optional `bittorrent-beps`, `rqbit`, or `libtorrent`
reference checkouts and that the sibling JSTorrent checkout contains unrelated
maintainer changes. The sibling revision needed for this tactical was readable;
none of those external trees or changes was modified.

### Controlled interoperability and presentation

Rasterbar libtorrent `2.0.13.0` passed three consecutive tracker-only magnet
runs. Each run observed one connect and one announce, acquired the exact
26,686-byte two-block metadata dictionary, verified all three content pieces,
and cleaned up.

The independent bidirectional metadata harness passed three consecutive runs
with the same 26,686-byte metadata and 40,000-byte payload. The forced-death
session harness passed three runs with the eight-piece fixture. It killed
after two or three durable pieces, retained one or two valid claims after
corrupting the first piece, uploaded exactly 7,340,032 or 6,291,456 bytes
after restart, matched payload SHA-1
`c995f4a05e42222e94d1133536701d6edd70dbc6`, reopened complete state, and
cleaned up.

Headless Chrome passed both shared-web scenarios:

- the invalid-address magnet rendered
  `blocked/discovery/no_enabled_discovery_source`,
  exposed `discovery_exhausted`, exercised profile and category filters,
  opened no public socket, and cleaned up the gateway, browser, and temporary
  profiles;
- the controlled libtorrent transfer acquired 26,686 metadata bytes, observed
  exact 16,384-byte requested/received/stored counters, rendered all three
  pieces complete, exercised pause/resume, joined gateway shutdown, and
  cleaned up.

The owned `jstorrent-dev` no-window AVD passed on API 34 arm64-v8a:

- the Android diagnostic surface rendered the same blocked reason and event,
  exercised profile/category filters, survived activity recreation and
  backgrounding, joined foreground-service stop, and cleaned the package and
  granted test directory;
- the controlled transfer acquired 26,788 metadata bytes, rendered eight
  pieces and 100 view updates, matched payload SHA-1
  `2c49ff134a7b68f0104e9f82ffea5c760d9a35b9`, exercised pause/resume,
  survived activity recreation and backgrounding, joined foreground-service
  stop, and cleaned up.

Both browser and Android runs produced screenshots that were visually
inspected. They remained temporary validation artifacts and were not added to
the repository. No physical device, visible emulator, Tauri process, desktop
window, or host GUI automation was used.

### Live online probe

The ignored metadata-only Big Buck Bunny probe ran once with `Online`,
15-second connect, and 30-second peer-I/O deadlines. It resolved and entered
the public UDP tracker path instead of returning policy rejection, but the
bounded run failed waiting five seconds for a UDP tracker connect response.
No metadata or content was written. This is negative current swarm/tracker
evidence and positive evidence that the public route is no longer filtered by
the engine's bring-up policy.

## Remaining Boundaries

The product remains a one-live-peer client with one-shot UDP tracker
discovery. It has no tracker retransmission or scheduling, DHT, PEX, incoming
listener, HTTP/WebSocket trackers, WebSeeds, proxy, interface selection, or
multi-peer request ownership. Online policy makes valid public routes
eligible; it does not make those missing mechanisms appear or guarantee a
changing public swarm.

Dynamic network restriction remains application work. Android should later
observe connectivity, metered capability, and VPN transport separately from
user settings, preserve torrent desired state while restricted, cancel active
network owners, and restart eligible work when prerequisites return. A
VPN-grade kill switch additionally requires socket binding, VPN-loss race,
kernel-buffer, proxy, and platform threat-model evidence.
