/**
 * Phase 11 (Local Congregant Companion View): the operator control panel
 * for the LAN-only, read-only companion server that mirrors Stage to a
 * congregant's phone browser - see `docs/phase-11-audit.md` and
 * `docs/congregant-companion.md`. Off by default; enabling/disabling
 * requires a logged-in Admin (the backend enforces this - this panel
 * does not hide itself for a non-Admin, matching
 * `ProductionIntegrationPanel`'s own precedent of surfacing the error
 * from the command rather than guessing the operator's role client-side).
 */
import { useCallback, useEffect, useState } from "react";
import {
  disableCongregantCompanion,
  enableCongregantCompanion,
  getCongregantCompanionStatus,
} from "../../lib/commands";
import type { CompanionStatus } from "../../domain";

export function CongregantCompanionPanel() {
  const [status, setStatus] = useState<CompanionStatus | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const refreshStatus = useCallback(() => {
    getCongregantCompanionStatus()
      .then(setStatus)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    refreshStatus();
  }, [refreshStatus]);

  const handleEnable = useCallback(() => {
    setBusy("enable");
    setMessage(null);
    enableCongregantCompanion()
      .then((s) => {
        setStatus(s);
        setMessage(
          s.urls.length > 0
            ? "Companion server started. Share an address below with the congregation."
            : "Companion server started, but no LAN address could be auto-detected - check this computer's network connection.",
        );
      })
      .catch((e) => setMessage(`Could not start the companion server: ${String(e)}`))
      .finally(() => setBusy(null));
  }, []);

  const handleDisable = useCallback(() => {
    setBusy("disable");
    setMessage(null);
    disableCongregantCompanion()
      .then((s) => {
        setStatus(s);
        setMessage("Companion server stopped.");
      })
      .catch((e) => setMessage(`Could not stop the companion server: ${String(e)}`))
      .finally(() => setBusy(null));
  }, []);

  return (
    <details className="live-brain__panel workspace-diagnostics">
      <summary>
        <h2 style={{ display: "inline" }}>Congregant Companion View</h2>
      </summary>
      <p className="live-brain__hint">
        A read-only page a congregant's phone can open on the church wifi to follow along with
        whatever CIP currently displays on Stage - no app to install, nothing leaves the LAN.
        Personal notes on that page are saved only on the phone itself; CIP never receives them.
        Disabled by default.
      </p>

      <p className="live-brain__hint">
        Status: {status?.running ? "Running" : "Stopped"}
        {status?.running ? ` on port ${status.port}` : ""}
      </p>

      {status?.running && status.urls.length > 0 && (
        <ul>
          {status.urls.map((url) => (
            <li key={url}>{url}</li>
          ))}
        </ul>
      )}
      {status?.running && status.urls.length === 0 && (
        <p className="live-brain__hint">
          Running, but no LAN address could be auto-detected - try{" "}
          {`http://<this computer's IP address>:${status.port}/`}.
        </p>
      )}

      <div className="live-brain__form-row">
        <button type="button" onClick={handleEnable} disabled={busy !== null || status?.running === true}>
          {busy === "enable" ? "Starting…" : "Start Companion Server"}
        </button>
        <button
          type="button"
          onClick={handleDisable}
          disabled={busy !== null || status?.running !== true}
        >
          {busy === "disable" ? "Stopping…" : "Stop Companion Server"}
        </button>
        <button type="button" onClick={refreshStatus} disabled={busy !== null}>
          Refresh status
        </button>
      </div>
      {message && <p className="live-brain__hint">{message}</p>}
    </details>
  );
}
