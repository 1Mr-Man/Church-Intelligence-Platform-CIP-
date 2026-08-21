/**
 * Proves the Phase 1.2.1 event-subscription guard: outside the Tauri
 * runtime there is no backend to emit anything, so subscribing must
 * resolve to a harmless no-op `UnlistenFn` rather than calling the real
 * `listen()` (which would reach into a `window.__TAURI_INTERNALS__` that
 * doesn't exist).
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { onSuggestionCreated } from "./liveEvents";

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
});
