import { describe, expect, it } from "vitest";
import { confidenceLevelFromScore, isAutoAcceptable } from "./confidence";

describe("confidenceLevelFromScore", () => {
  it("mirrors the Rust bucketing thresholds", () => {
    expect(confidenceLevelFromScore(0.95)).toBe("high");
    expect(confidenceLevelFromScore(0.8)).toBe("high");
    expect(confidenceLevelFromScore(0.65)).toBe("medium");
    expect(confidenceLevelFromScore(0.5)).toBe("medium");
    expect(confidenceLevelFromScore(0.1)).toBe("low");
  });
});

describe("isAutoAcceptable", () => {
  it("is true only for high confidence, per Phase 1 policy", () => {
    expect(isAutoAcceptable({ level: "high" })).toBe(true);
    expect(isAutoAcceptable({ level: "medium" })).toBe(false);
    expect(isAutoAcceptable({ level: "low" })).toBe(false);
  });
});
