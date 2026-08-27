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
  FindingStatus,
  IntelligenceCorrelation,
  IntelligenceFinding,
} from "./intelligence";
import type {
  PresentationDisplayPayload,
  PresentationDisplayState,
  PresentationItem,
  PresentationPreview,
  RenderedSlide,
} from "./presentation";
import type {
  ServiceIntelligenceSummary,
  ServicePhase,
  ServiceSession,
  TranscriptFreshness,
} from "./service";
import type { LiveStatus, TimelineEntry } from "./live";
import type {
  AcousticEngineStatus,
  CurrentSong,
  MusicDatasetInput,
  MusicImportReport,
  SongRecognitionCandidate,
} from "./music";
import type {
  Sermon,
  SermonFoundationSummary,
  SermonPoint,
  SermonSection,
  SermonSectionKind,
  SermonSegment,
  SermonState,
  SermonStateSnapshot,
  SermonStatus,
  SpeakerRole,
  ThemeCandidate,
} from "./sermon";
import type { ContentCandidate, ContentCandidateType } from "./contentIntelligence";

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

  it("constructs a PresentationDisplayPayload carrying both the active item and its already-rendered slide (local presentation display)", () => {
    const item: PresentationItem = {
      id: "00000000-0000-0000-0000-000000000008",
      serviceId: "00000000-0000-0000-0000-000000000002",
      content: { type: "scripture", reference: "ROM 8:28", translationId: "KJV", text: "..." },
      status: "active",
      createdAt: new Date().toISOString(),
      sourceSuggestionId: null,
      template: "SCRIPTURE_DEFAULT",
    };
    const slide: RenderedSlide = {
      template: "SCRIPTURE_DEFAULT",
      heading: "ROM 8:28",
      bodyLines: ["And we know that all things work together for good"],
      footer: "KJV",
    };
    const payload: PresentationDisplayPayload = { item, slide };
    expect(payload.item.status).toBe("active");
    expect(payload.slide.heading).toBe("ROM 8:28");
  });

  it("constructs a PresentationDisplayState with no active item and a closed window", () => {
    const state: PresentationDisplayState = { windowOpen: false, activeItem: null };
    expect(state.windowOpen).toBe(false);
    expect(state.activeItem).toBeNull();
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
      licensingStatus: "unknown",
    };
    expect(metadata.publisher).toBeNull();
    expect(metadata.status).toBe("enabled");
    expect(metadata.licensingStatus).toBe("unknown");
  });

  it("constructs a ContentMetadata for a verified-public-domain production dataset (real Bible dataset milestone)", () => {
    const metadata: ContentMetadata = {
      id: "bible:BSB",
      contentType: "bible",
      name: "Berean Standard Bible",
      version: "bsb-1.0",
      language: "en",
      source: "user-provided import",
      publisher: null,
      copyright: null,
      license: "Public Domain",
      distribution: "Public domain dedication effective April 30, 2023",
      importedAt: new Date().toISOString(),
      checksum: "abc123",
      status: "enabled",
      licensingStatus: "verified_public_domain",
    };
    expect(metadata.licensingStatus).toBe("verified_public_domain");
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
      licensingStatus: "verified_public_domain",
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
      sermonId: null,
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
      serviceId: "22222222-2222-2222-2222-222222222222",
      sourceFindingIds: ["11111111-1111-1111-1111-111111111111", "55555555-5555-5555-5555-555555555555"],
      domains: ["bible", "sermon"],
      kind: { kind: "temporal_proximity" },
      assertionLevel: "inferred",
      status: "detected",
      confidence,
      summary: "findings occurred near the same point in the transcript",
      evidence: [],
      ruleId: "temporal_association_v1",
      ruleVersion: "1.0",
      createdAt: "2026-01-01T00:00:00Z",
    };
    expect(correlation.sourceFindingIds).toHaveLength(2);
    expect(correlation.kind.kind).toBe("temporal_proximity");
    expect(correlation.status).toBe("detected");
  });

  it("constructs a Phase 2.4 ScriptureSermon correlation carrying both domains and rule provenance", () => {
    const confidence: ConfidenceResult = { score: 0.95, level: "high", source: "heuristic", reason: "exact shared scripture reference" };
    const correlation: IntelligenceCorrelation = {
      id: "77777777-7777-7777-7777-777777777777",
      serviceId: "22222222-2222-2222-2222-222222222222",
      sourceFindingIds: ["11111111-1111-1111-1111-111111111111", "66666666-6666-6666-6666-666666666666"],
      domains: ["sermon", "bible"],
      kind: { kind: "scripture_sermon" },
      assertionLevel: "inferred",
      status: "detected",
      confidence,
      summary: "Sermon references ROM 8:28, matching Bible finding ROM 8:28",
      evidence: [
        { kind: "another_finding", findingId: "11111111-1111-1111-1111-111111111111" },
        { kind: "another_finding", findingId: "66666666-6666-6666-6666-666666666666" },
      ],
      ruleId: "scripture_sermon_v1",
      ruleVersion: "1.0",
      createdAt: "2026-01-01T00:00:00Z",
    };
    expect(correlation.kind.kind).toBe("scripture_sermon");
    expect(correlation.domains).toEqual(["sermon", "bible"]);
    expect(correlation.ruleId).toBe("scripture_sermon_v1");
  });

  it("moves a correlation through the review/dismiss lifecycle using FindingStatus, never a separate status enum", () => {
    const detected: FindingStatus = "detected";
    const reviewed: FindingStatus = "reviewed";
    const rejected: FindingStatus = "rejected";
    expect([detected, reviewed, rejected]).toEqual(["detected", "reviewed", "rejected"]);
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
      sermonId: null,
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
      sermonId: null,
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

  // --- Phase 2.6 (per the authoritative Phase 2 roadmap; built under this
  // repository's earlier internal "Phase 2.3" label): sermon intelligence -

  it("constructs a Sermon IntelligenceFinding distinguishing an Observed main point from an Inferred theme, carrying its sermonId", () => {
    const point: IntelligenceFinding = {
      id: "99999999-9999-9999-9999-999999999999",
      serviceId: "22222222-2222-2222-2222-222222222222",
      domain: "sermon",
      kind: "sermon",
      assertionLevel: "observed",
      status: "detected",
      priority: "normal",
      confidence: { score: 0.9, level: "high", source: "heuristic", reason: "explicit main-point trigger phrase matched" },
      summary: "Main Point: My first point is that faith comes by hearing.",
      transcriptSegmentIds: ["11111111-1111-1111-1111-111111111111"],
      sermonId: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
      evidence: [
        {
          kind: "transcript",
          segmentIds: ["11111111-1111-1111-1111-111111111111"],
          excerpt: "My first point is that faith comes by hearing.",
        },
      ],
      provenance: { contentId: null, note: null },
      engineId: "sermon-core",
      engineVersion: "0.1.0",
      createdAt: "2026-01-01T00:00:00Z",
    };
    const theme: IntelligenceFinding = {
      ...point,
      id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
      assertionLevel: "inferred",
      summary: "Theme: faith",
    };

    expect(point.assertionLevel).toBe("observed");
    expect(theme.assertionLevel).toBe("inferred");
    // Never Generated - the core epistemic discipline this domain must
    // honor (spec section 7).
    expect([point, theme].every((f) => f.assertionLevel !== "generated")).toBe(true);
    expect(point.sermonId).toBe("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
  });

  it("constructs a SermonPoint that never carries a rewritten earlier point (Phase 2.3)", () => {
    const points: SermonPoint[] = [
      { sequence: 1, rawText: "My first point is faith.", subPoints: [] },
      {
        sequence: 2,
        rawText: "The second thing is obedience.",
        subPoints: [{ sequence: 1, rawText: "Under this point, obedience starts with listening." }],
      },
    ];
    expect(points).toHaveLength(2);
    expect(points[0].rawText).toContain("faith");
    expect(points[1].subPoints[0].rawText).toContain("listening");
  });

  it("constructs a ThemeCandidate that is always Inferred-shaped, never claiming direct observation", () => {
    const candidate: ThemeCandidate = {
      label: "faith and obedience",
      confidence: 0.75,
      repetitionCount: 5,
      structuralMentions: 2,
    };
    expect(candidate.confidence).toBeLessThan(1);
    expect(candidate.label).toContain("and");
  });

  it("constructs a SermonStateSnapshot for every SermonState value", () => {
    const states: SermonState[] = [
      "introduction",
      "teaching",
      "main_point",
      "illustration",
      "application",
      "conclusion",
      "prayer",
      "unknown",
    ];
    for (const state of states) {
      const snapshot: SermonStateSnapshot = { state, theme: null, points: [] };
      expect(snapshot.state).toBe(state);
    }
  });

  // --- Service Intelligence (Phase 2.4, per the authoritative Phase 2 roadmap) --

  it("constructs a ServicePhaseSnapshot-shaped ServiceIntelligenceSummary for every ServicePhase value", () => {
    const phases: ServicePhase[] = [
      "unknown",
      "opening",
      "worship",
      "prayer",
      "scripture_reading",
      "sermon",
      "offering",
      "announcement",
      "closing",
    ];
    for (const phase of phases) {
      const summary: ServiceIntelligenceSummary = {
        phase,
        phaseStartedAt: "2026-01-01T00:00:00Z",
        previousPhase: null,
        transitionCount: 0,
        transcriptFreshness: { status: "unknown" },
      };
      expect(summary.phase).toBe(phase);
    }
  });

  it("constructs every TranscriptFreshness variant, with secondsSince only on stale", () => {
    const unknown: TranscriptFreshness = { status: "unknown" };
    const fresh: TranscriptFreshness = { status: "fresh" };
    const stale: TranscriptFreshness = { status: "stale", secondsSince: 42 };
    expect(unknown.status).toBe("unknown");
    expect(fresh.status).toBe("fresh");
    expect(stale.status).toBe("stale");
    expect(stale.secondsSince).toBe(42);
  });

  it("constructs a Service-domain IntelligenceFinding for a phase transition, always Observed/Inferred/Suggested - never Generated", () => {
    const confidence: ConfidenceResult = { score: 0.85, level: "high", source: "heuristic", reason: "explicit trigger phrase matched" };
    const transition: IntelligenceFinding = {
      id: "88888888-8888-8888-8888-888888888888",
      serviceId: "22222222-2222-2222-2222-222222222222",
      domain: "service",
      kind: "service_state",
      assertionLevel: "inferred",
      status: "detected",
      priority: "high",
      confidence,
      summary: "Service phase changed #1: UNKNOWN -> PRAYER",
      transcriptSegmentIds: ["33333333-3333-3333-3333-333333333333"],
      sermonId: null,
      evidence: [
        {
          kind: "transcript",
          segmentIds: ["33333333-3333-3333-3333-333333333333"],
          excerpt: "let us pray",
        },
      ],
      provenance: { contentId: null, note: null },
      engineId: "service-state",
      engineVersion: "1.0.0",
      createdAt: "2026-01-01T00:00:00Z",
    };
    expect(transition.domain).toBe("service");
    expect(transition.kind).toBe("service_state");
    expect(transition.priority).toBe("high");
    expect(transition.assertionLevel).not.toBe("generated");
  });

  // --- Sermon Foundation (Phase 2.5, per the authoritative Phase 2 roadmap) --

  it("constructs a Sermon for every SermonStatus value, with unknown title/speaker as null", () => {
    const statuses: SermonStatus[] = ["planned", "active", "paused", "ended", "cancelled"];
    for (const status of statuses) {
      const sermon: Sermon = {
        id: "11111111-1111-1111-1111-111111111111",
        serviceId: "22222222-2222-2222-2222-222222222222",
        title: null,
        speaker: null,
        status,
        startedAt: null,
        endedAt: null,
        createdAt: "2026-01-01T00:00:00Z",
      };
      expect(sermon.status).toBe(status);
      expect(sermon.title).toBeNull();
      expect(sermon.speaker).toBeNull();
    }
  });

  it("a Sermon's id is never the same as its serviceId (invariant 1/2)", () => {
    const sermon: Sermon = {
      id: "11111111-1111-1111-1111-111111111111",
      serviceId: "22222222-2222-2222-2222-222222222222",
      title: "Grace Abounding",
      speaker: { id: "33333333-3333-3333-3333-333333333333", name: "Pastor Jane Doe", role: "primary" as SpeakerRole },
      status: "active",
      startedAt: "2026-01-01T00:00:00Z",
      endedAt: null,
      createdAt: "2026-01-01T00:00:00Z",
    };
    expect(sermon.id).not.toBe(sermon.serviceId);
  });

  it("constructs a SermonSection for every SermonSectionKind, open and closed", () => {
    const kinds: SermonSectionKind[] = [
      "introduction",
      "scripture_reading",
      "main_message",
      "illustration",
      "prayer",
      "altar_call",
      "conclusion",
    ];
    for (const kind of kinds) {
      const open: SermonSection = {
        id: "44444444-4444-4444-4444-444444444444",
        sermonId: "11111111-1111-1111-1111-111111111111",
        kind,
        origin: "operator_assigned",
        startedAt: "2026-01-01T00:00:00Z",
        endedAt: null,
        note: null,
      };
      expect(open.endedAt).toBeNull();
      const closed: SermonSection = { ...open, endedAt: "2026-01-01T00:05:00Z" };
      expect(closed.endedAt).not.toBeNull();
    }
  });

  it("constructs a SermonSegment linking a transcript segment - never carrying transcript text itself", () => {
    const segment: SermonSegment = {
      id: "55555555-5555-5555-5555-555555555555",
      sermonId: "11111111-1111-1111-1111-111111111111",
      transcriptSegmentId: "66666666-6666-6666-6666-666666666666",
      sequence: 0,
      sectionId: null,
      linkedAt: "2026-01-01T00:00:00Z",
    };
    expect(segment.sermonId).not.toBe(segment.transcriptSegmentId);
    expect("text" in segment).toBe(false);
  });

  it("constructs a SermonFoundationSummary with both fields null when nothing is active", () => {
    const summary: SermonFoundationSummary = { activeSermon: null, currentSection: null };
    expect(summary.activeSermon).toBeNull();
    expect(summary.currentSection).toBeNull();
  });

  it("constructs a ContentCandidate for every ContentCandidateType, tracing back to its source finding", () => {
    const types: ContentCandidateType[] = [
      "theme",
      "teaching",
      "reflection",
      "takeaway",
      "food_for_thought",
      "quote",
      "discussion_question",
      "scripture_reflection",
      "illustration",
    ];
    const confidence: ConfidenceResult = { score: 0.8, level: "high", source: "heuristic", reason: null };
    for (const candidateType of types) {
      const candidate: ContentCandidate = {
        id: "11111111-1111-1111-1111-111111111111",
        serviceId: "22222222-2222-2222-2222-222222222222",
        sermonId: "33333333-3333-3333-3333-333333333333",
        sourceFindingIds: ["44444444-4444-4444-4444-444444444444"],
        candidateType,
        titleOrLabel: "Theme: grace",
        workingConcept: "grace abounding",
        assertionLevel: "inferred",
        status: "detected",
        confidence,
        contentPotential: 0.7,
        evidence: [{ kind: "another_finding", findingId: "44444444-4444-4444-4444-444444444444" }],
        provenance: { contentId: null, note: "derived from a Phase 2.6 sermon finding" },
        engineId: "content-intelligence",
        engineVersion: "0.1.0",
        createdAt: "2026-01-01T00:00:00Z",
      };
      expect(candidate.candidateType).toBe(candidateType);
      expect(candidate.sourceFindingIds).toHaveLength(1);
    }
  });

  it("a ContentCandidate's contentPotential is independent of its confidence - not derived from it", () => {
    const highConfidenceLowPotential: ContentCandidate = {
      id: "11111111-1111-1111-1111-111111111111",
      serviceId: "22222222-2222-2222-2222-222222222222",
      sermonId: null,
      sourceFindingIds: ["44444444-4444-4444-4444-444444444444"],
      candidateType: "reflection",
      titleOrLabel: "Application: pray daily",
      workingConcept: "pray daily",
      assertionLevel: "suggested",
      status: "detected",
      confidence: { score: 0.95, level: "high", source: "heuristic", reason: null },
      contentPotential: 0.2,
      evidence: [],
      provenance: { contentId: null, note: null },
      engineId: "content-intelligence",
      engineVersion: "0.1.0",
      createdAt: "2026-01-01T00:00:00Z",
    };
    const lowConfidenceHighPotential: ContentCandidate = {
      ...highConfidenceLowPotential,
      candidateType: "quote",
      confidence: { score: 0.3, level: "low", source: "heuristic", reason: null },
      contentPotential: 0.9,
    };
    expect(highConfidenceLowPotential.confidence.score).toBeGreaterThan(highConfidenceLowPotential.contentPotential);
    expect(lowConfidenceHighPotential.contentPotential).toBeGreaterThan(lowConfidenceHighPotential.confidence.score);
  });

  it("moves a ContentCandidate through the accept/reject lifecycle using FindingStatus, never a separate status enum", () => {
    const detected: FindingStatus = "detected";
    const accepted: FindingStatus = "accepted";
    const rejected: FindingStatus = "rejected";
    expect([detected, accepted, rejected]).toEqual(["detected", "accepted", "rejected"]);
  });
});
