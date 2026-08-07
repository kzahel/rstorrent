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

For every engine, protocol, discovery, scheduling, storage, or performance
feature, the tactical must inspect the exact pinned libtorrent source **and its
tests** before the design is finalized. Libtorrent is the required feature-
completeness and edge-case oracle: record the exact paths, functions or test
cases studied, the resulting edge-case checklist, behavior RSTorrent adopts,
and intentional differences. This requirement does not authorize copying
source or silently adopting libtorrent's architecture.

## Local Reference Set

RSTorrent keeps reproducible external source checkouts under the gitignored
`reference/` directory and continues to use JSTorrent as a first-party sibling.
The tracked [`reference/pins.toml`](../reference/pins.toml) records exact
external revisions and the sibling branch; the
[`reference map`](../reference/README.md) records why each source exists,
license expectations, and useful distinctions between source-reading and
executable-oracle roles.

Use:

```bash
python3 scripts/references.py sync
python3 scripts/references.py status
```

The sync command refuses to overwrite local changes or divergent repositories.
External checkouts stay at detached exact revisions. The JSTorrent sibling may
fast-forward only when it is clean, on `main`, and its fetched `origin/main`
descends from the locally checked-out commit.

## Protocol Specifications

The [BitTorrent Enhancement Proposal index](https://www.bittorrent.org/beps/bep_0000.html)
is the normative starting point for protocol capabilities. Each implemented
extension should identify its BEP, accepted behavior, deliberate limitations,
and interoperability evidence.

The authoritative sources are available offline after reference sync at:

```text
reference/bittorrent.org/beps/
```

This managed checkout is pinned to the exact upstream revision in
`reference/pins.toml`. Prefer it to JSTorrent's Markdown conversion, which was
generated from a May 2020 snapshot. The original reStructuredText retains
document metadata, history, and each BEP's copyright section.

Where deployed clients disagree with a specification, record the observed
compatibility behavior rather than silently replacing the documented contract.

## License Posture

This inventory was checked against the managed revisions on 2026-07-29. It
describes the reference set; it is not a substitute for checking the precise
file before importing material.

- rqbit and its librqbit crates are Apache-2.0.
- Rasterbar libtorrent's main library is BSD-3-Clause. Its root `LICENSE`
  identifies separately licensed files; notably, its Python binding source is
  Boost Software License 1.0, while its optional `simulation/libsimulator`
  submodule is GPL-3.0.
- JSTorrent is MIT.
- The official BitTorrent BEP repository does not currently state a
  repository-wide license, and individual document statements vary. Cite and
  independently summarize protocol behavior rather than copying BEP prose or
  bundled material without checking the exact document.

Reading these sources and running a reference implementation as a separate
test peer does not make it an RSTorrent product dependency. If source, fixtures,
test data, or a reference binary will be copied, linked, vendored, or
distributed, stop and record the exact origin, revision, file-level license,
required notices, modification status, and reason for inclusion first.

The GPL-3.0 libsimulator submodule is not part of the planned oracle harness and
must not be linked into or distributed with RSTorrent without a separate
license and architecture decision. The similarly named
[libTorrent used by rTorrent](https://github.com/rakshasa/libtorrent) is
GPL-2.0 and is not the managed Rasterbar libtorrent reference.

RSTorrent is licensed under the MIT License. Bundled third-party material and
release-distribution considerations are recorded in the repository root
[`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md). The initial Rust and
Python lockfiles were audited during tactical `000`; the exact inventory at
that point is recorded in
[`tactical/000-first-verified-piece.md`](tactical/000-first-verified-piece.md).

The current dependency graphs include mostly permissive licenses plus some
MPL-2.0 components. These do not change the license of RSTorrent's original
source, but binary distributions must preserve their applicable notices and
license/source-availability obligations. Repeat the audit whenever dependency
graphs change and generate notices from each release's exact resolved Android,
desktop, web, Rust, and Python dependency sets.

## JSTorrent

Repository: [kzahel/jstorrent](https://github.com/kzahel/jstorrent)

Typical local checkout: `~/code/jstorrent`

Managed reference entry: `../jstorrent`, first-party `main`

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

The accepted download-root and add-options behavior is mapped to exact sibling
starting points in [`topics/download-roots.md`](topics/download-roots.md).
That map is a product cheat sheet for future implementation, not permission to
copy JSTorrent's storage manager, host topology, or persistence format.

Tactical `008` adapted the grid sizing, state layering, and color semantics
from JSTorrent's
`android/app/src/main/java/com/jstorrent/app/ui/components/PieceMap.kt` at
commit `0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`. The RSTorrent file and
JSTorrent source have the same author and copyright holder, so no separate
third-party license notice is required. No other JSTorrent source was imported
by that tactical.

## Client Binding References

The initial desktop adapter follows Tauri v2's official
[Rust command](https://v2.tauri.app/develop/calling-rust/) and
[Channel](https://v2.tauri.app/develop/calling-frontend/) APIs. The Android
adapter follows UniFFI's documented
[external and remote type](https://mozilla.github.io/uniffi-rs/latest/types/remote_ext_types.html)
model so application-contract types originate in `rstorrent-session` while
the exported client object remains in `rstorrent-android`. These are SDK/API
references; no documentation sample source was imported.

## TLS Platform Trust References

Tactical
[`098`](tactical/098-authenticated-https-tracker-platform-trust.md) audits the
locked reqwest/rustls platform-trust path rather than introducing a separate
TLS implementation. The exact resolved components are reqwest `0.13.4`,
rustls `0.23.43`, `rustls-platform-verifier` `0.7.0`, and
`rustls-platform-verifier-android` `0.1.1`. The tactical records the inspected
reqwest client builder, verifier platform backends and tests, Android
initialization API, version-matched Maven artifact, and pinned libtorrent and
JSTorrent comparison paths.

Reqwest, rcgen, and both platform-verifier crates are MIT OR Apache-2.0;
rustls is Apache-2.0 OR ISC OR MIT. The Android AAR is the support artifact
published by the already locked verifier dependency and is resolved from its
Cargo-provided on-disk Maven repository at build time; it is not copied into
this repository. Rcgen creates independently authored in-process certificate
fixtures. The external Python interoperability harness invokes the local
OpenSSL executable only to generate temporary certificates; OpenSSL is not an
RSTorrent runtime dependency.

No reference source, certificate, private key, CA store, test fixture, or
tracker credential was copied. Controlled roots exist only inside temporary
test directories or a feature-gated test process and never enter production
construction or a platform trust store.

## Application View And Web Client References

The accepted application-view direction in
[`topics/application-view-api.md`](topics/application-view-api.md) uses these
official references as design evidence:

- [qBittorrent WebUI API](https://github.com/qbittorrent/qBittorrent/wiki/WebUI-API-%28qBittorrent-5.0%29):
  its `sync/maindata` resource demonstrates a practical cursor, full reset,
  keyed upsert, and removed-ID polling model. RSTorrent does not adopt its
  complete endpoint or payload shape.
- [Transmission RPC specification](https://github.com/transmission/transmission/blob/main/docs/rpc-spec.md):
  requested fields, object results, and explicit recently removed IDs inform
  projection and collection-diff choices. RSTorrent does not adopt its
  positional table encoding or arbitrary field query as the initial contract.
- [`ts-rs` documentation](https://docs.rs/ts-rs/latest/ts_rs/): Serde-compatible
  TypeScript generation remains the initial declaration mechanism.
- [`schemars` documentation](https://docs.rs/schemars/latest/schemars/): JSON
  Schema generation from the same Rust DTOs removes duplicated handwritten
  structural type lists while separate validators retain semantic bounds.
- [Ajv JSON Schema documentation](https://ajv.js.org/): the pinned `8.20.0`
  client validator compiles the generated draft 2020-12 definitions while
  small handwritten checks retain canonical decimals, ranges, and negotiated
  resource invariants.
- [Zustand documentation](https://github.com/pmndrs/zustand): its vanilla
  store, React selector, shallow-selection, and direct subscription APIs fit a
  per-application materialized view store without making React own transport
  tasks.

These are API and architectural references. No source, fixtures, or assets are
imported by the design documentation. Binary WebSocket frames and Tauri raw
responses or Channels remain codec and delivery capabilities, not a reason to
make a second semantic API.

Tactical
[`101`](tactical/101-first-run-web-authentication.md) also uses the official
[IETF cookie draft 6265bis-22](https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis/),
[WHATWG Fetch Living Standard](https://fetch.spec.whatwg.org/), and
[WHATWG WebSockets Living Standard](https://websockets.spec.whatwg.org/) for
host-only/HttpOnly/SameSite/Secure cookie behavior, credentialed CORS, Origin,
and browser WebSocket cookie mechanics. It inspected the exact locally locked
Axum `0.8.9`, axum-core `0.5.6`, and tower-http `0.6.11` body-limit, CORS, and
static-service sources. These constrain mechanics but do not supply product
policy, source, fixtures, or wire compatibility.

The accepted multiplexed connection and future relay direction in
[`application-connection-architecture.md`](topics/application-connection-architecture.md)
also uses the maintainer's local `~/code/yepanywhere` sibling as an
architectural and failure reference. The topic records the exact observed
commit and paths for its inner request/subscription router, plain and encrypted
connection composition, opaque outer relay circuits, per-circuit fair queues
and head-of-line-blocking analysis. RSTorrent adopts those lessons without
copying source or adopting YepAnywhere's wire contract, HTTP-like operations,
cryptography, framing constants or relay limits.

## libtorrent

Project: [libtorrent](https://libtorrent.org/)

Source: [arvidn/libtorrent](https://github.com/arvidn/libtorrent)

Managed source checkout: `reference/libtorrent`, pinned to `v2.0.13`

Rasterbar libtorrent is the primary external interoperability oracle. It can
seed to RSTorrent, leech from it, create fixtures, enforce encryption modes,
and expose peer/session state for black-box assertions.

It is also the mandatory completeness and edge-case reference for feature
work. Review both implementation and tests at the pinned revision; a successful
black-box exchange alone does not establish that the relevant failure,
lifecycle, persistence, and resource cases were considered.

Use libtorrent as an independent peer rather than an RSTorrent runtime
dependency. Interoperability tests should verify payload hashes and observable
protocol results, not only that both processes remained alive.

This is distinct from the similarly named libTorrent used by rTorrent.

Tactical `051` used the pinned libtorrent peer-info projection, terminal
legend, and peer-list/fast-extension tests together with JSTorrent's active
Peer table formatter and typed incoming direction to define an independently
authored semantic flag vocabulary. Exact revisions, paths, functions, adopted
lessons, and deliberate differences are recorded in
[`peer-flag-vocabulary.md`](topics/peer-flag-vocabulary.md). No reference code,
fixture, test data, or asset was copied.

## rqbit And librqbit

Project: [rqbit](https://github.com/ikatson/rqbit)

Library documentation: [librqbit](https://docs.rs/librqbit/)

Managed source checkout: `reference/rqbit`, pinned to the exact current 9.0
release-candidate development revision recorded in `reference/pins.toml`

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
