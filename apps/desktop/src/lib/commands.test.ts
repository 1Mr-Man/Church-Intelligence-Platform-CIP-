/**
 * Proves the Phase 1.2.1 IPC guard: outside the Tauri runtime, these
 * wrappers must reject with `TauriUnavailableError` and must never call
 * the real `invoke()` - not just handle its failure after the fact. Both
 * `invoke` and `isTauri` are mocked so the "outside Tauri" case can be
 * simulated deterministically in a plain Node test environment, where
 * `window.__TAURI_INTERNALS__` never exists regardless.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  acceptMusicFinding,
  acceptSermonFinding,
  acknowledgeServiceAnomaly,
  analyzeBibleTranscript,
  analyzeCrossDomain,
  analyzeMusicAudio,
  analyzeMusicTranscript,
  analyzeSermonTranscript,
  analyzeServiceTranscript,
  assignSermonSpeaker,
  changeSermonSection,
  checkBibleDatasetIntegrity,
  clearCurrentSong,
  correctServicePhase,
  createManualPresentation,
  dismissCrossDomainCorrelation,
  endSermon,
  getAppConfig,
  getIntelligenceCapabilities,
  getServiceIntelligenceState,
  getSermon,
  getSermonFoundationState,
  getSermonState,
  importBibleDataset,
  importMusicDataset,
  linkTranscriptSegmentToSermon,
  listContentRegistry,
  listCrossDomainCorrelations,
  listMusicFindings,
  listServiceAnomalies,
  listServiceTransitions,
  listSermonFindings,
  listSermonHistory,
  listSermonSections,
  listSermonSegments,
  markServicePhase,
  pauseSermon,
  previewPresentation,
  previewScripture,
  rejectMusicFinding,
  rejectSermonFinding,
  resumeSermon,
  reviewCrossDomainCorrelation,
  searchBible,
  searchMusic,
  setContentEnabled,
  setSermonTitle,
  startSermon,
  TauriUnavailableError,
} from "./commands";

const invokeMock = vi.fn();
const isTauriMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  isTauri: () => isTauriMock(),
}));

describe("commands.ts Tauri IPC guard", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    isTauriMock.mockReset();
  });

  it("rejects with TauriUnavailableError and never calls invoke() outside the Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(getAppConfig()).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("carries a clear, non-raw-exception message identifying the desktop-only command", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(searchBible("Romans 8:28")).rejects.toThrow(/search_bible.*CIP desktop application/);
  });

  it("calls the real invoke() with the right command/args inside the Tauri runtime", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await searchBible("Romans 8:28");

    expect(invokeMock).toHaveBeenCalledWith("search_bible", {
      query: "Romans 8:28",
      translationId: null,
    });
  });

  it("passes translationId through when explicitly given (Phase 1.5)", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await searchBible("Romans 8:28", "NIV");

    expect(invokeMock).toHaveBeenCalledWith("search_bible", {
      query: "Romans 8:28",
      translationId: "NIV",
    });
  });

  it("previewPresentation calls preview_presentation, never prepare_presentation (Phase 1.4)", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await previewPresentation("suggestion-1");

    expect(invokeMock).toHaveBeenCalledWith("preview_presentation", { suggestionId: "suggestion-1" });
  });

  it("previewScripture calls preview_scripture with the raw reference", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await previewScripture("ROM 8:28");

    expect(invokeMock).toHaveBeenCalledWith("preview_scripture", { reference: "ROM 8:28" });
  });

  it("createManualPresentation calls create_manual_presentation, not prepare_presentation", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await createManualPresentation("JHN 3:16");

    expect(invokeMock).toHaveBeenCalledWith("create_manual_presentation", { reference: "JHN 3:16" });
  });

  it("listContentRegistry passes null contentType when omitted (Phase 1.5)", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await listContentRegistry();

    expect(invokeMock).toHaveBeenCalledWith("list_content_registry", { contentType: null });
  });

  it("listContentRegistry passes the requested contentType through", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await listContentRegistry("bible");

    expect(invokeMock).toHaveBeenCalledWith("list_content_registry", { contentType: "bible" });
  });

  it("setContentEnabled calls set_content_enabled with the requested state", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await setContentEnabled("bible:KJV", false);

    expect(invokeMock).toHaveBeenCalledWith("set_content_enabled", { contentId: "bible:KJV", enabled: false });
  });

  it("importBibleDataset never reads the filesystem itself - it only forwards already-read JSON text", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await importBibleDataset('{"translation":{}}');

    expect(invokeMock).toHaveBeenCalledWith("import_bible_dataset", { datasetJson: '{"translation":{}}' });
  });

  it("checkBibleDatasetIntegrity calls check_bible_dataset_integrity with the translation id", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await checkBibleDatasetIntegrity("KJV");

    expect(invokeMock).toHaveBeenCalledWith("check_bible_dataset_integrity", { translationId: "KJV" });
  });

  it("getIntelligenceCapabilities calls get_intelligence_capabilities with no arguments (Phase 2.0)", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await getIntelligenceCapabilities();

    expect(invokeMock).toHaveBeenCalledWith("get_intelligence_capabilities", undefined);
  });

  // --- music intelligence (Phase 2.1) --------------------------------------

  it("searchMusic passes null contentIds when omitted, searching only enabled datasets", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await searchMusic("Amazing Grace", "title");

    expect(invokeMock).toHaveBeenCalledWith("search_music", {
      query: "Amazing Grace",
      queryType: "title",
      contentIds: null,
    });
  });

  it("searchMusic forwards explicit contentIds, letting the operator name a disabled dataset", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await searchMusic("120", "number", ["music:dev-disabled-set"]);

    expect(invokeMock).toHaveBeenCalledWith("search_music", {
      query: "120",
      queryType: "number",
      contentIds: ["music:dev-disabled-set"],
    });
  });

  it("importMusicDataset never reads the filesystem itself - it only forwards already-read JSON text", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await importMusicDataset('{"contentId":"music:test"}');

    expect(invokeMock).toHaveBeenCalledWith("import_music_dataset", {
      datasetJson: '{"contentId":"music:test"}',
    });
  });

  it("analyzeMusicTranscript calls analyze_music_transcript with the raw text", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await analyzeMusicTranscript("Let's sing Amazing Grace");

    expect(invokeMock).toHaveBeenCalledWith("analyze_music_transcript", { text: "Let's sing Amazing Grace" });
  });

  it("listMusicFindings calls list_music_findings with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await listMusicFindings();

    expect(invokeMock).toHaveBeenCalledWith("list_music_findings", undefined);
  });

  /** Structural proof (mirroring `previewPresentation`'s test above): the
   * operator-decision wrappers each make exactly one IPC call, to their
   * own command - never a second, presentation-related call. Music
   * recognition must never automatically create a presentation item
   * (Phase 2.1 hard requirement). */
  it("acceptMusicFinding calls only accept_music_finding, never a presentation command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await acceptMusicFinding("finding-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("accept_music_finding", { findingId: "finding-1" });
  });

  it("rejectMusicFinding calls only reject_music_finding, never a presentation command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await rejectMusicFinding("finding-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("reject_music_finding", { findingId: "finding-1" });
  });

  // --- Phase 2.2: acoustic recognition ------------------------------------

  it("analyzeMusicAudio forwards raw samples and sampleRateHz to analyze_music_audio", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await analyzeMusicAudio([1, 2, 3], 16000);

    expect(invokeMock).toHaveBeenCalledWith("analyze_music_audio", {
      samples: [1, 2, 3],
      sampleRateHz: 16000,
    });
  });

  it("clearCurrentSong calls only clear_current_song, never a presentation command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue(undefined);

    await clearCurrentSong();

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("clear_current_song", undefined);
  });

  it("rejects analyzeMusicAudio/clearCurrentSong outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(analyzeMusicAudio([], 16000)).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(clearCurrentSong()).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  // --- sermon intelligence (Phase 2.3) --------------------------------------

  it("analyzeSermonTranscript calls analyze_sermon_transcript with the raw text", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await analyzeSermonTranscript("My first point is that faith comes by hearing.");

    expect(invokeMock).toHaveBeenCalledWith("analyze_sermon_transcript", {
      text: "My first point is that faith comes by hearing.",
    });
  });

  it("listSermonFindings calls list_sermon_findings with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await listSermonFindings();

    expect(invokeMock).toHaveBeenCalledWith("list_sermon_findings", undefined);
  });

  /** Proves acceptance is exactly the finding-status command's own
   * command - never a second, presentation-related call. */
  it("acceptSermonFinding calls only accept_sermon_finding, never a presentation command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await acceptSermonFinding("finding-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("accept_sermon_finding", { findingId: "finding-1" });
  });

  it("rejectSermonFinding calls only reject_sermon_finding, never a presentation command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await rejectSermonFinding("finding-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("reject_sermon_finding", { findingId: "finding-1" });
  });

  it("getSermonState calls get_sermon_state with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({ state: "introduction", theme: null, points: [] });

    await getSermonState();

    expect(invokeMock).toHaveBeenCalledWith("get_sermon_state", undefined);
  });

  it("rejects every sermon command outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(analyzeSermonTranscript("text")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(listSermonFindings()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(acceptSermonFinding("id")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(rejectSermonFinding("id")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(getSermonState()).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  // --- cross-domain intelligence (Phase 2.4) --------------------------------

  it("analyzeBibleTranscript calls analyze_bible_transcript with the raw text", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await analyzeBibleTranscript("Romans chapter eight verse twenty eight");

    expect(invokeMock).toHaveBeenCalledWith("analyze_bible_transcript", {
      text: "Romans chapter eight verse twenty eight",
    });
  });

  /** Proves the explicit, never-automatic nature of the correlation engine:
   * `analyzeCrossDomain` is its own distinct command, never bundled into a
   * transcript-analysis call. */
  it("analyzeCrossDomain calls only analyze_cross_domain with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await analyzeCrossDomain();

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("analyze_cross_domain", undefined);
  });

  it("listCrossDomainCorrelations calls list_cross_domain_correlations with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await listCrossDomainCorrelations();

    expect(invokeMock).toHaveBeenCalledWith("list_cross_domain_correlations", undefined);
  });

  it("reviewCrossDomainCorrelation calls only review_cross_domain_correlation, never a presentation command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await reviewCrossDomainCorrelation("correlation-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("review_cross_domain_correlation", {
      correlationId: "correlation-1",
    });
  });

  it("dismissCrossDomainCorrelation calls only dismiss_cross_domain_correlation, never a presentation command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await dismissCrossDomainCorrelation("correlation-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("dismiss_cross_domain_correlation", {
      correlationId: "correlation-1",
    });
  });

  it("rejects every cross-domain command outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(analyzeBibleTranscript("text")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(analyzeCrossDomain()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(listCrossDomainCorrelations()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(reviewCrossDomainCorrelation("id")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(dismissCrossDomainCorrelation("id")).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  // --- service intelligence (Phase 2.4, per the authoritative Phase 2 roadmap) --

  it("analyzeServiceTranscript calls analyze_service_transcript with the raw text", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await analyzeServiceTranscript("Let us pray.");

    expect(invokeMock).toHaveBeenCalledWith("analyze_service_transcript", { text: "Let us pray." });
  });

  it("getServiceIntelligenceState calls get_service_intelligence_state with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await getServiceIntelligenceState();

    expect(invokeMock).toHaveBeenCalledWith("get_service_intelligence_state", undefined);
  });

  it("listServiceTransitions and listServiceAnomalies each call their own distinct command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await listServiceTransitions();
    await listServiceAnomalies();

    expect(invokeMock).toHaveBeenCalledWith("list_service_transitions", undefined);
    expect(invokeMock).toHaveBeenCalledWith("list_service_anomalies", undefined);
  });

  it("markServicePhase and correctServicePhase pass phase and an explicit null note by default", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await markServicePhase("worship");
    await correctServicePhase("prayer", "actually still worship");

    expect(invokeMock).toHaveBeenCalledWith("mark_service_phase", { phase: "worship", note: null });
    expect(invokeMock).toHaveBeenCalledWith("correct_service_phase", {
      phase: "prayer",
      note: "actually still worship",
    });
  });

  it("acknowledgeServiceAnomaly calls only acknowledge_service_anomaly, never a presentation command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await acknowledgeServiceAnomaly("finding-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("acknowledge_service_anomaly", { findingId: "finding-1" });
  });

  it("rejects every service intelligence command outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(analyzeServiceTranscript("text")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(getServiceIntelligenceState()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(listServiceTransitions()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(listServiceAnomalies()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(markServicePhase("sermon")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(correctServicePhase("sermon")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(acknowledgeServiceAnomaly("id")).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  // --- sermon foundation (Phase 2.5, per the authoritative Phase 2 roadmap) --

  it("getSermonFoundationState calls get_sermon_foundation_state with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({ activeSermon: null, currentSection: null });

    await getSermonFoundationState();

    expect(invokeMock).toHaveBeenCalledWith("get_sermon_foundation_state", undefined);
  });

  it("startSermon passes title as null when omitted, and the given string otherwise", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await startSermon();
    await startSermon("Grace Abounding");

    expect(invokeMock).toHaveBeenCalledWith("start_sermon", { title: null });
    expect(invokeMock).toHaveBeenCalledWith("start_sermon", { title: "Grace Abounding" });
  });

  it("pauseSermon, resumeSermon, and endSermon each call their own distinct command with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await pauseSermon();
    await resumeSermon();
    await endSermon();

    expect(invokeMock).toHaveBeenCalledWith("pause_sermon", undefined);
    expect(invokeMock).toHaveBeenCalledWith("resume_sermon", undefined);
    expect(invokeMock).toHaveBeenCalledWith("end_sermon", undefined);
  });

  it("setSermonTitle and assignSermonSpeaker pass exactly the supplied values", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await setSermonTitle("Faith That Moves");
    await assignSermonSpeaker("Pastor Jane Doe", "primary");

    expect(invokeMock).toHaveBeenCalledWith("set_sermon_title", { title: "Faith That Moves" });
    expect(invokeMock).toHaveBeenCalledWith("assign_sermon_speaker", {
      name: "Pastor Jane Doe",
      role: "primary",
    });
  });

  it("changeSermonSection passes kind and an explicit null note by default", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await changeSermonSection("main_message");
    await changeSermonSection("illustration", "moved early");

    expect(invokeMock).toHaveBeenCalledWith("change_sermon_section", { kind: "main_message", note: null });
    expect(invokeMock).toHaveBeenCalledWith("change_sermon_section", {
      kind: "illustration",
      note: "moved early",
    });
  });

  it("linkTranscriptSegmentToSermon calls only link_transcript_segment_to_sermon, never a presentation command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await linkTranscriptSegmentToSermon("segment-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("link_transcript_segment_to_sermon", {
      transcriptSegmentId: "segment-1",
    });
  });

  it("listSermonSegments, listSermonSections, listSermonHistory, and getSermon each call their own distinct command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await listSermonSegments();
    await listSermonSections();
    await listSermonHistory(10);
    await getSermon("sermon-1");

    expect(invokeMock).toHaveBeenCalledWith("list_sermon_segments", undefined);
    expect(invokeMock).toHaveBeenCalledWith("list_sermon_sections", undefined);
    expect(invokeMock).toHaveBeenCalledWith("list_sermon_history", { limit: 10 });
    expect(invokeMock).toHaveBeenCalledWith("get_sermon", { sermonId: "sermon-1" });
  });

  it("rejects every sermon foundation command outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(getSermonFoundationState()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(startSermon()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(pauseSermon()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(resumeSermon()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(endSermon()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(setSermonTitle("x")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(assignSermonSpeaker("x", "primary")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(changeSermonSection("prayer")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(linkTranscriptSegmentToSermon("id")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(listSermonSegments()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(listSermonSections()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(listSermonHistory(10)).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(getSermon("id")).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
