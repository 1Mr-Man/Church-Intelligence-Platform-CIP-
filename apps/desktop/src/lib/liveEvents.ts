/**
 * Typed subscriptions to the backend events relevant to the Live Church
 * Brain, over `@tauri-apps/api/event`'s `listen` - see
 * `src/events/eventNames.ts` for the shared name registry and
 * `apps/desktop/src-tauri/src/commands.rs`'s `emit_processed_segment_events`
 * for what the backend actually sends.
 *
 * Every subscription goes through {@link listenSafe}, which checks
 * {@link isTauriRuntime} first (see `lib/runtime.ts`). Outside the Tauri
 * desktop shell there is no backend to emit these events at all, so
 * subscribing is a silent, resolved no-op (a no-op `UnlistenFn`) rather
 * than a call into a `window.__TAURI_INTERNALS__` that doesn't exist.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { AppEvents } from "../events/eventNames";
import type {
  CurrentSong,
  IntelligenceFinding,
  PresentationItem,
  ProcessedSegment,
  ScriptureDetection,
  Suggestion,
  TranscriptSegment,
} from "../domain";
import { isTauriRuntime } from "./runtime";

function listenSafe<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return Promise.resolve(() => {});
  }
  return listen<T>(event, (e) => handler(e.payload));
}

export function onTranscriptUpdated(handler: (segment: TranscriptSegment) => void): Promise<UnlistenFn> {
  return listenSafe<TranscriptSegment>(AppEvents.TranscriptUpdated, handler);
}

export function onScriptureDetected(handler: (detection: ScriptureDetection) => void): Promise<UnlistenFn> {
  return listenSafe<ScriptureDetection>(AppEvents.ScriptureDetected, handler);
}

export function onScriptureUpdated(handler: (detection: ScriptureDetection) => void): Promise<UnlistenFn> {
  return listenSafe<ScriptureDetection>(AppEvents.ScriptureUpdated, handler);
}

export function onSuggestionCreated(handler: (suggestion: Suggestion) => void): Promise<UnlistenFn> {
  return listenSafe<Suggestion>(AppEvents.SuggestionCreated, handler);
}

export function onSuggestionApproved(handler: (suggestion: Suggestion) => void): Promise<UnlistenFn> {
  return listenSafe<Suggestion>(AppEvents.SuggestionApproved, handler);
}

export function onSuggestionEdited(handler: (suggestion: Suggestion) => void): Promise<UnlistenFn> {
  return listenSafe<Suggestion>(AppEvents.SuggestionEdited, handler);
}

export function onSuggestionRejected(handler: (suggestion: Suggestion) => void): Promise<UnlistenFn> {
  return listenSafe<Suggestion>(AppEvents.SuggestionRejected, handler);
}

/** A non-mutating preview render - never implies anything was persisted. */
export function onPresentationPreviewed(handler: (item: PresentationItem) => void): Promise<UnlistenFn> {
  return listenSafe<PresentationItem>(AppEvents.PresentationPreviewed, handler);
}

export function onPresentationPrepared(handler: (item: PresentationItem) => void): Promise<UnlistenFn> {
  return listenSafe<PresentationItem>(AppEvents.PresentationPrepared, handler);
}

export function onPresentationCancelled(handler: (item: PresentationItem) => void): Promise<UnlistenFn> {
  return listenSafe<PresentationItem>(AppEvents.PresentationCancelled, handler);
}

/** A new Music Intelligence finding (Phase 2.1) - never implies a
 * presentation item was created; see `docs/music-intelligence.md`. */
export function onMusicFindingDetected(
  handler: (finding: IntelligenceFinding) => void,
): Promise<UnlistenFn> {
  return listenSafe<IntelligenceFinding>(AppEvents.MusicFindingDetected, handler);
}

export function onMusicFindingAccepted(
  handler: (finding: IntelligenceFinding) => void,
): Promise<UnlistenFn> {
  return listenSafe<IntelligenceFinding>(AppEvents.MusicFindingAccepted, handler);
}

export function onMusicFindingRejected(
  handler: (finding: IntelligenceFinding) => void,
): Promise<UnlistenFn> {
  return listenSafe<IntelligenceFinding>(AppEvents.MusicFindingRejected, handler);
}

/** The operator-confirmed "current song" changed (Phase 2.2) - `song` is
 * `null` when the operator cleared it (`clearCurrentSong`). */
export function onCurrentSongChanged(
  handler: (song: CurrentSong | null) => void,
): Promise<UnlistenFn> {
  return listenSafe<CurrentSong | null>(AppEvents.CurrentSongChanged, handler);
}

/** Unused by `ProcessedSegment` directly but kept for reference/parity -
 * `ProcessedSegment` itself is only ever returned from
 * `process_test_transcript`'s command response, never emitted as an
 * event (its detections/suggestions are emitted individually instead, so
 * the frontend doesn't have to unpack a compound event). */
export type { ProcessedSegment };
