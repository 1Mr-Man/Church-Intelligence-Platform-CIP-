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
  ProcessedSegment,
  ServiceSession,
  Suggestion,
  SuggestionStatus,
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

export function listTranscript(limit: number): Promise<TranscriptSegment[]> {
  return invokeCommand("list_transcript", { limit });
}

export function listSuggestions(status?: SuggestionStatus): Promise<Suggestion[]> {
  return invokeCommand("list_suggestions", { status: status ?? null });
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

export function preparePresentation(suggestionId: string): Promise<PresentationItem> {
  return invokeCommand("prepare_presentation", { suggestionId });
}

// --- manual Bible search (works with no audio/speech/network) ---------------

export function searchBible(query: string): Promise<BibleVerse[]> {
  return invokeCommand("search_bible", { query });
}

// --- live status --------------------------------------------------------------

export function getLiveStatus(): Promise<LiveStatus> {
  return invokeCommand("get_live_status");
}
