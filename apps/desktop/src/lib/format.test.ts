import { describe, expect, it } from "vitest";
import { describeAudioSignal, formatClockTime } from "./format";

describe("formatClockTime", () => {
  it("returns the original string for an unparseable timestamp", () => {
    expect(formatClockTime("not-a-date")).toBe("not-a-date");
  });

  it("formats a real ISO timestamp as a local HH:MM:SS clock time", () => {
    const result = formatClockTime("2026-01-01T12:00:00Z");
    expect(result).toMatch(/^\d{2}:\d{2}:\d{2}$/);
  });
});

describe("describeAudioSignal", () => {
  it("reports 'not yet reported' before any reading arrives", () => {
    expect(describeAudioSignal(null)).toBe("Capturing — input level not yet reported");
  });

  it("reports NO SIGNAL at and below the silence floor", () => {
    expect(describeAudioSignal(0)).toContain("NO SIGNAL");
    expect(describeAudioSignal(0.01)).toContain("NO SIGNAL");
  });

  it("reports LOW SIGNAL with a concrete suggestion for a quiet-but-real level", () => {
    // The exact level (6%) a real Windows pilot session reported alongside
    // an unusable, hallucination-filled transcript - see docs/phase-14-audit.md.
    const message = describeAudioSignal(0.06);
    expect(message).toContain("LOW SIGNAL");
    expect(message).toContain("6%");
    expect(message.toLowerCase()).toContain("microphone");
  });

  it("reports plain SIGNAL CAPTURED at a healthy level", () => {
    const message = describeAudioSignal(0.4);
    expect(message).toBe("SIGNAL CAPTURED — input level 40%");
    expect(message).not.toContain("LOW SIGNAL");
  });

  it("never mislabels the boundary between LOW SIGNAL and healthy signal", () => {
    expect(describeAudioSignal(0.14)).toContain("LOW SIGNAL");
    expect(describeAudioSignal(0.15)).not.toContain("LOW SIGNAL");
  });
});
