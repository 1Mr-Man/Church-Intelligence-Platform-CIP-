/**
 * Service Replay's pure scheduling/segmentation logic (Phase 3.8, revised
 * Phase 3.8.1). Kept free of React/Tauri so it stays trivially unit
 * testable (see `replay.test.ts`) - this file decides *how a transcript is
 * split and paced*, never *what it means*: no Bible/Sermon detection lives
 * here, only text chunking and timing.
 */

export type ReplaySpeed = 0.25 | 0.5 | 1 | 2 | 4 | "instant";

const BASE_DELAY_MS = 4000;

export function delayForSpeed(speed: ReplaySpeed): number {
  if (speed === "instant") return 0;
  return Math.round(BASE_DELAY_MS / speed);
}

/** One scheduled replay segment (spec 3.8.1 section 3's required fields,
 * minus `source`/session identity - those are operational concerns the
 * component attaches at the point of use, not properties of a pure text
 * chunk). */
export interface ReplaySegment {
  sequence: number;
  timestampLabel: string | null;
  text: string;
}

/**
 * A "sensible service speech chunk" ceiling (Phase 3.8.1 section 3): large
 * enough that a real sermon isn't reduced to hundreds of one-sentence
 * calls, small enough that a real sermon isn't reduced to a couple of
 * giant blocks (the exact defect reported against Phase 3.8 - a real
 * 52-minute transcript with only two blank-line breaks collapsed to two
 * segments). Roughly two to three spoken sentences.
 */
const MAX_CHUNK_CHARS = 220;

/** A line that is *only* one or two timecodes - the common convention for
 * exported/subtitle-style transcripts (`00:00:04.560 --> 00:00:13.920`,
 * `[00:00:04 - 00:00:13]`, or a single `00:00:04:`). A wrapping `[...]` is
 * stripped before matching (see below); the inner pattern then requires
 * the ENTIRE remaining line to be just the timecode(s), so an ordinary
 * sentence that merely mentions a time is never mistaken for a cue
 * marker. */
const CUE_LINE =
  /^(\d{1,2}:\d{2}:\d{2}(?:[.,]\d{1,3})?)\s*(?:-->|[-–—])?\s*(\d{1,2}:\d{2}:\d{2}(?:[.,]\d{1,3})?)?:?$/;

function parseTimestampCues(text: string): ReplaySegment[] | null {
  const lines = text.split(/\r?\n/);
  const cues: Array<{ label: string; text: string }> = [];
  let currentLabel: string | null = null;
  let buffer: string[] = [];

  const flush = () => {
    if (currentLabel !== null) {
      const combined = buffer.join(" ").replace(/\s+/g, " ").trim();
      if (combined.length > 0) cues.push({ label: currentLabel, text: combined });
    }
  };

  for (const rawLine of lines) {
    let line = rawLine.trim();
    if (!line) continue;
    if (line.startsWith("[") && line.endsWith("]")) line = line.slice(1, -1).trim();
    const match = line.match(CUE_LINE);
    if (match) {
      flush();
      currentLabel = match[2] ? `${match[1]}–${match[2]}` : match[1];
      buffer = [];
    } else if (currentLabel !== null) {
      buffer.push(rawLine.trim());
    }
    // Text encountered before any cue marker is seen is ignored - this is
    // only a cue-based transcript once at least one marker has appeared.
  }
  flush();

  if (cues.length < 2) return null; // not really a timestamped transcript

  return cues.map((cue, i) => ({ sequence: i, timestampLabel: cue.label, text: cue.text }));
}

function splitIntoSentences(paragraph: string): string[] {
  const whole = paragraph.replace(/\s+/g, " ").trim();
  if (!whole) return [];
  const sentences = whole
    .split(/(?<=[.?!])\s+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  return sentences.length > 0 ? sentences : [whole];
}

/** Groups sentences into chunks bounded by `MAX_CHUNK_CHARS`, never
 * splitting a single sentence and never leaving a non-empty buffer
 * unflushed. This is the actual fix for the reported 2-segment collapse:
 * every paragraph - whether there is one giant paragraph or only two -
 * passes through here rather than being returned unbounded. */
function chunkSentences(sentences: string[]): string[] {
  const chunks: string[] = [];
  let buffer = "";
  for (const sentence of sentences) {
    const candidate = buffer ? `${buffer} ${sentence}` : sentence;
    if (buffer && candidate.length > MAX_CHUNK_CHARS) {
      chunks.push(buffer);
      buffer = sentence;
    } else {
      buffer = candidate;
    }
  }
  if (buffer) chunks.push(buffer);
  return chunks;
}

/**
 * Splits a transcript into sequential replay segments.
 *
 * 1. If the transcript looks like a cue-based export (at least two
 *    standalone timestamp lines), segment by cue - each cue's associated
 *    text becomes one segment, in order.
 * 2. Otherwise, split on blank lines into paragraphs (or treat the whole
 *    text as one paragraph if there are no blank-line breaks), then chunk
 *    every paragraph's sentences into bounded-size groups. A short
 *    paragraph that already fits under the chunk ceiling stays as one
 *    segment; a long one (or a small number of long ones - the reported
 *    defect) is split into several reasonably-sized segments instead of
 *    being returned as one unbounded block.
 */
export function segmentTranscript(text: string): ReplaySegment[] {
  const timestamped = parseTimestampCues(text);
  if (timestamped) return timestamped;

  const paragraphs = text
    .split(/\n\s*\n/)
    .map((p) => p.replace(/\s+/g, " ").trim())
    .filter((p) => p.length > 0);

  const sourceParagraphs = paragraphs.length > 0 ? paragraphs : [text];

  const chunks: string[] = [];
  for (const paragraph of sourceParagraphs) {
    chunks.push(...chunkSentences(splitIntoSentences(paragraph)));
  }

  return chunks
    .filter((c) => c.length > 0)
    .map((c, i) => ({ sequence: i, timestampLabel: null, text: c }));
}
