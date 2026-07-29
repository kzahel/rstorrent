# Product And Engine Direction

Topic: `product-direction`

Status: initial direction and successor vision accepted; bounded large-piece
pipeline proven, with a selective multi-file storage foundation recommended
next.

## Scope

This topic owns why the RSTorrent implementation exists, what initially
distinguishes it from the current JSTorrent implementation, the engine
constraints, the first platform posture, and the decisions that remain
deliberately open. The likely long-term product succession is recorded in
[`../vision.md`](../vision.md).

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

### Independent implementation, possible product successor

RSTorrent is implemented independently from the current JSTorrent engine and
may coexist with it during bring-up. Its likely destination is to graduate into
a new generation of the JSTorrent product once evidence supports replacement,
not necessarily to remain a separate public brand. It does not begin with a
migration or parity obligation.

### First-party engine and clients

The torrent engine is authored in this repository. libtorrent, librqbit, and
other clients are references and test peers, not the runtime engine.

Product clients are also first-party. A platform UI framework or operating
system library is ordinary infrastructure; a generic remote UI controlling a
third-party client would change the product. A future first-party JSTorrent
extension may control this native engine through a deliberately narrow
application boundary.

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
architecture. A future extension control channel carries commands, snapshots,
and events rather than proxying peer sockets, filesystems, or piece payloads.

### Initial platforms

Android/ChromeOS and desktop are the initial product surfaces. Desktop is the
fastest bring-up and diagnostic environment. Android/ChromeOS supplies the
primary product pressure and must receive physical-device validation.

### Bounded implementation tacticals

Substantial implementation begins with a numbered tactical. An automated run
may work deeply within that slice, but it should not silently expand the
feature or platform surface. The next slice begins only after the prior
tactical records its outcome and validation honestly.

### Pure protocol and domain boundaries

Protocol values, codecs, and deterministic state transitions remain independent
from async runtimes, sockets, filesystems, task handles, and platform adapters.
Tokio is the expected initial execution environment, not a dependency that may
leak into lower-layer contracts. Runtime and platform code depend inward on the
pure layers.

Keep an eye out for module and crate boundaries as implementation provides
evidence, especially runtime leakage, poor test seams, and modules accumulating
unrelated responsibilities. Refactor when the benefit is concrete and
proportionate to the active tactical. Speculative abstraction is not a
substitute for evidence.

### Initial implementation boundary

Tactical `000` established the first workspace boundary as two crates:
`rstorrent-protocol` owns bounded parsing, codecs, and deterministic piece
state, while `rstorrent-engine` owns Tokio, TCP, timeouts, and verified output.
An automated architecture test enforces the inward dependency direction.

Tactical `001` kept that boundary while replacing piece-sized resident payload
with reservation-before-request accounting, block-at-a-time unverified
staging writes, and streamed verification. Its controlled 32 MiB
interoperability fixture reached a 256 KiB engine-owned payload high-water and
used a 16 KiB verification buffer. These are component bounds, not an exact
process-RSS promise.

This is an accepted starting shape backed by unit and libtorrent
interoperability evidence, not a promise that two crates are the final engine
layout. Add or split crates only when later ownership, reuse, lifecycle, or
testing evidence justifies it.

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

- Whether `RSTorrent` remains only an incubation name or is used for public
  previews before the implementation graduates into JSTorrent.
- The evidence, compatibility work, and release process required for that
  graduation.
- The public license.
- The first desktop UI approach.
- The Android Rust/Kotlin binding generator and ownership model.
- The exact minimum useful BitTorrent feature set.
- Which JSTorrent fixtures can be reused directly and which should be
  independently recreated.

## Candidate Bring-Up Sequence

This is recommended direction beyond the accepted first slice:

1. Completed:
   [`000-first-verified-piece.md`](../tactical/000-first-verified-piece.md)
   established independently tested bencode, metainfo, peer-wire, one-piece
   state, and a libtorrent-interoperable vertical thread.
2. Completed:
   [`001-bounded-large-piece.md`](../tactical/001-bounded-large-piece.md)
   established block-granular payload accounting, staging, and streamed
   verification independently of piece length.
3. Establish selective multi-file storage with cross-file piece mapping,
   skipped-file part storage, verified publication, and bounded durable-state
   handling before generalizing the explicit-peer CLI.
4. Prove Rust networking, Android foreground lifetime, and SAF-backed bulk I/O
   on a physical Chromebook before assuming the desktop storage seam transfers.
5. Define the application command/snapshot/event boundary from real CLI needs.
6. Add the first Android and desktop product clients.
7. Evaluate product migration, extension control, and JSTorrent brand
   graduation from the proven application contracts.

The platform feasibility probe remains a separate tactical so failure in it
does not distort the protocol vertical slice.

## Recommended Next Work

Draft tactical `002` around a selective multi-file storage foundation rather
than a single-file happy path. Research the pinned libtorrent part-file,
file-priority, cross-file mapping, and materialization behavior first, then
compare the current JSTorrent implementation and Android/SAF constraints.

Use an edge-rich deterministic fixture to force the initial model to represent
wanted/skipped boundary pieces, pieces touching only skipped files, blocks
crossing file boundaries, zero-length and pad files, a final short piece,
part-file reopen and corruption, verified publication, and materializing a
newly wanted file. Reuse tactical `001`'s block reservation and streamed
verification bounds. Keep discovery, general peer policy, and unrelated
product breadth outside the slice.
