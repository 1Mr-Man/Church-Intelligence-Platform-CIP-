//! Tauri commands: the IPC surface the frontend calls.
//!
//! Every command validates its own input (empty strings, malformed ids)
//! before touching state, and returns `Result<T, AppError>` so failures
//! reach the frontend as a clear message rather than a panic - per the
//! "manual fallback" requirement, nothing here may crash the application.

use crate::access;
use crate::acoustic;
use crate::companion;
use crate::config::AppConfig;
use crate::content;
use crate::display_registry;
use crate::errors::AppError;
use crate::events::{emit, emit_to, AppEvent};
use crate::logging::LogCategory;
use crate::music;
use crate::persistence;
use crate::pipeline::handle_final_transcript;
use crate::presentation;
use crate::presentation_display;
use crate::presentation_router::{self, RouteMode};
use crate::production;
use crate::segmentation::TranscriptSegmenter;
use crate::sermon_foundation;
use crate::state::{AppState, DEFAULT_TRANSLATION_ID};
use crate::timeline::{self, TimelineEntry};
use cip_core_ai::{
    SpeechEngineError, Suggestion, SuggestionKind, SuggestionStatus, TranscriptSegment,
};
use cip_core_bible::{
    book_alias::BOOKS, check_bible_integrity, search_bible as dispatch_bible_search, BibleBook,
    BibleSearchResult, BibleTranslation, IntegrityReport, PartialScriptureReference, ReferenceKind,
    ScriptureContext, ScriptureContextManager, ScriptureReference,
};
use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use cip_core_content::{ContentMetadata, ContentRegistryError, ContentStatus, ContentType};
use cip_core_intelligence::{
    ContentCandidate, ContentIntelligenceEngine, ContextBounds, CrossDomainCorrelationEngine,
    EngineCapability, FindingStatus, IntelligenceContext, IntelligenceCorrelation,
    IntelligenceDomain, IntelligenceFinding, IntelligenceInput, QueueAddOutcome, ServicePhase,
};
use cip_core_music::{search_songs, MatchThresholds, MusicQuery, SongRecognitionCandidate};
use cip_core_presentation::{PresentationContent, PresentationItem, PresentationItemStatus};
use cip_core_sermon::foundation::{
    is_valid_transition, SectionOrigin, Sermon, SermonSection, SermonSectionKind, SermonSegment,
    SermonStatus, Speaker, SpeakerRole,
};
use cip_core_service::{
    AudioChunk, AudioChunkSink, AudioDevice, AudioEngineStatus, ScriptureDetection, ServiceSession,
    ServiceStatus,
};
use cip_integrations_bible::{BibleDatasetInput, ImportReport};
use cip_integrations_music::{ImportReport as MusicImportReport, MusicDatasetInput};
use cip_presentation_renderer::{render_content, RenderedSlide, SCRIPTURE_DEFAULT_TEMPLATE};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub database_connected: bool,
    pub applied_migrations: usize,
    pub environment: crate::config::AppEnvironment,
}

/// Log an [`AppError`] under its category, then return it unchanged so
/// `?` in a command body both logs and propagates in one step.
fn log_and_return(err: AppError) -> AppError {
    log::error!(target: err.category().target(), "{err}");
    log::error!(target: LogCategory::Error.target(), "{err}");
    err
}

fn parse_uuid(value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value).map_err(|_| AppError::InvalidInput(format!("not a valid id: {value}")))
}

fn require_non_empty(value: &str, field: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(format!("{field} must not be empty")));
    }
    Ok(trimmed.to_string())
}

// --- Phase 10: Church/User Roles & Permissions ------------------------
//
// See docs/phase-10-audit.md/docs/roles-permissions.md for the full
// design record. `access::ensure_admin` is the pure, directly-testable
// gate (mirroring `ensure_ai_processing_permitted`, Phase 9); these two
// small wrappers exist only so every gated command below can write one
// short line instead of repeating the lock/clone/map_err boilerplate.

fn ensure_admin(state: &State<'_, AppState>) -> Result<(), AppError> {
    let current = state
        .current_operator
        .lock()
        .expect("current operator lock poisoned")
        .clone();
    access::ensure_admin(&current).map_err(AppError::Forbidden)
}

fn ensure_admin_string_err(state: &State<'_, AppState>) -> Result<(), String> {
    let current = state
        .current_operator
        .lock()
        .expect("current operator lock poisoned")
        .clone();
    access::ensure_admin(&current)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorAccountSummaryDto {
    pub id: String,
    pub display_name: String,
    pub role: cip_core_access::Role,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<&cip_core_access::OperatorAccount> for OperatorAccountSummaryDto {
    fn from(account: &cip_core_access::OperatorAccount) -> Self {
        Self {
            id: account.id.clone(),
            display_name: account.display_name.clone(),
            role: account.role,
            created_at: account.created_at,
        }
    }
}

/// Every operator account, oldest first - never includes `pin_hash`/
/// `pin_salt` (see `OperatorAccountSummaryDto`'s own docs). Available
/// without being logged in: the login screen itself needs this list to
/// render (or to know the store is empty and show account-creation
/// instead).
#[tauri::command]
pub fn list_operator_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<OperatorAccountSummaryDto>, AppError> {
    Ok(state
        .operator_account_store
        .list()
        .map_err(|e| AppError::InvalidInput(e.to_string()))
        .map_err(log_and_return)?
        .iter()
        .map(OperatorAccountSummaryDto::from)
        .collect())
}

/// Creates a new operator account - see `access::create_operator_account`
/// for the bootstrap rule (the very first account ever created becomes
/// Admin unconditionally; every account after that requires a logged-in
/// Admin).
#[tauri::command]
pub fn create_operator_account(
    display_name: String,
    pin: String,
    role: cip_core_access::Role,
    state: State<'_, AppState>,
) -> Result<OperatorAccountSummaryDto, AppError> {
    let current = state
        .current_operator
        .lock()
        .expect("current operator lock poisoned")
        .clone();
    let account = access::create_operator_account(
        state.operator_account_store.as_ref(),
        &current,
        &display_name,
        &pin,
        role,
    )
    .map_err(|e| match e {
        cip_core_access::AccessError::Forbidden(msg) => AppError::Forbidden(msg),
        other => AppError::InvalidInput(other.to_string()),
    })
    .map_err(log_and_return)?;
    Ok(OperatorAccountSummaryDto::from(&account))
}

/// Verifies `pin` against `accountId` and, on success, sets this
/// process's `current_operator` - see `access::login`'s own docs for why
/// a wrong PIN and an unknown account id return the same error text.
#[tauri::command]
pub fn login(
    account_id: String,
    pin: String,
    state: State<'_, AppState>,
) -> Result<OperatorAccountSummaryDto, AppError> {
    let account_id = require_non_empty(&account_id, "accountId").map_err(log_and_return)?;
    let session = access::login(state.operator_account_store.as_ref(), &account_id, &pin)
        .map_err(|e| AppError::Forbidden(e.to_string()))
        .map_err(log_and_return)?;
    let account = state
        .operator_account_store
        .get(&session.id)
        .map_err(|e| AppError::InvalidInput(e.to_string()))
        .map_err(log_and_return)?
        .ok_or_else(|| {
            log_and_return(AppError::Forbidden("incorrect account or PIN".to_string()))
        })?;
    *state
        .current_operator
        .lock()
        .expect("current operator lock poisoned") = Some(session);
    log::info!(
        target: LogCategory::Security.target(),
        "operator {} logged in ({:?})",
        account.display_name,
        account.role
    );
    Ok(OperatorAccountSummaryDto::from(&account))
}

/// Clears this process's `current_operator` - the operator must log in
/// again before any command (Admin-gated or not) that checks
/// `AppState.current_operator` behaves as "someone is logged in."
#[tauri::command]
pub fn logout(state: State<'_, AppState>) {
    let mut current = state
        .current_operator
        .lock()
        .expect("current operator lock poisoned");
    if let Some(session) = current.take() {
        log::info!(
            target: LogCategory::Security.target(),
            "operator {} logged out",
            session.display_name
        );
    }
}

#[tauri::command]
pub fn get_current_operator(state: State<'_, AppState>) -> Option<OperatorAccountSummaryDto> {
    let current = state
        .current_operator
        .lock()
        .expect("current operator lock poisoned")
        .clone()?;
    state
        .operator_account_store
        .get(&current.id)
        .ok()
        .flatten()
        .as_ref()
        .map(OperatorAccountSummaryDto::from)
}

/// Phase 11 (Local Congregant Companion View): the operator-facing
/// on/off/address status of the LAN companion server - see
/// `companion.rs`'s own docs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionStatusDto {
    pub running: bool,
    pub port: u16,
    pub urls: Vec<String>,
}

impl From<companion::CompanionStatus> for CompanionStatusDto {
    fn from(status: companion::CompanionStatus) -> Self {
        Self {
            running: status.running,
            port: status.port,
            urls: status.urls,
        }
    }
}

/// Starts the companion server - Admin-gated, joining the seven commands
/// Phase 10 already gates: turning on a LAN-listening server is a
/// configuration act, not day-to-day operation. Idempotent - calling it
/// while already running just returns the current status unchanged.
#[tauri::command]
pub fn enable_congregant_companion(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CompanionStatusDto, AppError> {
    ensure_admin(&state)?;
    let status = companion::enable(&app)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    log::info!(
        target: LogCategory::Network.target(),
        "congregant companion server enabled on port {}",
        status.port
    );
    Ok(status.into())
}

/// Stops the companion server - Admin-gated. A safe no-op if it wasn't
/// running.
#[tauri::command]
pub fn disable_congregant_companion(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CompanionStatusDto, AppError> {
    ensure_admin(&state)?;
    let status = companion::disable(&app);
    log::info!(
        target: LogCategory::Network.target(),
        "congregant companion server disabled"
    );
    Ok(status.into())
}

/// Current companion server status - available to any logged-in
/// operator (read-only, no gate), matching `get_current_operator`'s own
/// openness.
#[tauri::command]
pub fn get_congregant_companion_status(app: AppHandle) -> CompanionStatusDto {
    companion::status(&app).into()
}

/// Record one service-timeline entry (Phase 1.3), logging - never
/// propagating - a failure: a timeline write is a secondary effect of
/// whatever operator/pipeline action triggered it, and per section 26
/// ("do not crash the entire UI" on a persistence failure), losing one
/// audit row must never fail the primary action or leave the caller
/// unsure whether the *real* work (approving a suggestion, starting a
/// service, ...) succeeded.
fn record_timeline(
    conn: &Connection,
    service_id: Option<Uuid>,
    event: AppEvent,
    category: LogCategory,
    payload: impl Serialize,
) {
    if let Err(e) = timeline::record_event(conn, service_id, event, category, payload) {
        log::warn!(target: LogCategory::Database.target(), "failed to record timeline entry for {}: {e}", event.name());
    }
}

fn current_service_id(state: &State<'_, AppState>) -> Result<Uuid, AppError> {
    state
        .active_service
        .lock()
        .expect("active_service mutex poisoned")
        .as_ref()
        .map(|s| s.id)
        .ok_or(AppError::NoActiveService)
}

// --- Phase 1.3 lifecycle/workflow guards --------------------------------
//
// Pulled out as plain, `AppHandle`/`State`-free functions so they're
// directly unit-testable (see this module's test suite) - the same reason
// `parse_uuid`/`parse_display_reference`/`parse_suggestion_status` above
// are plain functions rather than being inlined into their one call site.
// This project has no `tauri::test` harness (see `docs/live-service.md`'s
// testing section for why), so every command's *decision logic* is kept
// here, testable independent of Tauri's `AppHandle`/`State` machinery,
// while the command function itself stays a thin wrapper that also
// touches state/emits events.

/// "Every service must have a distinct service ID" / "do not create a new
/// service when resuming": `start_service` may only run when no service
/// is currently tracked in `AppState` - see its own docs for why presence
/// alone (regardless of status) is the right check.
fn ensure_no_active_service(active: Option<&ServiceSession>) -> Result<(), AppError> {
    if active.is_some() {
        return Err(AppError::InvalidInput(
            "a service is already active - end it before starting a new one".to_string(),
        ));
    }
    Ok(())
}

/// `pause_service`/`resume_service`'s invalid-transition guard: pause only
/// from `Started`, resume only from `Paused`.
fn ensure_service_status(
    session: &ServiceSession,
    expected: ServiceStatus,
    action: &str,
) -> Result<(), AppError> {
    if session.status != expected {
        return Err(AppError::InvalidInput(format!(
            "cannot {action} a service that is not {expected:?} (current status: {:?})",
            session.status
        )));
    }
    Ok(())
}

/// `approve_suggestion`/`edit_suggestion`/`reject_suggestion`'s shared
/// guard: none of the three may act on a suggestion that has already left
/// the editable `Pending`/`Edited` states - an already-approved or
/// already-rejected suggestion is a closed decision, not something a
/// second action silently overwrites.
fn ensure_suggestion_editable(status: SuggestionStatus, action: &str) -> Result<(), AppError> {
    if !matches!(status, SuggestionStatus::Pending | SuggestionStatus::Edited) {
        return Err(AppError::InvalidInput(format!(
            "cannot {action} a suggestion with status {status:?}"
        )));
    }
    Ok(())
}

/// `preview_presentation`'s guard (Phase 1.4): unlike `prepare_presentation`,
/// preview is non-mutating and deliberately available before approval - the
/// whole point of separating it from the approval-gated prepare path (see
/// `docs/presentation.md`). A rejected suggestion is the one status that
/// makes no operational sense to preview: the operator has already said no.
fn ensure_suggestion_previewable(status: SuggestionStatus) -> Result<(), AppError> {
    if status == SuggestionStatus::Rejected {
        return Err(AppError::InvalidInput(
            "cannot preview a rejected suggestion".to_string(),
        ));
    }
    Ok(())
}

// --- foundation commands (Phase 1.0/1.1, unchanged) ------------------------

#[tauri::command]
pub fn get_app_config(state: State<'_, AppState>) -> AppConfig {
    state.config.clone()
}

#[tauri::command]
pub fn app_health_check(state: State<'_, AppState>) -> Result<HealthReport, AppError> {
    let db = state.db.lock().expect("db connection poisoned");
    let applied_migrations: usize = db
        .query_row("SELECT count(*) FROM schema_migrations", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|e| cip_database::DatabaseError::Migration(e.to_string()))
        .map_err(AppError::from)
        .map_err(log_and_return)? as usize;

    Ok(HealthReport {
        database_connected: true,
        applied_migrations,
        environment: state.config.environment,
    })
}

// --- Phase 3.2 pilot hardware diagnostics -----------------------------------
//
// A single, honest place for an operator (or whoever is setting CIP up for
// a pilot) to see exactly what CIP can currently detect about the
// hardware/model it depends on - never collapsed into one generic
// "unavailable" (spec: distinguish missing hardware, inaccessible
// hardware, configuration error, model missing/corrupt, runtime failure).
// Composes only already-existing state (`AppState.audio_engine`,
// `AppState.config.whisper_model_path`, Tauri's own monitor API) - no new
// engine, no new persisted state, no new dependency.

/// Which of four honestly-distinguishable states the configured Whisper
/// model path is currently in. Never claims more than a filesystem check
/// can prove: `Present` means a readable file exists at the path, not
/// that its *content* is a valid ggml/gguf model - only
/// `WhisperSpeechEngine::load` (called at real startup, when the
/// `whisper` feature is enabled) can prove that, since doing so requires
/// actually parsing the file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum WhisperModelDiagnostic {
    /// No file exists at the configured path at all.
    #[serde(rename_all = "camelCase")]
    Missing { expected_path: String },
    /// A path exists but this process could not open it for reading (a
    /// directory, a permissions error, or similar).
    #[serde(rename_all = "camelCase")]
    Unreadable { path: String, reason: String },
    /// A readable file exists at the configured path.
    #[serde(rename_all = "camelCase")]
    Present { path: String, size_bytes: u64 },
}

/// Pure, directly-testable classification - the part of
/// [`get_pilot_diagnostics`] worth testing without a real Tauri runtime.
fn diagnose_whisper_model(path: &std::path::Path) -> WhisperModelDiagnostic {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            return WhisperModelDiagnostic::Missing {
                expected_path: path.display().to_string(),
            }
        }
    };
    if !metadata.is_file() {
        return WhisperModelDiagnostic::Unreadable {
            path: path.display().to_string(),
            reason: "not a regular file".to_string(),
        };
    }
    match std::fs::File::open(path) {
        Ok(_) => WhisperModelDiagnostic::Present {
            path: path.display().to_string(),
            size_bytes: metadata.len(),
        },
        Err(e) => WhisperModelDiagnostic::Unreadable {
            path: path.display().to_string(),
            reason: e.to_string(),
        },
    }
}

/// Phase 3.8.7.1: lets an operator install a Whisper model file they've
/// already downloaded themselves (this build environment's own egress to
/// the standard model host is confirmed blocked - see
/// `docs/phase-3-8-7-1-audit.md` - but a real Windows machine typically
/// has ordinary internet access, so the operator downloading the file
/// directly and pointing CIP at it is the fastest real path to a working
/// model) without hand-editing a path or using a file manager to place it.
///
/// Never trusts the candidate file's name or extension: it validates by
/// actually attempting to load it as a real Whisper model - the exact
/// same [`cip_ai_speech::WhisperSpeechEngine::load`] call this
/// application itself uses at startup - so a renamed unrelated file, a
/// truncated download, or an HTML error page saved with a `.bin`
/// extension is rejected with the real underlying error, never silently
/// accepted. Only once that validation succeeds is the file copied into
/// place, atomically (written to a temp file in the destination
/// directory first, then renamed over the real path, so a crash or a
/// full disk mid-copy can never leave a half-written file where CIP
/// expects a real model).
///
/// Installing a model this way takes effect on CIP's **next launch** -
/// `AppState.speech_engine` is constructed once at startup
/// (`create_speech_engine`) and held for the life of the process, so this
/// command deliberately does not attempt to hot-swap the running engine;
/// the returned diagnostic reflects the file on disk, not the live
/// engine's state.
#[cfg(feature = "whisper")]
#[tauri::command]
pub fn install_whisper_model(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<WhisperModelDiagnostic, String> {
    ensure_admin_string_err(&state)?;
    let source = std::path::PathBuf::from(&source_path);
    let metadata =
        std::fs::metadata(&source).map_err(|e| format!("cannot read \"{source_path}\": {e}"))?;
    if !metadata.is_file() {
        return Err(format!("\"{source_path}\" is not a regular file"));
    }

    cip_ai_speech::WhisperSpeechEngine::load(&source).map_err(|e| {
        format!(
            "\"{source_path}\" did not load as a valid Whisper model ({e}) - \
             this is the same check CIP itself performs at startup, so this \
             file would not have worked even if installed"
        )
    })?;

    let dest = &state.config.whisper_model_path;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let tmp_dest = dest.with_extension("bin.installing");
    std::fs::copy(&source, &tmp_dest).map_err(|e| format!("could not copy model file: {e}"))?;
    std::fs::rename(&tmp_dest, dest)
        .map_err(|e| format!("could not finalize model install: {e}"))?;

    log::info!(
        target: LogCategory::Speech.target(),
        "installed Whisper model from {source_path} to {} - restart CIP for it to take effect",
        dest.display()
    );

    Ok(diagnose_whisper_model(dest))
}

/// Non-`whisper`-feature builds have no [`cip_ai_speech::WhisperSpeechEngine`]
/// to validate a candidate file against at all - honestly refuses rather
/// than installing an unverified file, mirroring `create_speech_engine`'s
/// own feature-gated behavior in `lib.rs`.
#[cfg(not(feature = "whisper"))]
#[tauri::command]
pub fn install_whisper_model(_source_path: String) -> Result<WhisperModelDiagnostic, String> {
    Err(
        "this build was not compiled with the `whisper` feature, so there is no speech engine \
         available to validate a model file against"
            .to_string(),
    )
}

// --- Phase 12: multi-language Whisper ---------------------------------------
//
// Which language a service is being preached in is a live-workflow choice
// (like selecting a Bible translation or the audio input device), not a
// system-configuration item - unlike Phase 10's seven Admin-gated commands,
// these are available to any logged-in operator. See `docs/phase-12-audit.md`
// for the full design record, including why Igbo is deliberately not offered.

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechLanguageOptionDto {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechLanguageCapabilitiesDto {
    pub current_language: String,
    pub supported_languages: Vec<SpeechLanguageOptionDto>,
    /// `None` until a model has actually loaded - see
    /// `state::SpeechDiagnostics::model_is_multilingual`'s own docs.
    pub model_is_multilingual: Option<bool>,
}

fn speech_language_capabilities(state: &State<'_, AppState>) -> SpeechLanguageCapabilitiesDto {
    let current_language = state
        .speech_language
        .lock()
        .expect("speech_language mutex poisoned")
        .clone();
    let model_is_multilingual = state
        .speech_diagnostics
        .lock()
        .expect("speech_diagnostics mutex poisoned")
        .model_is_multilingual;
    SpeechLanguageCapabilitiesDto {
        current_language,
        supported_languages: cip_ai_speech::SUPPORTED_LANGUAGES
            .iter()
            .map(|(code, name)| SpeechLanguageOptionDto {
                code: code.to_string(),
                name: name.to_string(),
            })
            .collect(),
        model_is_multilingual,
    }
}

/// Current speech-language selection and what CIP actually supports -
/// available without being logged in as Admin, matching this command
/// family's own "live-workflow, not configuration" reasoning above.
#[tauri::command]
pub fn get_speech_language_capabilities(
    state: State<'_, AppState>,
) -> SpeechLanguageCapabilitiesDto {
    speech_language_capabilities(&state)
}

/// Selects the language the speech engine's *next* inference pass should
/// condition on - takes effect immediately (no restart), applied via
/// `SpeechEngine::set_language`. Rejects any code outside
/// `cip_ai_speech::SUPPORTED_LANGUAGES` rather than silently ignoring or
/// passing through an unverified one.
#[tauri::command]
pub fn set_speech_language(
    language: String,
    state: State<'_, AppState>,
) -> Result<SpeechLanguageCapabilitiesDto, AppError> {
    if !cip_ai_speech::is_supported_language(&language) {
        return Err(log_and_return(AppError::InvalidInput(format!(
            "unsupported speech language \"{language}\" - see get_speech_language_capabilities \
             for the supported set"
        ))));
    }
    *state
        .speech_language
        .lock()
        .expect("speech_language mutex poisoned") = language.clone();
    state
        .speech_engine
        .lock()
        .expect("speech_engine mutex poisoned")
        .set_language(&language);
    log::info!(
        target: LogCategory::Speech.target(),
        "speech transcription language set to \"{language}\""
    );
    Ok(speech_language_capabilities(&state))
}

// --- Phase 4.4: semantic (embedding-based) Bible search --------------------
//
// Mirrors the Whisper model-provisioning pattern immediately above exactly:
// an operator-supplied file, installed by copying (never downloaded), with
// an honest per-file diagnostic. Two files are required together (model
// weights + tokenizer - see `cip_ai_embeddings::CandleEmbeddingEngine`'s own
// docs), so `EmbeddingCapabilities` reports both independently rather than
// collapsing them into one status.

/// Reuses `WhisperModelDiagnostic`'s exact shape/semantics for a single
/// embedding-related file (model weights or tokenizer) - see that type's
/// own doc comment for what each variant does and does not prove.
pub type EmbeddingFileDiagnostic = WhisperModelDiagnostic;

/// Everything an operator needs to know about Phase 4.4's semantic search
/// readiness in one call - mirrors `PilotDiagnostics.speech`'s role for
/// Whisper. `verse_embedding_coverage` is `None` whenever the engine isn't
/// ready (nothing to count against - "already embedded, ready to search"
/// requires a real model_id to key by, not the shape of an
/// engine-independent count).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingCapabilities {
    pub feature_compiled: bool,
    pub model_load_attempted: bool,
    pub model_loaded: bool,
    pub model_load_error: Option<String>,
    pub engine_ready: bool,
    pub model_id: String,
    pub dimensions: usize,
    pub model_file: EmbeddingFileDiagnostic,
    pub tokenizer_file: EmbeddingFileDiagnostic,
    /// `(embedded, total)` verse counts for `DEFAULT_TRANSLATION_ID` under
    /// the engine's current `model_id` - `None` when the engine isn't
    /// ready (see this struct's own doc comment).
    pub verse_embedding_coverage: Option<(u64, u64)>,
}

#[tauri::command]
pub fn get_embedding_capabilities(
    state: State<'_, AppState>,
) -> Result<EmbeddingCapabilities, String> {
    let diagnostics = state
        .embedding_diagnostics
        .lock()
        .expect("embedding diagnostics lock poisoned")
        .clone();
    let engine = state
        .embedding_engine
        .lock()
        .expect("embedding engine lock poisoned");

    let verse_embedding_coverage = if state.embedding_ready {
        let conn = state
            .verse_embedding_store
            .connection()
            .lock()
            .expect("verse embedding store connection poisoned");
        crate::embeddings::embedding_coverage(
            &conn,
            &resolve_default_translation_id(&state),
            engine.model_id(),
        )
        .ok()
    } else {
        None
    };

    Ok(EmbeddingCapabilities {
        feature_compiled: diagnostics.feature_compiled,
        model_load_attempted: diagnostics.model_load_attempted,
        model_loaded: diagnostics.model_loaded,
        model_load_error: diagnostics.model_load_error,
        engine_ready: state.embedding_ready,
        model_id: engine.model_id().to_string(),
        dimensions: engine.dimensions(),
        model_file: diagnose_whisper_model(&state.config.embedding_model_path),
        tokenizer_file: diagnose_whisper_model(&state.config.embedding_tokenizer_path),
        verse_embedding_coverage,
    })
}

/// Copies `source_path` to `dest`, atomically (temp file in the
/// destination directory, then renamed over the real path) - mirrors
/// `install_whisper_model`'s own copy step exactly, factored out since
/// Phase 4.4 needs it twice (model weights, tokenizer) instead of once.
fn install_file_at(source_path: &str, dest: &std::path::Path) -> Result<(), String> {
    let source = std::path::PathBuf::from(source_path);
    let metadata =
        std::fs::metadata(&source).map_err(|e| format!("cannot read \"{source_path}\": {e}"))?;
    if !metadata.is_file() {
        return Err(format!("\"{source_path}\" is not a regular file"));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let tmp_dest = dest.with_extension(format!(
        "{}.installing",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
    ));
    std::fs::copy(&source, &tmp_dest).map_err(|e| format!("could not copy file: {e}"))?;
    std::fs::rename(&tmp_dest, dest).map_err(|e| format!("could not finalize install: {e}"))?;
    Ok(())
}

/// Installs an operator-supplied embedding model weights file
/// (`model.safetensors`) at the configured path. Unlike
/// `install_whisper_model`, this cannot validate the file by itself - a
/// `CandleEmbeddingEngine` needs *both* the model and tokenizer together
/// (see that type's own docs) - so it only copies the file into place;
/// callers refresh via `get_embedding_capabilities` afterward to see
/// whether the pair is now loadable. Like `install_whisper_model`, this
/// takes effect on CIP's **next launch**, never a hot-swap of the running
/// engine.
#[tauri::command]
pub fn install_embedding_model_file(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<EmbeddingFileDiagnostic, String> {
    ensure_admin_string_err(&state)?;
    install_file_at(&source_path, &state.config.embedding_model_path)?;
    log::info!(
        target: LogCategory::Bible.target(),
        "installed embedding model weights from {source_path} - restart CIP for it to take effect"
    );
    Ok(diagnose_whisper_model(&state.config.embedding_model_path))
}

/// Installs an operator-supplied tokenizer file (`tokenizer.json`) at the
/// configured path - the counterpart to `install_embedding_model_file`,
/// see its docs for the same caveats.
#[tauri::command]
pub fn install_embedding_tokenizer_file(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<EmbeddingFileDiagnostic, String> {
    ensure_admin_string_err(&state)?;
    install_file_at(&source_path, &state.config.embedding_tokenizer_path)?;
    log::info!(
        target: LogCategory::Bible.target(),
        "installed embedding tokenizer from {source_path} - restart CIP for it to take effect"
    );
    Ok(diagnose_whisper_model(
        &state.config.embedding_tokenizer_path,
    ))
}

// --- Phase 7.2: real audio fingerprinting enrollment -----------------------
//
// Mirrors the Whisper/embedding model-provisioning pattern immediately
// above exactly: an operator-supplied file, installed by copying (never
// downloaded/recorded), never taking effect until CIP restarts (see
// `create_acoustic_recognizer` in `lib.rs` - `AppState.acoustic_recognizer`
// is built once at startup, exactly like the speech/embedding engines).
// The one real difference from a single fixed-path model file is that the
// acoustic manifest holds N entries (one per enrolled song), so these
// commands read-modify-write `cip_integrations_music_acoustic`'s manifest
// rather than overwriting one path.

/// One entry in the acoustic manifest, as the frontend sees it - mirrors
/// `cip_integrations_music_acoustic::ManifestSong` field-for-field (a
/// thin re-declaration, not a re-export, so this crate's public API
/// surface stays in `commands.rs` alongside every other command type).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcousticEnrollment {
    pub song_id: String,
    pub content_id: String,
    pub audio_path: String,
}

impl From<cip_integrations_music_acoustic::ManifestSong> for AcousticEnrollment {
    fn from(entry: cip_integrations_music_acoustic::ManifestSong) -> Self {
        Self {
            song_id: entry.song_id,
            content_id: entry.content_id,
            audio_path: entry.audio_path,
        }
    }
}

/// Every song currently named in the acoustic manifest - a read of the
/// manifest file itself, not of `AppState.acoustic_recognizer`'s live
/// status: an operator can enroll several songs across multiple calls to
/// `enroll_acoustic_reference` before ever restarting, and this command
/// lets the UI show that in-progress list even though none of it is
/// active yet (see `AcousticEngineStatus`/`get_live_status` for what
/// *is* currently active).
#[tauri::command]
pub fn list_acoustic_enrollments(
    state: State<'_, AppState>,
) -> Result<Vec<AcousticEnrollment>, AppError> {
    let entries =
        cip_integrations_music_acoustic::read_manifest_entries(&state.config.acoustic.model_dir)
            .map_err(AppError::InvalidInput)
            .map_err(log_and_return)?;
    Ok(entries.into_iter().map(AcousticEnrollment::from).collect())
}

/// Enrolls one reference recording for real audio fingerprinting:
/// validates `source_path` is a usable WAV file (the exact same check
/// the recognizer itself performs at startup - see
/// `cip_integrations_music_acoustic::validate_reference_wav`'s docs),
/// copies it into the acoustic model directory, and upserts the manifest
/// entry for `song_id` (replacing any prior enrollment of the same song,
/// never leaving two stale entries for one song behind). Like every
/// other model-provisioning command in this file, this never takes
/// effect until CIP restarts.
#[tauri::command]
pub fn enroll_acoustic_reference(
    song_id: String,
    content_id: String,
    source_path: String,
    state: State<'_, AppState>,
) -> Result<AcousticEnrollment, AppError> {
    let song_id = require_non_empty(&song_id, "songId")
        .map_err(log_and_return)?
        .to_string();
    let content_id = require_non_empty(&content_id, "contentId")
        .map_err(log_and_return)?
        .to_string();

    let source = std::path::PathBuf::from(&source_path);
    cip_integrations_music_acoustic::validate_reference_wav(&source)
        .map_err(|e| {
            AppError::InvalidInput(format!(
                "\"{source_path}\" is not a usable reference recording: {e}"
            ))
        })
        .map_err(log_and_return)?;

    let model_dir = &state.config.acoustic.model_dir;
    std::fs::create_dir_all(model_dir)
        .map_err(|e| {
            AppError::InvalidInput(format!("could not create {}: {e}", model_dir.display()))
        })
        .map_err(log_and_return)?;
    let audio_filename = format!("{song_id}.wav");
    let dest = model_dir.join(&audio_filename);
    let tmp_dest = model_dir.join(format!("{song_id}.wav.installing"));
    std::fs::copy(&source, &tmp_dest)
        .map_err(|e| AppError::InvalidInput(format!("could not copy reference recording: {e}")))
        .map_err(log_and_return)?;
    std::fs::rename(&tmp_dest, &dest)
        .map_err(|e| AppError::InvalidInput(format!("could not finalize enrollment: {e}")))
        .map_err(log_and_return)?;

    let mut entries = cip_integrations_music_acoustic::read_manifest_entries(model_dir)
        .map_err(AppError::InvalidInput)
        .map_err(log_and_return)?;
    entries.retain(|e| e.song_id != song_id);
    let entry = cip_integrations_music_acoustic::ManifestSong {
        song_id: song_id.clone(),
        content_id: content_id.clone(),
        audio_path: audio_filename.clone(),
    };
    entries.push(entry.clone());
    cip_integrations_music_acoustic::write_manifest_entries(model_dir, &entries)
        .map_err(AppError::InvalidInput)
        .map_err(log_and_return)?;

    log::info!(
        target: LogCategory::Music.target(),
        "enrolled acoustic reference recording for song {song_id} ({content_id}) from {source_path} - restart CIP for it to take effect"
    );

    Ok(AcousticEnrollment::from(entry))
}

/// Removes one enrollment (Phase 7.3) - the counterpart
/// `enroll_acoustic_reference` never had: an operator who enrolled the
/// wrong song, a bad recording, or simply no longer wants a song
/// fingerprinted had no way to undo it short of editing files inside the
/// app's data directory by hand. Errors honestly if `song_id` names no
/// current enrollment, rather than silently succeeding. The manifest
/// entry is what actually matters for recognition, so removing it is
/// unconditional; deleting the now-orphaned audio file from disk is
/// best-effort cleanup and never fails the command - a leftover WAV file
/// is untidy, not incorrect, and must never block the operator from
/// fixing the manifest.
#[tauri::command]
pub fn remove_acoustic_reference(
    song_id: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let song_id = require_non_empty(&song_id, "songId")
        .map_err(log_and_return)?
        .to_string();

    let model_dir = &state.config.acoustic.model_dir;
    let mut entries = cip_integrations_music_acoustic::read_manifest_entries(model_dir)
        .map_err(AppError::InvalidInput)
        .map_err(log_and_return)?;
    let Some(removed) = entries.iter().position(|e| e.song_id == song_id) else {
        return Err(log_and_return(AppError::InvalidInput(format!(
            "no enrollment found for song \"{song_id}\""
        ))));
    };
    let removed_entry = entries.remove(removed);
    cip_integrations_music_acoustic::write_manifest_entries(model_dir, &entries)
        .map_err(AppError::InvalidInput)
        .map_err(log_and_return)?;

    let audio_path = model_dir.join(&removed_entry.audio_path);
    match std::fs::remove_file(&audio_path) {
        Ok(()) => {}
        Err(e) => log::warn!(
            target: LogCategory::Music.target(),
            "removed enrollment for song {song_id} but could not delete its reference audio file at {}: {e} (not fatal - the manifest no longer references it)",
            audio_path.display()
        ),
    }

    log::info!(
        target: LogCategory::Music.target(),
        "removed acoustic reference recording for song {song_id} - restart CIP for it to take effect"
    );

    Ok(())
}

// --- Phase 8: Production Integration (OBS/vMix) ----------------------------

/// Wire-format mirror of [`cip_integrations_obs::ObsTarget`] - the crate
/// type itself carries no `serde` dependency (it has no reason to know
/// about JSON), so this DTO is the one place that boundary is crossed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObsTargetConfig {
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
    pub source_name: String,
}

impl From<ObsTargetConfig> for cip_integrations_obs::ObsTarget {
    fn from(c: ObsTargetConfig) -> Self {
        cip_integrations_obs::ObsTarget {
            host: c.host,
            port: c.port,
            password: c.password,
            source_name: c.source_name,
        }
    }
}

/// Wire-format mirror of [`cip_integrations_vmix::VmixTarget`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmixTargetConfig {
    pub host: String,
    pub port: u16,
    pub input: String,
    pub selected_name: Option<String>,
}

impl From<VmixTargetConfig> for cip_integrations_vmix::VmixTarget {
    fn from(c: VmixTargetConfig) -> Self {
        cip_integrations_vmix::VmixTarget {
            host: c.host,
            port: c.port,
            input: c.input,
            selected_name: c.selected_name,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionIntegrationConfigInput {
    pub obs: Option<ObsTargetConfig>,
    pub vmix: Option<VmixTargetConfig>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushOutcomeDto {
    pub success: bool,
    pub error_text: Option<String>,
    pub at: chrono::DateTime<chrono::Utc>,
}

impl From<production::PushOutcome> for PushOutcomeDto {
    fn from(o: production::PushOutcome) -> Self {
        PushOutcomeDto {
            success: o.success,
            error_text: o.error_text,
            at: o.at,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionIntegrationStatusDto {
    pub obs_last_push: Option<PushOutcomeDto>,
    pub vmix_last_push: Option<PushOutcomeDto>,
}

/// Replaces the operator's current OBS/vMix push targets outright -
/// `obs`/`vmix` each `None` disables that integration. Live-editable, no
/// restart required (see `production.rs`'s own docs for why this differs
/// from every restart-required model-provisioning command in this
/// codebase).
#[tauri::command]
pub fn set_production_integration_config(
    config: ProductionIntegrationConfigInput,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    ensure_admin(&state).map_err(log_and_return)?;
    let obs_configured = config.obs.is_some();
    let vmix_configured = config.vmix.is_some();
    let new_config = production::ProductionIntegrationConfig {
        obs: config.obs.map(Into::into),
        vmix: config.vmix.map(Into::into),
    };
    *state
        .production_integration_config
        .lock()
        .expect("production_integration_config mutex poisoned") = new_config;
    log::info!(
        target: LogCategory::Network.target(),
        "production integration config updated: obs={obs_configured} vmix={vmix_configured}"
    );
    Ok(())
}

/// The most recent OBS/vMix push outcome, if any push has been attempted
/// this session - lets the operator see a failure without waiting for
/// the next live/replayed verse.
#[tauri::command]
pub fn get_production_integration_status(
    state: State<'_, AppState>,
) -> ProductionIntegrationStatusDto {
    let status = state
        .production_integration_status
        .lock()
        .expect("production_integration_status mutex poisoned")
        .clone();
    ProductionIntegrationStatusDto {
        obs_last_push: status.obs_last_push.map(Into::into),
        vmix_last_push: status.vmix_last_push.map(Into::into),
    }
}

/// Synchronous connection test, called directly from an operator's "Test
/// Connection" button press - pushes a real, visible test string so the
/// operator can confirm the right source updated, not just that a socket
/// opened. Does not touch `production_integration_config`; the operator
/// must still save the config for it to be used by live pushes.
#[tauri::command]
pub fn test_obs_connection(target: ObsTargetConfig) -> Result<(), AppError> {
    production::test_obs_connection(&target.into())
        .map_err(AppError::InvalidInput)
        .map_err(log_and_return)
}

#[tauri::command]
pub fn test_vmix_connection(target: VmixTargetConfig) -> Result<(), AppError> {
    production::test_vmix_connection(&target.into())
        .map_err(AppError::InvalidInput)
        .map_err(log_and_return)
}

/// Embeds every not-yet-embedded verse of `DEFAULT_TRANSLATION_ID` using
/// the currently loaded embedding engine - the explicit, operator-triggered
/// action that populates `bible_verse_embeddings` (nothing does this
/// automatically; see `docs/phase-4-4-semantic-bible-search.md`).
/// Idempotent/resumable: re-running only ever embeds verses still missing
/// under the engine's current `model_id` (see
/// `embeddings::generate_verse_embeddings_for_translation`'s own docs).
///
/// A known, documented limitation: this runs synchronously on the calling
/// command thread and reports no incremental progress while it runs - for
/// a full-Bible translation on CPU this can take minutes. Tauri dispatches
/// command handlers off its main UI thread, so the application does not
/// freeze while this runs, but the frontend has no progress signal until
/// it completes; a future phase could add progress events if this proves
/// too opaque in practice.
///
/// Resolves the real default translation the same way twelve other
/// commands already do (`resolve_default_translation_id` - real BSB
/// production id first, the KJV dev-fixture id only as a fallback), fixing
/// a Phase 9 finding: this command previously hardcoded the literal
/// `DEFAULT_TRANSLATION_ID`, which is never registered in a real
/// production build, silently making this command a no-op against BSB.
///
/// Phase 9's licensing gate: refuses to run unless the resolved
/// translation's Content Registry record explicitly grants
/// `ai_processing_allowed` (see `ensure_ai_processing_permitted`'s docs) -
/// the real enforcement point the Bible Translation Registry's usage
/// permissions exist to protect.
#[tauri::command]
pub fn generate_verse_embeddings(
    state: State<'_, AppState>,
) -> Result<crate::embeddings::EmbeddingGenerationSummary, String> {
    ensure_admin_string_err(&state)?;
    if !state.embedding_ready {
        return Err(
            "no embedding model is loaded - install a model weights + tokenizer file pair and \
             restart CIP before generating verse embeddings"
                .to_string(),
        );
    }
    let translation_id = resolve_default_translation_id(&state);
    ensure_ai_processing_permitted(state.content_registry.as_ref(), &translation_id)?;
    let engine = state
        .embedding_engine
        .lock()
        .expect("embedding engine lock poisoned");
    crate::embeddings::generate_verse_embeddings_for_translation(
        state.verse_embedding_store.connection(),
        engine.as_ref(),
        &translation_id,
    )
    .map_err(|e| e.to_string())
}

/// Refuses to run AI/embedding processing over a translation's Bible text
/// unless its Content Registry record explicitly grants
/// `ai_processing_allowed` (`cip_core_content::UsagePermissions`) - the
/// real enforcement point the Bible Translation Registry's licensing
/// metadata exists to protect (see docs/bible-translation-registry.md and
/// docs/phase-9-audit.md).
///
/// Deliberately the OPPOSITE default from `ensure_translation_selectable`,
/// on purpose: a missing/unknown registration there still lets an
/// operator browse/search/display a translation (failing open protects
/// against blocking legitimate content by a bookkeeping gap that has
/// nothing to do with rights), but sending a translation's text into an
/// AI model is exactly the class of action `LicensingStatus`'s own "never
/// assume permissive" doctrine exists for - so this fails CLOSED: no
/// registry entry, or a registry entry that never explicitly recorded
/// `ai_processing_allowed = true`, is refused, never silently allowed
/// through. Split out as a pure function (real in-memory SQLite registry,
/// no `State`) for the same reason `resolve_default_translation_id_from_registry`
/// is: this project has no `tauri::test` harness.
fn ensure_ai_processing_permitted(
    registry: &dyn cip_core_content::ContentRegistry,
    translation_id: &str,
) -> Result<(), String> {
    let metadata = registry
        .get(&content::bible_content_id(translation_id))
        .map_err(|e| format!("content registry error: {e}"))?;
    match metadata {
        Some(m) if m.usage.permits_ai_processing() => Ok(()),
        Some(_) => Err(format!(
            "translation {translation_id:?} has not been explicitly marked \
             ai_processing_allowed in the Bible Translation Registry - refusing to generate \
             embeddings from its text until that permission is recorded (see \
             docs/bible-translation-registry.md)"
        )),
        None => Err(format!(
            "translation {translation_id:?} is not registered in the Content Registry - \
             refusing to generate embeddings from its text"
        )),
    }
}

/// One physical (or virtual, e.g. Xvfb) display this process can detect -
/// via Tauri's own `AppHandle::available_monitors`, not a new dependency.
/// Software display-window logic and physical display *readability* are
/// two different things (spec section 11/29): this only ever reports
/// what the OS/windowing layer reports exists, never whether a human
/// could actually read text on it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayDiagnostic {
    pub name: Option<String>,
    pub width_px: u32,
    pub height_px: u32,
    /// Phase 3.4: this display's top-left position in the OS's virtual
    /// screen coordinate space - the piece of Windows multi-monitor
    /// layout (primary + a second display extended to one side) that
    /// width/height alone can't show.
    pub position_x: i32,
    pub position_y: i32,
    pub scale_factor: f64,
    pub is_primary: bool,
}

/// Phase 3.3: which machine/build produced this diagnostic report - the
/// minimum an evidence record needs to be attributable to a specific
/// pilot machine and a specific release. `build_commit` is embedded at
/// compile time by `build.rs` (a build-time-only `git rev-parse`, never a
/// runtime process spawn) and reads `"unknown"` for a build that wasn't
/// made from a git checkout (e.g. a source tarball) - never fabricated.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineDiagnostic {
    pub os: String,
    pub arch: String,
    pub cip_version: String,
    pub build_commit: String,
    /// Phase 3.8.7: whether `git status --porcelain` reported uncommitted
    /// changes at build time - this project's own workflow always builds
    /// and verifies an artifact before committing the changes that
    /// produced it, so `build_commit` alone is routinely one phase behind
    /// a freshly built binary. `true` here means "built from `build_commit`
    /// plus uncommitted work", not "built from `build_commit` exactly".
    pub build_dirty: bool,
}

/// Phase 3.3: is the database file actually writable/readable right now -
/// distinct from "did migrations apply" (`app_health_check` already
/// answers that). A real disk-full or permissions problem on the pilot
/// machine would show up here even if the connection that opened the app
/// still works.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseDiagnostic {
    pub path: String,
    pub readable: bool,
    pub writable: bool,
}

/// Phase 3.8.6: what the running process actually observed about the
/// speech pipeline - distinct from `WhisperModelDiagnostic` (a filesystem
/// check anyone can run without starting the engine) in that
/// `model_loaded` is only ever `true` after `WhisperSpeechEngine::load`
/// genuinely parsed the file and initialized a whisper.cpp context. Every
/// field here mirrors `state::SpeechDiagnostics` one-to-one - see that
/// struct's own docs for what each one means and where it's set.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechRuntimeDiagnostics {
    pub feature_compiled: bool,
    pub model_load_attempted: bool,
    pub model_loaded: bool,
    pub model_load_error: Option<String>,
    pub engine_ready: bool,
    pub chunks_received: u64,
    pub last_chunk_sample_rate_hz: Option<u32>,
    pub last_chunk_sample_count: Option<usize>,
    pub last_resampled_sample_count: Option<usize>,
    /// Phase 3.8.7: chunks that arrived while the engine wasn't ready - see
    /// `state::SpeechDiagnostics::chunks_skipped_engine_not_ready`. Never
    /// double-counted in `inferences_attempted` below.
    pub chunks_skipped_engine_not_ready: u64,
    /// Phase 3.8.7.3: only counts calls where `SpeechEngine::feed_audio`
    /// actually ran real inference (`last_feed_triggered_inference() ==
    /// true`) - see `docs/phase-3-8-7-3-audit.md` Finding 1. Previously
    /// this counted every chunk fed to a ready engine, which for
    /// `WhisperSpeechEngine`'s ~3s buffering window meant it was
    /// misleadingly ~300x too high.
    pub inferences_attempted: u64,
    pub inferences_succeeded: u64,
    pub last_error: Option<String>,
    /// Current estimated wall-clock duration (ms) of audio queued for the
    /// speech worker but not yet fed to the engine.
    pub queue_pending_ms: u64,
    /// Highest `queue_pending_ms` observed since the current listening
    /// session started.
    pub queue_high_water_ms: u64,
    /// How many times the backlog crossed the overload threshold and
    /// queued/buffered audio was discarded to catch back up to real time.
    pub overload_events: u64,
    /// Total estimated milliseconds of audio discarded across all
    /// overload events.
    pub audio_ms_dropped_overload: u64,
    pub last_inference_duration_ms: Option<u64>,
    pub max_inference_duration_ms: Option<u64>,
    /// Average inference duration across every real inference so far -
    /// derived at read time from `inference_duration_ms_sum` /
    /// `inference_duration_samples`, never stored redundantly. `None`
    /// until at least one real inference has completed.
    pub avg_inference_duration_ms: Option<u64>,
    pub last_transcript_pipeline_duration_ms: Option<u64>,
    /// Derived from `queue_pending_ms` against fixed thresholds - see
    /// `classify_overload`'s own docs. Never stored redundantly.
    pub overload_state: OverloadState,
    /// Phase 5.3: count of fully-buffered windows the speech engine's own
    /// voice-activity detection classified as silence and skipped without
    /// running real inference - see
    /// `state::SpeechDiagnostics::silent_windows_skipped` and
    /// `docs/phase-5-3-audio-vad.md`.
    pub silent_windows_skipped: u64,
    /// Phase 14: count of real inference passes that produced only one of
    /// whisper.cpp's own known non-speech placeholder captions and were
    /// discarded rather than reported as real spoken content - see
    /// `state::SpeechDiagnostics::non_speech_placeholders_skipped` and
    /// `docs/phase-14-audit.md`.
    pub non_speech_placeholders_skipped: u64,
}

/// Phase 3.8.7.3: the speech pipeline's operator-visible backlog state,
/// derived purely from `queue_pending_ms` against fixed thresholds - see
/// `classify_overload`. Never persisted or set directly; always computed
/// fresh at diagnostics-read time so it can never drift from the counter
/// it's derived from.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverloadState {
    #[default]
    Normal,
    Busy,
    FallingBehind,
    Overloaded,
}

/// Backlog thresholds (milliseconds of queued-but-not-yet-processed
/// audio), per `docs/phase-3-8-7-3-audit.md`'s backpressure design.
/// `OVERLOAD_THRESHOLD_MS` is also the point at which `spawn_speech_worker`
/// actively drains and discards the backlog (see that function) - the
/// other two are purely descriptive, operator-facing states between
/// "fine" and "actively shedding audio."
const BUSY_THRESHOLD_MS: u64 = 3_000;
const FALLING_BEHIND_THRESHOLD_MS: u64 = 6_000;
const OVERLOAD_THRESHOLD_MS: u64 = 10_000;

/// Classifies the current backlog depth for the operator - a pure
/// function so it's directly unit-testable without any real audio/thread
/// plumbing. See the threshold constants' own docs for the exact
/// boundaries.
fn classify_overload(pending_ms: u64) -> OverloadState {
    if pending_ms >= OVERLOAD_THRESHOLD_MS {
        OverloadState::Overloaded
    } else if pending_ms >= FALLING_BEHIND_THRESHOLD_MS {
        OverloadState::FallingBehind
    } else if pending_ms >= BUSY_THRESHOLD_MS {
        OverloadState::Busy
    } else {
        OverloadState::Normal
    }
}

/// Phase 3.8.7.7: on hardware where a single Whisper inference's own
/// wall-clock duration already exceeds `OVERLOAD_THRESHOLD_MS` (confirmed
/// on real Windows hardware: avg inference 14,991ms vs. a 10s threshold -
/// see `docs/phase-3-8-7-7-audit.md`), `spawn_speech_worker`'s overload
/// branch fires once after *every* successful inference, not only when
/// the pipeline is genuinely, persistently falling behind. Discarding the
/// stale audio backlog on every such crossing is still correct (Phase
/// 3.8.7.3's own design - never process minutes-old audio), but wiping
/// `TranscriptSegmenter`'s already-accumulated, validly-transcribed text
/// (Phase 3.8.7.5's `segmenter.reset()`) on the very first isolated
/// crossing destroys real output for no benefit: that text came from
/// audio that was captured essentially continuously with the backlog
/// being dropped, not across a genuine gap.
///
/// `consecutive_overloads` counts overload crossings since the worker was
/// last caught up (reset to 0 on every normal dequeue - see
/// `spawn_speech_worker`). A value of `1` means this is the first crossing
/// since the backlog last cleared - fully explained by the inference that
/// just finished, expected to resolve on its own once the drain empties
/// the channel. Only `>= 2` (backlog still elevated on the very next
/// dequeue, immediately after already draining once) indicates the
/// pipeline cannot keep up independent of any single inference - genuine
/// sustained overload, where resetting the segmenter remains correct.
fn should_reset_segmenter_on_overload(consecutive_overloads: u32) -> bool {
    consecutive_overloads >= 2
}

/// Estimated wall-clock duration (ms) of one `AudioChunk`'s worth of
/// audio, from its own reported sample rate and sample count - a pure
/// function so it's directly unit-testable. Chunk size/rate varies by
/// capture device (Phase 3.8.7.3's audit measured 480 samples @ 48kHz =
/// 10ms on the operator's own real hardware), which is exactly why
/// backlog is tracked in milliseconds-of-audio here, not raw chunk count.
fn chunk_duration_ms(chunk: &AudioChunk) -> u64 {
    if chunk.sample_rate_hz == 0 {
        return 0;
    }
    (chunk.samples.len() as u64 * 1000) / u64::from(chunk.sample_rate_hz)
}

/// Lock-free saturating subtraction: decrements `counter` by `amount`,
/// clamped at 0, without ever underflowing (a plain `fetch_sub` on
/// `AtomicU64` wraps around on underflow, which would corrupt the
/// backlog reading). Used by the speech pipeline's `pending_ms` tracker,
/// which multiple threads (the audio callback's sink, and the speech
/// worker) touch concurrently.
fn saturating_sub_u64(counter: &AtomicU64, amount: u64) {
    let mut current = counter.load(Ordering::SeqCst);
    loop {
        let new = current.saturating_sub(amount);
        match counter.compare_exchange_weak(current, new, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PilotDiagnostics {
    pub machine: MachineDiagnostic,
    pub whisper_model: WhisperModelDiagnostic,
    pub speech: SpeechRuntimeDiagnostics,
    pub audio_devices: Vec<AudioDevice>,
    pub audio: AudioEngineStatus,
    /// Every display this process can detect. `len() >= 2` is a necessary
    /// (not sufficient) condition for "a second display/projector is
    /// actually connected" - see `docs/phase-3-2-hardware-pilot.md` for
    /// why this alone can never be treated as VERIFIED physical-projector
    /// readiness.
    pub displays: Vec<DisplayDiagnostic>,
    pub bible: Option<ContentMetadata>,
    pub database: DatabaseDiagnostic,
}

#[tauri::command]
pub fn get_pilot_diagnostics(app: AppHandle, state: State<'_, AppState>) -> PilotDiagnostics {
    let machine = MachineDiagnostic {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cip_version: env!("CARGO_PKG_VERSION").to_string(),
        build_commit: env!("CIP_GIT_COMMIT").to_string(),
        build_dirty: env!("CIP_GIT_DIRTY") == "true",
    };

    let whisper_model = diagnose_whisper_model(&state.config.whisper_model_path);

    let speech = {
        let diag = state
            .speech_diagnostics
            .lock()
            .expect("speech_diagnostics mutex poisoned")
            .clone();
        // Phase 3.8.7.3: `speech_engine.lock()...is_ready()` used to be
        // read here on every poll - `state.speech_ready` is the same fact,
        // cached once at construction (`is_ready()` never changes after
        // that for either existing engine), so this can never block behind
        // an in-progress Whisper inference. See
        // `docs/phase-3-8-7-3-audit.md` Finding 3.
        let engine_ready = state.speech_ready;
        let avg_inference_duration_ms = if diag.inference_duration_samples > 0 {
            Some(diag.inference_duration_ms_sum / diag.inference_duration_samples)
        } else {
            None
        };
        SpeechRuntimeDiagnostics {
            feature_compiled: diag.feature_compiled,
            model_load_attempted: diag.model_load_attempted,
            model_loaded: diag.model_loaded,
            model_load_error: diag.model_load_error,
            engine_ready,
            chunks_received: diag.chunks_received,
            last_chunk_sample_rate_hz: diag.last_chunk_sample_rate_hz,
            last_chunk_sample_count: diag.last_chunk_sample_count,
            last_resampled_sample_count: diag.last_resampled_sample_count,
            chunks_skipped_engine_not_ready: diag.chunks_skipped_engine_not_ready,
            inferences_attempted: diag.inferences_attempted,
            inferences_succeeded: diag.inferences_succeeded,
            last_error: diag.last_error,
            queue_pending_ms: diag.queue_pending_ms,
            queue_high_water_ms: diag.queue_high_water_ms,
            overload_events: diag.overload_events,
            audio_ms_dropped_overload: diag.audio_ms_dropped_overload,
            last_inference_duration_ms: diag.last_inference_duration_ms,
            max_inference_duration_ms: diag.max_inference_duration_ms,
            avg_inference_duration_ms,
            last_transcript_pipeline_duration_ms: diag.last_transcript_pipeline_duration_ms,
            overload_state: classify_overload(diag.queue_pending_ms),
            silent_windows_skipped: diag.silent_windows_skipped,
            non_speech_placeholders_skipped: diag.non_speech_placeholders_skipped,
        }
    };

    let audio_engine = state
        .audio_engine
        .lock()
        .expect("audio_engine mutex poisoned");
    let audio_devices = audio_engine.list_devices().unwrap_or_default();
    let audio = audio_engine.status();
    drop(audio_engine);

    let primary_position = app.primary_monitor().ok().flatten().map(|m| *m.position());
    let displays = app
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|m| DisplayDiagnostic {
            name: m.name().cloned(),
            width_px: m.size().width,
            height_px: m.size().height,
            position_x: m.position().x,
            position_y: m.position().y,
            scale_factor: m.scale_factor(),
            is_primary: primary_position == Some(*m.position()),
        })
        .collect();

    let bible = state
        .content_registry
        .get(&content::bible_content_id(
            crate::bible_production_dataset::BSB_TRANSLATION_ID,
        ))
        .unwrap_or(None);

    let database = {
        let readable = {
            let db = state.db.lock().expect("db connection poisoned");
            db.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                .is_ok()
        };
        // Opening for write without truncating or writing any bytes is a
        // side-effect-free way to prove the file is writable - a "get
        // diagnostics" command must never itself mutate the database.
        let writable = std::fs::OpenOptions::new()
            .write(true)
            .open(&state.config.database_path)
            .is_ok();
        DatabaseDiagnostic {
            path: state.config.database_path.display().to_string(),
            readable,
            writable,
        }
    };

    PilotDiagnostics {
        machine,
        whisper_model,
        speech,
        audio_devices,
        audio,
        displays,
        bible,
        database,
    }
}

// --- Phase 3.2 backup ---------------------------------------------------
//
// A pilot church's only realistic recovery story for "the laptop died" or
// "the disk got corrupted" is a copy of the SQLite file taken while
// everything was healthy. The database runs in WAL mode
// (`database/src/connection.rs`), so a raw `fs::copy` of just
// `cip.sqlite3` while CIP is running would miss whatever is still only in
// the `-wal`/`-shm` sidecar files - `VACUUM INTO` is SQLite's own
// documented way to produce a single, complete, consistent snapshot file
// regardless of journal mode, taken directly through the live connection,
// with no need to pause or lock out normal use. Restoring is deliberately
// NOT a live in-app command: swapping out an actively-open database
// connection's backing file out from under it is a real corruption risk
// this phase's "minimal scope, no unnecessary risk" principle rules out.
// The safe restore procedure - close CIP, copy the backup file over
// `cip.sqlite3` (and delete any stale `-wal`/`-shm` sidecars), reopen -
// is documented in `docs/phase-3-2-hardware-pilot.md` and needs no new
// code: `cip_database::open` already runs the normal startup path
// (migrations no-op on an up-to-date schema, stale-Active reconciliation)
// against whatever file it finds there.

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupReport {
    pub backup_path: String,
    pub size_bytes: u64,
}

#[tauri::command]
pub fn backup_database(
    destination_dir: String,
    state: State<'_, AppState>,
) -> Result<BackupReport, AppError> {
    let destination_dir = require_non_empty(&destination_dir, "destinationDir")
        .map_err(log_and_return)?
        .to_string();
    let dest_dir = std::path::PathBuf::from(&destination_dir);
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| {
            AppError::InvalidInput(format!(
                "cannot create backup directory {}: {e}",
                dest_dir.display()
            ))
        })
        .map_err(log_and_return)?;

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let backup_path = dest_dir.join(format!("cip-backup-{timestamp}.sqlite3"));

    {
        let db = state.db.lock().expect("db connection poisoned");
        db.execute(
            "VACUUM INTO ?1",
            rusqlite::params![backup_path.to_string_lossy()],
        )
        .map_err(|e| AppError::from(cip_database::DatabaseError::Connection(e.to_string())))
        .map_err(log_and_return)?;
    }

    let size_bytes = std::fs::metadata(&backup_path)
        .map(|m| m.len())
        .unwrap_or(0);
    Ok(BackupReport {
        backup_path: backup_path.display().to_string(),
        size_bytes,
    })
}

/// `list_bible_translations`'s selection guard (Phase 1.5 section 10):
/// disabled content never appears in normal selection - but fails *open*.
/// A translation with no registry entry at all (`Ok(None)`) or a registry
/// read error (`Err`) is never hidden just because this phase's
/// bookkeeping hasn't caught up to it; only an explicit `Disabled` record
/// hides one. Pulled out as a plain function (same reasoning as this
/// module's other guards) so the decision is directly unit-testable
/// without a full `AppState`.
pub(crate) fn is_translation_selectable(
    registry_lookup: Result<Option<&ContentMetadata>, &ContentRegistryError>,
) -> bool {
    !matches!(registry_lookup, Ok(Some(metadata)) if metadata.status == ContentStatus::Disabled)
}

#[tauri::command]
pub fn list_bible_translations(
    state: State<'_, AppState>,
) -> Result<Vec<BibleTranslation>, AppError> {
    let translations = state
        .bible_provider
        .list_translations()
        .map_err(AppError::from)
        .map_err(log_and_return)?;

    Ok(translations
        .into_iter()
        .filter(|t| {
            let lookup = state
                .content_registry
                .get(&content::bible_content_id(&t.id));
            is_translation_selectable(lookup.as_ref().map(|opt| opt.as_ref()))
        })
        .collect())
}

// --- service lifecycle -----------------------------------------------------

/// "Do not create a new service when resuming" / "every service must have
/// a distinct service ID": `active_service` is only ever cleared by
/// `end_service`, so its presence alone (regardless of `status`) is
/// sufficient proof a service is already live-or-paused - starting a
/// second one on top of it would silently orphan the first (still
/// `started` in the database, but no longer reachable from `AppState`).
#[tauri::command]
pub fn start_service(
    title: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ServiceSession, AppError> {
    let title = require_non_empty(&title, "title").map_err(log_and_return)?;
    ensure_no_active_service(
        state
            .active_service
            .lock()
            .expect("active_service mutex poisoned")
            .as_ref(),
    )
    .map_err(log_and_return)?;
    let session = ServiceSession::start(title);

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::persist_service(&db, &session)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        record_timeline(
            &db,
            Some(session.id),
            AppEvent::ServiceStarted,
            LogCategory::App,
            &session,
        );
    }
    *state
        .active_service
        .lock()
        .expect("active_service mutex poisoned") = Some(session.clone());

    let _ = emit(&app, AppEvent::ServiceStarted, session.clone());
    Ok(session)
}

/// Pauses the active (`Started`) service in place - "do not treat pause as
/// service termination." Stops audio capture via `pause()` where the
/// engine supports it (falling back to `stop()` otherwise), best-effort:
/// a capture failure must not block the service record itself from
/// pausing. Transcript, detections, suggestions, and the active Scripture
/// context are all left exactly as they are.
#[tauri::command]
pub fn pause_service(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ServiceSession, AppError> {
    let mut session = {
        let guard = state
            .active_service
            .lock()
            .expect("active_service mutex poisoned");
        guard
            .clone()
            .ok_or(AppError::NoActiveService)
            .map_err(log_and_return)?
    };
    ensure_service_status(&session, ServiceStatus::Started, "pause").map_err(log_and_return)?;

    {
        let mut audio = state
            .audio_engine
            .lock()
            .expect("audio_engine mutex poisoned");
        if audio.pause().is_err() {
            if let Err(e) = audio.stop() {
                log::warn!(target: LogCategory::Audio.target(), "pause-on-pause-service fell back to stop and failed too (continuing): {e}");
            }
        }
    }

    session.pause();
    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::update_service_status(&db, session.id, session.status, session.ended_at)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        record_timeline(
            &db,
            Some(session.id),
            AppEvent::ServicePaused,
            LogCategory::App,
            &session,
        );
        record_timeline(
            &db,
            Some(session.id),
            AppEvent::AudioStopped,
            LogCategory::Audio,
            serde_json::json!({}),
        );
        record_timeline(
            &db,
            Some(session.id),
            AppEvent::SpeechStopped,
            LogCategory::Speech,
            serde_json::json!({}),
        );
    }
    *state
        .active_service
        .lock()
        .expect("active_service mutex poisoned") = Some(session.clone());

    let _ = emit(&app, AppEvent::ServicePaused, session.clone());
    Ok(session)
}

/// Resumes a `Paused` service - "do not create a new service when
/// resuming." Restores live processing by calling `resume()` on the audio
/// engine where supported (best-effort - see module docs on why an
/// operator retry, not an automatic loop, handles a failed resume);
/// transcript sequence numbering and Scripture context continue from
/// exactly where they were, since neither was ever reset by pausing.
#[tauri::command]
pub fn resume_service(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ServiceSession, AppError> {
    let mut session = {
        let guard = state
            .active_service
            .lock()
            .expect("active_service mutex poisoned");
        guard
            .clone()
            .ok_or(AppError::NoActiveService)
            .map_err(log_and_return)?
    };
    ensure_service_status(&session, ServiceStatus::Paused, "resume").map_err(log_and_return)?;

    session.resume();
    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::update_service_status(&db, session.id, session.status, session.ended_at)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        record_timeline(
            &db,
            Some(session.id),
            AppEvent::ServiceResumed,
            LogCategory::App,
            &session,
        );
    }
    *state
        .active_service
        .lock()
        .expect("active_service mutex poisoned") = Some(session.clone());

    if let Err(e) = state
        .audio_engine
        .lock()
        .expect("audio_engine mutex poisoned")
        .resume()
    {
        log::warn!(target: LogCategory::Audio.target(), "resume-on-resume-service could not restore capture (operator can retry via start_listening): {e}");
    }

    let _ = emit(&app, AppEvent::ServiceResumed, session.clone());
    Ok(session)
}

/// Ends the active service. Stops audio capture first (best-effort - a
/// failure to stop cleanly must not block ending the service record).
#[tauri::command]
pub fn end_service(app: AppHandle, state: State<'_, AppState>) -> Result<ServiceSession, AppError> {
    let mut session = state
        .active_service
        .lock()
        .expect("active_service mutex poisoned")
        .take()
        .ok_or(AppError::NoActiveService)
        .map_err(log_and_return)?;

    if let Err(e) = state
        .audio_engine
        .lock()
        .expect("audio_engine mutex poisoned")
        .stop()
    {
        log::warn!(target: LogCategory::Audio.target(), "stop-on-end-service failed (continuing): {e}");
    }

    session.end();
    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::update_service_status(&db, session.id, session.status, session.ended_at)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        record_timeline(
            &db,
            Some(session.id),
            AppEvent::ServiceEnded,
            LogCategory::App,
            &session,
        );
    }

    let _ = emit(&app, AppEvent::ServiceEnded, session.clone());
    Ok(session)
}

// --- audio ------------------------------------------------------------------

#[tauri::command]
pub fn list_audio_devices(state: State<'_, AppState>) -> Result<Vec<AudioDevice>, AppError> {
    state
        .audio_engine
        .lock()
        .expect("audio_engine mutex poisoned")
        .list_devices()
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// Starts capture and wires it straight into the speech engine and Bible
/// Intelligence Core pipeline. `device_id: None` picks the reported
/// default input device; if none exists, this reports
/// `AudioEngineError::NoDevice` rather than guessing - "do not
/// automatically select an arbitrary device if the system cannot
/// determine a safe default."
#[tauri::command]
pub fn start_listening(
    device_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;

    let resolved_device_id = match device_id {
        Some(id) => id,
        None => {
            let devices = state
                .audio_engine
                .lock()
                .expect("audio_engine mutex poisoned")
                .list_devices()?;
            devices
                .into_iter()
                .find(|d| d.is_default)
                .map(|d| d.id)
                .ok_or(cip_core_service::AudioEngineError::NoDevice)
                .map_err(AppError::from)
                .map_err(log_and_return)?
        }
    };

    // Phase 3.8.5: audio capture must not be gated on speech-engine
    // readiness - see docs/phase-3-8-5-audit.md section M/N. `AudioEngine`
    // and `SpeechEngine` are independent capabilities (docs/live-speech.md's
    // "four independent signals"); a device with no Whisper model installed
    // must still be able to capture, meter, and feed the acoustic/music
    // pipeline. Whether speech is *also* available is checked below, after
    // capture has actually started, purely to decide whether
    // `AppEvent::SpeechStarted` is honestly emitted - it is never assumed.
    //
    // Phase 2.2: a second consumer inside the same single sink closure,
    // never a second `AudioEngine::start()` call - the trait allows
    // exactly one sink. `try_send`/bounded channel so a slow/backed-up
    // acoustic worker can never block the audio capture thread or the
    // speech-engine feed right after it; a dropped chunk here only means
    // one less acoustic analysis window, never lost transcript audio
    // (only `acoustic_tx`'s clone below feeds the acoustic path).
    let (acoustic_tx, acoustic_rx) =
        mpsc::sync_channel::<AudioChunk>(acoustic::ACOUSTIC_CHANNEL_CAPACITY);
    spawn_acoustic_worker(app.clone(), service_id, acoustic_rx);

    // Phase 3.8.7.2: `handle_audio_chunk` used to run inline, synchronously,
    // on cpal's own real-time audio callback thread - the same thread that
    // must return in low-single-digit milliseconds to keep the OS's audio
    // ring buffer fed. Once every ~3s of buffered audio it instead ran a
    // real, blocking whisper.cpp inference call there, which real-hardware
    // evidence traced to app slowness, degraded transcription (a stalled
    // callback can starve/glitch the audio backend feeding Whisper), and
    // the UI's own status poll intermittently blocking on the same
    // `speech_engine` mutex the inference held - see
    // docs/phase-3-8-7-2-audit.md. Fixed the same way the acoustic path
    // already was: hand each chunk off through a channel to a dedicated
    // worker thread, so the audio callback only ever does cheap,
    // non-blocking work.
    //
    // Phase 3.8.7.3: the channel itself is still unbounded (`mpsc::channel`,
    // not `sync_channel`) - dropping a chunk mid-buffer here would still
    // introduce a gap into whatever Whisper is accumulating. What changed
    // is that the *backlog* is no longer allowed to grow without limit:
    // `pending_ms` tracks how much queued audio the worker hasn't consumed
    // yet (incremented here on send, decremented by the worker on
    // dequeue), and once the worker observes that backlog crossing
    // `OVERLOAD_THRESHOLD_MS` it drains and discards it in bulk rather than
    // grinding through an ever-more-stale FIFO - see `spawn_speech_worker`
    // and `docs/phase-3-8-7-3-audit.md` Finding 2.
    let (speech_tx, speech_rx) = mpsc::channel::<AudioChunk>();
    let pending_ms = Arc::new(AtomicU64::new(0));
    let worker_pending_ms = Arc::clone(&pending_ms);
    // Phase 3.8.7.3 Finding 4: a fresh generation for this listening
    // session - `spawn_speech_worker` tags every non-empty result it
    // produces with this value, so a stale worker from a *previous*
    // `start_listening` call (still finishing its last, unavoidably
    // uncancellable `feed_audio` call - whisper.cpp has no cancellation
    // API) can never write output into this new session. Reset
    // `queue_high_water_ms` for the new session while holding the lock.
    let generation = state.listening_generation.fetch_add(1, Ordering::SeqCst) + 1;
    {
        let mut diag = state
            .speech_diagnostics
            .lock()
            .expect("speech_diagnostics mutex poisoned");
        diag.queue_pending_ms = 0;
        diag.queue_high_water_ms = 0;
    }
    spawn_speech_worker(
        app.clone(),
        service_id,
        speech_rx,
        worker_pending_ms,
        generation,
    );

    let sink: AudioChunkSink = Arc::new(move |chunk: AudioChunk| {
        let _ = acoustic_tx.try_send(chunk.clone());
        let duration_ms = chunk_duration_ms(&chunk);
        if speech_tx.send(chunk).is_ok() {
            pending_ms.fetch_add(duration_ms, Ordering::SeqCst);
        }
    });

    let start_result = state
        .audio_engine
        .lock()
        .expect("audio_engine mutex poisoned")
        .start(&resolved_device_id, sink);

    if let Err(e) = &start_result {
        // Phase 1.3 audio failure recovery: recorded so `get_live_status`
        // reports `AudioStatusKind::Error` (not just `Unavailable`) until
        // a retry succeeds - see `state::AppState::audio_error`.
        *state
            .audio_error
            .lock()
            .expect("audio_error mutex poisoned") = Some(e.to_string());
        let db = state.db.lock().expect("db connection poisoned");
        record_timeline(
            &db,
            Some(service_id),
            AppEvent::ErrorOccurred,
            LogCategory::Audio,
            serde_json::json!({ "context": "start_listening", "error": e.to_string() }),
        );
    } else {
        *state
            .audio_error
            .lock()
            .expect("audio_error mutex poisoned") = None;
    }
    start_result
        .map_err(AppError::from)
        .map_err(log_and_return)?;

    // Audio genuinely started, so this event is recorded/emitted
    // unconditionally. Speech readiness is checked only now, after a
    // successful `audio_engine.start()`, and only to decide whether
    // `SpeechStarted` is honest to emit - it must never be fabricated when
    // no speech engine is actually ready to transcribe. Phase 3.8.7.3: reads
    // the cached `state.speech_ready` field rather than locking
    // `speech_engine` - see that field's own docs (Finding 3).
    let speech_ready = state.speech_ready;

    {
        let db = state.db.lock().expect("db connection poisoned");
        record_timeline(
            &db,
            Some(service_id),
            AppEvent::AudioStarted,
            LogCategory::Audio,
            serde_json::json!({ "deviceId": resolved_device_id }),
        );
        if speech_ready {
            record_timeline(
                &db,
                Some(service_id),
                AppEvent::SpeechStarted,
                LogCategory::Speech,
                serde_json::json!({}),
            );
        }
    }
    let _ = emit(
        &app,
        AppEvent::AudioStarted,
        serde_json::json!({ "deviceId": resolved_device_id }),
    );
    if speech_ready {
        let _ = emit(&app, AppEvent::SpeechStarted, serde_json::json!({}));
    }
    Ok(())
}

#[tauri::command]
pub fn stop_listening(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    state
        .audio_engine
        .lock()
        .expect("audio_engine mutex poisoned")
        .stop()
        .map_err(AppError::from)
        .map_err(log_and_return)?;

    // Mirrors `start_listening`'s honesty rule: only claim speech stopped
    // if a speech engine was actually ready to have been running. Phase
    // 3.8.7.3: reads the cached `state.speech_ready` field (Finding 3).
    let speech_ready = state.speech_ready;

    if let Ok(guard) = state.active_service.lock() {
        if let Some(session) = guard.as_ref() {
            let db = state.db.lock().expect("db connection poisoned");
            record_timeline(
                &db,
                Some(session.id),
                AppEvent::AudioStopped,
                LogCategory::Audio,
                serde_json::json!({}),
            );
            if speech_ready {
                record_timeline(
                    &db,
                    Some(session.id),
                    AppEvent::SpeechStopped,
                    LogCategory::Speech,
                    serde_json::json!({}),
                );
            }
        }
    }
    let _ = emit(&app, AppEvent::AudioStopped, serde_json::json!({}));
    if speech_ready {
        let _ = emit(&app, AppEvent::SpeechStopped, serde_json::json!({}));
    }
    Ok(())
}

/// Background worker thread for acoustic (audio-fingerprint) recognition
/// (Phase 2.2) - reads `AudioChunk`s from `rx` (fed by `start_listening`'s
/// sink closure, alongside but never blocking the speech-engine feed)
/// until the channel closes, which happens automatically and cleanly when
/// `stop_listening`/a failed `start_listening` causes the audio engine to
/// drop the sink that held `rx`'s matching sender (see this function's
/// caller). Owns its own `acoustic::AcousticWorkerState` exclusively -
/// never shared with any other thread, unlike `AppState`'s
/// `Mutex`-guarded fields this loop does still need to reach (the
/// recognizer, the finding queue, the database).
///
/// Every genuine failure - a bad `IntelligenceContext` build, a recognizer
/// error, an engine error - is logged and recorded to the timeline, then
/// the loop simply continues to the next window. One bad window must
/// never end acoustic recognition for the rest of the service (Phase
/// 2.2's failure-isolation requirement), and this loop must never take
/// down the process - nothing here panics on a data-dependent path.
fn spawn_acoustic_worker(app: AppHandle, service_id: Uuid, rx: mpsc::Receiver<AudioChunk>) {
    std::thread::spawn(move || {
        let config = {
            let state = app.state::<AppState>();
            let acoustic_config = &state.config.acoustic;
            cip_core_music::AcousticAnalysisConfig {
                window_ms: acoustic_config.analysis_window_ms,
                overlap_ms: acoustic_config.overlap_ms,
                min_duration_ms: acoustic_config.minimum_audio_ms,
                ..cip_core_music::AcousticAnalysisConfig::default()
            }
        };
        let mut worker = acoustic::AcousticWorkerState::new(config);

        while let Ok(chunk) = rx.recv() {
            let Some(segment) = worker.ingest(&chunk.samples, chunk.sample_rate_hz) else {
                continue;
            };
            if !worker.should_attempt_recognition(&segment) {
                continue;
            }

            let state = app.state::<AppState>();
            let context = match build_music_context(&state, service_id) {
                Ok(context) => context,
                Err(e) => {
                    log::error!(
                        target: LogCategory::Music.target(),
                        "acoustic worker: failed to build intelligence context: {e}"
                    );
                    continue;
                }
            };
            let content_ids = acoustic::enabled_music_dataset_ids(&context);
            if content_ids.is_empty() {
                // No enabled Music dataset to resolve into - never call
                // the recognizer at all, and never count this as a
                // recognition attempt (see `record_recognition_attempt`
                // below, deliberately skipped here) so the very next
                // window is retried immediately once a dataset is
                // enabled, rather than staying rate-limited by a call
                // that never happened.
                continue;
            }
            worker.record_recognition_attempt(&segment);

            let outcome = {
                let mut recognizer = state
                    .acoustic_recognizer
                    .lock()
                    .expect("acoustic_recognizer mutex poisoned");
                let mut findings = state
                    .intelligence_findings
                    .lock()
                    .expect("intelligence_findings mutex poisoned");
                acoustic::recognize_fuse_and_queue(
                    recognizer.as_mut(),
                    &state.acoustic_music_engine,
                    &segment,
                    &content_ids,
                    service_id,
                    &context,
                    &mut findings,
                )
            };

            match outcome {
                Ok(queued) if queued.is_empty() => {}
                Ok(queued) => {
                    let db = state.db.lock().expect("db connection poisoned");
                    for finding in &queued {
                        record_timeline(
                            &db,
                            Some(service_id),
                            AppEvent::MusicFindingDetected,
                            LogCategory::Music,
                            serde_json::json!({
                                "findingId": finding.id,
                                "summary": &finding.summary,
                                "confidence": finding.confidence.score,
                                "source": "acoustic",
                            }),
                        );
                    }
                    drop(db);
                    for finding in queued {
                        let _ = emit(&app, AppEvent::MusicFindingDetected, finding);
                    }
                }
                Err(e) => {
                    log::warn!(
                        target: LogCategory::Music.target(),
                        "acoustic recognition failed: {e}"
                    );
                    let db = state.db.lock().expect("db connection poisoned");
                    record_timeline(
                        &db,
                        Some(service_id),
                        AppEvent::ErrorOccurred,
                        LogCategory::Music,
                        serde_json::json!({ "context": "acoustic_recognition", "error": e.to_string() }),
                    );
                }
            }
        }
    });
}

/// Phase 3.8.6: linear-interpolation resampler bridging `AudioEngine`'s
/// device-native sample rate to whatever fixed rate a `SpeechEngine`
/// requires (see `SpeechEngine::required_sample_rate_hz`). `integrations/audio`
/// deliberately never resamples itself (see its own module docs) - this is
/// the one consumer that currently needs a fixed rate, so the conversion
/// happens here, at the call site, not inside a second audio engine.
/// Deliberately simple (linear interpolation, not a windowed-sinc
/// resampler): adequate for feeding a buffering, non-realtime-critical
/// speech engine, not claimed to be broadcast-quality DSP.
fn resample_pcm16(samples: &[i16], from_hz: u32, to_hz: u32) -> Vec<i16> {
    if samples.is_empty() || from_hz == 0 || to_hz == 0 || from_hz == to_hz {
        return samples.to_vec();
    }
    let ratio = f64::from(to_hz) / f64::from(from_hz);
    // `.max(1)`: a non-empty input must never resample down to a fully
    // empty buffer (a downsampled single-sample/very-short chunk would
    // otherwise round to zero output samples) - that would look
    // indistinguishable from silence to a diagnostics reader, when in
    // fact real (if brief) input existed.
    let out_len = (((samples.len() as f64) * ratio).round() as usize).max(1);
    let last_idx = samples.len() - 1;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx = (src_pos.floor() as usize).min(last_idx);
        let frac = src_pos - idx as f64;
        let s0 = f64::from(samples[idx]);
        let s1 = f64::from(samples[(idx + 1).min(last_idx)]);
        let interpolated = s0 + (s1 - s0) * frac;
        out.push(
            interpolated
                .round()
                .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16,
        );
    }
    out
}

/// Phase 3.8.7.2: the dedicated worker thread `handle_audio_chunk` actually
/// runs on - see that function's own docs and `docs/phase-3-8-7-2-audit.md`
/// for why this exists (previously it ran inline on cpal's real-time audio
/// callback thread, where a blocking whisper.cpp inference call every ~3s
/// violated the real-time-audio contract). Mirrors `spawn_acoustic_worker`
/// exactly: reads from `rx` (fed by `start_listening`'s sink closure) until
/// the channel closes, which happens automatically when `stop_listening`/a
/// failed `start_listening` drops the sink that held the matching sender.
/// No explicit `flush()` on channel closure - buffered-but-not-yet-inferred
/// audio (less than one `CHUNK_SAMPLES` window) is simply dropped, exactly
/// the same behavior this had before this phase (unchanged, not a new gap).
///
/// Phase 3.8.7.3: `pending_ms` is the shared backlog tracker - `start_listening`'s
/// sink closure adds each chunk's duration on send; this loop subtracts it
/// back off on dequeue. When the remaining backlog (after removing the
/// chunk just dequeued) is at or above `OVERLOAD_THRESHOLD_MS`, the worker
/// is falling behind badly enough that grinding through the rest of the
/// backlog would only ever process increasingly stale audio - so instead it
/// drains and discards everything currently queued (`rx.try_recv()` until
/// empty) plus whatever `WhisperSpeechEngine` itself had buffered
/// (`discard_buffered_audio`), and resumes from the next fresh chunk. This
/// keeps memory bounded (the backlog can only grow across a single
/// worker-loop iteration, never indefinitely) and guarantees Whisper is
/// never fed minutes-old audio - see `docs/phase-3-8-7-3-audit.md` Finding 2
/// for the full design and why a plain bounded/drop-newest channel alone
/// was rejected. `generation` is this listening session's id (Finding 4) -
/// passed through unchanged to `handle_audio_chunk`.
fn spawn_speech_worker(
    app: AppHandle,
    service_id: Uuid,
    rx: mpsc::Receiver<AudioChunk>,
    pending_ms: Arc<AtomicU64>,
    generation: u64,
) {
    std::thread::spawn(move || {
        // Phase 3.8.7.5 Part A: owned exclusively by this worker thread for
        // the lifetime of this one listening session - never shared, never
        // behind a `Mutex` (mirrors `acoustic::AcousticWorkerState`). See
        // `segmentation.rs`'s own module docs.
        let mut segmenter = TranscriptSegmenter::new();
        // Phase 3.8.7.7: counts overload crossings since the worker was
        // last caught up - see `should_reset_segmenter_on_overload`'s own
        // docs. Owned exclusively by this thread, same as `segmenter`.
        let mut consecutive_overloads: u32 = 0;

        while let Ok(chunk) = rx.recv() {
            let chunk_ms = chunk_duration_ms(&chunk);
            saturating_sub_u64(&pending_ms, chunk_ms);
            let backlog_ms = pending_ms.load(Ordering::SeqCst);

            let state = app.state::<AppState>();
            {
                let mut diag = state
                    .speech_diagnostics
                    .lock()
                    .expect("speech_diagnostics mutex poisoned");
                diag.queue_pending_ms = backlog_ms;
                if backlog_ms > diag.queue_high_water_ms {
                    diag.queue_high_water_ms = backlog_ms;
                }
            }

            if backlog_ms >= OVERLOAD_THRESHOLD_MS {
                // The chunk just dequeued is itself already stale at this
                // backlog depth - discard it along with everything else
                // still queued, rather than spending an inference on audio
                // that will be minutes old by the time it's transcribed.
                let mut dropped_ms = chunk_ms;
                while let Ok(stale) = rx.try_recv() {
                    let stale_ms = chunk_duration_ms(&stale);
                    dropped_ms += stale_ms;
                    saturating_sub_u64(&pending_ms, stale_ms);
                }
                state
                    .speech_engine
                    .lock()
                    .expect("speech_engine mutex poisoned")
                    .discard_buffered_audio();
                // Phase 3.8.7.7: only discard the segmenter's already-
                // accumulated text when overload has persisted across
                // consecutive dequeues - a single isolated crossing (the
                // common case on hardware whose own inference duration
                // alone exceeds `OVERLOAD_THRESHOLD_MS`) means that text
                // is real, contiguous output that would otherwise flush
                // normally once the backlog clears. See
                // `should_reset_segmenter_on_overload`'s own docs and
                // `docs/phase-3-8-7-7-audit.md`. Genuine sustained overload
                // (Phase 3.8.7.5's original concern - pre-overload text
                // spliced onto unrelated post-recovery text) still resets.
                consecutive_overloads = consecutive_overloads.saturating_add(1);
                if should_reset_segmenter_on_overload(consecutive_overloads) {
                    segmenter.reset();
                }
                {
                    let mut diag = state
                        .speech_diagnostics
                        .lock()
                        .expect("speech_diagnostics mutex poisoned");
                    diag.overload_events += 1;
                    diag.audio_ms_dropped_overload += dropped_ms;
                    diag.queue_pending_ms = pending_ms.load(Ordering::SeqCst);
                }
                log::warn!(
                    target: LogCategory::Speech.target(),
                    "speech worker overloaded: discarded ~{dropped_ms}ms of backlog audio, resuming from live audio"
                );
                continue;
            }
            consecutive_overloads = 0;

            handle_audio_chunk(&app, service_id, chunk, generation, &mut segmenter);
        }

        // Phase 3.8.7.5: `stop_listening` closed the channel - whatever is
        // still buffered in the segmenter (less than one full 12-20s
        // window) is real speech that must not be silently dropped just
        // because listening stopped mid-window.
        if let Some(mut remaining) = segmenter.flush_remaining() {
            let state = app.state::<AppState>();
            if state.listening_generation.load(Ordering::SeqCst) == generation {
                remaining.sequence = state.transcript_sequence.fetch_add(1, Ordering::SeqCst);
                finalize_and_route_segment(&app, &state, service_id, remaining);
            }
        }
    });
}

/// Runs on the dedicated speech worker thread (`spawn_speech_worker`) -
/// never on cpal's own real-time audio callback thread (Phase 3.8.7.2:
/// moved off it - see `docs/phase-3-8-7-2-audit.md` for the real-hardware
/// evidence that running whisper.cpp inference there caused app slowness,
/// degraded transcription quality, and intermittent UI status stalls), and
/// never on a Tauri command thread either. Re-fetches `AppState` from the
/// cloned `AppHandle` rather than capturing a `State<'_, AppState>`
/// directly, since the latter's lifetime is tied to a single command
/// invocation and can't be captured into a closure/thread that outlives it.
///
/// `generation` is the listening session this chunk was captured under
/// (Phase 3.8.7.3 Finding 4, set by `start_listening`/passed through
/// `spawn_speech_worker`) - checked before any non-empty result is
/// emitted/persisted, so a worker whose channel closed but is still
/// finishing an in-flight `feed_audio` call cannot write output into a
/// newer listening session that has since started.
///
/// `segmenter` (Phase 3.8.7.5 Part A) accumulates each raw ~3s Whisper
/// window into a bounded 12-20s logical segment - only a completed
/// window is ever persisted, routed, or emitted. See `segmentation.rs`.
fn handle_audio_chunk(
    app: &AppHandle,
    service_id: Uuid,
    chunk: AudioChunk,
    generation: u64,
    segmenter: &mut TranscriptSegmenter,
) {
    let state = app.state::<AppState>();

    let segments = {
        let mut speech = state
            .speech_engine
            .lock()
            .expect("speech_engine mutex poisoned");

        // Phase 3.8.7: a real Windows session with no model installed
        // showed "60,684 inferences attempted / 0 succeeded" - misleading,
        // since every one of those chunks was rejected by `NullSpeechEngine`
        // before whisper.cpp ever ran. `is_ready()` is a reliable,
        // always-available per-engine signal (`NullSpeechEngine` always
        // `false`; `WhisperSpeechEngine` always `true` once constructed -
        // see both engines' own `is_ready` impls), so check it here and
        // skip `feed_audio` entirely when not ready: there is nothing to
        // resample or feed, and calling it anyway would only reproduce the
        // same `SpeechEngineError::NotInitialized` on every single chunk,
        // which previously also wrote a redundant timeline row per chunk
        // for a static condition that doesn't change chunk-to-chunk.
        if !speech.is_ready() {
            let error_text = SpeechEngineError::NotInitialized.to_string();
            let mut diag = state
                .speech_diagnostics
                .lock()
                .expect("speech_diagnostics mutex poisoned");
            diag.chunks_received += 1;
            diag.last_chunk_sample_rate_hz = Some(chunk.sample_rate_hz);
            diag.last_chunk_sample_count = Some(chunk.samples.len());
            diag.chunks_skipped_engine_not_ready += 1;
            diag.last_error = Some(error_text.clone());
            drop(diag);
            *state
                .speech_error
                .lock()
                .expect("speech_error mutex poisoned") = Some(error_text);
            return;
        }

        // Phase 3.8.6: `AudioEngine` delivers chunks at the device's own
        // native rate and never resamples (see `integrations/audio`'s
        // module docs); a speech engine with a fixed rate requirement
        // (only `WhisperSpeechEngine` has one, at 16kHz) is the consumer
        // responsible for converting - this is that conversion. Computed
        // fresh per chunk since `required_sample_rate_hz()` is cheap and
        // `chunk.sample_rate_hz` can legitimately change across a
        // stop/start with a different device selected.
        let target_rate = speech.required_sample_rate_hz();
        let resampled;
        let (feed_samples, resampled_len): (&[i16], Option<usize>) = match target_rate {
            Some(target) if target != chunk.sample_rate_hz && chunk.sample_rate_hz > 0 => {
                resampled = resample_pcm16(&chunk.samples, chunk.sample_rate_hz, target);
                let len = resampled.len();
                (&resampled, Some(len))
            }
            _ => (&chunk.samples, None),
        };

        {
            let mut diag = state
                .speech_diagnostics
                .lock()
                .expect("speech_diagnostics mutex poisoned");
            diag.chunks_received += 1;
            diag.last_chunk_sample_rate_hz = Some(chunk.sample_rate_hz);
            diag.last_chunk_sample_count = Some(chunk.samples.len());
            diag.last_resampled_sample_count = resampled_len;
        }

        // Phase 3.8.7.3 Finding 1: `feed_audio` only actually runs
        // whisper.cpp's `full()` once per ~3s buffering window - most
        // calls just append to an internal buffer and return immediately.
        // Counting every call as an "inference attempt" (the old
        // behavior) was off by roughly the buffering-window factor.
        // `last_feed_triggered_inference()` reports the truth after the
        // fact, so both the counters and the duration measurement below
        // are gated on it - never on "a `feed_audio` call happened".
        let inference_start = std::time::Instant::now();
        let feed_result = speech.feed_audio(feed_samples);
        let triggered_inference = speech.last_feed_triggered_inference();

        if triggered_inference {
            let duration_ms =
                u64::try_from(inference_start.elapsed().as_millis()).unwrap_or(u64::MAX);
            let mut diag = state
                .speech_diagnostics
                .lock()
                .expect("speech_diagnostics mutex poisoned");
            diag.inferences_attempted += 1;
            diag.last_inference_duration_ms = Some(duration_ms);
            diag.max_inference_duration_ms =
                Some(diag.max_inference_duration_ms.unwrap_or(0).max(duration_ms));
            diag.inference_duration_ms_sum += duration_ms;
            diag.inference_duration_samples += 1;
        }

        // Phase 5.3: a full window that VAD classified as silence also
        // reports `triggered_inference == false` (no real inference ran),
        // so this check must happen independently of the block above, not
        // nested inside it - see `SpeechEngine::last_feed_was_silence`'s
        // doc comment for why the two cases must stay distinguishable.
        if speech.last_feed_was_silence() {
            state
                .speech_diagnostics
                .lock()
                .expect("speech_diagnostics mutex poisoned")
                .silent_windows_skipped += 1;
        }

        // Phase 14: distinct from the silence check above - inference did
        // run here, it just produced one of whisper.cpp's own known
        // non-speech placeholder captions rather than real speech. See
        // `SpeechEngine::last_feed_was_non_speech_placeholder`'s own docs.
        if speech.last_feed_was_non_speech_placeholder() {
            state
                .speech_diagnostics
                .lock()
                .expect("speech_diagnostics mutex poisoned")
                .non_speech_placeholders_skipped += 1;
        }

        match feed_result {
            Ok(segments) => {
                if triggered_inference {
                    state
                        .speech_diagnostics
                        .lock()
                        .expect("speech_diagnostics mutex poisoned")
                        .inferences_succeeded += 1;
                }
                segments
            }
            Err(e) => {
                log::error!(target: LogCategory::Speech.target(), "speech engine error: {e}");
                // Phase 1.3 speech failure recovery: the service stays
                // LIVE, previously recorded transcript/suggestions are
                // untouched - only this one chunk is dropped. The next
                // successful `feed_audio` clears this automatically.
                *state
                    .speech_error
                    .lock()
                    .expect("speech_error mutex poisoned") = Some(e.to_string());
                state
                    .speech_diagnostics
                    .lock()
                    .expect("speech_diagnostics mutex poisoned")
                    .last_error = Some(e.to_string());
                let db = state.db.lock().expect("db connection poisoned");
                record_timeline(
                    &db,
                    Some(service_id),
                    AppEvent::ErrorOccurred,
                    LogCategory::Speech,
                    serde_json::json!({ "context": "feed_audio", "error": e.to_string() }),
                );
                return;
            }
        }
    };
    *state
        .speech_error
        .lock()
        .expect("speech_error mutex poisoned") = None;

    for segment in segments {
        // Phase 3.8.7.3 Finding 4: discard output from a listening session
        // that has since been superseded by a newer `start_listening` call
        // - see this function's own docs and `docs/phase-3-8-7-3-audit.md`.
        // Checked once per non-empty result, not per chunk (buffering-only
        // calls never reach here at all).
        if state.listening_generation.load(Ordering::SeqCst) != generation {
            log::debug!(
                target: LogCategory::Speech.target(),
                "discarding transcript segment from stale listening generation {generation}"
            );
            continue;
        }

        if !segment.is_final {
            let _ = emit(app, AppEvent::TranscriptUpdated, segment);
            continue;
        }

        // Phase 4.3: Bible detection runs immediately on this raw ~3s
        // window - not on the bounded 12-20s logical segment below - so a
        // spoken reference reaches the operator in seconds, not up to 15-
        // 20s later. See `finalize_bible_only`'s own docs for why this is
        // safe (still a real, already-final Whisper segment, never a
        // partial/interim guess) and what it deliberately does not do.
        finalize_bible_only(app, &state, service_id, segment.clone());

        // Phase 3.8.7.5 Part A: accumulate this raw ~3s Whisper window into
        // a bounded 12-20s logical segment for the *other* live-connectable
        // engines (Sermon/Service/Music - see `route_segment_to_live_intelligence_engines`),
        // which do need full sentences - only a completed window is ever
        // routed there. See `segmentation.rs`.
        let Some(mut accumulated) = segmenter.push(&segment) else {
            continue;
        };
        accumulated.sequence = state.transcript_sequence.fetch_add(1, Ordering::SeqCst);
        finalize_and_route_segment(app, &state, service_id, accumulated);
    }
}

/// Phase 4.3: Bible reference detection on one raw, already-final ~3s
/// Whisper window - deliberately *not* the bounded 12-20s logical segment
/// `segmenter` produces (see `handle_audio_chunk`'s caller comment).
/// Persists its own `transcript_segments` row (so `scripture_detections`/
/// `ai_suggestions` have a real row to reference - both columns are a
/// genuine, enforced foreign key, `PRAGMA foreign_keys = ON`), runs the
/// exact same `handle_final_transcript` Bible Intelligence Core pipeline
/// `finalize_and_route_segment` used to run only once per 12-20s batch,
/// and emits `TranscriptUpdated` so the Live Transcript panel now updates
/// roughly every ~3s instead of every ~15-20s.
///
/// Never a guess: this is still a real, already-final (`is_final: true`)
/// Whisper segment - whisper.rs already never fabricates interim output
/// (see its own module docs) - just processed at Whisper's own natural
/// per-window cadence instead of waiting for several windows to
/// accumulate. This function is now Bible detection's only live-audio
/// entry point - the 12-20s batch (`finalize_and_route_segment`) no
/// longer re-runs it at all, so a reference is never detected twice.
/// Deliberately does **not** call
/// `route_segment_to_live_intelligence_engines`: Sermon/Service/Music
/// still need the fuller 12-20s window, unchanged, from
/// `finalize_and_route_segment`.
fn finalize_bible_only(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    service_id: Uuid,
    segment: TranscriptSegment,
) {
    let segment_for_event = segment.clone();

    let processed = {
        let db = state.db.lock().expect("db connection poisoned");
        let mut context = state
            .context_manager
            .lock()
            .expect("context_manager mutex poisoned");
        if state.embedding_ready {
            let engine = state
                .embedding_engine
                .lock()
                .expect("embedding engine lock poisoned");
            let semantic = cip_core_service::SemanticSearch {
                engine: engine.as_ref(),
                store: &state.verse_embedding_store,
            };
            crate::pipeline::handle_final_transcript_with_semantic_search(
                &db,
                state.bible_provider.as_ref(),
                &mut context,
                service_id,
                &resolve_default_translation_id(state),
                segment,
                &semantic,
            )
        } else {
            handle_final_transcript(
                &db,
                state.bible_provider.as_ref(),
                &mut context,
                service_id,
                &resolve_default_translation_id(state),
                segment,
            )
        }
    };

    match processed {
        Ok(processed) => {
            *state
                .last_transcript_at
                .lock()
                .expect("last_transcript_at mutex poisoned") = Some(chrono::Utc::now());
            let _ = emit(app, AppEvent::TranscriptUpdated, segment_for_event);
            let db = state.db.lock().expect("db connection poisoned");
            emit_processed_segment_events(app, &db, service_id, &processed);
        }
        Err(e) => {
            log::error!(target: LogCategory::Database.target(), "failed to persist raw transcript segment: {e}");
            let db = state.db.lock().expect("db connection poisoned");
            record_timeline(
                &db,
                Some(service_id),
                AppEvent::ErrorOccurred,
                LogCategory::Database,
                serde_json::json!({ "context": "finalize_bible_only", "error": e.to_string() }),
            );
        }
    }
}

/// Persists + Bible-detects one completed logical segment
/// (`handle_final_transcript`), then - Phase 3.8.7.5 Part B - routes the
/// same segment to every other live-connectable intelligence engine.
/// Called both from `handle_audio_chunk`'s normal per-window flush and
/// from `spawn_speech_worker`'s stop-mid-window flush, so both paths
/// share identical persistence/routing/event behavior.
///
/// Phase 4.3: no longer runs Bible detection itself - `finalize_bible_only`
/// already did that, per raw ~3s window, well before this bounded 12-20s
/// window even closed. Running it again here on the same underlying
/// speech (now concatenated with its neighbors) would only ever
/// re-detect the same reference and get silently deduplicated by
/// `handle_final_transcript`'s own 60s window - real but wasted work -
/// and would double-update `context_manager`'s continuation state for a
/// single spoken reference. This function still persists its own
/// `transcript_segments` row (Sermon/Service/Music's own persistence has
/// a hard, `NOT NULL` foreign key on exactly this row - see
/// `database/migrations/0008_sermon_foundation.sql`) and still routes to
/// them below, unchanged - only Bible detection and the operator-facing
/// `TranscriptUpdated` event moved to the faster lane (re-emitting the
/// same speech's text a second time, now regrouped into a bigger block,
/// would just duplicate what the operator already saw a few seconds
/// earlier).
fn finalize_and_route_segment(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    service_id: Uuid,
    segment: TranscriptSegment,
) {
    let segment_for_event = segment.clone();

    let pipeline_start = std::time::Instant::now();
    let persisted = {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::persist_transcript_segment(&db, service_id, &segment)
    };
    let pipeline_duration_ms =
        u64::try_from(pipeline_start.elapsed().as_millis()).unwrap_or(u64::MAX);
    state
        .speech_diagnostics
        .lock()
        .expect("speech_diagnostics mutex poisoned")
        .last_transcript_pipeline_duration_ms = Some(pipeline_duration_ms);

    match persisted {
        Ok(()) => {
            // Phase 2.4: the one real signal `service::transcript_freshness`
            // reads - a genuine final segment from the live audio/
            // speech pipeline, never the manual/test-mode harnesses
            // (see `AppState::last_transcript_at`'s own docs).
            *state
                .last_transcript_at
                .lock()
                .expect("last_transcript_at mutex poisoned") = Some(chrono::Utc::now());
            route_segment_to_live_intelligence_engines(app, state, service_id, &segment_for_event);
        }
        Err(e) => {
            log::error!(target: LogCategory::Database.target(), "failed to persist transcript segment: {e}");
            // Phase 1.3 database failure recovery: report clearly,
            // never silently swallow - this segment's persistence is
            // lost, but the service/runtime state is untouched and
            // the next segment still gets a fresh attempt.
            let db = state.db.lock().expect("db connection poisoned");
            record_timeline(
                &db,
                Some(service_id),
                AppEvent::ErrorOccurred,
                LogCategory::Database,
                serde_json::json!({ "context": "persist_transcript_segment", "error": e.to_string() }),
            );
        }
    }
}

/// Phase 3.8.7.5 Part B - the Live Intelligence Router
/// (`docs/phase-3-8-7-4-audit.md`'s "smallest safe insertion point",
/// `docs/phase-3-8-7-5-audit.md`). Runs immediately after Bible detection
/// on the exact same bounded logical segment `handle_final_transcript`
/// just persisted, calling each other live-connectable domain's
/// already-tested `analyze_and_queue` the same way its own manual Tauri
/// command already does - no new engine logic, no new database schema,
/// no new event contracts, only a new caller.
///
/// Deliberately excludes Cross-Domain Correlation and Content
/// Intelligence: both are explicitly documented, in their own doc
/// comments, as "an explicit operator/diagnostic action, never triggered
/// automatically by a transcript segment arriving" - a considered
/// design decision from Phase 2.4/2.7/2.8 this router does not reverse.
///
/// Builds one `IntelligenceContext` and reuses it for all three engines
/// below, rather than each rebuilding/re-locking it independently like
/// three separate manual commands would.
fn route_segment_to_live_intelligence_engines(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    service_id: Uuid,
    segment: &TranscriptSegment,
) {
    let context = match build_music_context(state, service_id) {
        Ok(context) => context,
        Err(e) => {
            log::error!(
                target: LogCategory::App.target(),
                "live intelligence router: failed to build context: {e}"
            );
            return;
        }
    };

    route_segment_to_sermon(app, state, service_id, segment, &context);
    route_segment_to_service(app, state, service_id, segment, &context);
    route_segment_to_music_text(app, state, service_id, segment, &context);
}

/// Covers Sermon Intelligence **and** Prayer detection: `PrayerPoint` is
/// a `SermonElementKind` this engine already detects internally
/// (`core/sermon/src/detection.rs`) - no separate call is needed or
/// exists (`docs/phase-3-8-7-4-audit.md`). Mirrors
/// `analyze_sermon_transcript`'s post-context logic exactly.
fn route_segment_to_sermon(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    service_id: Uuid,
    segment: &TranscriptSegment,
    context: &IntelligenceContext,
) {
    let before = state.sermon_engine.snapshot();
    let input = IntelligenceInput::new(service_id, segment.clone());
    let queued = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        match crate::sermon::analyze_and_queue(&state.sermon_engine, &input, context, &mut findings)
        {
            Ok(queued) => queued,
            Err(e) => {
                log::warn!(
                    target: LogCategory::App.target(),
                    "live intelligence router: sermon analysis failed: {e}"
                );
                return;
            }
        }
    };
    let after = state.sermon_engine.snapshot();

    for finding in &queued {
        let _ = emit(app, AppEvent::SermonFindingDetected, finding.clone());
    }
    if after.state != before.state {
        let _ = emit(app, AppEvent::SermonStateChanged, after.state);
    }
    if after.theme != before.theme {
        let _ = emit(app, AppEvent::SermonThemeChanged, after.theme.clone());
    }
    if after.points.len() != before.points.len()
        || after.points.last().map(|p| p.sub_points.len())
            != before.points.last().map(|p| p.sub_points.len())
    {
        let _ = emit(app, AppEvent::SermonStructureUpdated, after.points.clone());
    }
}

/// Covers Service Phase Intelligence **and** Worship detection:
/// `ServicePhase::Worship` is a phase this engine already detects
/// internally (`core/intelligence/src/service_adapter.rs`) - no separate
/// call is needed or exists (`docs/phase-3-8-7-4-audit.md`). Mirrors
/// `analyze_service_transcript`'s post-context logic exactly.
fn route_segment_to_service(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    service_id: Uuid,
    segment: &TranscriptSegment,
    context: &IntelligenceContext,
) {
    let input = IntelligenceInput::new(service_id, segment.clone());
    let queued = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        match crate::service::analyze_and_queue(
            &state.service_engine,
            &input,
            context,
            &mut findings,
        ) {
            Ok(queued) => queued,
            Err(e) => {
                log::warn!(
                    target: LogCategory::App.target(),
                    "live intelligence router: service analysis failed: {e}"
                );
                return;
            }
        }
    };
    if queued.is_empty() {
        return;
    }
    {
        let db = state.db.lock().expect("db connection poisoned");
        for finding in &queued {
            let event = if crate::service::is_anomaly_finding(finding) {
                AppEvent::ServiceAnomalyDetected
            } else {
                AppEvent::ServicePhaseChanged
            };
            record_timeline(
                &db,
                Some(service_id),
                event,
                LogCategory::App,
                serde_json::json!({
                    "findingId": finding.id,
                    "summary": &finding.summary,
                    "confidence": finding.confidence.score,
                }),
            );
        }
    }
    for finding in &queued {
        let event = if crate::service::is_anomaly_finding(finding) {
            AppEvent::ServiceAnomalyDetected
        } else {
            AppEvent::ServicePhaseChanged
        };
        let _ = emit(app, event, finding.clone());
    }
}

/// Music's text/lyric path - included because Phase 3.8.7.4's audit
/// found this exact engine already built to accept arbitrary transcript
/// text safely: its own distinctiveness/confidence gating already
/// returns zero findings for non-lyric prose (Phase 2.1's own design),
/// so routing ordinary sermon speech through it is not a new safety
/// concern this router introduces. Mirrors `analyze_music_transcript`'s
/// post-context logic exactly.
fn route_segment_to_music_text(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    service_id: Uuid,
    segment: &TranscriptSegment,
    context: &IntelligenceContext,
) {
    let Some(engine) = state
        .intelligence_registry
        .resolve(IntelligenceDomain::Music)
    else {
        // Not registered in this build - `lib.rs` always registers it
        // today, so this is defensive, not an expected runtime state.
        return;
    };
    let input = IntelligenceInput::new(service_id, segment.clone());
    let queued = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        match music::analyze_and_queue(engine, &input, context, &mut findings) {
            Ok(queued) => queued,
            Err(e) => {
                log::warn!(
                    target: LogCategory::App.target(),
                    "live intelligence router: music transcript analysis failed: {e}"
                );
                return;
            }
        }
    };
    if queued.is_empty() {
        return;
    }
    {
        let db = state.db.lock().expect("db connection poisoned");
        for finding in &queued {
            record_timeline(
                &db,
                Some(service_id),
                AppEvent::MusicFindingDetected,
                LogCategory::Music,
                serde_json::json!({
                    "findingId": finding.id,
                    "summary": &finding.summary,
                    "confidence": finding.confidence.score,
                }),
            );
        }
    }
    for finding in &queued {
        let _ = emit(app, AppEvent::MusicFindingDetected, finding.clone());
    }
}

fn emit_processed_segment_events(
    app: &AppHandle,
    conn: &Connection,
    service_id: Uuid,
    processed: &cip_core_service::ProcessedSegment,
) {
    for detection in &processed.detections {
        let event = match detection.kind {
            ReferenceKind::Unresolved => continue, // too frequent/noisy to be useful as an event
            ReferenceKind::Sequential => AppEvent::ScriptureUpdated,
            _ => AppEvent::ScriptureDetected,
        };
        record_timeline(
            conn,
            Some(service_id),
            event,
            LogCategory::Bible,
            serde_json::json!({
                "kind": detection.kind.label(),
                "reference": detection.reference.as_ref().map(|r| r.to_string()),
                "rawText": detection.raw_text,
                "confidence": detection.confidence.score,
            }),
        );
        let _ = emit(app, event, detection.clone());
    }
    for suggestion in &processed.suggestions {
        record_timeline(
            conn,
            Some(service_id),
            AppEvent::SuggestionCreated,
            LogCategory::Ai,
            serde_json::json!({
                "suggestionId": suggestion.id,
                "kind": &suggestion.kind,
                "confidence": suggestion.confidence.score,
            }),
        );
        let _ = emit(app, AppEvent::SuggestionCreated, suggestion.clone());
    }
}

// --- deterministic transcript harness (also the manual-entry bridge) ------

/// The Phase 1.1 deterministic test harness, exposed over IPC: feeds
/// `text` through exactly the same `handle_final_transcript` path real
/// audio uses, without needing a microphone or speech model. Useful both
/// for manual testing and as an operator fallback when speech recognition
/// is unavailable but the pastor's reference is known.
#[tauri::command]
pub fn process_test_transcript(
    text: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<cip_core_service::ProcessedSegment, AppError> {
    let text = require_non_empty(&text, "text").map_err(log_and_return)?;
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let sequence = state.transcript_sequence.fetch_add(1, Ordering::SeqCst);

    let segment = TranscriptSegment {
        id: Uuid::new_v4(),
        sequence,
        text,
        is_final: true,
        confidence: ConfidenceResult::new(
            1.0,
            ConfidenceSource::Human,
            Some("manually entered test transcript".to_string()),
        ),
        start_ms: 0,
        end_ms: 0,
        language: Some("en".to_string()),
        speaker_id: None,
    };

    let processed = {
        let db = state.db.lock().expect("db connection poisoned");
        let mut context = state
            .context_manager
            .lock()
            .expect("context_manager mutex poisoned");
        if state.embedding_ready {
            let engine = state
                .embedding_engine
                .lock()
                .expect("embedding engine lock poisoned");
            let semantic = cip_core_service::SemanticSearch {
                engine: engine.as_ref(),
                store: &state.verse_embedding_store,
            };
            crate::pipeline::handle_final_transcript_with_semantic_search(
                &db,
                state.bible_provider.as_ref(),
                &mut context,
                service_id,
                &resolve_default_translation_id(&state),
                segment,
                &semantic,
            )
            .map_err(AppError::from)
            .map_err(log_and_return)?
        } else {
            handle_final_transcript(
                &db,
                state.bible_provider.as_ref(),
                &mut context,
                service_id,
                &resolve_default_translation_id(&state),
                segment,
            )
            .map_err(AppError::from)
            .map_err(log_and_return)?
        }
    };

    {
        let db = state.db.lock().expect("db connection poisoned");
        emit_processed_segment_events(&app, &db, service_id, &processed);
    }
    Ok(processed)
}

// --- transcript & suggestions -----------------------------------------------

/// Resolves an explicit `service_id` override (for the Phase 1.3 service
/// archive - inspecting a *completed* service's transcript without
/// disturbing whichever service, if any, is currently live) or falls back
/// to the active service when `None`, matching every Phase 1.0-1.2 call
/// site that never passed one.
fn resolve_service_id(
    state: &State<'_, AppState>,
    service_id: Option<String>,
) -> Result<Uuid, AppError> {
    match service_id {
        Some(id) => parse_uuid(&id),
        None => current_service_id(state),
    }
}

#[tauri::command]
pub fn list_transcript(
    limit: u32,
    service_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<TranscriptSegment>, AppError> {
    let service_id = resolve_service_id(&state, service_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::list_transcript_segments(&db, service_id, limit)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

fn parse_suggestion_status(value: &str) -> Result<SuggestionStatus, AppError> {
    match value {
        "pending" => Ok(SuggestionStatus::Pending),
        "approved" => Ok(SuggestionStatus::Approved),
        "edited" => Ok(SuggestionStatus::Edited),
        "rejected" => Ok(SuggestionStatus::Rejected),
        other => Err(AppError::InvalidInput(format!(
            "unknown suggestion status: {other}"
        ))),
    }
}

#[tauri::command]
pub fn list_suggestions(
    status: Option<String>,
    service_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<Suggestion>, AppError> {
    let service_id = resolve_service_id(&state, service_id).map_err(log_and_return)?;
    let status_filter = status
        .as_deref()
        .map(parse_suggestion_status)
        .transpose()
        .map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::list_suggestions(&db, service_id, status_filter)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// The service timeline (Phase 1.3) - same `service_id`-override pattern
/// as `list_transcript`/`list_suggestions`.
#[tauri::command]
pub fn list_timeline(
    service_id: Option<String>,
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<TimelineEntry>, AppError> {
    let service_id = resolve_service_id(&state, service_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    timeline::list_timeline(&db, service_id, limit)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// Completed services, most recent first - the service archive's list
/// view (Phase 1.3 section 34).
#[tauri::command]
pub fn list_service_history(
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<ServiceSession>, AppError> {
    let db = state.db.lock().expect("db connection poisoned");
    persistence::list_services(&db, Some(ServiceStatus::Ended), limit)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// A single service by id, independent of whichever one (if any) is
/// currently active - the service archive's detail view.
#[tauri::command]
pub fn get_service(
    service_id: String,
    state: State<'_, AppState>,
) -> Result<ServiceSession, AppError> {
    let id = parse_uuid(&service_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::get_service(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// The Phase 5.1 post-service observability report for a single service -
/// a read-only aggregation of already-persisted data plus a labeled,
/// process-lifetime diagnostics snapshot. See `service_report.rs` for why
/// the diagnostics half is honestly scoped "since app launch," not
/// service-specific.
#[tauri::command]
pub fn get_service_report(
    service_id: String,
    state: State<'_, AppState>,
) -> Result<crate::service_report::ServiceReport, AppError> {
    let id = parse_uuid(&service_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    let speech_diagnostics = state
        .speech_diagnostics
        .lock()
        .expect("speech_diagnostics mutex poisoned")
        .clone();
    let embedding_diagnostics = state
        .embedding_diagnostics
        .lock()
        .expect("embedding diagnostics lock poisoned")
        .clone();
    crate::service_report::build_service_report(
        &db,
        id,
        &speech_diagnostics,
        &embedding_diagnostics,
        state.embedding_ready,
    )
    .map_err(AppError::from)
    .map_err(log_and_return)
}

#[tauri::command]
pub fn approve_suggestion(
    suggestion_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Suggestion, AppError> {
    let id = parse_uuid(&suggestion_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    let current = persistence::get_suggestion(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    ensure_suggestion_editable(current.status, "approve").map_err(log_and_return)?;
    let updated = persistence::update_suggestion_status(&db, id, SuggestionStatus::Approved, None)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    record_timeline(
        &db,
        Some(updated.service_id),
        AppEvent::SuggestionApproved,
        LogCategory::Ai,
        serde_json::json!({ "suggestionId": updated.id, "kind": &updated.kind }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::SuggestionApproved, updated.clone());
    Ok(updated)
}

/// Operator correction of a suggestion before approval. Per section 17,
/// the edited reference must be a real, Bible-validated verse - an
/// operator typo or a nonexistent verse number must never be allowed to
/// become `Approved`, exactly like an automatically-detected reference
/// never would.
#[tauri::command]
pub fn edit_suggestion(
    suggestion_id: String,
    new_reference: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Suggestion, AppError> {
    let id = parse_uuid(&suggestion_id).map_err(log_and_return)?;
    let new_reference =
        require_non_empty(&new_reference, "new_reference").map_err(log_and_return)?;

    let (book, chapter, verse) = parse_display_reference(&new_reference).map_err(log_and_return)?;
    let reference = ScriptureReference::single(
        resolve_default_translation_id(&state),
        &book,
        chapter,
        verse,
    );
    state
        .bible_provider
        .get_verse(&reference)
        .map_err(AppError::from)
        .map_err(log_and_return)?
        .ok_or_else(|| {
            log_and_return(AppError::InvalidInput(format!(
                "not a real verse in the current translation: {new_reference}"
            )))
        })?;

    let db = state.db.lock().expect("db connection poisoned");
    let original = persistence::get_suggestion(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    ensure_suggestion_editable(original.status, "edit").map_err(log_and_return)?;
    let kind = SuggestionKind::Scripture {
        reference: reference.to_string(),
    };
    let updated =
        persistence::update_suggestion_status(&db, id, SuggestionStatus::Edited, Some(&kind))
            .map_err(AppError::from)
            .map_err(log_and_return)?;
    record_timeline(
        &db,
        Some(updated.service_id),
        AppEvent::SuggestionEdited,
        LogCategory::Ai,
        serde_json::json!({ "suggestionId": updated.id, "original": &original.kind, "edited": &updated.kind }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::SuggestionEdited, updated.clone());
    Ok(updated)
}

#[tauri::command]
pub fn reject_suggestion(
    suggestion_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Suggestion, AppError> {
    let id = parse_uuid(&suggestion_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    let current = persistence::get_suggestion(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    ensure_suggestion_editable(current.status, "reject").map_err(log_and_return)?;
    let updated = persistence::update_suggestion_status(&db, id, SuggestionStatus::Rejected, None)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    record_timeline(
        &db,
        Some(updated.service_id),
        AppEvent::SuggestionRejected,
        LogCategory::Ai,
        serde_json::json!({ "suggestionId": updated.id, "kind": &updated.kind }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::SuggestionRejected, updated.clone());
    Ok(updated)
}

/// `"ROM 8:28"` -> `("ROM", 8, 28)` - reverses `ScriptureReference`'s own
/// `Display` impl. Only ever applied to text this application generated
/// itself (a `Suggestion`'s reference string), never to raw user input.
fn parse_display_reference(text: &str) -> Result<(String, u32, u32), AppError> {
    let invalid =
        || AppError::InvalidInput(format!("not a recognized scripture reference: {text}"));
    let (book, rest) = text.rsplit_once(' ').ok_or_else(invalid)?;
    let (chapter_str, verse_str) = rest.split_once(':').ok_or_else(invalid)?;
    let chapter: u32 = chapter_str.parse().map_err(|_| invalid())?;
    let verse: u32 = verse_str
        .split('-')
        .next()
        .unwrap_or(verse_str)
        .parse()
        .map_err(|_| invalid())?;
    Ok((book.to_string(), chapter, verse))
}

/// Non-mutating render of a scripture reference - the shared core behind
/// both `preview_presentation` and `preview_scripture` below. Never
/// persists anything and never requires an active service (Phase 1.4
/// section 14): the operator can preview before approving, before a
/// service even exists to attach a prepared item to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationPreview {
    pub content: PresentationContent,
    pub slide: RenderedSlide,
}

/// The `PresentationStarted` event payload - both the updated `PresentationItem`
/// (so the operator's own "Current Output" panel can flip its status) and
/// the already-rendered `RenderedSlide` (so the display window has exactly
/// what it needs to show, with no second rendering system in the frontend
/// and no database internals exposed - spec sections 14/15).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationDisplayPayload {
    pub item: PresentationItem,
    pub slide: RenderedSlide,
}

fn preview_reference(
    reference: &str,
    translation_id: &str,
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<PresentationPreview, AppError> {
    ensure_translation_selectable(state, translation_id).map_err(log_and_return)?;
    let (content, slide) = presentation::build_scripture_slide(
        state.bible_provider.as_ref(),
        translation_id,
        reference,
    )
    .map_err(AppError::from)
    .map_err(log_and_return)?;
    let preview = PresentationPreview { content, slide };
    let _ = emit(app, AppEvent::PresentationPreviewed, preview.clone());
    Ok(preview)
}

/// Previews a suggestion's scripture reference - available before
/// approval, unlike `prepare_presentation` (section 14: "Preview and
/// Prepare are separate actions"). This is the fix for the pre-1.4 UI bug
/// where the operator's "Preview" button called the approval-gated
/// prepare command directly. `translationId` is optional and defaults to
/// `DEFAULT_TRANSLATION_ID` (unchanged behavior for every existing
/// caller); passing one explicitly lets the operator preview against a
/// different installed, enabled translation (e.g. the real production
/// dataset) without CIP ever silently substituting one translation for
/// another (section 21).
#[tauri::command]
pub fn preview_presentation(
    suggestion_id: String,
    translation_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PresentationPreview, AppError> {
    let id = parse_uuid(&suggestion_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    let suggestion = persistence::get_suggestion(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    drop(db);
    ensure_suggestion_previewable(suggestion.status).map_err(log_and_return)?;
    let SuggestionKind::Scripture { reference } = &suggestion.kind else {
        return Err(log_and_return(AppError::InvalidInput(
            "suggestion is not a scripture reference".to_string(),
        )));
    };
    let translation_id = translation_id.unwrap_or_else(|| resolve_default_translation_id(&state));
    preview_reference(reference, &translation_id, &app, &state)
}

/// Previews an arbitrary scripture reference with no suggestion involved -
/// the manual Bible search path's preview (section 5/20: manual creation
/// must work independently of speech recognition). `translationId` is
/// optional; see `preview_presentation`'s docs.
#[tauri::command]
pub fn preview_scripture(
    reference: String,
    translation_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PresentationPreview, AppError> {
    let reference = require_non_empty(&reference, "reference").map_err(log_and_return)?;
    let translation_id = translation_id.unwrap_or_else(|| resolve_default_translation_id(&state));
    preview_reference(&reference, &translation_id, &app, &state)
}

/// Prepares (never projects) a presentation item from an approved
/// suggestion. There is no "active"/projected state anywhere in this
/// command - see `docs/live-speech.md`'s "no automatic projection"
/// section.
#[tauri::command]
pub fn prepare_presentation(
    suggestion_id: String,
    translation_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PresentationItem, AppError> {
    let id = parse_uuid(&suggestion_id).map_err(log_and_return)?;
    let translation_id = translation_id.unwrap_or_else(|| resolve_default_translation_id(&state));
    ensure_translation_selectable(&state, &translation_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    let suggestion = persistence::get_suggestion(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)?;

    presentation::ensure_suggestion_approved(suggestion.status)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    let SuggestionKind::Scripture { reference } = &suggestion.kind else {
        return Err(log_and_return(AppError::from(
            presentation::PresentationError::SuggestionNotScripture,
        )));
    };

    let (content, _slide) = presentation::build_scripture_slide(
        state.bible_provider.as_ref(),
        &translation_id,
        reference,
    )
    .map_err(AppError::from)
    .map_err(log_and_return)?;

    let item = presentation::persist_prepared_item(
        &db,
        suggestion.service_id,
        content,
        SCRIPTURE_DEFAULT_TEMPLATE,
        Some(suggestion.id),
    )
    .map_err(AppError::from)
    .map_err(log_and_return)?;
    record_timeline(
        &db,
        Some(item.service_id),
        AppEvent::PresentationPrepared,
        LogCategory::Presentation,
        serde_json::json!({
            "presentationItemId": item.id,
            "reference": reference,
            "sourceSuggestionId": item.source_suggestion_id,
            "template": item.template,
        }),
    );
    drop(db);

    let _ = emit(&app, AppEvent::PresentationPrepared, item.clone());
    Ok(item)
}

/// Creates a prepared presentation item directly from a reference, with no
/// suggestion involved - the manual fallback (section 5/20) that keeps
/// presentation preparation working with no audio, no speech engine, and
/// no network. Requires an active service to attach the item to (the
/// schema's `service_id` is not nullable), same as every other
/// service-scoped write in this file.
#[tauri::command]
pub fn create_manual_presentation(
    reference: String,
    translation_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PresentationItem, AppError> {
    let reference = require_non_empty(&reference, "reference").map_err(log_and_return)?;
    let translation_id = translation_id.unwrap_or_else(|| resolve_default_translation_id(&state));
    ensure_translation_selectable(&state, &translation_id).map_err(log_and_return)?;
    let service_id = current_service_id(&state).map_err(log_and_return)?;

    let (content, _slide) = presentation::build_scripture_slide(
        state.bible_provider.as_ref(),
        &translation_id,
        &reference,
    )
    .map_err(AppError::from)
    .map_err(log_and_return)?;

    let db = state.db.lock().expect("db connection poisoned");
    let item = presentation::persist_prepared_item(
        &db,
        service_id,
        content,
        SCRIPTURE_DEFAULT_TEMPLATE,
        None,
    )
    .map_err(AppError::from)
    .map_err(log_and_return)?;
    record_timeline(
        &db,
        Some(service_id),
        AppEvent::PresentationPrepared,
        LogCategory::Presentation,
        serde_json::json!({ "presentationItemId": item.id, "reference": reference, "manual": true }),
    );
    drop(db);

    let _ = emit(&app, AppEvent::PresentationPrepared, item.clone());
    Ok(item)
}

/// What's currently prepared for the active service - the "Current
/// Output" panel's data source (section 27). Never includes cancelled
/// (`Stopped`) items.
#[tauri::command]
pub fn list_prepared_presentations(
    state: State<'_, AppState>,
) -> Result<Vec<PresentationItem>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::list_presentation_items(&db, service_id, Some(PresentationItemStatus::Prepared))
        .map_err(AppError::from)
        .map_err(log_and_return)
}

#[tauri::command]
pub fn get_presentation_item(
    item_id: String,
    state: State<'_, AppState>,
) -> Result<PresentationItem, AppError> {
    let id = parse_uuid(&item_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::get_presentation_item(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// Presentation History (Phase 3.6): every presentation item ever prepared
/// for a given (possibly past, possibly the live) service, regardless of
/// status - unlike `list_prepared_presentations` above, which is
/// deliberately hardcoded to the live service's still-`Prepared` items
/// only. This is not a new persistence mechanism: `presentation_items`
/// already records `service_id` for every item and already survives a
/// restart (see docs/phase-3-6-church-libraries.md's audit); this command
/// only exposes the existing `persistence::list_presentation_items` with
/// an operator-supplied `service_id` instead of the hardcoded live one.
#[tauri::command]
pub fn list_presentation_history(
    service_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<PresentationItem>, AppError> {
    let id = parse_uuid(&service_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::list_presentation_items(&db, id, None)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// Cancels ("retracts") a still-prepared item before it's ever displayed.
/// Only valid from `Prepared` - an already-cancelled item cannot be
/// cancelled again.
#[tauri::command]
pub fn cancel_presentation(
    item_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PresentationItem, AppError> {
    let id = parse_uuid(&item_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    let item = presentation::cancel_item(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    record_timeline(
        &db,
        Some(item.service_id),
        AppEvent::PresentationCancelled,
        LogCategory::Presentation,
        serde_json::json!({ "presentationItemId": item.id }),
    );
    drop(db);

    let _ = emit(&app, AppEvent::PresentationCancelled, item.clone());
    Ok(item)
}

// --- local presentation display -------------------------------------------
//
// The first real, local, on-screen output for a prepared presentation item
// - a dedicated Tauri window under direct operator control (never anything
// automatic - see `presentation_display.rs`'s module docs and
// `docs/presentation.md`'s "Local display architecture" section). Reuses
// `PresentationItemStatus::Active`/`Stopped`, the `PresentationStarted`/
// `PresentationStopped` events, and the same timeline/error conventions
// every other presentation command already established above - nothing
// here invents a second lifecycle, error hierarchy, or event bus.

/// Parses a `screen` command argument (`"stage"`/`"confidence"`/`"lobby"`)
/// into a [`presentation_display::DisplayScreen`], or a clean
/// `AppError::Presentation(UnknownDisplayScreen)` for anything else -
/// shared by every Phase 3.10 screen-parametrized command below so the
/// rejection message and error type stay identical across all of them.
fn parse_display_screen(screen: &str) -> Result<presentation_display::DisplayScreen, AppError> {
    presentation_display::DisplayScreen::parse(screen).ok_or_else(|| {
        AppError::from(presentation::PresentationError::UnknownDisplayScreen(
            screen.to_string(),
        ))
    })
}

/// Resolves which physical monitor (if any) `screen` should open on, per
/// the Display Registry (Phase 3.10.2) - `None` when nothing is assigned
/// to that screen's role, or the assigned monitor is not currently
/// connected, in which case the caller passes `None` through to
/// `open_display_window` and gets the exact pre-3.10.2 unpositioned
/// behavior. Never fails the caller's own command: enumeration/lookup
/// errors are treated the same as "nothing assigned" (best-effort, since
/// a Display Registry problem must never block the operator from
/// displaying something at all).
fn resolve_screen_placement(
    app: &AppHandle,
    state: &AppState,
    screen: presentation_display::DisplayScreen,
) -> Option<display_registry::MonitorPlacement> {
    let physical = display_registry::enumerate_monitors(app);
    let assignments = {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::list_display_role_assignments(&db).ok()?
    };
    let displays = display_registry::merge_displays(physical, &assignments);
    display_registry::resolve_role_position(&displays, display_registry::screen_role(screen))
}

/// Delivers a presentation event (`PresentationStarted`/`PresentationStopped`)
/// only to the screens currently `Live` (Phase 3.10.3) - a `Held` screen's
/// window, even if open, does not receive it, and stays frozen on whatever
/// it currently shows. Replaces the pre-3.10.3 unconditional broadcast
/// (`events::emit`, which every open screen received with no way to opt
/// out); a screen missing from `state.screen_route_modes` is `Live`, so
/// with nothing ever set this behaves identically to the pre-3.10.3
/// broadcast. Errors from any one target window are logged and do not
/// stop delivery to the others.
fn broadcast_to_live_screens(
    app: &AppHandle,
    state: &AppState,
    event: AppEvent,
    payload: impl Serialize + Clone,
) {
    let open_screens: Vec<_> = presentation_display::DisplayScreen::ALL
        .into_iter()
        .filter(|s| presentation_display::is_display_window_open(app, *s))
        .collect();
    let modes = state
        .screen_route_modes
        .lock()
        .expect("screen_route_modes mutex poisoned")
        .clone();
    for screen in presentation_router::screens_to_broadcast(&open_screens, &modes) {
        if let Err(e) = emit_to(app, screen.window_label(), event, payload.clone()) {
            log::warn!(
                target: crate::logging::LogCategory::Presentation.target(),
                "failed to deliver {} to {}: {e}",
                event.name(),
                screen.window_label()
            );
        }
    }
}

/// Opens (or, if already open, focuses) `screen`'s presentation display
/// window - useful on its own for positioning it on a projector/second
/// monitor before anything is ready to show. `display_presentation` calls
/// this internally for the Stage screen specifically when needed; the
/// Confidence Monitor and Lobby/Overflow screens are only ever opened via
/// this command, on explicit operator request (Phase 3.10 - see
/// `docs/phase-3-10-multi-screen-audit.md`). Phase 3.10.2: opens directly
/// on the monitor assigned that screen's role in the Display Registry,
/// when one is connected - see `resolve_screen_placement`.
///
/// Phase 3.8.4: `async fn`, not a plain synchronous command - real
/// Windows testing showed the display window appearing but staying
/// completely white (WebView2's own default background, never this
/// app's CSS). The vendored Tauri crate's own docs on
/// `WebviewWindowBuilder::new`/`build` name this exact scenario as a
/// documented deadlock on Windows when called from a synchronous command
/// (https://github.com/tauri-apps/wry/issues/583); this command's name,
/// parameters, and return type are unchanged, so the JS command contract
/// in `commands.ts` requires no changes - `invoke()` already returns a
/// `Promise` regardless. See `docs/phase-3-8-4-audit.md` section D.
#[tauri::command]
pub async fn open_presentation_display(
    screen: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let screen = parse_display_screen(&screen).map_err(log_and_return)?;
    let placement = resolve_screen_placement(&app, &state, screen);
    presentation_display::open_display_window(&app, screen, placement)
        .map_err(|e| {
            AppError::from(presentation::PresentationError::DisplayUnavailable(
                e.to_string(),
            ))
        })
        .map_err(log_and_return)
}

/// One screen's open/closed state, as reported to the operator UI -
/// Phase 3.10. `route_mode` (Phase 3.10.3) is `"live"`/`"held"` regardless
/// of `window_open` - a screen's route mode is independent of whether its
/// window currently exists, matching `screen_route_modes`' own semantics.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationScreenState {
    pub screen: String,
    pub label: String,
    pub window_open: bool,
    pub route_mode: String,
}

/// Which display screens currently exist, which item (if any) is
/// currently `Active` for the active service, and that item already
/// rendered - the operator UI's sync point on mount, never assumed from
/// local state alone.
///
/// `active_slide` (Phase 3.8.2) exists specifically so a display window
/// itself can hydrate on mount rather than depending solely on catching
/// the `PRESENTATION_STARTED` event live: `WebviewWindowBuilder::build()`
/// returning in Rust does not mean the new window's JavaScript has loaded
/// and subscribed to events yet, so an event emitted immediately after
/// window creation (exactly what `display_presentation` does) can be
/// missed entirely, leaving the display permanently blank. Computed via
/// the same pure, deterministic `render_content` `display_presentation`
/// already calls for the live-event payload - no second rendering system,
/// no new command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationDisplayState {
    pub screens: Vec<PresentationScreenState>,
    pub active_item: Option<PresentationItem>,
    pub active_slide: Option<RenderedSlide>,
}

#[tauri::command]
pub fn get_presentation_display_state(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PresentationDisplayState, AppError> {
    let active_item = match current_service_id(&state) {
        Ok(service_id) => {
            let db = state.db.lock().expect("db connection poisoned");
            persistence::list_presentation_items(
                &db,
                service_id,
                Some(PresentationItemStatus::Active),
            )
            .map_err(AppError::from)
            .map_err(log_and_return)?
            .into_iter()
            .next()
        }
        Err(_) => None,
    };
    let active_slide = active_item.as_ref().and_then(|item| {
        render_content(&item.content)
            .map_err(|e| {
                log::warn!(
                    target: crate::logging::LogCategory::Presentation.target(),
                    "failed to re-render the active presentation item for display hydration: {e}"
                );
            })
            .ok()
    });
    let route_modes = state
        .screen_route_modes
        .lock()
        .expect("screen_route_modes mutex poisoned")
        .clone();
    let screens: Vec<PresentationScreenState> = presentation_display::DisplayScreen::ALL
        .into_iter()
        .map(|screen| PresentationScreenState {
            screen: screen.id().to_string(),
            label: screen.operator_label().to_string(),
            window_open: presentation_display::is_display_window_open(&app, screen),
            route_mode: route_modes
                .get(&screen)
                .copied()
                .unwrap_or(RouteMode::Live)
                .as_str()
                .to_string(),
        })
        .collect();
    log::info!(
        target: crate::logging::LogCategory::Presentation.target(),
        "[diagnostic] get_presentation_display_state (checkpoint 5/6): screensOpen={} activeItem={} activeSlide={}",
        screens.iter().filter(|s| s.window_open).count(),
        active_item.is_some(),
        active_slide.is_some()
    );
    Ok(PresentationDisplayState {
        screens,
        active_item,
        active_slide,
    })
}

/// Displays a still-`Prepared` item for real: renders it, opens the display
/// window if needed, and only then commits `Prepared -> Active` - never
/// the other way around (spec section 8/28: an item is never marked
/// `Active` before the real display operation has actually succeeded, and
/// nothing but this explicit operator action may cross that boundary).
///
/// Phase 3.8.4: `async fn` for the same reason as
/// `open_presentation_display` above - this command is the one the
/// manual detect->approve->prepare->Display click path actually exercises,
/// and it calls the same `open_display_window` whose window creation
/// deadlocks on Windows when invoked from a synchronous command. See
/// `docs/phase-3-8-4-audit.md` section D.
#[tauri::command]
pub async fn display_presentation(
    item_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PresentationItem, AppError> {
    let id = parse_uuid(&item_id).map_err(log_and_return)?;

    let db = state.db.lock().expect("db connection poisoned");
    let (_item, slide) = presentation::prepare_to_activate(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    drop(db); // release before the window-manager call below

    log::info!(
        target: crate::logging::LogCategory::Presentation.target(),
        "[diagnostic] display_presentation: render_content produced heading={:?} bodyLines={} footer={:?} (checkpoint 13)",
        slide.heading,
        slide.body_lines.len(),
        slide.footer
    );

    // Phase 3.10: always opens Stage specifically, preserving this
    // command's pre-3.10 contract exactly. Confidence Monitor/Lobby are
    // opened separately, by explicit operator choice, via
    // `open_presentation_display` - once open they receive the same
    // broadcast `PresentationStarted` event below with no separate path.
    // Phase 3.10.2: opens directly on the monitor assigned the Projector
    // role, when one is connected - see `resolve_screen_placement`.
    let placement =
        resolve_screen_placement(&app, &state, presentation_display::DisplayScreen::Stage);
    presentation_display::open_display_window(
        &app,
        presentation_display::DisplayScreen::Stage,
        placement,
    )
    .map_err(|e| {
        AppError::from(presentation::PresentationError::DisplayUnavailable(
            e.to_string(),
        ))
    })
    .map_err(log_and_return)?;

    let db = state.db.lock().expect("db connection poisoned");
    let activated = presentation::commit_activation(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    record_timeline(
        &db,
        Some(activated.service_id),
        AppEvent::PresentationStarted,
        LogCategory::Presentation,
        serde_json::json!({ "presentationItemId": activated.id }),
    );
    drop(db);

    log::info!(
        target: crate::logging::LogCategory::Presentation.target(),
        "[diagnostic] display_presentation: about to emit PresentationStarted for item {} (checkpoint 14 - lifecycle ordering: window opened -> activation committed -> event emitted now)",
        activated.id
    );
    // Phase 8: best-effort push to any configured OBS/vMix target, on its
    // own worker thread - never blocks or delays the broadcast below, and
    // a push failure never affects CIP's own local display.
    production::push_to_configured_targets(&app, production::slide_push_text(&slide));
    // Phase 11: update whatever the congregant companion server
    // broadcasts, in lockstep with the OBS/vMix push above - a no-op if
    // the server isn't running.
    companion::update_snapshot(&app, Some(&slide));
    broadcast_to_live_screens(
        &app,
        &state,
        AppEvent::PresentationStarted,
        PresentationDisplayPayload {
            item: activated.clone(),
            slide,
        },
    );
    Ok(activated)
}

/// Stops whichever presentation item is currently `Active` for the active
/// service, if any - blanks the display window without closing it (spec
/// section 5/9). Safe and idempotent when nothing is active: returns
/// `Ok(None)` rather than an error, and never crashes.
///
/// Shared by the explicit operator Stop action and, via
/// [`clear_active_presentation`], the display window's own manual-close
/// reconciliation - both leave persistence in exactly the same state.
#[tauri::command]
pub fn clear_presentation_display(
    app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<Option<PresentationItem>, AppError> {
    clear_active_presentation(&app).map_err(log_and_return)
}

/// The plain-function core of [`clear_presentation_display`], callable
/// without a command's `State<'_, AppState>` extractor - `AppHandle::state`
/// reaches the same managed `AppState` either way. Used directly by
/// `presentation_display.rs`'s window-`Destroyed` handler, which has no
/// command invocation to extract state from.
pub(crate) fn clear_active_presentation(
    app: &AppHandle,
) -> Result<Option<PresentationItem>, AppError> {
    let state = app.state::<AppState>();
    let service_id = state
        .active_service
        .lock()
        .expect("active_service mutex poisoned")
        .as_ref()
        .map(|s| s.id);

    let db = state.db.lock().expect("db connection poisoned");
    let stopped = match service_id {
        Some(sid) => presentation::stop_active_item(&db, sid).map_err(AppError::from)?,
        None => None,
    };
    if let Some(ref item) = stopped {
        record_timeline(
            &db,
            Some(item.service_id),
            AppEvent::PresentationStopped,
            LogCategory::Presentation,
            serde_json::json!({ "presentationItemId": item.id }),
        );
    }
    drop(db);

    if let Some(ref item) = stopped {
        // Phase 8: blank any configured OBS/vMix target too - best-effort,
        // same discipline as the push in `display_presentation`.
        production::push_to_configured_targets(app, String::new());
        // Phase 11: clear whatever the congregant companion server
        // broadcasts too - same discipline, same lockstep timing.
        companion::update_snapshot(app, None);
        broadcast_to_live_screens(app, &state, AppEvent::PresentationStopped, item.clone());
    }
    Ok(stopped)
}

/// Closes `screen`'s presentation display window outright (as opposed to
/// `clear_presentation_display`, which blanks it but leaves it open).
///
/// Phase 3.8.2: reconciles the active item to `Stopped` *synchronously*,
/// before closing the window, rather than relying solely on the window's
/// own `Destroyed`-event handler. `window.close()` returns to this
/// command once the close is requested, not necessarily once the OS has
/// finished destroying the window and fired `Destroyed` - so a fast
/// operator (or scripted) Close-then-Reopen-and-Display-another sequence
/// could previously race ahead of that async reconciliation and hit
/// `PresentationError::AlreadyActive` on the new item's
/// `prepare_to_activate`, even though the operator had already closed the
/// display. Calling `clear_active_presentation` here first makes this
/// command's return the actual synchronization point; the `Destroyed`
/// handler still exists for a manual OS-level close (Alt+F4, window-manager
/// close), and is a safe, proven-idempotent no-op here since the item is
/// already `Stopped` by the time it fires.
///
/// Phase 3.10: only reconciles when `screen` is the *last* open screen -
/// closing one of several simultaneously open screens must never blank
/// the others that are still genuinely showing the active item to the
/// congregation/room they're driving. When exactly one screen is open
/// (the pre-3.10 case, and by far the common one), this is identical to
/// the pre-3.10 behavior: reconcile, then close.
#[tauri::command]
pub fn close_presentation_display(
    screen: String,
    app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<(), AppError> {
    let screen = parse_display_screen(&screen).map_err(log_and_return)?;

    let open_screens_before: Vec<_> = presentation_display::DisplayScreen::ALL
        .into_iter()
        .filter(|s| presentation_display::is_display_window_open(&app, *s))
        .collect();
    let is_last_open_screen = open_screens_before.len() == 1 && open_screens_before[0] == screen;

    if is_last_open_screen {
        clear_active_presentation(&app).map_err(log_and_return)?;
    }

    presentation_display::close_display_window(&app, screen)
        .map_err(|e| {
            AppError::from(presentation::PresentationError::DisplayUnavailable(
                e.to_string(),
            ))
        })
        .map_err(log_and_return)
}

/// Phase 3.8.3 TEMPORARY DIAGNOSTIC (spec section "REQUIRED TEMPORARY
/// DIAGNOSTICS"): the display window's own frontend has no other way to
/// surface what it observes - this app has no devtools/logging plugin, so
/// a secondary webview's `console.log` output is otherwise invisible to
/// the operator or to anyone reading the log file. This command exists
/// only to route the display window's own lifecycle checkpoints (mount,
/// hydration call/result, event received, payload applied) into the
/// existing log stream via the existing `log::` macros - nothing else.
/// No state is read or written, no capability beyond `core:default` is
/// needed (identical reasoning to every other command's grant), and
/// `stage`/`detail` are logged verbatim, never persisted, never sent
/// anywhere but this process's own log output.
#[tauri::command]
pub fn log_display_diagnostic(stage: String, detail: String) {
    log::info!(
        target: crate::logging::LogCategory::Presentation.target(),
        "[diagnostic] display window: {stage} - {detail}"
    );
}

// --- display registry (Phase 3.10.2) ---------------------------------------
//
// Which physical monitor plays which presentation role - global, not
// service-scoped (a property of the machine CIP runs on). Built from the
// same `AppHandle::available_monitors`/`primary_monitor` API
// `get_pilot_diagnostics` has used since Phase 3.2-3.4, now also driving
// where `presentation_display::open_display_window` actually places a
// window (see `resolve_screen_placement` above). See
// `docs/phase-3-10-2-display-registry.md`.

/// Every currently-known display - every connected monitor (role
/// `Unassigned` if nothing has been assigned yet) plus every previously
/// assigned monitor that is not currently connected, so a prior setup is
/// never silently dropped by an unplugged cable.
#[tauri::command]
pub fn list_displays(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<display_registry::Display>, AppError> {
    let physical = display_registry::enumerate_monitors(&app);
    let db = state.db.lock().expect("db connection poisoned");
    let assignments = persistence::list_display_role_assignments(&db)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    Ok(display_registry::merge_displays(physical, &assignments))
}

/// Assigns `role` (`"unassigned"`/`"operator"`/`"projector"`/`"stage"`/
/// `"confidence"`/`"lobby"`) to `monitor_id` (one of the ids `list_displays`
/// returned), replacing any prior assignment for that monitor. Takes
/// effect the next time a presentation display window for the
/// corresponding screen is opened - never moves an already-open window
/// (see `docs/phase-3-10-2-display-registry.md`'s known limitations).
#[tauri::command]
pub fn assign_display_role(
    monitor_id: String,
    role: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let parsed_role = display_registry::DisplayRole::parse(&role)
        .ok_or_else(|| AppError::InvalidInput(format!("unknown display role: {role}")))
        .map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::assign_display_role(&db, &monitor_id, parsed_role)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    log::info!(
        target: crate::logging::LogCategory::Presentation.target(),
        "assigned display role {role} to monitor {monitor_id}"
    );
    Ok(())
}

// --- presentation router (Phase 3.10.3) ------------------------------------
//
// Per-screen Live/Held routing - see `presentation_router.rs`'s module
// docs. Independent of the Display Registry above: this controls whether
// a screen currently receives the live broadcast, not where its window is
// positioned.

/// Sets `screen`'s route mode to `"live"` or `"held"` (Phase 3.10.3). A
/// screen coming back to `Live` from `Held`, with its window currently
/// open, is caught up immediately: `PresentationScreenSynced` is emitted
/// to just that window so it re-pulls current state via the same
/// hydration path it already uses on mount, rather than waiting for the
/// next live change to reach it. Going to `Held`, or setting `Live` on an
/// already-`Live`/closed screen, needs no catch-up and is a plain state
/// update.
#[tauri::command]
pub fn set_screen_route_mode(
    screen: String,
    mode: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let screen = parse_display_screen(&screen).map_err(log_and_return)?;
    let mode = RouteMode::parse(&mode)
        .ok_or_else(|| AppError::InvalidInput(format!("unknown route mode: {mode}")))
        .map_err(log_and_return)?;

    let previous = {
        let mut modes = state
            .screen_route_modes
            .lock()
            .expect("screen_route_modes mutex poisoned");
        modes.insert(screen, mode)
    };

    let became_live = mode == RouteMode::Live && previous != Some(RouteMode::Live);
    if became_live && presentation_display::is_display_window_open(&app, screen) {
        if let Err(e) = emit_to(
            &app,
            screen.window_label(),
            AppEvent::PresentationScreenSynced,
            (),
        ) {
            log::warn!(
                target: crate::logging::LogCategory::Presentation.target(),
                "failed to sync {} after switching it back to live: {e}",
                screen.window_label()
            );
        }
    }

    log::info!(
        target: crate::logging::LogCategory::Presentation.target(),
        "set {} route mode to {}",
        screen.window_label(),
        mode.as_str()
    );
    Ok(())
}

// --- ambiguity resolution & context correction (Phase 1.3) ----------------

/// Resolves an `Ambiguous` detection by an explicit operator choice - "the
/// selected reference becomes an explicit operator decision," never a
/// guess CIP makes on its own. `book`/`chapter`/`verse` come from
/// whichever `AmbiguousCandidate` the operator clicked (the frontend holds
/// the candidates it was shown; this command validates independently
/// rather than trusting them blindly). `candidates_shown` is the full set
/// that was offered, purely for the audit record.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn resolve_ambiguous_reference(
    book: String,
    chapter: u32,
    verse: u32,
    raw_text: String,
    candidates_shown: Vec<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Suggestion, AppError> {
    let book = require_non_empty(&book, "book").map_err(log_and_return)?;
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let translation_id = resolve_default_translation_id(&state);

    let reference = ScriptureReference::single(&translation_id, &book, chapter, verse);
    state
        .bible_provider
        .get_verse(&reference)
        .map_err(AppError::from)
        .map_err(log_and_return)?
        .ok_or_else(|| {
            log_and_return(AppError::InvalidInput(format!(
                "not a real verse in the current translation: {reference}"
            )))
        })?;

    {
        let mut context = state
            .context_manager
            .lock()
            .expect("context_manager mutex poisoned");
        context.record_resolved(reference.clone());
    }

    let confidence = ConfidenceResult::new(
        1.0,
        ConfidenceSource::Human,
        Some("operator resolved an ambiguous reference".to_string()),
    );
    let mut suggestion = Suggestion::new(
        service_id,
        SuggestionKind::Scripture {
            reference: reference.to_string(),
        },
        confidence.clone(),
    );
    suggestion.source_text = Some(raw_text.clone());

    let db = state.db.lock().expect("db connection poisoned");
    persistence::persist_suggestion(&db, &suggestion)
        .map_err(AppError::from)
        .map_err(log_and_return)?;

    let detection = ScriptureDetection {
        kind: ReferenceKind::Verse,
        reference: Some(reference.clone()),
        context: state
            .context_manager
            .lock()
            .expect("context_manager mutex poisoned")
            .active_context(),
        candidates: Vec::new(),
        confidence,
        raw_text: raw_text.clone(),
    };
    persistence::persist_scripture_detection(&db, service_id, None, &translation_id, &detection)
        .map_err(AppError::from)
        .map_err(log_and_return)?;

    record_timeline(
        &db,
        Some(service_id),
        AppEvent::ScriptureAmbiguousResolved,
        LogCategory::Bible,
        serde_json::json!({ "selected": reference.to_string(), "candidatesShown": candidates_shown, "rawText": raw_text }),
    );
    drop(db);

    let _ = emit(&app, AppEvent::ScriptureUpdated, detection);
    let _ = emit(&app, AppEvent::SuggestionCreated, suggestion.clone());
    Ok(suggestion)
}

/// Operator correction of the active Scripture context (section 22) - "CIP
/// misunderstood the pastor." Validated exactly like an automatically
/// detected chapter reference would be (the book+chapter must be real),
/// then takes effect for subsequent bare-verse fragments the same way an
/// automatic chapter detection would. Never rewrites historical
/// transcript content - only the *context*, going forward, changes.
#[tauri::command]
pub fn correct_scripture_context(
    book: String,
    chapter: u32,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ScriptureContext, AppError> {
    let book = require_non_empty(&book, "book").map_err(log_and_return)?;
    let service_id = current_service_id(&state).map_err(log_and_return)?;

    state
        .bible_provider
        .get_chapter(&resolve_default_translation_id(&state), &book, chapter)
        .map_err(AppError::from)
        .map_err(log_and_return)?
        .ok_or_else(|| {
            log_and_return(AppError::InvalidInput(format!(
                "not a real chapter in the current translation: {book} {chapter}"
            )))
        })?;

    let (old_context, new_context) = {
        let mut context = state
            .context_manager
            .lock()
            .expect("context_manager mutex poisoned");
        let old_context = context.active_context();
        context.resolve(PartialScriptureReference {
            book: Some(book.clone()),
            chapter: Some(chapter),
            verse_start: None,
            verse_end: None,
        });
        (
            old_context,
            context
                .active_context()
                .expect("resolve() with book+chapter always establishes a context"),
        )
    };

    let db = state.db.lock().expect("db connection poisoned");
    record_timeline(
        &db,
        Some(service_id),
        AppEvent::ScriptureContextCorrected,
        LogCategory::Bible,
        serde_json::json!({
            "previous": old_context.as_ref().map(|c| format!("{} {}", c.book, c.chapter)),
            "corrected": format!("{book} {chapter}"),
        }),
    );
    drop(db);

    let detection = ScriptureDetection {
        kind: ReferenceKind::Chapter,
        reference: None,
        context: Some(new_context.clone()),
        candidates: Vec::new(),
        confidence: new_context.confidence.clone(),
        raw_text: format!("operator correction: {book} {chapter}"),
    };
    let _ = emit(&app, AppEvent::ScriptureUpdated, detection);
    Ok(new_context)
}

/// Resolves which Bible translation a caller meant when it omitted
/// `translationId` entirely - the fix for Phase 3.7's "Bible readiness"
/// root-cause audit finding (docs/phase-3-7-offline-operator-test.md
/// section 4/5).
///
/// `DEFAULT_TRANSLATION_ID` ("KJV") is a Phase 1.2 dev/test-fixture
/// identifier, registered in the content registry only when
/// `apply_dev_seed` runs (`lib.rs`: every non-`Production` environment).
/// A real Windows release build always runs in `Production`, so the dev
/// seed never applies and `bible:KJV` is never registered there - only
/// `bible:BSB` (the real production dataset) is. Twelve call sites used to
/// fall back to (or hardcode outright) the literal `DEFAULT_TRANSLATION_ID`
/// whenever the frontend omitted `translationId` - not just the Bible
/// Library/Manual Bible Search commands
/// (`preview_presentation`/`preview_scripture`/`prepare_presentation`/
/// `create_manual_presentation`/`search_bible`/`list_bible_books`), but
/// also the live-microphone and manual-transcript detection pipelines
/// (`handle_audio_chunk`, `process_test_transcript`) and the operator
/// correction commands (`edit_suggestion`, `resolve_ambiguous_reference`,
/// `correct_scripture_context`) - every one of them validates or looks up
/// a verse/chapter against `state.bible_provider` using this id. Because
/// `ensure_translation_selectable`/`is_translation_selectable` deliberately
/// "fail open" for an *unregistered* id (see that function's own docs),
/// this never produced an error - every one of those commands would
/// silently query `translation_id = 'KJV'` against a real production
/// database that only has `'BSB'` rows, returning empty results (or, for
/// the detection pipelines, silently failing to validate any reference the
/// pastor or operator actually spoke/typed). That is the exact
/// contradiction this phase's baseline reported: Diagnostics correctly
/// showing BSB installed (`get_live_status`'s `bible` field already
/// resolved `BSB_TRANSLATION_ID` directly, never this default) while the
/// Bible Library's own search/browse - and, it turned out, Bible detection
/// itself - silently found nothing.
///
/// This resolves the SAME way `get_live_status` already does - real BSB
/// production id first, the KJV dev-fixture id only as a fallback (so
/// every existing dev/test-environment test and workflow, which seeds
/// only KJV, keeps working unchanged) - making it the one place "what
/// translation did the operator mean by default" is decided, instead of
/// a stale compile-time literal.
fn resolve_default_translation_id(state: &State<'_, AppState>) -> String {
    resolve_default_translation_id_from_registry(state.content_registry.as_ref())
}

/// The pure, directly-testable core of [`resolve_default_translation_id`] -
/// split out so this fix's regression test doesn't need a full `AppState`/
/// `State<'_, AppState>` (this project has no `tauri::test` harness; see
/// `pipeline.rs`/`presentation.rs`'s docs on keeping command *logic*
/// independently testable behind a thin command wrapper).
fn resolve_default_translation_id_from_registry(
    registry: &dyn cip_core_content::ContentRegistry,
) -> String {
    let bsb_id = crate::bible_production_dataset::BSB_TRANSLATION_ID;
    let registered = registry
        .get(&content::bible_content_id(bsb_id))
        .unwrap_or(None)
        .is_some();
    if registered {
        bsb_id.to_string()
    } else {
        DEFAULT_TRANSLATION_ID.to_string()
    }
}

/// Rejects an explicitly-disabled translation for any Bible operation that
/// resolves one by id (search, preview, prepare, manual creation) - the
/// real dataset-milestone counterpart to `is_translation_selectable`,
/// which only ever filtered a *list*. Fails open the same way
/// `is_translation_selectable` does: a translation with no registry entry
/// at all, or a registry read error, is never blocked just because this
/// bookkeeping hasn't caught up to it - only an explicit `Disabled` record
/// blocks anything (section 20/21: no silent fallback, an explicit
/// "unavailable" signal instead).
fn ensure_translation_selectable(
    state: &State<'_, AppState>,
    translation_id: &str,
) -> Result<(), AppError> {
    let lookup = state
        .content_registry
        .get(&content::bible_content_id(translation_id));
    if is_translation_selectable(lookup.as_ref().map(|opt| opt.as_ref())) {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!(
            "translation {translation_id:?} is disabled"
        )))
    }
}

// --- manual Bible search (works with no audio/speech/network) -------------

#[tauri::command]
pub fn search_bible(
    query: String,
    translation_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<BibleSearchResult>, AppError> {
    let query = require_non_empty(&query, "query").map_err(log_and_return)?;
    let translation_id = translation_id.unwrap_or_else(|| resolve_default_translation_id(&state));
    ensure_translation_selectable(&state, &translation_id).map_err(log_and_return)?;
    dispatch_bible_search(state.bible_provider.as_ref(), &translation_id, &query)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// The Bible Library book browser's book list (Phase 3.6): the canonical
/// 66-book order/testament from `book_alias::BOOKS` (the one place book
/// identity is known - see that module's docs), each looked up against
/// the real provider so `chapter_count` reflects what this translation
/// actually has imported rather than assumed canon. A book the dataset
/// doesn't have (e.g. a partial dev fixture) is simply omitted, never
/// invented with a guessed chapter count - see
/// docs/phase-3-6-church-libraries.md's "no fabricated Bible data" rule.
/// No new database table or provider method: this only composes the
/// existing `BibleProvider::get_book`, called once per canonical book.
#[tauri::command]
pub fn list_bible_books(
    translation_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<BibleBook>, AppError> {
    let translation_id = translation_id.unwrap_or_else(|| resolve_default_translation_id(&state));
    ensure_translation_selectable(&state, &translation_id).map_err(log_and_return)?;
    let mut books = Vec::new();
    for canonical in BOOKS {
        if let Some(book) = state
            .bible_provider
            .get_book(&translation_id, canonical.code)
            .map_err(AppError::from)
            .map_err(log_and_return)?
        {
            books.push(book);
        }
    }
    Ok(books)
}

// --- saved scriptures (Phase 3.6: Church Knowledge Libraries) --------------

/// Saves a Scripture reference (single verse or a verse range) for later
/// reuse from the Bible Library - see
/// `persistence::persist_saved_scripture`'s docs for why this is a
/// standalone, church-wide, cross-service table. Takes structured fields
/// rather than re-parsing a reference string a third time (a copy already
/// exists in both `commands.rs` and `presentation.rs` for their own
/// narrower purposes) - the frontend already has these fields from
/// whichever `BibleSearchResult`(s) the operator is saving.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn save_scripture(
    translation_id: String,
    book: String,
    chapter: u32,
    verse_start: u32,
    verse_end: Option<u32>,
    reference_display: String,
    note: Option<String>,
    state: State<'_, AppState>,
) -> Result<persistence::SavedScripture, AppError> {
    let translation_id =
        require_non_empty(&translation_id, "translationId").map_err(log_and_return)?;
    let book = require_non_empty(&book, "book").map_err(log_and_return)?;
    let reference_display =
        require_non_empty(&reference_display, "referenceDisplay").map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::persist_saved_scripture(
        &db,
        Uuid::new_v4(),
        &translation_id,
        &book,
        chapter,
        verse_start,
        verse_end,
        &reference_display,
        note.as_deref(),
    )
    .map_err(AppError::from)
    .map_err(log_and_return)
}

/// Every saved scripture, most recently saved first - the Bible Library's
/// "Saved" list.
#[tauri::command]
pub fn list_saved_scriptures(
    state: State<'_, AppState>,
) -> Result<Vec<persistence::SavedScripture>, AppError> {
    let db = state.db.lock().expect("db connection poisoned");
    persistence::list_saved_scriptures(&db)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

#[tauri::command]
pub fn delete_saved_scripture(id: String, state: State<'_, AppState>) -> Result<bool, AppError> {
    let uuid = parse_uuid(&id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::delete_saved_scripture(&db, uuid)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

// --- content registry (Phase 1.5) -------------------------------------------

fn parse_content_type(value: &str) -> Result<ContentType, AppError> {
    match value {
        "bible" => Ok(ContentType::Bible),
        "music" => Ok(ContentType::Music),
        "service" => Ok(ContentType::Service),
        "media" => Ok(ContentType::Media),
        "reference" => Ok(ContentType::Reference),
        other => Err(AppError::InvalidInput(format!(
            "unknown content type: {other}"
        ))),
    }
}

/// What local content exists - the Content Registry diagnostics panel's
/// data source. `contentType` (`"bible"`/`"music"`/...) optionally
/// narrows the list; omitted, every registered content item is returned.
#[tauri::command]
pub fn list_content_registry(
    content_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ContentMetadata>, AppError> {
    let content_type = content_type
        .map(|s| parse_content_type(&s))
        .transpose()
        .map_err(log_and_return)?;
    state
        .content_registry
        .list(content_type)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

#[tauri::command]
pub fn get_content_metadata(
    content_id: String,
    state: State<'_, AppState>,
) -> Result<ContentMetadata, AppError> {
    let content_id = require_non_empty(&content_id, "contentId").map_err(log_and_return)?;
    state
        .content_registry
        .get(&content_id)
        .map_err(AppError::from)
        .map_err(log_and_return)?
        .ok_or_else(|| {
            log_and_return(AppError::InvalidInput(format!(
                "content not found: {content_id}"
            )))
        })
}

/// Enables/disables a content item without deleting it (section 10) -
/// disabled content stops appearing in normal selection/search
/// (`list_bible_translations`) but its historical use (a service that
/// already presented from it) remains fully understandable.
#[tauri::command]
pub fn set_content_enabled(
    content_id: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<ContentMetadata, AppError> {
    ensure_admin(&state).map_err(log_and_return)?;
    let content_id = require_non_empty(&content_id, "contentId").map_err(log_and_return)?;
    state
        .content_registry
        .set_enabled(&content_id, enabled)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    state
        .content_registry
        .get(&content_id)
        .map_err(AppError::from)
        .map_err(log_and_return)?
        .ok_or_else(|| {
            log_and_return(AppError::InvalidInput(format!(
                "content not found: {content_id}"
            )))
        })
}

/// Imports a local Bible dataset, already read and parsed to JSON text by
/// the frontend (never a filesystem path - see `docs/bible-datasets.md`'s
/// security note: this command never touches the filesystem itself).
#[tauri::command]
pub fn import_bible_dataset(
    dataset_json: String,
    state: State<'_, AppState>,
) -> Result<ImportReport, AppError> {
    ensure_admin(&state).map_err(log_and_return)?;
    let dataset_json = require_non_empty(&dataset_json, "datasetJson").map_err(log_and_return)?;
    let dataset: BibleDatasetInput = serde_json::from_str(&dataset_json).map_err(|e| {
        log_and_return(AppError::InvalidInput(format!(
            "malformed dataset JSON: {e}"
        )))
    })?;
    let db = state.db.lock().expect("db connection poisoned");
    content::import_and_register(&db, state.content_registry.as_ref(), &dataset)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// Structural integrity check (section 11) against whatever is actually
/// stored for `translationId` - never a hard-coded canonical Bible fact
/// table. See `cip_core_bible::check_bible_integrity`'s docs.
#[tauri::command]
pub fn check_bible_dataset_integrity(
    translation_id: String,
    state: State<'_, AppState>,
) -> Result<IntegrityReport, AppError> {
    let translation_id =
        require_non_empty(&translation_id, "translationId").map_err(log_and_return)?;
    check_bible_integrity(state.bible_provider.as_ref(), &translation_id)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

// --- intelligence (Phase 2.0) -------------------------------------------------

/// One [`IntelligenceDomain`]'s real capability, for the diagnostic
/// "Intelligence Status" panel - see `intelligence.rs`'s module docs.
/// `engineId`/`engineVersion` are `None` for a domain with no registered
/// engine at all (Music/Sermon/Content/CrossDomain in Phase 2.0), never a
/// placeholder value.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainCapabilityReport {
    pub domain: IntelligenceDomain,
    pub capability: EngineCapability,
    pub engine_id: Option<String>,
    pub engine_version: Option<String>,
}

/// Minimal diagnostic command (Phase 2.0 spec section 41): reports each
/// reserved [`IntelligenceDomain`]'s real capability from the registry
/// built in `intelligence::build_registry`. Never calls `analyze()` on
/// anything - this only reads identity/capability.
#[tauri::command]
pub fn get_intelligence_capabilities(state: State<'_, AppState>) -> Vec<DomainCapabilityReport> {
    crate::intelligence::ALL_DOMAINS
        .iter()
        .map(
            |domain| match state.intelligence_registry.resolve(*domain) {
                Some(engine) => {
                    let identity = engine.identity();
                    DomainCapabilityReport {
                        domain: *domain,
                        capability: engine.capability(),
                        engine_id: Some(identity.engine_id),
                        engine_version: Some(identity.engine_version),
                    }
                }
                None => DomainCapabilityReport {
                    domain: *domain,
                    capability: EngineCapability::Unavailable,
                    engine_id: None,
                    engine_version: None,
                },
            },
        )
        .collect()
}

/// The deterministic Bible-analysis harness, exposed over IPC - the Phase
/// 2.4 bridge that makes a Bible-domain `IntelligenceFinding` reachable in
/// `AppState.intelligence_findings` at all. Mirrors `analyze_music_transcript`/
/// `analyze_sermon_transcript` exactly: persists `text` as an ordinary
/// transcript segment, builds a real `IntelligenceContext`, and calls the
/// already-registered (since Phase 2.0) `BibleIntelligenceEngine` via
/// `intelligence_registry.resolve(IntelligenceDomain::Bible)` - the same
/// engine `get_intelligence_capabilities` has always reported `Available`,
/// just never previously invoked from a live command. This is new,
/// additive wiring only: `core/bible` and `core/service`'s existing
/// `ScriptureDetection`/`Suggestion` pipeline (`handle_final_transcript`)
/// is completely unchanged and remains the operator-facing Bible workflow;
/// this command exists solely so a Bible finding can appear in
/// `context.recent_findings` for `analyze_cross_domain` to correlate
/// against (Phase 2.4 spec section 9's "if a compatibility fix is needed,
/// document it before changing it" - no fix to Bible Intelligence itself
/// was needed, only this new bridge). Never prepares or projects a
/// presentation item.
#[tauri::command]
pub fn analyze_bible_transcript(
    text: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<IntelligenceFinding>, AppError> {
    let text = require_non_empty(&text, "text").map_err(log_and_return)?;
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let sequence = state.transcript_sequence.fetch_add(1, Ordering::SeqCst);

    let segment = TranscriptSegment {
        id: Uuid::new_v4(),
        sequence,
        text,
        is_final: true,
        confidence: ConfidenceResult::new(
            1.0,
            ConfidenceSource::Human,
            Some("manually entered test transcript for bible analysis".to_string()),
        ),
        start_ms: 0,
        end_ms: 0,
        language: Some("en".to_string()),
        speaker_id: None,
    };

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::persist_transcript_segment(&db, service_id, &segment)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
    }
    let _ = emit(&app, AppEvent::TranscriptUpdated, segment.clone());

    let context = {
        let db = state.db.lock().expect("db connection poisoned");
        let recent_timeline = timeline::list_timeline(&db, service_id, 20)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        let active_service = state
            .active_service
            .lock()
            .expect("active_service mutex poisoned");
        let context_manager = state
            .context_manager
            .lock()
            .expect("context_manager mutex poisoned");
        crate::intelligence::build_intelligence_context(
            &db,
            state.content_registry.as_ref(),
            active_service.as_ref(),
            &*context_manager,
            &recent_timeline,
            Vec::new(),
            ContextBounds::default(),
        )
        .map_err(AppError::from)
        .map_err(log_and_return)?
    };

    let input = IntelligenceInput::new(service_id, segment);
    let engine = state
        .intelligence_registry
        .resolve(IntelligenceDomain::Bible)
        .ok_or_else(|| {
            log_and_return(AppError::InvalidInput(
                "bible intelligence engine is not registered".to_string(),
            ))
        })?;

    let result = engine
        .analyze(&input, &context)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    let queued = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        let mut queued = Vec::new();
        for finding in result.findings {
            if findings.add(finding.clone()) == QueueAddOutcome::Added {
                queued.push(finding);
            }
        }
        queued
    };

    Ok(queued)
}

// --- music intelligence (Phase 2.1) -----------------------------------------
//
// Dataset listing deliberately reuses `list_content_registry(Some("music"))`
// above rather than a second "list music datasets" command - Music
// datasets are ordinary Content Registry entries (`ContentType::Music`),
// so a dedicated listing command would only duplicate an existing one.

fn parse_music_query(query_type: &str, query_text: String) -> Result<MusicQuery, AppError> {
    match query_type {
        "title" => Ok(MusicQuery::Title(query_text)),
        "number" => Ok(MusicQuery::Number(query_text)),
        "lyric" => Ok(MusicQuery::Lyric(query_text)),
        other => Err(AppError::InvalidInput(format!(
            "unknown music query type: {other}"
        ))),
    }
}

/// Every currently-enabled Music dataset - `search_music`'s default scope
/// when the operator does not explicitly name datasets, mirroring
/// `is_translation_selectable`'s "disabled content is hidden from normal
/// selection" rule (applied to Music instead of Bible).
fn enabled_music_content_ids(state: &State<'_, AppState>) -> Result<Vec<String>, AppError> {
    Ok(state
        .content_registry
        .list(Some(ContentType::Music))
        .map_err(AppError::from)?
        .into_iter()
        .filter(|m| m.status == ContentStatus::Enabled)
        .map(|m| m.id)
        .collect())
}

/// Manual song search - works with no audio/speech/network, same reasoning
/// as `search_bible`. `content_ids` lets the operator explicitly name
/// which dataset(s) to search (including a disabled one, exactly like
/// `search_bible` accepts any `translation_id` regardless of that
/// translation's own enabled/disabled status); omitted, only
/// currently-enabled Music datasets are searched.
#[tauri::command]
pub fn search_music(
    query: String,
    query_type: String,
    content_ids: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> Result<Vec<SongRecognitionCandidate>, AppError> {
    let query_text = require_non_empty(&query, "query").map_err(log_and_return)?;
    let music_query = parse_music_query(&query_type, query_text).map_err(log_and_return)?;
    let content_ids = match content_ids {
        Some(ids) => ids,
        None => enabled_music_content_ids(&state).map_err(log_and_return)?,
    };
    search_songs(
        state.music_provider.as_ref(),
        &content_ids,
        &music_query,
        &MatchThresholds::default(),
    )
    .map_err(AppError::from)
    .map_err(log_and_return)
}

/// Imports a local music dataset, already read and parsed to JSON text by
/// the frontend (never a filesystem path) - mirrors `import_bible_dataset`
/// exactly; see `docs/music-datasets.md`.
#[tauri::command]
pub fn import_music_dataset(
    dataset_json: String,
    state: State<'_, AppState>,
) -> Result<MusicImportReport, AppError> {
    let dataset_json = require_non_empty(&dataset_json, "datasetJson").map_err(log_and_return)?;
    let dataset: MusicDatasetInput = serde_json::from_str(&dataset_json).map_err(|e| {
        log_and_return(AppError::InvalidInput(format!(
            "malformed dataset JSON: {e}"
        )))
    })?;
    let db = state.db.lock().expect("db connection poisoned");
    music::import_and_register_music(&db, state.content_registry.as_ref(), &dataset)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// The deterministic music-analysis harness, exposed over IPC - the Music
/// Intelligence counterpart to `process_test_transcript`. Persists `text`
/// as an ordinary transcript segment (so a later call's multi-line lyric
/// continuity has real history to look at - see `music_adapter`'s module
/// docs), builds a real `IntelligenceContext` from this app's actual
/// state, and calls the registered Music engine directly - never routed
/// through `handle_final_transcript`/the Bible pipeline, since Music must
/// be reachable independently of Bible's path (Phase 2.1 spec section 2).
/// Findings are queued in `AppState.intelligence_findings`; this command
/// never prepares or projects a presentation item.
#[tauri::command]
pub fn analyze_music_transcript(
    text: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<IntelligenceFinding>, AppError> {
    let text = require_non_empty(&text, "text").map_err(log_and_return)?;
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let sequence = state.transcript_sequence.fetch_add(1, Ordering::SeqCst);

    let segment = TranscriptSegment {
        id: Uuid::new_v4(),
        sequence,
        text,
        is_final: true,
        confidence: ConfidenceResult::new(
            1.0,
            ConfidenceSource::Human,
            Some("manually entered test transcript for music analysis".to_string()),
        ),
        start_ms: 0,
        end_ms: 0,
        language: Some("en".to_string()),
        speaker_id: None,
    };

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::persist_transcript_segment(&db, service_id, &segment)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
    }
    let _ = emit(&app, AppEvent::TranscriptUpdated, segment.clone());

    let context = {
        let db = state.db.lock().expect("db connection poisoned");
        let recent_timeline = timeline::list_timeline(&db, service_id, 20)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        let active_service = state
            .active_service
            .lock()
            .expect("active_service mutex poisoned");
        let context_manager = state
            .context_manager
            .lock()
            .expect("context_manager mutex poisoned");
        crate::intelligence::build_intelligence_context(
            &db,
            state.content_registry.as_ref(),
            active_service.as_ref(),
            &*context_manager,
            &recent_timeline,
            Vec::new(),
            ContextBounds::default(),
        )
        .map_err(AppError::from)
        .map_err(log_and_return)?
    };

    let input = IntelligenceInput::new(service_id, segment);
    let engine = state
        .intelligence_registry
        .resolve(IntelligenceDomain::Music)
        .ok_or_else(|| {
            log_and_return(AppError::InvalidInput(
                "music intelligence engine is not registered".to_string(),
            ))
        })?;

    let queued = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        music::analyze_and_queue(engine, &input, &context, &mut findings)
            .map_err(AppError::from)
            .map_err(log_and_return)?
    };

    {
        let db = state.db.lock().expect("db connection poisoned");
        for finding in &queued {
            record_timeline(
                &db,
                Some(service_id),
                AppEvent::MusicFindingDetected,
                LogCategory::Music,
                serde_json::json!({
                    "findingId": finding.id,
                    "summary": &finding.summary,
                    "confidence": finding.confidence.score,
                }),
            );
        }
    }
    for finding in &queued {
        let _ = emit(&app, AppEvent::MusicFindingDetected, finding.clone());
    }

    Ok(queued)
}

/// Music findings still awaiting an operator decision (`Detected`/`Reviewed`),
/// for the active service - the Music Intelligence panel's data source.
/// `FindingQueue` is not itself service-scoped (Phase 2.0's in-memory
/// design), so this filters to the active service and the Music domain
/// explicitly.
#[tauri::command]
pub fn list_music_findings(
    state: State<'_, AppState>,
) -> Result<Vec<IntelligenceFinding>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let findings = state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned");
    Ok(findings
        .pending()
        .into_iter()
        .filter(|f| f.service_id == service_id && f.domain == IntelligenceDomain::Music)
        .cloned()
        .collect())
}

/// Explicit operator acceptance of a music finding (Phase 2.1 hard
/// requirement: music recognition must never automatically create a
/// presentation item). This changes only the finding's own status in
/// `AppState.intelligence_findings` - there is no call from here into
/// `presentation::persist_prepared_item` or anything else that could
/// project a slide. An operator who wants a prepared item still uses the
/// existing, separate manual presentation commands.
#[tauri::command]
pub fn accept_music_finding(
    finding_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<IntelligenceFinding, AppError> {
    let id = parse_uuid(&finding_id).map_err(log_and_return)?;
    let updated = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        findings
            .accept(id)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        findings
            .get(id)
            .cloned()
            .expect("just-accepted finding is still present")
    };
    let db = state.db.lock().expect("db connection poisoned");
    record_timeline(
        &db,
        Some(updated.service_id),
        AppEvent::MusicFindingAccepted,
        LogCategory::Music,
        serde_json::json!({ "findingId": updated.id, "summary": &updated.summary }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::MusicFindingAccepted, updated.clone());

    // Phase 2.2: accepting a Music finding is the *only* way `current_song`
    // is ever set - never automatically from acoustic/lyric confidence
    // alone (see `cip_core_music::CurrentSong`'s docs). A finding missing
    // the `song_id:`/`content_id` evidence a real Music finding always
    // carries leaves `current_song` untouched rather than clearing it.
    if let Some(current) = music::current_song_from_finding(&updated) {
        *state
            .current_song
            .lock()
            .expect("current_song mutex poisoned") = Some(current.clone());
        let db = state.db.lock().expect("db connection poisoned");
        record_timeline(
            &db,
            Some(updated.service_id),
            AppEvent::CurrentSongChanged,
            LogCategory::Music,
            serde_json::json!({
                "contentId": &current.content_id,
                "songId": &current.song_id,
                "reason": "accepted",
            }),
        );
        drop(db);
        let _ = emit(&app, AppEvent::CurrentSongChanged, current);
    }

    Ok(updated)
}

#[tauri::command]
pub fn reject_music_finding(
    finding_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<IntelligenceFinding, AppError> {
    let id = parse_uuid(&finding_id).map_err(log_and_return)?;
    let updated = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        findings
            .reject(id)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        findings
            .get(id)
            .cloned()
            .expect("just-rejected finding is still present")
    };
    let db = state.db.lock().expect("db connection poisoned");
    record_timeline(
        &db,
        Some(updated.service_id),
        AppEvent::MusicFindingRejected,
        LogCategory::Music,
        serde_json::json!({ "findingId": updated.id, "summary": &updated.summary }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::MusicFindingRejected, updated.clone());
    Ok(updated)
}

/// Explicit operator clear of `current_song` (Phase 2.2) - the only other
/// way `current_song` ever changes besides `accept_music_finding` setting
/// it. Never inferred from silence/a song ending/a new detection; an
/// operator must actively call this.
#[tauri::command]
pub fn clear_current_song(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    let had_song = {
        let mut current = state
            .current_song
            .lock()
            .expect("current_song mutex poisoned");
        let had_song = current.is_some();
        *current = None;
        had_song
    };
    if had_song {
        let service_id = state
            .active_service
            .lock()
            .expect("active_service mutex poisoned")
            .as_ref()
            .map(|s| s.id);
        let db = state.db.lock().expect("db connection poisoned");
        record_timeline(
            &db,
            service_id,
            AppEvent::CurrentSongChanged,
            LogCategory::Music,
            serde_json::json!({ "reason": "cleared" }),
        );
        drop(db);
        let _ = emit(&app, AppEvent::CurrentSongChanged, ());
    }
    Ok(())
}

/// Build a real `IntelligenceContext` from this app's actual state,
/// including real `recent_findings` (unlike `analyze_music_transcript`'s
/// own context, which - unchanged since Phase 2.1 - passes `Vec::new()`).
/// Shared by `analyze_music_audio` and the acoustic worker
/// (`spawn_acoustic_worker`) so both see the same song-continuity history
/// `intelligence::build_intelligence_context`'s `recent_findings`
/// parameter now supports.
fn build_music_context(
    state: &AppState,
    service_id: Uuid,
) -> Result<IntelligenceContext, AppError> {
    let db = state.db.lock().expect("db connection poisoned");
    let recent_timeline = timeline::list_timeline(&db, service_id, 20)?;
    let active_service = state
        .active_service
        .lock()
        .expect("active_service mutex poisoned");
    let context_manager = state
        .context_manager
        .lock()
        .expect("context_manager mutex poisoned");
    let recent_findings = state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned")
        .all()
        .into_iter()
        .cloned()
        .collect();
    let context = crate::intelligence::build_intelligence_context(
        &db,
        state.content_registry.as_ref(),
        active_service.as_ref(),
        &*context_manager,
        &recent_timeline,
        recent_findings,
        ContextBounds::default(),
    )
    .map_err(AppError::from)?;

    // Phase 2.5 (Sermon Foundation, per the authoritative Phase 2
    // roadmap): additively attach the active sermon/section/segments so
    // every engine's context (Bible/Music/Sermon/Service alike) can
    // *observe* sermon structural state - never a reason for one engine
    // to call another (invariant 4).
    let active_sermon = state
        .active_sermon
        .lock()
        .expect("active_sermon mutex poisoned")
        .clone();
    let current_sermon_section = state
        .active_sermon_section
        .lock()
        .expect("active_sermon_section mutex poisoned")
        .clone();
    let recent_sermon_segments = match &active_sermon {
        Some(sermon) => persistence::list_sermon_segments(&db, sermon.id)?,
        None => Vec::new(),
    };
    let context = context.with_sermon_context(
        active_sermon,
        current_sermon_section,
        recent_sermon_segments,
    );

    // Phase 2.8 (per the authoritative Phase 2 roadmap): additively attach
    // whatever Content Intelligence candidates are already queued, so
    // Cross-Domain Intelligence can read them (see
    // `cip_core_intelligence::rule_sermon_content`) - never a reason for
    // either layer to call the other directly (invariant 4). Mirrors the
    // sermon-context attachment above exactly.
    let recent_content_candidates = state
        .content_candidate_queue
        .lock()
        .expect("content_candidate_queue mutex poisoned")
        .all()
        .into_iter()
        .cloned()
        .collect();
    Ok(context.with_content_candidates(recent_content_candidates))
}

/// The deterministic acoustic-analysis harness, exposed over IPC - the
/// Phase 2.2 counterpart to `analyze_music_transcript`, and the primary
/// way this app's acoustic pipeline is tested/demonstrated without a
/// microphone (Phase 2.2's "manual test mode" requirement). Wraps `samples`
/// directly into one `AudioSegment` (bypassing `AcousticWorkerState`'s
/// windowing/rate-limiting - a manual, single-shot call is not subject to
/// the same "how often can the recognizer be called" concern live audio
/// is) and runs it through the same `acoustic::recognize_fuse_and_queue`
/// path the live worker uses, so a `ScriptedAcousticMusicRecognizer` can
/// exercise the exact real pipeline end to end. Still gated by the
/// signal-quality check: a caller who feeds silence/too-short audio gets
/// an honest empty result, not a fake one.
#[tauri::command]
pub fn analyze_music_audio(
    samples: Vec<i16>,
    sample_rate_hz: u32,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<IntelligenceFinding>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let segment = cip_core_music::AudioSegment::new(samples, sample_rate_hz, 0);
    let quality = cip_core_music::assess_signal_quality(
        &segment,
        &cip_core_music::AcousticAnalysisConfig::default(),
    );
    if quality != cip_core_music::SignalQuality::Ready {
        return Ok(Vec::new());
    }

    let context = build_music_context(&state, service_id).map_err(log_and_return)?;
    let content_ids = acoustic::enabled_music_dataset_ids(&context);
    if content_ids.is_empty() {
        return Ok(Vec::new());
    }

    let queued = {
        let mut recognizer = state
            .acoustic_recognizer
            .lock()
            .expect("acoustic_recognizer mutex poisoned");
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        acoustic::recognize_fuse_and_queue(
            recognizer.as_mut(),
            &state.acoustic_music_engine,
            &segment,
            &content_ids,
            service_id,
            &context,
            &mut findings,
        )
        .map_err(|e| AppError::InvalidInput(e.to_string()))
        .map_err(log_and_return)?
    };

    let db = state.db.lock().expect("db connection poisoned");
    for finding in &queued {
        record_timeline(
            &db,
            Some(service_id),
            AppEvent::MusicFindingDetected,
            LogCategory::Music,
            serde_json::json!({
                "findingId": finding.id,
                "summary": &finding.summary,
                "confidence": finding.confidence.score,
                "source": "acoustic",
            }),
        );
    }
    drop(db);
    for finding in &queued {
        let _ = emit(&app, AppEvent::MusicFindingDetected, finding.clone());
    }

    Ok(queued)
}

// --- sermon intelligence (Phase 2.3) ----------------------------------------
//
// Deliberately manual-command-only, mirroring Music's Phase 2.1 lyric
// path (`analyze_music_transcript`) - nothing here is wired into
// `pipeline.rs::handle_final_transcript`, which stays exactly as Phase 1
// left it. `SermonIntelligenceEngine` needs no provider/dataset, so there
// is no dataset-import counterpart here.

/// The deterministic sermon-analysis harness, exposed over IPC - the
/// Sermon Intelligence counterpart to `analyze_music_transcript`. Persists
/// `text` as an ordinary transcript segment, builds a real
/// `IntelligenceContext` from this app's actual state (so scripture
/// cross-linking can see `active_scripture_context`), and calls
/// `AppState.sermon_engine` directly - the same accumulating-state
/// instance every call goes through, never the separate diagnostic-only
/// copy in `intelligence_registry` (see `sermon.rs`'s module docs).
/// Findings are queued in `AppState.intelligence_findings`; this command
/// never prepares or projects a presentation item, and never records a
/// timeline entry for a mere detection (spec section 41) - only
/// `accept_sermon_finding`/`reject_sermon_finding` do that.
#[tauri::command]
pub fn analyze_sermon_transcript(
    text: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<IntelligenceFinding>, AppError> {
    let text = require_non_empty(&text, "text").map_err(log_and_return)?;
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let sequence = state.transcript_sequence.fetch_add(1, Ordering::SeqCst);

    let segment = TranscriptSegment {
        id: Uuid::new_v4(),
        sequence,
        text,
        is_final: true,
        confidence: ConfidenceResult::new(
            1.0,
            ConfidenceSource::Human,
            Some("manually entered test transcript for sermon analysis".to_string()),
        ),
        start_ms: 0,
        end_ms: 0,
        language: Some("en".to_string()),
        speaker_id: None,
    };

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::persist_transcript_segment(&db, service_id, &segment)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
    }
    let _ = emit(&app, AppEvent::TranscriptUpdated, segment.clone());

    // Phase 2.6 (per the authoritative Phase 2 roadmap): reuse the shared,
    // generic context builder rather than a second hand-rolled one, so the
    // Sermon engine now observes the Phase 2.5 Sermon Foundation's
    // active_sermon/current_sermon_section/recent_sermon_segments exactly
    // like every other domain already does (see `build_music_context`'s
    // own docs on why its name is generic despite the domain it was first
    // written for).
    let context = build_music_context(&state, service_id).map_err(log_and_return)?;

    let before = state.sermon_engine.snapshot();
    let input = IntelligenceInput::new(service_id, segment);
    let queued = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        crate::sermon::analyze_and_queue(&state.sermon_engine, &input, &context, &mut findings)
            .map_err(AppError::from)
            .map_err(log_and_return)?
    };
    let after = state.sermon_engine.snapshot();

    for finding in &queued {
        let _ = emit(&app, AppEvent::SermonFindingDetected, finding.clone());
    }
    if after.state != before.state {
        let _ = emit(&app, AppEvent::SermonStateChanged, after.state);
    }
    if after.theme != before.theme {
        let _ = emit(&app, AppEvent::SermonThemeChanged, after.theme.clone());
    }
    if after.points.len() != before.points.len()
        || after.points.last().map(|p| p.sub_points.len())
            != before.points.last().map(|p| p.sub_points.len())
    {
        let _ = emit(&app, AppEvent::SermonStructureUpdated, after.points.clone());
    }

    Ok(queued)
}

/// Sermon findings still awaiting an operator decision (`Detected`/`Reviewed`),
/// for the active service - mirrors `list_music_findings` exactly.
#[tauri::command]
pub fn list_sermon_findings(
    state: State<'_, AppState>,
) -> Result<Vec<IntelligenceFinding>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let findings = state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned");
    Ok(findings
        .pending()
        .into_iter()
        .filter(|f| f.service_id == service_id && f.domain == IntelligenceDomain::Sermon)
        .cloned()
        .collect())
}

/// Explicit operator acceptance of a sermon finding - changes only the
/// finding's own status, exactly like `accept_music_finding`. There is no
/// code path from here into `presentation::persist_prepared_item` or
/// anything else that could project a slide (spec section 24/54).
#[tauri::command]
pub fn accept_sermon_finding(
    finding_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<IntelligenceFinding, AppError> {
    let id = parse_uuid(&finding_id).map_err(log_and_return)?;
    let updated = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        findings
            .accept(id)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        findings
            .get(id)
            .cloned()
            .expect("just-accepted finding is still present")
    };
    let db = state.db.lock().expect("db connection poisoned");
    // Phase 13 (Church Knowledge Base): explicit acceptance is the one
    // moment a Sermon-domain finding becomes durable history - mirrors
    // accept_content_candidate -> persist_saved_content_candidate exactly.
    // Detected/Reviewed/Rejected findings are never persisted here.
    let element_label = crate::sermon_knowledge_base::element_label_for_summary(&updated.summary);
    persistence::persist_saved_sermon_finding(&db, &updated, element_label)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    record_timeline(
        &db,
        Some(updated.service_id),
        AppEvent::SermonFindingAccepted,
        LogCategory::App,
        serde_json::json!({ "findingId": updated.id, "summary": &updated.summary }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::SermonFindingAccepted, updated.clone());
    Ok(updated)
}

/// Explicit operator rejection of a sermon finding - the same auditable,
/// explicit correction path spec section 40 asks for: rejecting a
/// mis-detected theme/point is recorded (`SERMON_FINDING_REJECTED`) and
/// never rewrites the transcript that led to it.
#[tauri::command]
pub fn reject_sermon_finding(
    finding_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<IntelligenceFinding, AppError> {
    let id = parse_uuid(&finding_id).map_err(log_and_return)?;
    let updated = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        findings
            .reject(id)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        findings
            .get(id)
            .cloned()
            .expect("just-rejected finding is still present")
    };
    let db = state.db.lock().expect("db connection poisoned");
    record_timeline(
        &db,
        Some(updated.service_id),
        AppEvent::SermonFindingRejected,
        LogCategory::App,
        serde_json::json!({ "findingId": updated.id, "summary": &updated.summary }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::SermonFindingRejected, updated.clone());
    Ok(updated)
}

/// The current theme/state/structure snapshot - read-only, never mutates
/// anything. The manual sermon test-mode UI and the Live Church Brain's
/// "SERMON INTELLIGENCE" panel both poll this rather than re-deriving it
/// from the raw finding queue.
#[tauri::command]
pub fn get_sermon_state(state: State<'_, AppState>) -> cip_core_intelligence::SermonStateSnapshot {
    state.sermon_engine.snapshot()
}

// --- sermon foundation (Phase 2.5, per the authoritative Phase 2 roadmap) --
//
// The durable entity/lifecycle layer beneath the historical `sermon.rs`/
// `SermonIntelligenceEngine` semantic-detection commands above - see
// `sermon_foundation.rs`'s module docs for the roadmap/architecture
// distinction. Every command here is an explicit operator action; nothing
// here is called from the live transcript pipeline, and none of it calls
// `SermonIntelligenceEngine` or any other engine directly.

/// "A sermon must have a distinct identity, and only one may be active at
/// once" - mirrors `ensure_no_active_service` exactly.
fn ensure_no_active_sermon(active: Option<&Sermon>) -> Result<(), AppError> {
    if active.is_some() {
        return Err(AppError::InvalidInput(
            "a sermon is already active - end it before starting a new one".to_string(),
        ));
    }
    Ok(())
}

/// Every mutating lifecycle command's shared guard - delegates to
/// `cip_core_sermon::foundation::is_valid_transition`, the single source
/// of truth for the state machine (spec's "SERMON STATE MACHINE" section),
/// and turns a rejected transition into a clear operator-facing message
/// rather than silently mutating state.
fn ensure_valid_sermon_transition(from: SermonStatus, to: SermonStatus) -> Result<(), AppError> {
    if !is_valid_transition(from, to) {
        return Err(AppError::InvalidInput(format!(
            "cannot move a sermon from {from:?} to {to:?}"
        )));
    }
    Ok(())
}

fn parse_speaker_role_input(value: &str) -> Result<SpeakerRole, AppError> {
    match value.trim().to_lowercase().as_str() {
        "primary" => Ok(SpeakerRole::Primary),
        "guest" => Ok(SpeakerRole::Guest),
        other => Err(AppError::InvalidInput(format!(
            "unknown speaker role: {other}"
        ))),
    }
}

/// Matches [`cip_core_sermon::foundation::SermonSectionKind::label`]
/// case-insensitively - the one place a plain string from the frontend
/// becomes a real `SermonSectionKind`, mirroring `service::parse_service_phase`.
fn parse_section_kind_input(value: &str) -> Result<SermonSectionKind, AppError> {
    let normalized = value.trim().to_uppercase().replace([' ', '-'], "_");
    [
        SermonSectionKind::Introduction,
        SermonSectionKind::ScriptureReading,
        SermonSectionKind::MainMessage,
        SermonSectionKind::Illustration,
        SermonSectionKind::Prayer,
        SermonSectionKind::AltarCall,
        SermonSectionKind::Conclusion,
    ]
    .into_iter()
    .find(|k| k.label() == normalized)
    .ok_or_else(|| AppError::InvalidInput(format!("unknown sermon section: {value}")))
}

fn active_sermon_or_error(state: &State<'_, AppState>) -> Result<Sermon, AppError> {
    state
        .active_sermon
        .lock()
        .expect("active_sermon mutex poisoned")
        .clone()
        .ok_or(AppError::NoActiveSermon)
}

/// The read-only summary `get_sermon_foundation_state` returns - what
/// structural state exists right now, independent of the `FindingQueue`'s
/// own operator-review status for any finding these commands produced.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonFoundationSummary {
    pub active_sermon: Option<Sermon>,
    pub current_section: Option<SermonSection>,
}

#[tauri::command]
pub fn get_sermon_foundation_state(state: State<'_, AppState>) -> SermonFoundationSummary {
    SermonFoundationSummary {
        active_sermon: state
            .active_sermon
            .lock()
            .expect("active_sermon mutex poisoned")
            .clone(),
        current_section: state
            .active_sermon_section
            .lock()
            .expect("active_sermon_section mutex poisoned")
            .clone(),
    }
}

/// Starts a new sermon within the active service - "start" begins
/// delivering immediately (no separate "planned" step in this phase's
/// operator workflow, mirroring `start_service`). Automatically opens an
/// `Introduction` section with `SectionOrigin::SystemBoundary` - a
/// deterministic structural fact ("a sermon has a beginning"), never a
/// judgment call about transcript content.
#[tauri::command]
pub fn start_sermon(
    title: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Sermon, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    ensure_no_active_sermon(
        state
            .active_sermon
            .lock()
            .expect("active_sermon mutex poisoned")
            .as_ref(),
    )
    .map_err(log_and_return)?;
    let title = title
        .map(|t| require_non_empty(&t, "title"))
        .transpose()
        .map_err(log_and_return)?;
    let sermon = Sermon::start(service_id, title);
    let section = SermonSection::open(
        sermon.id,
        SermonSectionKind::Introduction,
        SectionOrigin::SystemBoundary,
        None,
    );

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::persist_sermon(&db, &sermon)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        persistence::persist_sermon_section(&db, &section)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        record_timeline(
            &db,
            Some(service_id),
            AppEvent::SermonStarted,
            LogCategory::App,
            &sermon,
        );
    }
    *state
        .active_sermon
        .lock()
        .expect("active_sermon mutex poisoned") = Some(sermon.clone());
    *state
        .active_sermon_section
        .lock()
        .expect("active_sermon_section mutex poisoned") = Some(section);

    let finding = sermon_foundation::finding_for_lifecycle_event(service_id, &sermon, "started");
    state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned")
        .add(finding);
    let _ = emit(&app, AppEvent::SermonStarted, sermon.clone());
    Ok(sermon)
}

#[tauri::command]
pub fn pause_sermon(app: AppHandle, state: State<'_, AppState>) -> Result<Sermon, AppError> {
    let mut sermon = active_sermon_or_error(&state).map_err(log_and_return)?;
    ensure_valid_sermon_transition(sermon.status, SermonStatus::Paused).map_err(log_and_return)?;
    sermon.pause();

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::update_sermon(&db, &sermon)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        record_timeline(
            &db,
            Some(sermon.service_id),
            AppEvent::SermonPaused,
            LogCategory::App,
            &sermon,
        );
    }
    *state
        .active_sermon
        .lock()
        .expect("active_sermon mutex poisoned") = Some(sermon.clone());

    let finding =
        sermon_foundation::finding_for_lifecycle_event(sermon.service_id, &sermon, "paused");
    state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned")
        .add(finding);
    let _ = emit(&app, AppEvent::SermonPaused, sermon.clone());
    Ok(sermon)
}

#[tauri::command]
pub fn resume_sermon(app: AppHandle, state: State<'_, AppState>) -> Result<Sermon, AppError> {
    let mut sermon = active_sermon_or_error(&state).map_err(log_and_return)?;
    ensure_valid_sermon_transition(sermon.status, SermonStatus::Active).map_err(log_and_return)?;
    sermon.resume();

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::update_sermon(&db, &sermon)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        record_timeline(
            &db,
            Some(sermon.service_id),
            AppEvent::SermonResumed,
            LogCategory::App,
            &sermon,
        );
    }
    *state
        .active_sermon
        .lock()
        .expect("active_sermon mutex poisoned") = Some(sermon.clone());

    let finding =
        sermon_foundation::finding_for_lifecycle_event(sermon.service_id, &sermon, "resumed");
    state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned")
        .add(finding);
    let _ = emit(&app, AppEvent::SermonResumed, sermon.clone());
    Ok(sermon)
}

/// Ends the active sermon - clears `AppState.active_sermon` entirely
/// (mirrors `end_service`'s `.take()`), also closing whatever section was
/// still open so no section is ever left dangling with no end time once
/// its sermon has ended.
#[tauri::command]
pub fn end_sermon(app: AppHandle, state: State<'_, AppState>) -> Result<Sermon, AppError> {
    let mut sermon = state
        .active_sermon
        .lock()
        .expect("active_sermon mutex poisoned")
        .take()
        .ok_or(AppError::NoActiveSermon)
        .map_err(log_and_return)?;
    ensure_valid_sermon_transition(sermon.status, SermonStatus::Ended).map_err(log_and_return)?;
    sermon.end();

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::update_sermon(&db, &sermon)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        persistence::close_open_sermon_section(
            &db,
            sermon.id,
            sermon.ended_at.unwrap_or_else(chrono::Utc::now),
        )
        .map_err(AppError::from)
        .map_err(log_and_return)?;
        record_timeline(
            &db,
            Some(sermon.service_id),
            AppEvent::SermonEnded,
            LogCategory::App,
            &sermon,
        );
    }
    *state
        .active_sermon_section
        .lock()
        .expect("active_sermon_section mutex poisoned") = None;

    let finding =
        sermon_foundation::finding_for_lifecycle_event(sermon.service_id, &sermon, "ended");
    state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned")
        .add(finding);
    let _ = emit(&app, AppEvent::SermonEnded, sermon.clone());
    Ok(sermon)
}

/// Explicit operator correction/assignment of the active sermon's title -
/// unknown until an operator supplies it (spec's "unknown metadata
/// remains unknown" invariant); calling this again later is how a title
/// is corrected, not a separate "correct" action.
#[tauri::command]
pub fn set_sermon_title(
    title: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Sermon, AppError> {
    let title = require_non_empty(&title, "title").map_err(log_and_return)?;
    let mut sermon = active_sermon_or_error(&state).map_err(log_and_return)?;
    sermon.set_title(title.clone());

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::update_sermon(&db, &sermon)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        record_timeline(
            &db,
            Some(sermon.service_id),
            AppEvent::SermonMetadataChanged,
            LogCategory::App,
            &sermon,
        );
    }
    *state
        .active_sermon
        .lock()
        .expect("active_sermon mutex poisoned") = Some(sermon.clone());

    let finding = sermon_foundation::finding_for_metadata_updated(
        sermon.service_id,
        sermon.id,
        "title",
        &title,
    );
    state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned")
        .add(finding);
    let _ = emit(&app, AppEvent::SermonMetadataChanged, sermon.clone());
    Ok(sermon)
}

/// Explicit operator speaker assignment - never biometric/automatic
/// speaker recognition (spec's "SPEAKER MODEL" section). Calling this
/// again replaces the previously assigned speaker; the prior value is
/// still recoverable from the timeline/audit trail, never silently lost
/// with no record.
#[tauri::command]
pub fn assign_sermon_speaker(
    name: String,
    role: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Sermon, AppError> {
    let name = require_non_empty(&name, "name").map_err(log_and_return)?;
    let role = parse_speaker_role_input(&role).map_err(log_and_return)?;
    let mut sermon = active_sermon_or_error(&state).map_err(log_and_return)?;
    let speaker = Speaker::new(name, role);
    sermon.assign_speaker(speaker.clone());

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::update_sermon(&db, &sermon)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        record_timeline(
            &db,
            Some(sermon.service_id),
            AppEvent::SermonSpeakerChanged,
            LogCategory::App,
            &sermon,
        );
    }
    *state
        .active_sermon
        .lock()
        .expect("active_sermon mutex poisoned") = Some(sermon.clone());

    let finding =
        sermon_foundation::finding_for_speaker_assigned(sermon.service_id, sermon.id, &speaker);
    state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned")
        .add(finding);
    let _ = emit(&app, AppEvent::SermonSpeakerChanged, sermon.clone());
    Ok(sermon)
}

/// Explicit operator section assignment - closes whatever section was
/// previously open (with an explicit, shared timestamp, never deleting
/// its history) and opens the new one. Never inferred from transcript
/// content in this phase (spec's "message section state" rule: "do not
/// allow two active sections simultaneously").
#[tauri::command]
pub fn change_sermon_section(
    kind: String,
    note: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SermonSection, AppError> {
    let kind = parse_section_kind_input(&kind).map_err(log_and_return)?;
    let sermon = active_sermon_or_error(&state).map_err(log_and_return)?;
    let new_section = SermonSection::open(sermon.id, kind, SectionOrigin::OperatorAssigned, note);

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::close_open_sermon_section(&db, sermon.id, new_section.started_at)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        persistence::persist_sermon_section(&db, &new_section)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        record_timeline(
            &db,
            Some(sermon.service_id),
            AppEvent::SermonSectionChanged,
            LogCategory::App,
            &new_section,
        );
    }
    *state
        .active_sermon_section
        .lock()
        .expect("active_sermon_section mutex poisoned") = Some(new_section.clone());

    let finding =
        sermon_foundation::finding_for_section_changed(sermon.service_id, sermon.id, &new_section);
    state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned")
        .add(finding);
    let _ = emit(&app, AppEvent::SermonSectionChanged, new_section.clone());
    Ok(new_section)
}

/// Explicitly links an already-persisted transcript segment (from any
/// existing ingestion path - `process_test_transcript`, or a real live
/// segment) to the active sermon - "which portion of the transcript
/// belongs to this sermon," never a second transcript-creation path (spec's
/// "SERMON → TRANSCRIPT RELATIONSHIP" section). Rejects a segment that
/// belongs to a different service outright; never silently reassigns.
#[tauri::command]
pub fn link_transcript_segment_to_sermon(
    transcript_segment_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SermonSegment, AppError> {
    let transcript_segment_id = parse_uuid(&transcript_segment_id).map_err(log_and_return)?;
    let sermon = active_sermon_or_error(&state).map_err(log_and_return)?;

    let db = state.db.lock().expect("db connection poisoned");
    let owning_service = persistence::get_transcript_segment_service_id(&db, transcript_segment_id)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    match owning_service {
        Some(service_id) if service_id == sermon.service_id => {}
        Some(_) => {
            return Err(log_and_return(AppError::InvalidInput(
                "transcript segment belongs to a different service".to_string(),
            )))
        }
        None => {
            return Err(log_and_return(AppError::InvalidInput(
                "unknown transcript segment".to_string(),
            )))
        }
    }

    let sequence = persistence::count_sermon_segments(&db, sermon.id)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    let section_id = state
        .active_sermon_section
        .lock()
        .expect("active_sermon_section mutex poisoned")
        .as_ref()
        .map(|s| s.id);
    let segment = SermonSegment::new(sermon.id, transcript_segment_id, sequence, section_id);
    persistence::persist_sermon_segment(&db, &segment)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    record_timeline(
        &db,
        Some(sermon.service_id),
        AppEvent::SermonSegmentLinked,
        LogCategory::App,
        &segment,
    );
    drop(db);

    let _ = emit(&app, AppEvent::SermonSegmentLinked, segment.clone());
    Ok(segment)
}

/// Every transcript segment linked to the active sermon, in link order -
/// the read side of `link_transcript_segment_to_sermon`, never the
/// transcript text itself (follow `transcriptSegmentId` back to
/// `list_transcript_history`/`TranscriptUpdated` for that).
#[tauri::command]
pub fn list_sermon_segments(state: State<'_, AppState>) -> Result<Vec<SermonSegment>, AppError> {
    let sermon = active_sermon_or_error(&state).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::list_sermon_segments(&db, sermon.id)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// Every section (open or closed) recorded for the active sermon, in the
/// order they were opened - the read side of `change_sermon_section`,
/// including history `change_sermon_section` itself never exposes.
#[tauri::command]
pub fn list_sermon_sections(state: State<'_, AppState>) -> Result<Vec<SermonSection>, AppError> {
    let sermon = active_sermon_or_error(&state).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::list_sermon_sections(&db, sermon.id)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// Sermons previously delivered in the active service, most recently
/// created first - the sermon-history counterpart to `list_service_history`.
#[tauri::command]
pub fn list_sermon_history(
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<Sermon>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::list_sermons_for_service(&db, service_id, limit)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// A single sermon by id, independent of whichever one (if any) is
/// currently active - the sermon archive's detail view, mirroring
/// `get_service`.
#[tauri::command]
pub fn get_sermon(sermon_id: String, state: State<'_, AppState>) -> Result<Sermon, AppError> {
    let id = parse_uuid(&sermon_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::get_sermon(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// Phase 3.9: assembles the currently-active sermon's already-captured
/// data (sections, findings, Bible suggestions, transcript, timeline)
/// into one read-only bundle - see `crate::harvest`'s own module docs for
/// why this is deliberately not a new detection pass. Scoped to the
/// active sermon only: `IntelligenceFinding`'s `sermon_id` linkage lives
/// in the in-memory `FindingQueue` (never persisted, by the same Phase
/// 2.0 design `docs/phase-3-8-7-6-live-intelligence-integration-audit.md`
/// documents for every other domain), so harvesting a past sermon after
/// an app restart would silently produce an empty `elements` list - this
/// command refuses that case honestly instead.
#[tauri::command]
pub fn harvest_sermon(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::harvest::SermonHarvest, AppError> {
    let sermon = active_sermon_or_error(&state).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    let findings = state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned");
    let harvest = crate::harvest::harvest_sermon(&db, &findings, &sermon)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    drop(findings);
    record_timeline(
        &db,
        Some(sermon.service_id),
        AppEvent::SermonHarvested,
        LogCategory::App,
        serde_json::json!({ "sermonId": sermon.id, "elementCount": harvest.elements.len() }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::SermonHarvested, harvest.clone());
    Ok(harvest)
}

/// The Church Knowledge Base (Phase 13): read-only, cross-sermon
/// aggregation of every operator-accepted Sermon Intelligence finding,
/// spanning every service (not just the currently active one) and
/// surviving a restart - unlike `harvest_sermon`, which reads the live
/// in-memory finding queue for one sermon. Open to any operator - a
/// read of already-accepted history is no more sensitive than
/// `list_saved_content`.
#[tauri::command]
pub fn get_church_knowledge_base(
    state: State<'_, AppState>,
) -> Result<crate::sermon_knowledge_base::SermonKnowledgeBase, AppError> {
    let db = state.db.lock().expect("db connection poisoned");
    crate::sermon_knowledge_base::get_knowledge_base(&db)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

// --- cross-domain intelligence (Phase 2.4, extended in Phase 2.8) -----------
//
// The correlation layer only ever *reads* `state.intelligence_findings`
// (every domain, via `build_music_context` - generic despite its name, see
// that function's own docs) and, since Phase 2.8, `state.content_candidate_queue`
// (also via `build_music_context`) - and writes to its own, separate
// `state.correlation_queue`. It never calls another engine directly and
// never mutates a source finding or a content candidate - see
// `cross_domain.rs`'s module docs.

/// Run the correlation engine (Phase 2.4, extended in Phase 2.8) against
/// this app's real, current state and queue any new correlations - an
/// explicit operator/diagnostic action, never triggered automatically by a
/// transcript segment arriving (spec section 24: "read-only... never
/// automatic"). Reuses `build_music_context` to see every domain's queued
/// findings and (Phase 2.8) content candidates, not just Music's.
#[tauri::command]
pub fn analyze_cross_domain(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<IntelligenceCorrelation>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let context = build_music_context(&state, service_id).map_err(log_and_return)?;

    let engine = CrossDomainCorrelationEngine::new();
    let queued = {
        let mut correlations = state
            .correlation_queue
            .lock()
            .expect("correlation_queue mutex poisoned");
        crate::cross_domain::analyze_and_queue(&engine, &context, &mut correlations)
    };

    let db = state.db.lock().expect("db connection poisoned");
    for correlation in &queued {
        record_timeline(
            &db,
            Some(service_id),
            AppEvent::CrossDomainCorrelationDetected,
            LogCategory::App,
            serde_json::json!({
                "correlationId": correlation.id,
                "kind": correlation.kind.label(),
                "summary": &correlation.summary,
                "confidence": correlation.confidence.score,
            }),
        );
    }
    drop(db);
    for correlation in &queued {
        let _ = emit(
            &app,
            AppEvent::CrossDomainCorrelationDetected,
            correlation.clone(),
        );
    }

    Ok(queued)
}

/// Cross-domain correlations still awaiting an operator decision
/// (`Detected`/`Reviewed`), for the active service - the Cross-Domain
/// Intelligence panel's read-only data source. Mirrors
/// `list_music_findings`/`list_sermon_findings`.
#[tauri::command]
pub fn list_cross_domain_correlations(
    state: State<'_, AppState>,
) -> Result<Vec<IntelligenceCorrelation>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let correlations = state
        .correlation_queue
        .lock()
        .expect("correlation_queue mutex poisoned");
    Ok(correlations
        .pending()
        .into_iter()
        .filter(|c| c.service_id == service_id)
        .cloned()
        .collect())
}

/// Explicit operator review of a correlation - informational only
/// (spec section 25), changes only this correlation's own status.
#[tauri::command]
pub fn review_cross_domain_correlation(
    correlation_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<IntelligenceCorrelation, AppError> {
    let id = parse_uuid(&correlation_id).map_err(log_and_return)?;
    let updated = {
        let mut correlations = state
            .correlation_queue
            .lock()
            .expect("correlation_queue mutex poisoned");
        correlations
            .review(id)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        correlations
            .get(id)
            .cloned()
            .expect("just-reviewed correlation is still present")
    };
    let _ = emit(
        &app,
        AppEvent::CrossDomainCorrelationReviewed,
        updated.clone(),
    );
    Ok(updated)
}

/// Explicit operator dismissal of a correlation (spec section 25) - never
/// automatic, and has no way to alter the source findings, the transcript,
/// or the active Scripture context.
#[tauri::command]
pub fn dismiss_cross_domain_correlation(
    correlation_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<IntelligenceCorrelation, AppError> {
    let id = parse_uuid(&correlation_id).map_err(log_and_return)?;
    let updated = {
        let mut correlations = state
            .correlation_queue
            .lock()
            .expect("correlation_queue mutex poisoned");
        correlations
            .dismiss(id)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        correlations
            .get(id)
            .cloned()
            .expect("just-dismissed correlation is still present")
    };
    let db = state.db.lock().expect("db connection poisoned");
    record_timeline(
        &db,
        Some(updated.service_id),
        AppEvent::CrossDomainCorrelationDismissed,
        LogCategory::App,
        serde_json::json!({ "correlationId": updated.id, "summary": &updated.summary }),
    );
    drop(db);
    let _ = emit(
        &app,
        AppEvent::CrossDomainCorrelationDismissed,
        updated.clone(),
    );
    Ok(updated)
}

// --- content intelligence (Phase 2.7, per the authoritative Phase 2 roadmap) --
//
// The `ContentCandidate` counterpart to the cross-domain correlation block
// above. `ContentIntelligenceEngine` reads `context.recent_findings`
// (every domain, via `build_music_context`) and structures candidates -
// never calls another engine, never mutates a source finding. See
// `content_intelligence.rs`'s module docs.

/// Run the Phase 2.7 content-intelligence layer against this app's real,
/// current state and queue any new candidates - an explicit operator/
/// diagnostic action, never triggered automatically by a transcript
/// segment arriving (mirrors `analyze_cross_domain` exactly).
#[tauri::command]
pub fn analyze_content_intelligence(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ContentCandidate>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let context = build_music_context(&state, service_id).map_err(log_and_return)?;

    let engine = ContentIntelligenceEngine::new();
    let queued = {
        let mut candidates = state
            .content_candidate_queue
            .lock()
            .expect("content_candidate_queue mutex poisoned");
        crate::content_intelligence::analyze_and_queue(&engine, &context, &mut candidates)
    };

    let db = state.db.lock().expect("db connection poisoned");
    for candidate in &queued {
        record_timeline(
            &db,
            Some(service_id),
            AppEvent::ContentCandidateDetected,
            LogCategory::App,
            serde_json::json!({
                "candidateId": candidate.id,
                "candidateType": candidate.candidate_type.label(),
                "titleOrLabel": &candidate.title_or_label,
                "contentPotential": candidate.content_potential,
            }),
        );
    }
    drop(db);
    for candidate in &queued {
        let _ = emit(&app, AppEvent::ContentCandidateDetected, candidate.clone());
    }

    Ok(queued)
}

/// Content candidates still awaiting an operator decision
/// (`Detected`/`Reviewed`), for the active service - the Content
/// Intelligence panel's read-only data source. Mirrors
/// `list_cross_domain_correlations`.
#[tauri::command]
pub fn list_content_candidates(
    state: State<'_, AppState>,
) -> Result<Vec<ContentCandidate>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let candidates = state
        .content_candidate_queue
        .lock()
        .expect("content_candidate_queue mutex poisoned");
    Ok(candidates
        .pending()
        .into_iter()
        .filter(|c| c.service_id == service_id)
        .cloned()
        .collect())
}

/// Phase 3.0: content candidates the operator has already accepted, for
/// the active service - the "Saved Content" view's read-only data source.
/// Before this command existed, `accept_content_candidate` was a genuine
/// dead end: the candidate's text (`working_concept`) became permanently
/// unreachable in the running UI the moment it was accepted, since
/// `list_content_candidates` only ever returns `pending()`
/// (`Detected`/`Reviewed`). `ContentCandidateQueue::all()` already
/// retained every candidate regardless of status - this only exposes that
/// existing data over IPC, exactly the "smallest useful downstream
/// action" a saved-content list needs; it still has no code path into
/// `presentation::persist_prepared_item` or anything that could
/// publish/schedule/project it.
#[tauri::command]
pub fn list_accepted_content_candidates(
    state: State<'_, AppState>,
) -> Result<Vec<ContentCandidate>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let candidates = state
        .content_candidate_queue
        .lock()
        .expect("content_candidate_queue mutex poisoned");
    Ok(candidates
        .all()
        .into_iter()
        .filter(|c| c.service_id == service_id && c.status == FindingStatus::Accepted)
        .cloned()
        .collect())
}

/// Explicit operator acceptance of a content opportunity (spec section
/// 11/35) - changes only the candidate's own status; has no code path into
/// `presentation::persist_prepared_item` or anything else that could
/// publish/schedule/project it.
///
/// Phase 2.7.1: also persists a durable copy to `saved_content_candidates`
/// (see `database/migrations/0011_saved_content_candidates.sql`) - the
/// audit (`docs/phase-2-7-1-audit.md` section E) found that acceptance
/// previously only flipped the in-memory `ContentCandidateQueue` entry's
/// status, so an accepted candidate did not survive the service ending or
/// an application restart. The in-memory queue itself is untouched by
/// this addition; `list_accepted_content_candidates` still reads from it
/// exactly as before for the live-session view.
#[tauri::command]
pub fn accept_content_candidate(
    candidate_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ContentCandidate, AppError> {
    let id = parse_uuid(&candidate_id).map_err(log_and_return)?;
    let updated = {
        let mut candidates = state
            .content_candidate_queue
            .lock()
            .expect("content_candidate_queue mutex poisoned");
        candidates
            .accept(id)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        candidates
            .get(id)
            .cloned()
            .expect("just-accepted candidate is still present")
    };
    let db = state.db.lock().expect("db connection poisoned");
    persistence::persist_saved_content_candidate(&db, &updated)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    record_timeline(
        &db,
        Some(updated.service_id),
        AppEvent::ContentCandidateAccepted,
        LogCategory::App,
        serde_json::json!({ "candidateId": updated.id, "titleOrLabel": &updated.title_or_label }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::ContentCandidateAccepted, updated.clone());
    Ok(updated)
}

/// Every content candidate saved (accepted) for one service, most
/// recently saved first - reopens what `accept_content_candidate`
/// persisted, regardless of whether that service is still active or the
/// application has since restarted. Mirrors `list_presentation_history`'s
/// exact existing shape/signature (spec section 20: reuse the established
/// pattern rather than inventing a new one).
#[tauri::command]
pub fn list_saved_content(
    service_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ContentCandidate>, AppError> {
    let id = parse_uuid(&service_id).map_err(log_and_return)?;
    let db = state.db.lock().expect("db connection poisoned");
    persistence::list_saved_content_candidates_for_service(&db, id)
        .map_err(AppError::from)
        .map_err(log_and_return)
}

/// Explicit operator rejection of a content candidate - never automatic,
/// and has no way to alter the source finding, the transcript, or the
/// active Scripture context.
#[tauri::command]
pub fn reject_content_candidate(
    candidate_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ContentCandidate, AppError> {
    let id = parse_uuid(&candidate_id).map_err(log_and_return)?;
    let updated = {
        let mut candidates = state
            .content_candidate_queue
            .lock()
            .expect("content_candidate_queue mutex poisoned");
        candidates
            .reject(id)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        candidates
            .get(id)
            .cloned()
            .expect("just-rejected candidate is still present")
    };
    let db = state.db.lock().expect("db connection poisoned");
    record_timeline(
        &db,
        Some(updated.service_id),
        AppEvent::ContentCandidateRejected,
        LogCategory::App,
        serde_json::json!({ "candidateId": updated.id, "titleOrLabel": &updated.title_or_label }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::ContentCandidateRejected, updated.clone());
    Ok(updated)
}

// --- service intelligence (Phase 2.4, per the authoritative Phase 2 roadmap) --
//
// Distinct from the cross-domain correlation work above (an earlier
// prototype, developed under an internal label that also read "Phase
// 2.4" - the authoritative roadmap this section follows reserves that
// functionality for a future formal Phase 2.8 integration; nothing in
// it was modified to make room for this section). Deliberately manual-
// command-only for `analyze_service_transcript`, mirroring Music's/
// Sermon's/the Bible bridge's own established pattern - nothing here is
// wired into `pipeline.rs::handle_final_transcript`.

/// The current service-phase state plus a read-only transcript-freshness
/// signal - the shape `get_service_intelligence_state` returns. Never a
/// second `LiveStatus`: audio/speech/database health still comes from
/// `get_live_status` alone: this struct only adds what that one doesn't
/// already have (the inferred service phase, and whether the transcript
/// itself has gone quiet).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceIntelligenceSummary {
    pub phase: ServicePhase,
    pub phase_started_at: chrono::DateTime<chrono::Utc>,
    pub previous_phase: Option<ServicePhase>,
    pub transition_count: u32,
    pub transcript_freshness: crate::service::TranscriptFreshness,
}

/// The deterministic service-phase-analysis harness, exposed over IPC -
/// the Service Intelligence counterpart to `analyze_sermon_transcript`.
/// Persists `text` as an ordinary transcript segment, builds a real
/// `IntelligenceContext` (via `build_music_context`, generic despite its
/// name - see that function's own docs), and calls `AppState.service_engine`
/// directly - the same accumulating-state instance every call goes
/// through, never the separate diagnostic-only copy in
/// `intelligence_registry` (see `service.rs`'s module docs). Findings are
/// queued in `AppState.intelligence_findings`; this command never
/// prepares or projects a presentation item.
#[tauri::command]
pub fn analyze_service_transcript(
    text: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<IntelligenceFinding>, AppError> {
    let text = require_non_empty(&text, "text").map_err(log_and_return)?;
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let sequence = state.transcript_sequence.fetch_add(1, Ordering::SeqCst);

    let segment = TranscriptSegment {
        id: Uuid::new_v4(),
        sequence,
        text,
        is_final: true,
        confidence: ConfidenceResult::new(
            1.0,
            ConfidenceSource::Human,
            Some("manually entered test transcript for service analysis".to_string()),
        ),
        start_ms: 0,
        end_ms: 0,
        language: Some("en".to_string()),
        speaker_id: None,
    };

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::persist_transcript_segment(&db, service_id, &segment)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
    }
    let _ = emit(&app, AppEvent::TranscriptUpdated, segment.clone());

    let context = build_music_context(&state, service_id).map_err(log_and_return)?;
    let input = IntelligenceInput::new(service_id, segment);

    let queued = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        crate::service::analyze_and_queue(&state.service_engine, &input, &context, &mut findings)
            .map_err(AppError::from)
            .map_err(log_and_return)?
    };

    let db = state.db.lock().expect("db connection poisoned");
    for finding in &queued {
        let event = if crate::service::is_anomaly_finding(finding) {
            AppEvent::ServiceAnomalyDetected
        } else {
            AppEvent::ServicePhaseChanged
        };
        record_timeline(
            &db,
            Some(service_id),
            event,
            LogCategory::App,
            serde_json::json!({
                "findingId": finding.id,
                "summary": &finding.summary,
                "confidence": finding.confidence.score,
            }),
        );
    }
    drop(db);
    for finding in &queued {
        let event = if crate::service::is_anomaly_finding(finding) {
            AppEvent::ServiceAnomalyDetected
        } else {
            AppEvent::ServicePhaseChanged
        };
        let _ = emit(&app, event, finding.clone());
    }

    Ok(queued)
}

/// Read-only current phase/transition-count/transcript-freshness snapshot -
/// safe to poll at any time, including before any service has started
/// (freshness reports `unknown`, phase reports `unknown`).
#[tauri::command]
pub fn get_service_intelligence_state(state: State<'_, AppState>) -> ServiceIntelligenceSummary {
    let snapshot = state.service_engine.snapshot();
    let last_transcript_at = *state
        .last_transcript_at
        .lock()
        .expect("last_transcript_at mutex poisoned");
    let transcript_freshness =
        crate::service::transcript_freshness(last_transcript_at, chrono::Utc::now());
    ServiceIntelligenceSummary {
        phase: snapshot.phase,
        phase_started_at: snapshot.phase_started_at,
        previous_phase: snapshot.previous_phase,
        transition_count: snapshot.transition_count,
        transcript_freshness,
    }
}

/// Every recorded phase transition for the active service, oldest first -
/// a history view (like `list_timeline`), not an operator-review queue:
/// includes transitions an operator has already reviewed/accepted, not
/// just pending ones.
#[tauri::command]
pub fn list_service_transitions(
    state: State<'_, AppState>,
) -> Result<Vec<IntelligenceFinding>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let findings = state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned");
    Ok(findings
        .all()
        .into_iter()
        .filter(|f| f.service_id == service_id && crate::service::is_transition_finding(f))
        .cloned()
        .collect())
}

/// Anomaly findings still awaiting an operator decision - mirrors
/// `list_music_findings`/`list_sermon_findings`'s "operator review queue"
/// shape, filtered to only the `"Anomaly:"`-prefixed subset of Service
/// findings (transitions themselves are never anomalies - see
/// `list_service_transitions`).
#[tauri::command]
pub fn list_service_anomalies(
    state: State<'_, AppState>,
) -> Result<Vec<IntelligenceFinding>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let findings = state
        .intelligence_findings
        .lock()
        .expect("intelligence_findings mutex poisoned");
    Ok(findings
        .pending()
        .into_iter()
        .filter(|f| f.service_id == service_id && crate::service::is_anomaly_finding(f))
        .cloned()
        .collect())
}

/// Explicit operator declaration of the current service phase (spec
/// section 19) - for when nothing has been detected yet, or the operator
/// wants to proactively state the phase. Transitions immediately, bypasses
/// debounce entirely, and is always `Observed`. Never supersedes/rejects
/// any other pending finding - see `correct_service_phase` for that.
#[tauri::command]
pub fn mark_service_phase(
    phase: String,
    note: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<IntelligenceFinding, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let parsed_phase = crate::service::parse_service_phase(&phase).ok_or_else(|| {
        log_and_return(AppError::InvalidInput(format!(
            "unknown service phase: {phase}"
        )))
    })?;

    let finding = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        crate::service::apply_operator_action(
            &state.service_engine,
            service_id,
            parsed_phase,
            note.as_deref(),
            false,
            &mut findings,
        )
    };
    let db = state.db.lock().expect("db connection poisoned");
    record_timeline(
        &db,
        Some(service_id),
        AppEvent::ServicePhaseCorrected,
        LogCategory::App,
        serde_json::json!({ "findingId": finding.id, "summary": &finding.summary }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::ServicePhaseCorrected, finding.clone());
    Ok(finding)
}

/// Explicit operator correction of an incorrect system-detected phase
/// (spec section 20) - like `mark_service_phase`, but additionally
/// rejects any other still-pending transition finding for this service:
/// the operator's correction supersedes whatever the system last
/// inferred. Never rewrites or deletes that earlier finding - `reject`
/// only changes its own `status`, so it remains fully auditable via
/// `list_service_transitions` (which includes rejected transitions too).
#[tauri::command]
pub fn correct_service_phase(
    phase: String,
    note: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<IntelligenceFinding, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
    let parsed_phase = crate::service::parse_service_phase(&phase).ok_or_else(|| {
        log_and_return(AppError::InvalidInput(format!(
            "unknown service phase: {phase}"
        )))
    })?;

    let finding = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        let superseded: Vec<Uuid> = findings
            .pending()
            .iter()
            .filter(|f| f.service_id == service_id && crate::service::is_transition_finding(f))
            .map(|f| f.id)
            .collect();
        for id in superseded {
            let _ = findings.reject(id);
        }
        crate::service::apply_operator_action(
            &state.service_engine,
            service_id,
            parsed_phase,
            note.as_deref(),
            true,
            &mut findings,
        )
    };
    let db = state.db.lock().expect("db connection poisoned");
    record_timeline(
        &db,
        Some(service_id),
        AppEvent::ServicePhaseCorrected,
        LogCategory::App,
        serde_json::json!({ "findingId": finding.id, "summary": &finding.summary }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::ServicePhaseCorrected, finding.clone());
    Ok(finding)
}

/// Explicit operator acknowledgment of an anomaly finding - reuses the
/// ordinary `FindingQueue::accept` lifecycle exactly (spec section 27's
/// persistence-reuse preference: no bespoke anomaly-tracking system).
#[tauri::command]
pub fn acknowledge_service_anomaly(
    finding_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<IntelligenceFinding, AppError> {
    let id = parse_uuid(&finding_id).map_err(log_and_return)?;
    let updated = {
        let mut findings = state
            .intelligence_findings
            .lock()
            .expect("intelligence_findings mutex poisoned");
        findings
            .accept(id)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
        findings
            .get(id)
            .cloned()
            .expect("just-accepted finding is still present")
    };
    let db = state.db.lock().expect("db connection poisoned");
    record_timeline(
        &db,
        Some(updated.service_id),
        AppEvent::ServiceAnomalyAcknowledged,
        LogCategory::App,
        serde_json::json!({ "findingId": updated.id, "summary": &updated.summary }),
    );
    drop(db);
    let _ = emit(&app, AppEvent::ServiceAnomalyAcknowledged, updated.clone());
    Ok(updated)
}

// --- live status -------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveServiceStatus {
    Planned,
    Live,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioStatusKind {
    Unavailable,
    Ready,
    Listening,
    /// A real capture failure has been recorded and not yet cleared by a
    /// successful retry (Phase 1.3 audio failure recovery) - distinct
    /// from `Unavailable` (no device at all). See `AppState::audio_error`.
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechStatusKind {
    Unavailable,
    Ready,
    /// A real speech-engine failure has been recorded and not yet cleared
    /// by the next successful `feed_audio` call (Phase 1.3 speech failure
    /// recovery). See `AppState::speech_error`.
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkStatusKind {
    Offline,
    Online,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseStatusKind {
    Connected,
    Error,
}

/// Deliberately never derived from `NetworkStatusKind` - see
/// `docs/live-speech.md`: a fully offline machine with a local speech
/// model installed is `Available`, not `Degraded`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiStatusKind {
    Available,
    Degraded,
    /// Reserved for a total Bible Intelligence Core failure (e.g. the
    /// local database itself is unreachable). Nothing in Phase 1.2
    /// constructs this yet - a missing/not-ready speech engine is
    /// `Degraded`, not `Unavailable`, since manual operation still works.
    #[allow(dead_code)]
    Unavailable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    pub service: Option<ServiceSession>,
    pub service_status: LiveServiceStatus,
    pub audio: AudioEngineStatus,
    pub audio_status: AudioStatusKind,
    /// Phase 6.4 (Operator Ergonomics: buried audio/speech error
    /// visibility): the real error text behind `audio_status ==
    /// AudioStatusKind::Error`, computed with the exact same
    /// `audio.stream_error.or(state.audio_error)` precedence that
    /// decision already uses - so this can never say `Error` with no
    /// text, or show text for a status that isn't `Error`. `None`
    /// whenever `audio_status != Error`.
    pub audio_error_text: Option<String>,
    pub speech_status: SpeechStatusKind,
    /// Phase 6.4: the real error text behind `speech_status ==
    /// SpeechStatusKind::Error`, read from the exact same
    /// `state.speech_error` that decision already checks. `None`
    /// whenever `speech_status != Error`.
    pub speech_error_text: Option<String>,
    pub network_status: NetworkStatusKind,
    pub ai_status: AiStatusKind,
    pub database_status: DatabaseStatusKind,
    /// Phase 2.2: whether/why acoustic recognition can currently run -
    /// reused here rather than a separate `get_acoustic_music_status`
    /// command, since the frontend already polls `get_live_status` for
    /// every other engine's status.
    pub acoustic_status: acoustic::AcousticEngineStatus,
    /// Phase 2.2: the operator-confirmed current song, if any - `None`
    /// until an operator accepts a Music finding. See
    /// `cip_core_music::CurrentSong`'s docs.
    pub current_song: Option<cip_core_music::CurrentSong>,
    /// Phase 3.0: the real production Bible dataset's own registry row
    /// (name, version, licensing status, checksum), read fresh on every
    /// poll - reuses `ContentMetadata` unmodified rather than inventing a
    /// second Bible-readiness type. `None` means the dataset has not
    /// (yet, or successfully) been imported/registered; a genuine
    /// first-run condition every other domain in this struct already
    /// models the same way (`Unavailable`/`Error`), now extended to
    /// Bible/BSB, which `LiveStatus` never surfaced before this phase.
    pub bible: Option<ContentMetadata>,
}

/// Phase 6.4 (Operator Ergonomics: buried audio/speech error visibility) -
/// the one rule both `audio_error_text` and `speech_error_text` share:
/// only ever surface text when the status it belongs to actually
/// resolved to `Error`, even if a stale, not-yet-cleared error string is
/// still sitting in state (e.g. a past failure that hasn't been
/// overwritten by a subsequent success). Pure and directly testable
/// without a running engine or a locked mutex - the mutex-reading glue
/// around it is exercised via `get_live_status` itself.
fn error_text_if(is_error: bool, text: Option<String>) -> Option<String> {
    if is_error {
        text
    } else {
        None
    }
}

fn check_network_online() -> bool {
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    // A short, best-effort reachability probe for the status indicator
    // only - never a functional dependency of anything else (see
    // docs/live-speech.md).
    let addr: SocketAddr = ([1, 1, 1, 1], 443).into();
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

#[tauri::command]
pub fn get_live_status(state: State<'_, AppState>) -> LiveStatus {
    let service = state
        .active_service
        .lock()
        .expect("active_service mutex poisoned")
        .clone();
    let service_status = match &service {
        None => LiveServiceStatus::Planned,
        Some(s) => match s.status {
            ServiceStatus::Started => LiveServiceStatus::Live,
            ServiceStatus::Paused => LiveServiceStatus::Paused,
            ServiceStatus::Ended => LiveServiceStatus::Completed,
        },
    };

    let audio = state
        .audio_engine
        .lock()
        .expect("audio_engine mutex poisoned")
        .status();
    // Phase 6.4: the same precedence the `Error` branch below already
    // used to decide *whether* something is wrong now also captures
    // *what* - `audio.stream_error` (a real mid-capture hardware failure)
    // preferred over `state.audio_error` (a synchronous start_listening
    // failure), since a mid-capture failure is the more specific/recent
    // signal when both happen to be set.
    let audio_error_text = audio.stream_error.clone().or_else(|| {
        state
            .audio_error
            .lock()
            .expect("audio_error mutex poisoned")
            .clone()
    });
    let audio_status = if audio.is_capturing {
        AudioStatusKind::Listening
    } else if audio_error_text.is_some() {
        // Phase 3.2: `audio.stream_error` is a real mid-capture hardware
        // failure (e.g. a microphone physically unplugged while
        // listening), reported by the backend's own stream-error
        // callback - see `AudioEngineStatus::stream_error`'s docs. Without
        // this check, `is_capturing` would already be false (the backend
        // flips it the moment the stream dies) and this branch would fall
        // through to the generic `Ready`/`Unavailable` device-enumeration
        // check below, silently hiding a real failure the operator needs
        // to see.
        AudioStatusKind::Error
    } else {
        match state
            .audio_engine
            .lock()
            .expect("audio_engine mutex poisoned")
            .list_devices()
        {
            Ok(devices) if !devices.is_empty() => AudioStatusKind::Ready,
            _ => AudioStatusKind::Unavailable,
        }
    };
    // Phase 6.4: `audio_error_text` can be `Some` (a not-yet-cleared past
    // error) even while `audio_status` resolved to `Listening` above (a
    // fresh successful start doesn't retroactively clear
    // `stream_error`/`audio_error` here - only the next real error or
    // success does, per those fields' own docs) - only surface the text
    // when the status actually says `Error`, matching `LiveStatus`'s own
    // documented contract for this field.
    let audio_error_text = error_text_if(audio_status == AudioStatusKind::Error, audio_error_text);

    // Phase 3.8.7.3: reads the cached `state.speech_ready` field rather
    // than locking `speech_engine` - see that field's own docs (Finding 3).
    // This is the exact poll (frontend cadence 3000ms) that previously
    // blocked behind an in-progress Whisper inference holding that mutex.
    let speech_ready = state.speech_ready;
    // Phase 6.4: read once, reused for both the status decision and the
    // real text `LiveStatus.speech_error_text` exposes.
    let speech_error_text = state
        .speech_error
        .lock()
        .expect("speech_error mutex poisoned")
        .clone();
    let speech_status = if speech_error_text.is_some() {
        SpeechStatusKind::Error
    } else if speech_ready {
        SpeechStatusKind::Ready
    } else {
        SpeechStatusKind::Unavailable
    };
    let speech_error_text =
        error_text_if(speech_status == SpeechStatusKind::Error, speech_error_text);

    let network_status = if check_network_online() {
        NetworkStatusKind::Online
    } else {
        NetworkStatusKind::Offline
    };
    let ai_status = if speech_ready {
        AiStatusKind::Available
    } else {
        AiStatusKind::Degraded
    };

    let database_status = {
        let db = state.db.lock().expect("db connection poisoned");
        match db.query_row("SELECT 1", [], |row| row.get::<_, i64>(0)) {
            Ok(_) => DatabaseStatusKind::Connected,
            Err(_) => DatabaseStatusKind::Error,
        }
    };

    let acoustic_status = acoustic::describe_status(
        state
            .acoustic_recognizer
            .lock()
            .expect("acoustic_recognizer mutex poisoned")
            .as_ref(),
    );
    let current_song = state
        .current_song
        .lock()
        .expect("current_song mutex poisoned")
        .clone();

    // Phase 3.0: real Bible dataset readiness, alongside every other
    // domain this function already reports. A registry lookup failure is
    // treated the same as "not found" - `None` - never a panic, matching
    // this whole function's "always returns a status, never fails"
    // contract.
    let bible = state
        .content_registry
        .get(&content::bible_content_id(
            crate::bible_production_dataset::BSB_TRANSLATION_ID,
        ))
        .unwrap_or(None);

    LiveStatus {
        service,
        service_status,
        audio,
        audio_status,
        audio_error_text,
        speech_status,
        speech_error_text,
        network_status,
        ai_status,
        database_status,
        acoustic_status,
        current_song,
        bible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- input validation + extracted guard logic: the part of each
    // command worth testing in isolation from the full Tauri IPC harness -
    // see docs/live-service.md's testing section for why command tests
    // stop here rather than standing up `tauri::test::mock_builder()`
    // (which would require every command to be generic over `R: Runtime`,
    // a signature change to the whole module, not a test-only addition).
    // Persisted/pipeline behavior (what these commands call into) is
    // covered end to end in `persistence.rs`, `pipeline.rs`, and
    // `timeline.rs`.

    // --- Phase 3.8.6 resampling ------------------------------------------

    #[test]
    fn resample_pcm16_is_a_no_op_when_rates_already_match() {
        let samples = vec![1i16, 2, 3, 4, 5];
        assert_eq!(resample_pcm16(&samples, 16_000, 16_000), samples);
    }

    #[test]
    fn resample_pcm16_is_a_no_op_on_empty_input() {
        assert!(resample_pcm16(&[], 44_100, 16_000).is_empty());
    }

    #[test]
    fn resample_pcm16_downsamples_to_roughly_the_expected_length() {
        // 48kHz -> 16kHz is an exact 3:1 ratio (a real Windows "Stereo
        // Mix"-class native rate down to Whisper's required rate).
        let samples: Vec<i16> = (0..48_000).map(|i| (i % 100) as i16).collect();
        let out = resample_pcm16(&samples, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
    }

    #[test]
    fn resample_pcm16_upsamples_to_roughly_the_expected_length() {
        let samples: Vec<i16> = (0..16_000).map(|i| (i % 100) as i16).collect();
        let out = resample_pcm16(&samples, 16_000, 48_000);
        assert_eq!(out.len(), 48_000);
    }

    #[test]
    fn resample_pcm16_preserves_a_constant_signal() {
        // A DC-like constant buffer must resample to the same constant -
        // proves the interpolation doesn't introduce spurious ringing for
        // the simplest possible input.
        let samples = vec![1234i16; 44_100];
        let out = resample_pcm16(&samples, 44_100, 16_000);
        assert!(out.iter().all(|&s| s == 1234));
    }

    #[test]
    fn resample_pcm16_never_panics_on_a_single_sample() {
        let out = resample_pcm16(&[500], 44_100, 16_000);
        assert!(!out.is_empty());
        assert!(out.iter().all(|&s| s == 500));
    }

    // --- Phase 3.8.7.3 backpressure primitives --------------------------
    // Pure, deterministic functions - directly unit-testable without any
    // real threading/audio hardware. Long-run Tests A/B/D from the
    // operator's own spec are exercised through these: "no backlog
    // growth"/"bounded memory"/"recovers" are properties of
    // `classify_overload`'s thresholds and `chunk_duration_ms`'s honest
    // accounting, checked here directly.

    #[test]
    fn chunk_duration_ms_matches_the_operators_own_measured_real_hardware_chunk() {
        // Phase 3.8.7.3 audit: 480 samples @ 48,000 Hz on the operator's
        // real device = 10ms per chunk.
        let chunk = AudioChunk {
            samples: vec![0i16; 480],
            sample_rate_hz: 48_000,
        };
        assert_eq!(chunk_duration_ms(&chunk), 10);
    }

    #[test]
    fn chunk_duration_ms_is_zero_for_a_zero_sample_rate_never_divides_by_zero() {
        let chunk = AudioChunk {
            samples: vec![0i16; 480],
            sample_rate_hz: 0,
        };
        assert_eq!(chunk_duration_ms(&chunk), 0);
    }

    #[test]
    fn chunk_duration_ms_is_zero_for_an_empty_chunk() {
        let chunk = AudioChunk {
            samples: vec![],
            sample_rate_hz: 16_000,
        };
        assert_eq!(chunk_duration_ms(&chunk), 0);
    }

    #[test]
    fn classify_overload_reports_normal_below_the_busy_threshold() {
        assert_eq!(classify_overload(0), OverloadState::Normal);
        assert_eq!(
            classify_overload(BUSY_THRESHOLD_MS - 1),
            OverloadState::Normal
        );
    }

    #[test]
    fn classify_overload_reports_busy_between_busy_and_falling_behind() {
        assert_eq!(classify_overload(BUSY_THRESHOLD_MS), OverloadState::Busy);
        assert_eq!(
            classify_overload(FALLING_BEHIND_THRESHOLD_MS - 1),
            OverloadState::Busy
        );
    }

    #[test]
    fn classify_overload_reports_falling_behind_between_falling_behind_and_overload() {
        assert_eq!(
            classify_overload(FALLING_BEHIND_THRESHOLD_MS),
            OverloadState::FallingBehind
        );
        assert_eq!(
            classify_overload(OVERLOAD_THRESHOLD_MS - 1),
            OverloadState::FallingBehind
        );
    }

    #[test]
    fn classify_overload_reports_overloaded_at_and_above_the_overload_threshold() {
        assert_eq!(
            classify_overload(OVERLOAD_THRESHOLD_MS),
            OverloadState::Overloaded
        );
        assert_eq!(
            classify_overload(OVERLOAD_THRESHOLD_MS * 100),
            OverloadState::Overloaded
        );
    }

    #[test]
    fn classify_overload_recovers_back_to_normal_once_backlog_drains() {
        // Test D (Recovery) from the operator's spec, as a pure-function
        // property: the same backlog depth always classifies the same way
        // - there is no hidden hysteresis/latching state that could get
        // stuck in an overloaded reading after the real backlog clears.
        assert_eq!(
            classify_overload(OVERLOAD_THRESHOLD_MS),
            OverloadState::Overloaded
        );
        assert_eq!(classify_overload(0), OverloadState::Normal);
    }

    #[test]
    fn saturating_sub_u64_never_underflows() {
        let counter = AtomicU64::new(5);
        saturating_sub_u64(&counter, 10);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn should_not_reset_segmenter_on_the_first_isolated_overload_crossing() {
        // Phase 3.8.7.7: on hardware whose own inference duration alone
        // exceeds `OVERLOAD_THRESHOLD_MS`, every successful inference
        // triggers exactly one overload crossing (confirmed on real
        // Windows hardware - see docs/phase-3-8-7-7-audit.md). That single
        // crossing must not wipe the segmenter's just-produced, valid
        // text - it is expected to resolve once the drain clears the
        // channel.
        assert!(!should_reset_segmenter_on_overload(0));
        assert!(!should_reset_segmenter_on_overload(1));
    }

    #[test]
    fn should_reset_segmenter_once_overload_persists_across_consecutive_dequeues() {
        // Backlog still >= threshold on the very next dequeue, immediately
        // after already draining once, means the pipeline cannot keep up
        // independent of any single inference - genuine sustained
        // overload, where Phase 3.8.7.5's original concern (pre-overload
        // text spliced onto unrelated post-recovery text) still applies.
        assert!(should_reset_segmenter_on_overload(2));
        assert!(should_reset_segmenter_on_overload(5));
    }

    #[test]
    fn saturating_sub_u64_subtracts_normally_when_amount_fits() {
        let counter = AtomicU64::new(100);
        saturating_sub_u64(&counter, 30);
        assert_eq!(counter.load(Ordering::SeqCst), 70);
    }

    // --- Phase 1.3 lifecycle/workflow guards ---------------------------

    #[test]
    fn ensure_no_active_service_accepts_none_and_rejects_any_existing_service() {
        assert!(ensure_no_active_service(None).is_ok());
        let session = ServiceSession::start("Sunday Morning");
        assert!(matches!(
            ensure_no_active_service(Some(&session)),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn ensure_no_active_service_rejects_even_a_paused_or_ended_session() {
        // `active_service` is cleared to `None` only by `end_service` - a
        // session found in `Some(..)` here is "already active" regardless
        // of its own internal status.
        let mut paused = ServiceSession::start("Sunday Morning");
        paused.pause();
        assert!(ensure_no_active_service(Some(&paused)).is_err());
    }

    #[test]
    fn ensure_service_status_pause_only_valid_from_started() {
        let started = ServiceSession::start("Sunday Morning");
        assert!(ensure_service_status(&started, ServiceStatus::Started, "pause").is_ok());

        let mut paused = started.clone();
        paused.pause();
        assert!(matches!(
            ensure_service_status(&paused, ServiceStatus::Started, "pause"),
            Err(AppError::InvalidInput(_))
        ));

        let mut ended = started;
        ended.end();
        assert!(ensure_service_status(&ended, ServiceStatus::Started, "pause").is_err());
    }

    #[test]
    fn ensure_service_status_resume_only_valid_from_paused() {
        let mut paused = ServiceSession::start("Sunday Morning");
        paused.pause();
        assert!(ensure_service_status(&paused, ServiceStatus::Paused, "resume").is_ok());

        let started = ServiceSession::start("Sunday Morning");
        assert!(matches!(
            ensure_service_status(&started, ServiceStatus::Paused, "resume"),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn ensure_suggestion_editable_allows_pending_and_edited_only() {
        assert!(ensure_suggestion_editable(SuggestionStatus::Pending, "approve").is_ok());
        assert!(ensure_suggestion_editable(SuggestionStatus::Edited, "approve").is_ok());
        assert!(matches!(
            ensure_suggestion_editable(SuggestionStatus::Approved, "approve"),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            ensure_suggestion_editable(SuggestionStatus::Rejected, "reject"),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn ensure_suggestion_previewable_allows_everything_but_rejected() {
        assert!(ensure_suggestion_previewable(SuggestionStatus::Pending).is_ok());
        assert!(ensure_suggestion_previewable(SuggestionStatus::Edited).is_ok());
        assert!(ensure_suggestion_previewable(SuggestionStatus::Approved).is_ok());
        assert!(matches!(
            ensure_suggestion_previewable(SuggestionStatus::Rejected),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn parse_uuid_accepts_a_valid_uuid_and_rejects_garbage() {
        let id = Uuid::new_v4();
        assert_eq!(parse_uuid(&id.to_string()).unwrap(), id);
        assert!(matches!(
            parse_uuid("not-a-uuid"),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(parse_uuid(""), Err(AppError::InvalidInput(_))));
    }

    #[test]
    fn parse_display_screen_accepts_all_three_known_ids_and_rejects_anything_else() {
        assert_eq!(
            parse_display_screen("stage").unwrap(),
            presentation_display::DisplayScreen::Stage
        );
        assert_eq!(
            parse_display_screen("confidence").unwrap(),
            presentation_display::DisplayScreen::Confidence
        );
        assert_eq!(
            parse_display_screen("lobby").unwrap(),
            presentation_display::DisplayScreen::Lobby
        );
        assert!(matches!(
            parse_display_screen("projector"),
            Err(AppError::Presentation(
                presentation::PresentationError::UnknownDisplayScreen(_)
            ))
        ));
    }

    #[test]
    fn require_non_empty_trims_and_rejects_blank_input() {
        assert_eq!(
            require_non_empty("  Sunday Service  ", "title").unwrap(),
            "Sunday Service"
        );
        assert!(matches!(
            require_non_empty("", "title"),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            require_non_empty("   ", "title"),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn is_translation_selectable_fails_open_on_missing_or_errored_registry_lookups() {
        assert!(
            is_translation_selectable(Ok(None)),
            "no registry entry must never hide a translation"
        );
        let err = ContentRegistryError::Storage("boom".to_string());
        assert!(
            is_translation_selectable(Err(&err)),
            "a registry read error must never hide a translation"
        );
    }

    #[test]
    fn is_translation_selectable_hides_only_explicitly_disabled_content() {
        let enabled = ContentMetadata {
            id: "bible:KJV".to_string(),
            content_type: ContentType::Bible,
            name: "King James Version".to_string(),
            version: "1.0".to_string(),
            language: "en".to_string(),
            source: "test".to_string(),
            publisher: None,
            copyright: None,
            license: None,
            distribution: None,
            imported_at: chrono::Utc::now(),
            checksum: None,
            status: ContentStatus::Enabled,
            licensing_status: cip_core_content::LicensingStatus::Unknown,
            usage: cip_core_content::UsagePermissions::default(),
        };
        assert!(is_translation_selectable(Ok(Some(&enabled))));

        let disabled = ContentMetadata {
            status: ContentStatus::Disabled,
            ..enabled
        };
        assert!(!is_translation_selectable(Ok(Some(&disabled))));
    }

    /// Phase 3.7 root-cause regression: the exact bug behind "Diagnostics
    /// says BSB installed, but the Bible Library says no Bible content" -
    /// see `resolve_default_translation_id`'s docs and
    /// docs/phase-3-7-offline-operator-test.md section 4/5. In a real
    /// production build (no dev seed - see `lib.rs`'s
    /// `environment != Production` guard), only `bible:BSB` is ever
    /// registered, never `bible:KJV`. Before this fix, every command that
    /// fell back to the literal `DEFAULT_TRANSLATION_ID` ("KJV") on an
    /// omitted `translationId` silently queried a translation id with zero
    /// rows in a real production database.
    #[test]
    fn resolve_default_translation_id_prefers_bsb_when_registered_like_a_real_production_build() {
        use cip_core_content::ContentRegistry as _;
        use cip_database::{open_in_memory, run_migrations};
        use cip_integrations_content::SqliteContentRegistry;

        // A "production-like" registry: only BSB registered, exactly as a
        // real Windows release build leaves it (dev seed never applied).
        let registry = SqliteContentRegistry::new(open_in_memory_migrated());
        registry
            .register(&ContentMetadata {
                id: content::bible_content_id(crate::bible_production_dataset::BSB_TRANSLATION_ID),
                content_type: ContentType::Bible,
                name: "Berean Standard Bible".to_string(),
                version: "bsb-1.0".to_string(),
                language: "en".to_string(),
                source: "production".to_string(),
                publisher: None,
                copyright: None,
                license: None,
                distribution: None,
                imported_at: chrono::Utc::now(),
                checksum: None,
                status: ContentStatus::Enabled,
                licensing_status: cip_core_content::LicensingStatus::VerifiedPublicDomain,
                usage: cip_core_content::UsagePermissions::default(),
            })
            .unwrap();

        assert_eq!(
            resolve_default_translation_id_from_registry(&registry),
            crate::bible_production_dataset::BSB_TRANSLATION_ID,
            "with BSB registered (a real production build), the default must resolve to BSB, never the KJV dev-fixture id"
        );

        fn open_in_memory_migrated() -> rusqlite::Connection {
            let mut conn = open_in_memory().unwrap();
            run_migrations(&mut conn).unwrap();
            conn
        }
    }

    #[test]
    fn resolve_default_translation_id_falls_back_to_the_dev_fixture_when_bsb_is_not_registered() {
        use cip_database::{open_in_memory, run_migrations};
        use cip_integrations_content::SqliteContentRegistry;

        // A "dev-environment-like" registry: nothing registered at all
        // (or only KJV, from apply_dev_seed) - the pre-existing
        // dev/test behavior must be unchanged by this fix.
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        let registry = SqliteContentRegistry::new(conn);

        assert_eq!(
            resolve_default_translation_id_from_registry(&registry),
            DEFAULT_TRANSLATION_ID,
            "with no BSB registration at all, the default must still fall back to the dev fixture id, matching every existing dev/test workflow"
        );
    }

    #[test]
    fn ensure_ai_processing_permitted_refuses_when_translation_is_not_registered_at_all() {
        use cip_database::{open_in_memory, run_migrations};
        use cip_integrations_content::SqliteContentRegistry;

        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        let registry = SqliteContentRegistry::new(conn);

        assert!(
            ensure_ai_processing_permitted(&registry, "BSB").is_err(),
            "an unregistered translation must never be allowed through the AI-processing gate"
        );
    }

    #[test]
    fn ensure_ai_processing_permitted_refuses_when_registered_but_permission_never_recorded() {
        use cip_core_content::ContentRegistry as _;
        use cip_database::{open_in_memory, run_migrations};
        use cip_integrations_content::SqliteContentRegistry;

        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        let registry = SqliteContentRegistry::new(conn);
        registry
            .register(&ContentMetadata {
                id: content::bible_content_id("BSB"),
                content_type: ContentType::Bible,
                name: "Berean Standard Bible".to_string(),
                version: "bsb-1.0".to_string(),
                language: "en".to_string(),
                source: "production".to_string(),
                publisher: None,
                copyright: None,
                license: None,
                distribution: None,
                imported_at: chrono::Utc::now(),
                checksum: None,
                status: ContentStatus::Enabled,
                licensing_status: cip_core_content::LicensingStatus::VerifiedPublicDomain,
                usage: cip_core_content::UsagePermissions::default(),
            })
            .unwrap();

        assert!(
            ensure_ai_processing_permitted(&registry, "BSB").is_err(),
            "VerifiedPublicDomain licensing alone must not imply ai_processing_allowed - only \
             an explicit usage permission does"
        );
    }

    #[test]
    fn ensure_ai_processing_permitted_succeeds_only_once_explicitly_granted() {
        use cip_core_content::ContentRegistry as _;
        use cip_database::{open_in_memory, run_migrations};
        use cip_integrations_content::SqliteContentRegistry;

        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        let registry = SqliteContentRegistry::new(conn);
        registry
            .register(&ContentMetadata {
                id: content::bible_content_id("BSB"),
                content_type: ContentType::Bible,
                name: "Berean Standard Bible".to_string(),
                version: "bsb-1.0".to_string(),
                language: "en".to_string(),
                source: "production".to_string(),
                publisher: None,
                copyright: None,
                license: None,
                distribution: None,
                imported_at: chrono::Utc::now(),
                checksum: None,
                status: ContentStatus::Enabled,
                licensing_status: cip_core_content::LicensingStatus::VerifiedPublicDomain,
                usage: cip_core_content::UsagePermissions {
                    ai_processing_allowed: Some(true),
                    ..Default::default()
                },
            })
            .unwrap();

        assert!(ensure_ai_processing_permitted(&registry, "BSB").is_ok());
    }

    #[test]
    fn parse_content_type_accepts_known_values_and_rejects_unknown() {
        assert_eq!(parse_content_type("bible").unwrap(), ContentType::Bible);
        assert_eq!(parse_content_type("music").unwrap(), ContentType::Music);
        assert_eq!(parse_content_type("service").unwrap(), ContentType::Service);
        assert_eq!(parse_content_type("media").unwrap(), ContentType::Media);
        assert_eq!(
            parse_content_type("reference").unwrap(),
            ContentType::Reference
        );
        assert!(matches!(
            parse_content_type("sermon"),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn parse_music_query_accepts_known_types_and_rejects_unknown() {
        assert_eq!(
            parse_music_query("title", "Amazing Grace".to_string()).unwrap(),
            MusicQuery::Title("Amazing Grace".to_string())
        );
        assert_eq!(
            parse_music_query("number", "120".to_string()).unwrap(),
            MusicQuery::Number("120".to_string())
        );
        assert_eq!(
            parse_music_query("lyric", "grace how sweet".to_string()).unwrap(),
            MusicQuery::Lyric("grace how sweet".to_string())
        );
        assert!(matches!(
            parse_music_query("acoustic", "hum a few bars".to_string()),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn parse_suggestion_status_accepts_known_values_and_rejects_unknown() {
        assert_eq!(
            parse_suggestion_status("pending").unwrap(),
            SuggestionStatus::Pending
        );
        assert_eq!(
            parse_suggestion_status("approved").unwrap(),
            SuggestionStatus::Approved
        );
        assert_eq!(
            parse_suggestion_status("edited").unwrap(),
            SuggestionStatus::Edited
        );
        assert_eq!(
            parse_suggestion_status("rejected").unwrap(),
            SuggestionStatus::Rejected
        );
        assert!(matches!(
            parse_suggestion_status("projected"),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn parse_display_reference_reverses_scripture_reference_display() {
        assert_eq!(
            parse_display_reference("ROM 8:28").unwrap(),
            ("ROM".to_string(), 8, 28)
        );
        assert!(parse_display_reference("garbage").is_err());
        assert!(parse_display_reference("ROM eight").is_err());
    }

    #[test]
    fn parse_display_reference_takes_the_start_of_a_verse_range() {
        assert_eq!(
            parse_display_reference("ROM 8:28-30").unwrap(),
            ("ROM".to_string(), 8, 28)
        );
    }

    /// Every DTO exposed over IPC must serialize camelCase, matching the
    /// convention every existing Phase 1.0/1.1 type already follows.
    #[test]
    fn live_status_serializes_camel_case() {
        let status = LiveStatus {
            service: None,
            service_status: LiveServiceStatus::Planned,
            audio: AudioEngineStatus {
                is_capturing: false,
                is_paused: false,
                sample_rate_hz: 0,
                input_level: None,
                stream_error: None,
                selected_device: None,
                channels: None,
            },
            audio_status: AudioStatusKind::Unavailable,
            audio_error_text: None,
            speech_status: SpeechStatusKind::Unavailable,
            speech_error_text: None,
            network_status: NetworkStatusKind::Offline,
            ai_status: AiStatusKind::Degraded,
            database_status: DatabaseStatusKind::Connected,
            acoustic_status: acoustic::AcousticEngineStatus {
                status: cip_core_music::AcousticRecognitionStatus::Unavailable,
                method: cip_core_music::AcousticRecognitionMethod::None,
                reason: Some("no acoustic recognizer configured".to_string()),
            },
            current_song: None,
            bible: None,
        };
        let value = serde_json::to_value(&status).unwrap();
        assert!(value.get("serviceStatus").is_some());
        assert!(value.get("audioStatus").is_some());
        assert!(value.get("audioErrorText").is_some());
        assert!(value.get("speechStatus").is_some());
        assert!(value.get("speechErrorText").is_some());
        assert!(value.get("networkStatus").is_some());
        assert!(value.get("aiStatus").is_some());
        assert!(value.get("databaseStatus").is_some());
        assert!(value.get("acousticStatus").is_some());
        assert!(value.get("currentSong").is_some());
        assert_eq!(value["serviceStatus"], "planned");
        assert_eq!(value["audio"]["isCapturing"], false);
        assert_eq!(value["acousticStatus"]["status"], "unavailable");
    }

    // --- Phase 6.4 buried audio/speech error visibility ---------------

    #[test]
    fn error_text_if_returns_none_when_not_an_error_even_with_stale_text() {
        assert_eq!(
            error_text_if(false, Some("stale, not yet cleared".to_string())),
            None
        );
    }

    #[test]
    fn error_text_if_returns_the_text_when_it_is_an_error() {
        assert_eq!(
            error_text_if(true, Some("no audio device available".to_string())),
            Some("no audio device available".to_string())
        );
    }

    #[test]
    fn error_text_if_returns_none_for_an_error_with_no_text_rather_than_fabricating_one() {
        assert_eq!(error_text_if(true, None), None);
    }

    // --- Phase 3.2 pilot hardware diagnostics -------------------------

    #[test]
    fn diagnose_whisper_model_reports_missing_for_a_nonexistent_path() {
        let diagnostic =
            diagnose_whisper_model(std::path::Path::new("/nonexistent/ggml-tiny.en.bin"));
        assert!(matches!(diagnostic, WhisperModelDiagnostic::Missing { .. }));
    }

    #[test]
    fn diagnose_whisper_model_reports_unreadable_for_a_directory() {
        let dir = std::env::temp_dir().join(format!(
            "cip-diagnose-whisper-dir-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let diagnostic = diagnose_whisper_model(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(
            diagnostic,
            WhisperModelDiagnostic::Unreadable { .. }
        ));
    }

    #[test]
    fn diagnose_whisper_model_reports_present_with_the_real_size_for_a_readable_file() {
        let path = std::env::temp_dir().join(format!(
            "cip-diagnose-whisper-present-test-{}.bin",
            std::process::id()
        ));
        std::fs::write(&path, b"not a real model, just a readable file").unwrap();
        let diagnostic = diagnose_whisper_model(&path);
        let _ = std::fs::remove_file(&path);
        match diagnostic {
            WhisperModelDiagnostic::Present { size_bytes, .. } => {
                assert_eq!(
                    size_bytes,
                    "not a real model, just a readable file".len() as u64
                );
            }
            other => panic!("expected Present, got {other:?}"),
        }
    }

    #[test]
    fn pilot_diagnostics_serializes_camel_case() {
        let diagnostics = PilotDiagnostics {
            machine: MachineDiagnostic {
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                cip_version: "0.1.0".to_string(),
                build_commit: "abc123def456".to_string(),
                build_dirty: false,
            },
            whisper_model: WhisperModelDiagnostic::Missing {
                expected_path: "/tmp/ggml-tiny.en.bin".to_string(),
            },
            speech: SpeechRuntimeDiagnostics {
                feature_compiled: false,
                model_load_attempted: false,
                model_loaded: false,
                model_load_error: None,
                engine_ready: false,
                chunks_received: 0,
                last_chunk_sample_rate_hz: None,
                last_chunk_sample_count: None,
                last_resampled_sample_count: None,
                chunks_skipped_engine_not_ready: 0,
                inferences_attempted: 0,
                inferences_succeeded: 0,
                last_error: None,
                queue_pending_ms: 0,
                queue_high_water_ms: 0,
                overload_events: 0,
                audio_ms_dropped_overload: 0,
                last_inference_duration_ms: None,
                max_inference_duration_ms: None,
                avg_inference_duration_ms: None,
                last_transcript_pipeline_duration_ms: None,
                overload_state: OverloadState::Normal,
                silent_windows_skipped: 0,
                non_speech_placeholders_skipped: 0,
            },
            audio_devices: Vec::new(),
            audio: AudioEngineStatus {
                is_capturing: false,
                is_paused: false,
                sample_rate_hz: 0,
                input_level: None,
                stream_error: None,
                selected_device: None,
                channels: None,
            },
            displays: vec![DisplayDiagnostic {
                name: Some("Virtual-1".to_string()),
                width_px: 1280,
                height_px: 800,
                position_x: 0,
                position_y: 0,
                scale_factor: 1.0,
                is_primary: true,
            }],
            bible: None,
            database: DatabaseDiagnostic {
                path: "/tmp/cip.sqlite3".to_string(),
                readable: true,
                writable: true,
            },
        };
        let value = serde_json::to_value(&diagnostics).unwrap();
        assert_eq!(value["machine"]["cipVersion"], "0.1.0");
        assert_eq!(value["machine"]["buildCommit"], "abc123def456");
        assert_eq!(value["whisperModel"]["status"], "missing");
        assert_eq!(
            value["whisperModel"]["expectedPath"],
            "/tmp/ggml-tiny.en.bin"
        );
        assert_eq!(value["displays"][0]["widthPx"], 1280);
        assert_eq!(value["displays"][0]["positionX"], 0);
        assert_eq!(value["displays"][0]["positionY"], 0);
        assert_eq!(value["displays"][0]["scaleFactor"], 1.0);
        assert_eq!(value["displays"][0]["isPrimary"], true);
        assert_eq!(value["audio"]["selectedDevice"], serde_json::Value::Null);
        assert_eq!(value["audio"]["channels"], serde_json::Value::Null);
        assert_eq!(value["database"]["writable"], true);
    }

    /// Phase 3.3: the machine identifier a real church-hardware evidence
    /// record depends on must actually be populated from the real build,
    /// not left as a placeholder - `env!("CIP_GIT_COMMIT")` is set by
    /// `build.rs` at compile time.
    #[test]
    fn cip_git_commit_is_embedded_and_not_the_literal_placeholder() {
        let commit = env!("CIP_GIT_COMMIT");
        assert!(!commit.is_empty());
        // Either a real short hash from this git checkout, or the
        // documented, honest fallback for a non-git build - never blank,
        // never a fabricated-looking value.
        assert!(commit == "unknown" || commit.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Phase 3.2 backup/recovery validation (spec section 18): create real
    /// data, back it up via the exact mechanism `backup_database` uses
    /// (`VACUUM INTO` through a live connection), corrupt/replace the
    /// *working copy* only (never a real operator database - this is a
    /// throwaway temp file created and destroyed entirely within this
    /// test), restore by copying the backup over it, reopen, and verify
    /// the data survived intact.
    #[test]
    fn a_vacuum_into_backup_survives_a_simulated_working_database_loss() {
        use cip_core_service::ServiceSession;
        use cip_database::{open, run_migrations};

        let dir =
            std::env::temp_dir().join(format!("cip-backup-restore-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let working_path = dir.join("cip.sqlite3");
        let backup_path = dir.join("cip-backup.sqlite3");

        // --- create real data in the "working" database -------------------
        let session_id;
        {
            let mut conn = open(&working_path).unwrap();
            run_migrations(&mut conn).unwrap();
            let session = ServiceSession::start("Backup Restore Test Service");
            session_id = session.id;
            crate::persistence::persist_service(&conn, &session).unwrap();

            // --- back it up exactly as `backup_database` does -------------
            conn.execute(
                "VACUUM INTO ?1",
                rusqlite::params![backup_path.to_string_lossy()],
            )
            .unwrap();
        }
        assert!(backup_path.exists(), "the backup file must actually exist");

        // --- simulate total loss of the working database -------------------
        std::fs::remove_file(&working_path).unwrap();
        for sidecar in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{sidecar}", working_path.display()));
        }
        assert!(
            !working_path.exists(),
            "the working database must genuinely be gone before restoring"
        );

        // --- restore: copy the backup over the (now-missing) working path -
        std::fs::copy(&backup_path, &working_path).unwrap();

        // --- reopen exactly as a fresh CIP launch would, and verify --------
        let restored = open(&working_path).unwrap();
        let reloaded = crate::persistence::get_service(&restored, session_id).unwrap();
        assert_eq!(reloaded.title, "Backup Restore Test Service");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn domain_capability_report_serializes_camel_case_with_null_engine_for_unregistered_domains() {
        let registered = DomainCapabilityReport {
            domain: IntelligenceDomain::Bible,
            capability: EngineCapability::Available,
            engine_id: Some("bible".to_string()),
            engine_version: Some("1.0".to_string()),
        };
        let unregistered = DomainCapabilityReport {
            domain: IntelligenceDomain::Music,
            capability: EngineCapability::Unavailable,
            engine_id: None,
            engine_version: None,
        };
        let registered_json = serde_json::to_value(&registered).unwrap();
        let unregistered_json = serde_json::to_value(&unregistered).unwrap();
        assert_eq!(registered_json["engineId"], "bible");
        assert_eq!(registered_json["capability"], "available");
        assert!(unregistered_json["engineId"].is_null());
        assert_eq!(unregistered_json["capability"], "unavailable");
    }

    // --- sermon foundation guards (Phase 2.5, per the authoritative Phase
    // 2 roadmap) - mirrors the Phase 1.3 service guard tests above.

    #[test]
    fn ensure_no_active_sermon_accepts_none_and_rejects_any_existing_sermon() {
        assert!(ensure_no_active_sermon(None).is_ok());
        let sermon = Sermon::start(Uuid::new_v4(), None);
        assert!(matches!(
            ensure_no_active_sermon(Some(&sermon)),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn ensure_no_active_sermon_rejects_even_a_paused_sermon() {
        let mut sermon = Sermon::start(Uuid::new_v4(), None);
        sermon.pause();
        assert!(ensure_no_active_sermon(Some(&sermon)).is_err());
    }

    #[test]
    fn ensure_valid_sermon_transition_accepts_every_documented_transition() {
        assert!(ensure_valid_sermon_transition(SermonStatus::Active, SermonStatus::Paused).is_ok());
        assert!(ensure_valid_sermon_transition(SermonStatus::Paused, SermonStatus::Active).is_ok());
        assert!(ensure_valid_sermon_transition(SermonStatus::Active, SermonStatus::Ended).is_ok());
    }

    #[test]
    fn ensure_valid_sermon_transition_rejects_ended_to_active() {
        assert!(matches!(
            ensure_valid_sermon_transition(SermonStatus::Ended, SermonStatus::Active),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn ensure_valid_sermon_transition_rejects_a_same_state_call() {
        assert!(
            ensure_valid_sermon_transition(SermonStatus::Active, SermonStatus::Active).is_err()
        );
    }

    #[test]
    fn parse_speaker_role_input_accepts_known_roles_case_insensitively_and_rejects_garbage() {
        assert_eq!(
            parse_speaker_role_input("primary").unwrap(),
            SpeakerRole::Primary
        );
        assert_eq!(
            parse_speaker_role_input("GUEST").unwrap(),
            SpeakerRole::Guest
        );
        assert!(parse_speaker_role_input("keynote").is_err());
    }

    #[test]
    fn parse_section_kind_input_accepts_labels_case_insensitively_and_rejects_garbage() {
        assert_eq!(
            parse_section_kind_input("main_message").unwrap(),
            SermonSectionKind::MainMessage
        );
        assert_eq!(
            parse_section_kind_input("Altar Call").unwrap(),
            SermonSectionKind::AltarCall
        );
        assert_eq!(
            parse_section_kind_input("scripture-reading").unwrap(),
            SermonSectionKind::ScriptureReading
        );
        assert!(parse_section_kind_input("not-a-real-section").is_err());
    }

    #[test]
    fn sermon_foundation_summary_serializes_with_camel_case_fields() {
        let summary = SermonFoundationSummary {
            active_sermon: Some(Sermon::start(Uuid::new_v4(), Some("Grace".to_string()))),
            current_section: None,
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert!(json.get("activeSermon").is_some());
        assert!(json.get("currentSection").is_some());
    }
}
