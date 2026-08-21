/**
 * These tests exist primarily as compile-time proof that the Phase 1
 * domain contracts have the shape this file (and therefore any real
 * consumer) expects - if a field is renamed or removed on either side of
 * the Rust/TS mirror, this file fails to type-check before it fails at
 * runtime.
 */
import { describe, expect, it } from "vitest";
import type { AmbiguousCandidate, ScriptureContext, BibleTranslation, ScriptureReference } from "./bible";
import type { ConfidenceResult } from "./confidence";
import type { ProcessedSegment, ScriptureDetection, Suggestion, TranscriptSegment } from "./ai";
import type { PresentationItem } from "./presentation";
import type { ServiceSession } from "./service";
import type { LiveStatus, TimelineEntry } from "./live";

describe("domain contracts", () => {
  it("constructs a ScriptureReference and a matching BibleTranslation", () => {
    const reference: ScriptureReference = {
      translationId: "KJV",
      book: "ROM",
      chapter: 8,
      verseStart: 28,
      verseEnd: null,
    };
    const translation: BibleTranslation = {
      id: "KJV",
      name: "King James Version",
      abbreviation: "KJV",
      language: "en",
      isLocal: true,
    };
    expect(reference.translationId).toBe(translation.id);
  });

  it("constructs a pending Suggestion carrying a ConfidenceResult", () => {
    const confidence: ConfidenceResult = { score: 0.92, level: "high", source: "heuristic", reason: null };
    const suggestion: Suggestion = {
      id: "00000000-0000-0000-0000-000000000001",
      serviceId: "00000000-0000-0000-0000-000000000002",
      kind: { type: "scripture", reference: "ROM 8:28" },
      status: "pending",
      confidence,
      createdAt: new Date().toISOString(),
      transcriptSegmentId: null,
      sourceText: null,
    };
    expect(suggestion.status).toBe("pending");
  });

  it("constructs a ServiceSession and a PresentationItem scoped to it", () => {
    const session: ServiceSession = {
      id: "00000000-0000-0000-0000-000000000002",
      title: "Sunday Morning",
      status: "started",
      startedAt: new Date().toISOString(),
      endedAt: null,
    };
    const item: PresentationItem = {
      id: "00000000-0000-0000-0000-000000000003",
      serviceId: session.id,
      content: { type: "scripture", reference: "ROM 8:28", translationId: "KJV", text: "..." },
      status: "prepared",
      createdAt: new Date().toISOString(),
    };
    expect(item.serviceId).toBe(session.id);
  });

  it("constructs a ScriptureContext and an AmbiguousCandidate that references it", () => {
    const confidence: ConfidenceResult = { score: 0.87, level: "high", source: "heuristic", reason: null };
    const context: ScriptureContext = {
      translationId: "KJV",
      book: "ROM",
      chapter: 8,
      lastVerse: 28,
      confidence,
      establishedAt: new Date().toISOString(),
      valid: true,
    };
    const candidate: AmbiguousCandidate = {
      reference: { translationId: "KJV", book: context.book, chapter: context.chapter, verseStart: 31, verseEnd: null },
      confidence,
    };
    expect(candidate.reference.book).toBe(context.book);
  });

  it("constructs a final TranscriptSegment carrying id/sequence/language/speakerId", () => {
    const segment: TranscriptSegment = {
      id: "00000000-0000-0000-0000-000000000004",
      sequence: 3,
      text: "Turn with me to Romans chapter 8.",
      isFinal: true,
      confidence: { score: 0.95, level: "high", source: "model", reason: null },
      startMs: 3000,
      endMs: 3900,
      language: "en",
      speakerId: null,
    };
    expect(segment.isFinal).toBe(true);
    expect(segment.speakerId).toBeNull();
  });

  it("constructs a ScriptureDetection and a ProcessedSegment produced from it", () => {
    const confidence: ConfidenceResult = { score: 0.95, level: "high", source: "model", reason: null };
    const reference: ScriptureReference = { translationId: "KJV", book: "ROM", chapter: 8, verseStart: 28, verseEnd: null };
    const detection: ScriptureDetection = {
      kind: "direct",
      reference,
      context: null,
      candidates: [],
      confidence,
      rawText: "Romans 8:28",
    };
    const suggestion: Suggestion = {
      id: "00000000-0000-0000-0000-000000000005",
      serviceId: "00000000-0000-0000-0000-000000000002",
      kind: { type: "scripture", reference: "ROM 8:28" },
      status: "pending",
      confidence,
      createdAt: new Date().toISOString(),
      transcriptSegmentId: null,
      sourceText: null,
    };
    const processed: ProcessedSegment = {
      serviceId: suggestion.serviceId,
      detections: [detection],
      suggestions: [suggestion],
    };
    expect(processed.detections[0].reference?.book).toBe("ROM");
    expect(processed.suggestions[0].status).toBe("pending");
  });

  it("constructs a LiveStatus reflecting an active, listening service", () => {
    const status: LiveStatus = {
      service: {
        id: "00000000-0000-0000-0000-000000000002",
        title: "Sunday Morning",
        status: "started",
        startedAt: new Date().toISOString(),
        endedAt: null,
      },
      serviceStatus: "live",
      audio: { isCapturing: true, isPaused: false, sampleRateHz: 16000, inputLevel: 0.2 },
      audioStatus: "listening",
      speechStatus: "ready",
      networkStatus: "offline",
      aiStatus: "available",
      databaseStatus: "connected",
    };
    expect(status.serviceStatus).toBe("live");
    expect(status.networkStatus).toBe("offline");
    expect(status.aiStatus).toBe("available");
    expect(status.databaseStatus).toBe("connected");
  });

  it("constructs a TimelineEntry describing a service-lifecycle event", () => {
    const entry: TimelineEntry = {
      id: "00000000-0000-0000-0000-000000000006",
      serviceId: "00000000-0000-0000-0000-000000000002",
      eventName: "SUGGESTION_APPROVED",
      category: "ai",
      payload: { suggestionId: "00000000-0000-0000-0000-000000000005", kind: { reference: "ROM 8:28" } },
      createdAt: new Date().toISOString(),
    };
    expect(entry.eventName).toBe("SUGGESTION_APPROVED");
    expect(entry.payload?.kind).toEqual({ reference: "ROM 8:28" });
  });
});
