/**
 * These tests exercise the real `@tauri-apps/api/core`'s `isTauri()` (no
 * mocking) - the point is to prove `isTauriRuntime()` correctly reflects
 * the actual mechanism the Tauri WebView uses (`globalThis.isTauri`), not
 * just a mocked stand-in for it.
 */
import { afterEach, describe, expect, it } from "vitest";
import { isTauriRuntime } from "./runtime";

describe("isTauriRuntime", () => {
  afterEach(() => {
    delete (globalThis as { isTauri?: boolean }).isTauri;
  });

  it("is false in a normal browser/test environment with no Tauri runtime present", () => {
    expect(isTauriRuntime()).toBe(false);
  });

  it("is true once the Tauri WebView has set the runtime global", () => {
    (globalThis as { isTauri?: boolean }).isTauri = true;
    expect(isTauriRuntime()).toBe(true);
  });
});
