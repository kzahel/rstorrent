# Peer Transfer And Recency Columns

Status: complete.

## Motivation And Outcome

The active Peers view already receives per-connection payload upload rate,
physical uploaded bytes, connected age, and last-payload age through the
generated `PeerView` contract. The live adapter preserves the upload counters
but the React table does not render them, while it discards both age values.
This leaves ordinary seeding activity and stale connections harder to inspect
than the application-owned evidence permits.

Complete this slice when the shared React Peers table truthfully displays and
sorts Up, Uploaded, Connected, and Last payload values from live and demo rows,
with nullable behavior, responsive column policy, focused tests, and no change
to the generated application contract.

## Dependencies And References

- [`../topics/desktop-inspection-surface.md`](../topics/desktop-inspection-surface.md)
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md)
- [`086-long-lived-torrent-peer-runtime.md`](086-long-lived-torrent-peer-runtime.md)
- JSTorrent's current `packages/ui/src/tables/PeerTable.tsx` as the product
  vocabulary reference for Down, Up, Downloaded, and Uploaded.

No protocol specification or pinned libtorrent inspection is required because
this slice changes only presentation of already-owned application facts and no
protocol state, runtime behavior, or support claim.

## Scope

- Retain connected and last-payload ages in the frontend `PeerRow` mapping.
- Supply deterministic values for named demo peers.
- Add sortable Up, Uploaded, Connected, and Last payload columns.
- Keep Up visible with Down. Use the existing responsive age/source and
  wide-counter breakpoints for the other new columns.
- Format durations consistently with the existing Swarm inspection table and
  append `ago` only to an available last-payload value.
- Cover live reset/recovery mapping and rendered table columns.

## Contracts And Invariants

- Rust `PeerView` remains the wire authority; generated TypeScript and JSON
  Schema artifacts do not change.
- Upload rate is sampled payload bytes per second and uploaded bytes is the
  exact physical payload total for the active connection generation.
- Connected and last-payload values are elapsed durations captured by the
  application, not wall-clock timestamps invented by React.
- `null` remains unavailable or directionally unsupported and renders as an
  em dash. Numeric zero remains a real value.
- Sorting uses raw nullable numeric values rather than formatted strings.
- Existing persisted table preferences tolerate the new column identifiers
  without migration or loss of prior settings.

## Non-Goals

- ETA, torrent size/progress semantics, skipped-file accounting, or aggregate
  torrent upload projection.
- New engine observation, application DTO fields, generated-contract changes,
  peer history, or persistence.
- Protocol-rate columns, peer progress, queue detail, row detail, Android UI,
  or a general duration-formatting refactor.
- Visible browser, desktop shell, public-network, emulator, or physical-device
  validation.

## Implementation And Validation

1. Extend `PeerRow`, live mapping, and deterministic demo construction.
2. Add the four table columns with raw numeric sorting and nullable formatting.
3. Extend focused adapter and component assertions.
4. Run focused Vitest coverage, TypeScript typecheck, the production web build
   and CSP scan, generated-contract drift inspection, and `git diff --check`.

The next slice remains the product discussion and bounded design for ETA
ownership, selective-download remaining bytes, and stalled/complete display
semantics.

## Completion Evidence

Completed on 2026-08-07. The frontend row now retains the generated
connection-age and last-received-payload-age values alongside the upload rate
and physical upload total it already preserved. The Peers table adds sortable
Up, Uploaded, Connected, and Last payload columns. Up remains present beside
Down at every table viewport; the two ages follow the existing age/source
breakpoints; Uploaded follows the existing wide Downloaded counter. All four
are enabled in fresh and existing column settings, while prior widths,
visibility, sorting, and live-sort preferences remain intact.

The permanent demo supplies deterministic values for responsive and scale
coverage. The live view-set recovery test proves exact `2,048 B/s`, `8,192 B`,
`2,000 ms`, and `250 ms` values survive a reset into the frontend row. The
component suite proves the responsive headers and all four enabled column
controls. Last payload retains the existing application meaning: age of the
last received content payload, so upload-only and otherwise unsupported rows
truthfully show an em dash.

Validation passed the focused 51-test adapter/component suite, the complete
web suite with 206 tests passing and two opt-in tests skipped, TypeScript
typecheck, production Vite build, CSP scan, generated-contract drift
inspection, and `git diff --check`. No Rust, generated contract, browser
launch, desktop shell, network, emulator, or physical-device validation was
required.
