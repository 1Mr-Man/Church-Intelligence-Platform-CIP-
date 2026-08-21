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
use crate::state::{AppState, DEFAULT_TRANSLATION_ID};
use cip_core_ai::{Suggestion, SuggestionKind, SuggestionStatus, TranscriptSegment};
use cip_core_bible::{BibleTranslation, BibleVerse, ReferenceKind, ScriptureReference};
use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use cip_core_presentation::{PresentationContent, PresentationItem};
use cip_core_service::{
    AudioChunk, AudioChunkSink, AudioDevice, AudioEngineStatus, ServiceSession, ServiceStatus,
};
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

fn current_service_id(state: &State<'_, AppState>) -> Result<Uuid, AppError> {
    state
        .active_service
        .lock()
        .expect("active_service mutex poisoned")
        .as_ref()
        .map(|s| s.id)
        .ok_or(AppError::NoActiveService)
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

#[tauri::command]
pub fn start_service(
    title: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ServiceSession, AppError> {
    let title = require_non_empty(&title, "title").map_err(log_and_return)?;
    let session = ServiceSession::start(title);

    {
        let db = state.db.lock().expect("db connection poisoned");
        persistence::persist_service(&db, &session)
            .map_err(AppError::from)
            .map_err(log_and_return)?;
    }
    *state
        .active_service
        .lock()
        .expect("active_service mutex poisoned") = Some(session.clone());

    let _ = emit(&app, AppEvent::ServiceStarted, session.clone());
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

    state
        .audio_engine
        .lock()
        .expect("audio_engine mutex poisoned")
        .start(&resolved_device_id, sink)
        .map_err(AppError::from)
        .map_err(log_and_return)?;

    let _ = emit(
        &app,
        AppEvent::AudioStarted,
        serde_json::json!({ "deviceId": resolved_device_id }),
    );
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
    let _ = emit(&app, AppEvent::AudioStopped, serde_json::json!({}));
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
                return;
            }
        }
    };

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
                emit_processed_segment_events(app, &processed);
            }
            Err(e) => {
                log::error!(target: LogCategory::Database.target(), "failed to persist transcript segment: {e}");
            }
        }
    }
}

fn emit_processed_segment_events(app: &AppHandle, processed: &cip_core_service::ProcessedSegment) {
    for detection in &processed.detections {
        let event = match detection.kind {
            ReferenceKind::Unresolved => continue, // too frequent/noisy to be useful as an event
            ReferenceKind::Sequential => AppEvent::ScriptureUpdated,
            _ => AppEvent::ScriptureDetected,
        };
        let _ = emit(app, event, detection.clone());
    }
    for suggestion in &processed.suggestions {
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

    emit_processed_segment_events(&app, &processed);
    Ok(processed)
}

// --- transcript & suggestions -----------------------------------------------

#[tauri::command]
pub fn list_transcript(
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<TranscriptSegment>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
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
    state: State<'_, AppState>,
) -> Result<Vec<Suggestion>, AppError> {
    let service_id = current_service_id(&state).map_err(log_and_return)?;
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
    if !matches!(
        current.status,
        SuggestionStatus::Pending | SuggestionStatus::Edited
    ) {
        return Err(log_and_return(AppError::InvalidInput(format!(
            "cannot approve a suggestion with status {:?}",
            current.status
        ))));
    }
    let updated = persistence::update_suggestion_status(&db, id, SuggestionStatus::Approved, None)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    drop(db);
    let _ = emit(&app, AppEvent::SuggestionApproved, updated.clone());
    Ok(updated)
}

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
    let kind = SuggestionKind::Scripture {
        reference: new_reference,
    };

    let db = state.db.lock().expect("db connection poisoned");
    let updated =
        persistence::update_suggestion_status(&db, id, SuggestionStatus::Edited, Some(&kind))
            .map_err(AppError::from)
            .map_err(log_and_return)?;
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
    let updated = persistence::update_suggestion_status(&db, id, SuggestionStatus::Rejected, None)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
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

    if suggestion.status != SuggestionStatus::Approved {
        return Err(log_and_return(AppError::InvalidInput(
            "only an approved suggestion can be prepared for presentation".to_string(),
        )));
    }
    let SuggestionKind::Scripture { reference } = &suggestion.kind else {
        return Err(log_and_return(AppError::InvalidInput(
            "suggestion is not a scripture reference".to_string(),
        )));
    };

    let (book, chapter, verse) = parse_display_reference(reference).map_err(log_and_return)?;
    let scripture_reference =
        ScriptureReference::single(DEFAULT_TRANSLATION_ID, &book, chapter, verse);
    let verse_row = state
        .bible_provider
        .get_verse(&scripture_reference)
        .map_err(AppError::from)
        .map_err(log_and_return)?
        .ok_or_else(|| {
            log_and_return(AppError::InvalidInput(format!(
                "verse not found: {reference}"
            )))
        })?;

    let item = PresentationItem::prepare(
        suggestion.service_id,
        PresentationContent::Scripture {
            reference: reference.clone(),
            translation_id: DEFAULT_TRANSLATION_ID.to_string(),
            text: verse_row.text,
        },
    );
    persistence::persist_presentation_item(&db, &item)
        .map_err(AppError::from)
        .map_err(log_and_return)?;
    drop(db);

    let _ = emit(&app, AppEvent::PresentationPrepared, item.clone());
    Ok(item)
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechStatusKind {
    Unavailable,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkStatusKind {
    Offline,
    Online,
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
    let speech_status = if speech_ready {
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

    LiveStatus {
        service,
        service_status,
        audio,
        audio_status,
        speech_status,
        network_status,
        ai_status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- input validation: the part of each command worth testing in
    // isolation from the full Tauri IPC harness - see docs/live-speech.md's
    // testing section for why command tests stop here rather than
    // standing up `tauri::test::mock_builder()`. Persisted/pipeline
    // behavior (what these commands call into) is covered end to end in
    // `persistence.rs` and `pipeline.rs`.

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
        };
        let value = serde_json::to_value(&status).unwrap();
        assert!(value.get("serviceStatus").is_some());
        assert!(value.get("audioStatus").is_some());
        assert!(value.get("speechStatus").is_some());
        assert!(value.get("networkStatus").is_some());
        assert!(value.get("aiStatus").is_some());
        assert_eq!(value["serviceStatus"], "planned");
        assert_eq!(value["audio"]["isCapturing"], false);
    }
}
