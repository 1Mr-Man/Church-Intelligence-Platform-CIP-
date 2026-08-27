/**
 * The Unified Operator Workspace's "what needs attention" region (Phase
 * 2.9, per the authoritative Phase 2 roadmap). Deliberately not a new
 * confidence/priority system (spec section 7): the ranking reuses each
 * item's existing `confidence.score` exactly as `core/intelligence`
 * produced it, tie-broken deterministically. High-confidence information
 * is never hidden merely because it isn't actionable - this module only
 * ever narrows to items that already need a decision
 * (`UnifiedIntelligenceItem.needsAttention`); it never demotes anything
 * for being uninteresting.
 */
import type { UnifiedIntelligenceItem } from "./unifiedFeed";

/** Bound for the attention queue (spec rule 12) - deliberately smaller
 * than `MAX_VISIBLE_INTELLIGENCE_ITEMS`: attention is meant to stay sparse
 * by design, not become a second copy of the full feed. */
export const MAX_VISIBLE_ATTENTION_ITEMS = 8;

/**
 * Deterministic priority ordering (spec section 7's "prefer a simple
 * deterministic ordering"): confidence descending (the same
 * `ConfidenceResult.score` every panel already displays), then newest
 * first, then domain, then id as a final stable tiebreak. Never depends on
 * array/object iteration order.
 */
function compareByAttentionPriority(a: UnifiedIntelligenceItem, b: UnifiedIntelligenceItem): number {
  if (a.confidence.score !== b.confidence.score) return b.confidence.score - a.confidence.score;
  if (a.createdAt !== b.createdAt) return a.createdAt < b.createdAt ? 1 : -1;
  if (a.domain !== b.domain) return a.domain < b.domain ? -1 : 1;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/**
 * Narrow the unified feed down to what genuinely needs an operator
 * decision right now, in priority order. Pure and synchronous.
 */
export function buildAttentionQueue(feed: UnifiedIntelligenceItem[]): UnifiedIntelligenceItem[] {
  return feed
    .filter((item) => item.needsAttention)
    .slice()
    .sort(compareByAttentionPriority)
    .slice(0, MAX_VISIBLE_ATTENTION_ITEMS);
}
