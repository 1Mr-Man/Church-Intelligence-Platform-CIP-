/**
 * Frontend mirror of `apps/desktop/src-tauri/src/config.rs`'s `AppConfig`.
 * The frontend never resolves these paths itself - it only reads what the
 * backend already resolved, via the `get_app_config` command.
 */

export type AppEnvironment = "development" | "test" | "production";

export interface AppConfig {
  environment: AppEnvironment;
  dataDir: string;
  databasePath: string;
  modelDir: string;
  /** Phase 3.0: exact file path the speech engine looks for a local
   * Whisper model at - overridable via CIP_WHISPER_MODEL_PATH. Surfaced in
   * the UI so "speech unavailable" names where to put a model, not just
   * that one is missing. */
  whisperModelPath: string;
  logDir: string;
}

// --- Phase 3.2 pilot hardware diagnostics -----------------------------------
//
// Frontend mirror of `apps/desktop/src-tauri/src/commands.rs`'s
// `WhisperModelDiagnostic`/`DisplayDiagnostic`/`PilotDiagnostics`, via the
// `get_pilot_diagnostics` command - a single, honest place to see exactly
// what CIP can currently detect about the hardware/model it depends on.

export type WhisperModelDiagnostic =
  | { status: "missing"; expectedPath: string }
  | { status: "unreadable"; path: string; reason: string }
  | { status: "present"; path: string; sizeBytes: number };

export interface DisplayDiagnostic {
  name: string | null;
  widthPx: number;
  heightPx: number;
  isPrimary: boolean;
}

export interface PilotDiagnostics {
  whisperModel: WhisperModelDiagnostic;
  audioDevices: Array<{ id: string; name: string; isDefault: boolean }>;
  audio: {
    isCapturing: boolean;
    isPaused: boolean;
    sampleRateHz: number;
    inputLevel: number | null;
    streamError: string | null;
  };
  /**
   * Every display this process can detect (via Tauri's own monitor API).
   * `length >= 2` is necessary but not sufficient evidence that a real
   * second display/projector is connected - it may equally be one
   * physical monitor Tauri enumerates in an unusual way, or a virtual
   * display (Xvfb). Never treat this list alone as VERIFIED physical
   * projector readiness - see `docs/phase-3-2-hardware-pilot.md`.
   */
  displays: DisplayDiagnostic[];
}

/** Frontend mirror of `commands.rs`'s `BackupReport`, via `backup_database`. */
export interface BackupReport {
  backupPath: string;
  sizeBytes: number;
}
