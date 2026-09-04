/**
 * Phase 24: the dual Live/Preview panel from the operator's own reference
 * images (professional live-service software - ProPresenter/EasyWorship-
 * style) - the piece Phase 23's layout-shell milestone explicitly deferred.
 * See docs/phase-24-audit.md.
 *
 * Purely presentational and purely additive: every value it renders was
 * already being fetched by `LiveChurchBrain`/`ServiceReplay` (or, for
 * `activeSlide`, was already present in the exact same command/event
 * payloads those files already handle - `PresentationDisplayState.activeSlide`
 * and `PresentationDisplayPayload.slide` - just never surfaced before this
 * phase). No new Tauri command, no new backend call, no second rendering
 * path: the Live panel shows literally the same `RenderedSlide` the real
 * display window renders from (see `PresentationDisplay.tsx`), and the
 * Preview panel shows literally the same `RenderedSlide` the operator's
 * own existing "Preview" button already fetches via `preview_presentation`
 * - this component only adds a place to see it as a slide, not a new way
 * of computing it.
 *
 * The queue strip below intentionally reuses the exact same `onDisplay`
 * action `PresentationCard`'s own "Display" button already calls - a
 * second visual affordance for an already-existing, already-deliberate
 * single action, not a new one. Per this project's own explicit-
 * activation safety model (docs/presentation.md), clicking a queued
 * thumbnail sends it live immediately, exactly like `PresentationCard`'s
 * button does today; it is not a "select for preview" affordance, since
 * a prepared item has no separate re-renderable preview state to select
 * into without a new backend call this phase does not add.
 */
import type { PresentationItem, RenderedSlide } from "../../domain";

export interface LivePreviewStageProps {
  activeSlide: RenderedSlide | null;
  previewSlide: RenderedSlide | null;
  preparedItems: PresentationItem[];
  activeDisplayItemId: string | null;
  busy: string | null;
  onDisplayQueued: (itemId: string) => void;
}

function queueHeading(item: PresentationItem): string {
  return item.content.type === "scripture" ? item.content.reference : item.content.title ?? "(untitled)";
}

function queueSnippet(item: PresentationItem): string | null {
  return item.content.type === "scripture" ? item.content.text : null;
}

function SlideBox({ slide, emptyText }: { slide: RenderedSlide | null; emptyText: string }) {
  if (!slide) {
    return <div className="live-preview-stage__slide live-preview-stage__slide--empty">{emptyText}</div>;
  }
  return (
    <div className="live-preview-stage__slide">
      {slide.heading && <p className="live-preview-stage__slide-heading">{slide.heading}</p>}
      <div className="live-preview-stage__slide-body">
        {slide.bodyLines.map((line, i) => (
          <p key={i}>{line}</p>
        ))}
      </div>
      {slide.footer && <p className="live-preview-stage__slide-footer">{slide.footer}</p>}
    </div>
  );
}

export function LivePreviewStage({
  activeSlide,
  previewSlide,
  preparedItems,
  activeDisplayItemId,
  busy,
  onDisplayQueued,
}: LivePreviewStageProps) {
  return (
    <section className="live-preview-stage">
      <div className="live-preview-stage__panels">
        <div className="live-preview-stage__panel live-preview-stage__panel--live">
          <p className="live-preview-stage__panel-label live-preview-stage__panel-label--live">&#9679; LIVE</p>
          <SlideBox slide={activeSlide} emptyText="Nothing on screen" />
        </div>
        <div className="live-preview-stage__panel live-preview-stage__panel--preview">
          <p className="live-preview-stage__panel-label">PREVIEW</p>
          <SlideBox slide={previewSlide} emptyText="Preview an approved item below to see it here" />
        </div>
      </div>
      {preparedItems.length > 0 && (
        <div className="live-preview-stage__queue" role="list" aria-label="Prepared, ready to display">
          {preparedItems.map((item) => (
            <button
              key={item.id}
              type="button"
              role="listitem"
              className="live-preview-stage__queue-item"
              disabled={!!activeDisplayItemId || busy === `display-${item.id}`}
              title={activeDisplayItemId ? "Stop the currently active item before displaying another" : "Send this live"}
              onClick={() => onDisplayQueued(item.id)}
            >
              <span className="live-preview-stage__queue-heading">{queueHeading(item)}</span>
              {queueSnippet(item) && <span className="live-preview-stage__queue-snippet">{queueSnippet(item)}</span>}
            </button>
          ))}
        </div>
      )}
    </section>
  );
}
