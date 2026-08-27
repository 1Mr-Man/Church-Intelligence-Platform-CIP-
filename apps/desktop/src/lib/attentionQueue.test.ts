import { describe, expect, it } from "vitest";
import type { ConfidenceResult } from "../domain";
import { buildAttentionQueue, MAX_VISIBLE_ATTENTION_ITEMS } from "./attentionQueue";
import type { UnifiedIntelligenceItem } from "./unifiedFeed";

function confidence(score: number): ConfidenceResult {
  return { score, level: score >= 0.8 ? "high" : score >= 0.5 ? "medium" : "low", source: "heuristic", reason: null };
}

function item(overrides: Partial<UnifiedIntelligenceItem> = {}): UnifiedIntelligenceItem {
  return {
    id: "item-1",
    domain: "music",
    summary: "Amazing Grace recognized",
    confidence: confidence(0.7),
    assertionLevel: "inferred",
    rawStatus: "detected",
    needsAttention: true,
    createdAt: "2026-01-01T10:00:00Z",
    detailLine: null,
    evidenceCount: 1,
    source: {} as UnifiedIntelligenceItem["source"],
    ...overrides,
  };
}

describe("buildAttentionQueue", () => {
  it("includes only items that need attention", () => {
    const queue = buildAttentionQueue([
      item({ id: "pending", needsAttention: true }),
      item({ id: "resolved", needsAttention: false }),
    ]);
    expect(queue.map((i) => i.id)).toEqual(["pending"]);
  });

  it("orders by confidence descending, never hiding high-confidence items", () => {
    const queue = buildAttentionQueue([
      item({ id: "low", confidence: confidence(0.3) }),
      item({ id: "high", confidence: confidence(0.95) }),
      item({ id: "mid", confidence: confidence(0.6) }),
    ]);
    expect(queue.map((i) => i.id)).toEqual(["high", "mid", "low"]);
  });

  it("breaks confidence ties by newest first, then domain, then id - deterministically", () => {
    const a = item({ id: "a", domain: "bible", confidence: confidence(0.8), createdAt: "2026-01-01T10:00:00Z" });
    const b = item({ id: "b", domain: "music", confidence: confidence(0.8), createdAt: "2026-01-01T10:00:00Z" });
    const queue1 = buildAttentionQueue([a, b]).map((i) => i.id);
    const queue2 = buildAttentionQueue([b, a]).map((i) => i.id);
    expect(queue1).toEqual(queue2);
    expect(queue1).toEqual(["a", "b"]); // bible < music alphabetically
  });

  it("bounds the queue to MAX_VISIBLE_ATTENTION_ITEMS", () => {
    const items = Array.from({ length: MAX_VISIBLE_ATTENTION_ITEMS + 20 }, (_, i) =>
      item({ id: `i-${i}`, confidence: confidence(Math.random()) }),
    );
    expect(buildAttentionQueue(items).length).toBe(MAX_VISIBLE_ATTENTION_ITEMS);
  });

  it("returns an empty queue when nothing needs attention", () => {
    expect(buildAttentionQueue([item({ needsAttention: false })])).toEqual([]);
  });

  it("is a pure function - never mutates the input array", () => {
    const items = [item({ id: "a", confidence: confidence(0.2) }), item({ id: "b", confidence: confidence(0.9) })];
    const originalOrder = items.map((i) => i.id);
    buildAttentionQueue(items);
    expect(items.map((i) => i.id)).toEqual(originalOrder);
  });
});
