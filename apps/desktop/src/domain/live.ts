/**
 * Live service status contracts. Mirrors `commands.rs`'s `LiveStatus` and
 * its component enums (Rust) - the Live Church Brain's status header reads
 * directly from these, via `get_live_status`.
 */
import type { AudioEngineStatus, ServiceSession } from "./service";

/** Display-level service status - not `core/service::ServiceStatus`
 * itself (which only has started/paused/ended). `planned` covers "no
 * active service yet"; `completed` covers "ended". */
export type LiveServiceStatus = "planned" | "live" | "paused" | "completed";

export type AudioStatusKind = "unavailable" | "ready" | "listening";

export type SpeechStatusKind = "unavailable" | "ready";

export type NetworkStatusKind = "offline" | "online";

/**
 * Deliberately independent of `NetworkStatusKind` - a fully offline
 * machine with a local speech model installed is `available`, not
 * `degraded`. See `docs/live-speech.md`.
 */
export type AiStatusKind = "available" | "degraded" | "unavailable";

export interface LiveStatus {
  service: ServiceSession | null;
  serviceStatus: LiveServiceStatus;
  audio: AudioEngineStatus;
  audioStatus: AudioStatusKind;
  speechStatus: SpeechStatusKind;
  networkStatus: NetworkStatusKind;
  aiStatus: AiStatusKind;
}
