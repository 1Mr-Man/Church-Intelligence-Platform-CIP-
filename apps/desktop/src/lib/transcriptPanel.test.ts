import { describe, expect, it } from "vitest";
import { isLiveTranscriptCollapsed } from "./transcriptPanel";

describe("isLiveTranscriptCollapsed", () => {
  it("stays expanded when nothing has been stored yet", () => {
    expect(isLiveTranscriptCollapsed(null)).toBe(false);
  });

  it("stays expanded for an empty stored value", () => {
    expect(isLiveTranscriptCollapsed("")).toBe(false);
  });

  it("stays expanded for any value other than the exact collapsed marker", () => {
    expect(isLiveTranscriptCollapsed("true")).toBe(false);
  });

  it("collapses once the collapsed marker is stored", () => {
    expect(isLiveTranscriptCollapsed("collapsed")).toBe(true);
  });
});
