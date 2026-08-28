import { describe, expect, it } from "vitest";
import { delayForSpeed, segmentTranscript } from "./replay";

const texts = (text: string) => segmentTranscript(text).map((s) => s.text);

describe("segmentTranscript", () => {
  it("splits on blank lines into paragraphs when each stays under the chunk ceiling", () => {
    const text = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
    expect(texts(text)).toEqual(["First paragraph.", "Second paragraph.", "Third paragraph."]);
  });

  it("collapses internal whitespace within a paragraph", () => {
    const text = "Line one\nstill line one.\n\nLine two.";
    expect(texts(text)).toEqual(["Line one still line one.", "Line two."]);
  });

  it("groups short sentences in a single paragraph into one segment rather than one segment per sentence", () => {
    // Phase 3.8.1: "sensible service speech chunking," not a microscopic
    // one-sentence-per-segment split - short sentences that together stay
    // under the chunk ceiling are merged into a single segment.
    const text = "God so loved the world. He gave His only Son. Whoever believes shall not perish.";
    const segments = segmentTranscript(text);
    expect(segments).toHaveLength(1);
    expect(segments[0].text).toBe(text);
  });

  it("REGRESSION (Phase 3.8.1): a small number of large paragraphs does not collapse to a handful of giant segments", () => {
    // Reproduces the reported defect: a real ~52-minute transcript with
    // only two blank-line breaks was returned as exactly 2 unbounded
    // segments. Two large paragraphs here must instead split into several
    // bounded-size segments each.
    const sentence = (n: number) => `This is sentence number ${n} of the sermon, spoken today with feeling.`;
    const paragraph = Array.from({ length: 8 }, (_, i) => sentence(i + 1)).join(" ");
    const text = `${paragraph}\n\n${paragraph}`;

    const segments = segmentTranscript(text);

    expect(segments.length).toBeGreaterThan(2);
    for (const segment of segments) {
      expect(segment.text.length).toBeLessThanOrEqual(220);
    }
    // Sequential, gap-free numbering.
    segments.forEach((segment, i) => expect(segment.sequence).toBe(i));
  });

  it("segments a cue-based (timestamped) transcript by cue line, in order", () => {
    const text = [
      "00:00:04.560 --> 00:00:13.920",
      "Let's pray. Father, we thank You.",
      "",
      "00:00:18.480 --> 00:00:32.159",
      "We give You the glory and all of the honor.",
      "",
      "00:00:37.040 --> 00:00:48.480",
      "I ask that as we kick off this session, You would meet us here.",
    ].join("\n");

    const segments = segmentTranscript(text);

    expect(segments).toEqual([
      { sequence: 0, timestampLabel: "00:00:04.560–00:00:13.920", text: "Let's pray. Father, we thank You." },
      {
        sequence: 1,
        timestampLabel: "00:00:18.480–00:00:32.159",
        text: "We give You the glory and all of the honor.",
      },
      {
        sequence: 2,
        timestampLabel: "00:00:37.040–00:00:48.480",
        text: "I ask that as we kick off this session, You would meet us here.",
      },
    ]);
  });

  it("segments a bracketed single-timestamp transcript by cue line", () => {
    const text = ["[00:00:04]", "Good morning church.", "[00:00:12]", "Today's message is on faithfulness."].join(
      "\n",
    );
    const segments = segmentTranscript(text);
    expect(segments.map((s) => s.timestampLabel)).toEqual(["00:00:04", "00:00:12"]);
    expect(segments.map((s) => s.text)).toEqual(["Good morning church.", "Today's message is on faithfulness."]);
  });

  it("does not mistake a single ordinary timestamp mention for a cue transcript", () => {
    // Only one cue-like line (and it's not a standalone line, it's part of
    // a sentence) - must fall back to ordinary paragraph/sentence chunking.
    const text = "Service starts at 00:00:04 sharp. Please be seated.";
    const segments = segmentTranscript(text);
    expect(segments.every((s) => s.timestampLabel === null)).toBe(true);
  });

  it("returns an empty array for blank input", () => {
    expect(segmentTranscript("")).toEqual([]);
    expect(segmentTranscript("   \n\n  ")).toEqual([]);
  });

  it("returns the whole trimmed text as one segment when it has no sentence boundary", () => {
    expect(texts("just one clause with no terminal punctuation")).toEqual([
      "just one clause with no terminal punctuation",
    ]);
  });

  it("REGRESSION (Phase 3.8.2): a realistically long, single-block transcript segments into a manageable number of bounded pieces, quickly", () => {
    // Simulates the worst case for segmentation - a real transcript export
    // with no blank-line paragraph breaks at all, comparable in scale to
    // the operator's real ~52-minute sermon (the actual file was not
    // supplied to this environment - see docs/phase-3-8-2-audit.md section
    // I item 7 - so this is a project-authored synthetic transcript of
    // similar word count, not a copy of the real one). Roughly 7,000 words
    // / ~42,000 characters, one giant paragraph, no timestamps.
    const sentence = (n: number) =>
      `In this part of the message we consider point number ${n} about walking faithfully with God every day.`;
    const longTranscript = Array.from({ length: 400 }, (_, i) => sentence(i + 1)).join(" ");
    expect(longTranscript.length).toBeGreaterThan(30000);

    const start = performance.now();
    const segments = segmentTranscript(longTranscript);
    const elapsedMs = performance.now() - start;

    // Not 1-2 giant blocks (the original defect) and not thousands of
    // microscopic ones (the spec's other explicit constraint) - a few
    // hundred bounded, speech-sized chunks.
    expect(segments.length).toBeGreaterThan(50);
    expect(segments.length).toBeLessThan(1000);
    for (const segment of segments) {
      expect(segment.text.length).toBeLessThanOrEqual(220);
    }
    segments.forEach((segment, i) => expect(segment.sequence).toBe(i));

    // Synchronous, single-pass segmentation must never noticeably block
    // the UI thread even at this scale.
    expect(elapsedMs).toBeLessThan(500);
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
