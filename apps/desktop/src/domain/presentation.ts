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
}
