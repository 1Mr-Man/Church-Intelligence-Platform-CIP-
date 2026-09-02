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
  acceptContentCandidate,
  acceptMusicFinding,
  acceptSermonFinding,
  acknowledgeServiceAnomaly,
  analyzeBibleTranscript,
  analyzeContentIntelligence,
  analyzeCrossDomain,
  analyzeMusicAudio,
  analyzeMusicTranscript,
  analyzeSermonTranscript,
  analyzeServiceTranscript,
  assignSermonSpeaker,
  changeSermonSection,
  checkBibleDatasetIntegrity,
  clearCurrentSong,
  clearPresentationDisplay,
  closePresentationDisplay,
  correctServicePhase,
  createManualPresentation,
  createOperatorAccount,
  disableCongregantCompanion,
  displayPresentation,
  dismissCrossDomainCorrelation,
  enableCongregantCompanion,
  endSermon,
  enrollAcousticReference,
  getAppConfig,
  getChurchKnowledgeBase,
  getCongregantCompanionStatus,
  getCurrentOperator,
  getIntelligenceCapabilities,
  getPresentationDisplayState,
  getProductionIntegrationStatus,
  getServiceIntelligenceState,
  getServiceReport,
  getSermon,
  getSermonFoundationState,
  getSermonState,
  getSpeechLanguageCapabilities,
  importBibleDataset,
  importMusicDataset,
  linkTranscriptSegmentToSermon,
  listAcousticEnrollments,
  listContentCandidates,
  listContentRegistry,
  listCrossDomainCorrelations,
  listMusicFindings,
  listOperatorAccounts,
  listServiceAnomalies,
  listServiceTransitions,
  listSermonFindings,
  listSermonHistory,
  listSermonSections,
  listSermonSegments,
  logDisplayDiagnostic,
  login,
  logout,
  markServicePhase,
  openPresentationDisplay,
  pauseSermon,
  previewPresentation,
  previewScripture,
  rejectContentCandidate,
  rejectMusicFinding,
  rejectSermonFinding,
  removeAcousticReference,
  resumeSermon,
  reviewCrossDomainCorrelation,
  searchBible,
  searchMusic,
  setContentEnabled,
  setProductionIntegrationConfig,
  setSermonTitle,
  setSpeechLanguage,
  startSermon,
  TauriUnavailableError,
  testObsConnection,
  testVmixConnection,
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

    expect(invokeMock).toHaveBeenCalledWith("preview_presentation", {
      suggestionId: "suggestion-1",
      translationId: null,
    });
  });

  it("previewScripture calls preview_scripture with the raw reference", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await previewScripture("ROM 8:28");

    expect(invokeMock).toHaveBeenCalledWith("preview_scripture", {
      reference: "ROM 8:28",
      translationId: null,
    });
  });

  it("createManualPresentation calls create_manual_presentation, not prepare_presentation", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await createManualPresentation("JHN 3:16");

    expect(invokeMock).toHaveBeenCalledWith("create_manual_presentation", {
      reference: "JHN 3:16",
      translationId: null,
    });
  });

  it("createManualPresentation passes an explicit translationId through when given (real Bible dataset milestone)", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await createManualPresentation("JHN 3:16", "BSB");

    expect(invokeMock).toHaveBeenCalledWith("create_manual_presentation", {
      reference: "JHN 3:16",
      translationId: "BSB",
    });
  });

  it("openPresentationDisplay calls open_presentation_display with the screen argument", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue(undefined);

    await openPresentationDisplay("stage");

    expect(invokeMock).toHaveBeenCalledWith("open_presentation_display", { screen: "stage" });
  });

  it("getPresentationDisplayState calls get_presentation_display_state with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({ screens: [], activeItem: null });

    await getPresentationDisplayState();

    expect(invokeMock).toHaveBeenCalledWith("get_presentation_display_state", undefined);
  });

  it("displayPresentation calls only display_presentation, never prepare_presentation again", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await displayPresentation("item-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("display_presentation", { itemId: "item-1" });
  });

  it("clearPresentationDisplay calls clear_presentation_display with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue(null);

    await clearPresentationDisplay();

    expect(invokeMock).toHaveBeenCalledWith("clear_presentation_display", undefined);
  });

  it("closePresentationDisplay calls close_presentation_display with the screen argument", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue(undefined);

    await closePresentationDisplay("confidence");

    expect(invokeMock).toHaveBeenCalledWith("close_presentation_display", { screen: "confidence" });
  });

  it("rejects every presentation display command outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(openPresentationDisplay("stage")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(getPresentationDisplayState()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(displayPresentation("id")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(clearPresentationDisplay()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(closePresentationDisplay("stage")).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("logDisplayDiagnostic (Phase 3.8.3 temporary diagnostic) calls log_display_diagnostic with stage and detail", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue(undefined);

    await logDisplayDiagnostic("mounted", "component mounted");

    expect(invokeMock).toHaveBeenCalledWith("log_display_diagnostic", {
      stage: "mounted",
      detail: "component mounted",
    });
  });

  it("logDisplayDiagnostic rejects outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(logDisplayDiagnostic("mounted", "component mounted")).rejects.toBeInstanceOf(
      TauriUnavailableError,
    );
    expect(invokeMock).not.toHaveBeenCalled();
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

  it("listAcousticEnrollments takes no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await listAcousticEnrollments();

    expect(invokeMock).toHaveBeenCalledWith("list_acoustic_enrollments", undefined);
  });

  it("enrollAcousticReference forwards songId, contentId, and sourcePath as-is", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await enrollAcousticReference("hymn-1", "music:dev-hymnbook", "/home/operator/hymn-1.wav");

    expect(invokeMock).toHaveBeenCalledWith("enroll_acoustic_reference", {
      songId: "hymn-1",
      contentId: "music:dev-hymnbook",
      sourcePath: "/home/operator/hymn-1.wav",
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

  it("removeAcousticReference forwards songId as-is", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue(undefined);

    await removeAcousticReference("hymn-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("remove_acoustic_reference", { songId: "hymn-1" });
  });

  it("rejects listAcousticEnrollments/enrollAcousticReference/removeAcousticReference outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(listAcousticEnrollments()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(enrollAcousticReference("s1", "music:dev", "/tmp/s1.wav")).rejects.toBeInstanceOf(
      TauriUnavailableError,
    );
    await expect(removeAcousticReference("s1")).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  // --- Phase 8: Production Integration (OBS/vMix) ---------------------------

  it("setProductionIntegrationConfig forwards the config object as-is", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue(undefined);

    const config = {
      obs: { host: "127.0.0.1", port: 4455, password: null, sourceName: "verse-text" },
      vmix: null,
    };
    await setProductionIntegrationConfig(config);

    expect(invokeMock).toHaveBeenCalledWith("set_production_integration_config", { config });
  });

  it("getProductionIntegrationStatus takes no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({ obsLastPush: null, vmixLastPush: null });

    await getProductionIntegrationStatus();

    expect(invokeMock).toHaveBeenCalledWith("get_production_integration_status", undefined);
  });

  it("testObsConnection/testVmixConnection forward their target objects as-is", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue(undefined);

    const obsTarget = { host: "127.0.0.1", port: 4455, password: null, sourceName: "verse-text" };
    await testObsConnection(obsTarget);
    expect(invokeMock).toHaveBeenCalledWith("test_obs_connection", { target: obsTarget });

    const vmixTarget = { host: "127.0.0.1", port: 8088, input: "LowerThird", selectedName: null };
    await testVmixConnection(vmixTarget);
    expect(invokeMock).toHaveBeenCalledWith("test_vmix_connection", { target: vmixTarget });
  });

  it("rejects setProductionIntegrationConfig/getProductionIntegrationStatus/testObsConnection/testVmixConnection outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(setProductionIntegrationConfig({ obs: null, vmix: null })).rejects.toBeInstanceOf(
      TauriUnavailableError,
    );
    await expect(getProductionIntegrationStatus()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(
      testObsConnection({ host: "h", port: 1, password: null, sourceName: "s" }),
    ).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(
      testVmixConnection({ host: "h", port: 1, input: "i", selectedName: null }),
    ).rejects.toBeInstanceOf(TauriUnavailableError);
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

  // --- content intelligence (Phase 2.7, per the authoritative Phase 2 roadmap) --

  /** Proves the explicit, never-automatic nature of content intelligence:
   * `analyzeContentIntelligence` is its own distinct command, never bundled
   * into a transcript-analysis call. */
  it("analyzeContentIntelligence calls only analyze_content_intelligence with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await analyzeContentIntelligence();

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("analyze_content_intelligence", undefined);
  });

  it("listContentCandidates calls list_content_candidates with no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await listContentCandidates();

    expect(invokeMock).toHaveBeenCalledWith("list_content_candidates", undefined);
  });

  it("acceptContentCandidate calls only accept_content_candidate, never a presentation command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await acceptContentCandidate("candidate-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("accept_content_candidate", {
      candidateId: "candidate-1",
    });
  });

  it("rejectContentCandidate calls only reject_content_candidate, never a presentation command", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await rejectContentCandidate("candidate-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("reject_content_candidate", {
      candidateId: "candidate-1",
    });
  });

  it("rejects every content-intelligence command outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(analyzeContentIntelligence()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(listContentCandidates()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(acceptContentCandidate("id")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(rejectContentCandidate("id")).rejects.toBeInstanceOf(TauriUnavailableError);
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

  // --- post-service observability report (Phase 5.1) ----------------------

  it("getServiceReport calls get_service_report with the service id", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({});

    await getServiceReport("service-1");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("get_service_report", { serviceId: "service-1" });
  });

  it("rejects getServiceReport outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(getServiceReport("service-1")).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
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

  // --- Phase 10: Church/User Roles & Permissions -----------------------------

  it("listOperatorAccounts takes no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue([]);

    await listOperatorAccounts();

    expect(invokeMock).toHaveBeenCalledWith("list_operator_accounts", undefined);
  });

  it("createOperatorAccount forwards displayName/pin/role as-is", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({
      id: "op-1",
      displayName: "Pastor Sam",
      role: "admin",
      createdAt: "2026-01-01T00:00:00Z",
    });

    await createOperatorAccount("Pastor Sam", "4242", "admin");

    expect(invokeMock).toHaveBeenCalledWith("create_operator_account", {
      displayName: "Pastor Sam",
      pin: "4242",
      role: "admin",
    });
  });

  it("login forwards accountId/pin as-is", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({
      id: "op-1",
      displayName: "Pastor Sam",
      role: "admin",
      createdAt: "2026-01-01T00:00:00Z",
    });

    await login("op-1", "4242");

    expect(invokeMock).toHaveBeenCalledWith("login", { accountId: "op-1", pin: "4242" });
  });

  it("logout takes no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue(undefined);

    await logout();

    expect(invokeMock).toHaveBeenCalledWith("logout", undefined);
  });

  it("getCurrentOperator takes no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue(null);

    await getCurrentOperator();

    expect(invokeMock).toHaveBeenCalledWith("get_current_operator", undefined);
  });

  it("rejects listOperatorAccounts/createOperatorAccount/login/logout/getCurrentOperator outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(listOperatorAccounts()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(createOperatorAccount("Name", "1234", "operator")).rejects.toBeInstanceOf(
      TauriUnavailableError,
    );
    await expect(login("op-1", "1234")).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(logout()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(getCurrentOperator()).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  // --- Phase 11: Local Congregant Companion View ------------------------------

  it("enableCongregantCompanion takes no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({ running: true, port: 49876, urls: ["http://192.168.1.5:49876/"] });

    await enableCongregantCompanion();

    expect(invokeMock).toHaveBeenCalledWith("enable_congregant_companion", undefined);
  });

  it("disableCongregantCompanion takes no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({ running: false, port: 49876, urls: [] });

    await disableCongregantCompanion();

    expect(invokeMock).toHaveBeenCalledWith("disable_congregant_companion", undefined);
  });

  it("getCongregantCompanionStatus takes no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({ running: false, port: 49876, urls: [] });

    await getCongregantCompanionStatus();

    expect(invokeMock).toHaveBeenCalledWith("get_congregant_companion_status", undefined);
  });

  it("rejects enableCongregantCompanion/disableCongregantCompanion/getCongregantCompanionStatus outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(enableCongregantCompanion()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(disableCongregantCompanion()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(getCongregantCompanionStatus()).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  // --- Phase 12: multi-language Whisper ---------------------------------------

  it("getSpeechLanguageCapabilities takes no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({
      currentLanguage: "en",
      supportedLanguages: [{ code: "en", name: "English" }],
      modelIsMultilingual: null,
    });

    await getSpeechLanguageCapabilities();

    expect(invokeMock).toHaveBeenCalledWith("get_speech_language_capabilities", undefined);
  });

  it("setSpeechLanguage forwards language as-is", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({
      currentLanguage: "yo",
      supportedLanguages: [{ code: "en", name: "English" }],
      modelIsMultilingual: true,
    });

    await setSpeechLanguage("yo");

    expect(invokeMock).toHaveBeenCalledWith("set_speech_language", { language: "yo" });
  });

  it("rejects getSpeechLanguageCapabilities/setSpeechLanguage outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(getSpeechLanguageCapabilities()).rejects.toBeInstanceOf(TauriUnavailableError);
    await expect(setSpeechLanguage("en")).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  // --- Phase 13: Church Knowledge Base / cross-sermon analytics --------------

  it("getChurchKnowledgeBase takes no arguments", async () => {
    isTauriMock.mockReturnValue(true);
    invokeMock.mockResolvedValue({
      themeFrequency: [],
      sermonsBySpeaker: [],
      recentFindings: [],
      generatedAt: "2026-01-01T00:00:00Z",
    });

    await getChurchKnowledgeBase();

    expect(invokeMock).toHaveBeenCalledWith("get_church_knowledge_base", undefined);
  });

  it("rejects getChurchKnowledgeBase outside the Tauri runtime, without calling invoke()", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(getChurchKnowledgeBase()).rejects.toBeInstanceOf(TauriUnavailableError);
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
