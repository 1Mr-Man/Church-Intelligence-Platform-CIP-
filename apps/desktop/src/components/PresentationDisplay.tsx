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

/** Phase 3.8.3 TEMPORARY DIAGNOSTIC: best-effort, never awaited, never
 * throws to the caller - routes a checkpoint into the log file via
 * `log_display_diagnostic`. Silently does nothing if the call itself
 * fails (e.g. outside the Tauri runtime), since diagnostics must never be
 * able to affect the actual display. */
function logCheckpoint(stage: string, detail: string) {
  commands.logDisplayDiagnostic(stage, detail).catch(() => {});
}

export function PresentationDisplay() {
  const [payload, setPayload] = useState<PresentationDisplayPayload | null>(null);

  useEffect(() => {
    let cancelled = false;
    logCheckpoint("mounted", "PresentationDisplay component mounted (checkpoint 3)");
    logCheckpoint("effect-ran", "useEffect body executing (checkpoint 4)");

    logCheckpoint("hydration-call", "calling getPresentationDisplayState (checkpoint 5)");
    commands
      .getPresentationDisplayState()
      .then((state) => {
        logCheckpoint(
          "hydration-result",
          `windowOpen=${state.windowOpen} activeItem=${state.activeItem !== null} activeSlide=${state.activeSlide !== null} (checkpoint 6)`,
        );
        if (cancelled) return;
        const hydrated = resolveHydratedPayload(state);
        if (hydrated) {
          logCheckpoint(
            "payload-applied",
            `source=hydration heading=${hydrated.slide.heading} bodyLines=${hydrated.slide.bodyLines.length} footer=${hydrated.slide.footer ?? "null"} (checkpoints 9-12)`,
          );
          setPayload(hydrated);
        }
      })
      .catch((e) => {
        // Best-effort hydration only - the event listeners below remain
        // the primary, live source of truth once subscribed.
        logCheckpoint("hydration-error", String(e));
      });

    const unlistenPromises = [
      liveEvents.onPresentationStarted((p) => {
        logCheckpoint("presentation-started-received", "PresentationStarted event received (checkpoint 7)");
        if (!cancelled) {
          logCheckpoint(
            "payload-applied",
            `source=event heading=${p.slide.heading} bodyLines=${p.slide.bodyLines.length} footer=${p.slide.footer ?? "null"} (checkpoints 9-12)`,
          );
          setPayload(p);
        }
      }),
      liveEvents.onPresentationStopped(() => {
        logCheckpoint("presentation-stopped-received", "PresentationStopped event received (checkpoint 8)");
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
