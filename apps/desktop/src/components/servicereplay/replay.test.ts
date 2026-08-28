import { describe, expect, it } from "vitest";
import { delayForSpeed, segmentTranscript } from "./replay";

describe("segmentTranscript", () => {
  it("splits on blank lines into paragraphs", () => {
    const text = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
    expect(segmentTranscript(text)).toEqual(["First paragraph.", "Second paragraph.", "Third paragraph."]);
  });

  it("collapses internal whitespace within a paragraph", () => {
    const text = "Line one\nstill line one.\n\nLine two.";
    expect(segmentTranscript(text)).toEqual(["Line one still line one.", "Line two."]);
  });

  it("falls back to sentence splitting for a single huge paragraph", () => {
    const text = "God so loved the world. He gave His only Son. Whoever believes shall not perish.";
    expect(segmentTranscript(text)).toEqual([
      "God so loved the world.",
      "He gave His only Son.",
      "Whoever believes shall not perish.",
    ]);
  });

  it("returns an empty array for blank input", () => {
    expect(segmentTranscript("")).toEqual([]);
    expect(segmentTranscript("   \n\n  ")).toEqual([]);
  });

  it("returns the whole trimmed text as one segment when it has no sentence boundary", () => {
    expect(segmentTranscript("just one clause with no terminal punctuation")).toEqual([
      "just one clause with no terminal punctuation",
    ]);
  });
});

describe("delayForSpeed", () => {
  it("is zero at instant speed", () => {
    expect(delayForSpeed("instant")).toBe(0);
  });

  it("halves the delay when speed doubles", () => {
    const at1x = delayForSpeed(1);
    const at2x = delayForSpeed(2);
    expect(at2x).toBe(at1x / 2);
  });

  it("is slower than 1x at 0.5x", () => {
    expect(delayForSpeed(0.5)).toBeGreaterThan(delayForSpeed(1));
  });
});
