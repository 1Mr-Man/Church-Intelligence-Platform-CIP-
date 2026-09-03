//! Detection Accuracy Analytics (Phase 17) - a read-only, cross-service
//! aggregation of Bible suggestion outcomes this application already
//! persists (`ai_suggestions.status`, `.confidence_score`,
//! `.rejection_echo_count`) plus a correlation against the detection
//! method that produced each suggestion (`scripture_detections.detection_type`),
//! answering the question the master plan's own gap audit names explicitly
//! but this codebase has never instrumented: "measuring detection
//! accuracy/false-positive rate/correction rate empirically."
//!
//! Triggered by the operator's own real-hardware pilot feedback ("accuracy
//! fair") that motivated Phase 15's fuller-context paraphrase retry - this
//! phase does not change detection behavior at all, it only makes the
//! *existing* accept/edit/reject history observable, so a future
//! calibration decision (e.g. adjusting `MIN_PARAPHRASE_SCORE`) can be made
//! from real operator behavior instead of a guess.
//!
//! Mirrors `service_report.rs` and `sermon_knowledge_base.rs`'s own
//! discipline exactly: **no new detection logic**, no new AI, no new
//! persistence, no schema change. Everything here is either a plain read
//! of already-persisted `ai_suggestions`/`scripture_detections` rows or a
//! correlation performed in Rust (not SQL) between them, using the exact
//! same `(service_id, transcript_segment_id, reference)` join key
//! `pipeline.rs::persist_detections_and_suggestions` already guarantees is
//! shared between a detection and the suggestion it produced.
//!
//! ## Why the detection-kind breakdown can be incomplete, honestly
//!
//! A suggestion with no `transcript_segment_id` (e.g. one created via
//! manual operator context correction, not the live detection pipeline) or
//! whose reference cannot be matched against any persisted detection row
//! (a detection from before this correlation existed, or one that fell
//! outside `list_all_scripture_detections_with_segment`'s bound) is counted
//! in `overall` and `by_confidence_level` but not in `by_detection_kind` -
//! tallied instead in `unmatched_detection_kind_count`, never silently
//! dropped or guessed into a bucket it wasn't actually observed in.

use crate::persistence::{self, PersistError};
use chrono::{DateTime, Utc};
use cip_core_ai::{SuggestionKind, SuggestionStatus};
use cip_core_confidence::ConfidenceLevel;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

/// Bounds how much history one analytics build reads back - mirrors
/// `sermon_knowledge_base.rs`'s own `KNOWLEDGE_BASE_*_LIMIT` precedent
/// (generous enough for years of weekly services, not genuinely
/// unbounded).
const ANALYTICS_SUGGESTION_LIMIT: u32 = 50_000;
const ANALYTICS_DETECTION_LIMIT: u32 = 50_000;
const ANALYTICS_SERVICE_LIMIT: u32 = 5_000;

/// Suggestion outcome counts for some slice of history (overall, one
/// confidence level, one detection kind, or one service).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeCounts {
    pub total: u64,
    pub pending: u64,
    pub approved: u64,
    pub edited: u64,
    pub rejected: u64,
}

impl OutcomeCounts {
    fn record(&mut self, status: SuggestionStatus) {
        self.total += 1;
        match status {
            SuggestionStatus::Pending => self.pending += 1,
            SuggestionStatus::Approved => self.approved += 1,
            SuggestionStatus::Edited => self.edited += 1,
            SuggestionStatus::Rejected => self.rejected += 1,
        }
    }

    /// `approved / (approved + edited + rejected)` - `None` when nothing
    /// has been decided yet (only `Pending` suggestions, or zero
    /// suggestions at all), never a misleading 0%/100% for an empty
    /// denominator.
    pub fn approval_rate(&self) -> Option<f64> {
        let decided = self.approved + self.edited + self.rejected;
        (decided > 0).then(|| self.approved as f64 / decided as f64)
    }

    /// `(edited + rejected) / (approved + edited + rejected)` - how often
    /// a decided suggestion needed correcting or was wrong outright. The
    /// master plan's own "correction rate" language.
    pub fn correction_rate(&self) -> Option<f64> {
        let decided = self.approved + self.edited + self.rejected;
        (decided > 0).then(|| (self.edited + self.rejected) as f64 / decided as f64)
    }
}

fn confidence_level_key(level: ConfidenceLevel) -> &'static str {
    match level {
        ConfidenceLevel::Low => "low",
        ConfidenceLevel::Medium => "medium",
        ConfidenceLevel::High => "high",
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfidenceLevelBreakdown {
    pub level: String,
    pub counts: OutcomeCounts,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionKindBreakdown {
    /// A `ReferenceKind::label()` string (e.g. `"DIRECT_REFERENCE"`,
    /// `"PARAPHRASE_REFERENCE"`, `"SEMANTIC_REFERENCE"`).
    pub kind: String,
    pub counts: OutcomeCounts,
}

/// One service's own outcome counts, for a chronological trend list - "is
/// accuracy improving service to service."
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccuracyTrendEntry {
    pub service_id: Uuid,
    pub service_title: String,
    pub started_at: DateTime<Utc>,
    pub counts: OutcomeCounts,
}

/// The complete cross-service Bible detection accuracy bundle.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BibleDetectionAnalytics {
    pub overall: OutcomeCounts,
    /// `overall.approval_rate()`, computed once here so the frontend never
    /// has to re-derive "what counts as decided" itself.
    pub overall_approval_rate: Option<f64>,
    /// `overall.correction_rate()` - see [`OutcomeCounts::correction_rate`].
    pub overall_correction_rate: Option<f64>,
    /// Sum of every suggestion's `rejection_echo_count` (Phase 5.4) across
    /// every service - a systemic false-positive signal: how often, in
    /// total, a rejected reference kept resurfacing and being suppressed
    /// rather than resurrected.
    pub rejection_echoes: u64,
    /// Fixed order: low, medium, high (never reordered by count).
    pub by_confidence_level: Vec<ConfidenceLevelBreakdown>,
    /// Ascending by kind label, only kinds actually observed.
    pub by_detection_kind: Vec<DetectionKindBreakdown>,
    /// How many suggestions counted in `overall`/`by_confidence_level`
    /// could not be correlated to a detection kind - see this module's own
    /// doc comment for why that happens and why it's reported rather than
    /// hidden.
    pub unmatched_detection_kind_count: u64,
    /// Oldest first, so a chart or list reads as a timeline.
    pub service_trend: Vec<ServiceAccuracyTrendEntry>,
    pub generated_at: DateTime<Utc>,
}

/// Pure aggregation over already-persisted rows - reads `ai_suggestions`,
/// `scripture_detections`, and `services`; writes nothing.
pub fn build_bible_detection_analytics(
    conn: &Connection,
) -> Result<BibleDetectionAnalytics, PersistError> {
    let suggestions = persistence::list_all_suggestions(conn, ANALYTICS_SUGGESTION_LIMIT)?;
    let detections =
        persistence::list_all_scripture_detections_with_segment(conn, ANALYTICS_DETECTION_LIMIT)?;
    let services = persistence::list_services(conn, None, ANALYTICS_SERVICE_LIMIT)?;

    // First match wins for a given (service, segment, reference) - in the
    // rare case a segment produced more than one detection sharing a
    // reference, this is a best-effort correlation, not a guarantee; see
    // this module's own doc comment.
    let mut detection_kind_by_key: HashMap<(Uuid, Uuid, String), String> = HashMap::new();
    for (service_id, segment_id, reference, detection_type) in detections {
        detection_kind_by_key
            .entry((service_id, segment_id, reference))
            .or_insert(detection_type);
    }

    let mut overall = OutcomeCounts::default();
    let mut rejection_echoes = 0u64;
    let mut by_confidence: HashMap<&'static str, OutcomeCounts> = HashMap::new();
    let mut by_kind: HashMap<String, OutcomeCounts> = HashMap::new();
    let mut unmatched_detection_kind_count = 0u64;
    let mut per_service: HashMap<Uuid, OutcomeCounts> = HashMap::new();

    for suggestion in &suggestions {
        overall.record(suggestion.status);
        rejection_echoes += u64::from(suggestion.rejection_echo_count);

        let level = confidence_level_key(suggestion.confidence.level);
        by_confidence
            .entry(level)
            .or_default()
            .record(suggestion.status);

        per_service
            .entry(suggestion.service_id)
            .or_default()
            .record(suggestion.status);

        let reference = match &suggestion.kind {
            SuggestionKind::Scripture { reference } => Some(reference.clone()),
            _ => None,
        };
        let matched_kind =
            reference
                .zip(suggestion.transcript_segment_id)
                .and_then(|(reference, segment_id)| {
                    detection_kind_by_key
                        .get(&(suggestion.service_id, segment_id, reference))
                        .cloned()
                });
        match matched_kind {
            Some(kind) => {
                by_kind.entry(kind).or_default().record(suggestion.status);
            }
            None => unmatched_detection_kind_count += 1,
        }
    }

    let by_confidence_level = ["low", "medium", "high"]
        .into_iter()
        .map(|level| ConfidenceLevelBreakdown {
            level: level.to_string(),
            counts: by_confidence.get(level).copied().unwrap_or_default(),
        })
        .collect();

    let mut by_detection_kind: Vec<DetectionKindBreakdown> = by_kind
        .into_iter()
        .map(|(kind, counts)| DetectionKindBreakdown { kind, counts })
        .collect();
    by_detection_kind.sort_by(|a, b| a.kind.cmp(&b.kind));

    let mut service_trend: Vec<ServiceAccuracyTrendEntry> = services
        .into_iter()
        .map(|service| ServiceAccuracyTrendEntry {
            counts: per_service.get(&service.id).copied().unwrap_or_default(),
            service_id: service.id,
            service_title: service.title,
            started_at: service.started_at,
        })
        .collect();
    service_trend.sort_by_key(|entry| entry.started_at);

    Ok(BibleDetectionAnalytics {
        overall_approval_rate: overall.approval_rate(),
        overall_correction_rate: overall.correction_rate(),
        overall,
        rejection_echoes,
        by_confidence_level,
        by_detection_kind,
        unmatched_detection_kind_count,
        service_trend,
        generated_at: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_ai::Suggestion;
    use cip_core_bible::{ReferenceKind, ScriptureReference};
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use cip_core_service::{ScriptureDetection, ServiceSession};
    use cip_database::{open_in_memory, run_migrations};

    fn open_test_db() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn persist_test_service(conn: &Connection, title: &str) -> ServiceSession {
        let session = ServiceSession::start(title);
        persistence::persist_service(conn, &session).unwrap();
        session
    }

    fn sample_suggestion(
        service_id: Uuid,
        reference: &str,
        score: f32,
        status: SuggestionStatus,
    ) -> Suggestion {
        let mut s = Suggestion::new(
            service_id,
            SuggestionKind::Scripture {
                reference: reference.to_string(),
            },
            ConfidenceResult::new(score, ConfidenceSource::Heuristic, None),
        );
        s.status = status;
        s
    }

    fn sample_detection(kind: ReferenceKind, score: f32) -> ScriptureDetection {
        ScriptureDetection {
            kind,
            reference: Some(ScriptureReference::single("KJV", "ROM", 8, 28)),
            context: None,
            candidates: Vec::new(),
            confidence: ConfidenceResult::new(score, ConfidenceSource::Heuristic, None),
            raw_text: "Romans 8:28".to_string(),
        }
    }

    #[test]
    fn overall_breaks_down_every_suggestion_by_status_across_services() {
        let conn = open_test_db();
        let a = persist_test_service(&conn, "Service A");
        let b = persist_test_service(&conn, "Service B");
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(a.id, "ROM 8:28", 0.9, SuggestionStatus::Approved),
        )
        .unwrap();
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(b.id, "JHN 3:16", 0.9, SuggestionStatus::Rejected),
        )
        .unwrap();
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(b.id, "PSA 23:1", 0.5, SuggestionStatus::Pending),
        )
        .unwrap();

        let analytics = build_bible_detection_analytics(&conn).unwrap();

        assert_eq!(analytics.overall.total, 3);
        assert_eq!(analytics.overall.approved, 1);
        assert_eq!(analytics.overall.rejected, 1);
        assert_eq!(analytics.overall.pending, 1);
        assert_eq!(analytics.overall.approval_rate(), Some(0.5));
    }

    #[test]
    fn approval_rate_is_none_when_nothing_has_been_decided() {
        let conn = open_test_db();
        let a = persist_test_service(&conn, "Service A");
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(a.id, "ROM 8:28", 0.9, SuggestionStatus::Pending),
        )
        .unwrap();

        let analytics = build_bible_detection_analytics(&conn).unwrap();

        assert_eq!(analytics.overall.approval_rate(), None);
        assert_eq!(analytics.overall.correction_rate(), None);
    }

    #[test]
    fn approval_rate_is_none_with_zero_suggestions() {
        let conn = open_test_db();
        let analytics = build_bible_detection_analytics(&conn).unwrap();
        assert_eq!(analytics.overall.total, 0);
        assert_eq!(analytics.overall.approval_rate(), None);
    }

    #[test]
    fn confidence_level_breakdown_always_reports_all_three_levels_in_order() {
        let conn = open_test_db();
        let a = persist_test_service(&conn, "Service A");
        // Only a high-confidence suggestion exists - low/medium must still
        // appear, with zero counts, not be omitted.
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(a.id, "ROM 8:28", 0.97, SuggestionStatus::Approved),
        )
        .unwrap();

        let analytics = build_bible_detection_analytics(&conn).unwrap();

        let levels: Vec<&str> = analytics
            .by_confidence_level
            .iter()
            .map(|b| b.level.as_str())
            .collect();
        assert_eq!(levels, vec!["low", "medium", "high"]);
        let high = &analytics.by_confidence_level[2];
        assert_eq!(high.counts.approved, 1);
        assert_eq!(analytics.by_confidence_level[0].counts.total, 0);
    }

    fn persist_test_segment(conn: &Connection, service_id: Uuid, text: &str) -> Uuid {
        let segment = cip_core_ai::TranscriptSegment {
            id: Uuid::new_v4(),
            sequence: 0,
            text: text.to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Model, None),
            start_ms: 0,
            end_ms: 1000,
            language: None,
            speaker_id: None,
        };
        persistence::persist_transcript_segment(conn, service_id, &segment).unwrap();
        segment.id
    }

    #[test]
    fn detection_kind_breakdown_correlates_suggestion_outcome_with_its_detection_type() {
        let conn = open_test_db();
        let service = persist_test_service(&conn, "Service A");
        let segment_id = persist_test_segment(&conn, service.id, "Romans 8:28");

        persistence::persist_scripture_detection(
            &conn,
            service.id,
            Some(segment_id),
            "KJV",
            &sample_detection(ReferenceKind::Direct, 0.97),
        )
        .unwrap();
        let mut suggestion =
            sample_suggestion(service.id, "ROM 8:28", 0.97, SuggestionStatus::Approved);
        suggestion = suggestion.with_source(segment_id, "Romans 8:28");
        persistence::persist_suggestion(&conn, &suggestion).unwrap();

        let analytics = build_bible_detection_analytics(&conn).unwrap();

        let direct = analytics
            .by_detection_kind
            .iter()
            .find(|b| b.kind == "DIRECT_REFERENCE")
            .expect("direct-reference bucket present");
        assert_eq!(direct.counts.approved, 1);
        assert_eq!(analytics.unmatched_detection_kind_count, 0);
    }

    #[test]
    fn a_suggestion_with_no_matching_detection_counts_as_unmatched_not_dropped() {
        let conn = open_test_db();
        let service = persist_test_service(&conn, "Service A");
        // A manual context-correction suggestion: no transcript_segment_id.
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(service.id, "ROM 8:28", 0.9, SuggestionStatus::Approved),
        )
        .unwrap();

        let analytics = build_bible_detection_analytics(&conn).unwrap();

        assert_eq!(analytics.overall.total, 1, "still counted overall");
        assert_eq!(analytics.unmatched_detection_kind_count, 1);
        assert!(analytics.by_detection_kind.is_empty());
    }

    #[test]
    fn rejection_echoes_sum_across_every_service() {
        let conn = open_test_db();
        let a = persist_test_service(&conn, "Service A");
        let b = persist_test_service(&conn, "Service B");
        let rejected_a = sample_suggestion(a.id, "ROM 8:28", 0.7, SuggestionStatus::Rejected);
        persistence::persist_suggestion(&conn, &rejected_a).unwrap();
        persistence::record_rejection_echo(&conn, rejected_a.id).unwrap();
        persistence::record_rejection_echo(&conn, rejected_a.id).unwrap();

        let rejected_b = sample_suggestion(b.id, "JHN 3:16", 0.7, SuggestionStatus::Rejected);
        persistence::persist_suggestion(&conn, &rejected_b).unwrap();
        persistence::record_rejection_echo(&conn, rejected_b.id).unwrap();

        let analytics = build_bible_detection_analytics(&conn).unwrap();

        assert_eq!(analytics.rejection_echoes, 3);
    }

    #[test]
    fn service_trend_is_ordered_oldest_first_and_scoped_per_service() {
        let conn = open_test_db();
        let older = persist_test_service(&conn, "Older Service");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let newer = persist_test_service(&conn, "Newer Service");
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(older.id, "ROM 8:28", 0.9, SuggestionStatus::Approved),
        )
        .unwrap();
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(newer.id, "JHN 3:16", 0.9, SuggestionStatus::Rejected),
        )
        .unwrap();
        persistence::persist_suggestion(
            &conn,
            &sample_suggestion(newer.id, "PSA 23:1", 0.9, SuggestionStatus::Rejected),
        )
        .unwrap();

        let analytics = build_bible_detection_analytics(&conn).unwrap();

        assert_eq!(analytics.service_trend.len(), 2);
        assert_eq!(analytics.service_trend[0].service_id, older.id);
        assert_eq!(analytics.service_trend[0].counts.approved, 1);
        assert_eq!(analytics.service_trend[1].service_id, newer.id);
        assert_eq!(analytics.service_trend[1].counts.rejected, 2);
    }

    #[test]
    fn a_service_with_no_suggestions_still_appears_in_the_trend_with_zero_counts() {
        let conn = open_test_db();
        let service = persist_test_service(&conn, "Quiet Service");

        let analytics = build_bible_detection_analytics(&conn).unwrap();

        assert_eq!(analytics.service_trend.len(), 1);
        assert_eq!(analytics.service_trend[0].service_id, service.id);
        assert_eq!(analytics.service_trend[0].counts.total, 0);
    }
}
