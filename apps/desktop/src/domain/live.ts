/**
 * Live service status contracts. Mirrors `commands.rs`'s `LiveStatus` and
 * its component enums (Rust) - the Live Church Brain's status header reads
 * directly from these, via `get_live_status`.
 */
import type { AcousticEngineStatus, CurrentSong } from "./music";
import type { AudioEngineStatus, ServiceSession } from "./service";

/** Display-level service status - not `core/service::ServiceStatus`
 * itself (which only has started/paused/ended). `planned` covers "no
 * active service yet"; `completed` covers "ended". */
export type LiveServiceStatus = "planned" | "live" | "paused" | "completed";

/** `error`: a real capture failure was recorded and not yet cleared by a
 * successful retry (Phase 1.3) - distinct from `unavailable` (no device
 * at all). See `docs/live-service.md`'s recovery section. */
export type AudioStatusKind = "unavailable" | "ready" | "listening" | "error";

/** `error`: a real speech-engine failure was recorded and not yet cleared
 * by the next successful transcription (Phase 1.3). */
export type SpeechStatusKind = "unavailable" | "ready" | "error";

export type NetworkStatusKind = "offline" | "online";

/**
 * Deliberately independent of `NetworkStatusKind` - a fully offline
 * machine with a local speech model installed is `available`, not
 * `degraded`. See `docs/live-speech.md`.
 */
export type AiStatusKind = "available" | "degraded" | "unavailable";

export type DatabaseStatusKind = "connected" | "error";

export interface LiveStatus {
  service: ServiceSession | null;
  serviceStatus: LiveServiceStatus;
  audio: AudioEngineStatus;
  audioStatus: AudioStatusKind;
  speechStatus: SpeechStatusKind;
  networkStatus: NetworkStatusKind;
  aiStatus: AiStatusKind;
  databaseStatus: DatabaseStatusKind;
  /** Phase 2.2: whether/why acoustic recognition can currently run. */
  acousticStatus: AcousticEngineStatus;
  /** Phase 2.2: the operator-confirmed current song, if any - `null`
   * until an operator accepts a Music finding. */
  currentSong: CurrentSong | null;
}

/**
 * One service-timeline entry (Phase 1.3). Mirrors `timeline::TimelineEntry`
 * (Rust), itself a thin read model over the existing `audit_events` table -
 * no second event bus, no redundant timeline table. `eventName` is one of
 * the `AppEvents` values (`events/eventNames.ts`); the Live Church Brain
 * derives a human-readable line from `eventName` + `payload` rather than
 * the backend pre-formatting a description string.
 */
export interface TimelineEntry {
  id: string;
  serviceId: string | null;
  eventName: string;
  category: string;
  payload: Record<string, unknown> | null;
  createdAt: string; // ISO-8601
}
