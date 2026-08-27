/**
 * The presentation display window's own React component - a passive
 * renderer only. Rendered by `main.tsx` instead of `App` when the current
 * webview's own label is `"display"` (see `main.tsx`'s docs). Loads the
 * exact same frontend bundle as the operator's main window; nothing here
 * ever calls a Tauri command, only listens for the two events the backend
 * already emits (`onPresentationStarted`/`onPresentationStopped`).
 *
 * No operator controls, no navigation, no debug output - just the current
 * `RenderedSlide`, or a blank/inactive state when nothing is active. See
 * `docs/presentation.md`'s "Local display architecture" section.
 */
import { useEffect, useState } from "react";
import type { PresentationDisplayPayload } from "../domain";
import * as liveEvents from "../lib/liveEvents";
import "./PresentationDisplay.css";

export function PresentationDisplay() {
  const [payload, setPayload] = useState<PresentationDisplayPayload | null>(null);

  useEffect(() => {
    let cancelled = false;
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
