# Tactical 149: iOS Lifecycle, Recovery, And Distribution Readiness

Status: Decision-complete and queued immediately after Tactical `148` by
explicit maintainer direction on 2026-08-13.

Topics: `client-surfaces`, `product-direction`,
`product-surfaces-and-migration`, `client-persistence`, `download-roots`,
`capability-readiness`

Dependencies: Tacticals `147` and `148` provide the signed maintained app,
qualified roots, Rust application lifecycle, and complete foreground product
surface. Tacticals `116` and `123` provide the physical expiration,
continued-processing, force-close, and root-recovery evidence this slice must
turn into product ownership.

## Decision And Desired Outcome

Make the iOS product recover predictably across scene transitions, finite
background execution, suspension, ordinary termination, force-close, and
relaunch. Package a reproducible development and unsigned/distribution archive
candidate without uploading, publishing, changing signing accounts, or making
an App Store availability claim.

The foreground application owns one Rust service generation. Entering the
background does not immediately discard it. The lifecycle owner first requests
the finite platform opportunity appropriate to the user-started work:

- on iOS 26+, an explicitly user-started active download/check may request one
  `BGContinuedProcessingTask` with truthful progress and cancellation;
- all supported versions may use a bounded ordinary UIKit background assertion
  to checkpoint and join when continued processing is unavailable, denied, or
  expires; and
- no path promises indefinite background downloading or seeding. Suspension or
  force-close is recovered from synchronized payload and SQLite facts.

## Stopping Condition

1. One `IOSApplicationLifecycleOwner` owns scene phase, application-service
   generation, root access, platform request/release pumps, background task,
   notification state, cancellation, checkpoint, shutdown, and relaunch.
2. Foreground/background churn, multiple scenes, duplicate lifecycle callbacks,
   and late task completions cannot create a second engine, close root scope
   before Rust leases, replay a command, or resurrect a stopped generation.
3. A user-started controlled download continues under the iOS 26 finite task
   when granted, reports bounded progress, completes or expires truthfully, and
   posts only user-authorized local notification state.
4. Denial, expiration, low-time shutdown, suspension, ordinary termination,
   SIGKILL/force-close, and relaunch preserve exact durable intent and never
   trust unsynchronized content. Active roots reacquire scope and re-probe
   before work resumes.
5. Controlled process-death points cover metadata, payload write/sync,
   checkpoint commit, publication, selected-root release, and removal. Relaunch
   reaches exact verified content or a truthful checking/repair state with no
   duplicate artifact or leaked owner.
6. Magnet URL and `.torrent` document handoff work from cold and warm launch,
   queue one bounded pending intent until the service/root is ready, and never
   execute it twice.
7. The app contains a reviewed privacy manifest, required usage descriptions,
   document and magnet declarations, supported orientations, version/build
   settings, and no forbidden signing/device/private values.
8. Debug physical deployment, simulator tests, a generic unsigned archive, and
   a locally signed development archive all build reproducibly. The exact `.app`
   alone is installed for hardware validation; archives and signed products are
   ignored and never committed.
9. Repeated physical lifecycle runs end with zero Rust tasks, platform requests,
   coordinated leases, security scopes, cached handles, background tasks, and
   run-owned files after explicit shutdown/removal.
10. Living topics and readiness rows describe the actual foreground and finite
    background support, OS/version evidence, root limits, distribution boundary,
    and remaining release work exactly.

## Owner And State Map

```text
IOSApplicationLifecycleOwner
  generation + phase + cancellation
  -> Rust ApplicationService
  -> RootCapabilityRegistry / coordinated leases
  -> UIKitBackgroundAssertion (at most 1)
  -> BGContinuedProcessingTask (at most 1, iOS 26+)
  -> local notification coordinator
  -> bounded pending external intake (at most 1)
```

Shutdown order is admission stop, command/subscription cancellation, platform
request cancellation, torrent/application join and checkpoint, pooled-handle
release drain, coordinator/scope release, background-task completion, and
terminal generation publication. Force-close can interrupt that order and is
therefore recovered only from durable facts.

## Resource Bounds

| Resource | Bound |
| --- | ---: |
| Live Rust application generations | 1 |
| UIKit background assertions | 1 |
| Continued-processing tasks | 1 |
| Pending cold-launch inputs | 1, at most 64 MiB |
| Local notification categories/requests owned by app | 2 / 8 |
| Lifecycle transition history retained for diagnostics | 64 records |
| Graceful shutdown attempt | bounded by remaining OS time, maximum 25 s |

## Shape-Changing Cases

- inactive/active noise, scene disconnect without process death, multiple
  scenes, background task denial, expiration before/after checkpoint, and a
  completion callback from an old generation;
- force-close without callback at every durable boundary, OS termination while
  suspended, root bookmark change while inactive, and unavailable root on
  relaunch;
- cold magnet/file intake before service readiness, repeated `onOpenURL`, file
  security-scope loss, duplicate receipt, and oversized/empty input; and
- notification denial/revocation, app opened from notification, archive build
  without signing, development signing renewal, and deployment without
  overwriting another product bundle.

## Implementation Stages And Gates

1. Add the lifecycle state machine and deterministic generation/death fixtures.
2. Integrate scene handling, finite UIKit time, iOS 26 continued processing,
   progress, expiration, cancellation, and local notification permission.
3. Add cold/warm URL and document handoff plus force-close recovery harnesses.
4. Add privacy metadata, archive scripts, generic signing configuration, and
   build artifact inspection.
5. Run simulator then transactional physical foreground/background/expiration/
   force-close/relaunch matrices against app-owned and selected roots.
6. Run complete repository gates, clean temporary device/host artifacts,
   reconcile evidence, and close the three-tactical campaign.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | lifecycle/generation transitions, expiry races, duplicate input, shutdown order, force-close facts |
| Rust/session | synchronized checkpoint, conservative restart, exact publication/removal, terminal owner/resource snapshots |
| Swift/simulator | scene churn, task availability/denial/expiration, notifications, cold/warm URL/file handoff |
| Physical iPhone | foreground transfer, granted finite continuation, ordinary expiration, suspension/resume, SIGKILL/relaunch at named phases, root reacquisition, exact cleanup |
| Packaging | debug build, generic unsigned archive, locally development-signed archive, plist/privacy/entitlement inspection |
| Repository | full Rust, web, Android compatibility, iOS tests/builds, documentation and diff checks |

## Non-Goals And Escalation

Indefinite background seeding/downloading, silent relaunch, push notifications,
remote control, analytics/crash SDKs, App Store Connect/TestFlight upload,
notarization, AltStore, production signing/profile changes, public release,
legacy migration, and older-than-iOS-16 support are not part of this tactical.

The authorized physical work includes build/install/launch, semantic UI
automation, backgrounding, ordinary expiration, process termination and
relaunch of the explicit development app, local notifications, controlled
payload files, and cleanup. Stop for protected device authorization, account or
certificate changes, new distribution entitlements, external publication, or
destructive action outside run-owned app/root artifacts.
