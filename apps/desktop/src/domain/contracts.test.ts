/**
 * These tests exist primarily as compile-time proof that the Phase 1
 * domain contracts have the shape this file (and therefore any real
 * consumer) expects - if a field is renamed or removed on either side of
 * the Rust/TS mirror, this file fails to type-check before it fails at
 * runtime.
 */
import { describe, expect, it } from "vitest";
import type {
  AmbiguousCandidate,
  BibleSearchResult,
  ScriptureContext,
  BibleTranslation,
  ScriptureReference,
} from "./bible";
import type { ConfidenceResult } from "./confidence";
import type { ProcessedSegment, ScriptureDetection, Suggestion, TranscriptSegment } from "./ai";
import type { ContentMetadata, ImportReport, IntegrityReport } from "./content";
import type {
  DomainCapabilityReport,
  IntelligenceCorrelation,
  IntelligenceFinding,
} from "./intelligence";
import type { PresentationItem, PresentationPreview, RenderedSlide } from "./presentation";
import type { ServiceSession } from "./service";
import type { LiveStatus, TimelineEntry } from "./live";
import type {
  AcousticEngineStatus,
  CurrentSong,
  MusicDatasetInput,
  MusicImportReport,
  SongRecognitionCandidate,
} from "./music";

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
      sourceSuggestionId: null,
      template: null,
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
      acousticStatus: { status: "unavailable", method: "none", reason: "no acoustic recognizer configured" },
      currentSong: null,
    };
    expect(status.serviceStatus).toBe("live");
    expect(status.networkStatus).toBe("offline");
    expect(status.aiStatus).toBe("available");
    expect(status.databaseStatus).toBe("connected");
    expect(status.acousticStatus.status).toBe("unavailable");
    expect(status.currentSong).toBeNull();
  });

  it("constructs a PresentationItem traceable to its source suggestion and template (Phase 1.4)", () => {
    const item: PresentationItem = {
      id: "00000000-0000-0000-0000-000000000007",
      serviceId: "00000000-0000-0000-0000-000000000002",
      content: { type: "scripture", reference: "ROM 8:28", translationId: "KJV", text: "..." },
      status: "prepared",
      createdAt: new Date().toISOString(),
      sourceSuggestionId: "00000000-0000-0000-0000-000000000001",
      template: "SCRIPTURE_DEFAULT",
    };
    expect(item.sourceSuggestionId).not.toBeNull();
    expect(item.template).toBe("SCRIPTURE_DEFAULT");
  });

  it("constructs a RenderedSlide and the PresentationPreview that wraps it (Phase 1.4)", () => {
    const slide: RenderedSlide = {
      template: "SCRIPTURE_DEFAULT",
      heading: "ROM 8:28",
      bodyLines: ["And we know that all things work together for good", "to them that love God."],
      footer: "KJV",
    };
    const preview: PresentationPreview = {
      content: { type: "scripture", reference: "ROM 8:28", translationId: "KJV", text: "And we know..." },
      slide,
    };
    expect(preview.slide.bodyLines.length).toBeGreaterThan(0);
    expect(preview.content.type).toBe("scripture");
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

  it("constructs a ContentMetadata with unknown licensing fields left null (Phase 1.5)", () => {
    const metadata: ContentMetadata = {
      id: "bible:KJV",
      contentType: "bible",
      name: "King James Version",
      version: "dev-fixture",
      language: "en",
      source: "development fixture",
      publisher: null,
      copyright: null,
      license: null,
      distribution: null,
      importedAt: new Date().toISOString(),
      checksum: null,
      status: "enabled",
    };
    expect(metadata.publisher).toBeNull();
    expect(metadata.status).toBe("enabled");
  });

  it("constructs an ImportReport reflecting an actual dataset import", () => {
    const report: ImportReport = {
      translationId: "KJV",
      datasetVersion: "1.0",
      books: 66,
      chapters: 1189,
      versesTotal: 31102,
      imported: 31102,
      alreadyPresent: 0,
      invalid: 0,
      errors: [],
      checksum: "abc123",
    };
    expect(report.imported).toBe(report.versesTotal);
    expect(report.errors).toHaveLength(0);
  });

  it("constructs an IntegrityReport distinguishing a development fixture from a complete dataset", () => {
    const report: IntegrityReport = {
      translationId: "KJV",
      status: "incomplete",
      booksPresent: 2,
      booksExpected: 66,
      chaptersChecked: 2,
      versesChecked: 6,
      issues: [],
    };
    expect(report.status).toBe("incomplete");
    expect(report.booksPresent).toBeLessThan(report.booksExpected);
  });

  it("constructs a BibleSearchResult with an honest relevance score (Phase 1.5)", () => {
    const exactMatch: BibleSearchResult = {
      translationId: "KJV",
      book: "ROM",
      chapter: 8,
      verse: 28,
      reference: "ROM 8:28",
      text: "And we know that all things work together for good...",
      relevance: 1.0,
    };
    const textMatch: BibleSearchResult = { ...exactMatch, relevance: null };
    expect(exactMatch.relevance).toBe(1.0);
    expect(textMatch.relevance).toBeNull();
  });

  it("constructs an IntelligenceFinding distinguishing inferred context from a suggested reference (Phase 2.0)", () => {
    const confidence: ConfidenceResult = { score: 0.9, level: "high", source: "heuristic", reason: null };
    const inferredContext: IntelligenceFinding = {
      id: "11111111-1111-1111-1111-111111111111",
      serviceId: "22222222-2222-2222-2222-222222222222",
      domain: "bible",
      kind: "scripture",
      assertionLevel: "inferred",
      status: "detected",
      priority: "normal",
      confidence,
      summary: "Active Scripture Context: ROM 8",
      transcriptSegmentIds: ["33333333-3333-3333-3333-333333333333"],
      evidence: [{ kind: "transcript", segmentIds: ["33333333-3333-3333-3333-333333333333"], excerpt: "Romans chapter eight" }],
      provenance: { contentId: "bible:KJV", note: null },
      engineId: "bible",
      engineVersion: "1.0",
      createdAt: "2026-01-01T00:00:00Z",
    };
    const suggestedVerse: IntelligenceFinding = { ...inferredContext, assertionLevel: "suggested", summary: "ROM 8:28" };

    expect(inferredContext.assertionLevel).toBe("inferred");
    expect(suggestedVerse.assertionLevel).toBe("suggested");
    expect(inferredContext.status).toBe("detected");
    expect(inferredContext.provenance.contentId).toBe("bible:KJV");
  });

  it("constructs an IntelligenceCorrelation referencing more than one finding", () => {
    const confidence: ConfidenceResult = { score: 0.6, level: "medium", source: "heuristic", reason: null };
    const correlation: IntelligenceCorrelation = {
      id: "44444444-4444-4444-4444-444444444444",
      sourceFindingIds: ["11111111-1111-1111-1111-111111111111", "55555555-5555-5555-5555-555555555555"],
      kind: { kind: "temporal_proximity" },
      confidence,
      evidence: [],
      createdAt: "2026-01-01T00:00:00Z",
    };
    expect(correlation.sourceFindingIds).toHaveLength(2);
    expect(correlation.kind.kind).toBe("temporal_proximity");
  });

  it("constructs a DomainCapabilityReport leaving engine identity null for an unregistered domain (Phase 2.0)", () => {
    const bible: DomainCapabilityReport = { domain: "bible", capability: "available", engineId: "bible", engineVersion: "1.0" };
    const music: DomainCapabilityReport = { domain: "music", capability: "unavailable", engineId: null, engineVersion: null };
    expect(bible.capability).toBe("available");
    expect(music.engineId).toBeNull();
  });

  it("constructs a music IntelligenceFinding distinguishing a strong title match from a weak partial lyric match (Phase 2.1)", () => {
    const explicitTitle: ConfidenceResult = { score: 0.97, level: "high", source: "heuristic", reason: "Exact title match" };
    const strong: IntelligenceFinding = {
      id: "66666666-6666-6666-6666-666666666666",
      serviceId: "22222222-2222-2222-2222-222222222222",
      domain: "music",
      kind: "music",
      assertionLevel: "suggested",
      status: "detected",
      priority: "normal",
      confidence: explicitTitle,
      summary: "Exact title match",
      transcriptSegmentIds: ["33333333-3333-3333-3333-333333333333"],
      evidence: [
        { kind: "transcript", segmentIds: ["33333333-3333-3333-3333-333333333333"], excerpt: "Amazing Grace" },
        { kind: "context", description: "song_id:h1" },
      ],
      provenance: { contentId: "music:dev-hymnbook", note: null },
      engineId: "music-lyric",
      engineVersion: "0.1.0",
      createdAt: "2026-01-01T00:00:00Z",
    };
    const weakPartial: IntelligenceFinding = {
      ...strong,
      assertionLevel: "inferred",
      confidence: { score: 0.3, level: "low", source: "heuristic", reason: null },
      summary: "1 song(s) match the phrase 'we praise you'",
    };

    expect(strong.domain).toBe("music");
    expect(strong.assertionLevel).toBe("suggested");
    expect(weakPartial.assertionLevel).toBe("inferred");
    expect(strong.provenance.contentId).toBe("music:dev-hymnbook");
  });

  it("constructs a SongRecognitionCandidate always carrying evidence, never a bare song reference (Phase 2.1)", () => {
    const candidate: SongRecognitionCandidate = {
      songId: "h1",
      matchType: "explicit_title",
      matchedText: "Test Fixture Hymn One",
      confidence: { score: 0.97, level: "high", source: "heuristic", reason: "Exact title match" },
      evidence: ["Exact title match"],
      source: "music:dev-hymnbook",
      ranking: 0,
      explanation: "Exact title match",
    };
    expect(candidate.evidence.length).toBeGreaterThan(0);
    expect(candidate.matchType).toBe("explicit_title");
  });

  it("constructs a MusicImportReport with dataset-derived song/lyric counts, distinct from the Bible ImportReport shape (Phase 2.1)", () => {
    const report: MusicImportReport = {
      contentId: "music:dev-hymnbook",
      datasetVersion: "dev-fixture",
      songsTotal: 3,
      songsImported: 3,
      songsAlreadyPresent: 0,
      songsInvalid: 0,
      lyricLinesImported: 4,
      lyricLinesAlreadyPresent: 0,
      errors: [],
      checksum: "abc123",
    };
    expect(report.songsImported).toBe(report.songsTotal);
    expect(report.errors).toHaveLength(0);
  });

  it("constructs a MusicDatasetInput with fictional fixture songs, mirroring the importer's JSON shape (Phase 2.1)", () => {
    const dataset: MusicDatasetInput = {
      contentId: "music:test",
      name: "Test Dataset",
      language: "en",
      publisher: "Test Publisher",
      license: "public domain",
      distribution: "public domain",
      datasetVersion: "1.0",
      songs: [
        {
          id: "s1",
          title: "Test Fixture Song",
          aliases: ["Fixture Song"],
          songType: "hymn",
          language: "en",
          number: "1",
          sections: [{ id: "v1", kind: "verse", sequence: 0 }],
          lyrics: [{ sectionId: "v1", sequence: 0, text: "A fictional test line" }],
        },
      ],
    };
    expect(dataset.songs).toHaveLength(1);
    expect(dataset.songs[0].sections?.[0].kind).toBe("verse");
  });

  // --- Phase 2.2: acoustic recognition ------------------------------------

  it("constructs an AcousticEngineStatus honestly reporting an unconfigured recognizer", () => {
    const status: AcousticEngineStatus = {
      status: "unavailable",
      method: "none",
      reason: "no acoustic model directory configured",
    };
    expect(status.status).toBe("unavailable");
    expect(status.reason).not.toBeNull();
  });

  it("constructs a CurrentSong distinct from a merely-detected finding", () => {
    const song: CurrentSong = {
      contentId: "music:dev-hymnbook",
      songId: "h1",
      confidence: { score: 0.9, level: "high", source: "model", reason: null },
    };
    expect(song.songId).toBe("h1");
  });

  it("constructs an acoustic-sourced IntelligenceFinding carrying an Acoustic evidence entry (Phase 2.2)", () => {
    const finding: IntelligenceFinding = {
      id: "77777777-7777-7777-7777-777777777777",
      serviceId: "22222222-2222-2222-2222-222222222222",
      domain: "music",
      kind: "music",
      assertionLevel: "suggested",
      status: "detected",
      priority: "normal",
      confidence: { score: 0.85, level: "high", source: "model", reason: null },
      summary: "Acoustic match (local_model)",
      transcriptSegmentIds: [],
      evidence: [
        {
          kind: "acoustic",
          segmentId: "88888888-8888-8888-8888-888888888888",
          method: "local_model",
          durationMs: 8000,
        },
        { kind: "context", description: "song_id:h1" },
      ],
      provenance: { contentId: "music:dev-hymnbook", note: null },
      engineId: "music-lyric",
      engineVersion: "0.1.0",
      createdAt: "2026-01-01T00:00:00Z",
    };
    // Acoustic-sourced findings never start anywhere but Detected - no
    // automatic approval/projection (Phase 2.0 spec section 36, unchanged).
    expect(finding.status).toBe("detected");
    expect(finding.evidence[0]).toEqual({
      kind: "acoustic",
      segmentId: "88888888-8888-8888-8888-888888888888",
      method: "local_model",
      durationMs: 8000,
    });
  });
});
