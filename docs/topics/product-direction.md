# Product And Engine Direction

Topic: `product-direction`

Status: initial direction and successor vision accepted. Android storage
foundations are proven on an AVD, Chromebook ARCVM, Pixel 7a, and Moto X4
internal and removable exFAT storage; the in-process engine bootstrap is
proven on the AVD, Chromebook, Pixel 7a, and Moto.

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

### Generated Kotlin boundary

Use UniFFI as the default Rust/Kotlin binding generator. Expose a narrow typed
control plane of opaque session objects, configuration and snapshot records,
errors, coarse events, and explicit asynchronous lifecycle operations. Peer
payloads, storage buffers, hashing, scheduling, and ordinary socket I/O remain
inside Rust.

Kotlin may extract an integer descriptor from a caller-owned
`ParcelFileDescriptor`; Rust must duplicate it before taking ownership.
Document creation, rename, permission, notification, and lifecycle operations
may cross the boundary at coarse granularity. Batch naturally repeated control
operations and continue to enforce hostile metainfo file-count limits rather
than designing a per-block interface around implausibly large file counts.

Handwritten JNI is a narrow escape hatch for a concrete Android capability
that UniFFI cannot express safely. It is not a parallel application API or a
payload path.

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

Tactical `002` kept protocol layout independent from runtime storage while
adding bounded multi-file metainfo, safe relative paths, binary file
selection, cross-file request mapping, and torrent-wide piece state. The
runtime now stages wanted paths, places skipped boundary ranges in a versioned
compact-slot part file, synthesizes padding zeroes during 16 KiB streamed
hashing, publishes the selected tree only after verification, reopens durable
placement metadata, and materializes verified skipped files.

The controlled five-piece fixture requested 97,232 real bytes in seven
requests under a 32 KiB payload allowance. One skipped-only piece and 3,304
padding bytes were not requested. Two boundary-piece slots survived reopen
and remained allocated because a permanently skipped file still overlapped
them. This proves a bounded selective-storage foundation, not unfinished-piece
resume, arbitrary priority changes, or a production filesystem format.

Tactical `003` exercised that provisional sparse part-file geometry through
direct Rust file-descriptor I/O on an API 34 AVD, the physical Chromebook's
API 33 ARCVM, a physical API 37 Pixel 7a, and a physical API 28 Moto X4.
Three fresh runs on each internal destination preserved a 256 MiB sparse hole
with only 36--40 KiB allocated. Three additional runs through the Moto's
removable exFAT SAF descriptor allocated 268,566,528 bytes for the
268,451,840-byte logical probe and spent 8.6--11.0 seconds in truncate, write,
sync, and read. All profiles verified both markers after close and process
death, proved duplicated-descriptor ownership and observable cancellation,
and supported directory and materialization renames.

The probe also established Android-specific seams worth retaining:
restart-critical URI state must be committed before process termination,
borrowed descriptors are duplicated before native ownership, buffer and
cancellation bounds remain independent of logical piece size, provider
capabilities are explicit, and grants target a user-visible child rather than
the protected Downloads root. The Moto evidence makes sparse allocation an
explicit destination observation rather than a portable disk-space promise.
The two physical devices still do not establish general OEM, cloud, removable
filesystem, or Android-version compatibility.

The pinned libtorrent reference uses the same compact piece-slot part-file
shape without selecting a filesystem-specific fallback. Sparse storage is its
recommended default, full allocation is optional, and allocation latency is
isolated behind disk jobs and queued-byte backpressure. RSTorrent should follow
that proven direction unless broader product evidence justifies divergence.

Tactical `004` packaged the real engine for x86_64 and arm64-v8a Android,
generated a locked UniFFI `0.31.0` control plane, and made a foreground service
the explicit owner of one Rust runtime and task. Three selective-download
cycles passed on an API 34 AVD, the Chromebook's API 33 ARCVM, and an API 28
Moto X4. The AVD and Moto also passed slow-storage backpressure, both
cancellation phases, peer failure, duplicate start, activity recreation, and
pre-existing-artifact preservation.

The Android bootstrap retained the 32 KiB payload high-water while separately
reporting requested, received, and stored bytes. Kotlin carried no piece
payload, Rust opened the peer socket directly, and every terminal result
joined the task and returned the controlled peer to zero connections. This
proves in-process engine packaging and lifecycle through app-private
path-backed storage; it does not yet connect the engine to SAF destinations.

Tactical `005` has implemented that SAF connection. Rust now derives an exact
bounded document plan, synchronously duplicates caller-owned descriptors for
wanted files, the compact part file, an independent reopen handle, and
materializations, and runs the existing selective placement and verification
logic over those owned handles. Native completion is `PREPARED`, not product
success. Kotlin performs only coarse provider renames; after process death a
fresh process reopens every published document and a fixed 16 KiB Rust loop
checks exact length and a native-produced per-file SHA-1 before cleanup.

Three complete publication/restart cycles passed on the API 34 AVD, the
physical Chromebook's API 33 ARCVM, and the physical API 37 Pixel 7a. AVD
evidence also covers slow storage at the 32 KiB payload high-water, peer
failure after a real request, duplicate start, activity recreation, and
cancellation before and after stored progress. The required Moto internal and
removable rows remain unrun because the previously identified Moto X4 was no
longer attached. Tactical `005` therefore remains in progress even though its
main descriptor and publication thread is implemented.

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
3. Completed:
   [`002-selective-multi-file-storage.md`](../tactical/002-selective-multi-file-storage.md)
   established cross-file mapping, skipped-file part storage, verified
   publication, durable reopen, and materialization under the bounded
   block pipeline.
4. Completed:
   [`003-android-storage-feasibility.md`](../tactical/003-android-storage-feasibility.md)
   established fixed-buffer Rust descriptor I/O, sparse-offset behavior,
   persisted SAF access, cancellation, publication capabilities, and cleanup
   on an AVD, Chromebook ARCVM, physical Pixel 7a, and Moto X4 internal and
   removable exFAT storage.
5. Completed:
   [`004-android-engine-bootstrap.md`](../tactical/004-android-engine-bootstrap.md)
   proved actual engine packaging, the UniFFI control plane,
   foreground-service ownership, direct Rust networking, bounded app-private
   storage, cancellation, and failure cleanup on Android.
6. Closed with explicit deferred validation:
   [`005-saf-selective-storage.md`](../tactical/005-saf-selective-storage.md)
   connected the real selective-storage engine to Android SAF capabilities
   without moving payloads through Kotlin. Moto and remaining provider-fault
   evidence is not claimed.
7. In progress:
   [`006-magnet-metadata-peer-hint.md`](../tactical/006-magnet-metadata-peer-hint.md)
   adds bounded v1 magnet parsing and bidirectional BEP 9 metadata exchange,
   then hands verified metadata to the existing content path.
8. Add durable resume and recheck, including persistence of verified magnet
   metadata and source intent.
9. Define the broader application command/snapshot/event boundary from the
   proven lifecycle and persistence evidence.
10. Add the first Android and desktop product clients.
11. Evaluate product migration, extension control, and JSTorrent brand
   graduation from the proven application contracts.

The platform feasibility probe remains a separate tactical so failure in it
does not distort the protocol vertical slice.

## Recommended Next Work

Execute Tactical `006` as a trackerless magnet vertical slice. Given only a
bounded v1 `btih` magnet and loopback `x.pe` hint, negotiate BEP 10, fetch and
verify BEP 9 metadata, parse the raw info dictionary, and continue over the
same peer into the existing bounded selective download path. Also serve
verified metadata to an independent libtorrent magnet client. Keep metadata
allocation under the existing bencode ceiling, retain bounded pre-metadata
peer state, and cover malformed extension handshakes, inconsistent sizes,
invalid pieces, rejects, disconnects, and hash mismatch before adding tracker
or DHT breadth. Durable resume, general peer discovery, v2 metadata, payload
upload, and UI architecture remain separate tacticals.
