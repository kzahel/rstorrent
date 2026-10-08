# Tactical 269: Android Incoming Peer Diagnostic Evidence

Status: Active, 2026-10-08. Bounded diagnostic follow-up to259/267.

Topics: `client-surfaces`, `application-view-api`, `incoming-reachability-and-seeding`.

## Scope, non-goals and stopping condition

The latest physical large-metadata fixture independently completes and its
native diagnostic reports completed-file seed registration, yet both targets'
leecher connections end before handshake. A separately owned two-positive-file
512-KiB fixture passes foreground/background/reopened upload and joined Live
restart on cohortB. Do not blame general background lifetime, infer missing
registration from EOF, or erase failed large-fixture receipts.

Expose a bounded read-only Android projection of the existing application's
incoming peer snapshot through the established generated resource-observer
boundary. The existing debug-only torrent-control ingress logs this projection;
there is no product UI, new protocol capability, engine mutation, privacy policy,
network/listener/seeding change, new dependency or shared web contract.
Observe current actual listener port, registrations, pending/established peers,
payload and rejection reasons/last hashes, without peer addresses or file paths.
Stop after meaningful closed/inactive and bounded projection tests, generated
normal dual-ABI Android build/JVM/lint/package checks, physical refusal diagnosis
and reconciled local commit/evidence. Any resulting engine fix needs its own
bounded source-first tactical; this diagnostic slice does not authorize it by
itself beyond the user's ongoing finish-line task.

## Ownership, cancellation and limits

The existing application service owns incoming state. A short accessor takes its
existing async service lock, copies its already-bounded snapshot and projects it.
No new persistent task or queue exists. Existing debug command coroutine and
10-second cancellation budget own a sample; generated UniFFI cancellation frees
its future. Retain at most32 rejection-count entries and four recent rejection
records; explicit truncation flags preserve truth. Each optional hash is exactly
40 lowercase hex characters. Do not include socket addresses, user paths,
peer IDs, arbitrary event messages or credential state in the projection.
The normal generated object closes exactly as before.

## Source oracle and required evidence

`ApplicationService::incoming_peer_snapshot`, `IncomingPeerServiceSnapshot`
and its exact registration/generation and rejection counters are authoritative.
This changes observability only, so no libtorrent protocol design is adopted.
Existing Android MSE/download resource observers establish the platform pattern.
Test absent and closed application access, exact counters/port, bounded recent
records and absence of remote addresses. Record actual native output on current
physical fixture and preserve complete old timeout/cleanup receipts.

## Portable observer checkpoint

All15 Android Rust cases and focused clippy pass. A real bounded loopback
application/listener rejects six unknown-hash handshakes; its projection preserves
actual port and six rejection counts while retaining exactly four latest hashes
with explicit truncation and no peer addresses. Offline returnsNone and closed
application access refuses. The first test assumed an initially enabled listener
and failed; it now explicitly enables only its owned loopback listener before
observation. No product default was changed. Normal generated build/physical
samples remain pending.
