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
//! `Verse`, `Sequential`, and `Paraphrase` detections - i.e. everything
//! that resolved to a real, Bible-validated piece of context or reference.
//! `Chapter` detections (no verse yet) store the book+chapter as
//! `reference` (e.g. `"ROM 8"`) since the column is free text, not a
//! strict verse citation. `Ambiguous` and `Unresolved` detections are
//! **not** persisted - the existing schema's `status` values (`detected`/`confirmed`/`rejected`/
//! `updated`) have no "this failed to resolve" state, and inventing one
//! would misrepresent a parser miss as a confirmed reference. They're
//! still visible to the operator in-session via the emitted event
//! payload - just not written to disk. See `docs/live-speech.md`.
//!
//! An `ai_suggestions` row is written for every `Suggestion` the pipeline
//! produces, always `status = 'pending'` - the speech pipeline itself
//! never writes `approved`/`edited`/`rejected`; only the operator-facing
//! commands in `commands.rs` do.

use crate::display_registry::DisplayRole;
use chrono::{DateTime, Utc};
use cip_core_ai::{Suggestion, SuggestionKind, SuggestionStatus};
use cip_core_bible::ReferenceKind;
use cip_core_confidence::{ConfidenceLevel, ConfidenceResult, ConfidenceSource};
use cip_core_service::{ScriptureDetection, ServiceSession, ServiceStatus};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum PersistError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("failed to encode payload: {0}")]
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

fn parse_service_status(value: &str) -> ServiceStatus {
    match value {
        "paused" => ServiceStatus::Paused,
        "ended" => ServiceStatus::Ended,
        _ => ServiceStatus::Started,
    }
}

fn row_to_service(
    id: String,
    title: String,
    status: String,
    started_at: String,
    ended_at: Option<String>,
) -> Result<ServiceSession, PersistError> {
    Ok(ServiceSession {
        id: Uuid::parse_str(&id).map_err(|_| PersistError::NotFound(id.clone()))?,
        title,
        status: parse_service_status(&status),
        started_at: chrono::DateTime::parse_from_rfc3339(&started_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        ended_at: ended_at.and_then(|t| {
            chrono::DateTime::parse_from_rfc3339(&t)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }),
    })
}

/// A single service by id - the service archive's detail view (Phase 1.3
/// section 34) uses this to look up a *completed* service independent of
/// whichever one (if any) is currently active, unlike every other
/// `list_*`/`get_*` function in this module, which is implicitly scoped to
/// `AppState::active_service` by its caller in `commands.rs`.
pub fn get_service(conn: &Connection, service_id: Uuid) -> Result<ServiceSession, PersistError> {
    conn.query_row(
        "SELECT id, title, status, started_at, ended_at FROM services WHERE id = ?1",
        params![service_id.to_string()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        },
    )
    .optional()?
    .ok_or_else(|| PersistError::NotFound(service_id.to_string()))
    .and_then(|(id, title, status, started_at, ended_at)| {
        row_to_service(id, title, status, started_at, ended_at)
    })
}

/// Completed services, most recently started first, bounded by `limit` -
/// the service archive's list view (Phase 1.3 section 34). Deliberately
/// only ever `ended` here (an in-progress service belongs in the live
/// view, not the archive) - see `commands::list_service_history`.
pub fn list_services(
    conn: &Connection,
    status: Option<ServiceStatus>,
    limit: u32,
) -> Result<Vec<ServiceSession>, PersistError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, status, started_at, ended_at FROM services
         WHERE (?1 IS NULL OR status = ?1)
         ORDER BY started_at DESC LIMIT ?2",
    )?;
    let status_filter = status.map(service_status_str);
    let rows = stmt
        .query_map(params![status_filter, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    rows.into_iter()
        .map(|(id, title, status, started_at, ended_at)| {
            row_to_service(id, title, status, started_at, ended_at)
        })
        .collect()
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

/// How many persisted `scripture_detections` rows exist for `service_id`,
/// grouped by `detection_type` (the `ReferenceKind::label()` string, e.g.
/// `"DIRECT_REFERENCE"`/`"SEMANTIC_REFERENCE"`) - the Phase 5.1 post-service
/// report's detection-kind breakdown. A plain `GROUP BY` count, not a row
/// dump: a service's detection count can run into the hundreds, and this
/// report only ever needs totals per kind.
pub fn scripture_detection_kind_counts(
    conn: &Connection,
    service_id: Uuid,
) -> Result<Vec<(String, u64)>, PersistError> {
    let mut stmt = conn.prepare(
        "SELECT detection_type, COUNT(*) FROM scripture_detections
         WHERE service_id = ?1 GROUP BY detection_type ORDER BY detection_type",
    )?;
    let rows = stmt
        .query_map(params![service_id.to_string()], |row| {
            let kind: Option<String> = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((kind.unwrap_or_else(|| "UNKNOWN".to_string()), count as u64))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
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
            (id, service_id, kind, payload, status, confidence_score, confidence_level, created_at,
             transcript_segment_id, source_text, confirmation_count, rejection_echo_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            suggestion.id.to_string(),
            suggestion.service_id.to_string(),
            suggestion_kind_tag(&suggestion.kind),
            payload,
            suggestion_status_str(suggestion.status),
            suggestion.confidence.score,
            confidence_level_str(suggestion.confidence.level),
            suggestion.created_at.to_rfc3339(),
            suggestion.transcript_segment_id.map(|id| id.to_string()),
            suggestion.source_text,
            suggestion.confirmation_count,
            suggestion.rejection_echo_count,
        ],
    )?;
    Ok(())
}

/// Which dedup category a detection's reference falls into - see
/// [`has_recent_detection_for_reference`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionCategory {
    /// `DIRECT_REFERENCE` / `VERSE_REFERENCE` / `SEQUENTIAL_REFERENCE` - an
    /// explicit citation.
    Explicit,
    /// `PARAPHRASE_REFERENCE` - a lexical-overlap guess, never a citation.
    Paraphrase,
    /// `SEMANTIC_REFERENCE` - an embedding-similarity guess, never a
    /// citation. Phase 5.2 fix: previously fell into the `Explicit` bucket
    /// by omission (`ReferenceKind::Semantic` was never routed to its own
    /// category in `pipeline.rs`), so a repeated semantic guess was never
    /// deduped against itself - only ever checked against
    /// `DIRECT`/`VERSE`/`SEQUENTIAL_REFERENCE` types, which a semantic
    /// match almost never shares a `detected_at` window with. Its own
    /// bucket restores the same "repeat within the window is suppressed"
    /// guarantee `Paraphrase` already had.
    Semantic,
}

/// Phase 1.3 session-aware suggestion deduplication, extended in Phase 4.1
/// with a `category` split: has this exact reference already had a
/// same-category detection for this service within the last
/// `window_seconds`? A pastor repeating "Romans 8:28" mid-explanation
/// should not flood the queue with identical suggestions, but a *genuine*
/// repeat later in the service (past the window) is legitimate and must
/// not be silently suppressed - see `docs/live-service.md`'s deduplication
/// policy. Scoped to one service (never cross-service or
/// permanent/global).
///
/// Queries `scripture_detections` (written for every detection, not just
/// the ones that survived dedup as a suggestion) and filters by
/// `category` - so a repeated `Paraphrase`/`Semantic` guess for the same
/// verse is suppressed, and a repeated explicit citation is suppressed,
/// but an explicit citation is **never** suppressed just because a
/// `Paraphrase`/`Semantic` guess for the same verse was already made
/// moments earlier (or vice versa). A pastor who paraphrases a verse and
/// then reads it verbatim - or the reverse - should see both, since the
/// second one is new, more specific information.
///
/// `excluding_transcript_segment_id` must be the current segment's id: the
/// caller (`pipeline::handle_final_transcript`) always persists every
/// detection's `scripture_detections` row *before* running this dedup
/// check, so without excluding the current segment's own just-written row,
/// every suggestion would look like a duplicate of itself.
pub fn has_recent_detection_for_reference(
    conn: &Connection,
    service_id: Uuid,
    reference_display: &str,
    category: DetectionCategory,
    window_seconds: i64,
    excluding_transcript_segment_id: Uuid,
) -> Result<bool, PersistError> {
    let cutoff = (Utc::now() - chrono::Duration::seconds(window_seconds)).to_rfc3339();
    let sql = match category {
        DetectionCategory::Explicit => {
            "SELECT count(*) FROM scripture_detections
             WHERE service_id = ?1 AND reference = ?2 AND detected_at >= ?3
             AND (transcript_segment_id IS NULL OR transcript_segment_id != ?4)
             AND detection_type IN ('DIRECT_REFERENCE', 'VERSE_REFERENCE', 'SEQUENTIAL_REFERENCE')"
        }
        DetectionCategory::Paraphrase => {
            "SELECT count(*) FROM scripture_detections
             WHERE service_id = ?1 AND reference = ?2 AND detected_at >= ?3
             AND (transcript_segment_id IS NULL OR transcript_segment_id != ?4)
             AND detection_type = 'PARAPHRASE_REFERENCE'"
        }
        DetectionCategory::Semantic => {
            "SELECT count(*) FROM scripture_detections
             WHERE service_id = ?1 AND reference = ?2 AND detected_at >= ?3
             AND (transcript_segment_id IS NULL OR transcript_segment_id != ?4)
             AND detection_type = 'SEMANTIC_REFERENCE'"
        }
    };
    let count: i64 = conn.query_row(
        sql,
        params![
            service_id.to_string(),
            reference_display,
            cutoff,
            excluding_transcript_segment_id.to_string()
        ],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// `confidence_level` is stored alongside `confidence_score` for
/// query/filtering convenience, but it's a pure function of the score
/// (see `ConfidenceLevel::from_score`), so reconstructing a `Suggestion`
/// only needs the score - `ConfidenceResult::new` recomputes the same
/// level from it. `reason` isn't a persisted column, so a reloaded
/// `ConfidenceResult` always has `reason: None`.
#[allow(clippy::too_many_arguments)]
fn row_to_suggestion(
    id: String,
    service_id: String,
    payload: String,
    status: String,
    confidence_score: f64,
    created_at: String,
    transcript_segment_id: Option<String>,
    source_text: Option<String>,
    confirmation_count: i64,
    rejection_echo_count: i64,
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
        transcript_segment_id: transcript_segment_id.and_then(|id| Uuid::parse_str(&id).ok()),
        source_text,
        confirmation_count: confirmation_count.max(0) as u32,
        rejection_echo_count: rejection_echo_count.max(0) as u32,
    })
}

const SUGGESTION_COLUMNS: &str = "id, service_id, payload, status, confidence_score, created_at, \
     transcript_segment_id, source_text, confirmation_count, rejection_echo_count";

#[allow(clippy::type_complexity)]
fn suggestion_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(
    String,
    String,
    String,
    String,
    f64,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

pub fn list_suggestions(
    conn: &Connection,
    service_id: Uuid,
    status: Option<SuggestionStatus>,
) -> Result<Vec<Suggestion>, PersistError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SUGGESTION_COLUMNS}
         FROM ai_suggestions WHERE service_id = ?1 AND (?2 IS NULL OR status = ?2)
         ORDER BY created_at DESC"
    ))?;
    let status_filter = status.map(suggestion_status_str);
    let rows = stmt
        .query_map(
            params![service_id.to_string(), status_filter],
            suggestion_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    rows.into_iter()
        .map(
            |(
                id,
                service_id,
                payload,
                status,
                score,
                created_at,
                seg_id,
                src,
                confirm_count,
                reject_echo_count,
            )| {
                row_to_suggestion(
                    id,
                    service_id,
                    payload,
                    status,
                    score,
                    created_at,
                    seg_id,
                    src,
                    confirm_count,
                    reject_echo_count,
                )
            },
        )
        .collect()
}

pub fn get_suggestion(conn: &Connection, suggestion_id: Uuid) -> Result<Suggestion, PersistError> {
    conn.query_row(
        &format!("SELECT {SUGGESTION_COLUMNS} FROM ai_suggestions WHERE id = ?1"),
        params![suggestion_id.to_string()],
        suggestion_row,
    )
    .optional()?
    .ok_or_else(|| PersistError::NotFound(suggestion_id.to_string()))
    .and_then(
        |(
            id,
            service_id,
            payload,
            status,
            score,
            created_at,
            seg_id,
            src,
            confirm_count,
            reject_echo_count,
        )| {
            row_to_suggestion(
                id,
                service_id,
                payload,
                status,
                score,
                created_at,
                seg_id,
                src,
                confirm_count,
                reject_echo_count,
            )
        },
    )
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
        &format!("SELECT {SUGGESTION_COLUMNS} FROM ai_suggestions WHERE id = ?1"),
        params![suggestion_id.to_string()],
        suggestion_row,
    )
    .optional()?
    .ok_or_else(|| PersistError::NotFound(suggestion_id.to_string()))
    .and_then(
        |(
            id,
            service_id,
            payload,
            status,
            score,
            created_at,
            seg_id,
            src,
            confirm_count,
            reject_echo_count,
        )| {
            row_to_suggestion(
                id,
                service_id,
                payload,
                status,
                score,
                created_at,
                seg_id,
                src,
                confirm_count,
                reject_echo_count,
            )
        },
    )
}

/// The most recently created still-`Pending` suggestion for `service_id`
/// whose `SuggestionKind::Scripture { reference }` exactly matches
/// `reference_display`, if any - the Phase 5.2 ("temporal confirmation")
/// target `pipeline::handle_final_transcript` boosts when a `Paraphrase`/
/// `Semantic` detection's reference recurs within the dedup window. Only
/// `Pending` is considered: once an operator has approved/edited/rejected
/// a suggestion, its fate is decided and a later redetection must never
/// touch it again. Reuses [`list_suggestions`] (already ordered
/// newest-first) rather than a second bespoke query - a service's pending
/// queue is never large enough for the extra `Vec` allocation to matter.
pub fn find_pending_suggestion_for_reference(
    conn: &Connection,
    service_id: Uuid,
    reference_display: &str,
) -> Result<Option<Suggestion>, PersistError> {
    let pending = list_suggestions(conn, service_id, Some(SuggestionStatus::Pending))?;
    Ok(pending.into_iter().find(|s| match &s.kind {
        SuggestionKind::Scripture { reference } => reference == reference_display,
        _ => false,
    }))
}

/// The most recently created `Rejected` suggestion for `service_id` whose
/// `SuggestionKind::Scripture { reference }` exactly matches
/// `reference_display`, if any - the Phase 5.4 ("wrong-verse feedback
/// loop") target `pipeline::handle_final_transcript` echoes when a
/// `Paraphrase`/`Semantic` detection's reference recurs within the dedup
/// window and no `Pending` suggestion exists to confirm instead. Only
/// `Rejected` is considered: an `Approved`/`Edited` suggestion's repeat is
/// left exactly as silently absorbed as it already was (the operator
/// already has what they wanted on screen; no feedback signal is missing
/// there). Reuses [`list_suggestions`] (already ordered newest-first) for
/// the same reason `find_pending_suggestion_for_reference` does.
pub fn find_rejected_suggestion_for_reference(
    conn: &Connection,
    service_id: Uuid,
    reference_display: &str,
) -> Result<Option<Suggestion>, PersistError> {
    let rejected = list_suggestions(conn, service_id, Some(SuggestionStatus::Rejected))?;
    Ok(rejected.into_iter().find(|s| match &s.kind {
        SuggestionKind::Scripture { reference } => reference == reference_display,
        _ => false,
    }))
}

/// Records that a `Rejected` suggestion's own reference was independently
/// redetected again (Phase 5.4, "wrong-verse feedback loop") - the repeat
/// itself is still silently suppressed by the caller's existing dedup
/// check exactly as before; this only makes that already-existing
/// suppression observable. Never changes `status`, `confidence`, or `kind`,
/// since a rejected suggestion is a decided suggestion and this must never
/// be the mechanism that quietly resurrects one. `rejection_echo_count` is
/// an honest, unconditionally incrementing count of how many times this
/// happened, mirroring `confirm_suggestion`'s own `confirmation_count`
/// discipline.
pub fn record_rejection_echo(
    conn: &Connection,
    suggestion_id: Uuid,
) -> Result<Suggestion, PersistError> {
    conn.execute(
        "UPDATE ai_suggestions SET rejection_echo_count = rejection_echo_count + 1 WHERE id = ?1",
        params![suggestion_id.to_string()],
    )?;
    get_suggestion(conn, suggestion_id)
}

/// Boosts a still-`Pending` suggestion's confidence because its own
/// reference was just independently redetected within the dedup window
/// (Phase 5.2, "temporal confirmation / sliding re-score") - repetition of
/// a heuristic (`Paraphrase`/`Semantic`) guess is corroborating evidence,
/// never a reason to create a second suggestion (the caller's dedup check
/// already suppresses that). The score only ever moves up, by exactly
/// `score_bonus`, and is capped at `max_score` - deliberately kept below
/// the ~0.97 an explicit citation earns, so a repeated heuristic guess can
/// never out-rank a real citation regardless of how many times it recurs.
/// `confirmation_count` increments unconditionally (even once the score
/// cap is reached), since it is an honest count of how many times this
/// happened, not a proxy for the score itself.
pub fn confirm_suggestion(
    conn: &Connection,
    suggestion_id: Uuid,
    score_bonus: f32,
    max_score: f32,
) -> Result<Suggestion, PersistError> {
    let current = get_suggestion(conn, suggestion_id)?;
    let new_score = (current.confidence.score + score_bonus)
        .min(max_score)
        .max(current.confidence.score);
    let new_level = ConfidenceLevel::from_score(new_score);
    let new_count = current.confirmation_count + 1;
    conn.execute(
        "UPDATE ai_suggestions
         SET confidence_score = ?1, confidence_level = ?2, confirmation_count = ?3
         WHERE id = ?4",
        params![
            new_score,
            confidence_level_str(new_level),
            new_count,
            suggestion_id.to_string(),
        ],
    )?;
    get_suggestion(conn, suggestion_id)
}

// --- presentation_items ---------------------------------------------------

fn presentation_item_status_str(
    status: cip_core_presentation::PresentationItemStatus,
) -> &'static str {
    use cip_core_presentation::PresentationItemStatus;
    match status {
        PresentationItemStatus::Prepared => "prepared",
        PresentationItemStatus::Active => "active",
        PresentationItemStatus::Stopped => "stopped",
    }
}

fn parse_presentation_item_status(status: &str) -> cip_core_presentation::PresentationItemStatus {
    use cip_core_presentation::PresentationItemStatus;
    match status {
        "active" => PresentationItemStatus::Active,
        "stopped" => PresentationItemStatus::Stopped,
        _ => PresentationItemStatus::Prepared,
    }
}

/// Persist a prepared presentation item (`status = 'prepared'`, matching
/// `PresentationItemStatus::Prepared`). Preparing never writes `'active'`
/// directly - only an explicit, later `display_presentation` command does
/// that (see `commands.rs`'s docs and `update_presentation_item_status`
/// below).
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
        "INSERT INTO presentation_items
            (id, service_id, content_type, content, status, created_at, source_suggestion_id, template)
         VALUES (?1, ?2, ?3, ?4, 'prepared', ?5, ?6, ?7)",
        params![
            item.id.to_string(),
            item.service_id.to_string(),
            content_type,
            content,
            item.created_at.to_rfc3339(),
            item.source_suggestion_id.map(|id| id.to_string()),
            item.template,
        ],
    )?;
    Ok(())
}

const PRESENTATION_ITEM_COLUMNS: &str =
    "id, service_id, content, status, created_at, source_suggestion_id, template";

#[allow(clippy::type_complexity)]
fn presentation_item_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

fn row_to_presentation_item(
    id: String,
    service_id: String,
    content: String,
    status: String,
    created_at: String,
    source_suggestion_id: Option<String>,
    template: Option<String>,
) -> Result<cip_core_presentation::PresentationItem, PersistError> {
    Ok(cip_core_presentation::PresentationItem {
        id: Uuid::parse_str(&id).map_err(|_| PersistError::NotFound(id.clone()))?,
        service_id: Uuid::parse_str(&service_id)
            .map_err(|_| PersistError::NotFound(service_id.clone()))?,
        content: serde_json::from_str(&content)?,
        status: parse_presentation_item_status(&status),
        created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        source_suggestion_id: source_suggestion_id.and_then(|id| Uuid::parse_str(&id).ok()),
        template,
    })
}

pub fn get_presentation_item(
    conn: &Connection,
    item_id: Uuid,
) -> Result<cip_core_presentation::PresentationItem, PersistError> {
    conn.query_row(
        &format!("SELECT {PRESENTATION_ITEM_COLUMNS} FROM presentation_items WHERE id = ?1"),
        params![item_id.to_string()],
        presentation_item_row,
    )
    .optional()?
    .ok_or_else(|| PersistError::NotFound(item_id.to_string()))
    .and_then(
        |(id, service_id, content, status, created_at, src, template)| {
            row_to_presentation_item(id, service_id, content, status, created_at, src, template)
        },
    )
}

/// List presentation items for a service, most recent first. `status`
/// filters to a single status when set (e.g. `Prepared` for "what's
/// currently prepared").
pub fn list_presentation_items(
    conn: &Connection,
    service_id: Uuid,
    status: Option<cip_core_presentation::PresentationItemStatus>,
) -> Result<Vec<cip_core_presentation::PresentationItem>, PersistError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PRESENTATION_ITEM_COLUMNS}
         FROM presentation_items WHERE service_id = ?1 AND (?2 IS NULL OR status = ?2)
         ORDER BY created_at DESC"
    ))?;
    let status_filter = status.map(presentation_item_status_str);
    let rows = stmt
        .query_map(
            params![service_id.to_string(), status_filter],
            presentation_item_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    rows.into_iter()
        .map(
            |(id, service_id, content, status, created_at, src, template)| {
                row_to_presentation_item(id, service_id, content, status, created_at, src, template)
            },
        )
        .collect()
}

/// Update a presentation item's status - cancelling a prepared item
/// (`Stopped`, "prepared then retracted"), activating one for real local
/// display (`Active`, see `commands::display_presentation`), or stopping
/// an active one (`Stopped`, see `commands::clear_presentation_display`).
/// Returns the updated row.
pub fn update_presentation_item_status(
    conn: &Connection,
    item_id: Uuid,
    new_status: cip_core_presentation::PresentationItemStatus,
) -> Result<cip_core_presentation::PresentationItem, PersistError> {
    conn.execute(
        "UPDATE presentation_items SET status = ?1 WHERE id = ?2",
        params![
            presentation_item_status_str(new_status),
            item_id.to_string()
        ],
    )?;
    get_presentation_item(conn, item_id)
}

/// Startup safety sweep (spec: "restart must never automatically
/// project"): any presentation item still `Active` from a previous,
/// uncleanly-ended run is reconciled to `Stopped` before the app manages
/// any state or opens any window. Never re-opens a display, never re-reads
/// which item it was - the whole point is that nothing downstream ever
/// sees a leftover `Active` row and treats it as "still showing." Returns
/// how many rows were reconciled, for a one-line startup log.
pub fn reconcile_stale_active_presentation_items(conn: &Connection) -> Result<usize, PersistError> {
    let affected = conn.execute(
        "UPDATE presentation_items SET status = 'stopped' WHERE status = 'active'",
        [],
    )?;
    Ok(affected)
}

// --- sermon foundation (Phase 2.5, per the authoritative Phase 2 roadmap) --
//
// Mirrors `services`' own persist/update/get shape exactly - see
// `docs/sermon-foundation.md`'s "Persistence decision" section for why a
// `Sermon` is durably persisted (service restart recovery / history /
// auditability) while a Sermon's own *finding* production is not
// (findings stay in `AppState.intelligence_findings`, unchanged).

fn sermon_status_str(status: cip_core_sermon::foundation::SermonStatus) -> &'static str {
    use cip_core_sermon::foundation::SermonStatus;
    match status {
        SermonStatus::Planned => "planned",
        SermonStatus::Active => "active",
        SermonStatus::Paused => "paused",
        SermonStatus::Ended => "ended",
        SermonStatus::Cancelled => "cancelled",
    }
}

fn parse_sermon_status(value: &str) -> cip_core_sermon::foundation::SermonStatus {
    use cip_core_sermon::foundation::SermonStatus;
    match value {
        "planned" => SermonStatus::Planned,
        "paused" => SermonStatus::Paused,
        "ended" => SermonStatus::Ended,
        "cancelled" => SermonStatus::Cancelled,
        _ => SermonStatus::Active,
    }
}

fn speaker_role_str(role: cip_core_sermon::foundation::SpeakerRole) -> &'static str {
    use cip_core_sermon::foundation::SpeakerRole;
    match role {
        SpeakerRole::Primary => "primary",
        SpeakerRole::Guest => "guest",
    }
}

fn parse_speaker_role(value: &str) -> cip_core_sermon::foundation::SpeakerRole {
    use cip_core_sermon::foundation::SpeakerRole;
    match value {
        "guest" => SpeakerRole::Guest,
        _ => SpeakerRole::Primary,
    }
}

fn parse_rfc3339(value: &str) -> DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

pub fn persist_sermon(
    conn: &Connection,
    sermon: &cip_core_sermon::foundation::Sermon,
) -> Result<(), PersistError> {
    conn.execute(
        "INSERT INTO sermons
            (id, service_id, title, speaker_id, speaker_name, speaker_role,
             status, started_at, ended_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            sermon.id.to_string(),
            sermon.service_id.to_string(),
            sermon.title,
            sermon.speaker.as_ref().map(|s| s.id.to_string()),
            sermon.speaker.as_ref().map(|s| s.name.clone()),
            sermon.speaker.as_ref().map(|s| speaker_role_str(s.role)),
            sermon_status_str(sermon.status),
            sermon.started_at.map(|t| t.to_rfc3339()),
            sermon.ended_at.map(|t| t.to_rfc3339()),
            sermon.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// Persists every field a mutating sermon-foundation operator action can
/// change (status/title/speaker/timestamps) in one statement, since
/// `Sermon` is small and every such action already has the full,
/// up-to-date value in hand - avoids five near-identical single-column
/// update functions for no real benefit.
pub fn update_sermon(
    conn: &Connection,
    sermon: &cip_core_sermon::foundation::Sermon,
) -> Result<(), PersistError> {
    let rows = conn.execute(
        "UPDATE sermons SET title = ?1, speaker_id = ?2, speaker_name = ?3, speaker_role = ?4,
             status = ?5, started_at = ?6, ended_at = ?7 WHERE id = ?8",
        params![
            sermon.title,
            sermon.speaker.as_ref().map(|s| s.id.to_string()),
            sermon.speaker.as_ref().map(|s| s.name.clone()),
            sermon.speaker.as_ref().map(|s| speaker_role_str(s.role)),
            sermon_status_str(sermon.status),
            sermon.started_at.map(|t| t.to_rfc3339()),
            sermon.ended_at.map(|t| t.to_rfc3339()),
            sermon.id.to_string(),
        ],
    )?;
    if rows == 0 {
        return Err(PersistError::NotFound(sermon.id.to_string()));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn row_to_sermon(
    id: String,
    service_id: String,
    title: Option<String>,
    speaker_id: Option<String>,
    speaker_name: Option<String>,
    speaker_role: Option<String>,
    status: String,
    started_at: Option<String>,
    ended_at: Option<String>,
    created_at: String,
) -> Result<cip_core_sermon::foundation::Sermon, PersistError> {
    let speaker = match (speaker_id, speaker_name) {
        (Some(id), Some(name)) => Some(cip_core_sermon::foundation::Speaker {
            id: Uuid::parse_str(&id).map_err(|_| PersistError::NotFound(id.clone()))?,
            name,
            role: parse_speaker_role(speaker_role.as_deref().unwrap_or("primary")),
        }),
        _ => None,
    };
    Ok(cip_core_sermon::foundation::Sermon {
        id: Uuid::parse_str(&id).map_err(|_| PersistError::NotFound(id.clone()))?,
        service_id: Uuid::parse_str(&service_id)
            .map_err(|_| PersistError::NotFound(service_id.clone()))?,
        title,
        speaker,
        status: parse_sermon_status(&status),
        started_at: started_at.as_deref().map(parse_rfc3339),
        ended_at: ended_at.as_deref().map(parse_rfc3339),
        created_at: parse_rfc3339(&created_at),
    })
}

pub fn get_sermon(
    conn: &Connection,
    sermon_id: Uuid,
) -> Result<cip_core_sermon::foundation::Sermon, PersistError> {
    conn.query_row(
        "SELECT id, service_id, title, speaker_id, speaker_name, speaker_role,
                status, started_at, ended_at, created_at
         FROM sermons WHERE id = ?1",
        params![sermon_id.to_string()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, String>(9)?,
            ))
        },
    )
    .optional()?
    .ok_or_else(|| PersistError::NotFound(sermon_id.to_string()))
    .and_then(
        |(id, sid, title, spid, spname, sprole, status, started, ended, created)| {
            row_to_sermon(
                id, sid, title, spid, spname, sprole, status, started, ended, created,
            )
        },
    )
}

/// Sermons for a service, most recently created first, bounded by
/// `limit` - the sermon-history counterpart to `list_services`.
pub fn list_sermons_for_service(
    conn: &Connection,
    service_id: Uuid,
    limit: u32,
) -> Result<Vec<cip_core_sermon::foundation::Sermon>, PersistError> {
    let mut stmt = conn.prepare(
        "SELECT id, service_id, title, speaker_id, speaker_name, speaker_role,
                status, started_at, ended_at, created_at
         FROM sermons WHERE service_id = ?1 ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![service_id.to_string(), limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, String>(9)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(id, sid, title, spid, spname, sprole, status, started, ended, created)| {
                row_to_sermon(
                    id, sid, title, spid, spname, sprole, status, started, ended, created,
                )
            },
        )
        .collect()
}

fn section_kind_str(kind: cip_core_sermon::foundation::SermonSectionKind) -> &'static str {
    use cip_core_sermon::foundation::SermonSectionKind;
    match kind {
        SermonSectionKind::Introduction => "introduction",
        SermonSectionKind::ScriptureReading => "scripture_reading",
        SermonSectionKind::MainMessage => "main_message",
        SermonSectionKind::Illustration => "illustration",
        SermonSectionKind::Prayer => "prayer",
        SermonSectionKind::AltarCall => "altar_call",
        SermonSectionKind::Conclusion => "conclusion",
    }
}

fn parse_section_kind(value: &str) -> cip_core_sermon::foundation::SermonSectionKind {
    use cip_core_sermon::foundation::SermonSectionKind;
    match value {
        "scripture_reading" => SermonSectionKind::ScriptureReading,
        "main_message" => SermonSectionKind::MainMessage,
        "illustration" => SermonSectionKind::Illustration,
        "prayer" => SermonSectionKind::Prayer,
        "altar_call" => SermonSectionKind::AltarCall,
        "conclusion" => SermonSectionKind::Conclusion,
        _ => SermonSectionKind::Introduction,
    }
}

fn section_origin_str(origin: cip_core_sermon::foundation::SectionOrigin) -> &'static str {
    use cip_core_sermon::foundation::SectionOrigin;
    match origin {
        SectionOrigin::OperatorAssigned => "operator_assigned",
        SectionOrigin::SystemBoundary => "system_boundary",
        SectionOrigin::Inferred => "inferred",
    }
}

fn parse_section_origin(value: &str) -> cip_core_sermon::foundation::SectionOrigin {
    use cip_core_sermon::foundation::SectionOrigin;
    match value {
        "system_boundary" => SectionOrigin::SystemBoundary,
        "inferred" => SectionOrigin::Inferred,
        _ => SectionOrigin::OperatorAssigned,
    }
}

pub fn persist_sermon_section(
    conn: &Connection,
    section: &cip_core_sermon::foundation::SermonSection,
) -> Result<(), PersistError> {
    conn.execute(
        "INSERT INTO sermon_sections (id, sermon_id, kind, origin, started_at, ended_at, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            section.id.to_string(),
            section.sermon_id.to_string(),
            section_kind_str(section.kind),
            section_origin_str(section.origin),
            section.started_at.to_rfc3339(),
            section.ended_at.map(|t| t.to_rfc3339()),
            section.note,
        ],
    )?;
    Ok(())
}

/// Closes the still-open section (`ended_at IS NULL`) for a sermon, if
/// any - the persistence half of "a new active section closes the
/// previous one" (spec's message-section-state rule). A no-op (not an
/// error) when no section is currently open.
pub fn close_open_sermon_section(
    conn: &Connection,
    sermon_id: Uuid,
    ended_at: DateTime<Utc>,
) -> Result<(), PersistError> {
    conn.execute(
        "UPDATE sermon_sections SET ended_at = ?1 WHERE sermon_id = ?2 AND ended_at IS NULL",
        params![ended_at.to_rfc3339(), sermon_id.to_string()],
    )?;
    Ok(())
}

fn row_to_section(
    id: String,
    sermon_id: String,
    kind: String,
    origin: String,
    started_at: String,
    ended_at: Option<String>,
    note: Option<String>,
) -> Result<cip_core_sermon::foundation::SermonSection, PersistError> {
    Ok(cip_core_sermon::foundation::SermonSection {
        id: Uuid::parse_str(&id).map_err(|_| PersistError::NotFound(id.clone()))?,
        sermon_id: Uuid::parse_str(&sermon_id)
            .map_err(|_| PersistError::NotFound(sermon_id.clone()))?,
        kind: parse_section_kind(&kind),
        origin: parse_section_origin(&origin),
        started_at: parse_rfc3339(&started_at),
        ended_at: ended_at.as_deref().map(parse_rfc3339),
        note,
    })
}

/// All sections for a sermon, in the order they were opened - never
/// deletes or overwrites an earlier section (spec's "do not delete
/// previous section history").
pub fn list_sermon_sections(
    conn: &Connection,
    sermon_id: Uuid,
) -> Result<Vec<cip_core_sermon::foundation::SermonSection>, PersistError> {
    let mut stmt = conn.prepare(
        "SELECT id, sermon_id, kind, origin, started_at, ended_at, note
         FROM sermon_sections WHERE sermon_id = ?1 ORDER BY started_at ASC",
    )?;
    let rows = stmt
        .query_map(params![sermon_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, sid, kind, origin, started, ended, note)| {
            row_to_section(id, sid, kind, origin, started, ended, note)
        })
        .collect()
}

pub fn persist_sermon_segment(
    conn: &Connection,
    segment: &cip_core_sermon::foundation::SermonSegment,
) -> Result<(), PersistError> {
    conn.execute(
        "INSERT INTO sermon_segments (id, sermon_id, transcript_segment_id, sequence, section_id, linked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            segment.id.to_string(),
            segment.sermon_id.to_string(),
            segment.transcript_segment_id.to_string(),
            segment.sequence,
            segment.section_id.map(|id| id.to_string()),
            segment.linked_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// How many transcript segments are already linked to this sermon - the
/// source of truth for a newly linked segment's next `sequence` number
/// (gapless, starting at 0 for a given sermon).
pub fn count_sermon_segments(conn: &Connection, sermon_id: Uuid) -> Result<u32, PersistError> {
    conn.query_row(
        "SELECT count(*) FROM sermon_segments WHERE sermon_id = ?1",
        params![sermon_id.to_string()],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n as u32)
    .map_err(PersistError::from)
}

pub fn list_sermon_segments(
    conn: &Connection,
    sermon_id: Uuid,
) -> Result<Vec<cip_core_sermon::foundation::SermonSegment>, PersistError> {
    let mut stmt = conn.prepare(
        "SELECT id, sermon_id, transcript_segment_id, sequence, section_id, linked_at
         FROM sermon_segments WHERE sermon_id = ?1 ORDER BY sequence ASC",
    )?;
    let rows = stmt
        .query_map(params![sermon_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, sid, tid, sequence, section_id, linked_at)| {
            Ok(cip_core_sermon::foundation::SermonSegment {
                id: Uuid::parse_str(&id).map_err(|_| PersistError::NotFound(id.clone()))?,
                sermon_id: Uuid::parse_str(&sid)
                    .map_err(|_| PersistError::NotFound(sid.clone()))?,
                transcript_segment_id: Uuid::parse_str(&tid)
                    .map_err(|_| PersistError::NotFound(tid.clone()))?,
                sequence: sequence as u32,
                section_id: section_id
                    .map(|s| Uuid::parse_str(&s).map_err(|_| PersistError::NotFound(s.clone())))
                    .transpose()?,
                linked_at: parse_rfc3339(&linked_at),
            })
        })
        .collect()
}

/// The service a transcript segment belongs to, if it exists at all -
/// `link_transcript_segment_to_sermon`'s ownership guard (spec's
/// "transcript segment attached to a different service" boundary).
pub fn get_transcript_segment_service_id(
    conn: &Connection,
    transcript_segment_id: Uuid,
) -> Result<Option<Uuid>, PersistError> {
    conn.query_row(
        "SELECT service_id FROM transcript_segments WHERE id = ?1",
        params![transcript_segment_id.to_string()],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|s| Uuid::parse_str(&s).map_err(|_| PersistError::NotFound(s.clone())))
    .transpose()
}

// --- saved scriptures (Phase 3.6: Church Knowledge Libraries) --------------

/// A church-wide, cross-service Scripture bookmark - see
/// `database/migrations/0010_saved_scriptures.sql` for why this is a
/// standalone table rather than reusing `scripture_detections`/
/// `ai_suggestions`/`presentation_items` (all of them are service-scoped,
/// one-shot records; this is neither).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedScripture {
    pub id: Uuid,
    pub translation_id: String,
    pub book: String,
    pub chapter: u32,
    pub verse_start: u32,
    pub verse_end: Option<u32>,
    pub reference_display: String,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[allow(clippy::type_complexity)]
fn saved_scripture_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(
    String,
    String,
    String,
    u32,
    u32,
    Option<u32>,
    String,
    Option<String>,
    String,
)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

#[allow(clippy::too_many_arguments)]
fn row_to_saved_scripture(
    id: String,
    translation_id: String,
    book: String,
    chapter: u32,
    verse_start: u32,
    verse_end: Option<u32>,
    reference_display: String,
    note: Option<String>,
    created_at: String,
) -> Result<SavedScripture, PersistError> {
    Ok(SavedScripture {
        id: Uuid::parse_str(&id).map_err(|_| PersistError::NotFound(id))?,
        translation_id,
        book,
        chapter,
        verse_start,
        verse_end,
        reference_display,
        note,
        created_at: parse_rfc3339(&created_at),
    })
}

const SAVED_SCRIPTURE_COLUMNS: &str =
    "id, translation_id, book, chapter, verse_start, verse_end, reference_display, note, created_at";

/// Saves a Scripture reference for later reuse. `note` is an optional
/// operator-written label (e.g. "Baptism series"); free text, never
/// interpreted.
#[allow(clippy::too_many_arguments)]
pub fn persist_saved_scripture(
    conn: &Connection,
    id: Uuid,
    translation_id: &str,
    book: &str,
    chapter: u32,
    verse_start: u32,
    verse_end: Option<u32>,
    reference_display: &str,
    note: Option<&str>,
) -> Result<SavedScripture, PersistError> {
    let created_at = Utc::now();
    conn.execute(
        "INSERT INTO saved_scriptures
            (id, translation_id, book, chapter, verse_start, verse_end, reference_display, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id.to_string(),
            translation_id,
            book,
            chapter,
            verse_start,
            verse_end,
            reference_display,
            note,
            created_at.to_rfc3339(),
        ],
    )?;
    Ok(SavedScripture {
        id,
        translation_id: translation_id.to_string(),
        book: book.to_string(),
        chapter,
        verse_start,
        verse_end,
        reference_display: reference_display.to_string(),
        note: note.map(str::to_string),
        created_at,
    })
}

/// Every saved scripture, most recently saved first.
pub fn list_saved_scriptures(conn: &Connection) -> Result<Vec<SavedScripture>, PersistError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SAVED_SCRIPTURE_COLUMNS} FROM saved_scriptures ORDER BY created_at DESC"
    ))?;
    let rows = stmt
        .query_map([], saved_scripture_row)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, t, b, c, vs, ve, rd, note, created_at)| {
            row_to_saved_scripture(id, t, b, c, vs, ve, rd, note, created_at)
        })
        .collect()
}

/// Deletes a saved scripture. Idempotent-safe from the caller's
/// perspective (returns whether a row actually existed) rather than
/// erroring on a double-delete - matches `stop_active_item`'s "safe and
/// idempotent" discipline for operator-facing cleanup actions.
pub fn delete_saved_scripture(conn: &Connection, id: Uuid) -> Result<bool, PersistError> {
    let affected = conn.execute(
        "DELETE FROM saved_scriptures WHERE id = ?1",
        params![id.to_string()],
    )?;
    Ok(affected > 0)
}

// --- saved content candidates (Phase 2.7.1) --------------------------------

/// Persists a durable copy of an already-accepted `ContentCandidate` -
/// see `database/migrations/0011_saved_content_candidates.sql` for why
/// this table exists (the in-memory `ContentCandidateQueue` alone does not
/// survive a service ending or an application restart). Called exactly
/// once, at the moment an operator accepts a candidate - never on mere
/// detection or review.
pub fn persist_saved_content_candidate(
    conn: &Connection,
    candidate: &cip_core_intelligence::ContentCandidate,
) -> Result<(), PersistError> {
    let payload = serde_json::to_string(candidate)?;
    conn.execute(
        "INSERT INTO saved_content_candidates (id, service_id, candidate_type, payload, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            candidate.id.to_string(),
            candidate.service_id.to_string(),
            candidate.candidate_type.label(),
            payload,
            candidate.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// Every saved content candidate for one service, most recently saved
/// first - mirrors `list_presentation_items`'s existing
/// service-scoped-but-never-tied-to-the-live-session shape, so it works
/// identically whether that service is still active or long since ended.
pub fn list_saved_content_candidates_for_service(
    conn: &Connection,
    service_id: Uuid,
) -> Result<Vec<cip_core_intelligence::ContentCandidate>, PersistError> {
    let mut stmt = conn.prepare(
        "SELECT payload FROM saved_content_candidates WHERE service_id = ?1 ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map(params![service_id.to_string()], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|payload| serde_json::from_str(&payload).map_err(PersistError::from))
        .collect()
}

// --- saved sermon findings (Phase 13: Church Knowledge Base) --------------

/// Persist an accepted Sermon Intelligence finding - see
/// `0018_saved_sermon_findings.sql` and `docs/phase-13-audit.md` for why
/// this exists and why it is only ever called from `accept_sermon_finding`,
/// never from detection. `element_label` is derived by the caller (see
/// `sermon_knowledge_base::element_label_for_summary`) so this function
/// stays a plain, untested-by-itself write, exactly like
/// `persist_saved_content_candidate`.
pub fn persist_saved_sermon_finding(
    conn: &Connection,
    finding: &cip_core_intelligence::IntelligenceFinding,
    element_label: &str,
) -> Result<(), PersistError> {
    let payload = serde_json::to_string(finding)?;
    conn.execute(
        "INSERT INTO saved_sermon_findings
            (id, service_id, sermon_id, element_label, summary, payload, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            finding.id.to_string(),
            finding.service_id.to_string(),
            finding.sermon_id.map(|id| id.to_string()),
            element_label,
            finding.summary,
            payload,
            finding.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// Every saved sermon finding across every service, most recently saved
/// first - the cross-sermon-analytics counterpart to
/// `list_saved_content_candidates_for_service`, deliberately unscoped by
/// service since a church knowledge base spans services by definition.
/// Bounded by `limit`, generous enough to cover years of weekly services
/// without being genuinely unbounded, for the same reason `harvest.rs`'s
/// reads are bounded.
pub fn list_all_saved_sermon_findings(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<cip_core_intelligence::IntelligenceFinding>, PersistError> {
    let mut stmt = conn
        .prepare("SELECT payload FROM saved_sermon_findings ORDER BY created_at DESC LIMIT ?1")?;
    let rows = stmt
        .query_map(params![limit], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|payload| serde_json::from_str(&payload).map_err(PersistError::from))
        .collect()
}

/// Every sermon across every service, most recently created first - the
/// cross-sermon-analytics counterpart to `list_sermons_for_service`.
/// Bounded by `limit`, mirroring `list_all_saved_sermon_findings`.
pub fn list_sermons(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<cip_core_sermon::foundation::Sermon>, PersistError> {
    let mut stmt = conn.prepare(
        "SELECT id, service_id, title, speaker_id, speaker_name, speaker_role,
                status, started_at, ended_at, created_at
         FROM sermons ORDER BY created_at DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, String>(9)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(id, sid, title, spid, spname, sprole, status, started, ended, created)| {
                row_to_sermon(
                    id, sid, title, spid, spname, sprole, status, started, ended, created,
                )
            },
        )
        .collect()
}

// --- display role assignments (Phase 3.10.2) -------------------------------

/// Assigns `role` to `monitor_id`, replacing any prior assignment for
/// that monitor - see `0012_display_role_assignments.sql` for why this
/// is the one upsert table in this schema (a role assignment has
/// "current value" semantics, not "one row per event").
pub fn assign_display_role(
    conn: &Connection,
    monitor_id: &str,
    role: DisplayRole,
) -> Result<(), PersistError> {
    conn.execute(
        "INSERT INTO display_role_assignments (monitor_id, role, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(monitor_id) DO UPDATE SET role = excluded.role, updated_at = excluded.updated_at",
        params![monitor_id, role.as_str(), Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

/// Every persisted display role assignment, keyed by `monitor_id` - the
/// input `display_registry::merge_displays` combines with a live monitor
/// enumeration to produce the full Display Registry.
pub fn list_display_role_assignments(
    conn: &Connection,
) -> Result<HashMap<String, DisplayRole>, PersistError> {
    let mut stmt = conn.prepare("SELECT monitor_id, role FROM display_role_assignments")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(monitor_id, role)| DisplayRole::parse(&role).map(|r| (monitor_id, r)))
        .collect())
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
    fn round_trips_a_presentation_item_with_source_and_template() {
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
        let suggestion_id = suggestion.id;
        let item = cip_core_presentation::PresentationItem::prepare(
            session.id,
            cip_core_presentation::PresentationContent::Scripture {
                reference: "ROM 8:28".into(),
                translation_id: "KJV".into(),
                text: "And we know...".into(),
            },
        )
        .with_source_suggestion(suggestion_id)
        .with_template("SCRIPTURE_DEFAULT");
        persist_presentation_item(&conn, &item).unwrap();

        let loaded = get_presentation_item(&conn, item.id).unwrap();
        assert_eq!(loaded, item);
        assert_eq!(loaded.source_suggestion_id, Some(suggestion_id));
        assert_eq!(loaded.template.as_deref(), Some("SCRIPTURE_DEFAULT"));
    }

    #[test]
    fn round_trips_a_manually_created_presentation_item_without_source() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let item = cip_core_presentation::PresentationItem::prepare(
            session.id,
            cip_core_presentation::PresentationContent::Scripture {
                reference: "JHN 3:16".into(),
                translation_id: "KJV".into(),
                text: "For God so loved the world...".into(),
            },
        );
        persist_presentation_item(&conn, &item).unwrap();

        let loaded = get_presentation_item(&conn, item.id).unwrap();
        assert_eq!(loaded.source_suggestion_id, None);
        assert_eq!(loaded.template, None);
    }

    #[test]
    fn get_presentation_item_returns_not_found_for_unknown_id() {
        let conn = migrated_conn();
        let err = get_presentation_item(&conn, Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, PersistError::NotFound(_)));
    }

    #[test]
    fn lists_presentation_items_for_a_service_most_recent_first_and_filters_by_status() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let first = cip_core_presentation::PresentationItem::prepare(
            session.id,
            cip_core_presentation::PresentationContent::Text {
                title: None,
                body: "first".into(),
            },
        );
        persist_presentation_item(&conn, &first).unwrap();
        let second = cip_core_presentation::PresentationItem::prepare(
            session.id,
            cip_core_presentation::PresentationContent::Text {
                title: None,
                body: "second".into(),
            },
        );
        persist_presentation_item(&conn, &second).unwrap();

        let all = list_presentation_items(&conn, session.id, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, second.id, "expected most recent item first");

        update_presentation_item_status(
            &conn,
            first.id,
            cip_core_presentation::PresentationItemStatus::Stopped,
        )
        .unwrap();

        let only_prepared = list_presentation_items(
            &conn,
            session.id,
            Some(cip_core_presentation::PresentationItemStatus::Prepared),
        )
        .unwrap();
        assert_eq!(only_prepared.len(), 1);
        assert_eq!(only_prepared[0].id, second.id);
    }

    #[test]
    fn update_presentation_item_status_persists_the_new_status() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let item = cip_core_presentation::PresentationItem::prepare(
            session.id,
            cip_core_presentation::PresentationContent::Text {
                title: None,
                body: "welcome".into(),
            },
        );
        persist_presentation_item(&conn, &item).unwrap();

        let updated = update_presentation_item_status(
            &conn,
            item.id,
            cip_core_presentation::PresentationItemStatus::Stopped,
        )
        .unwrap();
        assert_eq!(
            updated.status,
            cip_core_presentation::PresentationItemStatus::Stopped
        );
    }

    #[test]
    fn update_presentation_item_status_can_activate_a_prepared_item() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let item = cip_core_presentation::PresentationItem::prepare(
            session.id,
            cip_core_presentation::PresentationContent::Text {
                title: None,
                body: "welcome".into(),
            },
        );
        persist_presentation_item(&conn, &item).unwrap();

        let activated = update_presentation_item_status(
            &conn,
            item.id,
            cip_core_presentation::PresentationItemStatus::Active,
        )
        .unwrap();
        assert_eq!(
            activated.status,
            cip_core_presentation::PresentationItemStatus::Active
        );
    }

    #[test]
    fn reconcile_stale_active_presentation_items_stops_every_active_row_and_leaves_others_untouched(
    ) {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let active = cip_core_presentation::PresentationItem::prepare(
            session.id,
            cip_core_presentation::PresentationContent::Text {
                title: None,
                body: "left active from a previous run".into(),
            },
        );
        persist_presentation_item(&conn, &active).unwrap();
        update_presentation_item_status(
            &conn,
            active.id,
            cip_core_presentation::PresentationItemStatus::Active,
        )
        .unwrap();

        let prepared = cip_core_presentation::PresentationItem::prepare(
            session.id,
            cip_core_presentation::PresentationContent::Text {
                title: None,
                body: "still legitimately prepared".into(),
            },
        );
        persist_presentation_item(&conn, &prepared).unwrap();

        let affected = reconcile_stale_active_presentation_items(&conn).unwrap();
        assert_eq!(affected, 1);

        assert_eq!(
            get_presentation_item(&conn, active.id).unwrap().status,
            cip_core_presentation::PresentationItemStatus::Stopped
        );
        assert_eq!(
            get_presentation_item(&conn, prepared.id).unwrap().status,
            cip_core_presentation::PresentationItemStatus::Prepared
        );
    }

    #[test]
    fn reconcile_stale_active_presentation_items_is_a_safe_no_op_when_nothing_is_active() {
        let conn = migrated_conn();
        assert_eq!(reconcile_stale_active_presentation_items(&conn).unwrap(), 0);
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

    #[test]
    fn suggestion_transcript_source_round_trips_through_persistence() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let segment = sample_transcript_segment("Look at verse 28.", 0);
        let segment_id = segment.id;
        persist_transcript_segment(&conn, session.id, &segment).unwrap();
        let suggestion = Suggestion::new(
            session.id,
            SuggestionKind::Scripture {
                reference: "ROM 8:28".into(),
            },
            ConfidenceResult::new(0.95, ConfidenceSource::Heuristic, None),
        )
        .with_source(segment_id, "Look at verse 28.");
        persist_suggestion(&conn, &suggestion).unwrap();

        let loaded = get_suggestion(&conn, suggestion.id).unwrap();
        assert_eq!(loaded.transcript_segment_id, Some(segment_id));
        assert_eq!(loaded.source_text.as_deref(), Some("Look at verse 28."));
    }

    #[test]
    fn get_service_and_list_services_find_a_completed_service() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        update_service_status(&conn, session.id, ServiceStatus::Ended, Some(Utc::now())).unwrap();

        let fetched = get_service(&conn, session.id).unwrap();
        assert_eq!(fetched.status, ServiceStatus::Ended);

        let history = list_services(&conn, Some(ServiceStatus::Ended), 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, session.id);
    }

    #[test]
    fn get_service_reports_not_found_for_an_unknown_id() {
        let conn = migrated_conn();
        assert!(matches!(
            get_service(&conn, Uuid::new_v4()),
            Err(PersistError::NotFound(_))
        ));
    }

    /// A small helper for the `has_recent_detection_for_reference` tests
    /// below - builds a validated, reference-bearing detection of the
    /// given kind for "ROM 8:28" with unremarkable placeholder fields.
    fn rom_8_28_detection(kind: ReferenceKind) -> ScriptureDetection {
        ScriptureDetection {
            kind,
            reference: Some(ScriptureReference::single("KJV", "ROM", 8, 28)),
            context: None,
            candidates: vec![],
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Heuristic, None),
            raw_text: "Romans 8:28".into(),
        }
    }

    #[test]
    fn recent_same_category_detection_is_a_duplicate_within_the_window_and_not_after() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let earlier_segment = sample_transcript_segment("Romans 8:28", 0);
        let earlier_segment_id = earlier_segment.id;
        persist_transcript_segment(&conn, session.id, &earlier_segment).unwrap();
        persist_scripture_detection(
            &conn,
            session.id,
            Some(earlier_segment_id),
            "KJV",
            &rom_8_28_detection(ReferenceKind::Direct),
        )
        .unwrap();

        let current_segment = sample_transcript_segment("Romans 8:28 again", 1);
        let current_segment_id = current_segment.id;
        persist_transcript_segment(&conn, session.id, &current_segment).unwrap();
        assert!(has_recent_detection_for_reference(
            &conn,
            session.id,
            "ROM 8:28",
            DetectionCategory::Explicit,
            60,
            current_segment_id,
        )
        .unwrap());
        // A different reference in the same service is not a duplicate.
        assert!(!has_recent_detection_for_reference(
            &conn,
            session.id,
            "ROM 8:31",
            DetectionCategory::Explicit,
            60,
            current_segment_id,
        )
        .unwrap());
        // A window of -1 seconds excludes even a detection from "now" once
        // any time at all has elapsed since `detected_at` was recorded.
        assert!(!has_recent_detection_for_reference(
            &conn,
            session.id,
            "ROM 8:28",
            DetectionCategory::Explicit,
            -1,
            current_segment_id,
        )
        .unwrap());
    }

    /// The Phase 4.1 case: an explicit citation is never suppressed just
    /// because a `Paraphrase` guess for the same verse was already made
    /// moments earlier - only a repeat *within the same category* is a
    /// duplicate.
    #[test]
    fn an_explicit_citation_is_not_suppressed_by_a_recent_paraphrase_for_the_same_verse() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);

        let paraphrase_segment =
            sample_transcript_segment("And we know that all things work together for good.", 0);
        let paraphrase_segment_id = paraphrase_segment.id;
        persist_transcript_segment(&conn, session.id, &paraphrase_segment).unwrap();
        persist_scripture_detection(
            &conn,
            session.id,
            Some(paraphrase_segment_id),
            "KJV",
            &rom_8_28_detection(ReferenceKind::Paraphrase),
        )
        .unwrap();

        let explicit_segment = sample_transcript_segment("Look at verse twenty-eight", 1);
        let explicit_segment_id = explicit_segment.id;
        persist_transcript_segment(&conn, session.id, &explicit_segment).unwrap();
        assert!(
            !has_recent_detection_for_reference(
                &conn,
                session.id,
                "ROM 8:28",
                DetectionCategory::Explicit,
                60,
                explicit_segment_id,
            )
            .unwrap(),
            "an explicit citation must never be suppressed by a recent Paraphrase guess for the same verse"
        );
    }

    /// The reverse of the case above: a `Paraphrase` guess is never
    /// suppressed just because an explicit citation for the same verse
    /// was already made moments earlier.
    #[test]
    fn a_paraphrase_is_not_suppressed_by_a_recent_explicit_citation_for_the_same_verse() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);

        let explicit_segment = sample_transcript_segment("Romans 8:28", 0);
        let explicit_segment_id = explicit_segment.id;
        persist_transcript_segment(&conn, session.id, &explicit_segment).unwrap();
        persist_scripture_detection(
            &conn,
            session.id,
            Some(explicit_segment_id),
            "KJV",
            &rom_8_28_detection(ReferenceKind::Direct),
        )
        .unwrap();

        let paraphrase_segment = sample_transcript_segment(
            "All things work together for good for those who love God",
            1,
        );
        let paraphrase_segment_id = paraphrase_segment.id;
        persist_transcript_segment(&conn, session.id, &paraphrase_segment).unwrap();
        assert!(
            !has_recent_detection_for_reference(
                &conn,
                session.id,
                "ROM 8:28",
                DetectionCategory::Paraphrase,
                60,
                paraphrase_segment_id,
            )
            .unwrap(),
            "a Paraphrase guess must never be suppressed by a recent explicit citation for the same verse"
        );
    }

    /// `pipeline::handle_final_transcript` always persists a segment's own
    /// detections before running this dedup check - without excluding the
    /// current segment's own just-written row, the very first occurrence
    /// of a reference would always look like a duplicate of itself.
    #[test]
    fn has_recent_detection_for_reference_excludes_the_current_segments_own_row() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let segment = sample_transcript_segment("Romans 8:28", 0);
        let segment_id = segment.id;
        persist_transcript_segment(&conn, session.id, &segment).unwrap();
        persist_scripture_detection(
            &conn,
            session.id,
            Some(segment_id),
            "KJV",
            &rom_8_28_detection(ReferenceKind::Direct),
        )
        .unwrap();

        assert!(!has_recent_detection_for_reference(
            &conn,
            session.id,
            "ROM 8:28",
            DetectionCategory::Explicit,
            60,
            segment_id,
        )
        .unwrap());
    }

    // --- temporal confirmation / sliding re-score (Phase 5.2) -----------------

    /// Regression lock for the Phase 5.2 dedup-category fix: before this
    /// phase, `ReferenceKind::Semantic` fell into the `Explicit` bucket by
    /// omission (`pipeline.rs` never routed it to its own category), so a
    /// repeated semantic guess was checked only against
    /// `DIRECT`/`VERSE`/`SEQUENTIAL_REFERENCE` rows - never against another
    /// `SEMANTIC_REFERENCE` row - meaning it could never dedup against
    /// itself. `DetectionCategory::Semantic` restores that guarantee.
    #[test]
    fn has_recent_detection_for_reference_semantic_category_dedupes_against_itself() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);

        let first_segment = sample_transcript_segment("God causes all things to work for good", 0);
        let first_segment_id = first_segment.id;
        persist_transcript_segment(&conn, session.id, &first_segment).unwrap();
        persist_scripture_detection(
            &conn,
            session.id,
            Some(first_segment_id),
            "KJV",
            &rom_8_28_detection(ReferenceKind::Semantic),
        )
        .unwrap();

        let second_segment = sample_transcript_segment("Everything works for the good of those", 1);
        let second_segment_id = second_segment.id;
        persist_transcript_segment(&conn, session.id, &second_segment).unwrap();

        assert!(
            has_recent_detection_for_reference(
                &conn,
                session.id,
                "ROM 8:28",
                DetectionCategory::Semantic,
                60,
                second_segment_id,
            )
            .unwrap(),
            "a repeated Semantic guess for the same verse must dedup against a prior Semantic guess"
        );
    }

    /// The `Semantic` bucket is still its own category, isolated from
    /// `Explicit` in both directions - the same guarantee `Paraphrase`
    /// already had (see the tests above).
    #[test]
    fn has_recent_detection_for_reference_semantic_category_is_isolated_from_explicit() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);

        let semantic_segment = sample_transcript_segment("Everything works for good somehow", 0);
        let semantic_segment_id = semantic_segment.id;
        persist_transcript_segment(&conn, session.id, &semantic_segment).unwrap();
        persist_scripture_detection(
            &conn,
            session.id,
            Some(semantic_segment_id),
            "KJV",
            &rom_8_28_detection(ReferenceKind::Semantic),
        )
        .unwrap();

        let explicit_segment = sample_transcript_segment("Romans 8:28", 1);
        let explicit_segment_id = explicit_segment.id;
        persist_transcript_segment(&conn, session.id, &explicit_segment).unwrap();

        assert!(
            !has_recent_detection_for_reference(
                &conn,
                session.id,
                "ROM 8:28",
                DetectionCategory::Explicit,
                60,
                explicit_segment_id,
            )
            .unwrap(),
            "an explicit citation must never be suppressed by a recent Semantic guess for the same verse"
        );
    }

    fn sample_paraphrase_suggestion(service_id: Uuid, score: f32) -> Suggestion {
        Suggestion::new(
            service_id,
            SuggestionKind::Scripture {
                reference: "ROM 8:28".into(),
            },
            ConfidenceResult::new(score, ConfidenceSource::Heuristic, None),
        )
    }

    #[test]
    fn find_pending_suggestion_for_reference_finds_the_matching_pending_suggestion() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let suggestion = sample_paraphrase_suggestion(session.id, 0.8);
        persist_suggestion(&conn, &suggestion).unwrap();

        let found = find_pending_suggestion_for_reference(&conn, session.id, "ROM 8:28")
            .unwrap()
            .expect("a pending suggestion for this reference exists");
        assert_eq!(found.id, suggestion.id);
    }

    #[test]
    fn find_pending_suggestion_for_reference_ignores_non_pending_suggestions() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let suggestion = sample_paraphrase_suggestion(session.id, 0.8);
        persist_suggestion(&conn, &suggestion).unwrap();
        update_suggestion_status(&conn, suggestion.id, SuggestionStatus::Approved, None).unwrap();

        let found = find_pending_suggestion_for_reference(&conn, session.id, "ROM 8:28").unwrap();
        assert!(
            found.is_none(),
            "a suggestion the operator already acted on must never be a confirmation target"
        );
    }

    #[test]
    fn find_pending_suggestion_for_reference_returns_none_without_a_match() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);

        let found = find_pending_suggestion_for_reference(&conn, session.id, "ROM 8:28").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn confirm_suggestion_increments_count_and_boosts_score() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let suggestion = sample_paraphrase_suggestion(session.id, 0.75);
        persist_suggestion(&conn, &suggestion).unwrap();

        let confirmed = confirm_suggestion(&conn, suggestion.id, 0.1, 0.9).unwrap();
        assert_eq!(confirmed.confirmation_count, 1);
        assert!((confirmed.confidence.score - 0.85).abs() < 0.001);

        let reloaded = get_suggestion(&conn, suggestion.id).unwrap();
        assert_eq!(reloaded.confirmation_count, 1);
        assert!((reloaded.confidence.score - 0.85).abs() < 0.001);
    }

    #[test]
    fn confirm_suggestion_caps_the_score_but_keeps_counting() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let suggestion = sample_paraphrase_suggestion(session.id, 0.88);
        persist_suggestion(&conn, &suggestion).unwrap();

        let first = confirm_suggestion(&conn, suggestion.id, 0.1, 0.9).unwrap();
        assert!((first.confidence.score - 0.9).abs() < 0.001);
        assert_eq!(first.confirmation_count, 1);

        let second = confirm_suggestion(&conn, suggestion.id, 0.1, 0.9).unwrap();
        assert!(
            (second.confidence.score - 0.9).abs() < 0.001,
            "score must never exceed max_score even after another confirmation"
        );
        assert_eq!(
            second.confirmation_count, 2,
            "confirmation_count is an honest count of occurrences, not a proxy for the score"
        );
    }

    #[test]
    fn confirm_suggestion_never_decreases_the_score() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        // A negative bonus should never happen in practice (the caller
        // always passes CONFIRMATION_SCORE_BONUS), but confirm_suggestion
        // itself guards against ever lowering a suggestion's score.
        let suggestion = sample_paraphrase_suggestion(session.id, 0.8);
        persist_suggestion(&conn, &suggestion).unwrap();

        let confirmed = confirm_suggestion(&conn, suggestion.id, -0.5, 0.9).unwrap();
        assert!(
            (confirmed.confidence.score - 0.8).abs() < 0.001,
            "confirm_suggestion must never lower a suggestion's score"
        );
    }

    #[test]
    fn find_rejected_suggestion_for_reference_finds_the_matching_rejected_suggestion() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let suggestion = sample_paraphrase_suggestion(session.id, 0.8);
        persist_suggestion(&conn, &suggestion).unwrap();
        update_suggestion_status(&conn, suggestion.id, SuggestionStatus::Rejected, None).unwrap();

        let found = find_rejected_suggestion_for_reference(&conn, session.id, "ROM 8:28")
            .unwrap()
            .expect("a rejected suggestion for this reference exists");
        assert_eq!(found.id, suggestion.id);
    }

    #[test]
    fn find_rejected_suggestion_for_reference_ignores_pending_and_approved() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let pending = sample_paraphrase_suggestion(session.id, 0.8);
        persist_suggestion(&conn, &pending).unwrap();

        let approved = sample_paraphrase_suggestion(session.id, 0.8);
        persist_suggestion(&conn, &approved).unwrap();
        update_suggestion_status(&conn, approved.id, SuggestionStatus::Approved, None).unwrap();

        let found = find_rejected_suggestion_for_reference(&conn, session.id, "ROM 8:28").unwrap();
        assert!(
            found.is_none(),
            "a Pending or Approved suggestion must never be a rejection-echo target"
        );
    }

    #[test]
    fn find_rejected_suggestion_for_reference_returns_none_without_a_match() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);

        let found = find_rejected_suggestion_for_reference(&conn, session.id, "ROM 8:28").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn record_rejection_echo_increments_the_count_without_touching_status_or_score() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let suggestion = sample_paraphrase_suggestion(session.id, 0.8);
        persist_suggestion(&conn, &suggestion).unwrap();
        update_suggestion_status(&conn, suggestion.id, SuggestionStatus::Rejected, None).unwrap();

        let echoed = record_rejection_echo(&conn, suggestion.id).unwrap();
        assert_eq!(echoed.rejection_echo_count, 1);
        assert_eq!(
            echoed.status,
            SuggestionStatus::Rejected,
            "recording an echo must never resurrect a decided suggestion"
        );
        assert!(
            (echoed.confidence.score - 0.8).abs() < 0.001,
            "recording an echo must never change the suggestion's score"
        );

        let second = record_rejection_echo(&conn, suggestion.id).unwrap();
        assert_eq!(
            second.rejection_echo_count, 2,
            "rejection_echo_count is an honest, unconditionally incrementing count"
        );
    }

    // --- sermon foundation (Phase 2.5, per the authoritative Phase 2 roadmap) --

    fn seeded_sermon(conn: &Connection, service_id: Uuid) -> cip_core_sermon::foundation::Sermon {
        let sermon = cip_core_sermon::foundation::Sermon::start(
            service_id,
            Some("Grace Abounding".to_string()),
        );
        persist_sermon(conn, &sermon).unwrap();
        sermon
    }

    #[test]
    fn persists_and_retrieves_a_sermon_across_a_simulated_restart() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let sermon = seeded_sermon(&conn, session.id);

        // "Restart" = drop everything but the connection's own on-disk (or
        // here, in-memory-but-independent) row data and re-fetch by id -
        // proves the row alone (not any in-process cache) carries every
        // field forward.
        let reloaded = get_sermon(&conn, sermon.id).unwrap();
        assert_eq!(
            reloaded, sermon,
            "every field survives a reload identically"
        );
    }

    #[test]
    fn update_sermon_persists_status_and_speaker_changes() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let mut sermon = seeded_sermon(&conn, session.id);

        sermon.assign_speaker(cip_core_sermon::foundation::Speaker::new(
            "Pastor Jane Doe",
            cip_core_sermon::foundation::SpeakerRole::Primary,
        ));
        sermon.pause();
        update_sermon(&conn, &sermon).unwrap();

        let reloaded = get_sermon(&conn, sermon.id).unwrap();
        assert_eq!(
            reloaded.status,
            cip_core_sermon::foundation::SermonStatus::Paused
        );
        assert_eq!(reloaded.speaker.unwrap().name, "Pastor Jane Doe");
    }

    #[test]
    fn update_sermon_reports_not_found_for_an_unknown_id() {
        let conn = migrated_conn();
        let ghost = cip_core_sermon::foundation::Sermon::start(Uuid::new_v4(), None);
        let result = update_sermon(&conn, &ghost);
        assert!(matches!(result, Err(PersistError::NotFound(_))));
    }

    #[test]
    fn get_sermon_reports_not_found_for_an_unknown_id() {
        let conn = migrated_conn();
        assert!(matches!(
            get_sermon(&conn, Uuid::new_v4()),
            Err(PersistError::NotFound(_))
        ));
    }

    #[test]
    fn list_sermons_for_service_orders_most_recent_first_and_scopes_by_service() {
        let conn = migrated_conn();
        let session_a = seeded_service(&conn);
        let session_b = ServiceSession::start("Another Service");
        persist_service(&conn, &session_b).unwrap();

        let first = seeded_sermon(&conn, session_a.id);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = cip_core_sermon::foundation::Sermon::start(
            session_a.id,
            Some("Second Message".to_string()),
        );
        persist_sermon(&conn, &second).unwrap();
        let _unrelated = seeded_sermon(&conn, session_b.id);

        let sermons = list_sermons_for_service(&conn, session_a.id, 10).unwrap();
        assert_eq!(sermons.len(), 2, "only session_a's sermons are returned");
        assert_eq!(sermons[0].id, second.id, "most recently created first");
        assert_eq!(sermons[1].id, first.id);
    }

    #[test]
    fn a_sermon_referencing_a_nonexistent_service_is_rejected() {
        let conn = migrated_conn();
        let sermon = cip_core_sermon::foundation::Sermon::start(Uuid::new_v4(), None);
        let result = persist_sermon(&conn, &sermon);
        assert!(
            result.is_err(),
            "the schema's foreign key must reject an orphan sermon"
        );
    }

    #[test]
    fn opening_a_new_section_closes_the_previously_open_one() {
        use cip_core_sermon::foundation::{SectionOrigin, SermonSection, SermonSectionKind};

        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let sermon = seeded_sermon(&conn, session.id);

        let intro = SermonSection::open(
            sermon.id,
            SermonSectionKind::Introduction,
            SectionOrigin::SystemBoundary,
            None,
        );
        persist_sermon_section(&conn, &intro).unwrap();
        let open_count = list_sermon_sections(&conn, sermon.id)
            .unwrap()
            .iter()
            .filter(|s| s.ended_at.is_none())
            .count();
        assert_eq!(open_count, 1);

        let main = SermonSection::open(
            sermon.id,
            SermonSectionKind::MainMessage,
            SectionOrigin::OperatorAssigned,
            None,
        );
        close_open_sermon_section(&conn, sermon.id, main.started_at).unwrap();
        persist_sermon_section(&conn, &main).unwrap();

        let all = list_sermon_sections(&conn, sermon.id).unwrap();
        assert_eq!(
            all.len(),
            2,
            "the closed introduction section is never deleted"
        );
        let open_sections: Vec<_> = all.iter().filter(|s| s.ended_at.is_none()).collect();
        assert_eq!(
            open_sections.len(),
            1,
            "only one section may be open at a time"
        );
        assert_eq!(
            open_sections[0].id, main.id,
            "the new section is now the only open one"
        );
        let closed_intro = all.iter().find(|s| s.id == intro.id).unwrap();
        assert!(
            closed_intro.ended_at.is_some(),
            "history is preserved, not overwritten"
        );
    }

    #[test]
    fn closing_when_nothing_is_open_is_a_harmless_no_op() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let sermon = seeded_sermon(&conn, session.id);
        // No section has ever been opened for this sermon - must not error.
        close_open_sermon_section(&conn, sermon.id, Utc::now()).unwrap();
        assert!(list_sermon_sections(&conn, sermon.id).unwrap().is_empty());
    }

    #[test]
    fn sermon_segments_persist_with_gapless_sequence_and_survive_reload() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let sermon = seeded_sermon(&conn, session.id);
        let t1 = sample_transcript_segment("In the beginning.", 0);
        let t2 = sample_transcript_segment("God created the heavens.", 1);
        persist_transcript_segment(&conn, session.id, &t1).unwrap();
        persist_transcript_segment(&conn, session.id, &t2).unwrap();

        let seq0 = count_sermon_segments(&conn, sermon.id).unwrap();
        assert_eq!(seq0, 0);
        let seg1 = cip_core_sermon::foundation::SermonSegment::new(sermon.id, t1.id, seq0, None);
        persist_sermon_segment(&conn, &seg1).unwrap();

        let seq1 = count_sermon_segments(&conn, sermon.id).unwrap();
        assert_eq!(seq1, 1);
        let seg2 = cip_core_sermon::foundation::SermonSegment::new(sermon.id, t2.id, seq1, None);
        persist_sermon_segment(&conn, &seg2).unwrap();

        let reloaded = list_sermon_segments(&conn, sermon.id).unwrap();
        assert_eq!(reloaded.len(), 2);
        assert_eq!(reloaded[0].sequence, 0);
        assert_eq!(reloaded[1].sequence, 1);
        assert_eq!(reloaded[0].transcript_segment_id, t1.id);
    }

    #[test]
    fn a_sermon_segment_referencing_an_unknown_transcript_segment_is_rejected() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let sermon = seeded_sermon(&conn, session.id);
        let orphan =
            cip_core_sermon::foundation::SermonSegment::new(sermon.id, Uuid::new_v4(), 0, None);
        assert!(persist_sermon_segment(&conn, &orphan).is_err());
    }

    #[test]
    fn get_transcript_segment_service_id_distinguishes_known_from_unknown() {
        let conn = migrated_conn();
        let session = seeded_service(&conn);
        let segment = sample_transcript_segment("Let us pray.", 0);
        persist_transcript_segment(&conn, session.id, &segment).unwrap();

        assert_eq!(
            get_transcript_segment_service_id(&conn, segment.id).unwrap(),
            Some(session.id)
        );
        assert_eq!(
            get_transcript_segment_service_id(&conn, Uuid::new_v4()).unwrap(),
            None
        );
    }

    #[test]
    fn saved_scripture_create_retrieve_matches_the_committed_row() {
        // "create -> retrieve" durability proof (Phase 3.6 spec section 19):
        // a plain committed SQLite write is durable by construction, the
        // same proof pattern every other persistence test in this module
        // already uses (there is no in-process "restart" to simulate -
        // literal process-restart durability is proven at the Xvfb
        // relaunch level, see pilot-evidence/3.6/).
        let conn = migrated_conn();
        let id = Uuid::new_v4();
        let saved = persist_saved_scripture(
            &conn,
            id,
            "BSB",
            "ROM",
            8,
            28,
            None,
            "ROM 8:28",
            Some("Comfort verse"),
        )
        .unwrap();
        assert_eq!(saved.id, id);
        assert_eq!(saved.verse_end, None);

        let all = list_saved_scriptures(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0], saved);
    }

    #[test]
    fn saved_scripture_supports_a_verse_range() {
        let conn = migrated_conn();
        let saved = persist_saved_scripture(
            &conn,
            Uuid::new_v4(),
            "BSB",
            "ROM",
            8,
            29,
            Some(30),
            "ROM 8:29-30",
            None,
        )
        .unwrap();
        assert_eq!(saved.verse_start, 29);
        assert_eq!(saved.verse_end, Some(30));
        assert_eq!(saved.note, None);
    }

    #[test]
    fn list_saved_scriptures_orders_most_recently_saved_first() {
        let conn = migrated_conn();
        let first = persist_saved_scripture(
            &conn,
            Uuid::new_v4(),
            "BSB",
            "GEN",
            1,
            1,
            None,
            "GEN 1:1",
            None,
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = persist_saved_scripture(
            &conn,
            Uuid::new_v4(),
            "BSB",
            "JHN",
            3,
            16,
            None,
            "JHN 3:16",
            None,
        )
        .unwrap();

        let all = list_saved_scriptures(&conn).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, second.id);
        assert_eq!(all[1].id, first.id);
    }

    #[test]
    fn delete_saved_scripture_removes_it_and_is_safe_to_repeat() {
        let conn = migrated_conn();
        let saved = persist_saved_scripture(
            &conn,
            Uuid::new_v4(),
            "BSB",
            "PSA",
            23,
            1,
            None,
            "PSA 23:1",
            None,
        )
        .unwrap();

        assert!(delete_saved_scripture(&conn, saved.id).unwrap());
        assert!(list_saved_scriptures(&conn).unwrap().is_empty());
        // Idempotent: deleting an already-deleted (or never-existed) row
        // reports "nothing was there", never an error.
        assert!(!delete_saved_scripture(&conn, saved.id).unwrap());
    }

    fn sample_content_candidate(service_id: Uuid) -> cip_core_intelligence::ContentCandidate {
        use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
        use cip_core_intelligence::{AssertionLevel, ContentCandidate, ContentCandidateType};

        ContentCandidate::new(
            service_id,
            None,
            vec![Uuid::new_v4()],
            ContentCandidateType::Theme,
            "Theme: faithfulness",
            "Faithfulness in small things",
            AssertionLevel::Suggested,
            ConfidenceResult::new(0.8, ConfidenceSource::Model, None),
            0.6,
            "sermon-content-v1",
            "1.0",
        )
    }

    #[test]
    fn saved_content_candidate_create_retrieve_matches_the_committed_row() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Content Test");
        persist_service(&conn, &session).unwrap();

        let candidate = sample_content_candidate(session.id);
        persist_saved_content_candidate(&conn, &candidate).unwrap();

        let all = list_saved_content_candidates_for_service(&conn, session.id).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0], candidate, "the round-tripped candidate must match the original exactly - provenance/evidence/confidence preserved verbatim");
    }

    #[test]
    fn saved_content_candidates_are_scoped_to_their_own_service() {
        let conn = migrated_conn();
        let session_a = ServiceSession::start("Service A");
        persist_service(&conn, &session_a).unwrap();
        let session_b = ServiceSession::start("Service B");
        persist_service(&conn, &session_b).unwrap();

        persist_saved_content_candidate(&conn, &sample_content_candidate(session_a.id)).unwrap();
        persist_saved_content_candidate(&conn, &sample_content_candidate(session_b.id)).unwrap();

        assert_eq!(
            list_saved_content_candidates_for_service(&conn, session_a.id)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list_saved_content_candidates_for_service(&conn, session_b.id)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn list_saved_content_candidates_orders_most_recently_saved_first() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Content Order Test");
        persist_service(&conn, &session).unwrap();

        let first = sample_content_candidate(session.id);
        persist_saved_content_candidate(&conn, &first).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = sample_content_candidate(session.id);
        persist_saved_content_candidate(&conn, &second).unwrap();

        let all = list_saved_content_candidates_for_service(&conn, session.id).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, second.id, "most recently saved first");
        assert_eq!(all[1].id, first.id);
    }

    // --- display role assignments (Phase 3.10.2) ---------------------------

    #[test]
    fn assign_display_role_then_list_round_trips() {
        let conn = migrated_conn();
        assign_display_role(&conn, "tv", DisplayRole::Projector).unwrap();
        assign_display_role(&conn, "laptop", DisplayRole::Operator).unwrap();

        let assignments = list_display_role_assignments(&conn).unwrap();
        assert_eq!(assignments.get("tv"), Some(&DisplayRole::Projector));
        assert_eq!(assignments.get("laptop"), Some(&DisplayRole::Operator));
        assert_eq!(assignments.len(), 2);
    }

    #[test]
    fn assigning_a_role_twice_replaces_rather_than_duplicates() {
        let conn = migrated_conn();
        assign_display_role(&conn, "tv", DisplayRole::Unassigned).unwrap();
        assign_display_role(&conn, "tv", DisplayRole::Projector).unwrap();

        let assignments = list_display_role_assignments(&conn).unwrap();
        assert_eq!(
            assignments.len(),
            1,
            "re-assigning must update the existing row, not add a second one"
        );
        assert_eq!(assignments.get("tv"), Some(&DisplayRole::Projector));
    }

    #[test]
    fn list_display_role_assignments_is_empty_when_nothing_assigned_yet() {
        let conn = migrated_conn();
        assert!(list_display_role_assignments(&conn).unwrap().is_empty());
    }
}
