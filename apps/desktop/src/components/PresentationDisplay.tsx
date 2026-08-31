/**
 * A presentation display screen's own React component - a passive
 * renderer only. Rendered by `main.tsx` instead of `App` when the current
 * webview's own label maps to one of the three display roles (see
 * `main.tsx`'s docs). Loads the exact same frontend bundle as the
 * operator's main window.
 *
 * Phase 3.8.2: in addition to listening for the two events the backend
 * emits (`onPresentationStarted`/`onPresentationStopped`), this component
 * also pulls current state once on mount via `getPresentationDisplayState`
 * - the same command the operator's own window already uses to sync on
 * mount. This closes a real race: `display_presentation` opens the Stage
 * window and emits `PRESENTATION_STARTED` immediately afterward in Rust,
 * but the new window's JavaScript (this component, its `useEffect`, its
 * event subscription) loads and runs asynchronously - if the event fires
 * before this component has subscribed, it was previously lost forever
 * and the display stayed blank for the rest of the session. Pulling once
 * on mount means the display always reflects the true current state
 * regardless of that ordering, exactly like the operator's own window.
 *
 * Phase 3.10: this same component now backs all three display roles.
 * `PRESENTATION_STARTED`/`PRESENTATION_STOPPED` are broadcast to every
 * open webview (not targeted at one window), so no per-screen event
 * plumbing was needed - only which extra, already-present fields to
 * render differs by `role`. Stage/Lobby render identically (a Lobby
 * screen mirrors Stage exactly, e.g. for an overflow room); Confidence
 * additionally shows operator-only metadata already carried in the same
 * payload (`item.template`, whether `item.sourceSuggestionId` is set,
 * `item.status`) - no new backend query, no fabricated data.
 *
 * No operator controls, no navigation, no debug output beyond the
 * Confidence role's own metadata - just the current `RenderedSlide`, or a
 * blank/inactive state when nothing is active. See `docs/presentation.md`'s
 * "Local display architecture" section and
 * `docs/phase-3-10-multi-screen-audit.md`.
 *
 * Phase 3.10.3: a screen can be `held` (see `PresentationCard`'s per-screen
 * route toggle), in which case its window simply stops receiving further
 * `PRESENTATION_STARTED`/`PRESENTATION_STOPPED` events - nothing to
 * change here, since the backend controls delivery, not this component.
 * When switched back to `live`, this component receives a targeted
 * `PRESENTATION_SCREEN_SYNCED` event and re-runs the exact same hydration
 * pull it already does on mount - no second content-delivery path.
 */
import { useEffect, useState } from "react";
import type { PresentationDisplayPayload, PresentationScreen } from "../domain";
import * as commands from "../lib/commands";
import * as liveEvents from "../lib/liveEvents";
import { logCheckpoint } from "./presentationDiagnostics";
import { resolveHydratedPayload } from "./presentationDisplayHydration";
import "./PresentationDisplay.css";

export interface PresentationDisplayProps {
  role: PresentationScreen;
}

export function PresentationDisplay({ role }: PresentationDisplayProps) {
  const [payload, setPayload] = useState<PresentationDisplayPayload | null>(null);

  useEffect(() => {
    let cancelled = false;
    logCheckpoint("mounted", "PresentationDisplay component mounted (checkpoint 3)");
    logCheckpoint("effect-ran", "useEffect body executing (checkpoint 4)");

    const hydrate = (reason: "mount" | "route-synced") => {
      logCheckpoint("hydration-call", `calling getPresentationDisplayState (${reason}) (checkpoint 5)`);
      commands
        .getPresentationDisplayState()
        .then((state) => {
          const openScreens = state.screens.filter((s) => s.windowOpen).map((s) => s.screen);
          logCheckpoint(
            "hydration-result",
            `openScreens=${openScreens.join(",")} activeItem=${state.activeItem !== null} activeSlide=${state.activeSlide !== null} (checkpoint 6)`,
          );
          if (cancelled) return;
          const hydrated = resolveHydratedPayload(state);
          setPayload(hydrated);
          if (hydrated) {
            logCheckpoint(
              "payload-applied",
              `source=hydration heading=${hydrated.slide.heading} bodyLines=${hydrated.slide.bodyLines.length} footer=${hydrated.slide.footer ?? "null"} (checkpoints 9-12)`,
            );
          }
        })
        .catch((e) => {
          // Best-effort hydration only - the event listeners below remain
          // the primary, live source of truth once subscribed.
          logCheckpoint("hydration-error", String(e));
        });
    };

    hydrate("mount");

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
      // Phase 3.10.3: this screen was just switched back to `live` after
      // being `held` - re-sync via the same hydration pull mount uses,
      // rather than expecting this event to carry content itself.
      liveEvents.onPresentationScreenSynced(() => {
        logCheckpoint("route-synced-received", "PresentationScreenSynced event received");
        if (!cancelled) hydrate("route-synced");
      }),
    ];
    return () => {
      cancelled = true;
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, []);

  if (!payload) {
    return (
      <div
        className={`presentation-display presentation-display--blank presentation-display--${role}`}
        aria-label="No active presentation"
      />
    );
  }

  const { slide, item } = payload;
  return (
    <div className={`presentation-display presentation-display--${role}`}>
      {slide.heading && <h1 className="presentation-display__heading">{slide.heading}</h1>}
      <div className="presentation-display__body">
        {slide.bodyLines.map((line, i) => (
          <p key={i}>{line}</p>
        ))}
      </div>
      {slide.footer && <p className="presentation-display__footer">{slide.footer}</p>}
      {role === "confidence" && (
        <div className="presentation-display__monitor-meta" aria-label="Operator metadata">
          <p>Source: {item.sourceSuggestionId ? "Auto-detected" : "Manual"}</p>
          {item.template && <p>Template: {item.template}</p>}
          <p>Status: {item.status}</p>
        </div>
      )}
    </div>
  );
}
