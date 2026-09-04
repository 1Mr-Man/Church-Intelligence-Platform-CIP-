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
  /** Phase 24.3 (true dual-tier Whisper): exact file path the optional
   * second, quality-tier speech engine looks for a local Whisper model at -
   * overridable via CIP_WHISPER_QUALITY_MODEL_PATH. Mirrors
   * `whisperModelPath` exactly; unlike it, a missing file here is the
   * expected default state, not something a "speech unavailable" notice
   * needs to call out. */
  whisperQualityModelPath: string;
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
  | {
      status: "present";
      path: string;
      sizeBytes: number;
      /** Phase 22: a heuristic, size-based guess at the model's whisper.cpp
       * family (tiny/base/small/medium/large) - see `commands.rs`'s
       * `classify_model_size_tier` for why `path`'s filename alone can
       * never answer this. Never a certainty (quantized files of a larger
       * model can be smaller than an unquantized smaller one). */
      sizeTierHint: string;
    };

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
  /**
   * Phase 5.3: count of fully-buffered windows the speech engine's own
   * voice-activity detection classified as silence and skipped without
   * running real inference. Distinct from a window that simply hasn't
   * finished buffering yet - that case never increments this counter.
   */
  silentWindowsSkipped: number;
  /**
   * Phase 14: count of real inference passes that produced only one of
   * whisper.cpp's own known non-speech placeholder captions (e.g.
   * `"[BLANK_AUDIO]"`, `"(speaking in foreign language)"`) and were
   * discarded rather than reported as real spoken content. Distinct from
   * `silentWindowsSkipped` - that counter means inference never ran at
   * all; this one means it did run and produced a known non-answer.
   */
  nonSpeechPlaceholdersSkipped: number;
  /**
   * Phase 21: count of real inference passes triggered because a natural
   * pause was detected in the buffered audio, rather than because the
   * buffer hit the fixed ~3s cap. Not mutually exclusive with
   * `silentWindowsSkipped` - a pause-triggered flush can still turn out to
   * have been entirely silence.
   */
  vadEarlyFlushes: number;
}

/**
 * Phase 24.3 (true dual-tier Whisper): what the running process actually
 * observed about the second, optional quality-tier engine - mirrors
 * `commands.rs`'s `SpeechQualityRuntimeDiagnostics` one-to-one, the same
 * relationship `SpeechRuntimeDiagnostics` above has to the fast tier.
 */
export interface SpeechQualityRuntimeDiagnostics {
  featureCompiled: boolean;
  modelLoadAttempted: boolean;
  modelLoaded: boolean;
  modelLoadError: string | null;
  engineReady: boolean;
  /** Fast-tier final windows handed to the quality worker so far. */
  jobsSubmitted: number;
  /** Jobs dropped because the bounded quality channel was full - the
   * quality worker (a slower model, by design) was still catching up.
   * Never fatal, never blocks the fast tier. */
  jobsDroppedBacklog: number;
  /** Jobs that produced a real, non-empty correction routed through the
   * pipeline as a new, linked transcript segment. */
  jobsCompleted: number;
  /** Phase 24.3.2: consecutive jobs dropped since the worker last actually
   * processed one - the raw signal `backlogState` below is derived from. */
  consecutiveJobsDropped: number;
  /** Derived from `consecutiveJobsDropped` against fixed thresholds - see
   * `commands.rs::classify_quality_backlog`. A single drop reads `"busy"`
   * (usually harmless - the fast tier just produced two windows close
   * together); a streak of 3+ reads `"overloaded"` - real, sustained
   * evidence the configured quality model is too slow for this hardware
   * at this cadence, not a transient blip. */
  backlogState: "normal" | "busy" | "falling_behind" | "overloaded";
  lastError: string | null;
}

export interface PilotDiagnostics {
  machine: MachineDiagnostic;
  whisperModel: WhisperModelDiagnostic;
  speech: SpeechRuntimeDiagnostics;
  /** Phase 24.3: `{status: "missing", ...}` (the default) unless an
   * operator has installed a second, quality-tier model via
   * `installWhisperQualityModel`. */
  whisperQualityModel: WhisperModelDiagnostic;
  speechQuality: SpeechQualityRuntimeDiagnostics;
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

/** Phase 25 (Session Black Box): frontend mirror of `commands.rs`'s
 * `SessionReportExport`, via `export_session_report`. The frontend never
 * reads the exported JSON's own content - only where it landed on disk,
 * mirroring `BackupReport`'s own shape exactly. */
export interface SessionReportExport {
  reportPath: string;
  sizeBytes: number;
}
