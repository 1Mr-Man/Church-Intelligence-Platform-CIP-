//! The service timeline (Phase 1.3).
//!
//! Reuses the existing `audit_events` table (defined in Phase 1.0's
//! `0001_initial_schema.sql`, unused by any code until now) instead of
//! introducing a second event bus or a redundant timeline table - "if the
//! existing audit/event infrastructure can serve this purpose, reuse it."
//! An `AppEvent` already has a name and a `LogCategory` already has a
//! category at every call site that would emit one; this module just also
//! persists that same information as one `audit_events` row, so the
//! service timeline is exactly "every meaningful event that happened
//! during this service," reconstructable after a restart from SQLite
//! alone.
//!
//! `payload` is a small, event-specific JSON blob (a reference string, a
//! confidence score, an old/new value pair for a correction) - enough for
//! the Live Church Brain to render a human-readable line
//! ("09:16:07 Romans 8:28 suggested - confidence 98%") without re-deriving
//! it from other tables, but nothing already available elsewhere (full
//! transcript text, full suggestion history) is duplicated into it.

use crate::events::AppEvent;
use crate::logging::LogCategory;
use crate::persistence::PersistError;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEntry {
    pub id: Uuid,
    pub service_id: Option<Uuid>,
    /// The `AppEvent` name that produced this entry (e.g. `"SERVICE_STARTED"`,
    /// `"SUGGESTION_APPROVED"`) - the Live Church Brain derives a
    /// human-readable description from this + `payload` rather than the
    /// backend storing a separately-formatted description string.
    pub event_name: String,
    pub category: String,
    pub payload: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Matches `audit_events.category`'s `CHECK` constraint exactly (see
/// `database/migrations/0001_initial_schema.sql`) - deliberately not the
/// same string as `LogCategory::target()` (`"cip::database"`), which is a
/// `log` target, not a stored value.
fn category_str(category: LogCategory) -> &'static str {
    match category {
        LogCategory::App => "app",
        LogCategory::Database => "database",
        LogCategory::Audio => "audio",
        LogCategory::Speech => "speech",
        LogCategory::Bible => "bible",
        LogCategory::Ai => "ai",
        LogCategory::Presentation => "presentation",
        // `audit_events.category`'s CHECK constraint (migration 0001) has
        // no `'content'` value and nothing here writes a Content Registry
        // timeline entry (it's app-level content management, not a
        // service-scoped event) - mapped to the closest existing bucket
        // rather than an additive migration for a category with no
        // current caller.
        LogCategory::Content => "app",
        // Unlike Content above, Music Intelligence timeline entries are a
        // real, permanent caller (see commands.rs's music section) - the
        // CHECK constraint was extended for it (migration 0007) rather
        // than mapped onto an unrelated bucket.
        LogCategory::Music => "music",
        LogCategory::Network => "network",
        LogCategory::Security => "security",
        LogCategory::Error => "error",
    }
}

/// Record one timeline entry. `service_id: None` is valid (the schema
/// allows it, `ON DELETE SET NULL`) for an app-level event with no
/// service context, though Phase 1.3 only ever calls this with a real
/// service id in practice.
pub fn record_event(
    conn: &Connection,
    service_id: Option<Uuid>,
    event: AppEvent,
    category: LogCategory,
    payload: impl Serialize,
) -> Result<(), PersistError> {
    let payload_json = serde_json::to_string(&payload)?;
    conn.execute(
        "INSERT INTO audit_events (id, service_id, event_name, category, payload, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            Uuid::new_v4().to_string(),
            service_id.map(|id| id.to_string()),
            event.name(),
            category_str(category),
            payload_json,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// The service timeline, oldest first (reading order) - for the Live
/// Church Brain's history view. Bounded by `limit`, matching the pattern
/// `persistence::list_transcript_segments` already established (query
/// newest-first with `LIMIT`, then reverse) so a long service doesn't pull
/// its entire history into memory just to show the most recent slice.
pub fn list_timeline(
    conn: &Connection,
    service_id: Uuid,
    limit: u32,
) -> Result<Vec<TimelineEntry>, PersistError> {
    let mut stmt = conn.prepare(
        "SELECT id, service_id, event_name, category, payload, created_at
         FROM audit_events WHERE service_id = ?1
         ORDER BY created_at DESC LIMIT ?2",
    )?;
    let mut rows: Vec<TimelineEntry> = stmt
        .query_map(params![service_id.to_string(), limit], |row| {
            let payload: Option<String> = row.get(4)?;
            Ok(TimelineEntry {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
                service_id: row
                    .get::<_, Option<String>>(1)?
                    .and_then(|s| Uuid::parse_str(&s).ok()),
                event_name: row.get(2)?,
                category: row.get(3)?,
                payload: payload.and_then(|p| serde_json::from_str(&p).ok()),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.reverse(); // oldest first
    Ok(rows)
}

/// How many `audit_events` rows exist for `service_id`, grouped by
/// `category` - the Phase 5.1 post-service report's timeline/error-count
/// summary. A plain `GROUP BY` count, not a row dump: a long service's
/// timeline can run into the hundreds or thousands of entries, and this
/// report only ever needs totals per category (most importantly `"error"`,
/// so an operator can see at a glance whether anything went wrong without
/// reading the full timeline).
pub fn count_events_by_category(
    conn: &Connection,
    service_id: Uuid,
) -> Result<Vec<(String, u64)>, PersistError> {
    let mut stmt = conn.prepare(
        "SELECT category, COUNT(*) FROM audit_events
         WHERE service_id = ?1 GROUP BY category ORDER BY category",
    )?;
    let rows = stmt
        .query_map(params![service_id.to_string()], |row| {
            let category: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((category, count as u64))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::persist_service;
    use cip_core_service::ServiceSession;
    use cip_database::{open_in_memory, run_migrations};
    use serde_json::json;

    fn migrated_conn() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    /// `audit_events.service_id` is a real foreign key (`ON DELETE SET
    /// NULL`) - a timeline entry needs a real persisted `services` row
    /// to reference, exactly like every other Phase 1.3 table this
    /// module's tests exercise.
    fn seeded_service(conn: &Connection) -> Uuid {
        let session = ServiceSession::start("Timeline Test");
        persist_service(conn, &session).unwrap();
        session.id
    }

    #[test]
    fn records_and_lists_timeline_entries_oldest_first() {
        let conn = migrated_conn();
        let service_id = seeded_service(&conn);

        record_event(
            &conn,
            Some(service_id),
            AppEvent::ServiceStarted,
            LogCategory::App,
            json!({ "title": "Sunday Morning" }),
        )
        .unwrap();
        record_event(
            &conn,
            Some(service_id),
            AppEvent::SuggestionCreated,
            LogCategory::Ai,
            json!({ "reference": "ROM 8:28", "confidence": 0.98 }),
        )
        .unwrap();

        let timeline = list_timeline(&conn, service_id, 50).unwrap();
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[0].event_name, "SERVICE_STARTED");
        assert_eq!(timeline[1].event_name, "SUGGESTION_CREATED");
        assert_eq!(
            timeline[1].payload.as_ref().unwrap()["reference"],
            "ROM 8:28"
        );
    }

    #[test]
    fn timeline_is_scoped_to_its_service() {
        let conn = migrated_conn();
        let service_a = seeded_service(&conn);
        let service_b = seeded_service(&conn);

        record_event(
            &conn,
            Some(service_a),
            AppEvent::ServiceStarted,
            LogCategory::App,
            json!({}),
        )
        .unwrap();
        record_event(
            &conn,
            Some(service_b),
            AppEvent::ServiceStarted,
            LogCategory::App,
            json!({}),
        )
        .unwrap();

        assert_eq!(list_timeline(&conn, service_a, 50).unwrap().len(), 1);
        assert_eq!(list_timeline(&conn, service_b, 50).unwrap().len(), 1);
    }

    #[test]
    fn limit_bounds_the_result_even_with_more_rows_available() {
        let conn = migrated_conn();
        let service_id = seeded_service(&conn);
        for _ in 0..10 {
            record_event(
                &conn,
                Some(service_id),
                AppEvent::SuggestionCreated,
                LogCategory::Ai,
                json!({}),
            )
            .unwrap();
        }
        assert_eq!(list_timeline(&conn, service_id, 3).unwrap().len(), 3);
    }
}
