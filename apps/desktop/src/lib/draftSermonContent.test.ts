import { describe, expect, it } from "vitest";
import { buildSermonDraftPack } from "./draftSermonContent";

describe("buildSermonDraftPack", () => {
  it("creates review-only drafts from accepted concepts and transcript excerpts", () => {
    const pack = buildSermonDraftPack(
      "Sunday Service",
      [{ id: "t1", sequence: 0, text: "Walk by faith.", isFinal: true, confidence: { score: 0.9, level: "high", source: "model", reason: null }, startMs: 0, endMs: 1000, language: "en", speakerId: null }],
      [{ id: "c1", serviceId: "s1", sermonId: null, sourceFindingIds: ["f1"], candidateType: "theme", titleOrLabel: "Faith", workingConcept: "Trust God in uncertainty.", assertionLevel: "observed", status: "accepted", confidence: { score: 0.9, level: "high", source: "human", reason: null }, contentPotential: 0.8, evidence: [], provenance: { contentId: null, note: null }, engineId: "test", engineVersion: "1", createdAt: "2026-01-01T00:00:00Z" }],
    );
    expect(pack.notes).toContain("Trust God in uncertainty.");
    expect(pack.slides).toContain("Trust God in uncertainty.");
    expect(pack.devotional).toContain("review wording");
    expect(pack.newsletter).toContain("Sunday Service");
  });
});
