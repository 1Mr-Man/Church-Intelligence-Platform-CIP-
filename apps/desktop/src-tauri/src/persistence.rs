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
             transcript_segment_id, source_text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
        ],
    )?;
    Ok(())
}

/// Phase 1.3 session-aware suggestion deduplication: has this exact
/// reference already been suggested for this service within the last
/// `window_seconds`? A pastor repeating "Romans 8:28" mid-explanation
/// should not flood the queue with identical suggestions, but a
/// *genuine* repeat later in the service (past the window) is legitimate
/// and must not be silently suppressed - see `docs/live-service.md`'s
/// deduplication policy. Scoped to one service (never cross-service or
/// permanent/global), and status-independent (a reference the operator
/// already approved or rejected moments ago still counts - re-suggesting
/// it immediately would be noise either way).
///
/// Matches on `payload LIKE '%"reference":"<text>"%'` rather than a
/// dedicated column: `reference` is always a string this application
/// generated itself (`ScriptureReference::to_string()`, e.g. `"ROM 8:28"`),
/// using only alphanumerics, spaces, colons, and hyphens, so it can never
/// contain a SQL LIKE wildcard (`%`/`_`) that would need escaping.
pub fn has_recent_suggestion_for_reference(
    conn: &Connection,
    service_id: Uuid,
    reference_display: &str,
    window_seconds: i64,
) -> Result<bool, PersistError> {
    let cutoff = (Utc::now() - chrono::Duration::seconds(window_seconds)).to_rfc3339();
    let pattern = format!("%\"reference\":\"{reference_display}\"%");
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM ai_suggestions
         WHERE service_id = ?1 AND kind = 'scripture' AND payload LIKE ?2 AND created_at >= ?3",
        params![service_id.to_string(), pattern, cutoff],
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
    })
}

const SUGGESTION_COLUMNS: &str = "id, service_id, payload, status, confidence_score, created_at, \
     transcript_segment_id, source_text";

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
            |(id, service_id, payload, status, score, created_at, seg_id, src)| {
                row_to_suggestion(
                    id, service_id, payload, status, score, created_at, seg_id, src,
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
        |(id, service_id, payload, status, score, created_at, seg_id, src)| {
            row_to_suggestion(
                id, service_id, payload, status, score, created_at, seg_id, src,
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
        |(id, service_id, payload, status, score, created_at, seg_id, src)| {
            row_to_suggestion(
                id, service_id, payload, status, score, created_at, seg_id, src,
            )
        },
    )
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

/// Update a presentation item's status (e.g. cancelling a prepared item -
/// `Stopped`, reused as "prepared then retracted" since nothing in this
/// phase ever transitions an item to `Active`). Returns the updated row.
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

    #[test]
    fn recent_duplicate_suggestion_is_detected_within_the_window_and_not_after() {
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

        assert!(has_recent_suggestion_for_reference(&conn, session.id, "ROM 8:28", 60).unwrap());
        // A different reference in the same service is not a duplicate.
        assert!(!has_recent_suggestion_for_reference(&conn, session.id, "ROM 8:31", 60).unwrap());
        // A window of 0 seconds excludes even a suggestion from "now" once
        // any time at all has elapsed since `created_at` was recorded.
        assert!(!has_recent_suggestion_for_reference(&conn, session.id, "ROM 8:28", -1).unwrap());
    }
}
