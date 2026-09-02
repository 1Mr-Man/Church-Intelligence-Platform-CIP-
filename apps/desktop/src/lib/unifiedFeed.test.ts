import { describe, expect, it } from "vitest";
import type {
  ConfidenceResult,
  ContentCandidate,
  IntelligenceCorrelation,
  IntelligenceFinding,
  Suggestion,
} from "../domain";
import { buildUnifiedFeed, MAX_VISIBLE_INTELLIGENCE_ITEMS, type UnifiedFeedSources } from "./unifiedFeed";

function confidence(score: number): ConfidenceResult {
  return { score, level: score >= 0.8 ? "high" : score >= 0.5 ? "medium" : "low", source: "heuristic", reason: null };
}

function suggestion(overrides: Partial<Suggestion> = {}): Suggestion {
  return {
    id: "sug-1",
    serviceId: "svc-1",
    kind: { type: "scripture", reference: "ROM 8:28" },
    status: "pending",
    confidence: confidence(0.9),
    createdAt: "2026-01-01T10:00:00Z",
    transcriptSegmentId: "seg-1",
    sourceText: "for we know that all things work together for good",
    confirmationCount: 0,
    rejectionEchoCount: 0,
    ...overrides,
  };
}

function finding(overrides: Partial<IntelligenceFinding> = {}): IntelligenceFinding {
  return {
    id: "find-1",
    serviceId: "svc-1",
    domain: "music",
    kind: "music",
    assertionLevel: "inferred",
    status: "detected",
    priority: "normal",
    confidence: confidence(0.75),
    summary: "Amazing Grace recognized",
    transcriptSegmentIds: ["seg-2"],
    sermonId: null,
    evidence: [{ kind: "content", contentId: "hymn:amazing-grace" }],
    provenance: { contentId: "hymn:amazing-grace", note: null },
    engineId: "music",
    engineVersion: "1.0",
    createdAt: "2026-01-01T10:01:00Z",
    ...overrides,
  };
}

function candidate(overrides: Partial<ContentCandidate> = {}): ContentCandidate {
  return {
    id: "cand-1",
    serviceId: "svc-1",
    sermonId: null,
    sourceFindingIds: ["find-sermon-1"],
    candidateType: "theme",
    titleOrLabel: "Theme: faith",
    workingConcept: "Theme: faith",
    assertionLevel: "suggested",
    status: "detected",
    confidence: confidence(0.7),
    contentPotential: 0.5,
    evidence: [{ kind: "another_finding", findingId: "find-sermon-1" }],
    provenance: { contentId: null, note: null },
    engineId: "sermon-content",
    engineVersion: "1.0",
    createdAt: "2026-01-01T10:02:00Z",
    ...overrides,
  };
}

function correlation(overrides: Partial<IntelligenceCorrelation> = {}): IntelligenceCorrelation {
  return {
    id: "corr-1",
    serviceId: "svc-1",
    sourceFindingIds: ["find-a", "find-b"],
    domains: ["bible", "sermon"],
    kind: { kind: "scripture_sermon" },
    assertionLevel: "inferred",
    status: "detected",
    confidence: confidence(0.95),
    summary: "Sermon references ROM 8:28, matching Bible finding ROM 8:28",
    evidence: [],
    ruleId: "scripture_sermon_v1",
    ruleVersion: "1.0",
    createdAt: "2026-01-01T10:03:00Z",
    ...overrides,
  };
}

function emptySources(): UnifiedFeedSources {
  return {
    suggestions: [],
    musicFindings: [],
    sermonFindings: [],
    serviceTransitions: [],
    serviceAnomalies: [],
    contentCandidates: [],
    correlations: [],
  };
}

describe("buildUnifiedFeed", () => {
  it("maps every domain into the feed with its domain identity preserved", () => {
    const feed = buildUnifiedFeed({
      suggestions: [suggestion()],
      musicFindings: [finding({ id: "m-1", domain: "music" })],
      sermonFindings: [finding({ id: "s-1", domain: "sermon", summary: "Main Point: faith" })],
      serviceTransitions: [],
      serviceAnomalies: [finding({ id: "sv-1", domain: "service", summary: "Anomaly: unexpected phase order" })],
      contentCandidates: [candidate()],
      correlations: [correlation()],
    });

    const byDomain = Object.fromEntries(feed.map((item) => [item.domain, item]));
    expect(byDomain.bible?.id).toBe("sug-1");
    expect(byDomain.music?.id).toBe("m-1");
    expect(byDomain.sermon?.id).toBe("s-1");
    expect(byDomain.service?.id).toBe("sv-1");
    expect(byDomain.content?.id).toBe("cand-1");
    expect(byDomain.correlation?.id).toBe("corr-1");
    expect(feed).toHaveLength(6);
  });

  it("orders newest first", () => {
    const feed = buildUnifiedFeed({
      ...emptySources(),
      musicFindings: [
        finding({ id: "old", createdAt: "2026-01-01T09:00:00Z" }),
        finding({ id: "new", createdAt: "2026-01-01T11:00:00Z" }),
      ],
    });
    expect(feed.map((i) => i.id)).toEqual(["new", "old"]);
  });

  it("breaks createdAt ties by confidence descending, deterministically", () => {
    const feed = buildUnifiedFeed({
      ...emptySources(),
      musicFindings: [
        finding({ id: "low", createdAt: "2026-01-01T09:00:00Z", confidence: confidence(0.3) }),
        finding({ id: "high", createdAt: "2026-01-01T09:00:00Z", confidence: confidence(0.9) }),
      ],
    });
    expect(feed.map((i) => i.id)).toEqual(["high", "low"]);
  });

  it("produces the same order across repeated calls with the same input (determinism)", () => {
    const sources: UnifiedFeedSources = {
      ...emptySources(),
      musicFindings: [finding({ id: "a" }), finding({ id: "b", createdAt: "2026-01-01T10:01:00Z" })],
      sermonFindings: [finding({ id: "c", domain: "sermon" })],
    };
    const first = buildUnifiedFeed(sources).map((i) => i.id);
    for (let i = 0; i < 10; i++) {
      expect(buildUnifiedFeed(sources).map((item) => item.id)).toEqual(first);
    }
  });

  it("bounds the feed to MAX_VISIBLE_INTELLIGENCE_ITEMS even with far more input", () => {
    const musicFindings = Array.from({ length: MAX_VISIBLE_INTELLIGENCE_ITEMS + 50 }, (_, i) =>
      finding({ id: `m-${i}`, createdAt: `2026-01-01T10:${String(i % 60).padStart(2, "0")}:00Z` }),
    );
    const feed = buildUnifiedFeed({ ...emptySources(), musicFindings });
    expect(feed.length).toBe(MAX_VISIBLE_INTELLIGENCE_ITEMS);
  });

  it("never duplicates an item across domains", () => {
    const feed = buildUnifiedFeed({
      ...emptySources(),
      musicFindings: [finding({ id: "m-1" })],
      sermonFindings: [finding({ id: "s-1", domain: "sermon" })],
    });
    const ids = feed.map((i) => i.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("labels a pending Suggestion as the suggested assertion level, never fabricating one", () => {
    const feed = buildUnifiedFeed({ ...emptySources(), suggestions: [suggestion()] });
    expect(feed[0].assertionLevel).toBe("suggested");
    expect(feed[0].rawStatus).toBe("pending");
  });

  it("retains the real underlying source object for evidence/provenance traceability", () => {
    const f = finding();
    const feed = buildUnifiedFeed({ ...emptySources(), musicFindings: [f] });
    expect(feed[0].source).toBe(f);
    expect(feed[0].evidenceCount).toBe(f.evidence.length);
  });

  it("marks Detected/Reviewed findings as needing attention, and Accepted/Rejected as resolved", () => {
    const feed = buildUnifiedFeed({
      ...emptySources(),
      musicFindings: [
        finding({ id: "pending", status: "detected" }),
        finding({ id: "reviewed", status: "reviewed" }),
        finding({ id: "accepted", status: "accepted" }),
        finding({ id: "rejected", status: "rejected" }),
      ],
    });
    const byId = Object.fromEntries(feed.map((i) => [i.id, i.needsAttention]));
    expect(byId.pending).toBe(true);
    expect(byId.reviewed).toBe(true);
    expect(byId.accepted).toBe(false);
    expect(byId.rejected).toBe(false);
  });

  it("marks pending/edited Suggestions as needing attention, and approved/rejected as resolved", () => {
    const feed = buildUnifiedFeed({
      ...emptySources(),
      suggestions: [
        suggestion({ id: "p", status: "pending" }),
        suggestion({ id: "e", status: "edited" }),
        suggestion({ id: "a", status: "approved" }),
        suggestion({ id: "r", status: "rejected" }),
      ],
    });
    const byId = Object.fromEntries(feed.map((i) => [i.id, i.needsAttention]));
    expect(byId.p).toBe(true);
    expect(byId.e).toBe(true);
    expect(byId.a).toBe(false);
    expect(byId.r).toBe(false);
  });

  it("never marks a service transition as needing attention (no accept/reject action exists for one)", () => {
    const feed = buildUnifiedFeed({
      ...emptySources(),
      serviceTransitions: [finding({ id: "t-1", domain: "service", status: "detected" })],
    });
    expect(feed[0].needsAttention).toBe(false);
    expect(feed[0].rawStatus).toBe("detected");
  });

  it("marks a service anomaly as needing attention while still detected (acknowledgeServiceAnomaly exists)", () => {
    const feed = buildUnifiedFeed({
      ...emptySources(),
      serviceAnomalies: [finding({ id: "a-1", domain: "service", status: "detected" })],
    });
    expect(feed[0].needsAttention).toBe(true);
  });

  it("returns an empty feed for empty sources, never throwing", () => {
    expect(buildUnifiedFeed(emptySources())).toEqual([]);
  });

  it("carries a correlation's rule id as its detail line", () => {
    const feed = buildUnifiedFeed({ ...emptySources(), correlations: [correlation()] });
    expect(feed[0].detailLine).toBe("scripture_sermon_v1");
  });
});
