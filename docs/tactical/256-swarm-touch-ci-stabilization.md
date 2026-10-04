# Tactical 256: Swarm Touch CI Stabilization

Status: **Active, 2026-10-04.**

Topic: `web-ui-design`. Release owner: `beta-release-readiness`.

## Objective And Scope

The maintainer requests fixing current CI before Play-delivered upgrade
qualification and fresh signed candidates. Repair the failing swarm layout
journey without weakening its touch-input, overflow, column visibility or
accessibility assertions. Stop after local validation and exact-source hosted
CI pass. Commit and push this bounded fix for that qualification.

Engine, table behavior, release publication, production feeds and stores are
outside this repair. The subsequent installed and signed-candidate campaign
has its own tactical.

## Diagnosis And Ownership

CI run 37185108049 fails only the swarm layout E2E: after a 390-pixel touch
swipe and transition to 456 pixels, the left overflow remains true after an
explicit reset. All nine other executed jobs and Website pass. Five ordinary
local repeats pass. Eightfold CPU throttling reproduces the exact assertion
failure in one of three repeats; scroll instrumentation records a five-pixel
offset after the reset. The CI trace transitions to the second viewport about
60 ms after releasing the touch. The manually injected fast gesture can leave
compositor scrolling in flight across subsequent position changes.

The test helper owns its CDP session and detaches it in finally. Replace the
manual touch event train with Chromium's completed native touch-scroll gesture
and explicit fling suppression. Existing signed-distance, viewport geometry,
real scrolling and UI-effect assertions remain. No product task, application
boundary, generated code or Android semantics change.

## Ordered Work And Validation

1. Retain the failed trace privately and reproduce with bounded CPU pressure.
2. Make the touch helper finish its gesture before end-position and resize
   checks; preserve all existing assertions and session cleanup.
3. Repeat the pressured regression, then run web typecheck, unit/build gates
   and the complete deterministic browser suite with bundled Chromium.
4. Commit with verified maintainer identity, push, and inspect exact-source CI.
   Repair only new failures needed to close this bounded gate.

## Evidence And Result

Local repair passes six eightfold-CPU-throttled repetitions, web typecheck,
470 web unit tests (two skipped), production build/CSP and the complete
deterministic browser suite: 46 passed, 14 opt-in live cases skipped. All
original touch-effect, overflow, column-bounds and accessibility assertions
remain. Diagnostic instrumentation is removed. Hosted qualification pending.
