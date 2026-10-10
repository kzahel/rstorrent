# Tactical 292: Preserve Paused Intent Through Android Folder Repair

Status: Active, 2026-10-10, beneath finish-line259. Managed27 slice056
reproduces a paused complete torrent becoming Seeding after normal Repair.

## Scope and stopping condition

Remove Android presentation-driven Resume dispatches after SAF root probes.
The application service already reconciles admission from durable run intent;
an asynchronously delivered AwaitingStorage view cannot authorize Resume.
Keep the explicit restart IDs returned for runtimes stopped by replacement.
Stop after the regression fails on unchanged Android and passes on the bounded
fix, Android JVM/build checks, cleanup and updated candidate status. Actual
managed-store qualification of a newly signed version remains separate.

Non-goals: Rust engine/protocol changes, root-ID/schema changes, new grants,
automatic folder discovery, migration changes, iOS, Linux, store publication,
desktop feed activation or changing the native service's replacement lifecycle.

## Invariants, owners and reference review

ProductEngineService owns the coroutine, SAF mutation mutex and joined client
lifetime. Kotlin owns provider selection/grants; Rust owns durable run intent
and admission. Probe completion may precede a Compose view update. Never infer
the user's desired run state from a stale AwaitingStorage presentation. Preserve
paused, running, held-selection, archived and unrelated-root semantics; no new
tasks, timers, queues or resource limits are introduced.

Inspected application.rs probe_platform_storage_roots/reconcile_admission and
start_if_possible_with_mode: health probing reconciles existing desired_running,
while Pause remains authoritative. prepare_platform_storage_replacement returns
only affected active runtime IDs. Kotlin setSafTree and repair rollback instead
walk the asynchronously updated ProductState and dispatch Resume to every
AwaitingStorage row, which can include a paused row after the service has already
restored its root. This crosses the ownership boundary.

Pinned libtorrent7d7fc38 torrent.cpp files_checked, checking pause guards and
test_storage.cpp move_storage reset/self cases, test_resume.cpp paused and
override_resume_data_deprecated were inspected: storage/checking
does not grant fresh user Resume intent. No BitTorrent wire change is involved;
SAF repair is a platform capability, without a normative BEP equivalent. The
JSTorrent reference's native RPC Resume is an explicit control operation; its
legacy grant/history does not override the new durable application authority.

## Evidence and restart checkpoint

Managed27 actual screenshots show Paused before outage and Seeding after normal
picker repair, with the same independently hashed1MiB bytes and original source
closed. Cancel retains the original exact grant. Portrait picker clipping is
recovered through normal Landscape; the original setting must be restored.
The original failing assertion remains failed. Restore the owned original path
through normal Repair, remove only its row/file/marker, Shutdown and close every
tab/window before starting an isolated regression. Reports/images remain ignored.

Managed fixture cleanup passes separately: original URI/modes, unchanged other
grants/package/denied notification, empty library/folder, released alias grant,
Portrait restored, stopped service and zero tabs/windows. Normal original-grant
retake advances its creation time.

Next: reproduce the normal paused-repair journey
in an owned API35 emulator, remove the stale-view Resume loops, then repeat with
paused and running controls. Preserve old artifact receipts; managed27 is not
relabeled as fixed. A changed production Android candidate must exceed code27.
