# Metadata Display Name

Status: Complete.

Topics: `application-view-api`, `web-ui-design`,
`desktop-inspection-surface`

## Motivation

The live web adapter still labels every torrent as an info-hash prefix. That
fallback is truthful while a magnet is awaiting metadata, but it remains after
verified metainfo arrives even though the durable session already owns the
bounded torrent name. The library and General detail therefore hide the most
useful human identity precisely when it becomes available.

## Scope

- Derive an optional display name only from successfully parsed, verified
  durable metainfo.
- Publish it on the existing complete `TorrentView` row so ordinary list and
  selected-summary patches update together.
- Regenerate the TypeScript, JSON Schema, UniFFI, and Kotlin contracts.
- Use the name in live web library and General presentation, retaining the
  existing info-hash-prefix fallback before metadata.
- Give the Android bootstrap presentation the same verified-name fallback
  behavior without otherwise changing its UI.
- Prove the transition in pure projection/adapter tests and in the existing
  controlled headless magnet transfer.

## Invariants And Bounds

- Unverified magnet parameters and peer-provided diagnostic text never become
  the display name.
- The name comes from `Metainfo::from_info_bytes`, whose safe component rules
  bound it to 255 bytes and reject unsafe path-like values.
- A missing or invalid durable metainfo value produces `null`; clients retain
  a deterministic info-hash fallback.
- Name arrival is an ordinary complete-row upsert. It does not reset view-set
  identity, create a separate subscription, or change torrent selection.

## Non-goals

User-editable labels, aliases from magnet `dn`, renaming payload paths,
category naming, remote authentication changes, or a broader General-tab
redesign are outside this slice.

## Validation

- Rust projection tests cover no-name to verified-name replacement and both
  torrent-list and selected-summary patches.
- TypeScript validation and live-adapter tests cover the bound and fallback.
- The controlled libtorrent magnet fixture must show `magnet-fixture` in both
  the library row and General heading after metadata verification.
- Run format, warning-denying Clippy, workspace tests, generated-contract
  checks, web typecheck/tests/build, deterministic Playwright, and the
  controlled headless live proof in proportion to touched surfaces.

## Stopping Condition

The slice is complete when a magnet begins with the deterministic hash label,
automatically changes to its verified metainfo name in both requested web
summary surfaces, survives restart through the same durable derivation, passes
the controlled headless proof, and the evidence is recorded.

## Outcome And Evidence

`TorrentView.display_name` now carries the optional verified metainfo name.
The application derives it from the same successfully parsed durable
`raw_info` used for file geometry, both during the metadata checkpoint refresh
and after profile reopen. Replacing the durable model emits ordinary complete
row patches to both the torrent list and selected summary. Missing metadata
still serializes no name and retains the deterministic client fallback.

The live web adapter uses the field for its shared `TorrentRow`, so the library
Name cell and General heading update together. Its decoder enforces the
255-byte bound. The Android bootstrap card uses the same generated optional
field and otherwise retains the info-hash fallback.

Pure Rust coverage proves both projection patches and restart derivation.
TypeScript validation, reduction, and live-adapter coverage prove the bound,
complete-row replacement, and presentation mapping. The controlled headless
libtorrent 2.0.13.0 magnet proof showed `magnet-fixture` in the library row and
General heading before continuing through its 122-file, three-piece verified
transfer. The run passed in 30.4 seconds, every child joined, and temporary
state was removed.

Validation passed workspace formatting, warning-denying Clippy, all workspace
tests, generated-contract regeneration, 53 Vitest tests, the production web
build, six deterministic Playwright scenarios, the controlled live scenario,
and the two-ABI Android/UniFFI debug build plus unit tests.
