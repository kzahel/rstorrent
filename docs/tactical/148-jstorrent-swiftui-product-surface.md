# Tactical 148: JSTorrent SwiftUI Product Surface

Status: Decision-complete and queued immediately after Tactical `147` by
explicit maintainer direction on 2026-08-13.

Topics: `client-surfaces`, `product-direction`,
`product-surfaces-and-migration`, `application-control`,
`application-view-api`, `download-roots`, `capability-readiness`

Dependencies: Tactical `147` must first provide the maintained signed app,
durable in-process Rust application service, qualified root lifecycle, and
generated Swift boundary. Completed Tactical `117` supplies the precedent for
adapting a first-party JSTorrent product surface to truthful RSTorrent
semantics without importing its engine architecture.

## Decision And Desired Outcome

Reuse the first-party JSTorrent iOS SwiftUI presentation directly, at sibling
revision `9895410beeed6aff554053769bd006a3fbd373ef`, instead of recreating its
layout by inspection. Preserve its NavigationStack, Library header and cards,
Add and remove sheets, Settings presentation, torrent-detail sections, piece
map, typography, spacing, gestures, symbols, assets, and localization resources
verbatim wherever the RSTorrent application has the same semantic fact.

Replace `AppModel`, `AppSettings`, `EngineController`, JavaScriptCore payload
models, and every `JSTorrentKit` dependency with one RSTorrent-native Swift
application owner and presentation repository. RSTorrent branding follows the
existing Android successor precedent; the development app does not impersonate
or overwrite the existing JSTorrent installation.

Search and search plugins are explicitly deferred. The search screen,
view-model, route, button, strings used only by Search, and network provider do
not enter this app. Unsupported High file priority is omitted rather than
simulated. These are the two deliberate visible departures from the current
JSTorrent source.

## Source Reuse And Provenance

The following first-party Swift files are the import basis:

- `AddTorrentSheet.swift`, `AppLocalization.swift`, `ContentView.swift`,
  `RemoveTorrentSheet.swift`, `SettingsScreen.swift`;
- `TorrentListScreen.swift`, `TorrentRowView.swift`,
  `TorrentDetailScreen.swift`, and `TorrentPresentation.swift`; and
- `Assets.xcassets`, JSON localization resources, `Info.plist` document/magnet
  declarations, and `PrivacyInfo.xcprivacy` where applicable.

The original repository and RSTorrent share the maintainer, copyright holder,
and MIT license. Record the exact imported files and revision in the execution
record. Do not import JavaScriptCore, the bundled TypeScript engine,
`JSTorrentKit` bindings/runtime, Search sources, release signing settings, build
output, or private configuration.

## Stopping Condition

1. The maintained iOS app opens into the recognizable JSTorrent Library and
   uses the imported assets/localizations and view composition, with only the
   recorded RSTorrent model, branding, Search, and unsupported-capability
   deltas.
2. One `IOSPresentationRepository` owns bounded Rust subscriptions, fresh
   snapshots, patches/resets, stale generations, command receipts, and atomic
   `@MainActor` presentation models. Views own no Rust service, descriptors,
   tasks, or engine policy.
3. Library rows truthfully present stable torrent ID, verified name, state,
   progress, rates, peers, and pause/resume/removal behavior for multiple
   torrents. Loading, metadata-pending, queued, checking, unavailable-root,
   error, complete, and empty states are purposeful.
4. Add accepts pasted magnets and one bounded system-selected `.torrent` file,
   uses the established default root, reports duplicate/add outcomes, and
   survives view recreation without serializing bytes into UI state.
5. Torrent detail preserves the existing iOS section order and connects every
   visible Status, Files, Trackers, Peers, and Pieces fact to authoritative
   generated values. Files supports only Normal/Skip plus Download now where
   applicable; Trackers remains read-only.
6. Settings preserves JSTorrent's external-folder selection/reset experience
   while showing qualification, default, availability, repair, and the no-move
   meaning truthfully. iCloud rejection is actionable and no hidden fallback
   occurs.
7. Complete eligible files open/share through a security-scoped coordinated
   platform URL. Incomplete or unavailable content does not become shareable
   from progress inference.
8. Magnet and `.torrent` URL handoff, keyboard, VoiceOver labels, Dynamic Type,
   light/dark, phone/iPad layout, picker cancellation, command failure, and
   focus restoration receive deterministic and physical evidence.
9. Simulator and attached-iPhone automation traverse Library, Add, Settings,
   selected-folder state, every detail section, actions, and cleanup against a
   controlled exact torrent without requiring public swarm state.

## Ownership And Bounds

```text
SwiftUI views
  -> immutable presentation values and semantic intents
  -> IOSPresentationRepository (@MainActor state owner)
       -> bounded subscription tasks / command tasks
       -> IOSApplicationClient (UniFFI)
            -> Rust ApplicationService and engine
       -> RootCapabilityRegistry for platform-only URL actions
```

- One lifetime Library subscription remains open while the application service
  runs. One selected torrent may own its summary and one visible detail
  projection. Leaving detail cancels and joins those tasks.
- Files and Trackers retain one application-bounded page; Swift never collects
  an adversarial complete catalog.
- Command attempts are one per semantic action owner and are fenced by stable
  request identity. Re-rendering cannot replay Add, Remove, or root changes.
- Diagnostics, Search history, JavaScript tick state, and an engine replica do
  not enter the UI models.

## Implementation Stages And Gates

1. Import the bounded source/assets/localizations with provenance and prove the
   shell builds using fixture presentation models.
2. Implement generated-value adapters and the repository; connect Library,
   Add, incoming URLs, actions, and error/empty/loading states.
3. Connect every detail section with bounded interest and paging; add truthful
   file action/open behavior and deterministic piece rendering.
4. Connect Settings to Tactical `147` root ownership; remove Search and every
   unsupported High-priority path.
5. Add Swift tests, accessibility identifiers where they do not alter visual
   presentation, simulator navigation tests, and physical controlled-torrent
   traversal through machine-control.
6. Reconcile the imported-file inventory, screenshots/evidence, and living
   product claims before activating Tactical `149`.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure Swift | reducers/adapters, formatting, snapshots/patches/resets, paging, command fencing, unsupported states |
| SwiftUI | all routes/sections, sheets, picker cancellation, Dynamic Type, light/dark, accessibility labels and focus |
| Rust boundary | every consumed projection/action, raw byte intake, generated-binding drift, no payload callback |
| Simulator | fixture and live app navigation, magnet/file add, settings/root states, incoming URLs |
| Physical iPhone | controlled transfer from Add through publication; all sections/actions; file open/share; screenshots and semantic assertions |
| Repository | Rust baseline, affected web checks, iOS build/tests, Android compatibility, `git diff --check` |

## Non-Goals And Escalation

App Search, search plugins, tracker mutation, High/Low priority, embedded
playback, remote control, legacy JSTorrent state import, general provider/cloud
support, and public release remain out of scope. Ordinary adaptations needed
to map the imported views to existing typed semantics are authorized. Stop for
a new engine/persistence owner, a materially different visual product, a new
third-party dependency, unsupported storage widening, or external publication.

## Execution Record

### 2026-08-13 presentation checkpoint

- Preparatory implementation landed while Tactical `147` remains the sole
  **Now**. The source basis was JSTorrent revision
  `9895410beeed6aff554053769bd006a3fbd373ef`, specifically
  `ios/JSTorrent/App/{AddTorrentSheet,AppLocalization,ContentView,RemoveTorrentSheet,SettingsScreen,TorrentListScreen,TorrentRowView,TorrentDetailScreen,TorrentPresentation}.swift`
  and `ios/JSTorrent/App/Localization/en.json`.
- `AppLocalization.swift` and the English localization catalog are imported
  directly. The remaining views retain JSTorrent's navigation, Library,
  sheet, row, Settings, detail-section, typography, spacing, and gesture
  composition while replacing JavaScriptCore/JSTorrentKit model ownership
  with generated RSTorrent values. The deliberate visible changes are
  RSTorrent branding, omitted Search, omitted unsupported High priority, and
  an honest unavailable upload-rate placeholder where the list projection has
  no such fact.
- One `@MainActor` presentation repository now owns the Library subscription,
  contract-v2 sequence/epoch/revision continuity, resets, and the one visible
  detail projection. Add, pause/resume, file priority/download-now, Force
  recheck, removal, selected-root defaulting, platform publication, and exact
  managed cleanup are typed application commands or explicit platform
  transitions rather than view-owned engine behavior.
- The unsigned generic simulator build passes. Six focused Swift tests and one
  UI test pass, including real application-service startup and Library, Add,
  and Settings traversal. The signed development build installed on the
  attached iPhone and physically showed Ready Library/Add/Settings states plus
  the previously qualified external folder after process restart.

This is not tactical completion. Controlled torrent detail/action traversal,
file handoff/open/share, accessibility/layout variants, and final repository
gates remain outstanding.
