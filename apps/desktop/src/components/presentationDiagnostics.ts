/**
 * TEMPORARY DIAGNOSTIC (Phase 3.8.3, extended Phase 3.8.4): routes a
 * lifecycle checkpoint from the display window's own frontend into the
 * app's log output via `log_display_diagnostic` - the only way to
 * observe what a secondary webview's own JavaScript sees, since this app
 * has no devtools/logging plugin. Best-effort, never awaited, never
 * throws to the caller - diagnostics must never be able to affect the
 * actual display. Shared by `main.tsx` (Root() branch selection, frontend
 * exceptions) and `PresentationDisplay.tsx` (mount/hydration/event
 * checkpoints) so there is exactly one implementation of this pattern.
 */
import * as commands from "../lib/commands";

export function logCheckpoint(stage: string, detail: string) {
  commands.logDisplayDiagnostic(stage, detail).catch(() => {});
}
