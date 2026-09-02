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
import type { AppConfig, AppEnvironment, BackupReport, PilotDiagnostics, WhisperModelDiagnostic } from "../config/appConfig";
import type { ContentCandidate } from "../domain/contentIntelligence";
import type {
  AcousticEnrollment,
  AudioDevice,
  BibleBook,
  BibleSearchResult,
  BibleTranslation,
  SavedScripture,
  ContentMetadata,
  ContentType,
  Display,
  DisplayRole,
  DomainCapabilityReport,
  ImportReport,
  IntegrityReport,
  IntelligenceCorrelation,
  IntelligenceFinding,
  LiveStatus,
  MusicImportReport,
  MusicQueryType,
  ObsTargetConfig,
  PresentationDisplayState,
  PresentationItem,
  PresentationPreview,
  PresentationScreen,
  ProcessedSegment,
  ProductionIntegrationConfigInput,
  ProductionIntegrationStatus,
  RouteMode,
  ScriptureContext,
  Sermon,
  SermonFoundationSummary,
  SermonHarvest,
  SermonSection,
  SermonSegment,
  ServiceIntelligenceSummary,
  ServiceReport,
  ServiceSession,
  SermonStateSnapshot,
  SongRecognitionCandidate,
  Suggestion,
  SuggestionStatus,
  TimelineEntry,
  TranscriptSegment,
  VmixTargetConfig,
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

/** Phase 3.2: hardware/model diagnostics for pilot setup - see `PilotDiagnostics`. */
export function getPilotDiagnostics(): Promise<PilotDiagnostics> {
  return invokeCommand("get_pilot_diagnostics");
}

/**
 * Phase 3.8.7.1: install an operator-selected Whisper model file (already
 * downloaded by them, e.g. from https://huggingface.co/ggerganov/whisper.cpp
 * on their own machine's internet connection). Validates the file by
 * actually attempting to load it as a real Whisper model before copying
 * it anywhere - see the Rust command's own docs (`commands.rs`) for why
 * that matters. Rejects (rather than resolving) with a human-readable
 * message on any failure - a missing/unreadable file, or a file that
 * doesn't load as a valid model. Takes effect on CIP's next launch, not
 * immediately - see the returned diagnostic's own freshness caveat in
 * the panel that calls this.
 */
export function installWhisperModel(sourcePath: string): Promise<WhisperModelDiagnostic> {
  return invokeCommand("install_whisper_model", { sourcePath });
}

/**
 * Phase 3.2: a consistent, point-in-time database backup written to
 * `destinationDir` (created if needed) via SQLite's own `VACUUM INTO` -
 * safe to call while CIP is running. Restoring is a manual, CIP-closed
 * procedure (see `docs/phase-3-2-hardware-pilot.md`), not a command.
 */
export function backupDatabase(destinationDir: string): Promise<BackupReport> {
  return invokeCommand("backup_database", { destinationDir });
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

/** The Phase 5.1 post-service observability report for a single service -
 * see `ServiceReport`'s own doc comment for exactly what it does and does
 * not represent. */
export function getServiceReport(serviceId: string): Promise<ServiceReport> {
  return invokeCommand("get_service_report", { serviceId });
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

/** `translationId` defaults to the app's default translation
 * (`DEFAULT_TRANSLATION_ID` on the Rust side) when omitted - same
 * reasoning as `searchBible`. Previews a suggestion's scripture
 * reference - works on a still-`pending` suggestion, unlike
 * `preparePresentation`. */
export function previewPresentation(
  suggestionId: string,
  translationId?: string,
): Promise<PresentationPreview> {
  return invokeCommand("preview_presentation", { suggestionId, translationId: translationId ?? null });
}

/** Previews an arbitrary reference (e.g. from manual Bible search) with no
 * suggestion involved. `translationId` defaults as above. */
export function previewScripture(
  reference: string,
  translationId?: string,
): Promise<PresentationPreview> {
  return invokeCommand("preview_scripture", { reference, translationId: translationId ?? null });
}

/** `translationId` defaults as above. */
export function preparePresentation(
  suggestionId: string,
  translationId?: string,
): Promise<PresentationItem> {
  return invokeCommand("prepare_presentation", { suggestionId, translationId: translationId ?? null });
}

/** Creates a prepared presentation item directly from a reference, with no
 * suggestion or speech recognition involved - the manual fallback.
 * `translationId` defaults as above. */
export function createManualPresentation(
  reference: string,
  translationId?: string,
): Promise<PresentationItem> {
  return invokeCommand("create_manual_presentation", { reference, translationId: translationId ?? null });
}

/** What's currently prepared for the active service (never includes
 * cancelled items) - the Current Output panel's data source. */
export function listPreparedPresentations(): Promise<PresentationItem[]> {
  return invokeCommand("list_prepared_presentations");
}

/** Presentation History (Phase 3.6): every presentation item ever prepared
 * for `serviceId` (any status, any past or live service) - the History
 * view's data source. Unlike {@link listPreparedPresentations}, never
 * limited to the live service or to still-`Prepared` items. */
export function listPresentationHistory(serviceId: string): Promise<PresentationItem[]> {
  return invokeCommand("list_presentation_history", { serviceId });
}

export function getPresentationItem(itemId: string): Promise<PresentationItem> {
  return invokeCommand("get_presentation_item", { itemId });
}

/** Cancels ("retracts") a still-prepared item before it's ever displayed. */
export function cancelPresentation(itemId: string): Promise<PresentationItem> {
  return invokeCommand("cancel_presentation", { itemId });
}

// --- local presentation display -------------------------------------------
//
// The first real, local, on-screen output for a prepared presentation item
// - a dedicated Tauri window under direct operator control. Never anything
// automatic: only `displayPresentation` (an explicit operator click) may
// cross the Prepared -> Active boundary. See `docs/presentation.md`.

/** Opens (or focuses, if already open) `screen`'s presentation display
 * window - useful on its own for positioning it on a projector/second
 * monitor before anything is ready to show. Phase 3.10: `displayPresentation`
 * always opens `"stage"` itself when needed; `"confidence"`/`"lobby"` are
 * only ever opened via this command, on explicit operator request. */
export function openPresentationDisplay(screen: PresentationScreen): Promise<void> {
  return invokeCommand("open_presentation_display", { screen });
}

/** Which display screens currently exist, and which item (if any) is
 * currently `active` for the active service - call on mount to sync, never
 * assume from local state alone. */
export function getPresentationDisplayState(): Promise<PresentationDisplayState> {
  return invokeCommand("get_presentation_display_state");
}

/** Displays a still-`prepared` item for real: renders it, opens the
 * display window if needed, and only then marks it `active` - never the
 * other way around. This is the one explicit operator action that may
 * cross the Prepared -> Active boundary. */
export function displayPresentation(itemId: string): Promise<PresentationItem> {
  return invokeCommand("display_presentation", { itemId });
}

/** Stops whichever item is currently active, if any - blanks the display
 * window without closing it. Safe and idempotent when nothing is active
 * (resolves with `null`, never rejects for that reason). */
export function clearPresentationDisplay(): Promise<PresentationItem | null> {
  return invokeCommand("clear_presentation_display");
}

/** Closes `screen`'s presentation display window outright (as opposed to
 * `clearPresentationDisplay`, which blanks it but leaves it open). Only
 * stops the active item when `screen` was the last screen still open
 * (Phase 3.10) - closing one of several simultaneously open screens never
 * blanks the others. */
export function closePresentationDisplay(screen: PresentationScreen): Promise<void> {
  return invokeCommand("close_presentation_display", { screen });
}

/** Phase 3.8.3 TEMPORARY DIAGNOSTIC: routes a lifecycle checkpoint from the
 * display window's own frontend into the app's log output - the only way
 * to observe what a secondary webview's own JavaScript sees, since this
 * app has no devtools/logging plugin. Never rejects loudly enough to
 * disrupt the display itself - callers should not await this on the
 * critical rendering path. */
export function logDisplayDiagnostic(stage: string, detail: string): Promise<void> {
  return invokeCommand("log_display_diagnostic", { stage, detail });
}

// --- display registry (Phase 3.10.2) ---------------------------------------

/** Every physical monitor CIP currently detects, merged with any persisted
 * role assignment - the operator configuration UI's sync point. A monitor
 * whose assigned role's physical hardware is currently unplugged still
 * appears here (`connected: false`), so the operator can see and change
 * its assignment without it being plugged in. */
export function listDisplays(): Promise<Display[]> {
  return invokeCommand("list_displays");
}

/** Assigns `role` to `monitorId` and persists it immediately. The next
 * presentation window opened for a screen mapped to that role (see
 * `domain/presentation.ts`'s `DisplayRole` docs) is placed on this
 * monitor - existing open windows are not moved retroactively. */
export function assignDisplayRole(monitorId: string, role: DisplayRole): Promise<void> {
  return invokeCommand("assign_display_role", { monitorId, role });
}

// --- presentation router (Phase 3.10.3) -------------------------------------

/** Sets `screen`'s route mode to `"live"` (receives the live presentation
 * broadcast, the default) or `"held"` (frozen on whatever it currently
 * shows, until switched back). Switching an open screen back to `live`
 * catches it up immediately - no separate refresh needed. */
export function setScreenRouteMode(screen: PresentationScreen, mode: RouteMode): Promise<void> {
  return invokeCommand("set_screen_route_mode", { screen, mode });
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

/** The Bible Library's book browser (Phase 3.6): the canonical 66-book
 * order/testament, each only included if this translation actually has it
 * imported - see `commands::list_bible_books`'s docs (Rust). */
export function listBibleBooks(translationId?: string): Promise<BibleBook[]> {
  return invokeCommand("list_bible_books", { translationId: translationId ?? null });
}

// --- saved scriptures (Phase 3.6: Church Knowledge Libraries) ---------------

export function saveScripture(input: {
  translationId: string;
  book: string;
  chapter: number;
  verseStart: number;
  verseEnd?: number | null;
  referenceDisplay: string;
  note?: string | null;
}): Promise<SavedScripture> {
  return invokeCommand("save_scripture", {
    translationId: input.translationId,
    book: input.book,
    chapter: input.chapter,
    verseStart: input.verseStart,
    verseEnd: input.verseEnd ?? null,
    referenceDisplay: input.referenceDisplay,
    note: input.note ?? null,
  });
}

/** Every saved scripture, most recently saved first. */
export function listSavedScriptures(): Promise<SavedScripture[]> {
  return invokeCommand("list_saved_scriptures");
}

export function deleteSavedScripture(id: string): Promise<boolean> {
  return invokeCommand("delete_saved_scripture", { id });
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

/** Every song currently named in the acoustic manifest - reads the
 * on-disk manifest itself, not the currently-active recognizer's status
 * (see `AcousticEnrollment`'s own docs for why those two things can
 * differ). */
export function listAcousticEnrollments(): Promise<AcousticEnrollment[]> {
  return invokeCommand("list_acoustic_enrollments");
}

/** Enrolls one reference recording for real audio fingerprinting -
 * validates and copies `sourcePath`, then upserts the manifest entry for
 * `songId`. Mirrors `installWhisperModel`: never takes effect until CIP
 * restarts. */
export function enrollAcousticReference(
  songId: string,
  contentId: string,
  sourcePath: string,
): Promise<AcousticEnrollment> {
  return invokeCommand("enroll_acoustic_reference", { songId, contentId, sourcePath });
}

/** Removes one enrollment - the counterpart enrollAcousticReference never
 * had. Rejects if songId names no current enrollment. Like enrollment
 * itself, never takes effect until CIP restarts. */
export function removeAcousticReference(songId: string): Promise<void> {
  return invokeCommand("remove_acoustic_reference", { songId });
}

export function rejectMusicFinding(findingId: string): Promise<IntelligenceFinding> {
  return invokeCommand("reject_music_finding", { findingId });
}

/** Phase 8: replaces the operator's current OBS/vMix push targets outright -
 * `obs`/`vmix` each `null` disables that integration. Live-editable, no
 * restart required. */
export function setProductionIntegrationConfig(
  config: ProductionIntegrationConfigInput,
): Promise<void> {
  return invokeCommand("set_production_integration_config", { config });
}

export function getProductionIntegrationStatus(): Promise<ProductionIntegrationStatus> {
  return invokeCommand("get_production_integration_status", undefined);
}

/** Synchronous connection test - pushes a real, visible test string so the
 * operator can confirm the right source updated. Does not save the config. */
export function testObsConnection(target: ObsTargetConfig): Promise<void> {
  return invokeCommand("test_obs_connection", { target });
}

export function testVmixConnection(target: VmixTargetConfig): Promise<void> {
  return invokeCommand("test_vmix_connection", { target });
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

// --- sermon foundation (Phase 2.5, per the authoritative Phase 2 roadmap) --
//
// The durable entity/lifecycle layer beneath the semantic Sermon
// Intelligence commands above (built under this repository's earlier
// internal "Phase 2.3" label) - see `docs/sermon-foundation.md`. Every
// function here is an explicit operator action.

/** The current structural summary - active sermon and current section,
 * independent of any pending finding review state. Read-only, safe to poll. */
export function getSermonFoundationState(): Promise<SermonFoundationSummary> {
  return invokeCommand("get_sermon_foundation_state");
}

/** Starts a new sermon within the active service - begins delivering
 * immediately (no separate "planned" step in this phase's workflow),
 * automatically opening an `introduction` section. */
export function startSermon(title?: string): Promise<Sermon> {
  return invokeCommand("start_sermon", { title: title ?? null });
}

export function pauseSermon(): Promise<Sermon> {
  return invokeCommand("pause_sermon");
}

export function resumeSermon(): Promise<Sermon> {
  return invokeCommand("resume_sermon");
}

export function endSermon(): Promise<Sermon> {
  return invokeCommand("end_sermon");
}

/** Explicit operator correction/assignment of the active sermon's title -
 * calling this again later is how a title is corrected, not a separate
 * "correct" action. */
export function setSermonTitle(title: string): Promise<Sermon> {
  return invokeCommand("set_sermon_title", { title });
}

/** Explicit operator speaker assignment - never automatic/biometric
 * speaker recognition. */
export function assignSermonSpeaker(name: string, role: "primary" | "guest"): Promise<Sermon> {
  return invokeCommand("assign_sermon_speaker", { name, role });
}

/** Explicit operator section assignment - closes whatever section was
 * previously open and opens the new one; never inferred from transcript
 * content in this phase. */
export function changeSermonSection(kind: string, note?: string): Promise<SermonSection> {
  return invokeCommand("change_sermon_section", { kind, note: note ?? null });
}

/** Explicitly links an already-persisted transcript segment (from any
 * existing ingestion path) to the active sermon - never a second
 * transcript-creation path. */
export function linkTranscriptSegmentToSermon(transcriptSegmentId: string): Promise<SermonSegment> {
  return invokeCommand("link_transcript_segment_to_sermon", { transcriptSegmentId });
}

/** Every transcript segment linked to the active sermon, in link order. */
export function listSermonSegments(): Promise<SermonSegment[]> {
  return invokeCommand("list_sermon_segments");
}

/** Every section (open or closed) recorded for the active sermon, in the
 * order they were opened. */
export function listSermonSections(): Promise<SermonSection[]> {
  return invokeCommand("list_sermon_sections");
}

/** Sermons previously delivered in the active service, most recently
 * created first - the sermon-history counterpart to `listServiceHistory`. */
export function listSermonHistory(limit: number): Promise<Sermon[]> {
  return invokeCommand("list_sermon_history", { limit });
}

/** A single sermon by id, independent of whichever one (if any) is
 * currently active - the sermon archive's detail view. */
export function getSermon(sermonId: string): Promise<Sermon> {
  return invokeCommand("get_sermon", { sermonId });
}

/** Phase 3.9: assembles the currently-active sermon's already-captured
 * data (sections, findings, Bible suggestions, transcript, timeline) into
 * one read-only bundle - see `domain/sermon.ts`'s `SermonHarvest` docs.
 * Rejects if no sermon is currently active (mirrors every other
 * active-sermon-scoped command). */
export function harvestSermon(): Promise<SermonHarvest> {
  return invokeCommand("harvest_sermon");
}

// --- bible intelligence bridge (Phase 2.4) --------------------------------------
//
// Mirrors `analyzeSermonTranscript` exactly - the new bridge that makes a
// Bible-domain `IntelligenceFinding` reachable for cross-domain correlation
// (the live Scripture-detection workflow, unchanged, still runs through
// `processTestTranscript`/the real audio pipeline).

export function analyzeBibleTranscript(text: string): Promise<IntelligenceFinding[]> {
  return invokeCommand("analyze_bible_transcript", { text });
}

// --- cross-domain intelligence (Phase 2.4) --------------------------------------
//
// Read-only from the operator's perspective: `analyzeCrossDomain` is an
// explicit diagnostic/review action, never triggered automatically by a
// transcript segment arriving. Never prepares or projects a presentation
// item - see `docs/cross-domain-intelligence.md`.

/** Run the correlation engine against this app's real, current findings
 * and queue any new correlations - an explicit operator/diagnostic action. */
export function analyzeCrossDomain(): Promise<IntelligenceCorrelation[]> {
  return invokeCommand("analyze_cross_domain");
}

/** Cross-domain correlations still awaiting an operator decision, for the
 * active service - the Cross-Domain Intelligence panel's data source. */
export function listCrossDomainCorrelations(): Promise<IntelligenceCorrelation[]> {
  return invokeCommand("list_cross_domain_correlations");
}

/** Informational-only operator review - never a required step before
 * `dismissCrossDomainCorrelation`. */
export function reviewCrossDomainCorrelation(correlationId: string): Promise<IntelligenceCorrelation> {
  return invokeCommand("review_cross_domain_correlation", { correlationId });
}

/** Explicit operator dismissal - never automatic, and has no way to alter
 * the source findings, the transcript, or the active Scripture context. */
export function dismissCrossDomainCorrelation(correlationId: string): Promise<IntelligenceCorrelation> {
  return invokeCommand("dismiss_cross_domain_correlation", { correlationId });
}

// --- content intelligence (Phase 2.7, per the authoritative Phase 2 roadmap) --
//
// The `ContentCandidate` counterpart to the cross-domain correlation
// commands above. Read-only from the operator's perspective:
// `analyzeContentIntelligence` is an explicit diagnostic/review action,
// never triggered automatically by a transcript segment arriving. Never
// prepares or projects a presentation item, never publishes or schedules
// anything - see `docs/content-intelligence.md`.

/** Run the Phase 2.7 content-intelligence layer against this app's real,
 * current findings and queue any new candidates - an explicit operator/
 * diagnostic action. */
export function analyzeContentIntelligence(): Promise<ContentCandidate[]> {
  return invokeCommand("analyze_content_intelligence");
}

/** Content candidates still awaiting an operator decision, for the active
 * service - the Content Intelligence panel's data source. */
export function listContentCandidates(): Promise<ContentCandidate[]> {
  return invokeCommand("list_content_candidates");
}

/** Phase 3.0: content candidates the operator has already accepted, for
 * the active service - the "Saved Content" view's data source. Before this
 * existed, accepting a candidate made its text permanently unreachable in
 * the running UI (see docs/phase-3-first-use.md). */
export function listAcceptedContentCandidates(): Promise<ContentCandidate[]> {
  return invokeCommand("list_accepted_content_candidates");
}

/** Explicit operator acceptance of a content opportunity - changes only
 * the candidate's own status; never publishes, schedules, or projects it. */
export function acceptContentCandidate(candidateId: string): Promise<ContentCandidate> {
  return invokeCommand("accept_content_candidate", { candidateId });
}

/** Explicit operator rejection - never automatic, and has no way to alter
 * the source finding, the transcript, or the active Scripture context. */
export function rejectContentCandidate(candidateId: string): Promise<ContentCandidate> {
  return invokeCommand("reject_content_candidate", { candidateId });
}

/** Phase 2.7.1: every content candidate saved (accepted) for one service,
 * most recently saved first - durable across a service ending and an
 * application restart, unlike `listAcceptedContentCandidates` (which only
 * ever reads the in-memory, currently-active-service queue). The
 * History view's "Saved Content" section's data source. */
export function listSavedContent(serviceId: string): Promise<ContentCandidate[]> {
  return invokeCommand("list_saved_content", { serviceId });
}

// --- service intelligence (Phase 2.4, per the authoritative Phase 2 roadmap) --
//
// Distinct from the cross-domain correlation commands above - see
// `domain/service.ts`'s own docs. Deliberately manual-command-only for
// `analyzeServiceTranscript`, mirroring `analyzeSermonTranscript`.

export function analyzeServiceTranscript(text: string): Promise<IntelligenceFinding[]> {
  return invokeCommand("analyze_service_transcript", { text });
}

/** Read-only current phase/transition-count/transcript-freshness snapshot -
 * safe to poll at any time. */
export function getServiceIntelligenceState(): Promise<ServiceIntelligenceSummary> {
  return invokeCommand("get_service_intelligence_state");
}

/** Every recorded phase transition for the active service, oldest first -
 * a history view, not an operator-review queue. */
export function listServiceTransitions(): Promise<IntelligenceFinding[]> {
  return invokeCommand("list_service_transitions");
}

/** Anomaly findings still awaiting an operator decision. */
export function listServiceAnomalies(): Promise<IntelligenceFinding[]> {
  return invokeCommand("list_service_anomalies");
}

/** Explicit operator declaration of the current service phase - for when
 * nothing has been detected yet, or the operator wants to proactively
 * state the phase. */
export function markServicePhase(phase: string, note?: string): Promise<IntelligenceFinding> {
  return invokeCommand("mark_service_phase", { phase, note: note ?? null });
}

/** Explicit operator correction of an incorrect system-detected phase -
 * additionally supersedes (rejects, never deletes) any other still-
 * pending transition finding for this service. */
export function correctServicePhase(phase: string, note?: string): Promise<IntelligenceFinding> {
  return invokeCommand("correct_service_phase", { phase, note: note ?? null });
}

/** Explicit operator acknowledgment of an anomaly finding - reuses the
 * ordinary finding-accept lifecycle. */
export function acknowledgeServiceAnomaly(findingId: string): Promise<IntelligenceFinding> {
  return invokeCommand("acknowledge_service_anomaly", { findingId });
}

// --- live status --------------------------------------------------------------

export function getLiveStatus(): Promise<LiveStatus> {
  return invokeCommand("get_live_status");
}
