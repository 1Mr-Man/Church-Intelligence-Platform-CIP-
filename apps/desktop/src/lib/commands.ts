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
  BibleSearchResult,
  BibleTranslation,
  ContentMetadata,
  ContentType,
  DomainCapabilityReport,
  ImportReport,
  IntegrityReport,
  IntelligenceFinding,
  LiveStatus,
  MusicImportReport,
  MusicQueryType,
  PresentationItem,
  PresentationPreview,
  ProcessedSegment,
  ScriptureContext,
  ServiceSession,
  SermonStateSnapshot,
  SongRecognitionCandidate,
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

/** `translationId` defaults to the app's default translation
 * (`DEFAULT_TRANSLATION_ID` on the Rust side) when omitted. Dispatches to
 * an exact reference/chapter/range lookup or a free-text search - see
 * `cip_core_bible::search::search_bible`'s docs. */
export function searchBible(query: string, translationId?: string): Promise<BibleSearchResult[]> {
  return invokeCommand("search_bible", { query, translationId: translationId ?? null });
}

// --- content registry (Phase 1.5) --------------------------------------------
//
// What local content exists, and under what license/version - see
// `docs/content-registry.md`. Disabled content never appears in
// `listBibleTranslations`'s result but is never deleted.

export function listContentRegistry(contentType?: ContentType): Promise<ContentMetadata[]> {
  return invokeCommand("list_content_registry", { contentType: contentType ?? null });
}

export function getContentMetadata(contentId: string): Promise<ContentMetadata> {
  return invokeCommand("get_content_metadata", { contentId });
}

export function setContentEnabled(contentId: string, enabled: boolean): Promise<ContentMetadata> {
  return invokeCommand("set_content_enabled", { contentId, enabled });
}

/** `datasetJson` is the dataset file's contents, already read as text by
 * the frontend (e.g. via `FileReader`) - this never asks the backend to
 * touch the filesystem itself. See `docs/bible-datasets.md` for the
 * expected JSON shape (`BibleDatasetInput`). */
export function importBibleDataset(datasetJson: string): Promise<ImportReport> {
  return invokeCommand("import_bible_dataset", { datasetJson });
}

export function checkBibleDatasetIntegrity(translationId: string): Promise<IntegrityReport> {
  return invokeCommand("check_bible_dataset_integrity", { translationId });
}

// --- intelligence (Phase 2.0) --------------------------------------------------

/** Reports each reserved intelligence domain's real capability - never
 * fabricates a capability for a domain with no registered engine. */
export function getIntelligenceCapabilities(): Promise<DomainCapabilityReport[]> {
  return invokeCommand("get_intelligence_capabilities");
}

// --- music intelligence (Phase 2.1) -------------------------------------------
//
// Dataset listing reuses `listContentRegistry("music")` above - Music
// datasets are ordinary Content Registry entries, so there is no separate
// "list music datasets" command.

/** Manual song search - works with no audio/speech/network, same
 * reasoning as `searchBible`. `contentIds` lets the operator explicitly
 * name which dataset(s) to search (including a disabled one); omitted,
 * only currently-enabled Music datasets are searched. */
export function searchMusic(
  query: string,
  queryType: MusicQueryType,
  contentIds?: string[],
): Promise<SongRecognitionCandidate[]> {
  return invokeCommand("search_music", { query, queryType, contentIds: contentIds ?? null });
}

/** Imports a local music dataset, already read as text by the frontend -
 * mirrors `importBibleDataset`. See `docs/music-datasets.md` for the
 * expected JSON shape (`MusicDatasetInput`). */
export function importMusicDataset(datasetJson: string): Promise<MusicImportReport> {
  return invokeCommand("import_music_dataset", { datasetJson });
}

/** The deterministic music-analysis harness - the Music Intelligence
 * counterpart to `processTestTranscript`. Never creates a presentation
 * item; findings are queued for operator review only. */
export function analyzeMusicTranscript(text: string): Promise<IntelligenceFinding[]> {
  return invokeCommand("analyze_music_transcript", { text });
}

/** Music findings still awaiting an operator decision, for the active
 * service - the Music Intelligence panel's data source. */
export function listMusicFindings(): Promise<IntelligenceFinding[]> {
  return invokeCommand("list_music_findings");
}

export function acceptMusicFinding(findingId: string): Promise<IntelligenceFinding> {
  return invokeCommand("accept_music_finding", { findingId });
}

export function rejectMusicFinding(findingId: string): Promise<IntelligenceFinding> {
  return invokeCommand("reject_music_finding", { findingId });
}

// --- acoustic music recognition (Phase 2.2) ------------------------------------
//
// `get_acoustic_music_status` has no dedicated command - its status/reason
// is reused from `getLiveStatus().acousticStatus` rather than adding a
// second query command for the same data.

/** Explicit operator clear of the "current song" - the only other way it
 * ever changes besides `acceptMusicFinding` setting it. Never inferred
 * automatically. */
export function clearCurrentSong(): Promise<void> {
  return invokeCommand("clear_current_song");
}

/** The deterministic acoustic-analysis harness - the Phase 2.2 counterpart
 * to `analyzeMusicTranscript`, and the primary way to exercise the
 * acoustic pipeline without a microphone (e.g. with a scripted recognizer
 * configured on the backend for manual testing). `samples` is raw mono
 * PCM16 audio; a caller with no real audio can pass a synthetic buffer
 * (never real copyrighted audio - see `docs/acoustic-music.md`). Still
 * gated by the signal-quality check: silence/too-short audio returns an
 * honest empty result, never a fake one. */
export function analyzeMusicAudio(
  samples: number[],
  sampleRateHz: number,
): Promise<IntelligenceFinding[]> {
  return invokeCommand("analyze_music_audio", { samples, sampleRateHz });
}

// --- sermon intelligence (Phase 2.3) -------------------------------------------
//
// Deliberately manual-command-only, mirroring `analyzeMusicTranscript` -
// nothing here is wired into live audio; the pastor's live transcript
// reaches these through the same manual/test-mode entry point real audio
// would eventually use.

/** The deterministic sermon-analysis harness - the Sermon Intelligence
 * counterpart to `analyzeMusicTranscript`. Never creates a presentation
 * item; findings are queued for operator review only. */
export function analyzeSermonTranscript(text: string): Promise<IntelligenceFinding[]> {
  return invokeCommand("analyze_sermon_transcript", { text });
}

/** Sermon findings still awaiting an operator decision, for the active
 * service - the Sermon Intelligence panel's data source. */
export function listSermonFindings(): Promise<IntelligenceFinding[]> {
  return invokeCommand("list_sermon_findings");
}

export function acceptSermonFinding(findingId: string): Promise<IntelligenceFinding> {
  return invokeCommand("accept_sermon_finding", { findingId });
}

/** The generic, always-available correction path for a mis-detected
 * theme/point (spec section 40) - rejecting is itself the explicit,
 * auditable operator correction; it never rewrites transcript history. */
export function rejectSermonFinding(findingId: string): Promise<IntelligenceFinding> {
  return invokeCommand("reject_sermon_finding", { findingId });
}

/** The current theme/state/structure snapshot - read-only, safe to poll. */
export function getSermonState(): Promise<SermonStateSnapshot> {
  return invokeCommand("get_sermon_state");
}

// --- live status --------------------------------------------------------------

export function getLiveStatus(): Promise<LiveStatus> {
  return invokeCommand("get_live_status");
}
