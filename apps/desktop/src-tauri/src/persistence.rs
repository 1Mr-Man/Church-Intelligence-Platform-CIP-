//! SQLite persistence for the live speech pipeline.
//!
//! Deliberately plain functions over `&rusqlite::Connection` and domain
//! types - no `tauri::State`, no async - so they're fully unit-testable
//! against an in-memory migrated database (see this module's tests) and
//! reusable from both real Tauri commands and the pipeline orchestrator
//! (`pipeline.rs`). This is where `core/service`'s Tauri-agnostic,
//! SQLite-agnostic `ProcessedSegment` actually gets written to the
//! existing schema - persistence was deliberately deferred to this layer
//! in Phase 1.1 (see `docs/bible-intelligence.md`).
//!
//! ## What gets persisted, and what deliberately doesn't
//!
//! Every **final** transcript segment is persisted, regardless of whether
//! it contains a scripture reference - it's a record of what was said.
//! Interim segments are never persisted (runtime/UI state only).
//!
//! A `scripture_detections` row is written for `Direct`, `Chapter`,
//! `Verse`, and `Sequential` detections - i.e. everything that resolved to
//! a real, Bible-validated piece of context or reference. `Chapter`
//! detections (no verse yet) store the book+chapter as `reference` (e.g.
//! `"ROM 8"`) since the column is free text, not a strict verse citation.
//! `Ambiguous` and `Unresolved` detections are **not** persisted - the
//! existing schema's `status` values (`detected`/`confirmed`/`rejected`/
//! `updated`) have no "this failed to resolve" state, and inventing one
//! would misrepresent a parser miss as a confirmed reference. They're
//! still visible to the operator in-session via the emitted event
//! payload - just not written to disk. See `docs/live-speech.md`.
//!
//! An `ai_suggestions` row is written for every `Suggestion` the pipeline
//! produces, always `status = 'pending'` - the speech pipeline itself
//! never writes `approved`/`edited`/`rejected`; only the operator-facing
//! commands in `commands.rs` do.

use chrono::Utc;
use cip_core_ai::{Suggestion, SuggestionKind, SuggestionStatus};
use cip_core_bible::ReferenceKind;
use cip_core_confidence::{ConfidenceLevel, ConfidenceResult, ConfidenceSource};
use cip_core_service::{ScriptureDetection, ServiceSession, ServiceStatus};
use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum PersistError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("failed to encode suggestion payload: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("row {0} not found")]
    NotFound(String),
}

fn confidence_level_str(level: ConfidenceLevel) -> &'static str {
    match level {
        ConfidenceLevel::Low => "low",
        ConfidenceLevel::Medium => "medium",
        ConfidenceLevel::High => "high",
    }
}

fn service_status_str(status: ServiceStatus) -> &'static str {
    match status {
        ServiceStatus::Started => "started",
        ServiceStatus::Paused => "paused",
        ServiceStatus::Ended => "ended",
    }
}

// --- services ---------------------------------------------------------

pub fn persist_service(conn: &Connection, session: &ServiceSession) -> Result<(), PersistError> {
    conn.execute(
        "INSERT INTO services (id, title, status, started_at, ended_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            session.id.to_string(),
            session.title,
            service_status_str(session.status),
            session.started_at.to_rfc3339(),
            session.ended_at.map(|t| t.to_rfc3339()),
        ],
    )?;
    Ok(())
}

pub fn update_service_status(
    conn: &Connection,
    service_id: Uuid,
    status: ServiceStatus,
    ended_at: Option<chrono::DateTime<Utc>>,
) -> Result<(), PersistError> {
    let rows = conn.execute(
        "UPDATE services SET status = ?1, ended_at = ?2 WHERE id = ?3",
        params![
            service_status_str(status),
            ended_at.map(|t| t.to_rfc3339()),
            service_id.to_string()
        ],
    )?;
    if rows == 0 {
        return Err(PersistError::NotFound(service_id.to_string()));
    }
    Ok(())
}

// --- transcript_segments -----------------------------------------------

/// Persist one **final** transcript segment. Interim segments must never
/// be passed here - see module docs.
pub fn persist_transcript_segment(
    conn: &Connection,
    service_id: Uuid,
    segment: &cip_core_ai::TranscriptSegment,
) -> Result<(), PersistError> {
    conn.execute(
        "INSERT INTO transcript_segments
            (id, service_id, text, is_final, confidence_score, confidence_level,
             start_ms, end_ms, created_at, sequence_number, language, speaker_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            segment.id.to_string(),
            service_id.to_string(),
            segment.text,
            segment.is_final,
            segment.confidence.score,
            confidence_level_str(segment.confidence.level),
            segment.start_ms as i64,
            segment.end_ms as i64,
            Utc::now().to_rfc3339(),
            segment.sequence as i64,
            segment.language,
            segment.speaker_id,
        ],
    )?;
    Ok(())
}

/// Most recent transcript segments for a service, oldest first (reading
/// order) - for the Live Church Brain's transcript feed.
pub fn list_transcript_segments(
    conn: &Connection,
    service_id: Uuid,
    limit: u32,
) -> Result<Vec<cip_core_ai::TranscriptSegment>, PersistError> {
    let mut stmt = conn.prepare(
        "SELECT id, text, is_final, confidence_score, start_ms, end_ms, sequence_number, language, speaker_id
         FROM transcript_segments WHERE service_id = ?1
         ORDER BY start_ms DESC, created_at DESC LIMIT ?2",
    )?;
    let mut rows: Vec<cip_core_ai::TranscriptSegment> = stmt
        .query_map(params![service_id.to_string(), limit], |row| {
            Ok(cip_core_ai::TranscriptSegment {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
                text: row.get(1)?,
                is_final: row.get(2)?,
                confidence: ConfidenceResult::new(
                    row.get::<_, f64>(3)? as f32,
                    ConfidenceSource::Model,
                    None,
                ),
                start_ms: row.get::<_, i64>(4)? as u64,
                end_ms: row.get::<_, i64>(5)? as u64,
                sequence: row.get::<_, Option<i64>>(6)?.unwrap_or(0) as u64,
                language: row.get(7)?,
                speaker_id: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.reverse(); // oldest first
    Ok(rows)
}

// --- scripture_detections -----------------------------------------------

/// Persist a validated detection - see module docs for which
/// [`ReferenceKind`]s qualify. Returns `Ok(false)` (not an error) for a
/// kind that's deliberately never persisted (`Ambiguous`/`Unresolved`).
pub fn persist_scripture_detection(
    conn: &Connection,
    service_id: Uuid,
    transcript_segment_id: Option<Uuid>,
    translation_id: &str,
    detection: &ScriptureDetection,
) -> Result<bool, PersistError> {
    let reference_text = match (&detection.reference, &detection.context) {
        (Some(reference), _) => reference.to_string(),
        (None, Some(context)) if detection.kind == ReferenceKind::Chapter => {
            format!("{} {}", context.book, context.chapter)
        }
        _ => return Ok(false), // Ambiguous / Unresolved / no context to describe - never persisted
    };

    conn.execute(
        "INSERT INTO scripture_detections
            (id, service_id, transcript_segment_id, reference, translation_id,
             confidence_score, confidence_level, status, detected_at,
             detection_type, source_text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'detected', ?8, ?9, ?10)",
        params![
            Uuid::new_v4().to_string(),
            service_id.to_string(),
            transcript_segment_id.map(|id| id.to_string()),
            reference_text,
            translation_id,
            detection.confidence.score,
            confidence_level_str(detection.confidence.level),
            Utc::now().to_rfc3339(),
            detection.kind.label(),
            detection.raw_text,
        ],
    )?;
    Ok(true)
}

// --- ai_suggestions -----------------------------------------------------

fn suggestion_status_str(status: SuggestionStatus) -> &'static str {
    match status {
        SuggestionStatus::Pending => "pending",
        SuggestionStatus::Approved => "approved",
        SuggestionStatus::Edited => "edited",
        SuggestionStatus::Rejected => "rejected",
    }
}

fn parse_suggestion_status(value: &str) -> SuggestionStatus {
    match value {
        "approved" => SuggestionStatus::Approved,
        "edited" => SuggestionStatus::Edited,
        "rejected" => SuggestionStatus::Rejected,
        _ => SuggestionStatus::Pending,
    }
}

/// `kind` discriminates the row the same way `SuggestionKind`'s serde tag
/// does, so `list_suggestions` can reconstruct it without guessing.
fn suggestion_kind_tag(kind: &SuggestionKind) -> &'static str {
    match kind {
        SuggestionKind::Scripture { .. } => "scripture",
        SuggestionKind::Other { .. } => "other",
        _ => "other",
    }
}

pub fn persist_suggestion(conn: &Connection, suggestion: &Suggestion) -> Result<(), PersistError> {
    let payload = serde_json::to_string(&suggestion.kind)?;
    conn.execute(
        "INSERT INTO ai_suggestions
            (id, service_id, kind, payload, status, confidence_score, confidence_level, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            suggestion.id.to_string(),
            suggestion.service_id.to_string(),
            suggestion_kind_tag(&suggestion.kind),
            payload,
            suggestion_status_str(suggestion.status),
            suggestion.confidence.score,
            confidence_level_str(suggestion.confidence.level),
            suggestion.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// `confidence_level` is stored alongside `confidence_score` for
/// query/filtering convenience, but it's a pure function of the score
/// (see `ConfidenceLevel::from_score`), so reconstructing a `Suggestion`
/// only needs the score - `ConfidenceResult::new` recomputes the same
/// level from it. `reason` isn't a persisted column, so a reloaded
/// `ConfidenceResult` always has `reason: None`.
fn row_to_suggestion(
    id: String,
    service_id: String,
    payload: String,
    status: String,
    confidence_score: f64,
    created_at: String,
) -> Result<Suggestion, PersistError> {
    Ok(Suggestion {
        id: Uuid::parse_str(&id).map_err(|_| PersistError::NotFound(id.clone()))?,
        service_id: Uuid::parse_str(&service_id)
            .map_err(|_| PersistError::NotFound(service_id.clone()))?,
        kind: serde_json::from_str(&payload)?,
        status: parse_suggestion_status(&status),
        confidence: ConfidenceResult::new(
            confidence_score as f32,
            ConfidenceSource::Heuristic,
            None,
        ),
        created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    })
}

pub fn list_suggestions(
    conn: &Connection,
    service_id: Uuid,
    status: Option<SuggestionStatus>,
) -> Result<Vec<Suggestion>, PersistError> {
    let mut stmt = conn.prepare(
        "SELECT id, service_id, payload, status, confidence_score, created_at
         FROM ai_suggestions WHERE service_id = ?1 AND (?2 IS NULL OR status = ?2)
         ORDER BY created_at DESC",
    )?;
    let status_filter = status.map(suggestion_status_str);
    let rows = stmt
        .query_map(params![service_id.to_string(), status_filter], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    rows.into_iter()
        .map(|(id, service_id, payload, status, score, created_at)| {
            row_to_suggestion(id, service_id, payload, status, score, created_at)
        })
        .collect()
}

pub fn get_suggestion(conn: &Connection, suggestion_id: Uuid) -> Result<Suggestion, PersistError> {
    conn.query_row(
        "SELECT id, service_id, payload, status, confidence_score, created_at
         FROM ai_suggestions WHERE id = ?1",
        params![suggestion_id.to_string()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, String>(5)?,
            ))
        },
    )
    .optional()?
    .ok_or_else(|| PersistError::NotFound(suggestion_id.to_string()))
    .and_then(|(id, service_id, payload, status, score, created_at)| {
        row_to_suggestion(id, service_id, payload, status, score, created_at)
    })
}

/// Update a suggestion's status (and, for an edit, its payload). Returns
/// the updated row. `expected_from` is the set of statuses this transition
/// is valid from (e.g. approve is only valid from `Pending`/`Edited`) -
/// the speech pipeline never calls this at all, only operator-facing
/// commands do.
pub fn update_suggestion_status(
    conn: &Connection,
    suggestion_id: Uuid,
    new_status: SuggestionStatus,
    new_kind: Option<&SuggestionKind>,
) -> Result<Suggestion, PersistError> {
    if let Some(kind) = new_kind {
        let payload = serde_json::to_string(kind)?;
        conn.execute(
            "UPDATE ai_suggestions SET status = ?1, kind = ?2, payload = ?3 WHERE id = ?4",
            params![
                suggestion_status_str(new_status),
                suggestion_kind_tag(kind),
                payload,
                suggestion_id.to_string()
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE ai_suggestions SET status = ?1 WHERE id = ?2",
            params![suggestion_status_str(new_status), suggestion_id.to_string()],
        )?;
    }

    conn.query_row(
        "SELECT id, service_id, payload, status, confidence_score, created_at
         FROM ai_suggestions WHERE id = ?1",
        params![suggestion_id.to_string()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, String>(5)?,
            ))
        },
    )
    .optional()?
    .ok_or_else(|| PersistError::NotFound(suggestion_id.to_string()))
    .and_then(|(id, service_id, payload, status, score, created_at)| {
        row_to_suggestion(id, service_id, payload, status, score, created_at)
    })
}

// --- presentation_items ---------------------------------------------------

/// Persist a prepared presentation item (`status = 'prepared'`, matching
/// `PresentationItemStatus::Prepared`). Nothing in this pipeline ever
/// writes `'active'` - see `commands::prepare_presentation`'s docs.
pub fn persist_presentation_item(
    conn: &Connection,
    item: &cip_core_presentation::PresentationItem,
) -> Result<(), PersistError> {
    let content_type = match &item.content {
        cip_core_presentation::PresentationContent::Scripture { .. } => "scripture",
        cip_core_presentation::PresentationContent::Text { .. } => "text",
        _ => "text",
    };
    let content = serde_json::to_string(&item.content)?;
    conn.execute(
        "INSERT INTO presentation_items (id, service_id, content_type, content, status, created_at)
         VALUES (?1, ?2, ?3, ?4, 'prepared', ?5)",
        params![
            item.id.to_string(),
            item.service_id.to_string(),
            content_type,
            content,
            item.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_ai::TranscriptSegment;
    use cip_core_bible::{ScriptureContext, ScriptureReference};
    use cip_database::{open_in_memory, run_migrations};

    fn migrated_conn() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn seeded_service(conn: &Connection) -> ServiceSession {
        let session = ServiceSession::start("Test Service");
        persist_service(conn, &session).unwrap();
        session
    }

    fn sample_transcript_segment(text: &str, sequence: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence,
            text: text.to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Model, None),
            start_ms: 0,
            end_ms: 1000,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    #[test]
    fn persists_and_updates_a_service_session() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        update_service_status(&conn, session.id, ServiceStatus::Ended, Some(Utc::now())).unwrap();

        let status: String = conn
            .query_row(
                "SELECT status FROM services WHERE id = ?1",
                params![session.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "ended");
    }

    #[test]
    fn persists_a_final_transcript_segment() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let segment = sample_transcript_segment("Turn with me to Romans chapter 8.", 0);
        persist_transcript_segment(&conn, session.id, &segment).unwrap();

        let count: i64 = conn
            .query_row("SELECT count(*) FROM transcript_segments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn persists_a_chapter_detection_using_book_and_chapter_as_reference() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let context = ScriptureContext {
            translation_id: "KJV".into(),
            book: "ROM".into(),
            chapter: 8,
            last_verse: None,
            confidence: ConfidenceResult::new(0.95, ConfidenceSource::Heuristic, None),
            established_at: Utc::now(),
            valid: true,
        };
        let detection = ScriptureDetection {
            kind: ReferenceKind::Chapter,
            reference: None,
            context: Some(context),
            candidates: vec![],
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Heuristic, None),
            raw_text: "Romans chapter 8".into(),
        };

        let persisted =
            persist_scripture_detection(&conn, session.id, None, "KJV", &detection).unwrap();
        assert!(persisted);

        let reference: String = conn
            .query_row("SELECT reference FROM scripture_detections", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(reference, "ROM 8");
    }

    #[test]
    fn does_not_persist_unresolved_or_ambiguous_detections() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let unresolved = ScriptureDetection {
            kind: ReferenceKind::Unresolved,
            reference: None,
            context: None,
            candidates: vec![],
            confidence: ConfidenceResult::new(0.1, ConfidenceSource::Heuristic, None),
            raw_text: "verse 28".into(),
        };

        let persisted =
            persist_scripture_detection(&conn, session.id, None, "KJV", &unresolved).unwrap();
        assert!(!persisted);

        let count: i64 = conn
            .query_row("SELECT count(*) FROM scripture_detections", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn persists_a_direct_detection_with_the_full_reference() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let detection = ScriptureDetection {
            kind: ReferenceKind::Direct,
            reference: Some(ScriptureReference::single("KJV", "ROM", 8, 28)),
            context: None,
            candidates: vec![],
            confidence: ConfidenceResult::new(0.97, ConfidenceSource::Heuristic, None),
            raw_text: "Romans 8:28".into(),
        };
        persist_scripture_detection(&conn, session.id, None, "KJV", &detection).unwrap();

        let (reference, detection_type): (String, String) = conn
            .query_row(
                "SELECT reference, detection_type FROM scripture_detections",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(reference, "ROM 8:28");
        assert_eq!(detection_type, "DIRECT_REFERENCE");
    }

    #[test]
    fn suggestion_round_trips_through_persistence() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let suggestion = Suggestion::new(
            session.id,
            SuggestionKind::Scripture {
                reference: "ROM 8:28".into(),
            },
            ConfidenceResult::new(0.95, ConfidenceSource::Heuristic, None),
        );
        persist_suggestion(&conn, &suggestion).unwrap();

        let loaded = list_suggestions(&conn, session.id, None).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, suggestion.id);
        assert_eq!(loaded[0].status, SuggestionStatus::Pending);
        assert!(
            matches!(&loaded[0].kind, SuggestionKind::Scripture { reference } if reference == "ROM 8:28")
        );
    }

    #[test]
    fn approving_a_suggestion_updates_its_persisted_status() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let suggestion = Suggestion::new(
            session.id,
            SuggestionKind::Scripture {
                reference: "ROM 8:28".into(),
            },
            ConfidenceResult::new(0.95, ConfidenceSource::Heuristic, None),
        );
        persist_suggestion(&conn, &suggestion).unwrap();

        let updated =
            update_suggestion_status(&conn, suggestion.id, SuggestionStatus::Approved, None)
                .unwrap();
        assert_eq!(updated.status, SuggestionStatus::Approved);

        let reloaded =
            list_suggestions(&conn, session.id, Some(SuggestionStatus::Approved)).unwrap();
        assert_eq!(reloaded.len(), 1);
    }

    #[test]
    fn editing_a_suggestion_replaces_its_payload_and_status() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let suggestion = Suggestion::new(
            session.id,
            SuggestionKind::Scripture {
                reference: "ROM 8:28".into(),
            },
            ConfidenceResult::new(0.95, ConfidenceSource::Heuristic, None),
        );
        persist_suggestion(&conn, &suggestion).unwrap();

        let edited_kind = SuggestionKind::Scripture {
            reference: "ROM 8:29".into(),
        };
        let updated = update_suggestion_status(
            &conn,
            suggestion.id,
            SuggestionStatus::Edited,
            Some(&edited_kind),
        )
        .unwrap();
        assert_eq!(updated.status, SuggestionStatus::Edited);
        assert!(
            matches!(&updated.kind, SuggestionKind::Scripture { reference } if reference == "ROM 8:29")
        );
    }

    #[test]
    fn updating_a_nonexistent_suggestion_reports_not_found() {
        let conn = migrated_conn();
        let result =
            update_suggestion_status(&conn, Uuid::new_v4(), SuggestionStatus::Approved, None);
        assert!(matches!(result, Err(PersistError::NotFound(_))));
    }

    #[test]
    fn persists_a_prepared_presentation_item_never_active() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let item = cip_core_presentation::PresentationItem::prepare(
            session.id,
            cip_core_presentation::PresentationContent::Scripture {
                reference: "ROM 8:28".into(),
                translation_id: "KJV".into(),
                text: "And we know...".into(),
            },
        );
        persist_presentation_item(&conn, &item).unwrap();

        let status: String = conn
            .query_row("SELECT status FROM presentation_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "prepared");
    }

    #[test]
    fn list_transcript_segments_returns_oldest_first() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        persist_transcript_segment(&conn, session.id, &sample_transcript_segment("first", 0))
            .unwrap();
        persist_transcript_segment(&conn, session.id, &sample_transcript_segment("second", 1))
            .unwrap();

        let segments = list_transcript_segments(&conn, session.id, 10).unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "first");
        assert_eq!(segments[1].text, "second");
    }

    #[test]
    fn get_suggestion_returns_the_persisted_row() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let suggestion = Suggestion::new(
            session.id,
            SuggestionKind::Scripture {
                reference: "ROM 8:28".into(),
            },
            ConfidenceResult::new(0.95, ConfidenceSource::Heuristic, None),
        );
        persist_suggestion(&conn, &suggestion).unwrap();

        let loaded = get_suggestion(&conn, suggestion.id).unwrap();
        assert_eq!(loaded.id, suggestion.id);
    }
}
