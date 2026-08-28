/**
 * Pure Service Replay logic (Phase 3.8), extracted from `ServiceReplay.tsx`
 * so it stays independently unit-testable without a DOM - this project's
 * established convention for component-local pure logic (see
 * `components/workspace/actions.ts`).
 */

export type ReplaySpeed = 0.25 | 0.5 | 1 | 2 | 4 | "instant";

const BASE_DELAY_MS = 4000;

/** The delay between segments at a given speed - "instant" means no wait
 * at all (still sequential, never concurrent), never a substitute for
 * real live-audio timing. */
export function delayForSpeed(speed: ReplaySpeed): number {
  if (speed === "instant") return 0;
  return Math.round(BASE_DELAY_MS / speed);
}

/** Paragraph-first segmentation (blank-line separated) - a natural match
 * for how a spoken service actually pauses. Falls back to sentence
 * splitting only when the whole input is a single huge paragraph, so a
 * pasted wall of text still produces a meaningfully sequenced replay
 * instead of one giant segment. */
export function segmentTranscript(text: string): string[] {
  const paragraphs = text
    .split(/\n\s*\n/)
    .map((p) => p.replace(/\s+/g, " ").trim())
    .filter((p) => p.length > 0);
  if (paragraphs.length > 1) return paragraphs;
  const whole = text.replace(/\s+/g, " ").trim();
  if (!whole) return [];
  const sentences = whole
    .split(/(?<=[.?!])\s+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  return sentences.length > 0 ? sentences : [whole];
}
