/**
 * Proves the Phase 1.2.1 event-subscription guard: outside the Tauri
 * runtime there is no backend to emit anything, so subscribing must
 * resolve to a harmless no-op `UnlistenFn` rather than calling the real
 * `listen()` (which would reach into a `window.__TAURI_INTERNALS__` that
 * doesn't exist).
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  onCurrentSongChanged,
  onMusicFindingAccepted,
  onMusicFindingDetected,
  onMusicFindingRejected,
  onPresentationPrepared,
  onPresentationPreviewed,
  onSermonFindingAccepted,
  onSermonFindingDetected,
  onSermonFindingRejected,
  onSermonStateChanged,
  onSermonStructureUpdated,
  onSermonThemeChanged,
  onSuggestionCreated,
} from "./liveEvents";

const listenMock = vi.fn();
const isTauriMock = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

vi.mock("./runtime", () => ({
  isTauriRuntime: () => isTauriMock(),
}));

describe("liveEvents.ts Tauri event-subscription guard", () => {
  beforeEach(() => {
    listenMock.mockReset();
    isTauriMock.mockReset();
  });

  it("resolves to a no-op unlisten and never calls listen() outside the Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    const unlisten = await onSuggestionCreated(() => {});

    expect(listenMock).not.toHaveBeenCalled();
    expect(() => unlisten()).not.toThrow();
  });

  it("calls the real listen() inside the Tauri runtime", async () => {
    isTauriMock.mockReturnValue(true);
    listenMock.mockResolvedValue(() => {});

    await onSuggestionCreated(() => {});

    expect(listenMock).toHaveBeenCalledWith("SUGGESTION_CREATED", expect.any(Function));
  });

  it("subscribes to PRESENTATION_PREVIEWED, distinct from PRESENTATION_PREPARED (Phase 1.4)", async () => {
    isTauriMock.mockReturnValue(true);
    listenMock.mockResolvedValue(() => {});

    await onPresentationPreviewed(() => {});
    await onPresentationPrepared(() => {});

    expect(listenMock).toHaveBeenCalledWith("PRESENTATION_PREVIEWED", expect.any(Function));
    expect(listenMock).toHaveBeenCalledWith("PRESENTATION_PREPARED", expect.any(Function));
  });

  it("subscribes to the three distinct music finding events (Phase 2.1)", async () => {
    isTauriMock.mockReturnValue(true);
    listenMock.mockResolvedValue(() => {});

    await onMusicFindingDetected(() => {});
    await onMusicFindingAccepted(() => {});
    await onMusicFindingRejected(() => {});

    expect(listenMock).toHaveBeenCalledWith("MUSIC_FINDING_DETECTED", expect.any(Function));
    expect(listenMock).toHaveBeenCalledWith("MUSIC_FINDING_ACCEPTED", expect.any(Function));
    expect(listenMock).toHaveBeenCalledWith("MUSIC_FINDING_REJECTED", expect.any(Function));
  });

  it("subscribes to CURRENT_SONG_CHANGED (Phase 2.2)", async () => {
    isTauriMock.mockReturnValue(true);
    listenMock.mockResolvedValue(() => {});

    await onCurrentSongChanged(() => {});

    expect(listenMock).toHaveBeenCalledWith("CURRENT_SONG_CHANGED", expect.any(Function));
  });

  it("resolves to a no-op unlisten for onCurrentSongChanged outside the Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    const unlisten = await onCurrentSongChanged(() => {});

    expect(listenMock).not.toHaveBeenCalled();
    expect(() => unlisten()).not.toThrow();
  });

  // --- sermon intelligence (Phase 2.3) --------------------------------------

  it("subscribes to the three distinct sermon finding events", async () => {
    isTauriMock.mockReturnValue(true);
    listenMock.mockResolvedValue(() => {});

    await onSermonFindingDetected(() => {});
    await onSermonFindingAccepted(() => {});
    await onSermonFindingRejected(() => {});

    expect(listenMock).toHaveBeenCalledWith("SERMON_FINDING_DETECTED", expect.any(Function));
    expect(listenMock).toHaveBeenCalledWith("SERMON_FINDING_ACCEPTED", expect.any(Function));
    expect(listenMock).toHaveBeenCalledWith("SERMON_FINDING_REJECTED", expect.any(Function));
  });

  it("subscribes to structure/theme/state change events, distinct from raw finding events", async () => {
    isTauriMock.mockReturnValue(true);
    listenMock.mockResolvedValue(() => {});

    await onSermonStructureUpdated(() => {});
    await onSermonThemeChanged(() => {});
    await onSermonStateChanged(() => {});

    expect(listenMock).toHaveBeenCalledWith("SERMON_STRUCTURE_UPDATED", expect.any(Function));
    expect(listenMock).toHaveBeenCalledWith("SERMON_THEME_CHANGED", expect.any(Function));
    expect(listenMock).toHaveBeenCalledWith("SERMON_STATE_CHANGED", expect.any(Function));
  });

  it("resolves to a no-op unlisten for every sermon event outside the Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    const unlisten = await onSermonFindingDetected(() => {});

    expect(listenMock).not.toHaveBeenCalled();
    expect(() => unlisten()).not.toThrow();
  });
});
