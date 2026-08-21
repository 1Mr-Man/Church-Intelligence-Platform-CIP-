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
  createManualPresentation,
  getAppConfig,
  previewPresentation,
  previewScripture,
  searchBible,
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

    expect(invokeMock).toHaveBeenCalledWith("search_bible", { query: "Romans 8:28" });
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
});
