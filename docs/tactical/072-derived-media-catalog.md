# Tactical 072: Derived Media Catalog And Library Torrent Detail

Status: Completed on 2026-08-28. Tactical `176` resumes as the sole **Now**
with only its unchanged macOS-hosted iOS compile gate.

Topics: `application-interface-direction`, `application-view-api`,
`web-ui-design`, `desktop-inspection-surface`, `client-surfaces`

## Motivation And Outcome

Files is intentionally a literal torrent-content view. It exposes every
metainfo file, padding and selection semantics, exact byte progress, and
storage geometry. That makes it useful for inspection and control, but a
multi-episode torrent remains difficult to browse as media: release filenames
sort poorly as plain text, non-media sidecars dominate the list, and the
application has no typed episode semantics to reuse in its eventual Library.

Add a runtime-independent `rstorrent-media-catalog` crate that deterministically
classifies recognized video paths and extracts conservative television episode
hints. The application view owner retains one rebuildable derived media catalog
per torrent, joins it with the existing authoritative file progress, and
exposes a separately leased `torrent_media` projection. The shared React
application makes a Library-card activation open a content-focused torrent
detail. That detail defaults to a responsive virtualized media list ordered by
typed episode semantics, retains an explicit All files escape hatch, and keeps
the source torrent's Workbench handoff secondary.

This slice establishes media identity and a truthful first presentation. It
does not generate thumbnails, probe file contents, add playback presentation,
enrich filenames from an external service, change download scheduling, or
replace the current torrent-backed Library collection.

## Stable Scenarios

- Before magnet metadata is verified, the selected Library detail says
  that media information is waiting for metadata. It is not an empty catalog.
- A torrent containing `Sample Show/Season 01/Sample.Show.S01E02.mkv` and
  `Sample.Show.S01E10.mkv` presents episode 2 before episode 10 even when
  metainfo order or lexical filename order differs.
- `S01E07-E08`, `S01E07-08`, `S01E07E08`, and `1x07-08` retain an explicit
  starting and ending episode. A following `720p` or `-720p` release tag is not
  mistaken for episode 720.
- `Sample Show/Season 01/S01E07.mkv` uses the nearest meaningful parent folder
  as its series-title hint; a bare episode without a meaningful parent remains
  an unclassified video rather than inventing a title.
- A recognized video whose filename does not parse as an episode still appears
  under its exact filename after episode-classified items.
- `.nfo`, checksums, images, subtitles, padding, directories represented only
  by path components, and other unrecognized files do not produce Media rows.
  Their continued presence in Files is unchanged.
- Uppercase and mixed-case recognized extensions classify identically.
- Rows show exact filename/path context, size, High, Normal, or Skip selection,
  existing Done and Verified progress, and the existing media-availability
  state. Boundary-piece bytes retain the same meaning as Files; Media never
  upgrades them into playback readiness.
- A torrent with verified metadata and no recognized video shows the genuine
  empty state `No recognized video files`.
- Activating a Library card opens its detail immediately rather than merely
  drawing a selected outline. Back, Escape, and browser history return to the
  same Library category and retained scroll position; focus returns to the
  originating card when it still exists.
- A detail with recognized video defaults to Media. A detail with no recognized
  video falls back truthfully to All files. The explicit Media and All files
  choices lease only the visible projection and preserve the active torrent.
- Removing the open torrent or leaving Library closes the detail without a
  stale title, rows, history target, or hidden view lease.
- Suspension, view-set expiry, transport reset, and application restart rebuild
  a coherent Media snapshot without stale or duplicated rows.
- The legal 4,096-file catalog remains bounded in Rust, encoded delivery,
  reducer work, browser memory, and visible DOM size.

## Scope

- Add a small pure `rstorrent-media-catalog` workspace crate with closed
  classification values, a versioned deterministic classifier, and no runtime,
  storage, protocol, serialization, or platform dependencies.
- Add `regex` as the crate's one direct third-party dependency, using the
  workspace's already resolved compatible release. Compile the fixed patterns
  once per process rather than once per file or view.
- Recognize an explicit case-insensitive initial video-extension set and parse
  the bounded episode forms named in this tactical.
- Retain one immutable derived media catalog for each torrent with verified
  metainfo. Reuse it across view opens, progress activity, file-selection
  changes, durable refreshes, and engine-generation replacement.
- Keep classification rebuildable and out of SQLite. Recompute once after
  application restart or after a torrent is removed and independently added
  again.
- Add a separate application `MediaItemView`, `torrent_media` capability,
  view-set specification, snapshot, keyed complete-row patch, generated
  TypeScript/schema and UniFFI bridge types, strict browser validation, and
  reducer/store materialization.
- Reuse the existing file-progress authority for length, selection, Done,
  Verified, and media availability. Classification must not create a second
  progress or availability tracker.
- Add ephemeral Library-detail navigation with same-document browser history,
  explicit Back and Escape behavior, deterministic selection/removal repair,
  focus restoration, and retained collection scroll.
- Request `torrent_media` only while a Library detail's Media view is visible;
  switch to the existing `torrent_files` projection only while All files is
  visible. Keep Workbench detail leasing and tab state unchanged.
- Add a deterministic named demo catalog and a responsive, accessible,
  virtualized list with numeric episode ordering, stable file-index ties, a
  wide summary/list arrangement, and stacked compact/phone rows.
- Reduce the phone Library collection's generated-placeholder footprint to a
  compact landscape tile without claiming that it is artwork.
- Add pure, contract, reducer, component, headless browser, scale, and
  controlled live evidence; regenerate checked-in contracts and update the
  owning topics with the actual result.

## Non-Goals

- Thumbnail generation, thumbnail persistence, artwork download, poster or
  backdrop presentation, color extraction, or a server-side thumbnail cache.
- Changes to the completed HTTP byte-range server, capability URL, direct file
  handle, local path handoff, media probing, duration, dimensions, codecs,
  container support, or browser-playability detection.
- A Play, Open, Reveal, Watch, watched-state, resume-position, or media-file
  selection action from a Library detail row.
- Changes to streaming, playback-oriented piece priority, incomplete-file
  range waits, media prefetch, peer, scheduler, storage, or integrity owners.
- External metadata lookup, TMDB integration, normalized external identity,
  movie matching, episode names, user corrections, privacy policy, accounts,
  or cross-device media state.
- Replacing or aggregating the current torrent-backed top-level Library
  collection. This slice enriches only one explicitly opened source torrent;
  a later tactical may aggregate semantic media items across torrents.
- Audio, image, ebook, archive, subtitle, or sidecar catalog presentation. The
  first classifier emits video candidates only.
- Persistent derived-classification rows, a media database, migration, or a
  stable public remote-media contract.
- Android Compose Media UI, a router dependency, durable URL/deep-link
  compatibility, a visible Tauri run, public-swarm traffic, or physical-device
  testing.
- Copying PlaysVideo or JSTorrent source, tests, fixtures, persistence schemas,
  extension lists, CSS, or playback architecture mechanically.

## Vocabulary And Semantic Boundary

- **Media classifier** is the pure `rstorrent-media-catalog` function that maps one
  bounded relative path to either a recognized video classification or no
  media item.
- **Media classification** is the rebuildable result for one torrent file. It
  can be an episode hint or an unclassified video.
- **Derived media catalog** is the immutable, in-memory set of classifications
  for one verified torrent metainfo object.
- **Media item identity** is `(torrent_id, file_index)`. Parsed title and
  episode values are hints and never replace that stable file identity.
- **Media view** is the application DTO obtained by joining a derived item with
  current file selection and progress.
- **Media Library** is the future product-level aggregation that may combine
  derived items with durable enrichment, artwork, organization, and playback
  facts. It is not implemented here.

`deterministic` means that the same accepted path and classifier version
produce the same classification without a clock, locale, filesystem, network,
database, torrent runtime, or client preference. It does not mean that a
filename hint is verified factual metadata about a television series.

## Dependencies And Reference Dossier

### Existing RSTorrent owners

- [`041-live-file-inspection.md`](041-live-file-inspection.md) establishes the
  bounded 4,096-row file catalog, exact file-index identity, metadata-pending
  state, Done/Verified semantics, complete-row patches, large-snapshot path,
  generated contracts, and responsive virtualized presentation.
- [`055-application-destinations.md`](055-application-destinations.md)
  establishes Workbench as the detailed first-class destination and records
  that its initial Library has no media semantics or playback claims.
- [`../topics/application-interface-direction.md`](../topics/application-interface-direction.md)
  owns the eventual content-oriented Library and requires a source- and
  edge-case-driven tactical before media behavior.
- [`../topics/application-view-api.md`](../topics/application-view-api.md)
  owns named projections, bounded coherent snapshots, keyed patches, leased
  interest, reset recovery, and generated contracts.
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md) owns responsive
  React, CSS Modules, browser-local tab preference, accessibility, and bounded
  rendering.

This slice changes no BitTorrent protocol, network, scheduling, storage, or
integrity behavior. No BEP or pinned libtorrent state transition governs
filename classification, so a libtorrent oracle survey is not required.
Verified metainfo and the existing file-progress projection remain the only
torrent facts consumed.

### PlaysVideo product reference

Local PlaysVideo revision
`710323343f07487ba228165eab127174d949b4e4` is the primary behavior reference.
The older revision recorded by the draft is not present in the maintained
checkout, so activation re-audited the exact current source rather than
silently relying on the stale identifier:

- `app/src/folder-provider.ts` gates catalog intake through a conservative,
  case-insensitive video-extension set before parsing metadata.
- `app/src/media-metadata.ts` recognizes named and bare `SxxEyy` and `NxM`
  episode forms, multi-episode endings, season-folder fallback, release-tag
  boundaries, title cleanup, and later movie/provider identity that remains
  out of scope here.
- `tests/unit/app-media-metadata.test.ts` covers named episodes, release tags,
  parent fallback, `1x02`, and movie-title parsing. RSTorrent's independently
  authored corpus additionally covers the draft's multi-episode, resolution
  false-positive, numeric-bound, Unicode, and malformed cases.
- `app/src/catalog-groups.ts` orders television entries by numeric season and
  episode with filename as the final tie.
- `app/src/db.ts` demonstrates the useful distinction among stable local row
  identity, parsed hints, external metadata, playback facts, and disposable
  thumbnail records.

RSTorrent adopts the extension-first, typed-hint, parent-fallback, numeric-sort,
and replaceable-derived-data lessons. It independently implements the Rust
classifier and tests. This first slice intentionally differs by omitting movie
matching, normalized provider keys, persistent parse rows, external metadata,
playback facts, and thumbnails.

### JSTorrent product reference

Local JSTorrent revision
`9895410beeed6aff554053769bd006a3fbd373ef` supplies product-history context:

- `packages/ui/src/tables/FileTable.tsx` uses an explicit extension set before
  offering Watch actions and keeps ordinary file inspection available for all
  rows.
- `packages/client/src/AppContent.tsx` connects recognized video files to
  playback sessions while retaining file index as the source identity.
- `packages/engine/src/streaming/streaming-file-provider.ts` separates a byte
  range session from presentation, but no part of that playback boundary is
  adopted in this tactical.

RSTorrent adopts only the lesson that media convenience is a projection over
stable torrent-file identity. It does not copy JSTorrent's extension/engine
topology, playback session, popup, HTTP stream, or file-action behavior.

### Regex dependency posture

Add `regex` as a direct workspace dependency for `rstorrent-media-catalog`;
the current lock already resolves `regex 1.13.1` transitively. Fixed patterns
are compiled once in lazy process state. The crate's linear-time matching and
RSTorrent's existing path/file bounds avoid backtracking-driven hostile-input
work. No pattern is constructed from metainfo content.

The draft's original `rstorrent-media` crate name is no longer available:
completed Tacticals `138` and `139` use it for the Axum/Tokio HTTP media server,
and that crate depends outward on `rstorrent-session`. Keeping the classifier
in a new runtime-independent crate avoids a dependency cycle and prevents
socket/runtime types from owning deterministic filename semantics.

The direct dependency and independently authored tests are authorized by this
tactical when implementation is activated. Any additional parsing,
normalization, media, database, or UI dependency requires direction.

## Classifier Contract

The conceptual pure API is:

```text
CLASSIFIER_VERSION = 1

classify_video(path_components) -> Option<MediaClassification>

MediaClassification =
  episode {
    series_title_hint: String,
    season_number: u16,
    episode_number: u16,
    ending_episode_number: Option<u16>,
  }
  | unclassified_video
```

Exact Rust names and internal pattern organization may change during
implementation, but these semantics are fixed for the slice.

The initial recognized extension set is the PlaysVideo set observed at the
recorded revision:

```text
.mp4 .mkv .avi .webm .mov .m4v .ts .mts .m2ts
.flv .wmv .ogv .3gp
```

Recognition examines only the final component's final extension and is ASCII
case-insensitive. An extension establishes a video **candidate**, not support
by a future player. A zero-length or skipped recognized file remains a video
candidate with truthful zero/selection state. Padding is rejected before
classification even if its synthetic name has a recognized extension.

Episode parsing accepts, case-insensitively:

- a meaningful title prefix followed by `S<1-2 digits>E<1-3 digits>`;
- a meaningful title prefix followed by `<1-2 digits>x<1-3 digits>`;
- a second episode expressed as `E08`, `EP08`, or bare `-08`; and
- a bare episode code when a nearest meaningful parent supplies the title.

Spaces, dots, underscores, and hyphens may delimit title and episode tokens.
Parent lookup skips `Season <n>`, `Series <n>`, `S<n>`, `Special`, and
`Specials` components. Title cleanup replaces dots and underscores with
spaces, collapses repeated whitespace, trims bracket/separator noise, and
preserves the remaining Unicode text. It performs no locale-sensitive
transliteration or external-title normalization.

Season zero is valid. Parsed numbers must fit the stated digit bounds.
`ending_episode_number` is retained only when it is greater than or equal to
the starting episode. A bare ending must not consume a following resolution
tag such as `720p`. If any required title or number is absent or invalid, the
recognized file becomes `unclassified_video`; it is never dropped merely
because episode parsing failed.

Named samples, trailers, extras, and multiple files claiming the same episode
remain separate media items. This slice does not guess that a small file is a
sample, deduplicate releases, or select a preferred version.

## Derived Catalog Ownership And Caching

```text
verified immutable metainfo
  -> rstorrent-media-catalog classification, at most once per retained torrent
  -> Arc<DerivedMediaCatalog> owned by the application ViewHub TorrentModel

FileProgressModel authoritative rows
  -> selection / Done / Verified changes
  -> join only matching media file indexes
  -> MediaItemView upserts

ViewSpec::TorrentMedia interest
  -> existing leased view-set owner
  -> snapshot / coalesced keyed patches / removal / reset
  -> generated client reducer
  -> Zustand mediaByTorrent
  -> LibraryTorrentDetail / VirtualMediaList
```

`rstorrent-media-catalog` owns no cache, task, global torrent registry,
database, file handle, metainfo parser, or view DTO. Its fixed compiled regex
values are process-static implementation detail; classification itself is an
ordinary pure function.

The application `TorrentModel` owns one immutable catalog, shared by reference,
beside its existing `FileProgressModel`. The catalog contains only recognized
file indexes and classifications; file path, length, selection, and progress
remain authoritative in the file model and are not independently mutable media
state.

The catalog is created when a torrent first gains verified metainfo in the
ViewHub. Durable refreshes with the same torrent ID, file-selection changes,
piece progress, view open/close, and engine-generation replacement reuse the
same allocation and must not invoke the classifier again. Torrent removal drops
the catalog. Application restart deliberately recomputes it from retained
verified metainfo.

No SQLite table, schema version, migration, disk cache, background worker,
timer, channel, or cleanup task is added. A test-only classification counter or
equivalent identity assertion must prove reuse without becoming production
observability state.

## Application View Contract

Add capability `torrent_media` and `ViewSpec::TorrentMedia { view_id,
torrent_id, delivery }`. The conceptual generated contract is:

```text
MediaCatalogState = metadata_pending | available | torrent_missing

MediaRoleView =
  episode {
    series_title_hint: String,
    season_number: u16,
    episode_number: u16,
    ending_episode_number: Option<u16>,
  }
  | unclassified_video

MediaItemView {
    media_id: String,                 // decimal file index within this torrent
    file_index: u32,
    path: Vec<String>,
    extension: String,                // lowercase, without the leading dot
    length_bytes: decimal u64 string,
    selection: high | normal | skipped,
    done_bytes: decimal u64 string,
    verified_bytes: decimal u64 string,
    media_availability: MediaFileAvailability,
    role: MediaRoleView,
}

ViewSnapshot::Media {
    torrent_id: String,
    state: MediaCatalogState,
    total_non_padding_files: u32,
    items: Vec<MediaItemView>,
}

ViewPatch::Media {
    torrent_id: String,
    upsert: Vec<MediaItemView>,
    removed: Vec<media_id>,
}
```

The selected-torrent snapshot carries `torrent_id`; row identity is therefore
the file-index string used by the existing Files projection. Its globally
stable interpretation is the pair `(torrent_id, file_index)`. A later Library
projection may carry both parts on each row without changing that identity.

`metadata_pending` and `torrent_missing` contain no items and report zero total
files. `available` may be empty and then means that verified metainfo contains
no recognized video; `total_non_padding_files` still lets the client explain
and enter the All files fallback without leasing Files speculatively. Every
emitted item maps to one non-padding File row with the same index, path,
length, selection, Done, Verified, and media-availability values. Missing
required file state is an internal error rather than a fabricated zero or
silently omitted recognized item.

The metadata-pending to available transition, classifier-version/catalog
replacement, and torrent appearance/disappearance require a coherent fresh
snapshot. Ordinary progress and selection changes use complete-row keyed
upserts only for affected recognized file indexes. Changes to non-media files
produce no Media patch. Repeated upserts coalesce to the newest complete row;
removal wins over an earlier pending upsert.

Decimal byte strings retain the canonical validation and exact comparison
rules already established by Files. The browser additionally validates unique
IDs/indexes, lowercase recognized extension, selection presence, Done and
Verified not exceeding length, the closed media-availability value, episode-
number relationships, total-file bounds, and the role's closed field
combinations.

## Resource Bounds And Failure Policy

- Preserve `MAX_FILES = 4096`, `MAX_PATH_COMPONENTS = 32`,
  `MAX_PATH_COMPONENT_LENGTH = 255`, `MAX_PATH_LENGTH = 4096`, and the current
  1 MiB bencode/metadata input bound. This slice raises no admission limit.
- Classifier work is one linear pass across at most 4,096 accepted files and
  only fixed patterns over already bounded UTF-8 paths. No recursive parser,
  user-constructed pattern, unbounded candidate list, or retry exists.
- A catalog contains at most one item per non-padding metainfo file. It must not
  truncate recognized items or describe a partial result as complete.
- Parsed title output cannot exceed its bounded source component. Cleaning may
  reduce content but must not amplify it beyond the source byte length.
- The full legal Media snapshot must fit the existing 16 MiB coherent view-set
  snapshot limit. Measure a valid 4,096-video long-path fixture; do not raise
  the shared limit or add pagination in this tactical.
- Ordinary Media patch retention uses the existing bounded view-set queue and
  requests a minimum delivery interval of 250 ms, matching Files. Repeated
  progress changes coalesce by media ID rather than enqueueing per block.
- Retain one immutable classification allocation per recognized file, not one
  per subscriber, view set, durable refresh, or progress update. Record the
  4,096-file retained-byte high water and classification duration in debug
  evidence without turning them into release performance claims.
- The media list must render visible rows plus bounded overscan. A 4,096-item
  scenario must keep visible DOM count independent of logical item count and
  report browser heap and long-task observations proportionately to Tactical
  041.
- A classifier bug or internal join inconsistency fails the Media projection
  explicitly and diagnostically; it must not fail torrent download, mutate file
  selection, or make the torrent erroneous.

## Frontend Presentation Contract

Library has two ephemeral substates: collection and one torrent detail.
Activating a collection card establishes the shared current torrent and opens
that source's detail in one action. The detail is torrent-scoped and read-only,
with explicit Back and Open in Workbench actions. Returning preserves the
Library category and virtual-grid scroll offset. Escape and an owned same-
document browser-history entry perform the same Back transition. Leaving
Library or removing the target closes the detail and releases its projection.
No route dependency, durable deep link, or persisted open torrent is added.

The detail has Media and All files choices. Media is the automatic initial
choice when at least one recognized video exists. An available empty media
catalog automatically and truthfully falls back to All files while saying that
no recognized videos were found. Only visible Media requests `torrent_media`;
only visible All files requests the existing `torrent_files`. Workbench keeps
its independent active detail tab and projection rules.

The initial media row displays:

- a formatted episode badge such as `S01E02` or `S01E02–E03` when typed hints
  exist;
- the series-title hint for an episode, otherwise `Video` as a small category
  label;
- the exact filename as the primary label and folder path as bounded secondary
  context;
- formatted size and selection (`High`, `Normal`, or `Skip`);
- one truthful progress treatment that exposes both Done and Verified values
  to sighted and assistive-technology users; and
- `Downloaded` only when the existing per-file availability and full verified
  count establish complete offline content. Other states remain downloading,
  checking, skipped, or unavailable rather than inheriting torrent-wide
  completion.

No poster-shaped empty frame, generated artwork, Play icon, row action,
duration, resolution, codec, or watched state appears. Rows are list content
rather than buttons. Text truncation retains the complete bounded value through
accessible naming or native title where needed. The existing collection
initial/gradient remains explicitly generated placeholder art; phone reduces
it to a small landscape tile rather than devoting most of a card to it.

Frontend ordering is stable and presentation-owned:

1. episode-classified items before unclassified videos;
2. case-insensitive series-title hint;
3. numeric season;
4. numeric starting episode;
5. numeric ending episode, with no ending before a larger ending for the same
   start;
6. case-insensitive full relative path; and
7. file index as the final stable tie.

Typed episode numbers, not display strings, drive ordering. Sorting does not
mutate backend order or claim array position as application truth. A future
Library can apply a different grouping without reparsing filenames.

Wide detail uses a bounded summary column beside the virtualized media/file
list. Compact and phone detail stack the same summary above one list; phone
rows collapse table-like fields into a label/status line plus bounded filename,
folder, size, and progress. No horizontal scrolling is required for the
default Media view. Interface size, Light/Dark/Auto, reduced motion, zoom, long
filenames, mixed Unicode, focus restoration, and keyboard/assistive navigation
retain the existing presentation requirements.

## Shape-Changing Edge Cases

- verified metadata arrives while Media is visible, while another tab is
  visible, or after a view lease expires;
- single-file and nested multi-file torrents, root-level episode names, bare
  episode names, season/specials folders, and multiple show names in one
  torrent;
- season zero, one/two-digit seasons, one/three-digit episodes, multi-episode
  ranges, duplicate episode claims, and reverse invalid ranges;
- dot, space, underscore, and hyphen separators; mixed case; bracket noise;
  Unicode titles; filenames containing only an episode token; and empty
  cleaned title prefixes;
- release strings containing `720p`, `1080p`, `2160p`, years, codec tags, group
  suffixes, or digits unrelated to episode identity;
- uppercase recognized extensions, several dots, no extension, trailing dot,
  hidden files, zero-length video candidates, padding with a video-looking
  name, and non-media sidecars;
- skipped media with verified boundary bytes, selection changes during
  transfer, hash-failure Done regression, restart Verified rebuild, complete
  files, and removal;
- no recognized media, one item, all 4,096 files recognized, and a mixture in
  which only one row is media;
- unsupported older server, malformed new role fields, disconnected/stale
  view, queue reset, lease recovery, Media/All files switch, and torrent
  switch; and
- a classifier failure must remain isolated from torrent lifecycle and Files.

## Implementation Stages And Intermediate Gates

1. **Pure media crate.** Add the crate, fixed extension gate, compiled patterns,
   title cleanup, episode values, classifier version, and adversarial table
   tests. Gate on all stable filename scenarios and a bounded 4,096-path run
   before touching application DTOs.
2. **Application ownership.** Build and retain one immutable per-torrent
   derived catalog at the ViewHub boundary, reuse it across durable/progress/
   selection changes, and join existing File rows without duplicating progress
   authority. Gate on allocation reuse, removal/restart rebuild, and join-error
   isolation tests.
3. **View contract.** Add projection/capability/spec, coherent snapshots,
   coalesced keyed patches, reset/removal behavior, generated artifacts, strict
   semantic validation, and snapshot-size evidence. Rust and contract tests
   gate frontend work.
4. **Pure web model.** Add desired Library-detail interest, reducer/store
   materialization, ordered removal/eviction, navigation repair, and the stable
   typed comparator. Gate on Vitest without React or a server.
5. **Library detail presentation.** Add card-to-detail activation, Back/history/
   focus behavior, Media/All files leasing, responsive virtual rows, honest
   states, compact phone collection placeholders, a permanent named scenario,
   and accessibility/scale coverage. Keep every visual element read-only and
   free of playback/artwork claims.
6. **Controlled live proof.** Drive the production web build against a
   controlled multi-file libtorrent seed containing ordered/misordered episode
   names, an unclassified video, and non-media sidecars. Observe metadata
   pending, catalog arrival, numeric order, progress, completion, detail/filter
   eviction, lease recovery, exact final payloads, and joined cleanup without a
   visible client.
7. **Closure.** Run proportional Rust/web/Tauri/Android generated-contract
   gates, record exact duration/size/memory/DOM evidence, update owning topics
   and the tactical index, remove temporary artifacts, and commit only files in
   this slice.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure classifier | Extension gate, all named/bare/multi-episode forms, false-positive release tags, parent fallback, Unicode/title cleanup, numeric bounds, deterministic output, and 4,096-path execution |
| Application model | One catalog allocation per retained torrent/metainfo, reuse across refresh/progress/selection/view interest, removal/restart rebuild, exact file-progress join, and isolated classifier/join failure |
| Contract | Metadata-pending/available/empty/missing snapshots, unique bounded rows, recognized-only contents, keyed coalescing/removal, non-media no-op patches, reset/lease recovery, generated types/schema/validators, and 16 MiB bound |
| Web model | Strict semantic rejection, exact decimal mapping, typed stable episode sort, keyed reducer, detail/filter-specific leasing, ordered eviction, unsupported/stale/reset recovery, and no hidden Files dependency |
| Presentation | Card-to-detail activation, Back/history/focus restoration, Media/All files choice, read-only row content, honest empty/loading/error states, wide/compact/phone layout, themes/interface sizes, long/Unicode names, progress accessibility, bounded DOM, and empty serious/critical Axe findings |
| Controlled live | Headless metadata-to-completion catalog with episodes, unclassified video and sidecars; exact ordering/progress, detail/filter eviction, lease recovery, payload equality, and joined cleanup |
| Platform/repository | Generated-contract drift check, Rust formatting, warning-denying workspace Clippy, workspace tests, web typecheck/tests/build/CSP/Playwright, and proportional Tauri plus Android/UniFFI compilation |

No public network, visible application, emulator, physical device, HTTP media
server, media fixture download, or external metadata request is authorized or
required. The controlled fixture is independently generated from short test
bytes and contains no copied media.

## Implemented Result And Evidence

The completed slice adds the pure `rstorrent-media-catalog` crate with one
compiled, versioned classifier. Its explicit case-insensitive extension gate
and conservative `SxxExx`, multi-episode, and `NxM` parsing return typed hints
without probing payload bytes or inventing a title for a bare episode. Table,
adversarial, Unicode, false-positive, boundary, and 4,096-path tests pass.

`ViewHub` now retains one immutable `DerivedMediaCatalog` beside each verified
file-progress model. It reuses that classification across progress,
availability, priority, and view-interest changes, then joins the authoritative
file rows into the separately leased `torrent_media` snapshot and complete-row
patch. Removal and generation replacement discard the cache. Generated Rust,
TypeScript, JSON Schema, Kotlin/UniFFI, and Swift/UniFFI shapes plus all
first-party reducers remain exhaustive. A worst-case 4,096-row snapshot is
1,901,592 encoded bytes and 1,901,762 bytes including the retained view-set
envelope, below the existing 16 MiB snapshot ceiling.

Library cards now open an ephemeral same-document detail. Recognized videos
are the default when present and sort by typed series, season, starting episode,
ending episode, then stable file index; **All files** leases the existing Files
projection. Rows show the exact source name and folder, length, selection,
Done, Verified, and media availability. Only `verified == length` is labelled
**Downloaded**. Back, Escape, history, removal repair, filter changes, focus
return, and retained collection scroll are covered. Wide layout uses a summary
beside the virtual list, while compact and phone layouts stack without
horizontal overflow. Generated gradient/initial placeholders remain plainly
synthetic; thumbnails, artwork, and playback did not land.

The permanent `media-library` scenario contains six deliberately misordered
videos across two seasons plus image and NFO sidecars and complete, partial,
skipped, and unavailable states. Its browser checks prove numeric ordering,
Media/All files filtering, wide and 390-by-844 layouts, focus restoration,
zero serious/critical Axe findings, and no horizontal overflow. The 4,096-file
scale scenario yields 3,003 recognized videos while mounting 19 media rows and
421 total DOM elements; the observed Chromium heap was 53,192,260 bytes on the
development host.

The production-build controlled proof used a delayed loopback tracker and a
pinned libtorrent `2.0.13` peer with eight independently generated files. It
observed metadata-pending before catalog arrival, six Media rows, all eight
Files rows, Media/Files lease switches and eviction, one application-view
upgrade, zero semantic HTTP calls, zero serious/critical Axe findings, joined
shutdown, and exact cleanup. The 879-byte metainfo described 14 pieces; the
payload SHA-1 was `4caa9dfc2a7f691fd910e069b91e4e70d3118c6a`, and the
run completed in 33.570 seconds.

Recorded closure gates pass:

- `cargo fmt --all -- --check`, warning-denying workspace Clippy, and all
  workspace tests;
- generated-contract drift, web typecheck, 333 passing Vitest tests with two
  opt-in skips, production build/CSP scan, and 36 passing Playwright cases with
  14 opt-in cases skipped, followed by the focused scale-instrumented rerun;
- `clients/android/build.sh` for both retained ABIs, Android
  `testDebugUnitTest`, and generated Kotlin reducer coverage; and
- release `rstorrent-ios` compilation plus generated Swift binding inspection
  on the available Linux host.

## Stopping Condition

This slice is complete when verified metainfo produces one cached,
runtime-independent derived video catalog without touching torrent-engine
semantics; the separately leased generated `torrent_media` view carries exact
typed episode hints and existing file progress/availability; Library card
activation opens a responsive content detail whose default Media list shows
only recognized videos in numeric episode order and whose explicit All files
fallback leases the existing file view; Back/history/focus, metadata, empty,
stale, reset, removal, and recovery states are truthful; the
controlled headless proof and proportional repository/platform gates pass;
actual evidence is recorded in this tactical and the owning topics; and no
thumbnail, playback UI, HTTP/server change, external metadata, Library-wide
media aggregation, or persistent media state has landed.

## Escalation Contract

Explicit user direction activated this document. Ordinary module extraction,
the new pure crate, the direct `regex` dependency, exact internal names,
generated-contract changes, immutable-cache integration, same-document detail
history, deterministic fixtures, demo data, responsive list/collection styling,
test-harness extension, same-boundary bug fixes, and conservative tightening
of declared limits are in scope.

Stop for direction if evidence requires persistent media rows, a classifier
that probes payload bytes, movie/provider identity policy, a different
extension set with meaningful compatibility implications, server-side sorting
or pagination, a dependency beyond `regex`, a new application command, a file
data plane or HTTP-listener change, thumbnails, playback presentation,
scheduling changes, Library-wide item aggregation, Android presentation,
public traffic, or visible/physical client work.

## Next Boundary

After this slice, use real detail/catalog evidence to choose one independent
follow-up: aggregate derived items directly into the top-level Library, connect
existing verified media capabilities to playback presentation, or generate and
cache thumbnails in each client. None is implied by completing the detail.
