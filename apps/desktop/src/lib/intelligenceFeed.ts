import type { UnifiedIntelligenceItem } from "./unifiedFeed";

/**
 * Phase 6.3 (Operator Ergonomics: feed search) - filters `items` to those
 * whose `summary` or `detailLine` contains `query` (case-insensitive,
 * whitespace-trimmed). An empty/whitespace-only query returns `items`
 * unchanged, matching `libraryHelpers.ts::filterBooksByPrefix`'s own
 * "no query = show everything" convention. Substring, not prefix,
 * matching - unlike a short book name, a feed summary is a full sentence
 * ("Amazing Grace recognized"), so a mid-string word like "grace" still
 * needs to match. Deliberately scoped to `summary`/`detailLine` only:
 * domain and status are already covered by `IntelligenceFeed`'s own
 * filter chips, so searching them too would just duplicate an existing,
 * more discoverable control. Pure and DOM-free by design (this project
 * has no DOM testing environment configured), mirroring every other
 * filter/dispatch helper pulled into `lib/*.ts` for the same reason.
 */
export function searchIntelligenceFeed(items: UnifiedIntelligenceItem[], query: string): UnifiedIntelligenceItem[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return items;
  return items.filter(
    (item) => item.summary.toLowerCase().includes(needle) || (item.detailLine?.toLowerCase().includes(needle) ?? false),
  );
}
