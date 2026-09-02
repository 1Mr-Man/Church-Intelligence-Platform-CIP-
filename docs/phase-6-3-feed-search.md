# Phase 6.3 — Operator Ergonomics: Feed Search

## Baseline

Phase 6.2 closed Display confirmation/undo. This phase closes gap #3 from
Phase 6's own audit: "no text search over the live feed" - see
`docs/phase-6-3-audit.md` for the full breakdown.

## Audit

`IntelligenceFeed.tsx` renders the full cross-domain feed (bounded to
`MAX_VISIBLE_INTELLIGENCE_ITEMS = 50` upstream), already filterable by
domain via chip buttons (local state + an inline `useMemo`), but had no
text search at all. The "S" keyboard shortcut's `searchInputRef` is a
completely unrelated, pre-existing feature (Phase 1.3's Manual Bible
Search box) - not reusable here. `filterBooksByPrefix`
(`lib/libraryHelpers.ts`, Phase 4.2) is this codebase's closest
precedent: a pure, case-insensitive filter function with a dedicated
test file - prefix-only, right for a short book name, but a feed of full
sentences needs substring matching so a mid-summary word still matches.

Unlike Phase 6.2, this gap has one clear shape rather than a genuine
architectural fork, so this phase proceeded directly to implementation.

## What was built

- **`apps/desktop/src/lib/intelligenceFeed.ts`** (new):
  `searchIntelligenceFeed(items, query)` - case-insensitive substring
  match against `summary` and `detailLine` (null-safe), empty/
  whitespace-only query returns `items` unchanged. Pure, DOM-free.
- **`apps/desktop/src/components/workspace/IntelligenceFeed.tsx`**: a new
  text input composes with the existing domain filter chips (both narrow
  the same already-fetched `items` array); the empty-state message now
  reads "matching this filter" whenever either filter is active, not
  just for the domain chips.
- **Tests**: 6 new cases for `searchIntelligenceFeed`
  (`intelligenceFeed.test.ts`) - mid-string match, detailLine fallback,
  null-safety, whitespace trimming, empty-query passthrough, no-match
  empty result.

## Full regression result

Frontend only - no Rust files changed this phase (confirmed via git
status/diff). `npm run typecheck`: clean. `npm run lint`: same
pre-existing warnings as before this phase - no new ones. `npm run
test`: 242/242 passing (236 pre-existing + 6 new). `npm run build`:
succeeds.

## Windows rebuild

Frontend-only change - see
`pilot-evidence/6.3/windows/installer-contents-verification.json` for
the rebuild's direct binary verification, following the same
strings-tooling-limitation disclosure established in Phase 6.1/6.2.

## Architectural safety diff

- Zero new Tauri commands, zero new events, zero new database
  columns/tables - the search is a pure client-side filter over data
  already fetched for the existing feed.
- No change to the 50-item cap (`MAX_VISIBLE_INTELLIGENCE_ITEMS`,
  `lib/unifiedFeed.ts`) or to how/when the feed's underlying data is
  fetched.
- The panel remains read-only: search never becomes a second action
  surface, matching the panel's own existing doc comment.
- The existing domain filter chips are unchanged in behavior - search
  composes with them (AND), it does not replace them.

## Environment A / B / C

- **Environment A** (this container): PASSED - full frontend regression
  green as detailed above, including 6 new unit tests for
  `searchIntelligenceFeed`.
- **Environment B**: unavailable in this session's container, a
  pre-existing, already-documented limitation - not this phase's
  regression.
- **Environment C** (real Windows hardware, a real live service): NOT YET
  VERIFIED. The decisive pending gate is the operator's own real-hardware
  test: with a live feed containing more than a few items, type a word
  that only appears in one item's summary and confirm the feed narrows to
  just that item, then clear the box and confirm every item returns.

## Known limitations

- **Search matches only `summary`/`detailLine`, not domain or status
  text** - those are already covered by the existing filter chips, so
  searching them too would duplicate an existing, more discoverable
  control.
- **Substring matching only, no fuzzy/typo-tolerant matching** - an
  exact substring (case-insensitive) is required; a misspelled search
  term will not match.
- **The search box has no debounce** - filtering re-runs on every
  keystroke via `useMemo`, acceptable given the feed is capped at 50
  items; this could need revisiting if the cap ever grows substantially.
- **5 more ergonomics gaps from Phase 6's own audit remain unaddressed**
  after this phase (error visibility, error-banner dismiss/context,
  onboarding, Diagnostics Mode density, unified-queue Edit support) -
  each a candidate for a future Phase 6.x slice.
- **This exact rebuilt artifact has NOT yet been installed or launched on
  real Windows hardware** - see `physicalHardwareStatement` in
  `release/windows/release-manifest.json`.

## Deferred work

- The remaining Phase 6 ergonomics gaps from the original audit.
- Real-hardware Environment C verification.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, outside this container's reach). This phase adds a pure,
read-only, client-side filter over data the feed already fetches - it
introduces no new backend surface and changes no existing action's
behavior.
