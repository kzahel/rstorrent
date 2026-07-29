# Reference Implementations And Provenance

RSTorrent is independently implemented, but it uses specifications and other
clients as sources of behavioral evidence.

## Reference Policy

References may be used to:

- understand public protocol behavior and interoperability expectations;
- identify useful failure cases and adversarial scenarios;
- design independently authored tests and deterministic fixtures;
- compare wire output, state transitions, performance, and resource behavior;
  and
- study architectural tradeoffs without inheriting them automatically.

Before copying source, fixtures, or test data:

1. Identify the precise origin and applicable license.
2. Decide whether reuse is necessary instead of writing an independent test.
3. Record the provenance and any attribution obligations near the imported
   material.
4. Keep the imported surface bounded and reviewable.

Do not copy code between implementations merely because both are open source.
Reading an implementation does not make its internal API or architecture an
RSTorrent requirement.

## Protocol Specifications

The [BitTorrent Enhancement Proposal index](https://www.bittorrent.org/beps/bep_0000.html)
is the normative starting point for protocol capabilities. Each implemented
extension should identify its BEP, accepted behavior, deliberate limitations,
and interoperability evidence.

Where deployed clients disagree with a specification, record the observed
compatibility behavior rather than silently replacing the documented contract.

## JSTorrent

Repository: [kzahel/jstorrent](https://github.com/kzahel/jstorrent)

Typical local checkout: `~/code/jstorrent`

JSTorrent is the closest product reference because it captures the maintainer's
existing torrent behavior and platform lessons. High-value areas include:

- `packages/engine/integration/python/`: libtorrent-backed download, seeding,
  resume, recheck, multi-file, multi-peer, disconnect, encryption, and
  connection-limit scenarios;
- `packages/engine/test/`: peer, piece, scheduler, DHT, tracker, persistence,
  streaming, and protocol edge cases;
- `android/io-core/`: SAF random-access storage, file-descriptor lifetime, and
  Android socket lessons;
- `android/app/`: foreground service, notification, Doze, lifecycle, storage
  root, and Chromebook UX lessons; and
- `docs/topics/` and `docs/contracts/`: current evidence and explicitly
  documented platform failures.

JSTorrent is a behavior and evidence source. Its QuickJS embeddings,
native-host protocol, IO daemon, extension companion topology, TypeScript APIs,
and persistence formats are not migration requirements.

## libtorrent

Project: [libtorrent](https://libtorrent.org/)

Source: [arvidn/libtorrent](https://github.com/arvidn/libtorrent)

Rasterbar libtorrent is the primary external interoperability oracle. It can
seed to RSTorrent, leech from it, create fixtures, enforce encryption modes,
and expose peer/session state for black-box assertions.

Use libtorrent as an independent peer rather than an RSTorrent runtime
dependency. Interoperability tests should verify payload hashes and observable
protocol results, not only that both processes remained alive.

This is distinct from the similarly named libTorrent used by rTorrent.

## rqbit And librqbit

Project: [rqbit](https://github.com/ikatson/rqbit)

Library documentation: [librqbit](https://docs.rs/librqbit/)

rqbit is a BitTorrent client written natively in Rust. `librqbit` is its
reusable engine crate and exposes torrent sessions, torrent and magnet inputs,
statistics, persistence options, storage support, and an
application-oriented API. It is not a Rust wrapper around a C++ engine, and it
is not inherently a REST service; rqbit's server and web API are clients of the
library.

RSTorrent does not adopt librqbit as its engine. It remains useful for:

- Rust and Tokio implementation comparisons;
- behavior and performance comparison;
- identifying mature engine concerns that an early plan omitted;
- cross-client fixture and swarm testing; and
- comparing the shape of application-facing state without copying its API.

If RSTorrent ever imports rqbit source or fixtures, review and record the exact
license and provenance at that time.
