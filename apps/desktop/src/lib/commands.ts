/**
 * Typed wrappers around the Tauri commands registered in
 * `apps/desktop/src-tauri/src/lib.rs`'s `invoke_handler`. Keeping `invoke`
 * calls behind named, typed functions (rather than calling `invoke` inline
 * from components) is the one indirection Phase 1 needs: it's the single
 * place that has to change if a command's name or payload shape changes.
 *
 * Every function here goes through {@link invokeCommand}, which checks
 * {@link isTauriRuntime} first - see `lib/runtime.ts`. Outside the Tauri
 * desktop shell (e.g. the web deployment opened in a plain browser),
 * `@tauri-apps/api`'s real `invoke` is never called; callers get a
 * rejected promise carrying {@link TauriUnavailableError} instead of a
 * raw `TypeError` from a missing `window.__TAURI_INTERNALS__`.
 */
import { invoke } from "@tauri-apps/api/core";
import type { AppConfig, AppEnvironment } from "../config/appConfig";
import type {
  AudioDevice,
  BibleTranslation,
  BibleVerse,
  LiveStatus,
  PresentationItem,
  PresentationPreview,
  ProcessedSegment,
  ScriptureContext,
  ServiceSession,
  Suggestion,
  SuggestionStatus,
  TimelineEntry,
  TranscriptSegment,
} from "../domain";
import { isTauriRuntime } from "./runtime";

export interface HealthReport {
  databaseConnected: boolean;
  appliedMigrations: number;
  environment: AppEnvironment;
}

/** Thrown instead of calling Tauri IPC when this frontend is not running
 * inside the Tauri desktop shell - see `lib/runtime.ts`. */
export class TauriUnavailableError extends Error {
  constructor(command: string) {
    super(`"${command}" requires the CIP desktop application and is not available in a web browser.`);
    this.name = "TauriUnavailableError";
  }
}

function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriRuntime()) {
    return Promise.reject(new TauriUnavailableError(command));
  }
  return invoke<T>(command, args);
}

// --- foundation (Phase 1.0/1.1) --------------------------------------------

export function getAppConfig(): Promise<AppConfig> {
  return invokeCommand("get_app_config");
}

export function appHealthCheck(): Promise<HealthReport> {
  return invokeCommand("app_health_check");
}

export function listBibleTranslations(): Promise<BibleTranslation[]> {
  return invokeCommand("list_bible_translations");
}

// --- service lifecycle ------------------------------------------------------

export function startService(title: string): Promise<ServiceSession> {
  return invokeCommand("start_service", { title });
}

export function pauseService(): Promise<ServiceSession> {
  return invokeCommand("pause_service");
}

export function resumeService(): Promise<ServiceSession> {
  return invokeCommand("resume_service");
}

export function endService(): Promise<ServiceSession> {
  return invokeCommand("end_service");
}

// --- audio / live listening --------------------------------------------------

export function listAudioDevices(): Promise<AudioDevice[]> {
  return invokeCommand("list_audio_devices");
}

export function startListening(deviceId?: string): Promise<void> {
  return invokeCommand("start_listening", { deviceId: deviceId ?? null });
}

export function stopListening(): Promise<void> {
  return invokeCommand("stop_listening");
}

// --- deterministic transcript harness / manual entry -------------------------

export function processTestTranscript(text: string): Promise<ProcessedSegment> {
  return invokeCommand("process_test_transcript", { text });
}

// --- transcript & suggestions -------------------------------------------------

/** `serviceId` is optional and defaults to the active service - passing
 * it lets the Phase 1.3 service archive inspect a *completed* service's
 * transcript without disturbing the live view (see `resolve_service_id`
 * on the Rust side). */
export function listTranscript(limit: number, serviceId?: string): Promise<TranscriptSegment[]> {
  return invokeCommand("list_transcript", { limit, serviceId: serviceId ?? null });
}

export function listSuggestions(status?: SuggestionStatus, serviceId?: string): Promise<Suggestion[]> {
  return invokeCommand("list_suggestions", { status: status ?? null, serviceId: serviceId ?? null });
}

/** The service timeline (Phase 1.3) - same optional `serviceId` pattern. */
export function listTimeline(limit: number, serviceId?: string): Promise<TimelineEntry[]> {
  return invokeCommand("list_timeline", { serviceId: serviceId ?? null, limit });
}

/** Completed services, most recent first - the service archive's list view. */
export function listServiceHistory(limit: number): Promise<ServiceSession[]> {
  return invokeCommand("list_service_history", { limit });
}

/** A single service by id, independent of whichever one (if any) is
 * currently active - the service archive's detail view. */
export function getService(serviceId: string): Promise<ServiceSession> {
  return invokeCommand("get_service", { serviceId });
}

export function approveSuggestion(suggestionId: string): Promise<Suggestion> {
  return invokeCommand("approve_suggestion", { suggestionId });
}

export function editSuggestion(suggestionId: string, newReference: string): Promise<Suggestion> {
  return invokeCommand("edit_suggestion", { suggestionId, newReference });
}

export function rejectSuggestion(suggestionId: string): Promise<Suggestion> {
  return invokeCommand("reject_suggestion", { suggestionId });
}

// --- presentation (Phase 1.4) ------------------------------------------------
//
// Preview is non-mutating and available before approval (see
// `previewPresentation`/`previewScripture`); only `preparePresentation` and
// `createManualPresentation` ever persist a presentation_items row, and
// `preparePresentation` remains strictly gated on an approved suggestion -
// see `docs/presentation.md`.

/** Previews a suggestion's scripture reference - works on a still-`pending`
 * suggestion, unlike `preparePresentation`. */
export function previewPresentation(suggestionId: string): Promise<PresentationPreview> {
  return invokeCommand("preview_presentation", { suggestionId });
}

/** Previews an arbitrary reference (e.g. from manual Bible search) with no
 * suggestion involved. */
export function previewScripture(reference: string): Promise<PresentationPreview> {
  return invokeCommand("preview_scripture", { reference });
}

export function preparePresentation(suggestionId: string): Promise<PresentationItem> {
  return invokeCommand("prepare_presentation", { suggestionId });
}

/** Creates a prepared presentation item directly from a reference, with no
 * suggestion or speech recognition involved - the manual fallback. */
export function createManualPresentation(reference: string): Promise<PresentationItem> {
  return invokeCommand("create_manual_presentation", { reference });
}

/** What's currently prepared for the active service (never includes
 * cancelled items) - the Current Output panel's data source. */
export function listPreparedPresentations(): Promise<PresentationItem[]> {
  return invokeCommand("list_prepared_presentations");
}

export function getPresentationItem(itemId: string): Promise<PresentationItem> {
  return invokeCommand("get_presentation_item", { itemId });
}

/** Cancels ("retracts") a still-prepared item before it's ever displayed. */
export function cancelPresentation(itemId: string): Promise<PresentationItem> {
  return invokeCommand("cancel_presentation", { itemId });
}

// --- ambiguity resolution & context correction (Phase 1.3) ----------------

/** Resolves an `ambiguous` detection by an explicit operator choice - see
 * `ScriptureDetection.candidates`. `candidatesShown` is the full set that
 * was offered, kept purely for the audit record. */
export function resolveAmbiguousReference(
  book: string,
  chapter: number,
  verse: number,
  rawText: string,
  candidatesShown: string[],
): Promise<Suggestion> {
  return invokeCommand("resolve_ambiguous_reference", {
    book,
    chapter,
    verse,
    rawText,
    candidatesShown,
  });
}

/** Operator correction of the active Scripture context when CIP
 * misunderstood the pastor - validated against the Bible the same way an
 * automatic chapter detection would be. */
export function correctScriptureContext(book: string, chapter: number): Promise<ScriptureContext> {
  return invokeCommand("correct_scripture_context", { book, chapter });
}

// --- manual Bible search (works with no audio/speech/network) ---------------

export function searchBible(query: string): Promise<BibleVerse[]> {
  return invokeCommand("search_bible", { query });
}

// --- live status --------------------------------------------------------------

export function getLiveStatus(): Promise<LiveStatus> {
  return invokeCommand("get_live_status");
}
