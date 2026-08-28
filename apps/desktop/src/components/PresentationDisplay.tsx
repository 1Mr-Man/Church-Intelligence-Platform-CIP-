/**
 * The presentation display window's own React component - a passive
 * renderer only. Rendered by `main.tsx` instead of `App` when the current
 * webview's own label is `"display"` (see `main.tsx`'s docs). Loads the
 * exact same frontend bundle as the operator's main window.
 *
 * Phase 3.8.2: in addition to listening for the two events the backend
 * emits (`onPresentationStarted`/`onPresentationStopped`), this component
 * also pulls current state once on mount via `getPresentationDisplayState`
 * - the same command the operator's own window already uses to sync on
 * mount. This closes a real race: `display_presentation` opens this
 * window and emits `PRESENTATION_STARTED` immediately afterward in Rust,
 * but the new window's JavaScript (this component, its `useEffect`, its
 * event subscription) loads and runs asynchronously - if the event fires
 * before this component has subscribed, it was previously lost forever
 * and the display stayed blank for the rest of the session. Pulling once
 * on mount means the display always reflects the true current state
 * regardless of that ordering, exactly like the operator's own window.
 *
 * No operator controls, no navigation, no debug output - just the current
 * `RenderedSlide`, or a blank/inactive state when nothing is active. See
 * `docs/presentation.md`'s "Local display architecture" section.
 */
import { useEffect, useState } from "react";
import type { PresentationDisplayPayload } from "../domain";
import * as commands from "../lib/commands";
import * as liveEvents from "../lib/liveEvents";
import { resolveHydratedPayload } from "./presentationDisplayHydration";
import "./PresentationDisplay.css";

export function PresentationDisplay() {
  const [payload, setPayload] = useState<PresentationDisplayPayload | null>(null);

  useEffect(() => {
    let cancelled = false;

    commands
      .getPresentationDisplayState()
      .then((state) => {
        if (cancelled) return;
        const hydrated = resolveHydratedPayload(state);
        if (hydrated) setPayload(hydrated);
      })
      .catch(() => {
        // Best-effort hydration only - the event listeners below remain
        // the primary, live source of truth once subscribed.
      });

    const unlistenPromises = [
      liveEvents.onPresentationStarted((p) => {
        if (!cancelled) setPayload(p);
      }),
      liveEvents.onPresentationStopped(() => {
        if (!cancelled) setPayload(null);
      }),
    ];
    return () => {
      cancelled = true;
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, []);

  if (!payload) {
    return <div className="presentation-display presentation-display--blank" aria-label="No active presentation" />;
  }

  const { slide } = payload;
  return (
    <div className="presentation-display">
      {slide.heading && <h1 className="presentation-display__heading">{slide.heading}</h1>}
      <div className="presentation-display__body">
        {slide.bodyLines.map((line, i) => (
          <p key={i}>{line}</p>
        ))}
      </div>
      {slide.footer && <p className="presentation-display__footer">{slide.footer}</p>}
    </div>
  );
}
