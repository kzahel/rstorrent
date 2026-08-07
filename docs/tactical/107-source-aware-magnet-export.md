# Tactical 107: Source-Aware Magnet Export

Status: Complete on 2026-08-07.

Topics: `application-control`, `client-persistence`, `application-view-api`,
`client-surfaces`, `web-ui-design`, `table-interaction`

## Motivation And Outcome

The shared web UI currently builds copied magnet links from only the projected
v1 info hash. That discards the exact magnet URI retained since Tactical 081
and produces unnecessarily sparse links for torrents added from `.torrent`
bytes.

Add an explicit read-only application operation which returns a verified exact
magnet source when one exists and otherwise synthesizes the richest magnet the
current durable model can truthfully support: v1 identity, verified display
name, and the normalized tracker catalog in tier order. Route the shared
context-menu and More-menu copy action through that operation.

The slice stops when exact submitted magnets round-trip through Copy magnet
link, metainfo torrents copy a bounded usable link containing their verified
name and trackers, omission is reported rather than hidden when a bound is
reached, deterministic Rust and web coverage passes, and the owning topics
record the new boundary.

## Source And Reference Survey

- BEP 9 at pinned BEP commit
  `7b7b41a23e95b10406eb2082c4d15f347408c8a7`,
  `reference/bittorrent.org/beps/bep_0009.rst`, defines the v1 magnet identity
  (`xt`), optional display name (`dn`), repeated trackers (`tr`), and optional
  explicit peer (`x.pe`) vocabulary.
- Pinned libtorrent `7d7fc38e00982a3c0ade5720ee03d4a7e3224487`
  (`v2.0.13`) implements `make_magnet_uri(torrent_info const&)` and
  `make_magnet_uri(add_torrent_params const&)` in `src/magnet_uri.cpp`.
  `include/libtorrent/magnet_uri.hpp` documents the inputs, while
  `test/test_magnet.cpp` covers names, trackers, peers, v1/hybrid identity,
  selection, and round trips. It is a completeness oracle, not an architecture
  template.
- JSTorrent commit `9895410beeed6aff554053769bd006a3fbd373ef`
  uses the original `magnetLink` when available and otherwise calls
  `generateMagnet({ infoHash, name, announce })` from
  `packages/client/src/AppContent.tsx`. The generator in
  `packages/engine/src/utils/magnet.ts` emits `xt`, `dn`, and repeated `tr`;
  `packages/ui/src/components/GeneralPane.tsx` applies the same product rule.

RSTorrent adopts the exact-or-synthesize behavior. It deliberately does not
adopt libtorrent's dynamic peer, DHT-node, file-priority, or URL-seed export:
those values are either volatile, local policy, or not a normalized supported
source in the current durable model. Unsupported fields already present in an
exact submitted magnet remain intact because the verified source string is
returned verbatim.

## Ownership And Dependency Direction

- `rstorrent-session` owns source-integrity verification, durable tracker/name
  reads, synthesis, output bounds, and the export result classification.
- The existing synchronous application-control path owns request validation
  and a typed, non-mutating command result. It creates no task, receipt,
  revision, or cancellation path.
- Generated Rust/TypeScript contracts carry the result to live clients. Exact
  source text remains absent from routine service snapshots and application
  views.
- The live adapter maps the typed result into the client application model.
  The demo adapter deterministically synthesizes from its local row because it
  has no durable source store.
- The shared torrent-action context requests each selected export, commits one
  clipboard value after all requests succeed, and owns bounded user feedback.
  Where supported it starts a promise-backed `ClipboardItem` during the user
  activation so asynchronous application calls do not lose clipboard
  authority. Existing menu and focus owners remain unchanged.

The dependency remains UI -> generated application contract -> session store;
protocol parsing and metainfo authority do not depend on a client surface.

## Scope And Invariants

- Add `export_magnet { torrent_id }` as a semantic non-mutation and return a
  typed magnet, provenance (`verbatim`, `canonicalized`, or `synthesized`),
  and the number of normalized trackers omitted by output bounds.
- For retained magnet sources, verify recorded byte length and SHA-256,
  reparse the URI under current magnet bounds, and require its v1 identity to
  match the requested torrent before returning it. Preserve the source string
  byte-for-byte and classify it using the stored fidelity.
- A missing, corrupt, or non-magnet exact source falls back to synthesis from
  durable operational facts. Source corruption never invalidates the torrent
  itself and never leaks unverified bytes.
- Synthesis begins with lowercase hexadecimal
  `magnet:?xt=urn:btih:<info-hash>`, adds the verified metainfo publication name
  as `dn` when present, and adds normalized trackers in tier/position order as
  repeated `tr` values.
- Percent-encode synthesized query values using the existing URI encoding
  policy. The complete output must remain accepted by RSTorrent's magnet
  parser: at most 16 KiB, 128 parameters, and 32 trackers. Skip tracker values
  that do not fit and report every omission; retain later values that still
  fit instead of stopping at the first oversized candidate.
- Unknown torrent identity produces the existing typed `unknown_torrent`
  error. Export never changes the durable revision or snapshot.
- Multi-selection preserves current table order, performs one export per
  current target, joins links with one newline, and writes the clipboard once.
  Any failed export or clipboard call reports failure without claiming or
  performing a partial copy.
- Add no schema migration, dependency, background task, network operation,
  platform-private source field, or per-row magnet payload.

## Non-Goals

- `.torrent` file export, base64/source inspection APIs, QR/share sheets, or
  Android Compose presentation.
- Synthesizing web seeds, DHT nodes, volatile peers, peer hints, file
  selection, or other state the durable source model does not authorize.
- Canonicalizing or rewriting a valid retained magnet, merging later tracker
  observations into it, or promising an equivalent textual order.
- Adding v2/hybrid torrent identity support ahead of the engine's existing v1
  scope.

## Validation

- Session-store tests cover byte-for-byte retained magnet export, metainfo
  name/tracker synthesis and ordering, integrity-failure fallback, output
  bounds/omission accounting, parseability, unknown identity, and no revision
  change.
- Generated-contract and live-adapter tests cover exact command/result
  serialization and mapping.
- Demo and React component tests cover source-aware single/multi-selection,
  one clipboard write, table-order newline joining, omission feedback, and
  export/clipboard failures while retaining menu focus behavior.
- Run Rust formatting, clippy, workspace tests, generated-contract freshness,
  web formatting/type checking, focused and complete Vitest, production
  build/CSP validation, focused headless browser coverage, and
  `git diff --check` in proportion to the completed slice.

## Implemented Result

The generated application contract now exposes `export_magnet` as a semantic
non-mutation with a typed `verbatim`, `canonicalized`, or `synthesized` result
and exact omitted-tracker count. The session store verifies retained magnet
length, SHA-256, bounded parsing, and requested identity before returning the
text unchanged. A missing, metainfo, or corrupt exact source falls back to
lowercase v1 identity, verified publication name, and every valid normalized
tracker that fits the existing 16-KiB and 32-tracker limits. It continues
after an oversized candidate so a later short tracker can still be retained.
Unknown torrents return `unknown_torrent`; the operation adds no receipt,
revision, view refresh, discovery reconciliation, task, or schema field.

The live React adapter maps that result without placing source data in its
Zustand snapshot. The deterministic demo synthesizes identity and name. The
shared torrent action exports every stable target in application order and
commits one newline-delimited clipboard value only if all exports succeed.
Browsers with `ClipboardItem` begin the promise-backed write during the user
activation so the application round trip does not consume clipboard authority;
the existing `writeText` path remains as a capability fallback. Exact export,
application rejection, clipboard rejection, multi-target order, and omission
feedback all retain the established menu closure and focus-return behavior.

## Validation Evidence

The following ran on 2026-08-07:

- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace -- -D warnings`: pass, including desktop and
  Android Rust adapters.
- `cargo test --workspace`: pass. The new store cases covered verbatim and
  migrated-canonical source export, digest-corruption fallback, rich metainfo
  synthesis, unknown identity, no revision change, count/byte bounds, later
  short-tracker retention, and output reparsing. Existing opt-in live and
  maximum-allocation cases remained ignored.
- `npm run generate --prefix clients/web`: pass; generated TypeScript, JSON
  Schema, and validators include the new command and result.
- `npm run typecheck --prefix clients/web`: pass.
- Focused Vitest for contract validation, demo/live adapters, and React
  actions: 4 files and 76 tests passed.
- `npm test --prefix clients/web`: 35 files and 223 tests passed; the two
  existing opt-in files and tests remained skipped.
- `npm run build --prefix clients/web`: pass, including the production CSP
  scan. The existing large-chunk advisory remained non-fatal.
- Focused headless Chrome clipboard readback: pass. It exercised the
  promise-backed clipboard path, two synthesized name-rich demo magnets,
  one newline-delimited value, menu closure, focus return, and the existing
  accessibility assertion.
- `git diff --check`: pass.

The standard Playwright port was already occupied by an unrelated preview
process. Validation left it untouched, used isolated port `4189`, and joined
that temporary server afterward. No public-network operation, visible product
client, emulator, physical device, dependency, or database migration was
introduced or run.
