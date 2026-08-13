# Tactical 152: iOS Multifile Selected-Root Coordination

Status: **Active authoritative Now on 2026-08-13.** Explicit maintainer
direction selected end-to-end implementation of this correctness repair and
paused Tactical `151` before its implementation began.

Topics: `download-roots`, `client-surfaces`, `client-persistence`,
`capability-readiness`, `performance-and-live-evidence`

Dependencies: completed Tacticals
[`116`](116-platform-storage-coherence-and-ios-feasibility.md),
[`123`](123-ios-on-device-root-persistence-and-recovery.md),
[`147`](147-ios-client-foundation-and-qualified-roots.md),
[`148`](148-jstorrent-swiftui-product-surface.md), and
[`149`](149-ios-lifecycle-recovery-and-distribution-readiness.md) provide the
platform-storage seam, selected-root policy, maintained Swift client, product
surface, and lifecycle ownership this repair must preserve.

## Motivation And Observed Failure

The maintained iOS app can acquire the repository's real Big Buck Bunny magnet
from a public swarm on the attached physical iPhone, but the three-file
download does not complete or publish. The 276,445,467-byte torrent connected
to 38 peers, reached 15.1 MB/s, and then stopped at 1,052 of 1,055 pieces with:

```text
acquire selected file: platform storage: DeadlineExceeded:
platform storage request exceeded its deadline
```

Pause/resume, selected-root reauthorization, Force recheck, and process
relaunch did not recover it. The product rounded progress to 100% while the
torrent remained Staging; no playable media appeared in the selected folder.
Managed removal returned the selected folder to empty.

The source-backed working diagnosis is coordination granularity. Each
selected-root open currently coordinates the whole selected root and keeps
that accessor alive while Rust owns or caches the descriptor. A later request
for a sibling file coordinates the same root, waits behind the first cached
descriptor, and reaches the existing 30-second platform request deadline.
Tactical `147` physically proved a controlled single-file transfer, so it did
not exercise this sibling-file conflict. A controlled failing regression must
prove this diagnosis before production behavior changes.

## Decision And Desired Outcome

Keep Rust-owned selected-root descriptors covered by balanced security-scope
and `NSFileCoordinator` leases, but narrow each long-lived coordinated lease
from the selected root to the exact payload, part, or metadata file represented
by that descriptor. Different files beneath one root must be able to remain
open concurrently up to the existing pool bound. Opens of the same file remain
serialized where coordination requires it.

Short-lived namespace operations coordinate only the affected URLs or managed
subtree. Qualification may coordinate its run-owned workspace. Publication
coordinates the staging source and final destination only after pooled payload
descriptors are invalidated and released. Removal similarly drains handles
before coordinating deletion of the managed artifact or torrent tree. No
operation holds a long-lived coordinator accessor for the entire user-selected
root.

This repair preserves the accepted qualified on-device folder policy. It does
not add iCloud or positively identified File Provider support, redirect payload
through Swift, weaken path/type checks, or change publication ownership.

## Stopping Condition

This tactical is complete only when:

1. A pre-fix controlled regression holds one selected-root file lease while a
   sibling file is requested and reproduces the current timeout or blocked
   acquisition without relying on the public swarm.
2. Long-lived coordination is scoped to the exact descriptor target; balanced
   security-scope, descriptor, release-ID, worker, and cancellation ownership
   remains intact until Rust drops the final pooled handle.
3. Two sibling files can be acquired and used concurrently while the first
   remains cached, while duplicate opens, eviction, invalidation, cancellation,
   shutdown, and late release remain bounded and deterministic.
4. Missing targets and nested parents are created safely without symlink
   traversal, replacement, implicit merge, or a long-lived selected-root
   accessor. Publication and removal begin only after affected handles drain.
5. A controlled three-file torrent with a piece spanning file boundaries
   downloads, verifies, publishes, survives restart and Force recheck, supports
   a complete-file read, and removes exactly beneath a qualified external root
   on the attached physical iPhone.
6. The exact Big Buck Bunny catalog magnet completes all 1,055 pieces from its
   real public swarm, reaches Published/Seeding, exposes all three files in the
   selected folder, and its MP4 opens and begins playback through the iOS
   system media presentation. No embedded player is required.
7. Public and controlled runs end with exact managed cleanup and zero platform
   requests, cached handles, coordination workers, release leases, security
   scopes, and run-owned files. No device identity, bookmark bytes, selected
   path, or peer endpoint enters repository evidence.
8. Focused Swift, Rust boundary, simulator, signed physical build, and
   proportional repository compatibility gates pass, and the tactical plus
   owning topics reconcile the corrected support claim and evidence.

## Contracts And Invariants

- Stable root IDs, bookmark generations, qualification state, and per-torrent
  root binding do not change. An unavailable root never falls back to app
  Documents or another registered root.
- Relative components are validated beneath the resolved root before any
  coordination or mutation. Coordinator-substituted URLs receive the same
  containment, symlink, and expected-type checks before open.
- One accepted platform response transfers one duplicated descriptor and, for
  selected roots, one opaque release identity. Every accepted release identity
  is acknowledged exactly once after its coordinator worker has terminated.
- Rust remains the owner of payload reads, writes, hashing, synchronization,
  file-pool policy, and torrent state. Swift does not receive payload bytes.
- The selected-root security scope remains balanced for the entire lifetime of
  every descriptor it authorizes. Independent sibling leases may overlap;
  scope accounting and shutdown still return to zero.
- The existing eight-entry iOS storage-file pool, eight coordinated-descriptor
  leases, 16 concurrent platform requests, and 30-second request deadline stay
  unchanged. The fix must remove indefinite exclusion rather than hide it with
  a larger timeout, blind retry, or a one-entry pool.
- Final publication remains atomic and no-replace. External conflict, rename,
  deletion, or type substitution produces a truthful unavailable/conflict
  result rather than success against an unlinked or replaced inode.
- Public-swarm progress is evidence, not trusted completion. Exact metainfo
  geometry, piece verification, publication state, filesystem contents, and
  system playback are asserted independently.

## Owner, Task, Cancellation, And Dependency Map

```text
Swift IOSApplicationLifecycleOwner
  -> RootCapabilityRegistry generation and resolved selected root
  -> bounded PlatformStorageBridge request worker
       -> validate exact relative target
       -> balanced selected-root security scope
       -> exact-target coordinated accessor
            -> duplicate descriptor into Rust
            <- wait for exact opaque release identity
       -> terminate accessor and acknowledge release

Rust ApplicationService / torrent runtime
  -> StorageFilePool (8 iOS entries)
       -> cached descriptor and final-handle release guard
       -> bounded nonblocking release event
  -> invalidate/drain affected handles
  -> request short-lived publication or managed removal
```

No new daemon, socket proxy, payload callback, unbounded task, or second file
owner is introduced. Runtime-independent storage keys, safe relative paths,
release state, and generation checks remain independent of Swift, Foundation,
Tokio tasks, URLs, bookmarks, and descriptors. The existing lifecycle owner
continues to cancel request admission, join Rust, drain releases, and then
close platform owners in that order.

## Missing-Target And Namespace Direction

Before implementation, inspect the locally installed Apple SDK contract for
the exact `NSFileCoordinator` writing options and accessor behavior for a
nonexistent file and nonexistent nested parents. The preferred path is one
long-lived coordinated accessor at the exact validated target, with idempotent
parent creation and open inside the accessor.

If the SDK or a focused test proves that exact-target coordination cannot
admit a missing parent, the implementation may first perform one bounded,
short-lived coordinated namespace preparation at the nearest existing managed
ancestor, then acquire the long-lived exact-target lease. That fallback is
within scope provided it never holds the selected root while a Rust descriptor
is cached and preserves no-replace and containment invariants. A design that
retains only security scope after leaving coordination, or coordinates the
whole root through descriptor lifetime, requires maintainer direction.

## Shape-Changing Cases

- two sibling targets while the first is cached, the same target requested
  twice, read-to-write mode upgrade, all eight pool entries occupied, eviction,
  root invalidation, and release racing request completion;
- absent file, absent nested parents, zero-length file, piece spanning two or
  three files, part-file use, sparse positional writes, and the final piece at
  end of the final file;
- accessor URL substitution, symlink or type replacement before and during
  open, external rename/delete while a descriptor is live, and publication or
  removal requested before handle drain;
- request cancellation before coordination, during accessor acquisition,
  after descriptor duplication, and during release acknowledgement; ordinary
  shutdown and process death at the same boundaries; and
- selected-root repair or generation change while old requests/releases are
  in flight, alongside healthy app-owned and other selected roots.

## Implementation Stages And Gates

1. Add a deterministic Swift regression using the production bridge seam: hold
   file A's release, request sibling file B, and prove the pre-fix request is
   blocked. Add missing-target/nested-parent and same-target controls.
2. Refactor only the coordination target/namespace boundary needed to make
   long-lived opens exact-file operations. Preserve root resolution, security
   scope, descriptor duplication, and release-ledger ownership.
3. Extend Rust platform-file-pool and bridge tests to hold at least three
   selected-root handles, exercise eviction/invalidation/cancellation, and
   prove exact release delivery and acknowledgement without raising limits.
4. Run a controlled multifile application transfer through the production iOS
   service in simulator where meaningful, then on the qualified physical root.
   Include cross-file piece geometry, restart, Force recheck, publication,
   complete-file read, removal, and terminal resource accounting.
5. In one exclusive `machine-control` iOS transaction, install the exact signed
   development app and run the repository Big Buck Bunny catalog magnet against
   the real public swarm. Verify metainfo geometry and piece completion, open
   the published MP4 through Files/system media UI, then remove all run-owned
   torrent data and captures after recording minimized evidence.
6. Run proportional repository gates, append the actual execution record,
   correct Tactical `147` and living-topic evidence without rewriting their
   historical single-file result, and mark this tactical complete only after
   every stopping condition passes.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Swift deterministic | concurrent sibling acquisition, same-file serialization, missing target/parents, coordinator substitution, cancellation, exact ledger drain |
| Rust deterministic | three live platform handles, eviction/invalidation, final-handle release, duplicate/late response and acknowledgement, terminal zero |
| Controlled application | three-file cross-piece transfer, exact verification, restart, Force recheck, publication/read/removal under production service |
| Simulator/build | Swift tests, unsigned simulator build, generated-boundary compatibility, no signing or device values |
| Physical iPhone | qualified external root, controlled multifile completion, restart/recheck, exact publication and cleanup |
| Public swarm | catalog Big Buck Bunny 1,055/1,055, Published/Seeding, three visible files, MP4 system playback, exact cleanup |
| Repository | formatting, warning-denying affected Clippy, affected Rust/workspace tests, Swift tests/build, generated web/Android gates only if their boundary changes, `git diff --check` |

## Non-Goals And Escalation

This tactical does not add embedded playback, streaming-before-completion,
iCloud or identified File Provider roots, arbitrary external mutation support,
root migration, new background guarantees, a new storage backend, a
single-descriptor product limit, a longer request timeout, or App Store/TestFlight
publication. It does not change the JSTorrent-derived SwiftUI presentation
except for truthful failure/progress state exposed by the repaired behavior.

The maintainer has authorized use of the attached physical iPhone, its system
folder picker and Files/media presentation, controlled run-owned fixtures, the
exact repository Big Buck Bunny public magnet, process termination/relaunch,
and exact cleanup. Use the project-neutral `machine-control` iOS interface and
one exclusive transactional session for mutating flows. Public peers are
untrusted and availability is variable; bound attempts and retain no endpoint
or device identity.

Ordinary refactoring at the coordination/release boundary, focused adversarial
tests, local Apple SDK inspection, signed development build/install, and fixes
to same-owner defects exposed by the regression do not require new direction.
Stop for a weakened descriptor-coordination invariant, materially different
root/provider policy, new dependency or entitlement, signing-account or
protected-authentication change, destructive action outside run-owned data,
external publication, or inability to obtain the required real-swarm result
after bounded retries.

## Execution Record

### Controlled exclusion reproduction

The iOS 26.5 simulator now holds one `NSFileCoordinator` writer accessor open
while requesting a sibling operation. The legacy form, where both requests
coordinate the selected root, blocks the second accessor until the first is
released. Coordinating two exact nonexistent sibling targets lets the second
accessor enter immediately while the first remains held. All three focused
namespace tests pass in 0.229 seconds. This distinguishes coordinator item
exclusion from peer, filesystem-service, or generic platform-request latency
and validates exact-target creation as the implementation direction.

The motivating public-swarm run remains an observed failure, not completion
evidence. Production code is unchanged at this checkpoint.
