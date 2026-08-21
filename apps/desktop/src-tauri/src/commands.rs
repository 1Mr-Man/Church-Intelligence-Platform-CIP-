//! Tauri commands: the IPC surface the frontend calls.
//!
//! Every command validates its own input (empty strings, malformed ids)
//! before touching state, and returns `Result<T, AppError>` so failures
//! reach the frontend as a clear message rather than a panic - per the
//! "manual fallback" requirement, nothing here may crash the application.

use crate::config::AppConfig;
use crate::errors::AppError;
use crate::events::{emit, AppEvent};
use crate::logging::LogCategory;
use crate::persistence;
use crate::pipeline::handle_final_transcript;
use crate::presentation;
use crate::state::{AppState, DEFAULT_TRANSLATION_ID};
use crate::timeline::{self, TimelineEntry};
use cip_core_ai::{Suggestion, SuggestionKind, SuggestionStatus, TranscriptSegment};
use cip_core_bible::{
    BibleTranslation, BibleVerse, PartialScriptureReference, ReferenceKind, ScriptureContext,
    ScriptureContextManager, ScriptureReference,
};
use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use cip_core_presentation::{PresentationContent, PresentationItem, PresentationItemStatus};
use cip_core_service::{
    AudioChunk, AudioChunkSink, AudioDevice, AudioEngineStatus, ScriptureDetection, ServiceSession,
    ServiceStatus,
};
use cip_presentation_renderer::{RenderedSlide, SCRIPTURE_DEFAULT_TEMPLATE};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
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

#[tauri::command]
pub fn list_bible_translations(
    state: State<'_, AppState>,
) -> Result<Vec<BibleTranslation>, AppError> {
    state
        .bible_provider
        .list_translations()
        .map_err(AppError::from)
        .map_err(log_and_return)
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

    let sink_app = app.clone();
    let sink: AudioChunkSink = Arc::new(move |chunk: AudioChunk| {
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

fn preview_reference(
    reference: &str,
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<PresentationPreview, AppError> {
    let (content, slide) = presentation::build_scripture_slide(
        state.bible_provider.as_ref(),
        DEFAULT_TRANSLATION_ID,
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
/// prepare command directly.
#[tauri::command]
pub fn preview_presentation(
    suggestion_id: String,
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
    preview_reference(reference, &app, &state)
}

/// Previews an arbitrary scripture reference with no suggestion involved -
/// the manual Bible search path's preview (section 5/20: manual creation
/// must work independently of speech recognition).
#[tauri::command]
pub fn preview_scripture(
    reference: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PresentationPreview, AppError> {
    let reference = require_non_empty(&reference, "reference").map_err(log_and_return)?;
    preview_reference(&reference, &app, &state)
}

/// Prepares (never projects) a presentation item from an approved
/// suggestion. There is no "active"/projected state anywhere in this
/// command - see `docs/live-speech.md`'s "no automatic projection"
/// section.
#[tauri::command]
pub fn prepare_presentation(
    suggestion_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PresentationItem, AppError> {
    let id = parse_uuid(&suggestion_id).map_err(log_and_return)?;
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
        DEFAULT_TRANSLATION_ID,
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
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PresentationItem, AppError> {
    let reference = require_non_empty(&reference, "reference").map_err(log_and_return)?;
    let service_id = current_service_id(&state).map_err(log_and_return)?;

    let (content, _slide) = presentation::build_scripture_slide(
        state.bible_provider.as_ref(),
        DEFAULT_TRANSLATION_ID,
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

// --- manual Bible search (works with no audio/speech/network) -------------

#[tauri::command]
pub fn search_bible(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<BibleVerse>, AppError> {
    let query = require_non_empty(&query, "query").map_err(log_and_return)?;
    state
        .bible_provider
        .search(&query, DEFAULT_TRANSLATION_ID)
        .map_err(AppError::from)
        .map_err(log_and_return)
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
    } else if state
        .audio_error
        .lock()
        .expect("audio_error mutex poisoned")
        .is_some()
    {
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

    LiveStatus {
        service,
        service_status,
        audio,
        audio_status,
        speech_status,
        network_status,
        ai_status,
        database_status,
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
            },
            audio_status: AudioStatusKind::Unavailable,
            speech_status: SpeechStatusKind::Unavailable,
            network_status: NetworkStatusKind::Offline,
            ai_status: AiStatusKind::Degraded,
            database_status: DatabaseStatusKind::Connected,
        };
        let value = serde_json::to_value(&status).unwrap();
        assert!(value.get("serviceStatus").is_some());
        assert!(value.get("audioStatus").is_some());
        assert!(value.get("speechStatus").is_some());
        assert!(value.get("networkStatus").is_some());
        assert!(value.get("aiStatus").is_some());
        assert!(value.get("databaseStatus").is_some());
        assert_eq!(value["serviceStatus"], "planned");
        assert_eq!(value["audio"]["isCapturing"], false);
    }
}
