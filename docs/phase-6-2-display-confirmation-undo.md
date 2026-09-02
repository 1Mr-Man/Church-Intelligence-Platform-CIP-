# Phase 6.2 — Operator Ergonomics: Display Confirmation / Undo

## Baseline

Phase 6.1 fixed keyboard shortcuts targeting the right item in the Needs
Attention queue. This phase closes gap #2 from Phase 6's own audit: the
one-click Bible "Display" action had no confirmation before it fires and
no undo after - see `docs/phase-6-2-audit.md` for the full breakdown.

## Audit

Display fires immediately on click/keypress, chaining `approve_suggestion
-> prepare_presentation -> display_presentation` with no intervening step
- by the time the handler returns, content is already on the real
projector. This is deliberate (Phase 3.8.7.8 built Display to *remove*
clicks, at the operator's own request), so a blocking confirmation dialog
in front of every click would fight the feature's own purpose. A manual
"Stop" button already exists (`clear_presentation_display`) but only
blanks the screen - it does not restore what was showing before, and
nothing prompts the operator to use it right after a mistaken Display. No
confirmation-dialog pattern exists anywhere else in this codebase.

## Design choice

Put directly to the operator: confirm-before (a guard on every click, at
the cost of Display's one-click speed), undo-after (keeps every correct
click free, only costs the operator anything on a genuine misclick), or
both. The operator chose **both**.

## What was built

- **`apps/desktop/src/lib/confirmGuard.ts`** (new): `decideConfirmClick(key,
  pending, now)` - a small, generic, pure "arm on the first click, fire on
  a second matching click within `CONFIRM_WINDOW_MS` (4s)" guard, keyed by
  an arbitrary string rather than hardcoded to "display" so any future
  action could reuse it. Mirrors Phase 6.1's `resolveUnifiedShortcutAction`/
  `shortcutLegend` in being pure and DOM-free (this project has no DOM
  testing environment configured).
- **`LiveChurchBrain.tsx`'s `handleUnifiedAction`**: only the bible/`"display"`
  branch now calls `decideConfirmClick` first. A first click/keypress arms
  (returns without firing); a second one on the *same* item within the
  window fires the exact same three-command chain as before. Every other
  action (approve/reject/accept/acknowledge/review/dismiss) is completely
  unaffected and still fires on its first click. This applies uniformly to
  both mouse clicks and the "A" keyboard shortcut, since both already
  dispatch through this one function (Phase 6.1's own single-dispatcher
  discipline) - no separate keyboard-specific confirm logic was needed.
- **`IntelligenceCard.tsx`**: accepts a new `confirmingKey` prop; the one
  button matching it swaps its label to `"Confirm {label}?"` and a warn
  style, so the armed state is visible regardless of whether it was armed
  by a click or a keypress. `AttentionQueue.tsx` forwards the prop through
  unchanged.
- **Post-Display "Undo (blank screen)" banner** (`LiveChurchBrain.tsx`):
  once `display_presentation` actually succeeds (not before - an earlier
  throw in the chain never reaches this line), a banner appears near the
  top of the page for `DISPLAY_UNDO_WINDOW_MS` (8s) or until
  `activeDisplayItem` no longer matches what was just displayed (the
  operator already clicked Stop, or displayed something else) - whichever
  comes first. Its button calls the exact same `clear_presentation_display`
  command the Presentation card's own Stop button already calls; no new
  backend command, no new "restore the previous item" capability (that
  state isn't tracked anywhere today, per the audit's own honest framing
  of what "undo" can mean here).

## Full regression result

Frontend only - no Rust files changed this phase. `npm run typecheck`:
clean. `npm run lint`: the same pre-existing warnings as before this
phase (one `only-export-components` in `main.tsx`, four unrelated
`set-state-in-effect` warnings in files this phase's own new effects were
written specifically to avoid triggering - the first draft of the Undo
auto-dismiss effect *did* trigger a new one via a synchronous `setState`
call in the effect body, fixed by moving that logic into the timer
callback instead, matching Phase 6.1's own precedent of adjusting the
implementation rather than accepting a new warning). `npm run test`:
236/236 passing (231 pre-existing + 5 new `confirmGuard` tests). `npm run
build`: succeeds.

## Windows rebuild

Frontend-only change - see
`pilot-evidence/6.2/windows/installer-contents-verification.json` for the
rebuild's direct binary verification, following the same honest
`strings`-tooling-limitation disclosure established in Phase 6.1's own
evidence file for frontend-only changes.

## Architectural safety diff

- Zero new Tauri commands, zero new events, zero new database
  columns/tables.
- Every other unified action (approve/reject/accept/acknowledge/review/
  dismiss) fires exactly as before - only Bible's `"display"` action goes
  through the new guard.
- `decideConfirmClick` never fires an action on its own; it only ever
  gates the *same* pre-existing call the second click already made
  before this phase.
- Undo calls the exact same `clear_presentation_display` command the
  Presentation card's own Stop button has always called - no new
  "restore" capability was invented, and the banner's own visibility is
  entirely derived from state (`activeDisplayItem`) already tracked by
  this component before this phase.

## Environment A / B / C

- **Environment A** (this container): PASSED - full frontend regression
  green as detailed above, including 5 new unit tests for
  `decideConfirmClick` covering arm/fire/re-arm-after-expiry/different-key/
  nothing-pending.
- **Environment B**: unavailable in this session's container, a
  pre-existing, already-documented limitation since Phase 3.8.5 - not
  this phase's regression.
- **Environment C** (real Windows hardware, a real live service): NOT YET
  VERIFIED. The decisive pending gate is the operator's own real-hardware
  test: click Display once (confirm the button shows "Confirm Display?"
  and nothing is projected yet), click it again (confirm it fires and the
  Undo banner appears), then click Undo (confirm the screen blanks).

## Known limitations

- **Undo blanks the screen; it does not restore the previous item** - no
  code anywhere tracks "what was showing before," so a true restore
  remains future scope, stated honestly rather than implied.
- **The confirm guard is scoped to Bible's Display action only** - the one
  unified action that immediately projects to a real screen. No other
  action gained a confirmation step.
- **Both the confirm window (4s) and the undo window (8s) are documented
  policy choices, not tuned against real operator behavior** - consistent
  with every other threshold in this codebase (e.g.
  `SILENCE_RMS_THRESHOLD`, `MIN_SEMANTIC_SIMILARITY`).
- **The Undo banner disappears once its window elapses even if the
  operator hasn't looked at the screen yet** - there is no persistent
  "last action" log to recover a missed Undo window from; Stop remains
  available manually regardless.
- **This exact rebuilt artifact has NOT yet been installed or launched on
  real Windows hardware** - see `physicalHardwareStatement` in
  `release/windows/release-manifest.json`.
- **6 more ergonomics gaps from Phase 6's own audit remain unaddressed**
  (feed search, error visibility, error-banner context, onboarding,
  Diagnostics Mode density, unified-queue Edit support) - each a
  candidate for a future Phase 6.x slice.

## Deferred work

- A true "restore the previous item" undo, if ever justified by real
  operator feedback that blank-then-redisplay isn't fast enough.
- The remaining Phase 6 ergonomics gaps from the original audit.
- Real-hardware Environment C verification.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, outside this container's reach). This phase adds a guard rail
and a recovery path around one existing, already-tested action - it
introduces no new backend surface, changes no other action's behavior,
and reuses the exact same commands (`clear_presentation_display`) already
proven in Phase 3.8.2's own reconciliation work.
