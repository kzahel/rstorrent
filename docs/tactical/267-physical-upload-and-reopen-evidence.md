# Tactical 267: Physical Upload And Reopen Evidence

Status: Active, 2026-10-08. Bounded finish-line driver follow-up to 259.

Topics: `client-surfaces`, `incoming-reachability-and-seeding`.

## Trigger, scope and stopping condition

Both current physical devices complete full 3,600-second detached transfers
and independently hash all 40 MiB, then their separate upload phase times out.
Only an added-torrent alert survives; actual peer/payload counters are missing.
The reused helper starts a tunnel after a fixed 200-ms delay and budgets 30
seconds for ordinary small fixtures. Current large SAF fixtures need measured
bounded evidence; neither a timeout nor an accepted SSH process proves transport
readiness. Same-route read-only SSH preflight succeeds on both targets.

Make the existing tunnel wait for its actual local listener with bounded
process/timeout diagnostics and joined failure cleanup. Keep ordinary upload
budget 30 seconds; expose an explicitly bounded optional budget for the physical
large-fixture caller and report actual peer/payload counters on progress/failure.
Do not infer upload from the controller's seed counters or enlarge budgets until
an opaque failure disappears. Independently hash every leeched file and retain
nonzero received payload as before. Preserve exact target/owned forwards and
finally cleanup; no engine, network, tracker, privacy or seeding policy changes.

Source audit also shows the reopen driver assumes Android's normal task always
opens the library. Compose retains its Settings route. Navigate only through
freshly observed Back controls on the two known settings pages, at most twice,
then require actual Live plus the owned row. Unknown/ambiguous/disabled/clipped
controls must not become a false pass. No injected app state or forced activity
reset stands in for normal navigation.

Stop after meaningful portable ownership/refusal/late-listener/navigation tests,
bounded physical upload/reopen trials on both targets, normal cleanup and local
commit with reconciled receipts/report. Keep original hour/upload receipts failed
as complete runs, even when their independent hour/hash component passed.
Only an entire corrected end-to-end hour run can become an end-to-end hour pass.

## Owners, budgets and reference scope

Existing SSH process/forward, libtorrent leecher and temporary directory owners
remain; every branch terminates/joins/removes its own resources. Listener wait is
15 seconds; explicit upload budget is validated within 1..300 seconds before
mutation. Physical caller uses 120 seconds with compact actual counters.
Reopen keeps its 120-second wait and allows two observed navigation steps.
This changes qualification infrastructure, not protocol/engine support. Existing
controlled libtorrent verification and the native settings/navigation source
are the oracle; no new protocol behavior or reference source is copied.

## Initial portable checkpoint

Twenty driver safety/navigation tests and five upload-transport tests pass.
A real delayed local TCP listener proves that fixed sleeps no longer establish
readiness; real failed/timeout child processes are joined, authentication failure
is not retried, invalid budgets refuse before forwarding, and tunnel failure
removes only the owned forward/directory while preserving an unrelated sentinel.
Progress is on stderr to preserve the existing JSON stdout contract. Initial
short physical trials are active; no upload pass is inferred from these tests.

The corrected end-to-end runner additionally opens the actual native Logs page
and switches Warning/Info to replace its diagnostic subscription after a large
verified transfer. It requires observed retained-history health and no new
snapshot-over-queue failure. This exercises Tactical 266's repaired budget through
normal UI; it does not inject service state, change filters permanently or alter
native retention. Current first short trials predate that new phase and retain
that limit. Full reruns use the new phase after portable checks.

A targeted upload option can omit already-qualified cold repetitions only for
explicit completed-upload observations of 1..600 seconds. Its receipt marks
cold repetitions unrun and it cannot be used for an hour. The ordinary hour path
still requires all three repetitions. This bounds further stage-diagnosis work
without relabeling a targeted result as complete endurance qualification.
