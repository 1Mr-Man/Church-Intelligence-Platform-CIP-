# Phase 6.8 — Operator Ergonomics: Unified-Queue Edit Support Audit

## Baseline

Phase 6.7 closed Diagnostics Mode density. This phase closes gap #8 -
the last of Phase 6's original 8 audit gaps: "unified-queue Edit
support." Phase 6.1's own report named the reason directly: "'E'
(edit) ... remain Diagnostics-mode-only, since editing a reference has
no equivalent action in the domain-generic unified action model yet."

## What's actually there

- **Diagnostics Mode's Pending Suggestions panel** (`SuggestionCard` in
  `LiveChurchBrain.tsx`) already has a full Edit flow: an "Edit" button
  swaps the card into an inline `<input>` + Save/Cancel, backed by
  `editingId`/`editValue` state and `commands.editSuggestion(id,
  newReference)` - which re-parses the typed text as a scripture
  reference and validates it's a real verse before saving.
- **The Needs Attention queue** (`AttentionQueue`/`IntelligenceCard`,
  Operator Mode's primary panel) has no equivalent. Its action set
  (`actionsFor` in `workspace/actions.ts`) is a fixed label list per
  domain, dispatched through one generic `onAction(item, action)`
  callback (`handleUnifiedAction`) that always calls a single backend
  command per click - there is no "edit" action anywhere in that model,
  and `IntelligenceCard` has no inline-input rendering path at all.
- `UnifiedIntelligenceItem.source` for a bible item is the exact same
  `Suggestion` object `SuggestionCard` already edits, and
  `item.summary` is already exactly `suggestion.kind.reference` for a
  scripture-kind suggestion - the data Edit needs is already present in
  the unified feed, unused.

## Design choice

The shape is not a genuine fork: the target UI (an inline input +
Save/Cancel replacing the action-button row while editing) is the
existing, already-shipped pattern from `SuggestionCard` - reusing it
rather than inventing a second shape is the only reasonable choice.
The one real engineering constraint is keyboard-shortcut safety:
`resolveUnifiedShortcutAction` maps "A" to `actions[0]` and "R" to
`actions[1]` positionally - inserting `"edit"` between `"display"` and
`"reject"` in `actionsFor("bible")` would silently reassign "R" to
edit instead of reject, a real regression to Phase 6.1's own fix.
Appending `"edit"` at the end of the array avoids this with zero
changes to `resolveUnifiedShortcutAction` or its tests. No operator
design question exists here - proceeding directly to implementation.

## Scope boundary

Only the `bible` domain gets an `"edit"` action - `editSuggestion` is
inherently scripture-reference-specific (it parses the typed text as a
book/chapter/verse and validates it against the current translation);
no equivalent free-text-edit command exists for music/sermon/content/
correlation findings. This mirrors Diagnostics Mode's own existing
scope exactly - `SuggestionCard`'s Edit button already only ever
applies to `Suggestion` objects, never to `IntelligenceFinding`/
`ContentCandidate`/`IntelligenceCorrelation`.

The "E" keyboard shortcut is not extended to the unified queue this
phase - `resolveUnifiedShortcutAction` only resolves "A"/"R"
positionally today, and adding a third positional key is out of scope
for what "unified-queue Edit support" (a button on the card) actually
requires. Documented as a known limitation, not silently dropped.
