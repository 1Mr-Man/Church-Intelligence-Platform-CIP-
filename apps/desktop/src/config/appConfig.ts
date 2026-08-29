/**
 * Frontend mirror of `apps/desktop/src-tauri/src/config.rs`'s `AppConfig`.
 * The frontend never resolves these paths itself - it only reads what the
 * backend already resolved, via the `get_app_config` command.
 */

import type { ContentMetadata } from "../domain/content";

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
  /** Phase 3.4: top-left position in the OS's virtual screen coordinate space. */
  positionX: number;
  positionY: number;
  scaleFactor: number;
  isPrimary: boolean;
}

/** Which machine/build produced a diagnostic report (Phase 3.3). */
export interface MachineDiagnostic {
  os: string;
  arch: string;
  cipVersion: string;
  buildCommit: string;
  /** `true` means built from `buildCommit` plus uncommitted changes - not `buildCommit` exactly. */
  buildDirty: boolean;
}

/** Is the database file actually readable/writable right now (Phase 3.3). */
export interface DatabaseDiagnostic {
  path: string;
  readable: boolean;
  writable: boolean;
}

/**
 * Phase 3.8.6: what the running process actually observed about the
 * speech pipeline - distinct from `WhisperModelDiagnostic` (a filesystem
 * check) in that `modelLoaded` is only `true` after the real engine
 * parsed the file and initialized a whisper.cpp context. Mirrors
 * `commands.rs`'s `SpeechRuntimeDiagnostics` one-to-one.
 */
export interface SpeechRuntimeDiagnostics {
  featureCompiled: boolean;
  modelLoadAttempted: boolean;
  modelLoaded: boolean;
  modelLoadError: string | null;
  engineReady: boolean;
  chunksReceived: number;
  lastChunkSampleRateHz: number | null;
  lastChunkSampleCount: number | null;
  lastResampledSampleCount: number | null;
  /** Chunks skipped because `engineReady` was `false` - never counted in `inferencesAttempted`. */
  chunksSkippedEngineNotReady: number;
  /**
   * Phase 3.8.7.3: only counts calls where Whisper actually ran inference,
   * not every chunk fed to a ready engine (most chunks just buffer).
   */
  inferencesAttempted: number;
  inferencesSucceeded: number;
  lastError: string | null;
  /** Current estimated wall-clock duration (ms) of audio queued for the speech worker. */
  queuePendingMs: number;
  /** Highest `queuePendingMs` observed since the current listening session started. */
  queueHighWaterMs: number;
  /** How many times the backlog crossed the overload threshold and queued/buffered audio was discarded. */
  overloadEvents: number;
  /** Total estimated milliseconds of audio discarded across all overload events. */
  audioMsDroppedOverload: number;
  lastInferenceDurationMs: number | null;
  maxInferenceDurationMs: number | null;
  avgInferenceDurationMs: number | null;
  lastTranscriptPipelineDurationMs: number | null;
  /** Derived from `queuePendingMs` against fixed thresholds - see `commands.rs::classify_overload`. */
  overloadState: "normal" | "busy" | "falling_behind" | "overloaded";
}

export interface PilotDiagnostics {
  machine: MachineDiagnostic;
  whisperModel: WhisperModelDiagnostic;
  speech: SpeechRuntimeDiagnostics;
  audioDevices: Array<{ id: string; name: string; isDefault: boolean }>;
  audio: {
    isCapturing: boolean;
    isPaused: boolean;
    sampleRateHz: number;
    inputLevel: number | null;
    streamError: string | null;
    selectedDevice: string | null;
    channels: number | null;
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
  /** `null` when the BSB dataset is not registered - never fabricated. */
  bible: ContentMetadata | null;
  database: DatabaseDiagnostic;
}

/** Frontend mirror of `commands.rs`'s `BackupReport`, via `backup_database`. */
export interface BackupReport {
  backupPath: string;
  sizeBytes: number;
}
