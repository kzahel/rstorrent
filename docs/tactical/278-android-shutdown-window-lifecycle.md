# Tactical 278: Android Shutdown Window Lifecycle

Status: Active, 2026-10-08. Managed finish-line UI defect.

Topics: `android-jstorrent-replacement`, `client-surfaces`, `web-ui-design`.

## Scope and stopping condition

The real code26 managed Play app joins its engine after the normal Shutdown
menu action with cleanup_failed=false, but its visible activity stays bound to
the stopped service and continues to show Live and actionable torrent rows.
Close the foreground task when its explicit Shutdown action requests the
existing stop path. Reopening through the normal launcher must create a fresh
service and retain owned library records, payload bytes and SAF access.

No engine, background policy, network, persistence, privacy settings, generated
contract or dependency changes. Preserve the service's joined shutdown and
error-containment behavior from260. Bump the next Android candidate to27;
code26 is already available internally and cannot be replaced with new bytes.
Stop after build/JVM/lint evidence and an isolated actual normal-menu shutdown,
absent owned activity/service and fresh Live-library reopen with unchanged
bytes. Managed code26 remains unchanged; a new internal delivery needs its own
reviewed instruction. No production promotion is authorized.

## Ownership and validation

MainActivity owns its Android task/window and binds/unbinds the product service
in onStart/onStop. ProductEngineService remains sole owner of the native client,
SAF tasks, lifecycle and cancellation. Explicit Shutdown requests the existing
terminal stop, then finishes/removes the activity task so onStop releases the
binding. Service onDestroy joins its existing shutdownComplete barrier before
canceling its scope. No new task, queue, retry or timeout is introduced.

Read ProductApp's existing Shutdown route, MainActivity's binding lifecycle,
ProductEngineService requestStop/shutdown/onDestroy and260's shutdown contract.
Use the isolated qualification identity for the actual menu check; never
sideload over the managed Play installation or clear inherited production data.
Capture the real code26 stale Live failure and the corrected task/reopen states.

## Source checkpoint

The activity-owned Shutdown callback requests the existing service stop and
finishes/removes its task. ProductApp forwards the callback through its existing
navigation host; preview callers retain their service callback default. The first
debug compile misses that navigation forwarding and is retained as a failed
attempt; after correction, all122 JVM cases and debug lint pass. Candidate27
is prepared locally. Native qualification and signed release remain next.
