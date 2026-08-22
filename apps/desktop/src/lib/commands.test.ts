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
  analyzeMusicTranscript,
  checkBibleDatasetIntegrity,
  createManualPresentation,
  getAppConfig,
  getIntelligenceCapabilities,
  importBibleDataset,
  importMusicDataset,
  listContentRegistry,
  listMusicFindings,
  previewPresentation,
  previewScripture,
  rejectMusicFinding,
  searchBible,
  searchMusic,
  setContentEnabled,
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
});
