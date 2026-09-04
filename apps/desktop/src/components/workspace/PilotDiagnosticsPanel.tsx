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
import { open } from "@tauri-apps/plugin-dialog";
import { getPilotDiagnostics, installWhisperModel, installWhisperQualityModel } from "../../lib/commands";
import type { PilotDiagnostics, WhisperModelDiagnostic } from "../../config/appConfig";

function formatWhisperModelDiagnostic(model: WhisperModelDiagnostic): string {
  switch (model.status) {
    case "missing":
      return `Not found (expected at ${model.expectedPath})`;
    case "unreadable":
      return `Present but unreadable at ${model.path} (${model.reason})`;
    case "present":
      return `Present at ${model.path} (${model.sizeBytes.toLocaleString()} bytes - ${model.sizeTierHint})`;
    default:
      return "Unknown";
  }
}

function whisperModelSummary(diagnostics: PilotDiagnostics): string {
  return formatWhisperModelDiagnostic(diagnostics.whisperModel);
}

function formatMs(ms: number | null): string {
  return ms == null ? "n/a" : `${ms}ms`;
}

const OVERLOAD_STATE_LABELS: Record<PilotDiagnostics["speech"]["overloadState"], string> = {
  normal: "Normal",
  busy: "Busy",
  falling_behind: "Falling behind",
  overloaded: "Overloaded - discarding stale audio",
};

function overloadStateLabel(state: PilotDiagnostics["speech"]["overloadState"]): string {
  return OVERLOAD_STATE_LABELS[state];
}

/** Phase 24.3.2: mirrors `OVERLOAD_STATE_LABELS` above, for the quality
 * tier's own streak-based backlog state (`classify_quality_backlog`). */
const QUALITY_BACKLOG_STATE_LABELS: Record<PilotDiagnostics["speechQuality"]["backlogState"], string> = {
  normal: "Normal",
  busy: "Busy (one recent drop - likely a brief spike)",
  falling_behind: "Falling behind (2 drops in a row)",
  overloaded: "Overloaded - this model may be too slow for this hardware",
};

function qualityBacklogStateLabel(state: PilotDiagnostics["speechQuality"]["backlogState"]): string {
  return QUALITY_BACKLOG_STATE_LABELS[state];
}

export function PilotDiagnosticsPanel() {
  const [diagnostics, setDiagnostics] = useState<PilotDiagnostics | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installMessage, setInstallMessage] = useState<string | null>(null);
  const [installingQuality, setInstallingQuality] = useState(false);
  const [installQualityMessage, setInstallQualityMessage] = useState<string | null>(null);

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

  const selectModelFile = useCallback(() => {
    setInstallMessage(null);
    open({
      title: "Select a Whisper model file",
      filters: [{ name: "Whisper model", extensions: ["bin", "gguf"] }],
      multiple: false,
      directory: false,
    })
      .then((selected) => {
        if (!selected || Array.isArray(selected)) {
          return;
        }
        setInstalling(true);
        return installWhisperModel(selected)
          .then((result) => {
            const detail =
              result.status === "present"
                ? `${result.sizeBytes.toLocaleString()} bytes - ${result.sizeTierHint}`
                : result.status;
            setInstallMessage(`Installed (${detail}). Restart CIP for it to take effect.`);
            refresh();
          })
          .catch((e) => setInstallMessage(`Install failed: ${String(e)}`))
          .finally(() => setInstalling(false));
      })
      .catch((e) => setInstallMessage(`Could not open file picker: ${String(e)}`));
  }, [refresh]);

  // Phase 24.3 (true dual-tier Whisper): identical flow to
  // `selectModelFile` above, targeting the second, optional quality-tier
  // model instead.
  const selectQualityModelFile = useCallback(() => {
    setInstallQualityMessage(null);
    open({
      title: "Select a quality-tier Whisper model file",
      filters: [{ name: "Whisper model", extensions: ["bin", "gguf"] }],
      multiple: false,
      directory: false,
    })
      .then((selected) => {
        if (!selected || Array.isArray(selected)) {
          return;
        }
        setInstallingQuality(true);
        return installWhisperQualityModel(selected)
          .then((result) => {
            const detail =
              result.status === "present"
                ? `${result.sizeBytes.toLocaleString()} bytes - ${result.sizeTierHint}`
                : result.status;
            setInstallQualityMessage(`Installed (${detail}). Restart CIP for it to take effect.`);
            refresh();
          })
          .catch((e) => setInstallQualityMessage(`Install failed: ${String(e)}`))
          .finally(() => setInstallingQuality(false));
      })
      .catch((e) => setInstallQualityMessage(`Could not open file picker: ${String(e)}`));
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
              {diagnostics.machine.buildCommit}
              {diagnostics.machine.buildDirty ? " + uncommitted changes" : ""})
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
              {/* Phase 6.4: was already present in every diagnostics poll
                  (AudioEngineStatus::streamError) but never rendered here -
                  the one place in this panel real speech error text had no
                  audio equivalent. */}
              {diagnostics.audio.streamError && <div>Last error: {diagnostics.audio.streamError}</div>}
            </dd>
          </div>
          <div>
            <dt>Whisper model</dt>
            <dd>
              {whisperModelSummary(diagnostics)}
              {diagnostics.speech.featureCompiled && (
                <div>
                  <button type="button" onClick={selectModelFile} disabled={installing}>
                    {installing ? "Installing…" : "Select Existing Model File…"}
                  </button>
                  {installMessage && <span> {installMessage}</span>}
                </div>
              )}
            </dd>
          </div>
          <div>
            <dt>Whisper diagnostics</dt>
            <dd>
              <div>Feature compiled: {diagnostics.speech.featureCompiled ? "YES" : "NO"}</div>
              <div>
                Model loaded:{" "}
                {diagnostics.speech.modelLoadAttempted
                  ? diagnostics.speech.modelLoaded
                    ? "YES"
                    : `NO (${diagnostics.speech.modelLoadError ?? "unknown reason"})`
                  : "not attempted"}
              </div>
              <div>Speech engine ready: {diagnostics.speech.engineReady ? "YES" : "NO"}</div>
              <div>
                Audio chunks received: {diagnostics.speech.chunksReceived}
                {diagnostics.speech.lastChunkSampleRateHz != null && (
                  <>
                    {" "}
                    (last: {diagnostics.speech.lastChunkSampleCount} samples @{" "}
                    {diagnostics.speech.lastChunkSampleRateHz} Hz
                    {diagnostics.speech.lastResampledSampleCount != null
                      ? `, resampled to ${diagnostics.speech.lastResampledSampleCount} samples`
                      : ""}
                    )
                  </>
                )}
              </div>
              <div>
                Inferences: {diagnostics.speech.inferencesSucceeded} succeeded /{" "}
                {diagnostics.speech.inferencesAttempted} attempted
                {diagnostics.speech.chunksSkippedEngineNotReady > 0 && (
                  <> ({diagnostics.speech.chunksSkippedEngineNotReady} more chunks skipped - engine not ready)</>
                )}
                {diagnostics.speech.silentWindowsSkipped > 0 && (
                  <> ({diagnostics.speech.silentWindowsSkipped} windows skipped - classified as silence)</>
                )}
                {diagnostics.speech.nonSpeechPlaceholdersSkipped > 0 && (
                  <>
                    {" "}
                    ({diagnostics.speech.nonSpeechPlaceholdersSkipped} non-speech captions discarded - e.g.
                    "[BLANK_AUDIO]", not real speech)
                  </>
                )}
                {diagnostics.speech.vadEarlyFlushes > 0 && (
                  <> ({diagnostics.speech.vadEarlyFlushes} windows flushed early - a natural pause was detected)</>
                )}
              </div>
              {diagnostics.speech.lastError && <div>Last error: {diagnostics.speech.lastError}</div>}
            </dd>
          </div>
          <div>
            <dt>Quality-tier Whisper model (Phase 24.3, optional)</dt>
            <dd>
              {formatWhisperModelDiagnostic(diagnostics.whisperQualityModel)}
              {diagnostics.speechQuality.featureCompiled && (
                <div>
                  <button type="button" onClick={selectQualityModelFile} disabled={installingQuality}>
                    {installingQuality ? "Installing…" : "Select Quality-Tier Model File…"}
                  </button>
                  {installQualityMessage && <span> {installQualityMessage}</span>}
                </div>
              )}
              <div>
                A second, independent Whisper model run only to re-transcribe speech the
                fast tier already showed live, for a slower but more accurate second look. Entirely
                optional - never installing one leaves the fast tier working exactly as it always has.
              </div>
              {diagnostics.whisperQualityModel.status !== "missing" && (
                <>
                  <div>Quality engine ready: {diagnostics.speechQuality.engineReady ? "YES" : "NO"}</div>
                  <div>
                    Jobs: {diagnostics.speechQuality.jobsCompleted} completed / {diagnostics.speechQuality.jobsSubmitted}{" "}
                    submitted
                    {diagnostics.speechQuality.jobsDroppedBacklog > 0 && (
                      <> ({diagnostics.speechQuality.jobsDroppedBacklog} dropped total)</>
                    )}
                  </div>
                  <div>
                    Backlog status: <strong>{qualityBacklogStateLabel(diagnostics.speechQuality.backlogState)}</strong>
                    {diagnostics.speechQuality.consecutiveJobsDropped > 0 && (
                      <> ({diagnostics.speechQuality.consecutiveJobsDropped} dropped in a row right now)</>
                    )}
                  </div>
                  {diagnostics.speechQuality.lastError && <div>Last error: {diagnostics.speechQuality.lastError}</div>}
                </>
              )}
            </dd>
          </div>
          <div>
            <dt>Speech pipeline health</dt>
            <dd>
              <div>
                Status: <strong>{overloadStateLabel(diagnostics.speech.overloadState)}</strong>
              </div>
              <div>
                Queued audio: {diagnostics.speech.queuePendingMs}ms (high water: {diagnostics.speech.queueHighWaterMs}ms this session)
              </div>
              {diagnostics.speech.overloadEvents > 0 && (
                <div>
                  Overload events: {diagnostics.speech.overloadEvents} (total {diagnostics.speech.audioMsDroppedOverload}ms of
                  audio discarded to catch back up to real time)
                </div>
              )}
              <div>
                Inference duration: last {formatMs(diagnostics.speech.lastInferenceDurationMs)}, avg{" "}
                {formatMs(diagnostics.speech.avgInferenceDurationMs)}, max {formatMs(diagnostics.speech.maxInferenceDurationMs)}
              </div>
              <div>Transcript pipeline (DB + Bible detection): last {formatMs(diagnostics.speech.lastTranscriptPipelineDurationMs)}</div>
            </dd>
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
