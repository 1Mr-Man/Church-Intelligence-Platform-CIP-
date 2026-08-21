/**
 * Typed subscriptions to the backend events relevant to the Live Church
 * Brain, over `@tauri-apps/api/event`'s `listen` - see
 * `src/events/eventNames.ts` for the shared name registry and
 * `apps/desktop/src-tauri/src/commands.rs`'s `emit_processed_segment_events`
 * for what the backend actually sends.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { AppEvents } from "../events/eventNames";
import type { ProcessedSegment, ScriptureDetection, Suggestion, TranscriptSegment } from "../domain";

export function onTranscriptUpdated(handler: (segment: TranscriptSegment) => void): Promise<UnlistenFn> {
  return listen<TranscriptSegment>(AppEvents.TranscriptUpdated, (event) => handler(event.payload));
}

export function onScriptureDetected(handler: (detection: ScriptureDetection) => void): Promise<UnlistenFn> {
  return listen<ScriptureDetection>(AppEvents.ScriptureDetected, (event) => handler(event.payload));
}

export function onScriptureUpdated(handler: (detection: ScriptureDetection) => void): Promise<UnlistenFn> {
  return listen<ScriptureDetection>(AppEvents.ScriptureUpdated, (event) => handler(event.payload));
}

export function onSuggestionCreated(handler: (suggestion: Suggestion) => void): Promise<UnlistenFn> {
  return listen<Suggestion>(AppEvents.SuggestionCreated, (event) => handler(event.payload));
}

export function onSuggestionApproved(handler: (suggestion: Suggestion) => void): Promise<UnlistenFn> {
  return listen<Suggestion>(AppEvents.SuggestionApproved, (event) => handler(event.payload));
}

export function onSuggestionEdited(handler: (suggestion: Suggestion) => void): Promise<UnlistenFn> {
  return listen<Suggestion>(AppEvents.SuggestionEdited, (event) => handler(event.payload));
}

export function onSuggestionRejected(handler: (suggestion: Suggestion) => void): Promise<UnlistenFn> {
  return listen<Suggestion>(AppEvents.SuggestionRejected, (event) => handler(event.payload));
}

/** Unused by `ProcessedSegment` directly but kept for reference/parity -
 * `ProcessedSegment` itself is only ever returned from
 * `process_test_transcript`'s command response, never emitted as an
 * event (its detections/suggestions are emitted individually instead, so
 * the frontend doesn't have to unpack a compound event). */
export type { ProcessedSegment };
