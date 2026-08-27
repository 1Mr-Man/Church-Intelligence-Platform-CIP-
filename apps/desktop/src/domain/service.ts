/**
 * Service domain contracts. Mirrors `core/service` (the session lifecycle
 * below) and, as of Phase 2.4 (Service Intelligence, per the authoritative
 * Phase 2 roadmap), `cip_core_intelligence::service_adapter` (the phase-
 * inference types further down) - the same "one domain file mirrors both
 * the pure domain crate and its intelligence adapter" convention
 * `domain/sermon.ts` already established.
 */

export type ServiceStatus = "started" | "paused" | "ended";

export interface ServiceSession {
  id: string;
  title: string;
  status: ServiceStatus;
  startedAt: string; // ISO-8601
  endedAt: string | null;
}

export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

export interface AudioEngineStatus {
  isCapturing: boolean;
  isPaused: boolean;
  sampleRateHz: number;
  /** Coarse RMS input level in `0.0..=1.0`, if the backend can report one. */
  inputLevel: number | null;
  /**
   * Phase 3.2: the most recent mid-capture stream failure (e.g. a
   * microphone physically unplugged while listening), reported by the
   * backend's own stream-error callback. `null` when capture has never
   * failed, or has been cleared by a subsequent successful `start()`.
   */
  streamError: string | null;
  /**
   * Phase 3.4: the device id passed to the most recent successful
   * `start()`. `null` before the engine has ever started capturing;
   * persists after `stop()` so diagnostics can show "which microphone
   * was selected" without a live capture running.
   */
  selectedDevice: string | null;
  /** The input channel count negotiated on the most recent successful `start()`. */
  channels: number | null;
}

/**
 * The contract for capturing raw audio from an input device during a live
 * service. Mirrors `AudioEngine` in `core/service`. The frontend never
 * calls this directly - it uses the `list_audio_devices`/
 * `start_listening`/`stop_listening` commands - but keeps this shape for
 * documentation/type parity, matching `ai.ts`'s `SpeechEngine`.
 */
export interface AudioEngine {
  listDevices(): Promise<AudioDevice[]>;
  start(deviceId: string): Promise<void>;
  stop(): Promise<void>;
  status(): AudioEngineStatus;
}

// --- Service Intelligence (Phase 2.4, per the authoritative Phase 2 roadmap) --
//
// Distinct from `apps/desktop/src-tauri/src/cross_domain.rs`'s earlier
// cross-domain correlation prototype (reserved for a future formal Phase
// 2.8 integration - see `domain/intelligence.ts`'s `IntelligenceCorrelation`).
// Service Intelligence findings themselves are ordinary `IntelligenceFinding`s
// (see `domain/intelligence.ts`, `domain: "service"`, `kind: "service_state"`) -
// the types here are the phase taxonomy and the read-only summary
// `getServiceIntelligenceState` returns, mirroring `sermon.ts`'s
// `SermonState`/`SermonStateSnapshot` shape exactly.

/** The observable phase of a live service - distinct from `ServiceStatus`
 * above (lifecycle: is a service running at all). Deliberately a smaller
 * set than every phase a real service could contain - see
 * `docs/service-intelligence.md`'s "NOT AVAILABLE" section. */
export type ServicePhase =
  | "unknown"
  | "opening"
  | "worship"
  | "prayer"
  | "scripture_reading"
  | "sermon"
  | "offering"
  | "announcement"
  | "closing";

/** Whether the transcript itself is still actively updating - never a
 * reason to pause or end the service on its own; purely informational
 * (spec section 41). */
export type TranscriptFreshness =
  | { status: "unknown" }
  | { status: "fresh" }
  | { status: "stale"; secondsSince: number };

/** The read-only summary `get_service_intelligence_state` returns - what
 * `ServiceIntelligenceEngine` has derived so far, independent of any
 * pending/accepted/rejected finding review state. Audio/speech/database
 * health still comes from `LiveStatus` (`get_live_status`) alone - this
 * never duplicates those fields. */
export interface ServiceIntelligenceSummary {
  phase: ServicePhase;
  phaseStartedAt: string; // ISO-8601
  previousPhase: ServicePhase | null;
  transitionCount: number;
  transcriptFreshness: TranscriptFreshness;
}
