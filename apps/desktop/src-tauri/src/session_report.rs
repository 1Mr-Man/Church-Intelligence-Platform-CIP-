//! Phase 25 (Session Black Box): a single downloadable, self-contained
//! record of everything CIP observed during one service, for an operator
//! to hand back alongside their own transcript/notes after a real test
//! run - "I tested it for an hour, here's the transcript and the report
//! from CIP" - so a diagnosis never has to guess at what actually
//! happened from a partial description.
//!
//! Mirrors `service_report.rs`'s own discipline: **no new detection
//! logic, no new persistence** - every field is either the existing
//! [`crate::service_report::ServiceReport`] wholesale, or a full (not
//! summarized) read of a table `service_report.rs` already counts
//! (`transcript_segments`, `audit_events`, `ai_suggestions`,
//! `transcript_corrections`). The one genuinely new thing this module
//! adds is `human_summary`: a plain-text paragraph composed entirely from
//! data already gathered, meant to be pasted directly into a chat message
//! without also attaching the full JSON.
//!
//! Deliberately holds no dependency on `AppHandle` or any live Tauri
//! API (unlike `commands::get_pilot_diagnostics`, which needs one for
//! display/monitor enumeration) - every field here comes from either the
//! database (via `conn`) or an already-cloned diagnostics snapshot the
//! caller passes in. That keeps `build_session_report` itself directly
//! unit-testable with a real in-memory database, the same testing
//! boundary `service_report.rs`'s own tests already establish - see
//! `commands.rs`'s own test-module docs for why a command that *does*
//! need `AppHandle` (the thin `export_session_report` wrapper around this
//! function) is not itself unit tested.

use crate::persistence::{self, PersistError, TranscriptCorrection};
use crate::service_report::{self, ServiceReport};
use crate::state::{EmbeddingDiagnostics, SpeechDiagnostics, SpeechQualityDiagnostics};
use crate::timeline::{self, TimelineEntry};
use chrono::{DateTime, Utc};
use cip_core_ai::{Suggestion, TranscriptSegment};
use rusqlite::Connection;
use serde::Serialize;
use uuid::Uuid;

/// The complete Phase 25 session export for one service - every field is
/// data this application already had (see module docs); nothing here is
/// generated, inferred, or scored.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReport {
    pub service_id: Uuid,
    pub exported_at: DateTime<Utc>,
    pub cip_version: String,
    pub build_commit: String,
    /// The existing Phase 5.1 aggregation, embedded wholesale - see
    /// `ServiceReport`'s own docs for what its `live_diagnostics` half
    /// does and does not represent (process-lifetime, not service-scoped).
    pub summary: ServiceReport,
    /// Every transcript segment this service produced, oldest first -
    /// unlike `list_transcript`'s own operator-facing `limit` parameter,
    /// this is deliberately the complete history: a black box that only
    /// showed the most recent N lines would defeat its own purpose.
    pub transcript: Vec<TranscriptSegment>,
    /// The complete service timeline (every `audit_events` row), oldest
    /// first - includes every error CIP recorded during this service, not
    /// just the single most-recent one `live_diagnostics` carries.
    pub timeline: Vec<TimelineEntry>,
    pub suggestions: Vec<Suggestion>,
    /// Every quality-tier correction (Phase 24.3), oldest first, each
    /// carrying both the original and corrected text.
    pub corrections: Vec<TranscriptCorrection>,
    /// A plain-text paragraph, composed entirely from the fields above -
    /// meant to be pasted directly into a chat message. See
    /// `build_human_summary`'s own docs.
    pub human_summary: String,
}

/// Pure aggregation: reads back every already-persisted row for
/// `service_id` and the diagnostics snapshots the caller already holds,
/// and assembles them. Never writes anything - see module docs for why
/// this takes plain diagnostics structs rather than computing them
/// itself.
#[allow(clippy::too_many_arguments)]
pub fn build_session_report(
    conn: &Connection,
    service_id: Uuid,
    speech_diagnostics: &SpeechDiagnostics,
    speech_quality_diagnostics: &SpeechQualityDiagnostics,
    embedding_diagnostics: &EmbeddingDiagnostics,
    embedding_ready: bool,
    audio_error: Option<String>,
    cip_version: String,
    build_commit: String,
) -> Result<SessionReport, PersistError> {
    let summary = service_report::build_service_report(
        conn,
        service_id,
        speech_diagnostics,
        speech_quality_diagnostics,
        embedding_diagnostics,
        embedding_ready,
        audio_error,
    )?;
    let transcript = persistence::list_transcript_segments(conn, service_id, u32::MAX)?;
    let timeline = timeline::list_timeline(conn, service_id, u32::MAX)?;
    let suggestions = persistence::list_suggestions(conn, service_id, None)?;
    let corrections = persistence::list_transcript_corrections(conn, service_id)?;

    let human_summary = build_human_summary(&summary, &transcript, &timeline, &corrections);

    Ok(SessionReport {
        service_id,
        exported_at: Utc::now(),
        cip_version,
        build_commit,
        summary,
        transcript,
        timeline,
        suggestions,
        corrections,
        human_summary,
    })
}

/// Composes a short, plain-text paragraph from already-gathered data - a
/// pure function (no I/O) so it is directly unit-testable without a
/// database, the same style `commands::classify_overload`/
/// `classify_quality_backlog` already establish for pure derivations.
/// Deliberately never claims more than the data actually shows: an empty
/// transcript says so plainly rather than being silently omitted.
pub fn build_human_summary(
    summary: &ServiceReport,
    transcript: &[TranscriptSegment],
    timeline: &[TimelineEntry],
    corrections: &[TranscriptCorrection],
) -> String {
    let mut lines = Vec::new();

    let duration = match summary.duration_minutes {
        Some(minutes) => format!("{minutes:.1} minutes"),
        None => "still active (no end time recorded)".to_string(),
    };
    lines.push(format!(
        "Service \"{}\" - duration: {duration}, {} transcript segment(s) captured.",
        summary.service.title,
        transcript.len()
    ));

    let s = &summary.suggestion_stats;
    lines.push(format!(
        "Suggestions: {} total ({} approved, {} edited, {} rejected, {} still pending).",
        s.total, s.approved, s.edited, s.rejected, s.pending
    ));
    if s.rejection_echoes > 0 {
        lines.push(format!(
            "{} previously-rejected reference(s) were detected again and silently kept suppressed.",
            s.rejection_echoes
        ));
    }

    if !summary.detection_kind_counts.is_empty() {
        let kinds = summary
            .detection_kind_counts
            .iter()
            .map(|c| format!("{}: {}", c.kind, c.count))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("Detections by kind: {kinds}."));
    }

    let error_count = timeline.iter().filter(|e| e.category == "error").count();
    if error_count > 0 {
        lines.push(format!(
            "{error_count} error(s) were recorded in the timeline during this service - see the \"timeline\" \
             field for each one's own message and timestamp."
        ));
    } else {
        lines.push("No errors were recorded in the timeline during this service.".to_string());
    }

    if !corrections.is_empty() {
        lines.push(format!(
            "The quality tier produced {} correction(s) to the live transcript.",
            corrections.len()
        ));
    }

    let ld = &summary.live_diagnostics;
    lines.push(format!(
        "Live pipeline (since app launch, not this service alone): speech model {}, {}/{} inferences succeeded{}.",
        if ld.speech_model_loaded { "loaded" } else { "not loaded" },
        ld.inferences_succeeded,
        ld.inferences_attempted,
        ld.avg_inference_duration_ms
            .map(|ms| format!(", avg {ms}ms"))
            .unwrap_or_default()
    ));
    if let Some(err) = &ld.speech_last_error {
        lines.push(format!("Last speech error: {err}"));
    }
    if ld.quality_feature_compiled {
        lines.push(format!(
            "Quality tier: model {}, {} job(s) submitted, {} dropped for backlog, {} completed.",
            if ld.quality_model_loaded {
                "loaded"
            } else {
                "not loaded"
            },
            ld.quality_jobs_submitted,
            ld.quality_jobs_dropped_backlog,
            ld.quality_jobs_completed
        ));
        if let Some(err) = &ld.quality_last_error {
            lines.push(format!("Last quality-tier error: {err}"));
        }
    }
    if let Some(err) = &ld.audio_last_error {
        lines.push(format!("Last audio error: {err}"));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_service::ServiceSession;
    use cip_database::{open_in_memory, run_migrations};

    fn open_test_db() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn persist_test_service(conn: &Connection) -> ServiceSession {
        let session = ServiceSession::start("Sunday Morning");
        persistence::persist_service(conn, &session).unwrap();
        session
    }

    fn sample_segment(text: &str, sequence: u64) -> TranscriptSegment {
        use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
        TranscriptSegment {
            id: Uuid::new_v4(),
            text: text.to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Model, None),
            start_ms: sequence * 1000,
            end_ms: sequence * 1000 + 900,
            sequence,
            language: None,
            speaker_id: None,
        }
    }

    #[test]
    fn build_session_report_bundles_the_full_transcript_and_timeline() {
        let conn = open_test_db();
        let service = persist_test_service(&conn);
        let seg = sample_segment("Turn to Romans chapter 8.", 0);
        persistence::persist_transcript_segment(&conn, service.id, &seg).unwrap();
        timeline::record_event(
            &conn,
            Some(service.id),
            crate::events::AppEvent::ServiceStarted,
            crate::logging::LogCategory::App,
            serde_json::json!({}),
        )
        .unwrap();

        let report = build_session_report(
            &conn,
            service.id,
            &SpeechDiagnostics::default(),
            &SpeechQualityDiagnostics::default(),
            &EmbeddingDiagnostics::default(),
            false,
            None,
            "0.1.0".to_string(),
            "abc123".to_string(),
        )
        .unwrap();

        assert_eq!(report.transcript.len(), 1);
        assert_eq!(report.transcript[0].text, "Turn to Romans chapter 8.");
        assert_eq!(report.timeline.len(), 1);
        assert_eq!(report.corrections.len(), 0);
        assert_eq!(report.cip_version, "0.1.0");
        assert!(report.human_summary.contains("Sunday Morning"));
    }

    #[test]
    fn build_session_report_includes_corrections_with_both_texts() {
        let conn = open_test_db();
        let service = persist_test_service(&conn);
        let original = sample_segment("Turn to Romans ate.", 0);
        let corrected = sample_segment("Turn to Romans eight.", 1);
        persistence::persist_transcript_segment(&conn, service.id, &original).unwrap();
        persistence::persist_transcript_segment(&conn, service.id, &corrected).unwrap();
        persistence::persist_transcript_correction(&conn, original.id, corrected.id).unwrap();

        let report = build_session_report(
            &conn,
            service.id,
            &SpeechDiagnostics::default(),
            &SpeechQualityDiagnostics::default(),
            &EmbeddingDiagnostics::default(),
            false,
            None,
            "0.1.0".to_string(),
            "abc123".to_string(),
        )
        .unwrap();

        assert_eq!(report.corrections.len(), 1);
        assert_eq!(report.corrections[0].original_text, "Turn to Romans ate.");
        assert_eq!(
            report.corrections[0].corrected_text,
            "Turn to Romans eight."
        );
        assert!(report.human_summary.contains("1 correction"));
    }

    fn empty_summary(title: &str) -> ServiceReport {
        ServiceReport {
            service: cip_core_service::ServiceSession::start(title),
            duration_minutes: Some(42.0),
            suggestion_stats: service_report::SuggestionStats::default(),
            detection_kind_counts: Vec::new(),
            timeline_category_counts: Vec::new(),
            live_diagnostics: service_report::LiveDiagnosticsSnapshot {
                speech_feature_compiled: true,
                speech_model_loaded: true,
                chunks_received: 0,
                inferences_attempted: 10,
                inferences_succeeded: 9,
                last_inference_duration_ms: None,
                avg_inference_duration_ms: Some(120),
                max_inference_duration_ms: None,
                queue_high_water_ms: 0,
                overload_events: 0,
                audio_ms_dropped_overload: 0,
                last_transcript_pipeline_duration_ms: None,
                speech_last_error: None,
                audio_last_error: None,
                embedding_feature_compiled: false,
                embedding_model_loaded: false,
                embedding_ready: false,
                quality_feature_compiled: false,
                quality_model_loaded: false,
                quality_jobs_submitted: 0,
                quality_jobs_dropped_backlog: 0,
                quality_jobs_completed: 0,
                quality_consecutive_jobs_dropped: 0,
                quality_last_error: None,
            },
            generated_at: Utc::now(),
        }
    }

    #[test]
    fn human_summary_states_no_errors_plainly_when_the_timeline_has_none() {
        let summary = empty_summary("Evening Service");
        let text = build_human_summary(&summary, &[], &[], &[]);
        assert!(text.contains("No errors were recorded"));
    }

    #[test]
    fn human_summary_surfaces_error_count_from_the_timeline() {
        let summary = empty_summary("Evening Service");
        let timeline = vec![TimelineEntry {
            id: Uuid::new_v4(),
            service_id: None,
            event_name: "ERROR_OCCURRED".to_string(),
            category: "error".to_string(),
            payload: Some(serde_json::json!({ "error": "no audio device" })),
            created_at: Utc::now(),
        }];
        let text = build_human_summary(&summary, &[], &timeline, &[]);
        assert!(text.contains("1 error(s) were recorded"));
        assert!(!text.contains("No errors were recorded"));
    }

    #[test]
    fn human_summary_never_fabricates_a_duration_for_an_active_service() {
        let mut summary = empty_summary("Evening Service");
        summary.duration_minutes = None;
        let text = build_human_summary(&summary, &[], &[], &[]);
        assert!(text.contains("still active"));
    }
}
