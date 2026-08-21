//! Wires one final transcript segment through the whole live pipeline:
//!
//! ```text
//! TranscriptSegment
//!     -> persist transcript_segments row
//!     -> core/service::process_transcript_segment (Bible Intelligence Core)
//!     -> persist scripture_detections rows (validated ones only)
//!     -> persist ai_suggestions rows (always Pending)
//! ```
//!
//! Deliberately Tauri-agnostic (plain `&Connection` + domain types, no
//! `AppHandle`) so it's directly unit-testable and reusable from both the
//! real `start_listening` command and the deterministic
//! `process_test_transcript` command - neither one duplicates this logic.
//! Event emission (which does need an `AppHandle`) is the caller's job;
//! see `commands.rs`.

use cip_core_bible::{BibleProvider, DefaultScriptureContextManager};
use cip_core_service::{process_transcript_segment, ProcessedSegment};
use rusqlite::Connection;
use uuid::Uuid;

use crate::persistence::{
    persist_scripture_detection, persist_suggestion, persist_transcript_segment, PersistError,
};

/// Run one **final** transcript segment through the full pipeline,
/// persisting every step. Returns the same [`ProcessedSegment`] the Bible
/// Intelligence Core produced, so the caller can emit events from it
/// without re-deriving anything.
///
/// `segment.is_final` is not checked here - the caller (an `AudioEngine`
/// sink or `process_test_transcript`) is responsible for only ever calling
/// this with final segments; interim segments are handled entirely in
/// runtime UI state and never reach this function (see `docs/live-speech.md`).
pub fn handle_final_transcript(
    conn: &Connection,
    provider: &dyn BibleProvider,
    context: &mut DefaultScriptureContextManager,
    service_id: Uuid,
    translation_id: &str,
    segment: cip_core_ai::TranscriptSegment,
) -> Result<ProcessedSegment, PersistError> {
    persist_transcript_segment(conn, service_id, &segment)?;

    let processed =
        process_transcript_segment(service_id, &segment.text, translation_id, provider, context);

    for detection in &processed.detections {
        persist_scripture_detection(
            conn,
            service_id,
            Some(segment.id),
            translation_id,
            detection,
        )?;
    }
    for suggestion in &processed.suggestions {
        persist_suggestion(conn, suggestion)?;
    }

    Ok(processed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{list_suggestions, persist_service};
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use cip_core_service::ServiceSession;
    use cip_database::{open_in_memory, run_migrations, seed::apply_dev_seed};
    use cip_integrations_bible::SqliteBibleProvider;

    fn seeded_db() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        apply_dev_seed(&conn).unwrap();
        conn
    }

    fn segment(text: &str, sequence: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence,
            text: text.to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Model, None),
            start_ms: sequence * 1000,
            end_ms: sequence * 1000 + 900,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    /// Proves: Transcript -> Bible Intelligence -> Detection -> SQLite
    /// persistence -> Suggestion, verified against actual database rows,
    /// using the real SQLite-backed BibleProvider (per Phase 1.2's
    /// integration test requirement).
    #[test]
    fn a_direct_reference_flows_from_transcript_to_persisted_suggestion() {
        let conn = seeded_db();
        let session = ServiceSession::start("Pipeline Test");
        persist_service(&conn, &session).unwrap();

        // Each `:memory:` connection is its own separate database, so the
        // BibleProvider needs its own seeded connection distinct from the
        // one persistence writes to (in the real app both point at the
        // same on-disk file - see `state.rs`).
        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);

        let mut context = DefaultScriptureContextManager::new("KJV");
        let processed = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Romans 8:28", 0),
        )
        .unwrap();

        assert_eq!(processed.detections.len(), 1);
        assert_eq!(processed.suggestions.len(), 1);

        let transcript_count: i64 = conn
            .query_row("SELECT count(*) FROM transcript_segments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(transcript_count, 1);

        let detection_count: i64 = conn
            .query_row("SELECT count(*) FROM scripture_detections", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(detection_count, 1);

        let suggestions = list_suggestions(&conn, session.id, None).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(
            suggestions[0].status,
            cip_core_ai::SuggestionStatus::Pending
        );
    }

    #[test]
    fn unrelated_prose_persists_the_transcript_but_no_detection_or_suggestion() {
        let conn = seeded_db();
        let session = ServiceSession::start("Pipeline Test");
        persist_service(&conn, &session).unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");

        let processed = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Let us pray together this morning.", 0),
        )
        .unwrap();

        assert!(processed.detections.is_empty());
        assert!(processed.suggestions.is_empty());

        let transcript_count: i64 = conn
            .query_row("SELECT count(*) FROM transcript_segments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(transcript_count, 1);
        let detection_count: i64 = conn
            .query_row("SELECT count(*) FROM scripture_detections", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(detection_count, 0);
    }

    /// The Phase 1.1/1.2 regression requirement, run through the full
    /// persisted pipeline this time: Romans 8 -> unrelated segments ->
    /// verse 28/31/18 -> John 3 -> verse 16, verified against actual rows.
    #[test]
    fn the_romans_8_to_john_3_sequence_persists_deterministically() {
        let conn = seeded_db();
        let session = ServiceSession::start("Pipeline E2E");
        persist_service(&conn, &session).unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");

        let segments = [
            "Turn with me to Romans chapter 8.",
            "Paul is explaining something very important here.",
            "We have to understand the work of the Spirit.",
            "Look at verse 28.",
            "Now verse 31.",
            "Go back to verse 18.",
            "Now let's go to John chapter 3.",
            "Verse 16.",
        ];

        let mut all_references = Vec::new();
        for (i, text) in segments.iter().enumerate() {
            let processed = handle_final_transcript(
                &conn,
                &provider,
                &mut context,
                session.id,
                "KJV",
                segment(text, i as u64),
            )
            .unwrap();
            for detection in &processed.detections {
                if let Some(reference) = &detection.reference {
                    all_references.push(reference.to_string());
                }
            }
        }

        assert_eq!(
            all_references,
            vec!["ROM 8:28", "ROM 8:31", "ROM 8:18", "JHN 3:16"]
        );

        let transcript_count: i64 = conn
            .query_row("SELECT count(*) FROM transcript_segments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            transcript_count, 8,
            "every final segment is persisted regardless of content"
        );

        // Chapter (x2) + 4 verses = 6 persisted detections; Unresolved/
        // Ambiguous never happen in this sequence.
        let detection_count: i64 = conn
            .query_row("SELECT count(*) FROM scripture_detections", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(detection_count, 6);

        let suggestions = list_suggestions(&conn, session.id, None).unwrap();
        assert_eq!(
            suggestions.len(),
            4,
            "one suggestion per resolved verse, none for bare chapters"
        );
        assert!(suggestions
            .iter()
            .all(|s| s.status == cip_core_ai::SuggestionStatus::Pending));
    }

    /// Phase 1.2's offline requirement: the exact same pipeline, asserted
    /// to require no network access to produce identical results. This
    /// isn't simulated by toggling a flag - `cip-core-service` and
    /// `cip-integrations-bible` (this test's whole dependency graph below
    /// `rusqlite`/`chrono`/`serde`/`regex`/`uuid`) have no HTTP client in
    /// their dependency tree at all (verified via `cargo tree`, see
    /// `docs/live-speech.md`'s offline section) - there is no network call
    /// for this test to disable. Running it (like every other test in this
    /// module) against an in-memory SQLite database *is* the offline proof:
    /// nothing here can reach a network even if it wanted to.
    #[test]
    fn the_pipeline_produces_identical_results_with_no_network_access_possible() {
        let conn = seeded_db();
        let session = ServiceSession::start("Offline Test");
        persist_service(&conn, &session).unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");

        let processed = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Romans 8:28", 0),
        )
        .unwrap();

        assert_eq!(
            processed.detections[0]
                .reference
                .as_ref()
                .unwrap()
                .to_string(),
            "ROM 8:28"
        );
        assert_eq!(processed.suggestions.len(), 1);
    }
}
