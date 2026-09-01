//! Phase 5.1 (Post-Service Observability Report) - a single read-only
//! aggregation of data this application already captures during a live
//! service (suggestion outcomes, detection-kind counts, timeline/error
//! counts) plus a labeled snapshot of the live speech/embedding pipeline's
//! own already-tracked counters, assembled into one response.
//!
//! Mirrors `harvest.rs`'s own discipline exactly: **no new detection
//! logic**, no new AI, no new persistence. Everything here is either a
//! plain `COUNT(*) ... GROUP BY` over already-persisted rows
//! (`persistence::scripture_detection_kind_counts`,
//! `timeline::count_events_by_category`) or a read of `AppState`'s
//! already-existing `SpeechDiagnostics`/`EmbeddingDiagnostics` structs.
//!
//! ## Why the live-pipeline snapshot is honestly labeled "since app
//! launch," not "this service"
//!
//! `SpeechDiagnostics`/`EmbeddingDiagnostics` are process-lifetime
//! counters - they accumulate across every `start_listening` call in this
//! run of the application, and are never reset when one service ends and
//! another begins (see `state.rs`'s own docs on `AppState.speech_diagnostics`).
//! A report claiming "this service's average inference duration was Xms"
//! would misrepresent a single-process-lifetime figure as service-scoped
//! precision this codebase does not actually have - exactly the kind of
//! inflated software-only reading this project's own standing discipline
//! (see `docs/phase-3-2-hardware-pilot.md`'s Environment A/B/C distinction)
//! guards against. `LiveDiagnosticsSnapshot`'s field names and this
//! module's own doc comments say "since app launch" rather than implying
//! per-service precision that doesn't exist.

use crate::persistence::{self, PersistError};
use crate::state::{EmbeddingDiagnostics, SpeechDiagnostics};
use crate::timeline;
use chrono::{DateTime, Utc};
use cip_core_ai::SuggestionStatus;
use cip_core_service::ServiceSession;
use rusqlite::Connection;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionStats {
    pub total: u64,
    pub pending: u64,
    pub approved: u64,
    pub edited: u64,
    pub rejected: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionKindCount {
    /// A `ReferenceKind::label()` string (e.g. `"DIRECT_REFERENCE"`,
    /// `"SEMANTIC_REFERENCE"`) - see `scripture_detections.detection_type`.
    pub kind: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineCategoryCount {
    pub category: String,
    pub count: u64,
}

/// See this module's own doc comment for why every field here is
/// process-lifetime, not service-scoped.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveDiagnosticsSnapshot {
    pub speech_feature_compiled: bool,
    pub speech_model_loaded: bool,
    pub chunks_received: u64,
    pub inferences_attempted: u64,
    pub inferences_succeeded: u64,
    pub last_inference_duration_ms: Option<u64>,
    /// Derived from `inference_duration_ms_sum`/`inference_duration_samples`
    /// at read time, mirroring `commands::get_pilot_diagnostics`'s own exact
    /// derivation - never stored redundantly, never allowed to drift from it.
    pub avg_inference_duration_ms: Option<u64>,
    pub max_inference_duration_ms: Option<u64>,
    pub queue_high_water_ms: u64,
    pub overload_events: u64,
    pub audio_ms_dropped_overload: u64,
    pub last_transcript_pipeline_duration_ms: Option<u64>,
    pub embedding_feature_compiled: bool,
    pub embedding_model_loaded: bool,
    pub embedding_ready: bool,
}

/// The complete post-service report for one service - every field is data
/// this application already had; nothing is generated, inferred, or scored
/// by this module.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceReport {
    pub service: ServiceSession,
    /// `None` while the service is still active (no `ended_at` yet) -
    /// never a guessed/partial duration for an in-progress service.
    pub duration_minutes: Option<f64>,
    pub suggestion_stats: SuggestionStats,
    /// Ascending by kind label for a stable, deterministic order.
    pub detection_kind_counts: Vec<DetectionKindCount>,
    /// Ascending by category for a stable, deterministic order.
    pub timeline_category_counts: Vec<TimelineCategoryCount>,
    pub live_diagnostics: LiveDiagnosticsSnapshot,
    pub generated_at: DateTime<Utc>,
}

/// Pure aggregation: reads back already-persisted rows for `service_id`
/// and a snapshot of the live diagnostics structs already tracked in
/// `AppState`, and assembles them. Never writes anything.
pub fn build_service_report(
    conn: &Connection,
    service_id: Uuid,
    speech_diagnostics: &SpeechDiagnostics,
    embedding_diagnostics: &EmbeddingDiagnostics,
    embedding_ready: bool,
) -> Result<ServiceReport, PersistError> {
    let service = persistence::get_service(conn, service_id)?;

    let duration_minutes = service
        .ended_at
        .map(|ended_at| (ended_at - service.started_at).num_milliseconds() as f64 / 60_000.0);

    let suggestions = persistence::list_suggestions(conn, service_id, None)?;
    let mut suggestion_stats = SuggestionStats::default();
    for suggestion in &suggestions {
        suggestion_stats.total += 1;
        match suggestion.status {
            SuggestionStatus::Pending => suggestion_stats.pending += 1,
            SuggestionStatus::Approved => suggestion_stats.approved += 1,
            SuggestionStatus::Edited => suggestion_stats.edited += 1,
            SuggestionStatus::Rejected => suggestion_stats.rejected += 1,
        }
    }

    let detection_kind_counts = persistence::scripture_detection_kind_counts(conn, service_id)?
        .into_iter()
        .map(|(kind, count)| DetectionKindCount { kind, count })
        .collect();

    let timeline_category_counts = timeline::count_events_by_category(conn, service_id)?
        .into_iter()
        .map(|(category, count)| TimelineCategoryCount { category, count })
        .collect();

    let avg_inference_duration_ms = if speech_diagnostics.inference_duration_samples > 0 {
        Some(
            speech_diagnostics.inference_duration_ms_sum
                / speech_diagnostics.inference_duration_samples,
        )
    } else {
        None
    };

    let live_diagnostics = LiveDiagnosticsSnapshot {
        speech_feature_compiled: speech_diagnostics.feature_compiled,
        speech_model_loaded: speech_diagnostics.model_loaded,
        chunks_received: speech_diagnostics.chunks_received,
        inferences_attempted: speech_diagnostics.inferences_attempted,
        inferences_succeeded: speech_diagnostics.inferences_succeeded,
        last_inference_duration_ms: speech_diagnostics.last_inference_duration_ms,
        avg_inference_duration_ms,
        max_inference_duration_ms: speech_diagnostics.max_inference_duration_ms,
        queue_high_water_ms: speech_diagnostics.queue_high_water_ms,
        overload_events: speech_diagnostics.overload_events,
        audio_ms_dropped_overload: speech_diagnostics.audio_ms_dropped_overload,
        last_transcript_pipeline_duration_ms: speech_diagnostics
            .last_transcript_pipeline_duration_ms,
        embedding_feature_compiled: embedding_diagnostics.feature_compiled,
        embedding_model_loaded: embedding_diagnostics.model_loaded,
        embedding_ready,
    };

    Ok(ServiceReport {
        service,
        duration_minutes,
        suggestion_stats,
        detection_kind_counts,
        timeline_category_counts,
        live_diagnostics,
        generated_at: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use cip_core_ai::{Suggestion, SuggestionKind};
    use cip_core_bible::{ReferenceKind, ScriptureReference};
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use cip_core_service::ScriptureDetection;
    use cip_database::{open_in_memory, run_migrations};

    fn open_test_db() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn persist_test_service(conn: &Connection) -> ServiceSession {
        let session = ServiceSession::start("Test Service");
        persistence::persist_service(conn, &session).unwrap();
        session
    }

    fn sample_suggestion(service_id: Uuid, status: SuggestionStatus) -> Suggestion {
        let mut s = Suggestion::new(
            service_id,
            SuggestionKind::Scripture {
                reference: "ROM 8:28".to_string(),
            },
            ConfidenceResult::new(0.9, ConfidenceSource::Heuristic, None),
        );
        s.status = status;
        s
    }

    fn sample_detection(kind: ReferenceKind) -> ScriptureDetection {
        ScriptureDetection {
            kind,
            reference: Some(ScriptureReference::single("KJV", "ROM", 8, 28)),
            context: None,
            candidates: Vec::new(),
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Heuristic, None),
            raw_text: "Romans 8:28".to_string(),
        }
    }

    #[test]
    fn report_breaks_down_suggestions_by_status() {
        let conn = open_test_db();
        let service = persist_test_service(&conn);
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(service.id, SuggestionStatus::Approved),
        )
        .unwrap();
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(service.id, SuggestionStatus::Rejected),
        )
        .unwrap();
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(service.id, SuggestionStatus::Rejected),
        )
        .unwrap();
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(service.id, SuggestionStatus::Pending),
        )
        .unwrap();

        let report = build_service_report(
            &conn,
            service.id,
            &SpeechDiagnostics::default(),
            &EmbeddingDiagnostics::default(),
            false,
        )
        .unwrap();

        assert_eq!(report.suggestion_stats.total, 4);
        assert_eq!(report.suggestion_stats.approved, 1);
        assert_eq!(report.suggestion_stats.rejected, 2);
        assert_eq!(report.suggestion_stats.pending, 1);
        assert_eq!(report.suggestion_stats.edited, 0);
    }

    #[test]
    fn report_breaks_down_detections_by_kind() {
        let conn = open_test_db();
        let service = persist_test_service(&conn);
        persistence::persist_scripture_detection(
            &conn,
            service.id,
            None,
            "KJV",
            &sample_detection(ReferenceKind::Direct),
        )
        .unwrap();
        persistence::persist_scripture_detection(
            &conn,
            service.id,
            None,
            "KJV",
            &sample_detection(ReferenceKind::Semantic),
        )
        .unwrap();
        persistence::persist_scripture_detection(
            &conn,
            service.id,
            None,
            "KJV",
            &sample_detection(ReferenceKind::Semantic),
        )
        .unwrap();

        let report = build_service_report(
            &conn,
            service.id,
            &SpeechDiagnostics::default(),
            &EmbeddingDiagnostics::default(),
            false,
        )
        .unwrap();

        let semantic = report
            .detection_kind_counts
            .iter()
            .find(|c| c.kind == "SEMANTIC_REFERENCE")
            .expect("semantic count present");
        assert_eq!(semantic.count, 2);
        let direct = report
            .detection_kind_counts
            .iter()
            .find(|c| c.kind == "DIRECT_REFERENCE")
            .expect("direct count present");
        assert_eq!(direct.count, 1);
    }

    #[test]
    fn duration_is_none_for_a_still_active_service() {
        let conn = open_test_db();
        let service = persist_test_service(&conn);

        let report = build_service_report(
            &conn,
            service.id,
            &SpeechDiagnostics::default(),
            &EmbeddingDiagnostics::default(),
            false,
        )
        .unwrap();

        assert_eq!(
            report.duration_minutes, None,
            "an in-progress service must never report a guessed duration"
        );
    }

    #[test]
    fn duration_is_computed_for_an_ended_service() {
        let conn = open_test_db();
        let mut service = persist_test_service(&conn);
        service.ended_at = Some(service.started_at + Duration::minutes(42));
        persistence::update_service_status(
            &conn,
            service.id,
            cip_core_service::ServiceStatus::Ended,
            service.ended_at,
        )
        .unwrap();

        let report = build_service_report(
            &conn,
            service.id,
            &SpeechDiagnostics::default(),
            &EmbeddingDiagnostics::default(),
            false,
        )
        .unwrap();

        assert!((report.duration_minutes.unwrap() - 42.0).abs() < 0.01);
    }

    #[test]
    fn live_diagnostics_average_inference_duration_matches_get_pilot_diagnostics_derivation() {
        let conn = open_test_db();
        let service = persist_test_service(&conn);
        let speech = SpeechDiagnostics {
            inference_duration_ms_sum: 900,
            inference_duration_samples: 3,
            ..SpeechDiagnostics::default()
        };

        let report = build_service_report(
            &conn,
            service.id,
            &speech,
            &EmbeddingDiagnostics::default(),
            true,
        )
        .unwrap();

        assert_eq!(report.live_diagnostics.avg_inference_duration_ms, Some(300));
        assert!(report.live_diagnostics.embedding_ready);
    }

    #[test]
    fn live_diagnostics_average_is_none_with_zero_samples() {
        let conn = open_test_db();
        let service = persist_test_service(&conn);

        let report = build_service_report(
            &conn,
            service.id,
            &SpeechDiagnostics::default(),
            &EmbeddingDiagnostics::default(),
            false,
        )
        .unwrap();

        assert_eq!(report.live_diagnostics.avg_inference_duration_ms, None);
    }

    #[test]
    fn timeline_category_counts_reflect_recorded_events() {
        let conn = open_test_db();
        let service = persist_test_service(&conn);
        timeline::record_event(
            &conn,
            Some(service.id),
            crate::events::AppEvent::ServiceStarted,
            crate::logging::LogCategory::App,
            serde_json::json!({}),
        )
        .unwrap();
        timeline::record_event(
            &conn,
            Some(service.id),
            crate::events::AppEvent::ErrorOccurred,
            crate::logging::LogCategory::Error,
            serde_json::json!({}),
        )
        .unwrap();

        let report = build_service_report(
            &conn,
            service.id,
            &SpeechDiagnostics::default(),
            &EmbeddingDiagnostics::default(),
            false,
        )
        .unwrap();

        let error_count = report
            .timeline_category_counts
            .iter()
            .find(|c| c.category == "error")
            .map(|c| c.count)
            .unwrap_or(0);
        assert_eq!(error_count, 1);
    }

    #[test]
    fn a_report_only_ever_reflects_its_own_service() {
        let conn = open_test_db();
        let service_a = persist_test_service(&conn);
        let service_b = persist_test_service(&conn);
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(service_a.id, SuggestionStatus::Approved),
        )
        .unwrap();
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(service_b.id, SuggestionStatus::Rejected),
        )
        .unwrap();

        let report_a = build_service_report(
            &conn,
            service_a.id,
            &SpeechDiagnostics::default(),
            &EmbeddingDiagnostics::default(),
            false,
        )
        .unwrap();

        assert_eq!(report_a.suggestion_stats.total, 1);
        assert_eq!(report_a.suggestion_stats.approved, 1);
        assert_eq!(report_a.suggestion_stats.rejected, 0);
    }
}
