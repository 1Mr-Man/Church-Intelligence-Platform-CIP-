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

/** The `PRESENTATION_STARTED` event payload - both the updated
 * `PresentationItem` (now `Active`) and the already-rendered
 * `RenderedSlide` the display window shows verbatim. No second rendering
 * system on the frontend: the display window never re-derives a slide
 * from raw content, it only ever shows exactly what the backend already
 * rendered. */
export interface PresentationDisplayPayload {
  item: PresentationItem;
  slide: RenderedSlide;
}

/** Phase 3.10: the three display roles CIP can drive simultaneously -
 * mirrors `presentation_display::DisplayScreen` (Rust). "Multi-screen"
 * means the one active presentation item reaching more physical screens,
 * not more concurrent active items - see
 * `docs/phase-3-10-multi-screen-audit.md`. */
export type PresentationScreen = "stage" | "confidence" | "lobby";

/** Phase 3.10.3: whether a screen currently receives the live
 * presentation broadcast. `held` freezes a screen on whatever it
 * currently shows - it does not give it different content from a `live`
 * screen, only opts it out of future updates until switched back. See
 * `docs/phase-3-10-3-presentation-router.md`. */
export type RouteMode = "live" | "held";

/** One screen's open/closed state and route mode, as reported by
 * `get_presentation_display_state`. `routeMode` is independent of
 * `windowOpen` - a closed screen still has a route mode, applied the next
 * time its window opens and something is displayed. */
export interface PresentationScreenState {
  screen: PresentationScreen;
  label: string;
  windowOpen: boolean;
  routeMode: RouteMode;
}

/** The `get_presentation_display_state` response - the operator UI's sync
 * point on mount, never assumed from local state alone. `activeSlide`
 * (Phase 3.8.2) lets a display window itself hydrate on mount instead
 * of depending solely on catching `PRESENTATION_STARTED` live - closing a
 * real race where the event can fire before the display window's own
 * JavaScript has loaded and subscribed. */
export interface PresentationDisplayState {
  screens: PresentationScreenState[];
  activeItem: PresentationItem | null;
  activeSlide: RenderedSlide | null;
}

/** Phase 3.10.2: the role a *physical monitor* plays - mirrors
 * `display_registry::DisplayRole` (Rust). Distinct from
 * `PresentationScreen` above: a `PresentationScreen` identifies a
 * content stream/window (what CIP shows), while a `DisplayRole`
 * identifies which physical monitor that stream is placed on. `stage`
 * here means "speaker-facing information" per the operator's own
 * taxonomy and has no corresponding `PresentationScreen` yet - CIP maps
 * `PresentationScreen: "stage"` (the confidence-monitor-facing content
 * stream) to `DisplayRole: "projector"` for placement purposes. See
 * `docs/phase-3-10-2-display-registry.md`. */
export type DisplayRole =
  | "unassigned"
  | "operator"
  | "projector"
  | "stage"
  | "confidence"
  | "lobby";

/** One physical monitor, merged with its persisted role assignment (if
 * any) - mirrors `display_registry::Display` (Rust). `connected: false`
 * means the role was assigned to a monitor that isn't currently
 * detected (e.g. unplugged); its geometry fields are then placeholder
 * values, never fabricated real ones. */
export interface Display {
  monitorId: string;
  name: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
  isPrimary: boolean;
  assignedRole: DisplayRole;
  connected: boolean;
}
