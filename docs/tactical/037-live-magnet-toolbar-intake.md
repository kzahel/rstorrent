# Tactical 037: Live Magnet Toolbar Intake

Status: complete.

## Motivation And Outcome

`./scripts/webui` now opens the new live React surface, but the surface cannot
add anything to its isolated profile. Add the familiar JSTorrent toolbar
pattern: one text input immediately beside Add, keyboard or pointer submission,
and visible command feedback. A successful magnet add must flow through the
semantic frontend command, generated application command, live view set, and
torrent library without a raw API workaround.

The stopping result is the controlled libtorrent browser proof entering its
magnet through the toolbar, observing the new torrent and peer activity, and
completing its existing payload/recovery/cleanup assertions.

## Dependencies And References

- [`036-manual-live-webui-launcher.md`](036-manual-live-webui-launcher.md)
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md)
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md)
- [`../topics/application-control.md`](../topics/application-control.md)
- JSTorrent sibling revision `9895410beeed6aff554053769bd006a3fbd373ef`,
  `packages/client/src/AppContent.tsx::handleAddTorrent` and its toolbar.
- JSTorrent `packages/client/src/utils/torrent-input.ts` for honest rejection
  of unsupported remote torrent-file URLs.

Adopt the adjacent input/Add hierarchy, Enter submission, whitespace trimming,
success-only clearing, failure retention, and future empty-button file-picker
direction. Do not copy JSTorrent's inline styles, engine adapter, toast owner,
configuration hub, or file intake implementation.

## Scope And Invariants

- Extend the semantic `InspectionCommand` with a magnet add; React never builds
  a generated Rust request directly.
- The live adapter maps the command to `add_magnet` using the configured
  `downloads` root and no file skips. Existing application validation remains
  authoritative for magnet syntax, bounds, duplicates, storage, and busy
  policy.
- Trim outer whitespace. Bound input before dispatch. Empty, non-magnet, and
  HTTP(S)/file torrent URLs receive specific visible errors and never reach
  the backend.
- Use a form so Enter and Add share one guarded submission path. Prevent
  concurrent submission, retain input on rejection, and clear only after an
  accepted command.
- Keep command feedback in the existing polite status output and expose input
  labels, invalid state, disabled/busy state, focus, keyboard, touch, compact,
  and phone behavior accessibly.
- Demo scenarios keep their deterministic Add-demo behavior rather than
  fabricating a real magnet add.
- The empty Add action is intentionally reserved for a future hidden
  `.torrent` file input, matching JSTorrent's interaction direction; this
  slice reports that file selection is not available yet.

## Non-Goals

- fetching remote HTTP(S) `.torrent` URLs;
- `.torrent` byte upload, parsing, file picker, drag/drop, clipboard watching,
  OS file association, or deep-link intake;
- storage-root or per-file selection UI;
- changing the one-active-download application policy;
- switching Tauri to the new surface, or implementing the Logs redesign.

## Validation

- Pure input validation covers whitespace, empty input, remote file URLs,
  non-magnet values, byte bounds, and accepted magnets.
- Live-adapter tests assert the exact generated `add_magnet` request and
  success/error behavior.
- Component/browser tests cover label, Enter and click submission, busy guard,
  success-only clearing, retained invalid input, and accessible feedback.
- The controlled production browser/libtorrent harness adds through the visible
  toolbar, then retains its transfer, suspension recovery, responsive capture,
  payload hash, peer removal, shutdown, and cleanup gates.
- Run the ordinary TypeScript, Vitest, production build, Playwright, generated
  drift, formatting, and proportional Rust gates.

## Stopping Condition

The launcher-visible live UI can add a valid magnet without devtools or raw
HTTP, the controlled proof uses that path, unsupported inputs are honest, the
future file-picker seam is recorded, documentation and evidence are current,
and the committed tree is clean. Tauri migration remains the next confirmed
client boundary.

## Implemented Result

- Added one controlled, labelled input immediately beside Add in the live
  toolbar. The form shares click and Enter submission, prevents overlapping
  adds, exposes invalid state and polite feedback, retains rejected input, and
  clears only after application acceptance.
- Added a pure input validator with an exact 16,384-byte UTF-8 ceiling and
  truthful empty, malformed, remote-URL, and local-file messages. The backend
  remains authoritative for magnet parsing, durable identity, storage, busy
  state, and replay policy.
- Added `add_magnet` to the semantic inspection command and mapped it to the
  generated application request only in `LiveApplication`. Named demo
  scenarios keep Add demo and reject live magnet intent.
- Kept the intake usable at phone width by giving it a full toolbar row and
  wrapping actions and feedback below. No React component imports the
  generated request contract.
- Replaced the controlled proof's raw `fetch` add with visible toolbar input.
  It now rejects and retains an HTTP `.torrent` URL, adds a valid magnet with
  Enter, observes the disabled in-flight button, then continues through live
  torrent/peer views and transfer completion.

## Evidence

- `npm run typecheck --prefix clients/web`: pass.
- `npm test --prefix clients/web -- --run`: 35 passed, 2 skipped opt-in cases.
- `npm run build --prefix clients/web`: pass against the production bundle.
- `npm run test:e2e --prefix clients/web` against an isolated local Vite host:
  4 demo/browser cases passed; the opt-in controlled live case skipped.
- `uv run --project tests/interop --locked python
  tests/interop/browser_peer_inspection_surface.py --screenshot-dir
  target/headless-evidence/t037-magnet-intake`: pass. The production bundle
  added through the visible form, showed one controlled libtorrent peer,
  recovered from an expired view set, completed and hash-verified the payload,
  removed the joined peer, shut down, and cleaned its temporary profile. Wide,
  compact, phone-detail, reconnecting, and phone-library captures were retained
  as ignored local evidence; the accessibility scan found no serious or
  critical violation.
- `npm run generate --prefix clients/web`: pass with no generated drift.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace -- -D warnings`: pass.
- `cargo test --workspace`: 278 passed, 3 ignored live-network cases.

The stopping condition is met. `.torrent` file selection, remote URL fetching,
Tauri migration, and categorized Logs remain separate slices.
