/**
 * Presentation domain contracts. Mirrors `core/presentation` (Rust).
 *
 * This describes *what* is shown, not *how* it's rendered - rendering is a
 * separate concern (`presentation/renderer` on the Rust side) so the
 * AI/suggestion pipeline never couples directly to the on-screen renderer.
 */

export type PresentationContent =
  | { type: "scripture"; reference: string; translationId: string; text: string }
  | { type: "text"; title: string | null; body: string };

export type PresentationItemStatus = "prepared" | "active" | "stopped";

export interface PresentationItem {
  id: string;
  serviceId: string;
  content: PresentationContent;
  status: PresentationItemStatus;
  createdAt: string; // ISO-8601
  /** The suggestion this item was prepared from, when it came from the
   * automatic detection + approval path rather than manual creation. */
  sourceSuggestionId: string | null;
  /** The rendering template applied (e.g. `"SCRIPTURE_DEFAULT"`), when set. */
  template: string | null;
}

/** Mirrors `cip_presentation_renderer::RenderedSlide` (Rust) - the
 * deterministic, structured output of rendering a `PresentationContent`.
 * No styling here; a future renderer turns this into pixels. */
export interface RenderedSlide {
  template: string;
  heading: string;
  bodyLines: string[];
  footer: string | null;
}

/** The response of `preview_presentation`/`preview_scripture` - a
 * non-mutating render, never anything that was persisted. */
export interface PresentationPreview {
  content: PresentationContent;
  slide: RenderedSlide;
}
