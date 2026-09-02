/**
 * Canonical Unified Operator Workspace acceptance test (Phase 2.9, per the
 * authoritative Phase 2 roadmap, spec section 23E/36). Feeds a realistic
 * full-service scenario - Scripture, sermon, music, correlation, and
 * content-candidate detections, matching the shape Phase 2.8's own
 * canonical walkthrough used - through `buildUnifiedFeed`/
 * `buildAttentionQueue` and proves the workspace layer genuinely
 * coordinates existing capabilities. This is explicitly NOT a second
 * intelligence engine (spec section 17): every input here is a plain,
 * hand-built domain object of exactly the shape the real commands/events
 * already produce - nothing is inferred or detected by this test or by
 * the modules it exercises.
 */
import { describe, expect, it } from "vitest";
import type { ConfidenceResult } from "../domain";
import { buildAttentionQueue } from "./attentionQueue";
import { buildUnifiedFeed, type UnifiedFeedSources } from "./unifiedFeed";

function confidence(score: number): ConfidenceResult {
  return { score, level: score >= 0.8 ? "high" : score >= 0.5 ? "medium" : "low", source: "heuristic", reason: null };
}

describe("canonical operator workflow", () => {
  it("coordinates Scripture, sermon, music, correlation, and content-candidate detections into one bounded, prioritized view", () => {
    const sources: UnifiedFeedSources = {
      suggestions: [
        {
          id: "sug-rom-8-28",
          serviceId: "svc-1",
          kind: { type: "scripture", reference: "ROM 8:28" },
          status: "pending",
          confidence: confidence(0.97),
          createdAt: "2026-01-01T10:05:00Z",
          transcriptSegmentId: "seg-3",
          sourceText: "and we know that all things work together for good",
          confirmationCount: 0,
          rejectionEchoCount: 0,
        },
      ],
      musicFindings: [
        {
          id: "find-amazing-grace",
          serviceId: "svc-1",
          domain: "music",
          kind: "music",
          assertionLevel: "suggested",
          status: "detected",
          priority: "normal",
          confidence: confidence(0.85),
          summary: "Amazing Grace",
          transcriptSegmentIds: ["seg-1"],
          sermonId: null,
          evidence: [{ kind: "content", contentId: "hymn:amazing-grace" }],
          provenance: { contentId: "hymn:amazing-grace", note: null },
          engineId: "music",
          engineVersion: "1.0",
          createdAt: "2026-01-01T10:00:00Z",
        },
      ],
      sermonFindings: [
        {
          id: "find-main-point",
          serviceId: "svc-1",
          domain: "sermon",
          kind: "sermon",
          assertionLevel: "inferred",
          status: "detected",
          priority: "normal",
          confidence: confidence(0.82),
          summary: "Main Point: Trusting God during difficult seasons",
          transcriptSegmentIds: ["seg-4"],
          sermonId: "sermon-1",
          evidence: [],
          provenance: { contentId: null, note: null },
          engineId: "sermon",
          engineVersion: "1.0",
          createdAt: "2026-01-01T10:06:00Z",
        },
      ],
      // A real "Anomaly" finding, not a plain transition - transitions
      // have no accept/reject action and never enter the attention
      // queue (see unifiedFeed.test.ts).
      serviceTransitions: [],
      serviceAnomalies: [
        {
          id: "find-worship-phase",
          serviceId: "svc-1",
          domain: "service",
          kind: "service_state",
          assertionLevel: "observed",
          status: "detected",
          priority: "low",
          confidence: confidence(1.0),
          summary: "Anomaly: phase moved backward from Sermon to Worship",
          transcriptSegmentIds: ["seg-1"],
          sermonId: null,
          evidence: [],
          provenance: { contentId: null, note: null },
          engineId: "service",
          engineVersion: "1.0",
          createdAt: "2026-01-01T09:59:00Z",
        },
      ],
      contentCandidates: [
        {
          id: "cand-trusting-god",
          serviceId: "svc-1",
          sermonId: "sermon-1",
          sourceFindingIds: ["find-main-point"],
          candidateType: "teaching",
          titleOrLabel: "Teaching: Trusting God during difficult seasons",
          workingConcept: "Main Point: Trusting God during difficult seasons",
          assertionLevel: "suggested",
          status: "detected",
          confidence: confidence(0.82),
          contentPotential: 0.6,
          evidence: [{ kind: "another_finding", findingId: "find-main-point" }],
          provenance: { contentId: null, note: null },
          engineId: "sermon-content",
          engineVersion: "1.0",
          createdAt: "2026-01-01T10:07:00Z",
        },
      ],
      correlations: [
        {
          id: "corr-scripture-sermon",
          serviceId: "svc-1",
          sourceFindingIds: ["find-main-point", "sug-rom-8-28"],
          domains: ["sermon", "bible"],
          kind: { kind: "scripture_sermon" },
          assertionLevel: "inferred",
          status: "detected",
          confidence: confidence(0.95),
          summary: "Sermon references ROM 8:28, matching Bible finding ROM 8:28",
          evidence: [],
          ruleId: "scripture_sermon_v1",
          ruleVersion: "1.0",
          createdAt: "2026-01-01T10:08:00Z",
        },
      ],
    };

    const feed = buildUnifiedFeed(sources);

    // Every domain the service actually produced findings for appears.
    const domains = new Set(feed.map((i) => i.domain));
    expect(domains).toEqual(new Set(["bible", "music", "sermon", "service", "content", "correlation"]));

    // Each item retains its real domain identity and underlying source -
    // the feed never flattens six different data shapes into one lossy
    // record (spec rule 8).
    for (const item of feed) {
      expect(item.source).toBeDefined();
      expect(item.confidence.score).toBeGreaterThan(0);
    }

    // Everything here is still pending an operator decision.
    const attention = buildAttentionQueue(feed);
    expect(attention.length).toBe(feed.length);

    // The attention queue is sorted strictly by confidence descending -
    // the observed service-phase finding (1.0, deterministic/certain)
    // genuinely outranks the inferred correlation (0.95). This is
    // intentional: spec section 7 explicitly forbids hiding high-
    // confidence information merely because it is less "interesting."
    const scores = attention.map((i) => i.confidence.score);
    expect(scores).toEqual([...scores].sort((a, b) => b - a));
    expect(attention[0].id).toBe("find-worship-phase");

    // The correlation still appears, with its explanation inspectable -
    // "why did CIP produce this" (spec rule 8) - via its rule id and
    // summary.
    const correlationItem = attention.find((i) => i.domain === "correlation");
    expect(correlationItem?.detailLine).toBe("scripture_sermon_v1");
    expect(correlationItem?.summary).toContain("ROM 8:28");
  });

  it("resolving an item (accept/reject) removes it from the attention queue without deleting evidence", () => {
    // Mirrors the real operator flow: the backend command changes the
    // underlying object's status, an event delivers the updated object,
    // and the frontend re-runs buildUnifiedFeed/buildAttentionQueue over
    // the new state - never mutates the old item in place.
    const detected: UnifiedFeedSources = {
      suggestions: [],
      musicFindings: [
        {
          id: "find-1",
          serviceId: "svc-1",
          domain: "music",
          kind: "music",
          assertionLevel: "suggested",
          status: "detected",
          priority: "normal",
          confidence: confidence(0.8),
          summary: "Amazing Grace",
          transcriptSegmentIds: [],
          sermonId: null,
          evidence: [{ kind: "content", contentId: "hymn:amazing-grace" }],
          provenance: { contentId: "hymn:amazing-grace", note: null },
          engineId: "music",
          engineVersion: "1.0",
          createdAt: "2026-01-01T10:00:00Z",
        },
      ],
      sermonFindings: [],
      serviceTransitions: [],
      serviceAnomalies: [],
      contentCandidates: [],
      correlations: [],
    };
    const beforeAccept = buildAttentionQueue(buildUnifiedFeed(detected));
    expect(beforeAccept).toHaveLength(1);

    const accepted: UnifiedFeedSources = {
      ...detected,
      musicFindings: [{ ...detected.musicFindings[0], status: "accepted" }],
    };
    const afterAccept = buildAttentionQueue(buildUnifiedFeed(accepted));
    expect(afterAccept).toHaveLength(0);

    // The accepted item's evidence is still present in the full feed -
    // acceptance never deletes provenance, only changes status.
    const feedAfterAccept = buildUnifiedFeed(accepted);
    expect(feedAfterAccept[0].evidenceCount).toBe(1);
    expect(feedAfterAccept[0].rawStatus).toBe("accepted");
  });
});
