# Tactical 173: Mobile Web Table Horizontal Scrolling

Status: complete.

## Motivation And Desired Outcome

The shared web UI presents dense inspection tables on phone-sized viewports,
but responsive column thresholds currently remove configured-visible columns
from the rendered grid. On a 456-by-1024 touch viewport, the Swarm table
reports Sources, Last seen, Retry, Dials, Fails, Trust, and Parole as visible
in Columns while rendering only State and Address. Its `clientWidth` and
`scrollWidth` are both 456 pixels, so no horizontal gesture can reveal the
missing data.

This contradicts the accepted table contract: configured columns remain
available at narrow widths through horizontal scrolling. The desired outcome
is that a checked column is rendered at every viewport width, the table owns
the resulting two-axis overflow, and touch users receive a restrained visual
indication when more columns exist off screen.

## Scope

- Make shared `VirtualTable` configured visibility authoritative rather than
  applying a second viewport-width removal rule.
- Remove obsolete per-column viewport thresholds from every shared table.
- Keep existing column order, widths, user-hidden defaults, resizing,
  sorting, persistence, virtualization, and selection behavior.
- Make touch panning and horizontal overscroll containment explicit on the
  table viewport.
- Show non-interactive edge fades only while additional horizontal content is
  available in that direction.
- Add deterministic phone-width browser coverage for actual overflow,
  movement, rightmost-column reachability, and truthful Columns state.
- Reconcile the owning web UI topic and tactical index after validation.

## Non-Goals

- Replacing tables with cards or a separate mobile application.
- Column reordering, per-breakpoint preference profiles, or automatic column
  width compression.
- Sticky or frozen columns.
- Changing table data, application projections, commands, Android Compose, or
  native iOS presentation.
- Redesigning the Swarm summary strip or detail-tab scrolling.

## Invariants And Interaction Contract

- Every checked Columns entry has a rendered column header at every viewport
  width; every unchecked entry remains absent.
- The sum of configured column widths remains the grid minimum width. When it
  exceeds the viewport, horizontal scrolling reveals the complete configured
  set without allocating additional rows.
- Horizontal and vertical touch panning coexist. A pan continues to cancel a
  pending long press and must not activate or select a row accidentally.
- Edge fades are paint-only, ignore pointer input, appear only when content is
  available in their direction, and disappear at the corresponding boundary.
- Sort, resize, current-row, selection, keyboard, and persisted table
  configuration semantics remain unchanged.
- The implementation adds no task, timer, subscription, application state, or
  platform boundary. `VirtualTable` remains the sole scrolling owner.

## Validation

- Focused `VirtualTable` component tests.
- Deterministic Playwright phone checks at 390 and 456 CSS pixels that prove
  `scrollWidth > clientWidth`, horizontal position changes, the final checked
  column becomes reachable, and Columns agrees with rendered headers.
- Existing phone interaction checks continue to cover navigation,
  accessibility, and bounded virtual rows.
- `npm run typecheck --prefix clients/web`.
- `npm run test --prefix clients/web`.
- The focused deterministic Playwright scenario containing the new regression.

## Stopping Condition

This tactical is complete when every shared web table treats configured
visibility as authoritative, the Swarm regression passes at both reported and
representative phone widths, touch/selection and virtualization behavior
remain intact, the focused and standard web validation succeeds, and the
owning documentation records the corrected contract and evidence.

## Implemented Result

The shared `VirtualTable` no longer applies `minimumViewport` as a second,
silent visibility rule. Every table now renders exactly the columns selected
by its persisted configuration at every width. Existing summed column widths
therefore create ordinary horizontal overflow on a narrow viewport without
changing the virtual row window or allocating offscreen cells.

The viewport explicitly permits two-axis touch panning and contains horizontal
overscroll. `VirtualTable` observes its own horizontal position and paints
pointer-transparent left or right edge fades only while more configured
content is available in that direction. The overflow observation is local
component state driven by existing resize and scroll events; it introduces no
task, timer, subscription, or application state.

The Swarm browser regression now opens Columns at phone width and proves that
all nine checked defaults have rendered headers. At both 390-by-844 and the
reported 456-by-1024 geometry, it requires real overflow, sends a trusted
Chrome touch swipe, observes positive `scrollLeft`, checks both directional
edge states, scrolls to the far boundary, and requires Parole to be completely
reachable. The existing responsive ETA case now requires its checked column
to remain rendered and the Transfers table to overflow rather than accepting
silent phone-only removal.

## Validation Evidence

- `npm run typecheck --prefix clients/web`: passed.
- `npm run test --prefix clients/web -- src/inspection/components/VirtualTable.test.tsx`:
  11 tests passed.
- `npm run test:e2e --prefix clients/web -- --grep "swarm lifecycle remains readable"`:
  one focused Chrome case passed, including trusted touch input at 390 and 456
  CSS pixels.
- `NODE_OPTIONS=--no-experimental-webstorage npm run test --prefix clients/web`:
  44 files and 290 tests passed; two opt-in files and two tests skipped. Node
  25.2.0 otherwise exposes an experimental global `localStorage` that throws
  before the existing jsdom application tests render because no
  `--localstorage-file` is configured; disabling that unrelated Node
  experiment restores the repository test environment.
- `npm run test:e2e --prefix clients/web`: 33 deterministic Chrome cases
  passed and 12 live/opt-in cases skipped. Existing phone navigation,
  long-press selection, overlays, accessibility scans, and bounded virtual
  rendering passed with the new table behavior.
- `npm run build --prefix clients/web`: production Vite build and CSP bundle
  scan passed.
- The retained 456-by-1024 phone capture was inspected at the far horizontal
  boundary: Last seen through Parole are readable, the left continuation fade
  is visible, and the table remains aligned. Temporary evidence was removed.

## References

- [`../topics/web-ui-design.md`](../topics/web-ui-design.md)
- [`../topics/table-interaction.md`](../topics/table-interaction.md)
- [`064-registry-backed-swarm-inspection.md`](064-registry-backed-swarm-inspection.md)
- [`077-shared-overlay-menu-system.md`](077-shared-overlay-menu-system.md)
