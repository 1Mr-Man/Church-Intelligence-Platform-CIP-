/**
 * Typed wrappers around the Tauri commands registered in
 * `apps/desktop/src-tauri/src/lib.rs`'s `invoke_handler`. Keeping `invoke`
 * calls behind named, typed functions (rather than calling `invoke` inline
 * from components) is the one indirection Phase 1 needs: it's the single
 * place that has to change if a command's name or payload shape changes.
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

export interface HealthReport {
  databaseConnected: boolean;
  appliedMigrations: number;
  environment: AppEnvironment;
}

// --- foundation (Phase 1.0/1.1) --------------------------------------------

export function getAppConfig(): Promise<AppConfig> {
  return invoke("get_app_config");
}

export function appHealthCheck(): Promise<HealthReport> {
  return invoke("app_health_check");
}

export function listBibleTranslations(): Promise<BibleTranslation[]> {
  return invoke("list_bible_translations");
}

// --- service lifecycle ------------------------------------------------------

export function startService(title: string): Promise<ServiceSession> {
  return invoke("start_service", { title });
}

export function endService(): Promise<ServiceSession> {
  return invoke("end_service");
}

// --- audio / live listening --------------------------------------------------

export function listAudioDevices(): Promise<AudioDevice[]> {
  return invoke("list_audio_devices");
}

export function startListening(deviceId?: string): Promise<void> {
  return invoke("start_listening", { deviceId: deviceId ?? null });
}

export function stopListening(): Promise<void> {
  return invoke("stop_listening");
}

// --- deterministic transcript harness / manual entry -------------------------

export function processTestTranscript(text: string): Promise<ProcessedSegment> {
  return invoke("process_test_transcript", { text });
}

// --- transcript & suggestions -------------------------------------------------

export function listTranscript(limit: number): Promise<TranscriptSegment[]> {
  return invoke("list_transcript", { limit });
}

export function listSuggestions(status?: SuggestionStatus): Promise<Suggestion[]> {
  return invoke("list_suggestions", { status: status ?? null });
}

export function approveSuggestion(suggestionId: string): Promise<Suggestion> {
  return invoke("approve_suggestion", { suggestionId });
}

export function editSuggestion(suggestionId: string, newReference: string): Promise<Suggestion> {
  return invoke("edit_suggestion", { suggestionId, newReference });
}

export function rejectSuggestion(suggestionId: string): Promise<Suggestion> {
  return invoke("reject_suggestion", { suggestionId });
}

export function preparePresentation(suggestionId: string): Promise<PresentationItem> {
  return invoke("prepare_presentation", { suggestionId });
}

// --- manual Bible search (works with no audio/speech/network) ---------------

export function searchBible(query: string): Promise<BibleVerse[]> {
  return invoke("search_bible", { query });
}

// --- live status --------------------------------------------------------------

export function getLiveStatus(): Promise<LiveStatus> {
  return invoke("get_live_status");
}
