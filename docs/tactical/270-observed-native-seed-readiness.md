# Tactical 270: Observed Native Seed Readiness

Status: Active, 2026-10-08. Qualification-only follow-up to259/267/269.

Topics: `client-surfaces`, `incoming-reachability-and-seeding`,
`application-view-api`.

## Scope, non-goals and stopping condition

Current native evidence independently hashes all28 MiB and renders Seeding,
but the first observer has zero registrations. After the failed120-second
leecher it has one registration and two UnknownTorrent rejections for exactly
the owned fixture. Actual retained logs place joined engine completion at
03:08:10 and accepted structural seed registration at03:08:45. Starting a
leecher before admission, then relying on its failure backoff, did not qualify
completed-file upload. Do not change engine, listener, privacy, selection,
admission, background policy or peer retry behavior to conceal this ordering.

Wait for actual bounded native seed readiness before starting the independent
leecher. Resolve exactly one owned torrent ID from package-UID logs and the
fixture hash, then sample the existing read-only debug observer with fresh
threadtime evidence. Only the isolated single-fixture profile is eligible;
require one actual registration and a valid current port. Retain bounded
samples and elapsed time; timeout, malformed/ambiguous/stale observations and
unexpected registration counts refuse. The upload still independently hashes
every file; readiness alone is never an upload pass.

Correct the diagnostic assertion to the established Normal-profile contract:
severity minimum applies, while Warning/Error from other categories remain
visible. Actual selected labels alone still cannot qualify settled rows.
The current native peer capture intentionally contains discovery warnings;
the overly strict assertion is a runner failure, not a native filter defect.

Retry only explicit fresh UIAutomator idle/capture failures within one bounded
capture operation. Delete only the owned XML before each attempt; never reuse
old XML or retry authentication/transport failures. Final missing/failed fresh
capture refuses and cannot establish navigation success.

Stop after meaningful stale/ownership/refusal/late-readiness/capture tests,
current-source targeted large-metadata full-hash/upload/joined-Live checks on
both devices, cleanup, reconciled report/checklist and local commit. Default
three-repetition/hour qualification follows under259; a short pass is not an
hour or managed Play/store update. iOS and publication are out of scope.

## Source, ownership and bounds

Reviewed `ApplicationService::reconcile_admission`, `reconcile_incoming_torrent`,
`record_seed_reconcile`, `TorrentRuntimeHandle::reconcile_seed` and its joined
active-content eligibility. The existing engine's registry snapshot and native
observer269 are authoritative; no protocol design or implementation changes.
Reviewed `diagnostics::diagnostic_matches` and
`profiles_and_prefixes_filter_without_message_parsing`, which deliberately
preserve Normal warnings across category prefixes. No native filtering change.

Existing Machine Control/RemoteAdb transport and debug service coroutine own
samples; no persistent process/queue is added. Bound readiness to180 seconds,
each sample to the remaining deadline, retained evidence to32 snapshots and
one exact40-hex fixture identity. Never clear device logs. UI capture uses at
most three fresh attempts within its existing overall30-second budget. Existing
seed/tunnel/temp/profile/folder finally owners remain. Keep all prior failed
receipts and their actual component passes.

## Portable execution checkpoint

All37 driver cases pass. Tests prove exact hash/identity refusal, fresh native
observations instead of old ready rows, delayed admission/current-port selection,
ambiguous registry refusal, timeout with retained unready evidence and a32-sample
first/latest bound. Normal cross-category warnings remain visible; informational
rows still require the selected category. Real command ownership tests preserve
the nested-shell hostile-URI boundary. Explicit exit-zero idle/missing-file
failures cannot reuse XML; three failed fresh captures refuse, while a second
fresh successful capture proceeds. Transport refusal is not retried.

Both current targeted native large-fixture reruns are next; no physical pass is
inferred from these portable tests. The original observer failure/UnknownTorrent
and three successful two-positive-file uploads with later failed restart capture
remain immutable failed overall receipts, with successful owned cleanup.
