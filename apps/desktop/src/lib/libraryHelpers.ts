/**
 * Pure helpers for the Church Knowledge Libraries (Phase 3.6) - kept
 * separate from the components that use them so they're independently
 * unit-testable, matching this codebase's existing convention
 * (`unifiedFeed.ts`, `attentionQueue.ts`, `timelineFormat.ts` are all
 * plain functions with their own `.test.ts`, never tested only through a
 * rendered component).
 */
import type { PresentationItem } from "../domain";

/** `("ROM", 8, 28)` -> `"ROM 8:28"`; `("ROM", 8, 28, 30)` -> `"ROM
 * 8:28-30"` - the exact display form `searchBible`/`previewScripture`/
 * `createManualPresentation` expect, and what `build_scripture_slide`
 * (Rust) now parses as a genuine range (Phase 3.6 fixed the old
 * first-verse-only truncation - see docs/phase-3-6-church-libraries.md). */
export function referenceFor(book: string, chapter: number, verseStart: number, verseEnd?: number | null): string {
  if (verseEnd && verseEnd !== verseStart) {
    return `${book} ${chapter}:${verseStart}-${verseEnd}`;
  }
  return `${book} ${chapter}:${verseStart}`;
}

/** The History view's per-item heading - a `PresentationItem`'s Scripture
 * reference or text title, falling back honestly to "(untitled)" rather
 * than guessing (mirrors `PresentationCard.tsx`'s identical helper). */
export function presentationHeading(item: PresentationItem): string {
  return item.content.type === "scripture" ? item.content.reference : item.content.title ?? "(untitled)";
}

/** Validates an operator-entered "from"/"to" verse-range pair (Bible
 * Library's range tool) - both must be present, numeric, and non-inverted.
 * Returns the parsed pair or `null` for anything invalid, never throws. */
export function parseVerseRange(from: string, to: string): { from: number; to: number } | null {
  const start = Number.parseInt(from, 10);
  const end = Number.parseInt(to, 10);
  if (!Number.isFinite(start) || !Number.isFinite(end) || start < 1 || end < 1 || start > end) {
    return null;
  }
  return { from: start, to: end };
}
