# Product And Engine Direction

Topic: `product-direction`

Status: initial direction accepted; implementation planning not yet started.

## Scope

This topic owns why RSTorrent exists, what distinguishes it from JSTorrent, the
initial product and engine constraints, the first platform posture, and the
decisions that remain deliberately open.

It does not prescribe the internal crate graph, select a UI toolkit, enumerate
the final BitTorrent feature set, or serve as an implementation tactical.

## Motivation

JSTorrent proved that a TypeScript BitTorrent engine can support extension,
desktop, Android, iOS, CLI, and browser-adjacent products. It also accumulated
the cost of making that engine fit environments that cannot directly provide
its required sockets and files.

RSTorrent is a chance to build for current constraints:

- one first-party Rust engine rather than a shared JavaScript engine embedded
  into several native runtimes;
- direct operating-system networking rather than a general socket proxy;
- in-process clients rather than native-host and daemon protocols;
- a deliberately small product before feature breadth;
- Android/ChromeOS behavior designed as a product rather than a companion
  workaround; and
- an implementation campaign whose correctness can be reviewed against a
  familiar domain, public specifications, and mature interoperability oracles.

The reboot is also intended to be fun. It creates a clean place for ambitious
automated implementation runs while keeping each run bounded by reviewable
tacticals and executable acceptance evidence.

## Accepted Initial Decisions

### New product

RSTorrent is independent from JSTorrent. JSTorrent may remain available and
maintained separately; RSTorrent does not begin with a migration or parity
obligation.

### First-party engine and clients

The torrent engine is authored in this repository. libtorrent, librqbit, and
other clients are references and test peers, not the runtime engine.

Product clients are also first-party. A platform UI framework or operating
system library is ordinary infrastructure; a third-party torrent engine or
remote UI controlling another client would change the product.

### Rust owns the hot path

Rust should own peer TCP and UDP, protocol state, hashing, piece scheduling,
session state, and hot-path storage I/O. Platform adapters should provide
capabilities that truly require the host platform.

On Android, Kotlin may own activities, Compose UI, permissions, notifications,
foreground-service lifecycle, and Storage Access Framework document creation.
The preferred storage seam is to give Rust usable file descriptors or another
bulk-I/O capability rather than copying piece payloads through callbacks.

### In-process by default

Desktop and Android clients should normally load the engine into their own
process and communicate through a typed application API. A test driver or later
remote-control feature must not force the product itself into a daemon
architecture.

### Initial platforms

Android/ChromeOS and desktop are the initial product surfaces. Desktop is the
fastest bring-up and diagnostic environment. Android/ChromeOS supplies the
primary product pressure and must receive physical-device validation.

### Bounded implementation tacticals

Substantial implementation begins with a numbered tactical. An automated run
may work deeply within that slice, but it should not silently expand the
feature or platform surface. The next slice begins only after the prior
tactical records its outcome and validation honestly.

## Initial Non-Goals

- Chrome extension or Chrome native-messaging integration.
- Android companion HTTP/WebSocket service.
- A generic socket or filesystem daemon.
- iOS during initial bring-up.
- Search plugins, streaming playback, or remote administration in the first
  useful client.
- Exact JSTorrent API, UI, persistence, or feature parity.
- A literal all-Rust UI requirement. Rust engine ownership and first-party
  clients are the important constraints.

## Open Decisions

- Whether `RSTorrent` remains the public product name; it is close to the
  existing rTorrent name.
- The public license.
- The initial crate and workspace layout.
- The first desktop UI approach.
- The Android Rust/Kotlin binding generator and ownership model.
- The exact minimum useful BitTorrent feature set.
- Whether the first tactical should prove platform boundaries or the smallest
  libtorrent-backed download.
- Which JSTorrent fixtures can be reused directly and which should be
  independently recreated.

## Candidate Bring-Up Sequence

This is recommended direction, not yet an implementation plan:

1. Prove Rust TCP/UDP and random-access local storage on desktop.
2. Prove Rust networking, Android foreground lifetime, and SAF-backed bulk I/O
   on a physical Chromebook.
3. Establish an independently tested bencode, metainfo, and peer-wire layer.
4. Download and hash-verify a small single-file torrent from an explicitly
   supplied libtorrent peer.
5. Turn that vertical thread into a usable CLI before adding trackers,
   metadata exchange, DHT, multi-file storage, resume, and seeding.
6. Define the application command/snapshot/event boundary from real CLI needs.
7. Add the first Android and desktop product clients.

The platform feasibility probe and protocol vertical slice may become separate
tacticals so failure in one does not distort the other.

## Recommended Next Work

Use the next session to refine this topic and create
`docs/tactical/000-<bounded-slice>.md`. That tactical should select one
falsifiable bring-up result, state explicit non-goals, and identify the exact
libtorrent or physical-device evidence required before any broad scaffold is
generated.
