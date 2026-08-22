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
  IntelligenceCorrelation,
  IntelligenceFinding,
  PresentationItem,
  ProcessedSegment,
  ScriptureDetection,
  SermonPoint,
  SermonState,
  Suggestion,
  ThemeCandidate,
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

/** A new Sermon Intelligence finding (Phase 2.3) - never implies anything
 * was presented or auto-approved; see `docs/sermon-intelligence.md`. */
export function onSermonFindingDetected(
  handler: (finding: IntelligenceFinding) => void,
): Promise<UnlistenFn> {
  return listenSafe<IntelligenceFinding>(AppEvents.SermonFindingDetected, handler);
}

export function onSermonFindingAccepted(
  handler: (finding: IntelligenceFinding) => void,
): Promise<UnlistenFn> {
  return listenSafe<IntelligenceFinding>(AppEvents.SermonFindingAccepted, handler);
}

export function onSermonFindingRejected(
  handler: (finding: IntelligenceFinding) => void,
): Promise<UnlistenFn> {
  return listenSafe<IntelligenceFinding>(AppEvents.SermonFindingRejected, handler);
}

/** The sermon structure changed (a new main/sub-point recorded) - payload
 * is the full, current `SermonPoint[]`; earlier points are never
 * rewritten. */
export function onSermonStructureUpdated(
  handler: (points: SermonPoint[]) => void,
): Promise<UnlistenFn> {
  return listenSafe<SermonPoint[]>(AppEvents.SermonStructureUpdated, handler);
}

/** The current theme candidate changed - always `Inferred`; `candidate`
 * is `null` if evidence no longer supports one (should not normally
 * happen, since evidence only accumulates within a service). */
export function onSermonThemeChanged(
  handler: (candidate: ThemeCandidate | null) => void,
): Promise<UnlistenFn> {
  return listenSafe<ThemeCandidate | null>(AppEvents.SermonThemeChanged, handler);
}

/** The lightweight derived sermon state changed - a classification, never
 * a rigid state machine transition. */
export function onSermonStateChanged(handler: (state: SermonState) => void): Promise<UnlistenFn> {
  return listenSafe<SermonState>(AppEvents.SermonStateChanged, handler);
}

/** The Phase 2.4 correlation engine produced a new cross-domain
 * correlation - never implies anything was presented, approved, or
 * projected; see `docs/cross-domain-intelligence.md`. */
export function onCrossDomainCorrelationDetected(
  handler: (correlation: IntelligenceCorrelation) => void,
): Promise<UnlistenFn> {
  return listenSafe<IntelligenceCorrelation>(AppEvents.CrossDomainCorrelationDetected, handler);
}

/** The operator reviewed a correlation without dismissing it - the same
 * informational-only semantics as `IntelligenceFinding.review`. */
export function onCrossDomainCorrelationReviewed(
  handler: (correlation: IntelligenceCorrelation) => void,
): Promise<UnlistenFn> {
  return listenSafe<IntelligenceCorrelation>(AppEvents.CrossDomainCorrelationReviewed, handler);
}

/** The operator explicitly dismissed a correlation - never automatic. */
export function onCrossDomainCorrelationDismissed(
  handler: (correlation: IntelligenceCorrelation) => void,
): Promise<UnlistenFn> {
  return listenSafe<IntelligenceCorrelation>(AppEvents.CrossDomainCorrelationDismissed, handler);
}

/** Unused by `ProcessedSegment` directly but kept for reference/parity -
 * `ProcessedSegment` itself is only ever returned from
 * `process_test_transcript`'s command response, never emitted as an
 * event (its detections/suggestions are emitted individually instead, so
 * the frontend doesn't have to unpack a compound event). */
export type { ProcessedSegment };
