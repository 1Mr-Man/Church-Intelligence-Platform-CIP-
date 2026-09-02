import { describe, expect, it } from "vitest";
import type { ConfidenceResult } from "../domain";
import { searchIntelligenceFeed } from "./intelligenceFeed";
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
    needsAttention: false,
    createdAt: "2026-01-01T10:00:00Z",
    detailLine: null,
    evidenceCount: 1,
    source: {} as UnifiedIntelligenceItem["source"],
    ...overrides,
  };
}

describe("searchIntelligenceFeed", () => {
  const items = [
    item({ id: "a", summary: "Amazing Grace recognized" }),
    item({ id: "b", summary: "MAT 6:9 detected", detailLine: "The Lord's Prayer" }),
    item({ id: "c", summary: "Service phase changed to Worship" }),
  ];

  it("matches a mid-string word in the summary, case-insensitively", () => {
    expect(searchIntelligenceFeed(items, "grace").map((i) => i.id)).toEqual(["a"]);
    expect(searchIntelligenceFeed(items, "GRACE").map((i) => i.id)).toEqual(["a"]);
  });

  it("matches against detailLine when summary doesn't match", () => {
    expect(searchIntelligenceFeed(items, "lord's prayer").map((i) => i.id)).toEqual(["b"]);
  });

  it("never throws on an item with a null detailLine", () => {
    expect(() => searchIntelligenceFeed(items, "worship")).not.toThrow();
    expect(searchIntelligenceFeed(items, "worship").map((i) => i.id)).toEqual(["c"]);
  });

  it("trims whitespace from the query before matching", () => {
    expect(searchIntelligenceFeed(items, "  grace  ").map((i) => i.id)).toEqual(["a"]);
  });

  it("returns every item unchanged for an empty or whitespace-only query", () => {
    expect(searchIntelligenceFeed(items, "")).toEqual(items);
    expect(searchIntelligenceFeed(items, "   ")).toEqual(items);
  });

  it("returns an empty list when nothing matches, never guessing a fallback", () => {
    expect(searchIntelligenceFeed(items, "zzz")).toEqual([]);
  });
});
