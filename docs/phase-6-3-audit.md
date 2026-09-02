# Phase 6.3 — Audit: Feed Search

## Baseline

Phase 6.2 closed Display confirmation/undo. This audit opens gap #3 from
Phase 6's own audit: "no text search over the live feed"
(`docs/phase-6-1-operator-ergonomics-shortcuts.md`).

## What exists today

`apps/desktop/src/components/workspace/IntelligenceFeed.tsx` renders the
full cross-domain, chronological feed (`unifiedFeed`, bounded to
`MAX_VISIBLE_INTELLIGENCE_ITEMS = 50` upstream in
`lib/unifiedFeed.ts`), with domain filter chips (All/Bible/Music/Sermon/
Service/Content/Correlations) - filtering is entirely client-side over
the already-fetched array via local `useState` + an inline `useMemo`.
There is no text search anywhere in this component. The "S" keyboard
shortcut's `searchInputRef` is a completely unrelated, pre-existing
feature (Phase 1.3's Manual Bible Search box) - not reusable here.

Each `UnifiedIntelligenceItem` carries `summary: string` and
`detailLine: string | null` as its meaningful free-text fields (domain/
status are already covered by the filter chips, so searching them too
would be redundant with an existing, more discoverable control).

## Precedent

`lib/libraryHelpers.ts::filterBooksByPrefix` (Phase 4.2) is this
codebase's established shape for a text filter: a pure, case-insensitive,
trimmed function taking a list + a query string, with "empty query
returns the list unchanged" as the default state, and a dedicated test
file. `filterBooksByPrefix` is prefix-only (right for a short book-name
autocomplete); a feed of full sentences needs substring matching instead
so a mid-summary word like "grace" still finds "Amazing Grace
recognized."

## Design (no fork - proceeding directly)

Unlike Phase 6.2, this gap has one clear shape, not a genuine
architectural choice between comparably-sized options, so this phase
proceeds straight to implementation rather than asking the operator to
pick:

- New pure function `searchIntelligenceFeed(items, query)`
  (`lib/intelligenceFeed.ts`, new file - mirroring why `shortcutLegend`
  was moved out of `AttentionQueue.tsx` in Phase 6.1: keeping pure logic
  out of component files avoids an oxlint `only-export-components`
  warning and keeps it unit-testable with no DOM). Case-insensitive
  substring match against `summary` and `detailLine`; an empty/
  whitespace-only query returns the input unchanged, matching
  `filterBooksByPrefix`'s own convention.
- `IntelligenceFeed.tsx` gains a text input alongside the existing
  domain chips; both filters compose (domain AND text) over the same
  already-fetched `items` array - no new command, no new fetch, no
  change to the 50-item cap.
- Read-only, exactly like the rest of this panel - search never becomes
  a second action surface.
