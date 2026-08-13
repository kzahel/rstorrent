# Tactical 154: iOS Truthful Progress And System Preview

Status: **Active on 2026-08-13.** The maintainer authorized implementation,
commits, and end-to-end validation on the attached physical iPhone with the
repository Big Buck Bunny public magnet.

Topics: `client-surfaces`, `capability-readiness`, `download-roots`

Dependencies: completed Tacticals
[`147`](147-ios-client-foundation-and-qualified-roots.md),
[`148`](148-jstorrent-swiftui-product-surface.md),
[`149`](149-ios-lifecycle-recovery-and-distribution-readiness.md), and
[`152`](152-ios-multifile-selected-root-coordination.md) provide the maintained
iOS client, JSTorrent-derived SwiftUI presentation, lifecycle ownership,
selected-root publication, shareable-file lease, and real-swarm baseline this
slice must preserve.

## Motivation And Outcome

The physical Big Buck Bunny run exposed two presentation defects after the
engine and selected-root storage path behaved correctly:

1. transfer byte arithmetic could reach or round to `100%` while the torrent
   remained `AwaitingPublication`, even though the final files were not yet
   available; and
2. **Open using** presented a generic activity sheet, requiring a second app
   choice or manual Files navigation before the user reached Apple's media
   presentation.

Make completion truthful and direct. The iOS list and detail surfaces must
reserve `100%` and the Finished filter for the application service's canonical
complete-and-published state. A completed available file must open in Apple's
system Quick Look/video presentation with one tap while RSTorrent retains the
existing security-scoped lease for the presentation's entire lifetime.

## Stopping Condition

This tactical is complete only when:

1. `AwaitingPublication`, `Staging`, and every other nonterminal state display
   no more than `99%`, even when required remaining bytes are zero or ordinary
   percentage rounding would produce 100.
2. `100%` is returned only when the authoritative torrent state is `Complete`
   and storage is `Published`. The application service already derives that
   state only after all wanted pieces are verified and final ownership exists;
   the client does not duplicate selection or piece-completion policy.
3. Active/Finished filtering consumes the same completion-aware presentation
   value and cannot classify an awaiting-publication torrent as finished.
4. **Open using** acquires the existing complete-file lease and directly
   presents `QLPreviewController`; no in-app decoder, copied payload, Files
   navigation, or generic share sheet is required for initial playback.
5. The exact lease remains alive until system preview dismissal, then releases
   once on dismissal, error, view teardown, or deinitialization through the
   existing idempotent release contract. Quick Look's system controls may
   still offer onward sharing.
6. Focused deterministic Swift tests, simulator build/tests, signed physical
   build/install, and the repository Big Buck Bunny public-swarm playback flow
   pass. Run-owned torrent data and temporary captures are removed exactly.

## Contracts And Invariants

- Rust `TorrentState` and `StorageState` remain the source of truth. Swift may
  calculate a bounded display fraction but may not infer completion from bytes
  alone or change the generated application contract.
- Selective downloads remain valid: `Complete` means every wanted piece is
  verified. Comparing the global verified-piece count with the global piece
  count would incorrectly make skipped-file torrents permanently incomplete.
- A nonterminal display fraction is clamped to `[0, 0.99]`. Invalid byte text
  falls back to bounded verified-piece arithmetic; absent geometry displays
  zero. Text formatting independently prevents rounding a fraction below one
  to `100%`.
- System preview is presentation only. Rust still owns verification,
  publication, file selection, reads/writes, and payload lifetime; Swift
  passes only the URL authorized by `ShareableFileLease`.
- The qualified external-root policy is unchanged. iCloud and positively
  identified providers remain rejected, an unavailable root never falls back,
  and no bookmark, selected path, device identity, peer endpoint, or capture
  enters the repository.
- Quick Look is an Apple framework already in the platform SDK. No third-party
  dependency, entitlement, embedded player, daemon, or payload copy is added.

## Owner And Lifetime Map

```text
Rust ApplicationService
  -> verifies all wanted pieces
  -> publishes final storage
  -> TorrentView(Complete, Published)
       -> iOS presentation fraction may become 1.0

TorrentDetailScreen
  -> AppModel.shareableFile(torrent, file)
       -> ShareableFileLease(security-scoped root, complete file URL)
  -> full-screen QLPreviewController
       -> Apple system video/Quick Look controls
  -> dismiss
       -> release lease exactly once
```

The SwiftUI detail screen owns the presented lease. The Quick Look data source
owns only the URL and dismissal callback; it does not acquire a second scope or
copy file contents. Existing lease deinitialization remains the final safety
net if SwiftUI tears down presentation state unexpectedly.

## Reference Review

The first-party JSTorrent iOS reference at
`~/code/jstorrent/ios/JSTorrent/App/TorrentDetailScreen.swift` uses the same
complete-file handoff intent but currently attempts `openURL` and falls back to
`UIActivityViewController`. RSTorrent adopts the visible **Open using** action
and system-owned presentation intent, but intentionally uses
`QLPreviewController` so the first tap deterministically opens the supported
file inside Apple's preview/video UI without depending on URL routing or a
second activity choice.

Apple's locally installed Quick Look SDK interface supplies
`QLPreviewController` and `QLPreviewControllerDataSource`. This tactical does
not require internet research; the maintainer explicitly deferred search.

## Implementation And Validation

1. Extract a deterministic display-progress function over torrent state,
   storage state, byte totals, and piece counts. Reserve one only for canonical
   complete/published and test byte, fallback, malformed, zero, and rounding
   edges.
2. Add a small Quick Look representable and independently test its one-item
   data source and exact URL. Replace share-sheet state with preview-lease state
   while keeping the lease through cover dismissal.
3. Run focused and full iOS simulator tests plus the unsigned build. Run
   proportional formatting and affected Rust compatibility checks even though
   the application boundary is unchanged.
4. In one exclusive `machine-control` iOS transaction, build and install the
   exact signed app, add the repository Big Buck Bunny magnet against its real
   public swarm, verify 1,055/1,055 and Published/Seeding, and tap **Open
   using** once. Assert Apple Quick Look/video appears directly and playback
   advances, then dismiss and remove the managed torrent data exactly.
5. Reconcile this tactical and owning topics with actual evidence before
   marking complete. Return the displaced Stage 4 v2 magnet/hash-exchange
   planning item as Tactical `155`, without implementing or redesigning it in
   this product slice.

## Non-Goals And Escalation

No embedded player, incomplete-file streaming UI, custom playback controls,
format transcoding, background playback, arbitrary external file support,
iCloud/File Provider relaxation, engine-state change, generated-contract
change, App Store/TestFlight work, or web/Android presentation change is in
scope.

The attached physical iPhone, exact repository public magnet, run-owned files,
signed development install, ordinary playback interaction, and exact cleanup
are authorized by the maintainer. Stop for a new dependency or entitlement,
signing-account or protected-authentication change, destructive action outside
run-owned data, a weakened security-scope invariant, or external publication.

## Execution Record

Pending implementation and validation.
