# Tactical 154: iOS Truthful Progress And System Preview

Status: **Complete on 2026-08-13.** Truthful completion, direct Quick Look,
simulator and repository gates, signed physical installation, the real Big
Buck Bunny public-swarm playback flow, and exact cleanup pass. Stage 4 v2
magnet/hash-exchange planning resumes as the authoritative Tactical `155`.

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

### Implementation

`TorrentPresentation.swift` now derives one explicit published-completion
fact from `TorrentState.complete` and `StorageState.published`. Only that fact
returns `1.0`; all byte- or piece-derived nonterminal fractions are finite,
bounded, and capped at `0.99`. Text formatting separately maps every fraction
below one to at most `99%`. The Active and Finished filters consume the same
published-completion fact rather than independently interpreting rounded
progress or the display status.

This deliberately leaves wanted-piece policy in Rust. A selective torrent can
reach `Complete` without its global verified-piece count equaling its global
piece count, while `AwaitingPublication` with zero remaining bytes cannot be
presented as complete.

`SystemFilePreview.swift` supplies one `QLPreviewController` inside a system
navigation controller with Done plus Quick Look's own actions and sharing. The
detail view's full-screen cover captures the existing `ShareableFileLease` for
the preview lifetime and releases it on disappearance; the lease's idempotent
release and deinitialization remain the teardown safety net. The prior generic
`UIActivityViewController` first step is removed. No generated application
contract, dependency, entitlement, payload owner, or decoder changed.

### Deterministic and build evidence

Eight focused simulator tests pass. They cover the exact 1,055-piece
awaiting-publication/zero-remaining case at 99%, complete-but-unpublished,
canonical complete/published, selective-completion geometry, ordinary
rounding near 100%, malformed and out-of-range byte counts, nonfinite display
input, Quick Look's one-item exact-URL data source, and its dismissal callback.
The complete `RSTorrentTests` bundle also passes on an iOS 26.5 iPhone
simulator with signing disabled.

The following proportional repository gates pass:

- `cargo fmt --all -- --check`;
- `cargo clippy -p rstorrent-ios --all-targets -- -D warnings`;
- `cargo test -p rstorrent-ios` with three tests passed;
- focused and complete iOS simulator unit-test invocations; and
- `git diff --check`.

The application boundary did not change, so generated web/Kotlin, React, and
Android packaging gates are inapplicable. A generic device build then signed
and installed the exact development app without storing signing or device
values in the repository.

The signed build/install and UI proof used consecutive exclusive
`machine-control` leases. A rejected selector ended the post-install session
before magnet intake or torrent mutation; the complete real-swarm flow then
ran in one fresh exclusive transaction against that exact installed product.
No device mutation ran outside a testbed lease.

### Physical public-swarm and playback evidence

The attached wired iPhone passed host, Xcode, signing, runner, Developer Mode,
connection, and unlock readiness. The installed app restored the qualified
external root as healthy and began with zero torrents.

The exact `big-buck-bunny` magnet from `tests/live/torrents.json`, with v1 info
hash `dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c`, ran against the changing
public swarm. Samples progressed from 1% with 32 peers to 53% at 15.7 MB/s,
then reached `100%` only with Seeding. Detail independently reported
1,055/1,055 pieces and Published storage. Files reported the 140-byte subtitle,
276.1 MB MP4, and 310 KB poster as Available.

One tap on **Open using** changed directly from the RSTorrent file menu to
Apple Quick Look's `Video` surface in the RSTorrent process, with native Done,
Actions, Share, seek, speed, audio, and play/pause controls. No activity sheet,
Files navigation, or embedded RSTorrent player intervened. The system player's
elapsed position advanced from 1:46 to 2:10, and two independently inspected
captures contained different decoded frames.

Quick Look dismissed back to the torrent's Files tab. Managed removal with
downloaded-file deletion returned the app to zero torrents; Apple Files then
independently showed the qualified selected folder with zero items. RSTorrent,
Files, the automation runner, the signed temporary build, and both captures
were terminated or removed. No device identifier, selected path, bookmark,
signing value, peer endpoint, screenshot, recording, or payload remains in the
repository.

Every stopping condition passes. iCloud and identified providers, embedded or
progressive playback, incomplete-file preview, indefinite background work,
migration, and public distribution remain outside the support claim.
