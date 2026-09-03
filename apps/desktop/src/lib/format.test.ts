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

  it("gives loopback-specific NO SIGNAL guidance for a Stereo Mix device, never 'move the microphone'", () => {
    // The exact device name + reading (0%) a real Windows pilot session
    // reported while playing a YouTube video through Stereo Mix.
    const message = describeAudioSignal(0, "Stereo Mix (Realtek(R) Audio)");
    expect(message).toContain("NO SIGNAL");
    expect(message.toLowerCase()).toContain("recording");
    expect(message.toLowerCase()).not.toContain("microphone");
  });

  it("gives loopback-specific LOW SIGNAL guidance without suggesting a microphone gain adjustment", () => {
    const message = describeAudioSignal(0.06, "Stereo Mix (Realtek(R) Audio)");
    expect(message).toContain("LOW SIGNAL");
    expect(message.toLowerCase()).toContain("volume");
    expect(message.toLowerCase()).not.toContain("microphone");
  });

  it("detects loopback devices case-insensitively and by common alias", () => {
    expect(describeAudioSignal(0, "STEREO MIX").toLowerCase()).toContain("recording");
    expect(describeAudioSignal(0, "What U Hear (Realtek Audio)").toLowerCase()).toContain("recording");
  });

  it("keeps the original physical-microphone wording for a normal input device", () => {
    expect(describeAudioSignal(0, "Microphone Array (Intel Smart Sound Technology)")).toBe(
      "NO SIGNAL — audio device is capturing but no sound is being detected",
    );
    const low = describeAudioSignal(0.06, "Microphone Array (Intel Smart Sound Technology)");
    expect(low.toLowerCase()).toContain("microphone");
  });

  it("keeps the original wording when no device name is known", () => {
    expect(describeAudioSignal(0, null)).toBe("NO SIGNAL — audio device is capturing but no sound is being detected");
  });
});
