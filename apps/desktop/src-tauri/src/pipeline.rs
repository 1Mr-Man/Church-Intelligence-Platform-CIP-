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

use cip_core_ai::SuggestionKind;
use cip_core_bible::{BibleProvider, DefaultScriptureContextManager};
use cip_core_service::{process_transcript_segment, ProcessedSegment};
use rusqlite::Connection;
use std::time::Instant;
use uuid::Uuid;

use crate::persistence::{
    has_recent_suggestion_for_reference, persist_scripture_detection, persist_suggestion,
    persist_transcript_segment, PersistError,
};

/// Phase 1.3 suggestion deduplication window - see
/// `persistence::has_recent_suggestion_for_reference`'s docs for the full
/// policy. 60 seconds is long enough to absorb a pastor repeating the same
/// reference while still explaining it, short enough that a genuinely new
/// mention later in the service is never suppressed.
const SUGGESTION_DEDUP_WINDOW_SECONDS: i64 = 60;

/// Run one **final** transcript segment through the full pipeline,
/// persisting every step. Returns the same [`ProcessedSegment`] the Bible
/// Intelligence Core produced (with `suggestions` filtered per the
/// deduplication policy below), so the caller can emit events from it
/// without re-deriving anything.
///
/// `segment.is_final` is not checked here - the caller (an `AudioEngine`
/// sink or `process_test_transcript`) is responsible for only ever calling
/// this with final segments; interim segments are handled entirely in
/// runtime UI state and never reach this function (see `docs/live-speech.md`).
///
/// ## Deduplication (Phase 1.3)
///
/// Every `ScriptureDetection` is always persisted and returned unfiltered -
/// the transcript-and-detection record is never edited after the fact.
/// Only *suggestion* creation is deduplicated: a suggestion is skipped (not
/// persisted, not emitted, not present in the returned `ProcessedSegment`)
/// if an identical reference was already suggested for this same service
/// within `SUGGESTION_DEDUP_WINDOW_SECONDS` - see
/// `persistence::has_recent_suggestion_for_reference` for exactly why this
/// scope/window was chosen.
///
/// ## Performance logging (Phase 1.3 section 44)
///
/// Each stage is timed and logged at `debug` level - "record where
/// practical," not a formal benchmark harness. See `docs/live-service.md`'s
/// performance section for observed numbers from the test suite.
pub fn handle_final_transcript(
    conn: &Connection,
    provider: &dyn BibleProvider,
    context: &mut DefaultScriptureContextManager,
    service_id: Uuid,
    translation_id: &str,
    segment: cip_core_ai::TranscriptSegment,
) -> Result<ProcessedSegment, PersistError> {
    let persist_transcript_start = Instant::now();
    persist_transcript_segment(conn, service_id, &segment)?;
    log::debug!(
        target: "cip::performance",
        "persist_transcript_segment took {:?}",
        persist_transcript_start.elapsed()
    );

    let detect_start = Instant::now();
    let mut processed =
        process_transcript_segment(service_id, &segment.text, translation_id, provider, context);
    log::debug!(
        target: "cip::performance",
        "process_transcript_segment took {:?} ({} detection(s))",
        detect_start.elapsed(),
        processed.detections.len()
    );

    let persist_start = Instant::now();
    for detection in &processed.detections {
        persist_scripture_detection(
            conn,
            service_id,
            Some(segment.id),
            translation_id,
            detection,
        )?;
    }

    let mut kept_suggestions = Vec::with_capacity(processed.suggestions.len());
    for suggestion in processed.suggestions {
        let reference_display = match &suggestion.kind {
            SuggestionKind::Scripture { reference } => reference.clone(),
            _ => String::new(),
        };
        let is_duplicate = !reference_display.is_empty()
            && has_recent_suggestion_for_reference(
                conn,
                service_id,
                &reference_display,
                SUGGESTION_DEDUP_WINDOW_SECONDS,
            )?;
        if is_duplicate {
            log::debug!(
                target: "cip::ai",
                "suppressed duplicate suggestion for {reference_display} (repeated within {SUGGESTION_DEDUP_WINDOW_SECONDS}s)"
            );
            continue;
        }

        let suggestion = suggestion.with_source(segment.id, segment.text.clone());
        persist_suggestion(conn, &suggestion)?;
        kept_suggestions.push(suggestion);
    }
    processed.suggestions = kept_suggestions;
    log::debug!(
        target: "cip::performance",
        "persist detections+suggestions took {:?} ({} suggestion(s) kept)",
        persist_start.elapsed(),
        processed.suggestions.len()
    );

    Ok(processed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{list_suggestions, persist_service};
    use cip_core_ai::TranscriptSegment;
    use cip_core_bible::ScriptureContextManager;
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

    /// Phase 1.3's deduplication policy: the pastor repeating "Romans
    /// 8:28" moments later must not flood the suggestion queue - see
    /// `handle_final_transcript`'s docs and
    /// `persistence::has_recent_suggestion_for_reference`.
    #[test]
    fn repeating_the_same_reference_within_the_dedup_window_does_not_create_a_second_suggestion() {
        let conn = seeded_db();
        let session = ServiceSession::start("Dedup Test");
        persist_service(&conn, &session).unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");

        let first = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Romans 8:28", 0),
        )
        .unwrap();
        assert_eq!(first.suggestions.len(), 1);

        let second = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Romans 8:28", 1),
        )
        .unwrap();
        assert!(
            second.suggestions.is_empty(),
            "an immediate repeat within the dedup window must not create a second suggestion"
        );
        // The detection itself is never suppressed - only suggestion
        // creation is. The transcript/detection record stays complete.
        assert_eq!(second.detections.len(), 1);

        let all_suggestions = list_suggestions(&conn, session.id, None).unwrap();
        assert_eq!(
            all_suggestions.len(),
            1,
            "only one suggestion should ever be persisted"
        );
    }

    /// A different reference is never suppressed, even moments after an
    /// unrelated one - dedup keys on the reference, not just recency.
    #[test]
    fn a_different_reference_shortly_after_is_not_treated_as_a_duplicate() {
        let conn = seeded_db();
        let session = ServiceSession::start("Dedup Test 2");
        persist_service(&conn, &session).unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");

        handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Romans 8:28", 0),
        )
        .unwrap();
        let second = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Romans 8:31", 1),
        )
        .unwrap();

        assert_eq!(
            second.suggestions.len(),
            1,
            "a distinct reference must still produce a suggestion"
        );
    }

    // --- Phase 1.3: context correction ----------------------------------

    /// Section 39: an operator correction takes effect for subsequent bare
    /// verses exactly like an automatic chapter detection would, and never
    /// rewrites the transcript segment(s) that led up to it.
    #[test]
    fn operator_context_correction_updates_active_context_without_altering_transcript_history() {
        let conn = seeded_db();
        let session = ServiceSession::start("Context Correction Test");
        persist_service(&conn, &session).unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        // Romans 7 isn't in the dev seed - insert a chapter + one verse so
        // "Romans 7" is a real, validatable chapter for this scenario
        // (chapter existence is derived from having at least one verse -
        // see `docs/bible-intelligence.md` - but `bible_verses` itself has
        // a foreign key to `bible_chapters`, so that row must exist too).
        provider_conn
            .execute(
                "INSERT INTO bible_chapters (translation_id, book_code, chapter_number, verse_count)
                 VALUES ('KJV', 'ROM', 7, 1)",
                [],
            )
            .unwrap();
        provider_conn
            .execute(
                "INSERT INTO bible_verses (translation_id, book_code, chapter_number, verse_number, text)
                 VALUES ('KJV', 'ROM', 7, 1, 'placeholder')",
                [],
            )
            .unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");

        // Pastor says Romans 7 - CIP is about to have this corrected.
        handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Turn to Romans chapter 7.", 0),
        )
        .unwrap();
        assert_eq!(context.active_context().unwrap().chapter, 7);

        // The operator correction: the same `resolve()` call
        // `commands::correct_scripture_context` makes after validating
        // the book+chapter against the `BibleProvider`.
        provider
            .get_chapter("KJV", "ROM", 8)
            .unwrap()
            .expect("ROM 8 must be a real chapter");
        context.resolve(cip_core_bible::PartialScriptureReference {
            book: Some("ROM".to_string()),
            chapter: Some(8),
            verse_start: None,
            verse_end: None,
        });
        assert_eq!(
            context.active_context().unwrap().chapter,
            8,
            "correction takes effect immediately"
        );

        // A subsequent bare verse resolves against the corrected chapter.
        let processed = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("verse 28", 1),
        )
        .unwrap();
        assert_eq!(
            processed.detections[0]
                .reference
                .as_ref()
                .unwrap()
                .to_string(),
            "ROM 8:28",
            "\"verse 28\" now resolves against the corrected Romans 8, not the original Romans 7"
        );

        // The original transcript segment is untouched - a context
        // correction never rewrites historical transcript content.
        let stored = crate::persistence::list_transcript_segments(&conn, session.id, 10).unwrap();
        assert_eq!(stored[0].text, "Turn to Romans chapter 7.");
    }

    // --- Phase 1.3: ambiguity resolution ---------------------------------

    /// Section 40: an ambiguous detection never becomes a suggestion on
    /// its own; only an explicit operator choice (the same steps
    /// `commands::resolve_ambiguous_reference` performs) creates one, and
    /// it is still `Pending`, never auto-approved.
    #[test]
    fn operator_resolves_ambiguous_reference_into_a_pending_suggestion_only_after_explicit_action()
    {
        let conn = seeded_db();
        let session = ServiceSession::start("Ambiguity Test");
        persist_service(&conn, &session).unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        // Romans 8:16 isn't in the dev seed - add it so a bare "verse 16"
        // is genuinely ambiguous against both it and John 3:16.
        provider_conn
            .execute(
                "INSERT INTO bible_verses (translation_id, book_code, chapter_number, verse_number, text)
                 VALUES ('KJV', 'ROM', 8, 16, 'The Spirit itself beareth witness with our spirit.')",
                [],
            )
            .unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");

        handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("John chapter 3", 0),
        )
        .unwrap();
        handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Romans chapter 8", 1),
        )
        .unwrap();
        let processed = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("verse 16", 2),
        )
        .unwrap();

        let detection = &processed.detections[0];
        assert_eq!(detection.kind, cip_core_bible::ReferenceKind::Ambiguous);
        assert_eq!(detection.candidates.len(), 2);
        assert!(
            processed.suggestions.is_empty(),
            "must not guess - no suggestion until the operator chooses"
        );
        assert_eq!(
            crate::persistence::list_suggestions(&conn, session.id, None)
                .unwrap()
                .len(),
            0
        );

        // Operator picks the current-context candidate (Romans 8:16).
        let chosen = detection.candidates[0].reference.clone();
        assert_eq!(chosen.book, "ROM");
        provider
            .get_verse(&chosen)
            .unwrap()
            .expect("the chosen candidate must be independently validated, not trusted blindly");
        context.record_resolved(chosen.clone());

        let confidence = ConfidenceResult::new(
            1.0,
            ConfidenceSource::Human,
            Some("operator resolved an ambiguous reference".to_string()),
        );
        let suggestion = cip_core_ai::Suggestion::new(
            session.id,
            cip_core_ai::SuggestionKind::Scripture {
                reference: chosen.to_string(),
            },
            confidence,
        );
        crate::persistence::persist_suggestion(&conn, &suggestion).unwrap();

        let persisted = crate::persistence::list_suggestions(&conn, session.id, None).unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(
            persisted[0].status,
            cip_core_ai::SuggestionStatus::Pending,
            "an operator's ambiguity resolution is never auto-approved"
        );
    }

    // --- Phase 1.3: no automatic projection (section 43) -----------------

    #[test]
    fn high_confidence_detection_never_creates_a_projected_presentation_state() {
        let conn = seeded_db();
        let session = ServiceSession::start("No Auto Projection Test");
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

        assert_eq!(processed.suggestions.len(), 1);
        assert!(
            processed.suggestions[0].confidence.score > 0.9,
            "this is exactly the high-confidence case the rule must hold for"
        );
        assert_eq!(
            processed.suggestions[0].status,
            cip_core_ai::SuggestionStatus::Pending
        );

        let presentation_count: i64 = conn
            .query_row("SELECT count(*) FROM presentation_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            presentation_count, 0,
            "nothing in the pipeline may create a presentation item on its own, regardless of confidence"
        );
    }

    // --- Phase 1.3: restart / recovery (section 33) -----------------------

    /// Proves service history is reconstructable purely from SQLite after
    /// the application closes and reopens - a real file-backed database,
    /// not the same in-memory connection kept open, so the connection
    /// being dropped and reopened is the actual thing being tested.
    #[test]
    fn service_history_survives_a_simulated_application_restart() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cip-restart-test.sqlite3");
        let session_id;

        {
            let mut conn = cip_database::open(&db_path).unwrap();
            run_migrations(&mut conn).unwrap();

            let session = ServiceSession::start("Restart Test");
            session_id = session.id;
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
            let suggestion_id = processed.suggestions[0].id;
            crate::persistence::update_suggestion_status(
                &conn,
                suggestion_id,
                cip_core_ai::SuggestionStatus::Approved,
                None,
            )
            .unwrap();
            crate::timeline::record_event(
                &conn,
                Some(session.id),
                crate::events::AppEvent::ServiceStarted,
                crate::logging::LogCategory::App,
                serde_json::json!({}),
            )
            .unwrap();

            // "Close/restart the application": end the service, then drop
            // the connection at the end of this block.
            crate::persistence::update_service_status(
                &conn,
                session.id,
                cip_core_service::ServiceStatus::Ended,
                Some(chrono::Utc::now()),
            )
            .unwrap();
        }

        // Reopen, exactly as a fresh application launch would.
        let reopened = cip_database::open(&db_path).unwrap();

        let reloaded_service = crate::persistence::get_service(&reopened, session_id).unwrap();
        assert_eq!(
            reloaded_service.status,
            cip_core_service::ServiceStatus::Ended
        );

        let transcript =
            crate::persistence::list_transcript_segments(&reopened, session_id, 10).unwrap();
        assert_eq!(transcript.len(), 1);
        assert_eq!(transcript[0].text, "Romans 8:28");

        let suggestions =
            crate::persistence::list_suggestions(&reopened, session_id, None).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(
            suggestions[0].status,
            cip_core_ai::SuggestionStatus::Approved
        );

        let history = crate::timeline::list_timeline(&reopened, session_id, 10).unwrap();
        assert!(
            !history.is_empty(),
            "the timeline itself must be reconstructable, not just the raw rows"
        );
    }

    // --- Phase 1.3: the canonical end-to-end service simulation ----------
    //
    // Section 42/51's acceptance scenario: full lifecycle (start, pause,
    // resume, end), the Romans 8 -> John 3 sequence with real operator
    // approve/edit actions interleaved, continued (offline-capable)
    // operation, and every piece of history reconstructable afterward.
    #[test]
    fn phase_1_3_canonical_service_simulation() {
        let conn = seeded_db();
        let mut session = ServiceSession::start("Sunday Morning Service");
        persist_service(&conn, &session).unwrap();
        crate::timeline::record_event(
            &conn,
            Some(session.id),
            crate::events::AppEvent::ServiceStarted,
            crate::logging::LogCategory::App,
            serde_json::json!({}),
        )
        .unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");
        let mut seq = 0u64;

        macro_rules! process {
            ($text:expr) => {{
                let result = handle_final_transcript(
                    &conn,
                    &provider,
                    &mut context,
                    session.id,
                    "KJV",
                    segment($text, seq),
                )
                .unwrap();
                seq += 1;
                result
            }};
        }

        process!("Turn with me to Romans chapter eight");
        assert_eq!(context.active_context().unwrap().book, "ROM");
        process!("Paul is teaching us something important.");
        process!("We must understand the work of the Spirit.");
        let p1 = process!("Look at verse twenty-eight");
        assert_eq!(p1.suggestions.len(), 1);
        let romans_828 = p1.suggestions[0].id;

        // Operator approves Romans 8:28.
        let approved = crate::persistence::update_suggestion_status(
            &conn,
            romans_828,
            cip_core_ai::SuggestionStatus::Approved,
            None,
        )
        .unwrap();
        assert_eq!(approved.status, cip_core_ai::SuggestionStatus::Approved);
        crate::timeline::record_event(
            &conn,
            Some(session.id),
            crate::events::AppEvent::SuggestionApproved,
            crate::logging::LogCategory::Ai,
            serde_json::json!({ "suggestionId": approved.id }),
        )
        .unwrap();

        let p2 = process!("Now verse thirty-one");
        assert_eq!(
            p2.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:31"
        );
        let romans_831 = p2.suggestions[0].id;
        // Operator edits (to the same, already-correct reference - proving
        // edit validation accepts a real verse) then approves.
        crate::persistence::update_suggestion_status(
            &conn,
            romans_831,
            cip_core_ai::SuggestionStatus::Edited,
            Some(&cip_core_ai::SuggestionKind::Scripture {
                reference: "ROM 8:31".to_string(),
            }),
        )
        .unwrap();
        crate::persistence::update_suggestion_status(
            &conn,
            romans_831,
            cip_core_ai::SuggestionStatus::Approved,
            None,
        )
        .unwrap();

        let p3 = process!("Go back to verse eighteen");
        assert_eq!(
            p3.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:18"
        );

        let p4 = process!("Now let's go to John chapter three");
        assert_eq!(context.active_context().unwrap().book, "JHN");
        assert!(
            p4.suggestions.is_empty(),
            "a bare chapter never suggests a verse"
        );

        let p5 = process!("Verse sixteen");
        assert_eq!(
            p5.detections[0].reference.as_ref().unwrap().to_string(),
            "JHN 3:16"
        );

        // Operator pauses, then resumes mid-service.
        session.pause();
        crate::persistence::update_service_status(
            &conn,
            session.id,
            session.status,
            session.ended_at,
        )
        .unwrap();
        crate::timeline::record_event(
            &conn,
            Some(session.id),
            crate::events::AppEvent::ServicePaused,
            crate::logging::LogCategory::App,
            serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(session.status, cip_core_service::ServiceStatus::Paused);

        session.resume();
        crate::persistence::update_service_status(
            &conn,
            session.id,
            session.status,
            session.ended_at,
        )
        .unwrap();
        crate::timeline::record_event(
            &conn,
            Some(session.id),
            crate::events::AppEvent::ServiceResumed,
            crate::logging::LogCategory::App,
            serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(session.status, cip_core_service::ServiceStatus::Started);

        // "Disconnect network" and continue - the pipeline has no network
        // dependency at all (see
        // `the_pipeline_produces_identical_results_with_no_network_access_possible`),
        // so it keeps working identically.
        let p6 = process!("Let's return to Romans chapter eight");
        assert_eq!(context.active_context().unwrap().book, "ROM");
        assert_eq!(context.active_context().unwrap().chapter, 8);
        assert!(
            p6.suggestions.is_empty(),
            "re-establishing a chapter never suggests a verse on its own"
        );
        let _ = seq; // no further segments - silences unused-assignment on the macro's last increment

        // End the service.
        session.end();
        crate::persistence::update_service_status(
            &conn,
            session.id,
            session.status,
            session.ended_at,
        )
        .unwrap();
        crate::timeline::record_event(
            &conn,
            Some(session.id),
            crate::events::AppEvent::ServiceEnded,
            crate::logging::LogCategory::App,
            serde_json::json!({}),
        )
        .unwrap();

        // --- Verify everything the spec asks stays reconstructable ---

        let transcript =
            crate::persistence::list_transcript_segments(&conn, session.id, 20).unwrap();
        assert_eq!(
            transcript.len(),
            9,
            "every final segment persisted, including the two chapter-only ones"
        );

        let detections_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM scripture_detections WHERE service_id = ?1",
                [session.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert!(detections_count >= 6, "every validated detection persisted");

        let suggestions = crate::persistence::list_suggestions(&conn, session.id, None).unwrap();
        assert_eq!(
            suggestions.len(),
            4,
            "ROM 8:28, ROM 8:31, ROM 8:18, JHN 3:16"
        );
        assert_eq!(
            suggestions
                .iter()
                .filter(|s| s.status == cip_core_ai::SuggestionStatus::Approved)
                .count(),
            2
        );
        assert_eq!(
            suggestions
                .iter()
                .filter(|s| s.status == cip_core_ai::SuggestionStatus::Pending)
                .count(),
            2
        );

        let history_entries = crate::timeline::list_timeline(&conn, session.id, 50).unwrap();
        for expected in [
            "SERVICE_STARTED",
            "SERVICE_PAUSED",
            "SERVICE_RESUMED",
            "SERVICE_ENDED",
            "SUGGESTION_APPROVED",
        ] {
            assert!(
                history_entries.iter().any(|e| e.event_name == expected),
                "timeline missing {expected}"
            );
        }

        let recent: Vec<String> = context
            .recent_references(10)
            .iter()
            .map(|r| r.to_string())
            .collect();
        assert!(recent.contains(&"ROM 8:28".to_string()));
        assert!(recent.contains(&"JHN 3:16".to_string()));

        let final_service = crate::persistence::get_service(&conn, session.id).unwrap();
        assert_eq!(final_service.status, cip_core_service::ServiceStatus::Ended);
        assert!(final_service.ended_at.is_some());

        let history = crate::persistence::list_services(
            &conn,
            Some(cip_core_service::ServiceStatus::Ended),
            10,
        )
        .unwrap();
        assert!(history.iter().any(|s| s.id == session.id));
    }

    // --- Phase 1.4: presentation foundation acceptance --------------------
    //
    // The "most important acceptance criterion": the complete chain
    // SERVICE -> TRANSCRIPT -> "Romans 8" -> "verse 28" -> ROM 8:28 ->
    // PENDING SUGGESTION -> HUMAN APPROVAL -> PREVIEW -> PREPARE -> real
    // local Bible text -> PERSISTED OUTPUT -> SERVICE TIMELINE, then the
    // same reference prepared again through the offline/manual path with
    // no suggestion involved, with explicit proof that nothing here ever
    // auto-approves, auto-prepares, or auto-projects.
    #[test]
    fn phase_1_4_presentation_foundation_acceptance() {
        let conn = seeded_db();
        let session = ServiceSession::start("Phase 1.4 Acceptance");
        persist_service(&conn, &session).unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");

        // SERVICE -> TRANSCRIPT -> "Romans 8" -> "verse 28" -> ROM 8:28 ->
        // PENDING SUGGESTION.
        handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Turn with me to Romans chapter 8", 0),
        )
        .unwrap();
        let processed = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Look at verse 28", 1),
        )
        .unwrap();
        assert_eq!(processed.suggestions.len(), 1);
        let suggestion = &processed.suggestions[0];
        assert_eq!(suggestion.status, cip_core_ai::SuggestionStatus::Pending);
        let cip_core_ai::SuggestionKind::Scripture { reference } = &suggestion.kind else {
            panic!("expected a scripture suggestion");
        };
        assert_eq!(reference, "ROM 8:28");

        // A detected Scripture must NOT automatically become a prepared
        // presentation item, no matter how confident the detection - only
        // an explicit operator approval, followed by an explicit prepare
        // call, may do that.
        let auto_prepared_count: i64 = conn
            .query_row("SELECT count(*) FROM presentation_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            auto_prepared_count, 0,
            "a pending suggestion must never automatically produce a presentation item"
        );

        // HUMAN APPROVAL - only an explicit operator action moves this
        // suggestion out of Pending.
        let approved = crate::persistence::update_suggestion_status(
            &conn,
            suggestion.id,
            cip_core_ai::SuggestionStatus::Approved,
            None,
        )
        .unwrap();
        assert_eq!(approved.status, cip_core_ai::SuggestionStatus::Approved);

        // PREVIEW - non-mutating, must not create a presentation_items row.
        let (preview_content, preview_slide) =
            crate::presentation::build_scripture_slide(&provider, "KJV", reference).unwrap();
        let count_after_preview: i64 = conn
            .query_row("SELECT count(*) FROM presentation_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count_after_preview, 0,
            "preview must never persist a presentation item"
        );
        let cip_core_presentation::PresentationContent::Scripture { text, .. } = &preview_content
        else {
            panic!("expected scripture content");
        };
        assert!(
            text.contains("all things work together for good"),
            "preview must show real local Bible text, not invented/AI text: {text:?}"
        );
        assert_eq!(
            preview_slide.template,
            cip_presentation_renderer::SCRIPTURE_DEFAULT_TEMPLATE
        );

        // PREPARE - persists, records which suggestion it came from.
        let item = crate::presentation::persist_prepared_item(
            &conn,
            session.id,
            preview_content.clone(),
            cip_presentation_renderer::SCRIPTURE_DEFAULT_TEMPLATE,
            Some(suggestion.id),
        )
        .unwrap();
        assert_eq!(
            item.status,
            cip_core_presentation::PresentationItemStatus::Prepared
        );
        assert_eq!(item.source_suggestion_id, Some(suggestion.id));
        crate::timeline::record_event(
            &conn,
            Some(session.id),
            crate::events::AppEvent::PresentationPrepared,
            crate::logging::LogCategory::Presentation,
            serde_json::json!({ "presentationItemId": item.id, "reference": reference }),
        )
        .unwrap();

        // PERSISTED OUTPUT - reloadable independent of the in-memory value.
        let reloaded = crate::persistence::get_presentation_item(&conn, item.id).unwrap();
        assert_eq!(reloaded, item);
        assert_eq!(reloaded.content, preview_content);

        // SERVICE TIMELINE - the prepare is reconstructable from history,
        // not a side channel the timeline doesn't know about.
        let timeline_entries = crate::timeline::list_timeline(&conn, session.id, 20).unwrap();
        assert!(timeline_entries
            .iter()
            .any(|e| e.event_name == "PRESENTATION_PREPARED"));

        // No automatic projection: nothing in this whole chain ever set
        // the item to `Active` - the only states reachable from this test
        // are `Prepared` (and, if cancelled, `Stopped`).
        assert_ne!(
            reloaded.status,
            cip_core_presentation::PresentationItemStatus::Active
        );

        // --- Offline / manual fallback: the same reference prepared again
        // with no suggestion, no speech engine, no network - proving the
        // manual path produces the identical real Bible text independent
        // of the automatic detection path.
        let (manual_content, _) =
            crate::presentation::build_scripture_slide(&provider, "KJV", "ROM 8:28").unwrap();
        assert_eq!(
            manual_content, preview_content,
            "the automatic and manual paths must produce identical, real Bible-sourced content"
        );
        let manual_item = crate::presentation::persist_prepared_item(
            &conn,
            session.id,
            manual_content,
            cip_presentation_renderer::SCRIPTURE_DEFAULT_TEMPLATE,
            None,
        )
        .unwrap();
        assert_eq!(
            manual_item.source_suggestion_id, None,
            "a manually-prepared item must never be attributed to a suggestion it didn't come from"
        );
        assert_eq!(
            manual_item.status,
            cip_core_presentation::PresentationItemStatus::Prepared
        );

        // Every presentation item ever produced in this test belongs to
        // the service it was prepared during - no orphaned items.
        let all_items =
            crate::persistence::list_presentation_items(&conn, session.id, None).unwrap();
        assert_eq!(all_items.len(), 2);
        assert!(all_items.iter().all(|i| i.service_id == session.id));
        assert!(
            all_items
                .iter()
                .all(|i| i.status != cip_core_presentation::PresentationItemStatus::Active),
            "no automatic projection: nothing here may ever reach Active on its own"
        );
    }

    /// Explicit proof (section 15/19): an invalid reference is rejected
    /// with no presentation item, no prepared output, and no silent
    /// substitution of a different translation or verse.
    #[test]
    fn invalid_scripture_never_produces_a_presentation_item() {
        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);

        let err = crate::presentation::build_scripture_slide(&provider, "KJV", "ROM 999:999")
            .unwrap_err();
        assert!(matches!(
            err,
            crate::presentation::PresentationError::VerseNotFound(_)
        ));
    }

    /// Restart/recovery (section 28): a prepared item, and the timeline
    /// entry recording it, both survive a simulated application restart -
    /// and the item is still exactly `Prepared`, never auto-advanced to
    /// `Active`, after reopening.
    #[test]
    fn prepared_presentation_items_survive_a_simulated_restart_and_stay_prepared() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cip-presentation-restart-test.sqlite3");
        let session_id;
        let item_id;

        {
            let mut conn = cip_database::open(&db_path).unwrap();
            run_migrations(&mut conn).unwrap();
            let session = ServiceSession::start("Presentation Restart Test");
            session_id = session.id;
            persist_service(&conn, &session).unwrap();

            let mut provider_conn = open_in_memory().unwrap();
            run_migrations(&mut provider_conn).unwrap();
            apply_dev_seed(&provider_conn).unwrap();
            let provider = SqliteBibleProvider::new(provider_conn);

            let (content, _) =
                crate::presentation::build_scripture_slide(&provider, "KJV", "JHN 3:16").unwrap();
            let item = crate::presentation::persist_prepared_item(
                &conn,
                session.id,
                content,
                cip_presentation_renderer::SCRIPTURE_DEFAULT_TEMPLATE,
                None,
            )
            .unwrap();
            item_id = item.id;
            crate::timeline::record_event(
                &conn,
                Some(session.id),
                crate::events::AppEvent::PresentationPrepared,
                crate::logging::LogCategory::Presentation,
                serde_json::json!({ "presentationItemId": item.id }),
            )
            .unwrap();

            // "Close/restart the application": drop the connection at the
            // end of this block - nothing here ever displays the item, so
            // it must reopen exactly as `Prepared`.
        }

        let reopened = cip_database::open(&db_path).unwrap();
        let reloaded = crate::persistence::get_presentation_item(&reopened, item_id).unwrap();
        assert_eq!(
            reloaded.status,
            cip_core_presentation::PresentationItemStatus::Prepared,
            "a restart must never advance a prepared item toward display on its own"
        );

        let timeline = crate::timeline::list_timeline(&reopened, session_id, 10).unwrap();
        assert!(timeline
            .iter()
            .any(|e| e.event_name == "PRESENTATION_PREPARED"));
    }
}
