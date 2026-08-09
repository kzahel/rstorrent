# Tactical 115: MSE Policy, Advertisement, And Peer Detail

Status: Complete on 2026-08-09. The policy, tracker advertisement, optional
peer detail, generated consumers, controlled interoperability, and repository
baseline pass. Tactical `112` remains the strategic capability **Now**.

Topics: `protocol-support`, `tracker-discovery`, `peer-flag-vocabulary`,
`peer-lifecycle`, `application-view-api`, `web-ui-design`, `client-surfaces`,
`performance-and-live-evidence`, `utp-transport-campaign`

Dependency: completed Tactical
[`111`](111-mse-peer-stream-encryption.md) owns the MSE wire implementation,
four-value live session policy, structured diagnostics, performance evidence,
and Android graduation. This follow-up does not reopen its protocol or
resource architecture.

## Motivation And Decisions

Three small gaps remained after Tactical `111`:

1. The default `allow` policy existed for compatibility but selected RC4 when
   an incoming initiator offered both payload methods. Pinned
   libtorrent defaults to `prefer_rc4 = false` and selects plaintext-payload
   MSE. The measured RC4 cost makes paying it under compatibility-only policy
   unnecessary. `allow` now matches that default; `prefer` and `required`
   continue to select RC4 when both methods are offered. An RC4-only offer
   remains accepted by every MSE-accepting policy.
2. Pinned libtorrent appends `supportcrypto=1` to HTTP tracker announces when
   incoming MSE is enabled
   (`reference/libtorrent/src/http_tracker_connection.cpp:157-159`).
   RSTorrent now advertises the same legacy capability whenever its effective
   policy accepts incoming MSE and omit it under `disabled`. This is derived
   behavior, not a new setting.
3. The engine owned the exact negotiated method but `PeerView` collapsed it
   into the truthful `E` flag. The application contract now adds an optional
   closed method value. The web UI retains the single `E` glyph and column
   while using the exact method only in its accessible label and hover text.
   No additional glyph or always-visible column is added.

## Scope And Ownership

- `PeerEncryptionPolicy` owns the pure responder preference and incoming-MSE
  capability decisions. Incoming handshakes capture the policy once and pass
  its method preference into the existing sans-IO responder.
- The discovery-advertisement owner owns the live tracker capability value.
  A settings command changes it and requests a corrective announce for active
  registrations without replacing the owner or torrent registration. Each
  HTTP operation captures the current value in `HttpTrackerAnnounce`; UDP
  tracker packets are unchanged.
- Rust application projection maps `Option<MseMethod>` to an optional
  `PeerMseMethodView::{PlaintextPayload, Rc4}` value from the same coherent
  connection observation used for `PeerFlagView::Encrypted`.
- React keeps the existing compact flag string. Its flag-cell tooltip and
  accessible name refine `E` to either "MSE handshake with plaintext payload"
  or "MSE with RC4 payload" when the new field is available; older producers
  retain the generic label.

## Invariants And Non-Goals

- `disabled` accepts and announces no MSE; `allow`, `prefer`, and `required`
  announce `supportcrypto=1` over HTTP.
- `allow` prefers plaintext-payload only when both known methods are offered;
  it never rejects an RC4-only offer. `required` continues to require an MSE
  handshake, not RC4 payload encryption.
- A policy change affects future handshakes and tracker operations. Existing
  peer streams retain their captured method and cipher state.
- The tracker parameter is an untrusted compatibility hint. It creates no
  peer, reachability, privacy, or security claim and is never sent to UDP
  trackers.
- No raw `in_enc_policy`, `out_enc_policy`, `allowed_enc_level`,
  `prefer_rc4`, padding, exponent, or DH-concurrency setting is exposed.
- No per-torrent policy, Android Compose control, new peer-table column, new
  compact glyph, PEX capability flag, public-swarm claim, or performance
  threshold is added.
- uTP remains unsupported. The existing
  [`utp-transport-campaign`](../topics/utp-transport-campaign.md) already makes
  MSE-over-uTP composition a Stage 5 follow-up after the ordered stream and
  shared UDP runtime exist; this slice changes no uTP code.

## Validation

1. Pure policy tests cover all four compatibility and method-preference
   decisions; runtime and interoperability tests cover both method-offer
   shapes.
2. Incoming runtime tests prove `allow` selects plaintext from `0x03`, while
   `prefer`/`required` select RC4 and RC4-only remains usable.
3. The full pinned-libtorrent MSE matrix passes with revised incoming-method
   expectations and exact payload hashes.
4. HTTP target tests prove exact inclusion/omission. A live owner test changes
   policy without replacing the registration, observes a corrective announce,
   and proves terminal task/operation cleanup.
5. Rust view tests prove plain, plaintext-payload MSE, and RC4 MSE projection.
   Generated TypeScript/schema/UniFFI consumers and React mapping/presentation
   tests pass, with the visible glyph string unchanged.
6. Run the proportional repository baseline:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace --no-fail-fast
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
git diff --check
```

## Stopping Condition

This follow-up is complete when the policy, HTTP announce, exact peer method,
quiet web presentation, controlled libtorrent matrix, generated consumers,
owning topics, and retained tests agree; all owners terminate cleanly; and no
new product setting or protocol-support claim is introduced.

## Execution Record

- Commit `dd7fc27` accepted this bounded follow-up and fixed its decisions,
  non-goals, ownership, and evidence before implementation.
- Commit `c812c26` separated incoming-MSE compatibility from responder method
  preference. A focused live incoming test proves `allow` selects
  plaintext-payload from `0x03`; the retained controlled harness adds an
  RC4-only `allow` case.
- Commit `4a1096b` added conditional HTTP `supportcrypto=1`, a live
  advertisement-owner replacement command, a corrective update, and session
  settings convergence that retains the prior effective policy if that owner
  has stopped.
- Commit `01cb277` added optional `PeerMseMethodView`, regenerated the
  TypeScript/schema/validators, and refined only the existing `E` cell's
  tooltip and accessible name. Commit `0f7d3b0` made captured policy ownership
  explicit in handshake accounting and asserted both exact view methods.
- The uTP campaign already reserved MSE-over-uTP composition for Stage 5.
  Its checkpoint now names completed Tacticals `111`/`115` and requires that
  explicit composition after an ordered uTP stream exists.

The controlled libtorrent `2.0.13.0` matrix passed all 29 cases with exact
8,389,339-byte payload hashes. It observed `allow` selecting
`plaintext_payload` from both methods, `prefer`/`required` selecting `rc4`,
and `allow` accepting an RC4-only offer. The fixed-delay proxy still observed
two ordinary and four MSE delayed turns; the measured extra was 51.818 ms for
an expected 50 ms, within the retained 20 ms tolerance.

The final repository gate passed:

```text
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace --no-fail-fast
cargo check -p rstorrent-session --features uniffi
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web -- --run
git diff --check
```

The web suite reported 238 passed and 2 skipped tests across 34 passing and 2
skipped files. No public swarm, new performance cohort, visible product
client, physical device, Android setting, or uTP runtime was exercised or
needed for this correction.
