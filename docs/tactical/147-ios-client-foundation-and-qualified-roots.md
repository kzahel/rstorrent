# Tactical 147: iOS Client Foundation And Qualified Roots

Status: Complete on 2026-08-13. Maintainer direction superseded Tactical `145`
as the single current **Now** during execution and superseded Tactical `123`'s
app-owned-only product decision. Tactical `123` remains the historical physical
evidence for why iCloud must be rejected and why provider identity cannot be
inferred from volume flags alone.

Topics: `product-direction`, `product-surfaces-and-migration`,
`client-surfaces`, `download-roots`, `client-persistence`,
`application-control`, `application-view-api`, `capability-readiness`

Dependencies: completed Tacticals [`116`](116-platform-storage-coherence-and-ios-feasibility.md),
[`120`](120-per-torrent-trusting-fast-resume.md),
[`123`](123-ios-on-device-root-persistence-and-recovery.md), and
[`143`](143-dual-identity-and-persistence-foundation.md) provide the common
storage seam, physical Apple evidence, conservative restart policy, and opaque
torrent identity. The first-party JSTorrent sibling at exact revision
`9895410beeed6aff554053769bd006a3fbd373ef` supplies the existing iOS product
and folder-selection behavior.

## Decision And Desired Outcome

Create one maintained `clients/ios` SwiftUI application that loads the Rust
application service in-process through generated Swift UniFFI bindings. Rust
owns SQLite, torrent state, peer TCP/UDP, hashing, scheduling, storage buffers,
and payload I/O. Swift owns the Apple application lifecycle, root picker,
bookmark and security-scope state, file coordination, notifications, and
presentation.

The maintained project uses XcodeGen, targets iOS/iPadOS 16 or later, and uses
a development bundle identity distinct from the existing JSTorrent product so
device testing cannot overwrite it. iOS 26-only continued-processing behavior
is availability-gated and belongs to Tactical `149`.

The app initially registers its user-visible app Documents directory as one
path root and may register user-selected qualified external folders as distinct
stable platform roots. Changing the default affects future torrents only.
Existing torrents never silently move or fall back to Documents.

## External-Root Policy

The system folder picker is required product behavior. Selection is admitted
only when all of these conditions hold:

- the result is one file URL, is a directory, is not a symlink, and does not
  overlap another registered root;
- `isUbiquitousItem == false`, `volumeIsLocal == true`, and
  `volumeIsInternal == true`; missing or conflicting values reject it;
- a successfully returned File Provider identity rejects the root because the
  initial product does not support identified provider-backed payload storage;
- a File Provider lookup failure may proceed only because the repeated
  physical **On My iPhone** control in Tactical `123` produced that exact
  result, and only after the selected empty folder passes the complete bounded
  Rust capability qualification below; and
- the same build physically rejects an iCloud selection as ubiquitous before
  bookmark persistence, root registration, qualification, or mutation.

This intentionally replaces Tactical `123`'s rule that provider-lookup failure
is always unclassifiable. It does not claim that public Apple APIs can prove the
absence of every conceivable third-party locally materialized provider. The
support wording is therefore **qualified on-device folders; iCloud and
positively identified providers rejected**, not “only On My iPhone provider
identity.”

Qualification creates one hidden run-owned workspace, then uses Rust to prove
bounded positional create/write/read, truncate, sync, SHA-1, no-replace rename,
close/reopen, observation, and exact deletion. Failure leaves no root record or
workspace. The picker clearly states that the selected folder will contain
torrent payload.

## Stopping Condition

This tactical is complete only when:

1. `clients/ios` is a maintained generated Xcode project with a signed physical
   build, simulator tests, an isolated development bundle ID, and no embedded
   signing team, profile, device identifier, or credential.
2. A focused `rstorrent-ios` static library exposes the application service,
   subscriptions, commands, raw `.torrent` intake, root health, and platform
   storage requests through UniFFI-generated Swift. Android retains its
   generated boundary and both established ABIs.
3. A durable iOS profile lives in Application Support. Swift's platform root
   registry stores bounded labels, opaque stable IDs, generations, and minimal
   bookmark bytes; portable SQLite stores only stable root IDs and platform
   capability kind.
4. App-owned Documents and every qualified selected root reopen after ordinary
   termination and process death. A failed bookmark, scope, classification,
   coordination, descriptor, or observation produces unavailable/repair state
   without fallback or root-ID replacement.
5. Every selected-root descriptor is opened inside `NSFileCoordinator`, remains
   covered by that exact coordinated lease while Rust owns or caches the file,
   and produces a bounded release event when the Rust pool drops it. Security
   scopes, coordinators, leases, descriptors, requests, and tasks all return to
   zero after shutdown.
6. The selected root passes initial qualification, application root probing,
   controlled exact torrent download/publication, restart, Force recheck,
   complete-file read, removal, and cleanup on the attached physical iPhone.
7. The same physical build records a non-mutating iCloud rejection and repeats
   the qualified local result without retaining provider IDs, paths, bookmark
   bytes, or device identifiers in repository evidence.
8. App-owned and external-root tests cover cancellation, stale generations,
   no-replace conflicts, permission loss, repair, process death at durability
   boundaries, and pool eviction without false success or payload leakage
   through Swift.
9. Rust, generated-client, Swift, simulator, Android compatibility, and
   physical-device gates pass, and the tactical plus living topics record the
   exact evidence and remaining limitations.

## Architecture And Ownership

```text
Swift AppLifecycleOwner
  -> one iOSApplicationClient / Rust ApplicationService generation
  -> one Swift RootCapabilityRegistry
       -> picker and minimal bookmark persistence
       -> balanced security-scope leases
       -> bounded NSFileCoordinator workers
            -> duplicated descriptor into Rust StorageFilePool
            <- explicit Rust handle-release event
  -> one subscription/presentation repository
```

The Rust platform-storage response gains an optional opaque release identity.
`StorageFileHandle` owns its release guard and sends one nonblocking release
when the final pooled handle drops. Android completes files without such a
guard and keeps existing behavior. The iOS wrapper exposes separate bounded
request and release streams so shutdown can cancel pending acquisition, join
the application service, drain handle releases, then terminate coordinator
workers in that order.

Pure storage keys, observations, release state, and generation checks remain
independent of Tokio, Swift, Foundation, URLs, bookmarks, coordinators, and
descriptors. No payload byte, metainfo body, peer frame, or hash buffer crosses
into a Swift callback.

## Resource Bounds

| Resource | Initial bound |
| --- | ---: |
| Registered iOS roots | 8 |
| Bookmark bytes per selected root | 64 KiB |
| Concurrent platform requests | 16 |
| Coordinated descriptor leases | 8 |
| iOS storage-file pool entries | 8 |
| Concurrent root qualification | 1 |
| Qualification workspace files | 8 |
| Root label / failure detail | 256 / 1,024 UTF-8 bytes |
| Raw `.torrent` intake | existing 64 MiB application limit |
| Retained URLs/provider IDs in portable evidence | 0 |

The iOS pool limit is deliberately smaller than desktop/Android's 40-handle
limit because each external handle retains a coordination worker. The common
session constructor must accept this explicit platform limit without changing
the desktop or Android default.

## Shape-Changing Cases

- picker cancellation, multiple results, non-file URLs, symlinks, nested or
  overlapping roots, and duplicate bookmark selections;
- nil/conflicting resource values, ubiquitous iCloud, positively identified
  providers, provider lookup failure/timeout, and classification changes after
  bookmark restore;
- scope failure before observation, stale bookmark refresh under scope,
  coordinator failure, accessor URL substitution, and repair to a new folder;
- release before completion, completion before release, duplicate/unknown
  release IDs, pool eviction, mode upgrade, root invalidation, and shutdown
  with pending opens;
- process death before root record commit, after qualification, during payload
  sync, after SQLite commit, during publication, and before cleanup; and
- unavailable roots beside healthy torrents, default-root changes, removal of
  unused roots, and refusal to remove roots still referenced by torrents.

## Implementation Stages And Gates

1. Add the tactical, queue transition, project skeleton, and provenance record.
2. Add platform-file release guards and the iOS UniFFI crate; preserve full
   Rust and Android build/test gates.
3. Implement the Swift root registry, classification, qualification, bookmark,
   scope, coordinator, descriptor, repair, and shutdown owners with unit tests.
4. Open the real durable application service, drive root requests/releases,
   and prove app-owned plus selected-root behavior in simulator builds.
5. Build/sign/install only the explicit development app, use one transactional
   machine-control session for mutating physical flows, and run local/iCloud,
   controlled transfer, restart, recheck, removal, and cleanup evidence.
6. Reconcile all evidence and support wording before marking the tactical
   complete and activating Tactical `148`.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure Rust | optional release-guard state, duplicate/late release, pool eviction/invalidation, bounded queues, shutdown ordering |
| Rust application | durable profile, platform root probe, controlled transfer, restart/recheck/publication/removal, exact cleanup |
| Swift unit | eligibility truth table, registry bounds/corruption, bookmark generations, overlap, release routing, cancellation |
| Simulator/build | generated Swift compile, app tests, app-owned smoke, no embedded signing values |
| Android compatibility | both ABIs, UniFFI regeneration, Gradle unit/lint/assemble gates |
| Physical iPhone | local picker qualification, iCloud rejection, exact torrent transfer, restart, Force, file read, repair/unavailable, removal, resource drain |
| Repository | formatting, clippy, workspace tests, affected web generation/typecheck/tests, `git diff --check` |

## Non-Goals And Escalation

This slice does not import the complete JSTorrent presentation, implement app
Search, support iCloud/identified providers, move payload through Swift, add a
daemon, publish to TestFlight/App Store, migrate legacy JSTorrent state, or
promise indefinite background downloading/seeding.

Physical build/install/launch, semantic UI automation, system folder-picker
interaction, non-mutating iCloud classification, controlled run-owned files,
process termination/relaunch, and cleanup on the attached test iPhone are
explicitly authorized. Protected authentication and signing-account changes
remain human-only. Stop only for a materially different provider policy, a new
entitlement/dependency/license posture, payload callbacks, destructive
unrelated data handling, or external publication.

## Execution Record

### 2026-08-13 completion

- The workspace now contains the focused `rstorrent-ios` static library and a
  generated iOS 16+ SwiftUI/XcodeGen project. The signed development build uses
  a separate bundle identity supplied without a repository signing team,
  profile, account, or device identifier.
- The common file pool now accepts an optional platform release identity. A
  bounded, acknowledged release stream keeps the exact Swift coordinator and
  security-scope lease alive until the final pooled Rust handle drops. The iOS
  application selects an eight-entry pool while existing products retain 40.
- Swift owns the bounded root registry, bookmark restoration, eligibility
  checks, destructive Rust qualification, coordination workers, descriptor
  transfer, release acknowledgement, and application-service shutdown.
- Focused Rust tests pass for the iOS boundary and final-handle release. The
  generated project builds unsigned for the simulator, and five Swift unit
  tests pass there for eligibility and persistent-registry bounds.
- The development app builds signed, installs, and opens the real durable Rust
  service on the attached iPhone. Through the system picker, a run-owned
  `On My iPhone` folder passed qualification, persisted, survived the service
  restart, and reported `Qualified on-device folder`. Selecting the empty
  iCloud Drive root in the same build returned the non-mutating rejection
  `iCloud folders are not supported. Choose a folder under On My iPhone.`; the
  previously qualified root remained registered and ready.

- A controlled one-peer transfer published the exact 2 MiB fixture beneath the
  qualified selected root. The independent seed recorded exactly 2,097,152
  payload bytes and a peer high-water of one. The app reached Seeding, retained
  the torrent and root over process restart, returned to 100% after Force
  recheck, exposed the complete file, then removed the catalog and managed
  payload. Files independently reported the selected folder empty.
- Permanently removing the selected run-owned folder made the next process
  report the exact unavailable reason and Repair action without fallback.
  Repair selected a new run-owned On My iPhone folder, preserved the opaque
  root ID, advanced its generation, resumed affected intent, and requalified
  it. The final physical root remains empty and healthy.
- Root eligibility, bounded registry/corruption, stable repair, namespace, and
  release-guard tests pass. The Rust workspace, generated Swift and Kotlin
  boundaries, both Android ABIs, Gradle unit/lint/package gates, simulator
  suites, and signed physical build all pass at campaign closure. No signing
  identity, bookmark, path, provider identity, or device identifier entered
  repository evidence.

The stopping condition is met. The supported wording is exactly **app-owned
Documents and qualified on-device selected folders; iCloud and positively
identified providers rejected**. This does not widen support to cloud or an
arbitrary File Provider root.
