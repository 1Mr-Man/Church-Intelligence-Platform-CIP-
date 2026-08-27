/**
 * Phase 3.4: a first-run/troubleshooting view of `get_pilot_diagnostics`
 * that an operator can read without developer tools - the "operator
 * should be able to understand whether the microphone/Whisper/second
 * display/database are ready" requirement. Purely a display of facts
 * this command already computes; adds no new diagnosis logic of its own
 * and never inflates a software-only reading into a hardware-verified
 * claim (see `docs/phase-3-4-windows-pilot.md`'s Environment A/B/C
 * distinction - what this panel shows is whatever the current process
 * can actually observe, nothing more).
 */
import { useCallback, useEffect, useState } from "react";
import { getPilotDiagnostics } from "../../lib/commands";
import type { PilotDiagnostics } from "../../config/appConfig";

function whisperModelSummary(diagnostics: PilotDiagnostics): string {
  const model = diagnostics.whisperModel;
  switch (model.status) {
    case "missing":
      return `Not found (expected at ${model.expectedPath})`;
    case "unreadable":
      return `Present but unreadable at ${model.path} (${model.reason})`;
    case "present":
      return `Present at ${model.path} (${model.sizeBytes.toLocaleString()} bytes)`;
    default:
      return "Unknown";
  }
}

export function PilotDiagnosticsPanel() {
  const [diagnostics, setDiagnostics] = useState<PilotDiagnostics | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(() => {
    setLoading(true);
    setError(null);
    getPilotDiagnostics()
      .then(setDiagnostics)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return (
    <details className="live-brain__panel workspace-diagnostics">
      <summary>
        <h2 style={{ display: "inline" }}>System Diagnostics</h2>
      </summary>
      {error && (
        <p className="live-brain__error" role="alert">
          Could not read diagnostics: {error}
        </p>
      )}
      {diagnostics && (
        <dl className="workspace-header__grid">
          <div>
            <dt>Machine</dt>
            <dd>
              {diagnostics.machine.os} / {diagnostics.machine.arch} &mdash; CIP {diagnostics.machine.cipVersion} (
              {diagnostics.machine.buildCommit})
            </dd>
          </div>
          <div>
            <dt>Database</dt>
            <dd>
              {diagnostics.database.readable && diagnostics.database.writable
                ? "Healthy (readable and writable)"
                : `Problem detected (readable: ${diagnostics.database.readable}, writable: ${diagnostics.database.writable})`}
            </dd>
          </div>
          <div>
            <dt>Bible dataset</dt>
            <dd>{diagnostics.bible ? `${diagnostics.bible.name} (${diagnostics.bible.status})` : "NOT INSTALLED"}</dd>
          </div>
          <div>
            <dt>Microphone</dt>
            <dd>
              {diagnostics.audioDevices.length} device(s) detected
              {diagnostics.audio.selectedDevice ? ` — selected: ${diagnostics.audio.selectedDevice}` : ""}
              {diagnostics.audioDevices.length === 0 ? " (manual transcript entry still works)" : ""}
            </dd>
          </div>
          <div>
            <dt>Whisper model</dt>
            <dd>{whisperModelSummary(diagnostics)}</dd>
          </div>
          <div>
            <dt>Displays</dt>
            <dd>
              {diagnostics.displays.length} detected
              {diagnostics.displays.length < 2 ? " (no second display/projector detected - single-display/manual-preview mode)" : ""}
            </dd>
          </div>
        </dl>
      )}
      <button type="button" onClick={refresh} disabled={loading}>
        {loading ? "Refreshing…" : "Refresh diagnostics"}
      </button>
    </details>
  );
}
