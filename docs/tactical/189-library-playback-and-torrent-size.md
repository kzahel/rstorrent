# Tactical 189: Library Playback And Torrent Size

Status: Complete on 2026-08-28 by explicit user direction. Tactical `176`
keeps only its unchanged macOS-hosted iOS compile gate and resumes after this
bounded repair.

Topics: `application-view-api`, `web-ui-design`, `client-surfaces`,
`capability-readiness`

## Motivation And Outcome

Completed Tactical `072` deliberately rendered the Library Media row's play
glyph without a playback action. In the browser product this now reads as a
broken control: activating the visible glyph does nothing even when the file
is verified and the existing bounded HTTP media capability can serve it.

The shared torrent row also discards size unconditionally in the live React
adapter. Metadata-backed torrents consequently show `Size pending` in Library
detail and an em dash in Library, Transfers, and Workbench even though the
application already owns exact content geometry.

Make eligible Library Media rows open the existing ephemeral media capability
through the existing `open_file` semantic action. Add one exact nullable
decimal total-size field to the authoritative torrent summary, carry sparse
updates through every first-party reducer, and render it through the existing
web size formatters.

## Stable Scenarios

- A verified or typed streamable recognized video has an accessible **Play**
  button. Browser activation synchronously reserves an opener-isolated tab,
  requests the existing capability for the exact torrent and file index, and
  navigates that tab to the returned media URL.
- A Library Media row that is checking, unverified, skipped, unavailable, or
  otherwise not typed `available` or `streamable` cannot create a capability.
  Its disabled control explains that playback is unavailable.
- Only one Library Media play request is active at a time. The row and status
  presentation remain truthful on success, typed rejection, popup rejection,
  transport failure, navigation away, and torrent removal.
- Tauri keeps the existing system-opener behavior behind the same shared
  action; no loopback listener, embedded player, or second media path is added.
- Before verified metadata exists, torrent total size is null and existing
  pending/unknown presentation remains truthful.
- Once verified content geometry exists, `TorrentView.total_size_bytes` is the
  exact total content length as a decimal `u64` string, including metainfo
  padding geometry. It remains stable across priority changes, progress,
  completion, pause, checking, restart, and archive state.
- The live React adapter maps the authoritative value instead of permanently
  assigning null, so Library cards, Library detail, Transfers, and Workbench
  General all show the same size after metadata arrives.
- A metadata transition emits a sparse torrent-row size update. TypeScript,
  Android, and Swift reducers apply it and reject duplicate-field or identity
  discontinuity under their existing rules.

## Scope

- Add nullable `total_size_bytes` to `TorrentView` and to the closed sparse
  `TorrentFieldUpdate` inventory.
- Derive it from the immutable `FileProgressModel` content layout whenever the
  durable view model has verified metadata; do not infer size from piece count,
  transfer totals, selected payload, or client-side file pages.
- Regenerate the checked-in TypeScript/schema/validator artifacts and update
  the TypeScript, Android, and Swift sparse reducers.
- Map the exact decimal value into the existing React `TorrentRow.sizeBytes`
  presentation field and retain null only while geometry is absent.
- Turn the Library Media glyph into a real button for eligible rows and route
  it through the existing `open_file` application action.
- Add focused Rust contract/projection/update tests and React live mapping,
  command, failure/status, responsive, and accessibility coverage.
- Update the owning topics and readiness record with actual evidence.

## Non-Goals

- An embedded `<video>` player, playback overlay, resume position, watched
  state, duration, codec probing, browser-playability promises, subtitles,
  playlists, thumbnails, artwork, or Library-wide aggregation.
- A new HTTP route, capability lifetime, MIME mapping, media range policy,
  streaming-demand policy, scheduling priority, storage owner, or Tauri opener.
- Android Compose or iOS SwiftUI playback presentation. Their generated
  contracts and reducers remain exhaustive, but no new screen or listener is
  added.
- File-level ETA/progress redesign, selected/wanted size, payload accounting,
  protocol behavior, persistence schema, compatibility migration, or public
  API stability beyond the incubation contract regeneration.
- A visible desktop run, public swarm, emulator, simulator, physical device,
  remote service mutation, release, tag, or publication.

## Existing Owners And References

- Tactical [`072`](072-derived-media-catalog.md) owns the derived Media rows,
  stable `(torrent_id, file_index)` identity, availability join, responsive
  virtualization, and original playback deferral.
- Tacticals [`138`](138-verified-http-file-serving.md) and
  [`139`](139-incomplete-file-streaming-demand.md) own the ephemeral capability,
  MIME/range server, opener preparation, active demand, expiry, revocation,
  and joined lifecycle. This repair consumes those semantics unchanged.
- [`../topics/application-control.md`](../topics/application-control.md) owns
  `create_media_url` as a read-only semantic call.
- [`../topics/application-view-api.md`](../topics/application-view-api.md)
  owns complete torrent summaries and closed typed sparse row updates.
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md) owns shared React,
  responsive controls, accessibility, and deterministic browser evidence.
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md) owns browser,
  Tauri, Android, and iOS presentation boundaries.

This is application-contract and product-presentation work. It changes no
BitTorrent protocol state, engine scheduling, or storage transition, so no BEP
or pinned-libtorrent source survey applies. The existing independently
implemented and tested media server is the behavioral dependency; no reference
source, fixture, or asset is imported.

## Ownership, Dependency, And Lifecycle Map

```text
verified TorrentContent / ContentLayout
  -> FileProgressModel (existing immutable geometry owner)
  -> ViewHub TorrentView.total_size_bytes
  -> complete snapshot or typed TorrentFieldUpdate
  -> generated TypeScript / UniFFI boundary
  -> exhaustive web / Android / Swift reducers
  -> React TorrentRow.sizeBytes
  -> Library / Transfers / Workbench formatters

Library Media Play button
  -> existing InspectionCommand::open_file(torrent_id, file_index)
  -> synchronous existing browser/Tauri opener preparation
  -> existing create_media_url semantic call
  -> existing capability owner and HTTP byte-range server
```

No task, timer, channel, queue, cache, persistent record, or new mutable owner
is added. React owns only bounded ephemeral pending/status state and clears it
when the selected torrent changes or the detail unmounts. Existing capability
expiry/revocation and media-server shutdown remain the cancellation and
termination paths for payload service.

## Contract And Invariants

Conceptually:

```text
TorrentView {
    total_size_bytes: Option<decimal u64 string>,
}

TorrentFieldUpdate +=
    TotalSizeBytes { value: Option<decimal u64 string> }
```

- `Some` is derived only from an accepted immutable content layout and equals
  `ContentLayout::total_length()` exactly.
- `None` means verified metadata/content geometry is not available. It never
  means zero; accepted torrent geometry is nonzero under existing parser rules.
- The value includes padding because it describes complete torrent geometry,
  not selected user payload. `required_payload_bytes` retains its independent
  selected non-padding ETA meaning.
- Decimal strings cross the contract; clients do not serialize platform paths,
  media URLs, file pages, or recomputed sums into torrent summary state.
- Play eligibility remains the existing closed `MediaFileAvailability`
  authority. Presentation does not infer readiness from 100% text alone.
- The media capability is requested only from a direct user activation. The
  browser opener must be reserved before the first asynchronous boundary so
  popup policy remains deterministic.

## Result

- `FileProgressModel` now exposes its accepted content layout's exact total
  length. Complete torrent summaries and their closed sparse updates carry it
  as a nullable decimal string, and generated TypeScript plus exhaustive web,
  Android, and Swift reducers retain it.
- The live React adapter maps that value into the existing torrent-row model.
  One component regression proves the same 4.4 GB value in Library collection,
  Library detail, Transfers, and Workbench General and proves that none falls
  back to `Size pending` or an em dash after metadata.
- Every Media row now has a real accessible Play button. Verified `available`
  and active `streamable` rows call the existing `open_file` action with their
  exact stable file index; ineligible and demo rows explain why they are
  disabled. One bounded request/status owner prevents duplicate activation and
  discards completion after detail navigation or unmount.
- The existing live action remains the only payload path. Browser activation
  reserves a tab before requesting the capability and then navigates it to the
  media URL; Tauri retains its system opener. No player, server, scheduler, or
  capability semantics changed.

## Validation Evidence

- `cargo test -p rstorrent-session views::` passed 78 tests.
- `npm run generate --prefix clients/web` regenerated the checked-in contract,
  schema, and validators.
- Focused React/live-adapter/reducer/validation tests passed 127 tests.
- `npm run typecheck --prefix clients/web`, `npm run test --prefix clients/web`,
  and `npm run build --prefix clients/web` passed; the complete Vitest result
  was 336 passed and 2 skipped across 49 passing and 2 skipped files, and the
  production CSP scan passed all 10 bundles.
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, and
  `cargo test --workspace` passed.
- `clients/android/build.sh` passed both release native ABIs, generated Kotlin,
  the debug APK, and JVM unit tests. `cargo build -p rstorrent-ios --release`
  passed the Linux-available Apple Rust/binding boundary. The macOS Xcode/Swift
  compile remains Tactical `176`'s sole unchanged gate.
- The headless Library media-detail browser case passed at wide and phone sizes
  with 3,003 recognized rows, 19 mounted rows, and 440 DOM elements. The full
  Playwright run passed 35 tests and skipped 14; its one Swarm-only horizontal
  scroll-indicator assertion failed after prior scenarios but passed in an
  immediate isolated rerun. That test path does not exercise Library playback
  or torrent-size projection, so no broader browser-suite claim is made from
  this slice.

## Stopping Condition

This tactical is complete when an eligible Library Media Play activation is
proven to issue the exact existing media-capability action, metadata-backed
torrent size is proven coherent across snapshot and sparse-update delivery and
the four named React surfaces, every maintained first-party reducer is
exhaustive for the regenerated field, proportional repository/web/platform
gates pass, and the owning documentation records the result. Embedded playback
and broader media-library behavior remain separate future slices.

The condition is satisfied. Tactical `176` resumes as the sole **Now** with
only its unchanged macOS-hosted iOS compile gate.
