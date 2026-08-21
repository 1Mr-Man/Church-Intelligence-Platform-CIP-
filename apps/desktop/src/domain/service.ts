/**
 * Service domain contracts. Mirrors `core/service` (Rust).
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
