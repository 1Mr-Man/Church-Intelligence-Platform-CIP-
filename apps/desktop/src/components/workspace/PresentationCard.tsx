/**
 * Phase 3.5: the Presentation card (spec section 11/12, Priority 4) -
 * replaces two separate, visually-equal panels that used to sit far apart
 * in the old engineering-dashboard layout ("Approved — Ready to Prepare"
 * around line 948 and "Current Output" around line 1072 of the pre-3.5
 * `LiveChurchBrain.tsx`) with one visually dominant card that shows the
 * whole Approve → Prepare → Display pipeline in one place.
 *
 * This is a pure presentational extraction, not new functionality: every
 * prop here is state `LiveChurchBrain` already fetches, and every handler
 * calls exactly the same `commands.*` function the two old panels already
 * called (`previewPresentation`, `preparePresentation`,
 * `openPresentationDisplay`, `closePresentationDisplay`,
 * `displayPresentation`, `cancelPresentation`, `clearPresentationDisplay`).
 * Nothing here introduces a new Tauri command, a new event, or a second
 * copy of presentation state - see `docs/presentation.md`'s explicit-
 * activation safety model, which this card's PREPARED/ACTIVE/STOPPED
 * language exists specifically to make unmistakable to the operator
 * (spec section 11's hard rule: "Prepared" must never be confused with
 * "Displayed").
 *
 * Phase 3.10: the single Open/Close Display button is replaced with a row
 * of three independent screen controls (Stage/Confidence Monitor/Lobby-
 * Overflow), each open/closed on its own - multiple can be open at once,
 * all mirroring the same active item. See
 * `docs/phase-3-10-multi-screen-audit.md`.
 */
import type {
  PresentationItem,
  PresentationPreview,
  PresentationScreen,
  PresentationScreenState,
  Suggestion,
} from "../../domain";

function itemHeading(item: PresentationItem): string {
  return item.content.type === "scripture" ? item.content.reference : item.content.title ?? "(untitled)";
}

function suggestionHeading(s: Suggestion): string {
  return s.kind.type === "scripture" ? s.kind.reference : s.kind.label;
}

export interface PresentationCardProps {
  approvedSuggestions: Suggestion[];
  previews: Record<string, PresentationPreview>;
  preparedItems: PresentationItem[];
  activeDisplayItem: PresentationItem | null;
  screens: PresentationScreenState[];
  busy: string | null;
  onPreviewApproved: (suggestionId: string) => void;
  onPrepare: (suggestionId: string) => void;
  onOpenScreen: (screen: PresentationScreen) => void;
  onCloseScreen: (screen: PresentationScreen) => void;
  onDisplay: (itemId: string) => void;
  onCancel: (itemId: string) => void;
  onStopDisplay: () => void;
}

export function PresentationCard({
  approvedSuggestions,
  previews,
  preparedItems,
  activeDisplayItem,
  screens,
  busy,
  onPreviewApproved,
  onPrepare,
  onOpenScreen,
  onCloseScreen,
  onDisplay,
  onCancel,
  onStopDisplay,
}: PresentationCardProps) {
  const isBusy = (key: string) => busy === key;
  const anyScreenOpen = screens.some((s) => s.windowOpen);
  const nothingToShow = approvedSuggestions.length === 0 && preparedItems.length === 0 && !activeDisplayItem;

  return (
    <section className={`op-presentation${activeDisplayItem ? " op-presentation--active" : ""}`}>
      <div className="op-presentation__header">
        <h2>Presentation</h2>
        <span className={`op-badge ${anyScreenOpen ? "op-badge--good" : "op-badge--neutral"}`}>
          Display {anyScreenOpen ? "Open" : "Closed"}
        </span>
      </div>

      {activeDisplayItem && (
        <div className="op-presentation__item op-presentation__item--active">
          <div className="op-presentation__meta">
            <span className="op-badge op-badge--good">● On Screen</span>
          </div>
          <p className="op-presentation__scripture">{itemHeading(activeDisplayItem)}</p>
          {activeDisplayItem.content.type === "scripture" && (
            <p className="op-presentation__text">{activeDisplayItem.content.text}</p>
          )}
          <div className="op-presentation__actions">
            <button type="button" className="op-button--danger" disabled={isBusy("stop-display")} onClick={onStopDisplay}>
              Stop
            </button>
          </div>
        </div>
      )}

      {preparedItems.map((item) => (
        <div key={item.id} className="op-presentation__item">
          <div className="op-presentation__meta">
            <span className="op-badge op-badge--neutral">Ready to Present</span>
          </div>
          <p className="op-presentation__scripture">{itemHeading(item)}</p>
          {item.content.type === "scripture" && <p className="op-presentation__text">{item.content.text}</p>}
          <div className="op-presentation__actions">
            <button
              type="button"
              className="op-button--primary"
              disabled={!!activeDisplayItem || isBusy(`display-${item.id}`)}
              title={activeDisplayItem ? "Stop the currently active item before displaying another" : undefined}
              onClick={() => onDisplay(item.id)}
            >
              Display
            </button>
            <button type="button" disabled={isBusy(`cancel-${item.id}`)} onClick={() => onCancel(item.id)}>
              Cancel
            </button>
          </div>
        </div>
      ))}

      {approvedSuggestions.map((s) => (
        <div key={s.id} className="op-presentation__item">
          <div className="op-presentation__meta">
            <span className="op-badge op-badge--neutral">Approved</span>
          </div>
          <p className="op-presentation__scripture">{suggestionHeading(s)}</p>
          <div className="op-presentation__actions">
            <button type="button" disabled={isBusy(`preview-${s.id}`)} onClick={() => onPreviewApproved(s.id)}>
              Preview
            </button>
            <button
              type="button"
              className="op-button--primary"
              disabled={isBusy(`prepare-${s.id}`)}
              onClick={() => onPrepare(s.id)}
            >
              Prepare
            </button>
          </div>
          {previews[s.id] && (
            <div className="live-brain__preview-pane">
              <p className="live-brain__label">Preview &mdash; {previews[s.id].slide.template}</p>
              <p className="live-brain__preview-heading">
                <strong>{previews[s.id].slide.heading}</strong>
              </p>
              {previews[s.id].slide.bodyLines.map((line, i) => (
                <p key={i}>{line}</p>
              ))}
            </div>
          )}
        </div>
      ))}

      {nothingToShow && (
        <p className="op-presentation__empty">
          Nothing prepared yet. Approve a detected item above, then Prepare it here to get it ready to present.
        </p>
      )}

      <div className="op-presentation__screens" style={{ marginTop: "1rem" }}>
        {screens.map((s) => (
          <div key={s.screen} className="op-presentation__screen-row">
            <span className={`op-badge ${s.windowOpen ? "op-badge--good" : "op-badge--neutral"}`}>
              {s.label}: {s.windowOpen ? "Open" : "Closed"}
            </span>
            {!s.windowOpen ? (
              <button
                type="button"
                disabled={isBusy(`open-screen-${s.screen}`)}
                onClick={() => onOpenScreen(s.screen)}
              >
                Open
              </button>
            ) : (
              <button
                type="button"
                disabled={isBusy(`close-screen-${s.screen}`)}
                onClick={() => onCloseScreen(s.screen)}
              >
                Close
              </button>
            )}
          </div>
        ))}
      </div>
    </section>
  );
}
