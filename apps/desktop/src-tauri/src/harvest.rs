//! Sermon Harvest (Phase 3.9) - a single read-only aggregation of data
//! this application has already captured through its existing, separately-
//! tested pipelines (sermon foundation sections, Sermon Intelligence
//! findings, Bible suggestions, the transcript, the service timeline).
//!
//! This module intentionally contains **no detection logic of its own**.
//! It does not classify text, does not invent a title/summary when one is
//! absent, and does not run any new AI pass over the transcript - it only
//! reads back what `sermon_foundation`, `core/intelligence`'s Sermon
//! adapter, Bible Intelligence, and the transcript/timeline persistence
//! layers already produced, live, during the service. See
//! `docs/phase-3-9-sermon-harvest.md` for the design rationale and the
//! honest scope boundary (why this only ever harvests the *currently
//! active* sermon, never a past one, in this phase).

use crate::persistence;
use crate::timeline::{self, TimelineEntry};
use chrono::{DateTime, Utc};
use cip_core_ai::{Suggestion, TranscriptSegment};
use cip_core_intelligence::{FindingQueue, FindingStatus, IntelligenceDomain, IntelligenceFinding};
use cip_core_sermon::foundation::{Sermon, SermonSection};
use rusqlite::Connection;
use serde::Serialize;

/// Bounds on how much transcript/timeline a single harvest reads back -
/// generous enough to cover a full multi-hour service (at the ~15-18s
/// logical segments `TranscriptSegmenter` produces, a 3-hour service is
/// well under 1,000 segments) without ever being a genuinely unbounded
/// query. Mirrors the bounded-read discipline `IntelligenceContext`
/// already applies elsewhere in this codebase.
const HARVEST_TRANSCRIPT_LIMIT: u32 = 5_000;
const HARVEST_TIMELINE_LIMIT: u32 = 5_000;

/// The complete Sermon Harvest bundle - every already-captured piece of
/// data tied to one sermon/service, assembled into one response so the
/// operator does not have to open five separate panels to find it. Every
/// field here is data this application already had; nothing is generated
/// or summarized by this module.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonHarvest {
    pub sermon: Sermon,
    /// Structural spans (Introduction/Scripture Reading/Main Message/...),
    /// oldest first - `sermon_foundation`'s own already-tested data,
    /// untouched here.
    pub sections: Vec<SermonSection>,
    /// Every non-rejected Sermon Intelligence finding tied to this exact
    /// sermon (via `IntelligenceFinding::sermon_id`), oldest first. Each
    /// finding's own `summary` is already presentation-ready text (e.g.
    /// "Prayer Point: ...", "Food for Thought: ...") produced by the
    /// Sermon engine - this module does not re-parse or re-classify it.
    /// Rejected findings are excluded (an operator explicitly said this
    /// one was wrong) but `Detected`/`Reviewed`/`Accepted` are all kept -
    /// harvesting only what's been reviewed and accepted would silently
    /// drop everything the operator hadn't gotten around to clicking yet.
    pub elements: Vec<IntelligenceFinding>,
    /// Every Bible suggestion for the sermon's service, any status, oldest
    /// first - the same data `list_suggestions`/history already expose,
    /// gathered here for one bundle.
    pub scripture: Vec<Suggestion>,
    /// The full transcript for the sermon's service, oldest first, up to
    /// `HARVEST_TRANSCRIPT_LIMIT` segments.
    pub transcript: Vec<TranscriptSegment>,
    /// The service's audit timeline, oldest first, up to
    /// `HARVEST_TIMELINE_LIMIT` entries - the "00:14:23 - Worship,
    /// 00:37:18 - Sermon..." style record.
    pub timeline: Vec<TimelineEntry>,
    pub generated_at: DateTime<Utc>,
}

/// Pure aggregation: reads back already-persisted/already-in-memory data
/// for `sermon` and assembles it. Never queries anything not already
/// scoped to `sermon.id`/`sermon.service_id`, never writes anything.
pub fn harvest_sermon(
    conn: &Connection,
    findings: &FindingQueue,
    sermon: &Sermon,
) -> Result<SermonHarvest, persistence::PersistError> {
    let sections = persistence::list_sermon_sections(conn, sermon.id)?;

    let mut elements: Vec<IntelligenceFinding> = findings
        .all()
        .into_iter()
        .filter(|f| {
            f.domain == IntelligenceDomain::Sermon
                && f.sermon_id == Some(sermon.id)
                && f.status != FindingStatus::Rejected
        })
        .cloned()
        .collect();
    elements.sort_by_key(|f| f.id); // stable, deterministic order (creation-adjacent - IntelligenceFinding carries no created_at field)

    let scripture = persistence::list_suggestions(conn, sermon.service_id, None)?;

    let mut transcript =
        persistence::list_transcript_segments(conn, sermon.service_id, HARVEST_TRANSCRIPT_LIMIT)?;
    transcript.sort_by_key(|s| s.start_ms);

    let mut timeline_entries =
        timeline::list_timeline(conn, sermon.service_id, HARVEST_TIMELINE_LIMIT)?;
    timeline_entries.sort_by_key(|e| e.created_at);

    Ok(SermonHarvest {
        sermon: sermon.clone(),
        sections,
        elements,
        scripture,
        transcript,
        timeline: timeline_entries,
        generated_at: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use cip_core_intelligence::{AssertionLevel, FindingKind};
    use cip_core_sermon::foundation::{SectionOrigin, SermonSectionKind, SermonStatus};
    use cip_core_service::ServiceSession;
    use cip_database::{open_in_memory, run_migrations};
    use rusqlite::Connection;
    use uuid::Uuid;

    fn open_test_db() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    /// `sermons.service_id` is foreign-key constrained - every test needs
    /// a real persisted service row before it can persist a sermon.
    fn persist_test_service(conn: &Connection) -> Uuid {
        let session = ServiceSession::start("Test Service");
        persistence::persist_service(conn, &session).unwrap();
        session.id
    }

    fn sample_sermon(service_id: Uuid) -> Sermon {
        Sermon {
            id: Uuid::new_v4(),
            service_id,
            title: Some("The Waiting Season".to_string()),
            speaker: None,
            status: SermonStatus::Active,
            started_at: Some(Utc::now()),
            ended_at: None,
            created_at: Utc::now(),
        }
    }

    fn sample_finding(
        service_id: Uuid,
        sermon_id: Uuid,
        status: FindingStatus,
        summary: &str,
    ) -> IntelligenceFinding {
        let mut finding = IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Sermon,
            FindingKind::Sermon,
            AssertionLevel::Observed,
            ConfidenceResult::new(0.85, ConfidenceSource::Heuristic, None),
            summary.to_string(),
            "test-engine",
            "1.0",
        );
        finding.sermon_id = Some(sermon_id);
        finding.status = status;
        finding
    }

    #[test]
    fn harvest_assembles_sermon_sections_elements_scripture_transcript_and_timeline() {
        let conn = open_test_db();
        let service_id = persist_test_service(&conn);
        let sermon = sample_sermon(service_id);
        persistence::persist_sermon(&conn, &sermon).unwrap();

        let section = SermonSection::open(
            sermon.id,
            SermonSectionKind::MainMessage,
            SectionOrigin::SystemBoundary,
            None,
        );
        persistence::persist_sermon_section(&conn, &section).unwrap();

        let segment = TranscriptSegment {
            id: Uuid::new_v4(),
            sequence: 1,
            text: "Waiting develops dependence on God.".to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Model, None),
            start_ms: 1_000,
            end_ms: 5_000,
            language: None,
            speaker_id: None,
        };
        persistence::persist_transcript_segment(&conn, service_id, &segment).unwrap();

        let mut findings = FindingQueue::new();
        findings.add(sample_finding(
            service_id,
            sermon.id,
            FindingStatus::Detected,
            "Takeaway: Waiting develops dependence on God",
        ));
        findings.add(sample_finding(
            service_id,
            sermon.id,
            FindingStatus::Rejected,
            "Takeaway: this one was wrong",
        ));
        // A finding for a *different* sermon must never leak into this harvest.
        findings.add(sample_finding(
            service_id,
            Uuid::new_v4(),
            FindingStatus::Detected,
            "Takeaway: belongs to a different sermon",
        ));

        let harvest = harvest_sermon(&conn, &findings, &sermon).unwrap();

        assert_eq!(harvest.sermon.id, sermon.id);
        assert_eq!(harvest.sections.len(), 1);
        assert_eq!(harvest.sections[0].kind, SermonSectionKind::MainMessage);
        assert_eq!(
            harvest.elements.len(),
            1,
            "rejected and other-sermon findings must be excluded"
        );
        assert_eq!(
            harvest.elements[0].summary,
            "Takeaway: Waiting develops dependence on God"
        );
        assert_eq!(harvest.transcript.len(), 1);
        assert_eq!(
            harvest.transcript[0].text,
            "Waiting develops dependence on God."
        );
    }

    #[test]
    fn harvest_never_fabricates_a_title_when_none_was_set() {
        let conn = open_test_db();
        let service_id = persist_test_service(&conn);
        let mut sermon = sample_sermon(service_id);
        sermon.title = None;
        persistence::persist_sermon(&conn, &sermon).unwrap();

        let findings = FindingQueue::new();
        let harvest = harvest_sermon(&conn, &findings, &sermon).unwrap();

        assert_eq!(
            harvest.sermon.title, None,
            "a missing title must stay None, never a placeholder"
        );
    }

    #[test]
    fn harvest_keeps_detected_and_accepted_findings_not_only_accepted() {
        let conn = open_test_db();
        let service_id = persist_test_service(&conn);
        let sermon = sample_sermon(service_id);
        persistence::persist_sermon(&conn, &sermon).unwrap();

        let mut findings = FindingQueue::new();
        findings.add(sample_finding(
            service_id,
            sermon.id,
            FindingStatus::Detected,
            "Question: still pending review",
        ));
        findings.add(sample_finding(
            service_id,
            sermon.id,
            FindingStatus::Accepted,
            "Prayer Point: already accepted",
        ));

        let harvest = harvest_sermon(&conn, &findings, &sermon).unwrap();

        assert_eq!(
            harvest.elements.len(),
            2,
            "harvest must not silently drop findings the operator hasn't reviewed yet"
        );
    }
}
