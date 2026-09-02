/**
 * Phase 8 (Production Integration): the operator configuration UI for
 * pushing CIP's currently-displayed presentation text into an OBS text
 * source and/or a vMix title - see `docs/phase-8-audit.md`. Config is
 * in-memory/session-scoped and takes effect immediately on Save (no
 * restart required, unlike model-provisioning panels elsewhere in this
 * app) - the next `display_presentation`/`clear_presentation_display`
 * call is what actually pushes.
 */
import { useCallback, useEffect, useState } from "react";
import {
  getProductionIntegrationStatus,
  setProductionIntegrationConfig,
  testObsConnection,
  testVmixConnection,
} from "../../lib/commands";
import type { ProductionIntegrationStatus, PushOutcome } from "../../domain";

function outcomeLabel(outcome: PushOutcome | null): string {
  if (!outcome) return "No push attempted yet this session.";
  const when = new Date(outcome.at).toLocaleTimeString();
  return outcome.success
    ? `Last push OK at ${when}.`
    : `Last push FAILED at ${when}: ${outcome.errorText ?? "unknown error"}`;
}

export function ProductionIntegrationPanel() {
  const [obsEnabled, setObsEnabled] = useState(false);
  const [obsHost, setObsHost] = useState("127.0.0.1");
  const [obsPort, setObsPort] = useState("4455");
  const [obsPassword, setObsPassword] = useState("");
  const [obsSourceName, setObsSourceName] = useState("");

  const [vmixEnabled, setVmixEnabled] = useState(false);
  const [vmixHost, setVmixHost] = useState("127.0.0.1");
  const [vmixPort, setVmixPort] = useState("8088");
  const [vmixInput, setVmixInput] = useState("");
  const [vmixSelectedName, setVmixSelectedName] = useState("");

  const [status, setStatus] = useState<ProductionIntegrationStatus | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const refreshStatus = useCallback(() => {
    getProductionIntegrationStatus()
      .then(setStatus)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    refreshStatus();
  }, [refreshStatus]);

  const obsTarget = useCallback(
    () => ({
      host: obsHost,
      port: Number(obsPort) || 4455,
      password: obsPassword.length > 0 ? obsPassword : null,
      sourceName: obsSourceName,
    }),
    [obsHost, obsPort, obsPassword, obsSourceName],
  );

  const vmixTarget = useCallback(
    () => ({
      host: vmixHost,
      port: Number(vmixPort) || 8088,
      input: vmixInput,
      selectedName: vmixSelectedName.length > 0 ? vmixSelectedName : null,
    }),
    [vmixHost, vmixPort, vmixInput, vmixSelectedName],
  );

  const handleSave = useCallback(() => {
    setBusy("save");
    setMessage(null);
    setProductionIntegrationConfig({
      obs: obsEnabled ? obsTarget() : null,
      vmix: vmixEnabled ? vmixTarget() : null,
    })
      .then(() => setMessage("Saved. The next displayed item will push to any enabled target."))
      .catch((e) => setMessage(`Save failed: ${String(e)}`))
      .finally(() => setBusy(null));
  }, [obsEnabled, vmixEnabled, obsTarget, vmixTarget]);

  const handleTestObs = useCallback(() => {
    setBusy("test-obs");
    setMessage(null);
    testObsConnection(obsTarget())
      .then(() => setMessage("OBS test push succeeded - check the configured source."))
      .catch((e) => setMessage(`OBS test push failed: ${String(e)}`))
      .finally(() => setBusy(null));
  }, [obsTarget]);

  const handleTestVmix = useCallback(() => {
    setBusy("test-vmix");
    setMessage(null);
    testVmixConnection(vmixTarget())
      .then(() => setMessage("vMix test push succeeded - check the configured input."))
      .catch((e) => setMessage(`vMix test push failed: ${String(e)}`))
      .finally(() => setBusy(null));
  }, [vmixTarget]);

  return (
    <details className="live-brain__panel workspace-diagnostics">
      <summary>
        <h2 style={{ display: "inline" }}>Production Integration (OBS / vMix)</h2>
      </summary>
      <p className="live-brain__hint">
        Pushes the text of whatever CIP currently displays into an OBS text source and/or a vMix
        title - never switches scenes, never controls recording/streaming. Disabled by default.
      </p>

      <fieldset>
        <legend>
          <label>
            <input type="checkbox" checked={obsEnabled} onChange={(e) => setObsEnabled(e.target.checked)} />{" "}
            OBS (obs-websocket v5)
          </label>
        </legend>
        {obsEnabled && (
          <div className="live-brain__form-row">
            <input placeholder="Host" value={obsHost} onChange={(e) => setObsHost(e.target.value)} />
            <input placeholder="Port" value={obsPort} onChange={(e) => setObsPort(e.target.value)} />
            <input
              placeholder="Password (if set)"
              type="password"
              value={obsPassword}
              onChange={(e) => setObsPassword(e.target.value)}
            />
            <input
              placeholder="Text source name"
              value={obsSourceName}
              onChange={(e) => setObsSourceName(e.target.value)}
            />
            <button type="button" onClick={handleTestObs} disabled={busy !== null || obsSourceName.length === 0}>
              {busy === "test-obs" ? "Testing…" : "Test OBS Connection"}
            </button>
          </div>
        )}
        <p className="live-brain__hint">{outcomeLabel(status?.obsLastPush ?? null)}</p>
      </fieldset>

      <fieldset>
        <legend>
          <label>
            <input type="checkbox" checked={vmixEnabled} onChange={(e) => setVmixEnabled(e.target.checked)} />{" "}
            vMix
          </label>
        </legend>
        {vmixEnabled && (
          <div className="live-brain__form-row">
            <input placeholder="Host" value={vmixHost} onChange={(e) => setVmixHost(e.target.value)} />
            <input placeholder="Port" value={vmixPort} onChange={(e) => setVmixPort(e.target.value)} />
            <input
              placeholder="Input name or number"
              value={vmixInput}
              onChange={(e) => setVmixInput(e.target.value)}
            />
            <input
              placeholder="Selected text layer (optional)"
              value={vmixSelectedName}
              onChange={(e) => setVmixSelectedName(e.target.value)}
            />
            <button type="button" onClick={handleTestVmix} disabled={busy !== null || vmixInput.length === 0}>
              {busy === "test-vmix" ? "Testing…" : "Test vMix Connection"}
            </button>
          </div>
        )}
        <p className="live-brain__hint">{outcomeLabel(status?.vmixLastPush ?? null)}</p>
      </fieldset>

      <button type="button" onClick={handleSave} disabled={busy !== null}>
        {busy === "save" ? "Saving…" : "Save Production Integration Config"}
      </button>
      <button type="button" onClick={refreshStatus} disabled={busy !== null}>
        Refresh status
      </button>
      {message && <p className="live-brain__hint">{message}</p>}
    </details>
  );
}
