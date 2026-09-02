# Phase 6.7 — Operator Ergonomics: Diagnostics Mode Density

## Baseline

Phase 6.6 closed onboarding. This phase closes gap #7 from Phase 6's
own audit: "still-dense Diagnostics Mode" - named since Phase 6.1's
report but never scoped further until now. See `docs/phase-6-7-audit.md`.

## Audit

Diagnostics Mode's main block (`{mode === "diagnostics" && (...)}` in
`LiveChurchBrain.tsx`) renders 11 top-level panels back to back, always
fully expanded: Pending Suggestions, Service Timeline, Manual Bible
Search, Music Intelligence, Sermon Foundation, Sermon Intelligence,
Sermon Harvest, Cross-Domain Intelligence, Content Intelligence,
Service Intelligence, Service History. Two further panels above this
block (`PilotDiagnosticsPanel`/"System Diagnostics" and a nested
"Content Registry"/"Diagnostics: Intelligence Status" pair) were
already collapsible via `<details>` - the rest were plain `<section>`,
with no way to collapse any of them. A technician debugging one domain
had to scroll past everything else, always fully rendered.

The fix's shape was clear (wrap each in `<details>`, matching the
precedent the two already-collapsible panels set); what genuinely
forked was the default state. Put to the operator: all default open
(purely additive, zero visible change until manually collapsed) vs.
collapse the panels an operator is least likely to need mid-service by
default, keeping the two with real actions (Pending Suggestions,
Service Timeline) open. They chose the latter.

## What was built

- 9 of the 11 top-level panels - Manual Bible Search, Music
  Intelligence, Sermon Foundation, Sermon Intelligence, Sermon Harvest,
  Cross-Domain Intelligence, Content Intelligence, Service Intelligence,
  Service History - converted from `<section className="live-brain__panel">`
  to `<details className="live-brain__panel">` (`<h2>` replaced by
  `<summary>`), collapsed by default (no `open` attribute). Pending
  Suggestions and Service Timeline are untouched, still plain `<section>`,
  still always visible.
- Service History's `<summary>` contains its existing Show/Hide button
  (`onClick={openHistory}`) for the history list beneath it. Since a
  click inside `<summary>` bubbles up and would otherwise also toggle
  the new outer panel collapse, its `onClick` now calls
  `e.stopPropagation()` before `openHistory()` - the button still does
  exactly what it always did, independent of the new panel-level
  collapse.
- `LiveChurchBrain.css`: a new `.live-brain__panel summary` rule
  mirrors `.live-brain__panel h2`'s existing uppercase/letter-spacing
  styling, so collapsing a panel doesn't also change how its heading
  looks. This also fixes the same gap in the two panels that were
  already `<details>` before this phase (their `<summary>` never had
  this styling either).
- No new component, no new state, no new command/event - purely a
  collapse affordance wrapped around content that already existed.

## Full regression result

Frontend only - no Rust files changed this phase (the fourth Phase 6.x
frontend-only slice, after 6.2/6.3/6.6). `npm run typecheck`: clean
(the authoritative check that every `<details>`/`<section>` tag pair
in the ~1,260-line block still balances correctly after 9 structural
conversions). `npm run lint`: same 5 pre-existing warnings as before
this phase - no new ones. `npm run test`: 257/257 passing (unchanged
count - this phase is purely structural JSX/CSS, no new pure logic to
unit-test). `npm run build`: succeeds.

## Windows rebuild

Frontend-only change - see
`pilot-evidence/6.7/windows/installer-contents-verification.json` for
the rebuild's direct binary verification, following the same
strings-tooling-limitation disclosure established in Phase 6.1-6.6.

## Architectural safety diff

- Zero new Tauri commands, zero new events, zero new database
  columns/tables - purely a frontend collapse-affordance and styling
  change.
- No panel's content changed - every field, button, and action inside
  the 9 converted panels renders exactly as before; only the outer
  wrapper element and default visibility changed.
- Pending Suggestions and Service Timeline (the two panels with actual
  mid-service actions) are completely unaffected - same element, same
  always-visible behavior as every prior phase.
- Service History's Show/Hide button's behavior is unchanged from an
  operator's perspective (same toggle, same history list) - only its
  event no longer also toggles the new outer collapse.

## Environment A / B / C

- **Environment A** (this container): PASSED - full frontend regression
  green as detailed above. `npm run typecheck`'s success is itself
  direct proof the 9 `<details>`/`<section>` conversions are correctly
  paired (a mismatched open/close tag is a compile error, not a runtime
  surprise).
- **Environment B**: unavailable in this session's container, a
  pre-existing, already-documented limitation - not this phase's
  regression.
- **Environment C** (real Windows hardware, a real live service): NOT YET
  VERIFIED. The decisive pending gate is the operator's own real-hardware
  test: open Diagnostics Mode and confirm Pending Suggestions and
  Service Timeline are visible immediately while the other 9 panels
  start collapsed; click each summary and confirm it expands/collapses
  correctly without losing any content or control; on Service History,
  confirm clicking Show/Hide toggles the history list without also
  collapsing the panel itself.

## Known limitations

- **Collapsed state is not persisted** - every panel resets to its
  default (open for Pending Suggestions/Service Timeline, closed for
  the other 9) on every app restart. No `localStorage` or other
  per-panel memory was added - a deliberate scope boundary, consistent
  with every other session-only UI state in this codebase.
- **No "collapse all"/"expand all" control** - a technician wanting to
  see everything at once (or nothing) must still click each of the 9
  panels individually. A reasonable future addition, not attempted
  this phase.
- **The choice of which 2 panels stay open (Pending Suggestions,
  Service Timeline) is fixed, not operator-configurable** - a
  technician who cares most about, say, Sermon Intelligence must still
  expand it every time Diagnostics Mode is entered fresh.
- **1 more ergonomics gap from Phase 6's own audit remains
  unaddressed** after this phase: unified-queue Edit support - a
  candidate for a future Phase 6.x slice.
- **This exact rebuilt artifact has NOT yet been installed or launched
  on real Windows hardware** - see `physicalHardwareStatement` in
  `release/windows/release-manifest.json`.

## Deferred work

- The 1 remaining Phase 6 ergonomics gap from the original audit.
- Real-hardware Environment C verification.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, outside this container's reach). This phase wraps 9 existing
panels in a native collapse control and adds no new backend surface -
it changes how much is visible by default in Diagnostics Mode, not
what any control does.
