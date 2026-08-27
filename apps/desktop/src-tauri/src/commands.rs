//! Tauri commands: the IPC surface the frontend calls.
//!
//! Every command validates its own input (empty strings, malformed ids)
//! before touching state, and returns `Result<T, AppError>` so failures
//! reach the frontend as a clear message rather than a panic - per the
//! "manual fallback" requirement, nothing here may crash the application.

use crate::acoustic;
use crate::config::AppConfig;
use crate::content;
use crate::errors::AppError;
use crate::events::{emit, AppEvent};
use crate::logging::LogCategory;
use crate::music;
use crate::persistence;
use crate::pipeline::handle_final_transcript;
use crate::presentation;
use crate::presentation_display;
use crate::sermon_foundation;
use crate::state::{AppState, DEFAULT_TRANSLATION_ID};
use crate::timeline::{self, TimelineEntry};
use cip_core_ai::{Suggestion, SuggestionKind, SuggestionStatus, TranscriptSegment};
use cip_core_bible::{
    check_bible_integrity, search_bible as dispatch_bible_search, BibleSearchResult,
    BibleTranslation, IntegrityReport, PartialScriptureReference, ReferenceKind, ScriptureContext,
    ScriptureContextManager, ScriptureReference,
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
use cip_presentation_renderer::{RenderedSlide, SCRIPTURE_DEFAULT_TEMPLATE};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::atomic::Ordering;
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PilotDiagnostics {
    pub machine: MachineDiagnostic,
    pub whisper_model: WhisperModelDiagnostic,
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
    };

    let whisper_model = diagnose_whisper_model(&state.config.whisper_model_path);

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

    if !state
        .speech_engine
        .lock()
        .expect("speech_engine mutex poisoned")
        .is_ready()
    {
        return Err(log_and_return(AppError::SpeechEngine(
            cip_core_ai::SpeechEngineError::NotInitialized,
        )));
    }

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

    let sink_app = app.clone();
    let sink: AudioChunkSink = Arc::new(move |chunk: AudioChunk| {
        let _ = acoustic_tx.try_send(chunk.clone());
        handle_audio_chunk(&sink_app, service_id, chunk);
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

    {
        let db = state.db.lock().expect("db connection poisoned");
        record_timeline(
            &db,
            Some(service_id),
            AppEvent::AudioStarted,
            LogCategory::Audio,
            serde_json::json!({ "deviceId": resolved_device_id }),
        );
        record_timeline(
            &db,
            Some(service_id),
            AppEvent::SpeechStarted,
            LogCategory::Speech,
            serde_json::json!({}),
        );
    }
    let _ = emit(
        &app,
        AppEvent::AudioStarted,
        serde_json::json!({ "deviceId": resolved_device_id }),
    );
    let _ = emit(&app, AppEvent::SpeechStarted, serde_json::json!({}));
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
            record_timeline(
                &db,
                Some(session.id),
                AppEvent::SpeechStopped,
                LogCategory::Speech,
                serde_json::json!({}),
            );
        }
    }
    let _ = emit(&app, AppEvent::AudioStopped, serde_json::json!({}));
    let _ = emit(&app, AppEvent::SpeechStopped, serde_json::json!({}));
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

/// Runs on the `AudioEngine`'s own capture thread (see
/// `integrations/audio`'s worker-thread design) - never on a Tauri command
/// thread. Re-fetches `AppState` from the cloned `AppHandle` rather than
/// capturing a `State<'_, AppState>` directly, since the latter's lifetime
/// is tied to a single command invocation and can't be captured into a
/// closure that outlives it.
fn handle_audio_chunk(app: &AppHandle, service_id: Uuid, chunk: AudioChunk) {
    let state = app.state::<AppState>();

    let segments = {
        let mut speech = state
            .speech_engine
            .lock()
            .expect("speech_engine mutex poisoned");
        match speech.feed_audio(&chunk.samples) {
            Ok(segments) => segments,
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

    for mut segment in segments {
        if !segment.is_final {
            let _ = emit(app, AppEvent::TranscriptUpdated, segment);
            continue;
        }

        segment.sequence = state.transcript_sequence.fetch_add(1, Ordering::SeqCst);
        let segment_for_event = segment.clone();

        let processed = {
            let db = state.db.lock().expect("db connection poisoned");
            let mut context = state
                .context_manager
                .lock()
                .expect("context_manager mutex poisoned");
            handle_final_transcript(
                &db,
                state.bible_provider.as_ref(),
                &mut context,
                service_id,
                DEFAULT_TRANSLATION_ID,
                segment,
            )
        };

        match processed {
            Ok(processed) => {
                // Phase 2.4: the one real signal `service::transcript_freshness`
                // reads - a genuine final segment from the live audio/
                // speech pipeline, never the manual/test-mode harnesses
                // (see `AppState::last_transcript_at`'s own docs).
                *state
                    .last_transcript_at
                    .lock()
                    .expect("last_transcript_at mutex poisoned") = Some(chrono::Utc::now());
                let _ = emit(app, AppEvent::TranscriptUpdated, segment_for_event);
                let db = state.db.lock().expect("db connection poisoned");
                emit_processed_segment_events(app, &db, service_id, &processed);
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
        handle_final_transcript(
            &db,
            state.bible_provider.as_ref(),
            &mut context,
            service_id,
            DEFAULT_TRANSLATION_ID,
            segment,
        )
        .map_err(AppError::from)
        .map_err(log_and_return)?
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
    let reference = ScriptureReference::single(DEFAULT_TRANSLATION_ID, &book, chapter, verse);
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
    let translation_id = translation_id.unwrap_or_else(|| DEFAULT_TRANSLATION_ID.to_string());
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
    let translation_id = translation_id.unwrap_or_else(|| DEFAULT_TRANSLATION_ID.to_string());
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
    let translation_id = translation_id.unwrap_or_else(|| DEFAULT_TRANSLATION_ID.to_string());
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
    let translation_id = translation_id.unwrap_or_else(|| DEFAULT_TRANSLATION_ID.to_string());
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

/// Opens (or, if already open, focuses) the presentation display window -
/// useful on its own for positioning it on a projector/second monitor
/// before anything is ready to show, and called automatically by
/// `display_presentation` when needed.
#[tauri::command]
pub fn open_presentation_display(
    app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<(), AppError> {
    presentation_display::open_display_window(&app)
        .map_err(|e| {
            AppError::from(presentation::PresentationError::DisplayUnavailable(
                e.to_string(),
            ))
        })
        .map_err(log_and_return)
}

/// Whether the display window currently exists, and which item (if any) is
/// currently `Active` for the active service - the operator UI's sync
/// point on mount, never assumed from local state alone.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationDisplayState {
    pub window_open: bool,
    pub active_item: Option<PresentationItem>,
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
    Ok(PresentationDisplayState {
        window_open: presentation_display::is_display_window_open(&app),
        active_item,
    })
}

/// Displays a still-`Prepared` item for real: renders it, opens the display
/// window if needed, and only then commits `Prepared -> Active` - never
/// the other way around (spec section 8/28: an item is never marked
/// `Active` before the real display operation has actually succeeded, and
/// nothing but this explicit operator action may cross that boundary).
#[tauri::command]
pub fn display_presentation(
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

    presentation_display::open_display_window(&app)
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

    let _ = emit(
        &app,
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
        let _ = emit(app, AppEvent::PresentationStopped, item.clone());
    }
    Ok(stopped)
}

/// Closes the presentation display window outright (as opposed to
/// `clear_presentation_display`, which blanks it but leaves it open) -
/// stops any active item first via the same `Destroyed`-event
/// reconciliation a manual close triggers, so this and a manual close
/// always leave identical state.
#[tauri::command]
pub fn close_presentation_display(
    app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<(), AppError> {
    presentation_display::close_display_window(&app)
        .map_err(|e| {
            AppError::from(presentation::PresentationError::DisplayUnavailable(
                e.to_string(),
            ))
        })
        .map_err(log_and_return)
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

    let reference = ScriptureReference::single(DEFAULT_TRANSLATION_ID, &book, chapter, verse);
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
    persistence::persist_scripture_detection(
        &db,
        service_id,
        None,
        DEFAULT_TRANSLATION_ID,
        &detection,
    )
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
        .get_chapter(DEFAULT_TRANSLATION_ID, &book, chapter)
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
    let translation_id = translation_id.unwrap_or_else(|| DEFAULT_TRANSLATION_ID.to_string());
    ensure_translation_selectable(&state, &translation_id).map_err(log_and_return)?;
    dispatch_bible_search(state.bible_provider.as_ref(), &translation_id, &query)
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
    pub speech_status: SpeechStatusKind,
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
    let audio_status = if audio.is_capturing {
        AudioStatusKind::Listening
    } else if audio.stream_error.is_some()
        || state
            .audio_error
            .lock()
            .expect("audio_error mutex poisoned")
            .is_some()
    {
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

    let speech_ready = state
        .speech_engine
        .lock()
        .expect("speech_engine mutex poisoned")
        .is_ready();
    let speech_status = if state
        .speech_error
        .lock()
        .expect("speech_error mutex poisoned")
        .is_some()
    {
        SpeechStatusKind::Error
    } else if speech_ready {
        SpeechStatusKind::Ready
    } else {
        SpeechStatusKind::Unavailable
    };

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
        speech_status,
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
        };
        assert!(is_translation_selectable(Ok(Some(&enabled))));

        let disabled = ContentMetadata {
            status: ContentStatus::Disabled,
            ..enabled
        };
        assert!(!is_translation_selectable(Ok(Some(&disabled))));
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
            speech_status: SpeechStatusKind::Unavailable,
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
        assert!(value.get("speechStatus").is_some());
        assert!(value.get("networkStatus").is_some());
        assert!(value.get("aiStatus").is_some());
        assert!(value.get("databaseStatus").is_some());
        assert!(value.get("acousticStatus").is_some());
        assert!(value.get("currentSong").is_some());
        assert_eq!(value["serviceStatus"], "planned");
        assert_eq!(value["audio"]["isCapturing"], false);
        assert_eq!(value["acousticStatus"]["status"], "unavailable");
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
            },
            whisper_model: WhisperModelDiagnostic::Missing {
                expected_path: "/tmp/ggml-tiny.en.bin".to_string(),
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
