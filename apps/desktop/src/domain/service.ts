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
  sampleRateHz: number;
}

/**
 * The contract for capturing raw audio from an input device during a live
 * service. Mirrors `AudioEngine` in `core/service` - no implementation
 * exists yet (speech/audio capture is explicitly out of scope for Phase 1).
 */
export interface AudioEngine {
  listDevices(): Promise<AudioDevice[]>;
  start(deviceId: string): Promise<void>;
  stop(): Promise<void>;
  status(): AudioEngineStatus;
}
