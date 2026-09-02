# Phase 6.8 — Operator Ergonomics: Unified-Queue Edit Support

## Baseline

Phase 6.7 closed Diagnostics Mode density. This phase closes gap #8 -
the last of Phase 6's own original 8 audit gaps. See
`docs/phase-6-8-audit.md`.

## Audit

Diagnostics Mode's Pending Suggestions panel already had a full inline
Edit flow (input + Save/Cancel, backed by `editingId`/`editValue` state
and `commands.editSuggestion`). The Needs Attention queue
(`AttentionQueue`/`IntelligenceCard`) had no equivalent - its action
model (`actionsFor`, `workspace/actions.ts`) was a fixed per-domain
label list dispatched through one generic one-shot `onAction` callback,
with no "edit" action and no inline-input rendering path. The data
Edit needs (`item.summary` equals `suggestion.kind.reference` for a
scripture-kind bible item) was already present in the unified feed,
unused. No genuine architectural fork existed - the target UI (reuse
the existing inline edit pattern) was the only reasonable shape; the
one real constraint was keyboard-shortcut safety (see below).

## What was built

- **`workspace/actions.ts`**: `UnifiedItemAction` gains `"edit"`;
  `actionsFor("bible")` now returns `["display", "reject", "edit"]` -
  edit appended last, not inserted before reject, so
  `resolveUnifiedShortcutAction`'s positional "A"→`actions[0]`/
  "R"→`actions[1]` mapping is untouched (inserting edit earlier would
  have silently reassigned "R" away from reject, a regression to Phase
  6.1's own fix). Only `bible` gets it - `edit_suggestion` is
  inherently scripture-reference-specific; no equivalent free-text-edit
  command exists for music/sermon/content/correlation.
- **`IntelligenceCard.tsx`**: gains `editingId`/`editValue`/
  `onEditValueChange`/`onSaveEdit`/`onCancelEdit` props. When
  `editingId === item.id`, the action-button row is replaced by an
  inline `<input>` + Save/Cancel, mirroring Diagnostics Mode's
  `SuggestionCard` exactly. Clicking "Edit" (when not already editing)
  still dispatches through the existing generic `onAction(item,
  "edit")` path.
- **`AttentionQueue.tsx`**: forwards the same five props straight
  through to each `IntelligenceCard` - still never talks to the
  backend itself, matching its own established discipline.
- **`LiveChurchBrain.tsx`**: `handleUnifiedAction`'s `bible` case
  gains an `edit` branch that sets the exact same `editingId`/
  `editValue` state the Diagnostics Mode Edit flow already uses (no
  new state) - `item.summary` is already the reference text to
  prefill. `<AttentionQueue>` now passes `editingId`/`editValue`/
  `onEditValueChange`/`onCancelEdit`/`onSaveEdit`, with `onSaveEdit`
  calling the exact same `commands.editSuggestion` Diagnostics Mode's
  own Save button calls.
- No new Tauri command, event, or database schema - `edit_suggestion`
  already existed and is unchanged.

## Full regression result

Frontend only - no Rust files changed this phase (the fifth Phase 6.x
frontend-only slice, after 6.2/6.3/6.6/6.7). `npm run typecheck`:
clean. `npm run lint`: same 5 pre-existing warnings as before this
phase - no new ones. `npm run test`: 258/258 passing (257 pre-existing
+ 1 new). `npm run build`: succeeds.

## Windows rebuild

Frontend-only change - see
`pilot-evidence/6.8/windows/installer-contents-verification.json` for
the rebuild's direct binary verification, following the same
strings-tooling-limitation disclosure established in Phase 6.1-6.7.

## Architectural safety diff

- Zero new Tauri commands, zero new events, zero new database
  columns/tables - `edit_suggestion` (Phase 3.5) is reused entirely
  unchanged.
- The unified queue's Edit reuses the exact same `editingId`/
  `editValue` state Diagnostics Mode's Pending Suggestions panel
  already uses - editing the same `Suggestion` from either surface can
  never desync, since they share one state, not two independent copies.
- Every other unified-queue action (display, reject, accept,
  acknowledge, review, dismiss) is byte-identical to before - Edit is
  strictly additive to the `bible` domain's action list.
- `resolveUnifiedShortcutAction`'s "A"/"R" keyboard shortcuts are
  unaffected - `actions[0]`/`actions[1]` still resolve to "display"/
  "reject" exactly as before, verified by a new dedicated test.

## Environment A / B / C

- **Environment A** (this container): PASSED - full frontend regression
  green as detailed above, including a new test confirming edit's
  position never displaces reject from the keyboard-shortcut-relevant
  `actions[1]` slot.
- **Environment B**: unavailable in this session's container, a
  pre-existing, already-documented limitation - not this phase's
  regression.
- **Environment C** (real Windows hardware, a real live service): NOT YET
  VERIFIED. The decisive pending gate is the operator's own real-hardware
  test: with a Bible reference in the Needs Attention queue, click
  Edit, confirm the action buttons are replaced by an input pre-filled
  with the current reference, type a different valid reference, click
  Save, and confirm the queue updates to the new reference; confirm
  pressing A/R on the top item still displays/rejects it exactly as
  before, unaffected by Edit's presence.

## Known limitations

- **Only the `bible` domain has Edit** - music/sermon/content/
  correlation findings have no equivalent free-text-edit command to
  wire up; this mirrors Diagnostics Mode's own existing scope exactly,
  not a new boundary this phase introduces.
- **The "E" keyboard shortcut is not extended to the unified queue** -
  `resolveUnifiedShortcutAction` only resolves "A"/"R" positionally;
  Diagnostics Mode's own "E"/"P" shortcuts remain Diagnostics-Mode-only,
  same as before this phase. A future slice could add a third
  positional key if the operator asks for it.
- **Editing a non-scripture-kind bible suggestion (e.g. a paraphrase
  match) through this new Edit button will fail the same way it
  already does in Diagnostics Mode** - `edit_suggestion` always parses
  the typed text as a book/chapter/verse reference; this is pre-existing
  behavior, not a new gap.
- **All 8 ergonomics gaps from Phase 6's own original audit are now
  closed.** No further Phase 6.x slices are planned unless a new gap
  is found.
- **This exact rebuilt artifact has NOT yet been installed or launched
  on real Windows hardware** - see `physicalHardwareStatement` in
  `release/windows/release-manifest.json`.

## Deferred work

- Real-hardware Environment C verification.
- Any future ergonomics gap found in actual pilot use.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, outside this container's reach). This phase adds one new
per-domain action reusing an existing command and existing state - it
introduces no new backend surface and changes no other action's
behavior.
