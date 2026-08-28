/**
 * Pure decision logic for `PresentationDisplay.tsx`'s mount-time hydration
 * (Phase 3.8.2) - extracted so it's directly unit-testable without a
 * component-rendering test library, matching this project's established
 * convention (see `components/servicereplay/replay.ts`).
 */
import type { PresentationDisplayPayload, PresentationDisplayState } from "../domain";

/** Derives the payload the display window should show immediately on
 * mount from a `get_presentation_display_state` response - `null` when
 * nothing is genuinely active (never fabricates a slide). */
export function resolveHydratedPayload(state: PresentationDisplayState): PresentationDisplayPayload | null {
  if (!state.activeItem || !state.activeSlide) return null;
  return { item: state.activeItem, slide: state.activeSlide };
}
