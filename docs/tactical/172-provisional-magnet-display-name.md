# Provisional Magnet Display Name

Status: Complete (2026-08-27).

Topics: `client-persistence`, `application-view-api`, `web-ui-design`,
`client-surfaces`, `protocol-support`

## Motivation

RSTorrent retained an added magnet URI verbatim, but its bounded operational
magnet parser discarded `dn`. A metadata-less torrent therefore appeared under
an opaque owner fallback even when its source supplied the display name that
BEP 9 explicitly defines for presentation while metadata is pending.

This slice makes that useful source label visible immediately without
reclassifying it as verified metadata or using it for any filesystem,
publication, integrity, or routing decision.

## Scope And Stopping Condition

- Parse at most one effective bounded `dn` value from magnet intake.
- Preserve it in the operational canonical magnet while retaining the exact
  source record unchanged.
- Recover it from a verified retained source for existing schema-19 rows whose
  older operational magnet omitted `dn`, without a schema reset or migration.
- Project it separately from the verified metainfo `display_name`.
- Use verified name, then provisional source name, then the existing opaque
  fallback in first-party presentation.
- Prove parsing, restart/recovery, view replacement, generated boundaries, and
  web/mobile presentation with deterministic tests.

The slice stops when a fresh or retained metadata-less magnet with a valid
`dn` is named in the torrent list, verified metadata supersedes that label,
invalid labels remain absent, proportional validation passes, and the owning
topics record the completed behavior.

## Non-Goals

- Treating `dn` as authenticated metadata, a payload path, publication name,
  or user-editable alias.
- Exposing the complete magnet URI or credential-bearing tracker parameters in
  routine snapshots, views, logs, or presentation.
- Replacing the retained exact source, changing duplicate-add source policy,
  adding a schema version, or changing metadata and payload acquisition.
- UI provenance badges, renaming controls, categories, or search policy.

## Source Dossier

- Pinned BEP source: `reference/bittorrent.org` at
  `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`.
  `beps/bep_0009.rst`, **magnet URI format**, defines optional `dn` as the
  display name a client may use while waiting for metadata.
- Pinned libtorrent: `reference/libtorrent` at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d`.
  `src/magnet_uri.cpp::parse_magnet_uri` percent-decodes case-insensitive `dn`
  into `add_torrent_params::name`; `make_magnet_uri` emits that name.
  `test/test_magnet.cpp` covers ordinary, mixed-case, percent-escaped, `+`
  space, missing-hash, and select-only magnets carrying `dn`.
- JSTorrent `main` was inspected at
  `packages/engine/src/utils/magnet.ts::{parseMagnet,generateMagnet}`.
  It uses `URLSearchParams`, projects the first nonempty `dn` as `name`, and
  emits it during generation. Product fixtures rely on pre-metadata names.

RSTorrent adopts case-insensitive parsing, percent/plus decoding, pre-metadata
presentation, and verified-metadata precedence. It deliberately bounds the
decoded value to 255 UTF-8 bytes, rejects empty or control-bearing labels, and
keeps source and verified names as distinct typed fields. Optional invalid
`dn` text is ignored rather than invalidating an otherwise usable magnet.

## Ownership, Data Flow, And Bounds

```text
bounded magnet parser
    -> parsed source display name (untrusted presentation text)
    -> canonical operational magnet + exact retained source
    -> restart derivation
    -> TorrentView.source_display_name
    -> verified name ?? source name ?? opaque fallback

verified raw info
    -> TorrentView.display_name
    -> filesystem/publication authority remains unchanged
```

- The runtime-free protocol parser owns decoding, singular-value policy, and
  the 255-byte/control-character bound. It allocates no work beyond the
  existing 16-KiB URI and 128-parameter bounds.
- The session store owns canonical persistence. For older current-schema rows,
  it may read the one bounded retained magnet only after byte-length, SHA-256,
  current parser, and torrent-identity validation, then extract only `dn`.
- The view hub owns separate optional verified and source fields. Metadata
  arrival is an ordinary complete-row upsert and does not reset a lease.
- Web, Android, desktop notifications, and iOS own presentation precedence.
  Android filesystem/open operations continue to use only the verified name.
- No owner, background task, channel, retry, cancellation path, network
  operation, or new long-lived collection is introduced.

## Required Evidence

- Protocol tests: decoding/case/space behavior, repeated value policy, byte
  bound, empty value, and control rejection.
- Store tests: canonical retention, exact source byte preservation, restart,
  and recovery from an older current-schema operational magnet.
- View tests: separate source projection and verified-name replacement in list
  and selected summary patches.
- Generated TypeScript/schema/validator and UniFFI consumer builds.
- Web adapter/validation tests and first-party Android/iOS presentation checks
  in proportion to available host toolchains.

## Outcome And Evidence

Completed on 2026-08-27.

- `Magnet` now retains the first acceptable case-insensitive `dn`, after
  query decoding, under the 255-byte and control-character bounds. Canonical
  operational magnets emit it with percent encoding; the exact submitted
  source remains byte-for-byte unchanged.
- Resume recovers a missing operational `dn` from an older schema-19 retained
  source only through the shared length, SHA-256, bounded parse, fidelity, and
  identity verification used by exact magnet export. The recovery is derived
  in memory and does not mutate or promote source provenance.
- `TorrentView` carries optional `source_display_name` separately from the
  verified `display_name`. Rust projection tests prove the source-only row and
  later verified-name replacement in both torrent-list and selected-summary
  views.
- Generated TypeScript/schema/validators and generated Kotlin bindings carry
  the additive optional field. Web, Android, iOS, and desktop-notification
  presentation all prefer verified name, then provisional source name, then
  the existing opaque fallback. Android file opening still consults only
  verified `displayName`.
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, the
  focused protocol/store/view/application tests, and the focused protocol plus
  session crate suites pass. `cargo test --workspace` also passes in full.
- `npm run generate --prefix clients/web`, `npm run typecheck --prefix
  clients/web`, the 41 focused live-adapter and validator tests, and `npm run
  build --prefix clients/web` pass. The complete web run passes 42 files (286
  tests) and skips two files, while the unrelated demo-only bounded-scale test
  remains red because one microtask wait does not finish selected Swarm view
  materialization; an isolated rerun reproduces that same failure outside the
  changed source-name path.
- `clients/android/build.sh` passes both release ABIs, host binding generation,
  `assembleDebug`, and `testDebugUnitTest`. The Rust iOS crate compiles in the
  workspace; Swift/Xcode validation is unavailable on the Linux host.
