# RSTorrent

RSTorrent is a functional alpha BitTorrent client built around a first-party
Rust engine. Public signed desktop incubation builds exist, but the product is
not yet a supported beta and is not feature-complete.

The current product can perform real v1 downloads from magnet intake through
verified publication, with tracker and DHT discovery, multiple peers, durable
session state, selective multi-file storage, and maintained first-party
desktop, Android, and iOS clients. Exact support claims and their evidence live
in the [feature-completeness scoreboard](docs/topics/capability-readiness.md)
and [protocol support matrix](docs/topics/protocol-support.md).

## Current Status

- **Functional, not feature-complete.** Ordinary supported downloads work, but
  important capabilities and product behavior remain unfinished. The
  [capability readiness record](docs/topics/capability-readiness.md) is the
  authoritative checklist and work queue.
- **Public incubation releases, not a supported beta.** Signed desktop `0.1.0`
  and updater-validation `0.1.1` packages are public. Installed macOS arm64,
  Linux arm64, and bounded Windows updater mechanics have passed, but
  cross-platform clean-machine gates remain. The Windows fresh-profile defect
  in those two packages is repaired on `main`; current unsigned Windows
  x86_64 and Linux arm64 packages also pass the selected single-instance,
  tray/background, updater-action, and joined-Quit lifecycle. The first signed
  package carrying those repairs and its installed update proof remain open.
  Installed external `magnet:`/`.torrent` intake now passes on macOS arm64,
  Windows x86_64 applications, and Linux arm64 without taking over JSTorrent's
  inherited macOS default handler.
  See the
  [beta release ledger](docs/topics/beta-release-readiness.md).
- **Platforms are at different readiness levels.** Desktop/web is the leading
  product and inspection surface. Android is functional with native engine and
  durable storage integration but still has product gaps. ChromeOS deployment
  remains planned rather than released. A minimal JSTorrent Beta Manifest V3
  seed plus a bounded desktop compatibility/launch host now exists, but its
  store identity and installed Chrome smoke are still pending and it is not a
  torrent-control surface. The
  first-party in-process iOS campaign now has simulator, physical-device,
  public-swarm, system-preview, and unsigned/development archive evidence, but
  no TestFlight or App Store distribution. See
  [client and platform readiness](docs/topics/client-surfaces.md).

## What RSTorrent Is

RSTorrent has one reusable Rust engine behind a typed application service:

```text
Desktop client ─┐
Android client ─┼──> application service ──> Rust torrent engine
iOS client ─────┤
CLI and tests ──┘
```

The engine runs in-process in first-party clients and owns peer networking,
discovery, protocol state, hashing, scheduling, persistence, and hot-path file
I/O. Platform code owns operating-system integration such as windows,
activities, lifecycle, notifications, permissions, and Android document
access. RSTorrent is an independently implemented engine, not a wrapper around
libtorrent, librqbit, or a separate torrent daemon.

The desktop product uses a shared React interface hosted by Tauri and provides
Library, Transfers, and detailed Workbench views. Eligible verified files can
be opened through a bounded ephemeral HTTP capability in the browser or the
desktop system opener. Android uses a platform-appropriate Compose interface
over the same engine and application semantics, retaining its native complete-
file open path; presentation parity is not required.

In project terminology, unqualified **UI** or **web UI** means that shared
React product interface, whether it is browser-hosted or embedded by Tauri.
The former direct-DOM proof has been retired. Android presentation is named
explicitly as the **Android**, **Compose**, or **Android UI**; the Astro
`website/` is the project website rather than a product client.

## Intended Product And Deployment

**RSTorrent** is the public product identity for the foreseeable release line,
not merely a temporary beta label. A later, separately planned graduation into
the next generation of **JSTorrent** may ship as an ordinary JSTorrent update:
it would retain JSTorrent's existing public name, desktop application identity,
and updater trust root while replacing the engine and related internals. Any
legacy-state migration will be best effort and scoped when that work begins.
That future direction is not part of the incubation beta and does not change
current RSTorrent package, route, or updater identity implicitly.

Desktop, Android, ChromeOS, and the maintained iOS client use or are intended
to use the same first-party Rust engine. A future JSTorrent browser extension
may attach as a control and presentation surface, while networking, hashing,
scheduling, persistence, and payload I/O remain in the native engine. The
rollout, backend choices, coexistence, and later graduation direction are
recorded in the
[product deployment and graduation plan](docs/topics/product-surfaces-and-migration.md)
and [long-term product vision](docs/vision.md).

## Incubation Compatibility Policy

Public desktop `0.1.0` and `0.1.1` releases now create real installations and
persisted user state, so the former unreleased freedom to discard provisional
contracts has ended for that release line. Changes must account for supported
upgrade paths, including database and state migrations, protocol and API
versioning, coexistence or rollback where appropriate, and safe handling of
older installations. Development-only Android/iOS identities and explicitly
ephemeral test profiles may still be recreated within their documented
boundaries. This compatibility requirement does not relax RSTorrent's
interoperability obligations to external BitTorrent peers and protocols.

## Engineering Character

Keep the Rust engine understandable: prefer explicit state ownership, plain
structs and enums, deterministic protocol logic, bounded handling of untrusted
input, supervised task lifecycles, structured diagnostics, and support claims
backed by executable evidence. Introduce abstraction when it solves a concrete
ownership, dependency, testing, reuse, or measured performance problem.

See the [engine engineering principles](docs/engineering-principles.md) for
the durable Rust and architecture guidance. See [Development](DEVELOPMENT.md)
for toolchain setup, build, test, and launch instructions.

## Documentation

- [Beta release readiness and gap checklist](docs/topics/beta-release-readiness.md)
- [Feature completeness and current queue](docs/topics/capability-readiness.md)
- [Client and platform readiness](docs/topics/client-surfaces.md)
- [Exact protocol support](docs/topics/protocol-support.md)
- [Deployment and later JSTorrent graduation](docs/topics/product-surfaces-and-migration.md)
- [Product vision](docs/vision.md)
- [Project history and original motivation](docs/project-history.md)
- [Reference implementations and provenance](docs/references.md)
- [Living topics](docs/topics/README.md) and
  [bounded implementation tacticals](docs/tactical/README.md)

## License

RSTorrent is licensed under the [MIT License](LICENSE). Third-party components
and adapted source remain under their respective licenses; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
