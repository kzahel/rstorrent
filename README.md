# RSTorrent

RSTorrent is a new, first-party BitTorrent client built around a Rust engine.
It is an independent product rather than a source translation or compatibility
layer for JSTorrent.

The project is currently in its planning and bring-up stage. There is no
committed crate layout, UI toolkit, or release target yet.

## Motivation

JSTorrent began as a Chrome App and adapted repeatedly as the platform changed:
Chrome extension, native messaging host, IO daemon, Android companion, native
mobile embedding, and Tauri desktop app. Each adaptation solved a real problem,
but the combined product now carries several runtimes, process boundaries,
bridges, and conformance surfaces.

RSTorrent starts from the product constraints that exist now:

- the torrent engine, networking, hashing, persistence, and scheduling should
  be first-party Rust;
- the engine should normally run in the same process as its client;
- platform code should expose operating-system capabilities rather than proxy
  ordinary torrent sockets through another runtime;
- Android and ChromeOS should be treated as a first-class product surface;
- desktop should provide the fast development and validation surface and become
  a first-class client of the same engine; and
- the project should be enjoyable to evolve experimentally without inheriting
  JSTorrent feature parity as its starting obligation.

It is also a deliberately well-bounded environment for ambitious automated
implementation and review experiments. BitTorrent is a domain the maintainer
already understands, so generated work can be evaluated against protocol
specifications, mature implementations, deterministic fixtures, and known
product behavior instead of by appearance alone.

## Initial Direction

The intended center of gravity is a reusable Rust engine with a small
application-facing command, snapshot, and event API. Platform clients are
authored in this repository and call that API directly:

```text
Android client ─┐
Desktop client ─┼──> application service ──> Rust torrent engine
CLI and tests ──┘
```

An Android client may still need Kotlin for activities, Compose, foreground
services, notifications, permissions, and Storage Access Framework operations.
That is platform integration, not a second torrent engine. Hot-path peer data
and file data should not bounce through a Kotlin socket proxy or serialized
daemon protocol.

Candidate desktop and Android UI technologies remain decisions for later
tacticals. Tauri and Jetpack Compose are useful starting references, but they
are not yet selected contracts.

## Initial Non-Goals

- Reproduce JSTorrent feature or UI parity before establishing a small,
  reliable product.
- Build a Chrome extension, native-messaging host, Android companion server, or
  browser-to-daemon socket proxy.
- Adopt an existing torrent engine as the runtime implementation.
- Support every platform in the first bring-up.
- Preserve JSTorrent's internal APIs, persistence format, or process topology.
- Select a public license before the project has considered that decision
  explicitly.

## References

RSTorrent owns its implementation, but it does not need to rediscover the
problem without evidence:

- [JSTorrent](https://github.com/kzahel/jstorrent) is the product-behavior,
  test-harness, Android/ChromeOS, and historical design reference. On the
  maintainer's machines it is normally available at `~/code/jstorrent`.
- [libtorrent](https://libtorrent.org/) is the primary interoperability oracle
  and a mature reference for protocol behavior.
- [rqbit](https://github.com/ikatson/rqbit) is a native Rust BitTorrent client.
  Its `librqbit` crate is a reusable Rust torrent engine, not a C++ wrapper or a
  REST-only service. RSTorrent will study and test against it where useful, but
  will not use it as the product engine.
- [BitTorrent Enhancement Proposals](https://www.bittorrent.org/beps/bep_0000.html)
  are the normative starting point for supported protocol behavior.

See [docs/references.md](docs/references.md) for the reference-use and
provenance policy.

## Documentation

- [Product and engine direction](docs/topics/product-direction.md) records the
  current decisions, open questions, and next direction.
- [Topics](docs/topics/README.md) hold living truth for continuing concerns.
- [Tacticals](docs/tactical/README.md) hold numbered, bounded implementation
  plans and their execution records.
- [Development](DEVELOPMENT.md) is the maintainer entry point once
  implementation begins.

## License

No public license has been selected. Until one is added, this repository is not
licensed for redistribution or reuse.
