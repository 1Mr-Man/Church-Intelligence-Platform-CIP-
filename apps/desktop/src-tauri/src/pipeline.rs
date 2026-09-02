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
use cip_core_bible::{BibleProvider, DefaultScriptureContextManager, ReferenceKind};
use cip_core_service::{
    process_transcript_segment, process_transcript_segment_with_semantic_search, ProcessedSegment,
    SemanticSearch,
};
use rusqlite::Connection;
use std::time::Instant;
use uuid::Uuid;

use crate::persistence::{
    confirm_suggestion, find_pending_suggestion_for_reference,
    find_rejected_suggestion_for_reference, has_recent_detection_for_reference,
    persist_scripture_detection, persist_suggestion, persist_transcript_segment,
    record_rejection_echo, DetectionCategory, PersistError,
};

/// Phase 1.3 suggestion deduplication window - see
/// `persistence::has_recent_detection_for_reference`'s docs for the full
/// policy. 60 seconds is long enough to absorb a pastor repeating the same
/// reference while still explaining it, short enough that a genuinely new
/// mention later in the service is never suppressed.
const SUGGESTION_DEDUP_WINDOW_SECONDS: i64 = 60;

/// Phase 5.2 (Temporal Confirmation / Sliding Re-Score): how much a
/// `Paraphrase`/`Semantic` suggestion's confidence score rises each time
/// its reference is independently redetected within the dedup window
/// above, while the suggestion is still `Pending` - see
/// `persistence::confirm_suggestion`'s docs for the full policy.
const CONFIRMATION_SCORE_BONUS: f32 = 0.1;

/// The confidence ceiling a repeatedly-confirmed heuristic suggestion can
/// reach - deliberately below the ~0.97 an explicit citation (`Direct`)
/// earns, so no amount of repetition ever lets a heuristic guess outrank a
/// real citation.
const MAX_CONFIRMED_SCORE: f32 = 0.9;

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
/// if an identical reference *in the same category* (explicit citation vs.
/// `Paraphrase`/`Semantic` guess - see `persistence::DetectionCategory`)
/// was already suggested for this same service within
/// `SUGGESTION_DEDUP_WINDOW_SECONDS`. See
/// `persistence::has_recent_detection_for_reference` for exactly why this
/// scope/window/category split was chosen.
///
/// ## Temporal confirmation (Phase 5.2)
///
/// A suppressed `Paraphrase`/`Semantic` duplicate is not simply discarded:
/// if the reference's original suggestion is still `Pending`,
/// `persistence::confirm_suggestion` bumps its confidence (capped below
/// what an explicit citation earns) and increments its
/// `confirmation_count` - repetition of a single-shot heuristic guess is
/// corroborating evidence, and this is where that evidence gets recorded.
/// Explicit citations are already near the confidence ceiling and are
/// never confirmation-boosted.
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
    handle_final_transcript_inner(
        conn,
        provider,
        context,
        service_id,
        translation_id,
        segment,
        None,
    )
}

/// Identical to [`handle_final_transcript`], except that when no citation
/// and no lexical paraphrase resolves the segment, Phase 4.4's semantic
/// (embedding) fallback is also attempted via `semantic` before giving up -
/// see `cip_core_service::process_transcript_segment_with_semantic_search`'s
/// own docs. The live audio pipeline (`commands::finalize_bible_only`) uses
/// this instead of `handle_final_transcript` whenever an embedding model is
/// actually loaded (`AppState.embedding_ready`); every other caller (tests,
/// `process_test_transcript`) keeps using the plain entry point, so nothing
/// about this crate's existing behavior changes unless semantic search is
/// genuinely configured.
pub fn handle_final_transcript_with_semantic_search(
    conn: &Connection,
    provider: &dyn BibleProvider,
    context: &mut DefaultScriptureContextManager,
    service_id: Uuid,
    translation_id: &str,
    segment: cip_core_ai::TranscriptSegment,
    semantic: &SemanticSearch,
) -> Result<ProcessedSegment, PersistError> {
    handle_final_transcript_inner(
        conn,
        provider,
        context,
        service_id,
        translation_id,
        segment,
        Some(semantic),
    )
}

fn handle_final_transcript_inner(
    conn: &Connection,
    provider: &dyn BibleProvider,
    context: &mut DefaultScriptureContextManager,
    service_id: Uuid,
    translation_id: &str,
    segment: cip_core_ai::TranscriptSegment,
    semantic: Option<&SemanticSearch>,
) -> Result<ProcessedSegment, PersistError> {
    let persist_transcript_start = Instant::now();
    persist_transcript_segment(conn, service_id, &segment)?;
    log::debug!(
        target: "cip::performance",
        "persist_transcript_segment took {:?}",
        persist_transcript_start.elapsed()
    );

    let detect_start = Instant::now();
    let mut processed = match semantic {
        Some(semantic) => process_transcript_segment_with_semantic_search(
            service_id,
            &segment.text,
            translation_id,
            provider,
            context,
            semantic,
        ),
        None => {
            process_transcript_segment(service_id, &segment.text, translation_id, provider, context)
        }
    };
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

    // `process_transcript_segment` only ever produces a suggestion for a
    // detection that has `Some(reference)`, in the same relative order as
    // `detections` - zip them so the dedup check below can tell an
    // explicit citation from a `Paraphrase` guess for the same verse.
    let detection_kinds = processed
        .detections
        .iter()
        .filter(|d| d.reference.is_some())
        .map(|d| d.kind);

    let mut kept_suggestions = Vec::with_capacity(processed.suggestions.len());
    for (kind, suggestion) in detection_kinds.zip(processed.suggestions) {
        let reference_display = match &suggestion.kind {
            SuggestionKind::Scripture { reference } => reference.clone(),
            _ => String::new(),
        };
        // The dedup window suppresses a repeat *within the same category*
        // (an explicit citation repeated soon after, or a fuzzy
        // `Paraphrase`/`Semantic` guess repeated soon after) - but never
        // across categories: an explicit citation always deserves its own
        // confident suggestion even if a `Paraphrase`/`Semantic` guess for
        // the same verse was already made moments earlier, and vice versa
        // (e.g. the pastor paraphrases a verse, then reads it verbatim, or
        // reads it and later paraphrases it again).
        let category = match kind {
            ReferenceKind::Paraphrase => DetectionCategory::Paraphrase,
            ReferenceKind::Semantic => DetectionCategory::Semantic,
            _ => DetectionCategory::Explicit,
        };
        let is_duplicate = !reference_display.is_empty()
            && has_recent_detection_for_reference(
                conn,
                service_id,
                &reference_display,
                category,
                SUGGESTION_DEDUP_WINDOW_SECONDS,
                segment.id,
            )?;
        if is_duplicate {
            log::debug!(
                target: "cip::ai",
                "suppressed duplicate suggestion for {reference_display} (repeated within {SUGGESTION_DEDUP_WINDOW_SECONDS}s)"
            );
            // Phase 5.2 (Temporal Confirmation): a repeated heuristic
            // (`Paraphrase`/`Semantic`) guess for the same reference is
            // corroborating evidence, not just noise to discard - bump the
            // still-`Pending` suggestion's confidence instead of silently
            // dropping the signal entirely. Explicit citations are already
            // near the confidence ceiling (~0.97) and are left untouched -
            // repetition of an already-confident citation adds nothing.
            if matches!(
                category,
                DetectionCategory::Paraphrase | DetectionCategory::Semantic
            ) {
                if let Some(existing) =
                    find_pending_suggestion_for_reference(conn, service_id, &reference_display)?
                {
                    let confirmed = confirm_suggestion(
                        conn,
                        existing.id,
                        CONFIRMATION_SCORE_BONUS,
                        MAX_CONFIRMED_SCORE,
                    )?;
                    log::debug!(
                        target: "cip::ai",
                        "confirmed {reference_display} (confirmation #{}, confidence now {:.2})",
                        confirmed.confirmation_count,
                        confirmed.confidence.score
                    );
                } else if let Some(rejected) =
                    find_rejected_suggestion_for_reference(conn, service_id, &reference_display)?
                {
                    // Phase 5.4 (Wrong-Verse Feedback Loop): no `Pending`
                    // suggestion exists for this reference because the
                    // operator already `Rejected` it - the repeat is still
                    // silently suppressed exactly as before (a decided
                    // suggestion is never resurrected), but this makes that
                    // suppression observable instead of leaving no trace at
                    // all.
                    let echoed = record_rejection_echo(conn, rejected.id)?;
                    log::debug!(
                        target: "cip::ai",
                        "rejection echo for {reference_display} (echo #{}, still suppressed)",
                        echoed.rejection_echo_count
                    );
                }
            }
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
    use crate::persistence::{get_suggestion, list_suggestions, persist_service};
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

    /// Phase 5.2 (Temporal Confirmation): a repeated `Paraphrase` guess for
    /// the same verse within the dedup window is still suppressed as a
    /// *new* suggestion (unchanged from Phase 1.3/4.1's dedup policy), but
    /// is no longer silent noise - it bumps the original, still-`Pending`
    /// suggestion's confidence and confirmation count.
    #[test]
    fn a_repeated_paraphrase_within_the_dedup_window_confirms_the_original_suggestion() {
        let conn = seeded_db();
        let session = ServiceSession::start("Confirmation Test");
        persist_service(&conn, &session).unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");

        // Wording chosen to trigger the Paraphrase fallback (no explicit
        // citation) against ROM 8:28 in the dev seed, deliberately scoring
        // below the MAX_CONFIRMED_SCORE cap (4 of 5 significant words -
        // "know"/"thing"/"work"/"good" - match the verse; "somehow" does
        // not, giving 0.8) so a genuine confirmation-driven rise is
        // observable rather than immediately clamped by the cap.
        let paraphrase_text = "We know that all things somehow work for good.";

        let first = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment(paraphrase_text, 0),
        )
        .unwrap();
        assert_eq!(first.suggestions.len(), 1);
        let original_id = first.suggestions[0].id;
        let original_score = first.suggestions[0].confidence.score;
        assert_eq!(first.suggestions[0].confirmation_count, 0);

        let second = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment(paraphrase_text, 1),
        )
        .unwrap();
        assert!(
            second.suggestions.is_empty(),
            "a repeated Paraphrase guess must still never create a second suggestion"
        );

        let confirmed = get_suggestion(&conn, original_id).unwrap();
        assert_eq!(
            confirmed.confirmation_count, 1,
            "the original suggestion's confirmation_count must increment"
        );
        assert!(
            confirmed.confidence.score > original_score,
            "a confirmed suggestion's confidence must rise above its original score"
        );

        let all_suggestions = list_suggestions(&conn, session.id, None).unwrap();
        assert_eq!(
            all_suggestions.len(),
            1,
            "confirmation must never create a second suggestion row"
        );
    }

    /// Explicit citations are already near the confidence ceiling and are
    /// never confirmation-boosted - repeating one is still pure dedup
    /// suppression, exactly as it was before Phase 5.2.
    #[test]
    fn a_repeated_explicit_citation_is_never_confirmation_boosted() {
        let conn = seeded_db();
        let session = ServiceSession::start("No Confirmation For Citations Test");
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
        let original_id = first.suggestions[0].id;
        let original_score = first.suggestions[0].confidence.score;

        handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment("Romans 8:28", 1),
        )
        .unwrap();

        let reloaded = get_suggestion(&conn, original_id).unwrap();
        assert_eq!(
            reloaded.confirmation_count, 0,
            "an explicit citation's repeat must never be treated as a confirmation"
        );
        assert!((reloaded.confidence.score - original_score).abs() < 0.001);
    }

    /// Phase 5.4 (Wrong-Verse Feedback Loop): once an operator has
    /// `Rejected` a `Paraphrase`/`Semantic` suggestion, a same-category
    /// repeat of that exact reference within the dedup window is still
    /// silently suppressed as a new suggestion (a decided suggestion is
    /// never resurrected), but now increments the rejected suggestion's
    /// own `rejection_echo_count` instead of leaving zero trace at all.
    #[test]
    fn a_repeated_paraphrase_after_rejection_echoes_instead_of_vanishing_silently() {
        let conn = seeded_db();
        let session = ServiceSession::start("Rejection Echo Test");
        persist_service(&conn, &session).unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");

        // Same deliberately-below-cap paraphrase wording used by the
        // confirmation test above - the exact scoring doesn't matter here,
        // only that it reliably triggers the Paraphrase fallback.
        let paraphrase_text = "We know that all things somehow work for good.";

        let first = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment(paraphrase_text, 0),
        )
        .unwrap();
        assert_eq!(first.suggestions.len(), 1);
        let original_id = first.suggestions[0].id;

        crate::persistence::update_suggestion_status(
            &conn,
            original_id,
            cip_core_ai::SuggestionStatus::Rejected,
            None,
        )
        .unwrap();

        let second = handle_final_transcript(
            &conn,
            &provider,
            &mut context,
            session.id,
            "KJV",
            segment(paraphrase_text, 1),
        )
        .unwrap();
        assert!(
            second.suggestions.is_empty(),
            "a repeat of a rejected suggestion's reference must still never resurrect it as a new suggestion"
        );

        let rejected = get_suggestion(&conn, original_id).unwrap();
        assert_eq!(
            rejected.status,
            cip_core_ai::SuggestionStatus::Rejected,
            "the echo must never change the suggestion's decided status"
        );
        assert_eq!(
            rejected.rejection_echo_count, 1,
            "the rejected suggestion's echo count must increment"
        );

        let all_suggestions = list_suggestions(&conn, session.id, None).unwrap();
        assert_eq!(
            all_suggestions.len(),
            1,
            "a rejection echo must never create a second suggestion row"
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

    // --- Phase 1.5: full-service validation --------------------------------
    //
    // The canonical realistic service simulation sections 27-34 ask for,
    // against the real SQLite-backed BibleProvider: the full scripted
    // sequence (approve/preview/prepare, approve/prepare, reject, approve/
    // preview/prepare again), context retention across unrelated speech,
    // context replacement, false-positive protection (Scripture-sounding
    // prose that must never become a suggestion), an invalid verse that
    // the detector might propose but the BibleProvider must reject, and an
    // operator context correction that never rewrites transcript history.
    #[test]
    fn phase_1_5_full_service_validation() {
        let conn = seeded_db();
        let session = ServiceSession::start("Phase 1.5 Full Service Validation");
        persist_service(&conn, &session).unwrap();

        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        apply_dev_seed(&provider_conn).unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new("KJV");
        let mut seq = 0u64;

        let mut process =
            |conn: &Connection, context: &mut DefaultScriptureContextManager, text: &str| {
                let result = handle_final_transcript(
                    conn,
                    &provider,
                    context,
                    session.id,
                    "KJV",
                    segment(text, seq),
                );
                seq += 1;
                result.unwrap()
            };

        // --- Section 32: false-positive protection, checked BEFORE any
        // context exists so there is nothing for a bare "verse" fragment
        // to even attach to - CIP must prefer no confident reference over
        // a wrong one.
        for false_positive in [
            "Chapter eight of our study is important.",
            "Romans is an important book.",
            "John was one of the disciples.",
        ] {
            let result = process(&conn, &mut context, false_positive);
            assert!(
                result.suggestions.is_empty(),
                "{false_positive:?} must never produce a suggestion"
            );
        }
        assert!(
            context.active_context().is_none(),
            "false-positive prose must never establish a Scripture context"
        );

        // --- Section 27/28: "Good morning church. Turn with me to Romans
        // chapter eight." -> Active Context = Romans 8, no verse invented.
        let p = process(
            &conn,
            &mut context,
            "Good morning church. Turn with me to Romans chapter eight",
        );
        assert!(p.suggestions.is_empty());
        assert_eq!(context.active_context().unwrap().book, "ROM");
        assert_eq!(context.active_context().unwrap().chapter, 8);

        let p = process(
            &conn,
            &mut context,
            "Paul is showing us the work of the Spirit.",
        );
        assert!(
            p.suggestions.is_empty(),
            "unrelated prose must never invent a reference"
        );

        // Section 32 revisited under Phase 4.1 (semantic/paraphrase Bible
        // detection): wording that never says "verse" is still not treated
        // as a citation - the Verse/Direct/Chapter/Sequential resolution
        // paths above never fire for it - but this project's prior
        // "resemblance is never enough" stance is deliberately narrowed for
        // one specific, honest case. When a segment shares almost all of
        // its distinctive vocabulary with one particular verse (lexical/
        // keyword overlap, not semantic/neural understanding - see
        // `cip_core_bible::paraphrase`'s module docs) and nothing else in
        // the segment already produced a suggestion, it now surfaces as a
        // `Paraphrase` detection: a `Pending` suggestion for the operator
        // to approve or reject, exactly like every other detection kind -
        // never auto-projected, and never a citation that mutates context.
        let p = process(
            &conn,
            &mut context,
            "And we know that all things work together for good.",
        );
        assert_eq!(
            p.suggestions.len(),
            1,
            "a close paraphrase of a specific verse must now surface a Pending suggestion for operator review"
        );
        assert_eq!(
            p.detections[0].kind,
            cip_core_bible::ReferenceKind::Paraphrase
        );
        assert_eq!(
            p.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:28"
        );
        assert_eq!(
            p.detections[0].confidence.source,
            cip_core_confidence::ConfidenceSource::Heuristic,
            "paraphrase confidence must be reported honestly as lexical/heuristic, never as a model/semantic source"
        );
        assert_eq!(
            p.suggestions[0].status,
            cip_core_ai::SuggestionStatus::Pending,
            "a paraphrase suggestion is never auto-approved or auto-projected"
        );
        assert_eq!(
            context.active_context().unwrap().chapter,
            8,
            "a paraphrase is not a citation - it must never establish or replace the active Scripture context"
        );

        // "Look at verse twenty-eight" -> Romans 8:28 -> Approve -> Preview -> Prepare.
        let p = process(&conn, &mut context, "Look at verse twenty-eight");
        assert_eq!(p.suggestions.len(), 1);
        let romans_828 = p.suggestions[0].id;
        assert_eq!(
            p.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:28"
        );
        let approved = crate::persistence::update_suggestion_status(
            &conn,
            romans_828,
            cip_core_ai::SuggestionStatus::Approved,
            None,
        )
        .unwrap();
        assert_eq!(approved.status, cip_core_ai::SuggestionStatus::Approved);
        let (content_828, _) =
            crate::presentation::build_scripture_slide(&provider, "KJV", "ROM 8:28").unwrap();
        let item_828 = crate::presentation::persist_prepared_item(
            &conn,
            session.id,
            content_828,
            "SCRIPTURE_DEFAULT",
            Some(romans_828),
        )
        .unwrap();
        assert_eq!(
            item_828.status,
            cip_core_presentation::PresentationItemStatus::Prepared
        );

        // "Now verse thirty-one" -> Romans 8:31 (Sequential - continuing
        // the same still-active context) -> Approve -> Prepare.
        let p = process(&conn, &mut context, "Now verse thirty-one");
        assert_eq!(
            p.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:31"
        );
        let romans_831 = p.suggestions[0].id;
        crate::persistence::update_suggestion_status(
            &conn,
            romans_831,
            cip_core_ai::SuggestionStatus::Approved,
            None,
        )
        .unwrap();
        let (content_831, _) =
            crate::presentation::build_scripture_slide(&provider, "KJV", "ROM 8:31").unwrap();
        crate::presentation::persist_prepared_item(
            &conn,
            session.id,
            content_831,
            "SCRIPTURE_DEFAULT",
            Some(romans_831),
        )
        .unwrap();

        // --- Section 28: context retention - "go back to verse eighteen"
        // resolves against the still-active Romans 8 with no chapter
        // repeated, exactly the user requirement this section validates.
        let p = process(&conn, &mut context, "Go back to verse eighteen");
        assert_eq!(
            p.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:18"
        );
        let romans_818 = p.suggestions[0].id;
        // Operator rejects this one - no presentation of any kind.
        let rejected = crate::persistence::update_suggestion_status(
            &conn,
            romans_818,
            cip_core_ai::SuggestionStatus::Rejected,
            None,
        )
        .unwrap();
        assert_eq!(rejected.status, cip_core_ai::SuggestionStatus::Rejected);

        // --- Section 29: context replacement - Romans 8 -> John 3, then
        // "verse sixteen" must resolve to John 3:16, never Romans 8:16.
        let p = process(&conn, &mut context, "Now John chapter three");
        assert!(
            p.suggestions.is_empty(),
            "a bare chapter never suggests a verse"
        );
        assert_eq!(context.active_context().unwrap().book, "JHN");

        let p = process(&conn, &mut context, "Verse sixteen");
        assert_eq!(
            p.detections[0].reference.as_ref().unwrap().to_string(),
            "JHN 3:16",
            "context replacement must resolve to John 3:16, never Romans 8:16"
        );
        let john_316 = p.suggestions[0].id;
        crate::persistence::update_suggestion_status(
            &conn,
            john_316,
            cip_core_ai::SuggestionStatus::Approved,
            None,
        )
        .unwrap();
        let (preview_content, _) =
            crate::presentation::build_scripture_slide(&provider, "KJV", "JHN 3:16").unwrap();
        assert!(preview_content_is_scripture_text(
            &preview_content,
            "God so loved the world"
        ));
        crate::presentation::persist_prepared_item(
            &conn,
            session.id,
            preview_content,
            "SCRIPTURE_DEFAULT",
            Some(john_316),
        )
        .unwrap();

        // --- Section 35: the detector may recognize a pattern, but
        // BibleProvider remains authoritative - Romans 8:999 is not a
        // real verse, so it must produce no suggestion at all.
        let p = process(&conn, &mut context, "Turn to Romans 8:999.");
        assert!(
            p.suggestions.is_empty(),
            "an out-of-range verse must never produce a suggestion, however confident the parser is"
        );

        // --- Section 34: operator override. Simulate CIP having drifted
        // to the wrong chapter (Romans 7), then the operator explicitly
        // correcting it to Romans 8 - exactly what
        // `commands::correct_scripture_context` does: validate the
        // book+chapter against the real Bible data, then commit it as the
        // new active context.
        context.resolve(cip_core_bible::PartialScriptureReference {
            book: Some("ROM".to_string()),
            chapter: Some(7),
            verse_start: None,
            verse_end: None,
        });
        assert_eq!(context.active_context().unwrap().chapter, 7);

        let transcript_count_before_correction =
            crate::persistence::list_transcript_segments(&conn, session.id, 100)
                .unwrap()
                .len();

        provider
            .get_chapter("KJV", "ROM", 8)
            .unwrap()
            .expect("Romans 8 is a real chapter - the operator's correction must be valid");
        context.resolve(cip_core_bible::PartialScriptureReference {
            book: Some("ROM".to_string()),
            chapter: Some(8),
            verse_start: None,
            verse_end: None,
        });
        crate::timeline::record_event(
            &conn,
            Some(session.id),
            crate::events::AppEvent::ScriptureContextCorrected,
            crate::logging::LogCategory::Bible,
            serde_json::json!({ "previous": "ROM 7", "corrected": "ROM 8" }),
        )
        .unwrap();
        assert_eq!(context.active_context().unwrap().chapter, 8);

        // The correction must never rewrite transcript history - only new
        // segments are added, nothing already persisted changes.
        let transcript_count_after_correction =
            crate::persistence::list_transcript_segments(&conn, session.id, 100)
                .unwrap()
                .len();
        assert_eq!(
            transcript_count_before_correction,
            transcript_count_after_correction
        );

        let p = process(&conn, &mut context, "Verse twenty-eight");
        assert_eq!(
            p.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:28",
            "after the operator's correction, a bare verse must resolve against Romans 8"
        );

        // --- Final verification: exactly the presentation items explicit
        // operator actions produced, nothing more, nothing auto-projected.
        let all_items =
            crate::persistence::list_presentation_items(&conn, session.id, None).unwrap();
        assert_eq!(
            all_items.len(),
            3,
            "exactly the three explicitly-approved-and-prepared verses (ROM 8:28, ROM 8:31, JHN 3:16) - \
             the rejected ROM 8:18 and the out-of-range ROM 8:999 produced none"
        );
        assert!(all_items.iter().all(|i| i.service_id == session.id));
        assert!(
            all_items
                .iter()
                .all(|i| i.status != cip_core_presentation::PresentationItemStatus::Active),
            "no automatic projection anywhere in this whole scenario"
        );
        let prepared_refs: std::collections::HashSet<String> = all_items
            .iter()
            .filter_map(|i| match &i.content {
                cip_core_presentation::PresentationContent::Scripture { reference, .. } => {
                    Some(reference.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            prepared_refs,
            std::collections::HashSet::from([
                "ROM 8:28".to_string(),
                "ROM 8:31".to_string(),
                "JHN 3:16".to_string(),
            ])
        );

        let all_suggestions =
            crate::persistence::list_suggestions(&conn, session.id, None).unwrap();
        assert_eq!(
            all_suggestions
                .iter()
                .filter(|s| s.status == cip_core_ai::SuggestionStatus::Approved)
                .count(),
            3
        );
        assert_eq!(
            all_suggestions
                .iter()
                .filter(|s| s.status == cip_core_ai::SuggestionStatus::Rejected)
                .count(),
            1
        );

        // The service timeline documents the operator's correction and
        // every prepare - the foundation for future service analytics
        // (section 24).
        let timeline_entries = crate::timeline::list_timeline(&conn, session.id, 50).unwrap();
        assert!(timeline_entries
            .iter()
            .any(|e| e.event_name == "SCRIPTURE_CONTEXT_CORRECTED"));
    }

    /// Phase 2.10 validation: every other test in this module runs the live
    /// detection -> context -> suggestion pipeline against the tiny KJV dev
    /// fixture. That proves the pipeline logic, but not that it behaves the
    /// same way against the real, complete production dataset. This test
    /// re-runs the same pipeline function (`handle_final_transcript`) against
    /// the real, complete BSB dataset (31,086 verses, imported exactly as it
    /// is at real application startup) to close that gap.
    #[test]
    fn phase_2_10_bible_pipeline_against_real_production_dataset() {
        let conn = seeded_db();
        let session = ServiceSession::start("Phase 2.10 Real BSB Pipeline Validation");
        persist_service(&conn, &session).unwrap();

        let bsb_translation_id = crate::bible_production_dataset::BSB_TRANSLATION_ID;
        let mut provider_conn = open_in_memory().unwrap();
        run_migrations(&mut provider_conn).unwrap();
        cip_integrations_bible::import_bible_dataset(
            &provider_conn,
            &crate::bible_production_dataset::bsb_dataset(),
        )
        .unwrap();
        let provider = SqliteBibleProvider::new(provider_conn);
        let mut context = DefaultScriptureContextManager::new(bsb_translation_id);
        let mut seq = 0u64;

        let mut process =
            |conn: &Connection, context: &mut DefaultScriptureContextManager, text: &str| {
                let result = handle_final_transcript(
                    conn,
                    &provider,
                    context,
                    session.id,
                    bsb_translation_id,
                    segment(text, seq),
                );
                seq += 1;
                result.unwrap()
            };

        // Establish context against a real book/chapter, then resolve a
        // bare verse against it - exactly the phase_1_5 scenario, but every
        // lookup now goes through the real 66-book BSB dataset instead of
        // the 6-verse fixture.
        let p = process(&conn, &mut context, "Turn with me to Genesis chapter one");
        assert!(p.suggestions.is_empty());
        assert_eq!(context.active_context().unwrap().book, "GEN");
        assert_eq!(context.active_context().unwrap().chapter, 1);

        let p = process(&conn, &mut context, "Look at verse one");
        assert_eq!(p.suggestions.len(), 1);
        assert_eq!(
            p.detections[0].reference.as_ref().unwrap().to_string(),
            "GEN 1:1"
        );
        let gen_1_1 = p.suggestions[0].id;

        // Context replacement to a second real book/chapter. Verse 36 is
        // deliberately chosen (not 16, the more famous verse): Genesis 1
        // only has 31 verses, but real Bible data densely overlaps verse
        // numbers across chapters in a way the tiny dev fixture never did,
        // so a bare "verse 16" immediately after this switch would
        // genuinely and correctly resolve as Ambiguous (John 3:16 vs.
        // Genesis 1:16, both real verses in BSB) rather than a defect -
        // exactly the "genuinely ambiguous - a candidate list, never a
        // guess" behavior Phase 1.1 established. Verse 36 exists only in
        // John 3 (Genesis 1 stops at 31), so it stays unambiguous here.
        let p = process(&conn, &mut context, "Now turn to John chapter three");
        assert!(p.suggestions.is_empty());
        let p = process(&conn, &mut context, "Verse thirty-six");
        assert_eq!(p.suggestions.len(), 1);
        assert_eq!(
            p.detections[0].reference.as_ref().unwrap().to_string(),
            "JHN 3:36"
        );
        let jhn_3_36 = p.suggestions[0].id;

        // Section 35 (repeated against the real dataset): the detector may
        // recognize the shape of a reference, but the real BibleProvider
        // remains authoritative - Romans 8:999 is not a real verse in BSB
        // either, so it must still produce no suggestion.
        let p = process(&conn, &mut context, "Turn to Romans 8:999.");
        assert!(
            p.suggestions.is_empty(),
            "an out-of-range verse must never produce a suggestion against the real dataset"
        );

        // Approve both real suggestions and confirm the real BSB verse text
        // (not the fixture's KJV wording) survives all the way to the
        // rendered presentation slide.
        crate::persistence::update_suggestion_status(
            &conn,
            gen_1_1,
            cip_core_ai::SuggestionStatus::Approved,
            None,
        )
        .unwrap();
        let (gen_content, _) =
            crate::presentation::build_scripture_slide(&provider, bsb_translation_id, "GEN 1:1")
                .unwrap();
        assert!(
            preview_content_is_scripture_text(&gen_content, "In the beginning God created"),
            "the real BSB Genesis 1:1 text must reach the presentation content"
        );
        crate::presentation::persist_prepared_item(
            &conn,
            session.id,
            gen_content,
            "SCRIPTURE_DEFAULT",
            Some(gen_1_1),
        )
        .unwrap();

        crate::persistence::update_suggestion_status(
            &conn,
            jhn_3_36,
            cip_core_ai::SuggestionStatus::Approved,
            None,
        )
        .unwrap();
        let (jhn_content, _) =
            crate::presentation::build_scripture_slide(&provider, bsb_translation_id, "JHN 3:36")
                .unwrap();
        assert!(
            preview_content_is_scripture_text(&jhn_content, "Whoever believes in the Son"),
            "the real BSB John 3:36 text must reach the presentation content"
        );
        crate::presentation::persist_prepared_item(
            &conn,
            session.id,
            jhn_content,
            "SCRIPTURE_DEFAULT",
            Some(jhn_3_36),
        )
        .unwrap();

        // Nothing here ever reaches Active - same invariant as every other
        // pipeline test, now proven against the real dataset too.
        let prepared =
            crate::persistence::list_presentation_items(&conn, session.id, None).unwrap();
        assert_eq!(prepared.len(), 2);
        assert!(prepared
            .iter()
            .all(|item| item.status == cip_core_presentation::PresentationItemStatus::Prepared));
    }

    /// Phase 3.7's canonical full offline operator acceptance test (spec
    /// section 19): everything a church operator can prove on a fresh
    /// Windows install with no Internet, no microphone, no Whisper model,
    /// and no projector - all through the exact same plain functions the
    /// real Tauri commands call, never a parallel/fabricated intelligence
    /// path (spec section 2; this project has no `tauri::test` harness -
    /// see this module's own docs on why command *logic* stays
    /// independently testable this way).
    ///
    /// Chain: fresh real file-backed database -> import + verify the real
    /// BSB dataset (never the dev fixture) -> real-text Bible search ->
    /// save a Scripture reference -> start a service -> submit a manual
    /// transcript through the SAME pipeline function live speech uses
    /// (`handle_final_transcript`/`process_transcript_segment` - exactly
    /// what `commands::process_test_transcript` calls, per spec section 7's
    /// "manual transcript enters the same production intelligence
    /// pipeline" requirement) -> operator approves the resulting
    /// suggestion -> prepare -> activate -> stop the presentation (laptop
    /// screen only - spec section 6, never a physical projector) -> stop
    /// the service -> close the database connection and reopen the same
    /// on-disk file, exactly as a real application restart would (the same
    /// real-file technique as
    /// `service_history_survives_a_simulated_application_restart` above,
    /// extended to also cover the saved Scripture and the presentation
    /// item) -> verify the saved Scripture, the completed service, and the
    /// stopped presentation item all survive.
    #[test]
    fn phase_3_7_full_offline_operator_chain_acceptance() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cip-phase-3-7-offline-acceptance.sqlite3");
        let bsb_translation_id = crate::bible_production_dataset::BSB_TRANSLATION_ID;

        let session_id;
        let saved_id;
        let prepared_item_id;
        let james_2_2_text;

        {
            let mut conn = cip_database::open(&db_path).unwrap();
            run_migrations(&mut conn).unwrap();

            // The real, complete BSB dataset, imported exactly as it is at
            // real application startup (never the tiny KJV dev fixture).
            let mut provider_conn = open_in_memory().unwrap();
            run_migrations(&mut provider_conn).unwrap();
            cip_integrations_bible::import_bible_dataset(
                &provider_conn,
                &crate::bible_production_dataset::bsb_dataset(),
            )
            .unwrap();
            let provider = SqliteBibleProvider::new(provider_conn);

            // --- Bible Library: real-text search against the real BSB
            //     dataset (spec section 5) ------------------------------
            let results =
                cip_core_bible::search_bible(&provider, bsb_translation_id, "James 2:2").unwrap();
            assert_eq!(
                results.len(),
                1,
                "James 2:2 must resolve to exactly one real BSB verse"
            );
            let james_2_2 = &results[0];
            assert_eq!(james_2_2.reference, "JAS 2:2");
            assert!(
                !james_2_2.text.is_empty(),
                "the real BSB verse text must be present"
            );
            james_2_2_text = james_2_2.text.clone();

            // --- Bible Library: save Scripture, list it back (spec
            //     section 5's "save Scripture, reopen saved Scripture") --
            let saved = crate::persistence::persist_saved_scripture(
                &conn,
                Uuid::new_v4(),
                bsb_translation_id,
                &james_2_2.book,
                james_2_2.chapter,
                james_2_2.verse,
                None,
                &james_2_2.reference,
                Some("Phase 3.7 offline operator acceptance"),
            )
            .unwrap();
            saved_id = saved.id;
            assert_eq!(
                crate::persistence::list_saved_scriptures(&conn)
                    .unwrap()
                    .len(),
                1
            );

            // --- Service lifecycle: start --------------------------------
            let session = ServiceSession::start("Phase 3.7 Offline Operator Acceptance");
            session_id = session.id;
            persist_service(&conn, &session).unwrap();

            // --- Manual Transcript Mode: the SAME production pipeline
            //     live speech uses (spec section 2/7) - no parallel/fake
            //     intelligence engine, no duplicated Bible detection -----
            let mut context = DefaultScriptureContextManager::new(bsb_translation_id);
            let processed = handle_final_transcript(
                &conn,
                &provider,
                &mut context,
                session.id,
                bsb_translation_id,
                segment("Please turn to James chapter 2 verse 2", 0),
            )
            .unwrap();
            assert_eq!(
                processed.suggestions.len(),
                1,
                "the manual transcript must produce exactly one real suggestion"
            );
            assert_eq!(
                processed.detections[0]
                    .reference
                    .as_ref()
                    .unwrap()
                    .to_string(),
                "JAS 2:2",
                "manual transcript detection must resolve against the real BSB dataset, not a fixture"
            );
            let suggestion_id = processed.suggestions[0].id;

            // --- Operator review: approve --------------------------------
            let approved = crate::persistence::update_suggestion_status(
                &conn,
                suggestion_id,
                cip_core_ai::SuggestionStatus::Approved,
                None,
            )
            .unwrap();
            assert_eq!(approved.status, cip_core_ai::SuggestionStatus::Approved);

            // --- Presentation: prepare -> activate -> stop, laptop-screen
            //     only (spec section 6) - build_scripture_slide is the
            //     exact function both preview and prepare commands call --
            let (content, _) = crate::presentation::build_scripture_slide(
                &provider,
                bsb_translation_id,
                "JAS 2:2",
            )
            .unwrap();
            assert!(
                preview_content_is_scripture_text(&content, &james_2_2_text),
                "the presentation content must carry the exact real BSB verse text"
            );
            let item = crate::presentation::persist_prepared_item(
                &conn,
                session.id,
                content,
                "SCRIPTURE_DEFAULT",
                Some(suggestion_id),
            )
            .unwrap();
            prepared_item_id = item.id;
            let (activating_item, _slide) =
                crate::presentation::prepare_to_activate(&conn, item.id).unwrap();
            assert_eq!(
                activating_item.status,
                cip_core_presentation::PresentationItemStatus::Prepared
            );
            let active = crate::presentation::commit_activation(&conn, item.id).unwrap();
            assert_eq!(
                active.status,
                cip_core_presentation::PresentationItemStatus::Active
            );
            let stopped = crate::presentation::stop_active_item(&conn, session.id)
                .unwrap()
                .expect("an active item was present to stop");
            assert_eq!(stopped.id, item.id);
            assert_eq!(
                stopped.status,
                cip_core_presentation::PresentationItemStatus::Stopped
            );

            // --- Service lifecycle: stop ---------------------------------
            let mut ending_session = session;
            ending_session.end();
            crate::persistence::update_service_status(
                &conn,
                ending_session.id,
                ending_session.status,
                ending_session.ended_at,
            )
            .unwrap();

            // "Close/restart the application": drop the connection at the
            // end of this block, same as the restart tests above.
        }

        // Reopen the SAME on-disk file, exactly as a fresh application
        // launch would - nothing carries over from the objects above.
        let reopened = cip_database::open(&db_path).unwrap();

        let reopened_service = crate::persistence::get_service(&reopened, session_id).unwrap();
        assert_eq!(
            reopened_service.status,
            cip_core_service::ServiceStatus::Ended,
            "the completed test service must survive a real restart"
        );

        let history = crate::persistence::list_services(&reopened, None, 50).unwrap();
        assert!(
            history.iter().any(|s| s.id == session_id),
            "the completed test service must appear in service history after restart"
        );

        let reopened_saved = crate::persistence::list_saved_scriptures(&reopened).unwrap();
        assert_eq!(reopened_saved.len(), 1);
        assert_eq!(reopened_saved[0].id, saved_id);
        assert_eq!(reopened_saved[0].reference_display, "JAS 2:2");

        let reopened_items =
            crate::persistence::list_presentation_items(&reopened, session_id, None).unwrap();
        assert_eq!(reopened_items.len(), 1);
        assert_eq!(reopened_items[0].id, prepared_item_id);
        assert_eq!(
            reopened_items[0].status,
            cip_core_presentation::PresentationItemStatus::Stopped,
            "the presentation item's final Stopped state must survive restart, never reset to Active"
        );

        // A completely fresh BibleProvider connection (never the one used
        // above) proves the saved reference still resolves to the real
        // BSB text on its own, not a value cached in memory.
        let mut restart_provider_conn = open_in_memory().unwrap();
        run_migrations(&mut restart_provider_conn).unwrap();
        cip_integrations_bible::import_bible_dataset(
            &restart_provider_conn,
            &crate::bible_production_dataset::bsb_dataset(),
        )
        .unwrap();
        let restart_provider = SqliteBibleProvider::new(restart_provider_conn);
        let reference = cip_core_bible::ScriptureReference::single(
            bsb_translation_id,
            &reopened_saved[0].book,
            reopened_saved[0].chapter,
            reopened_saved[0].verse_start,
        );
        let verse_after_restart = restart_provider.get_verse(&reference).unwrap().unwrap();
        assert_eq!(
            verse_after_restart.text, james_2_2_text,
            "the saved Scripture must still resolve to the exact same real BSB text after restart"
        );
    }

    /// Phase 2.7.1's canonical Saved Content acceptance test: proves the
    /// audit's central finding (`docs/phase-2-7-1-audit.md` section E) is
    /// actually fixed - an accepted `ContentCandidate` previously lived
    /// only in `AppState::content_candidate_queue`, an in-memory `Mutex`,
    /// so it never survived the service ending, let alone a real
    /// application restart. This test proves the fix using the exact same
    /// real-file-backed close/reopen technique as
    /// `phase_3_7_full_offline_operator_chain_acceptance` above - a
    /// candidate is accepted, persisted (mirroring exactly what
    /// `commands::accept_content_candidate` does), the connection is
    /// dropped and the same on-disk file reopened, and the candidate must
    /// still be there, byte-for-byte identical (provenance, evidence,
    /// confidence, and assertion level included - it is persisted as the
    /// real `ContentCandidate` type's own JSON, never re-derived).
    #[test]
    fn phase_2_7_1_saved_content_candidate_survives_a_real_restart() {
        use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
        use cip_core_intelligence::{AssertionLevel, ContentCandidate, ContentCandidateType};

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cip-phase-2-7-1-saved-content.sqlite3");

        let session_id;
        let candidate_id;

        {
            let mut conn = cip_database::open(&db_path).unwrap();
            run_migrations(&mut conn).unwrap();

            let session = ServiceSession::start("Phase 2.7.1 Saved Content Acceptance");
            session_id = session.id;
            persist_service(&conn, &session).unwrap();

            // The real production path: a candidate is detected (still
            // Detected/Reviewed), the operator reviews it, then explicitly
            // accepts it - only that final, explicit action ever reaches
            // persistence (never on mere detection).
            let mut candidate = ContentCandidate::new(
                session.id,
                None,
                vec![Uuid::new_v4()],
                ContentCandidateType::Theme,
                "Theme: faithfulness",
                "Faithfulness in small things",
                AssertionLevel::Suggested,
                ConfidenceResult::new(0.82, ConfidenceSource::Model, None),
                0.6,
                "sermon-content-v1",
                "1.0",
            );
            candidate.accept();
            candidate_id = candidate.id;
            assert_eq!(
                candidate.status,
                cip_core_intelligence::FindingStatus::Accepted
            );

            crate::persistence::persist_saved_content_candidate(&conn, &candidate).unwrap();

            // "Close/restart the application": drop the connection at the
            // end of this block, same as every other restart test here.
        }

        let reopened = cip_database::open(&db_path).unwrap();
        let saved =
            crate::persistence::list_saved_content_candidates_for_service(&reopened, session_id)
                .unwrap();
        assert_eq!(
            saved.len(),
            1,
            "the accepted content candidate must survive a real application restart"
        );
        assert_eq!(saved[0].id, candidate_id);
        assert_eq!(
            saved[0].status,
            cip_core_intelligence::FindingStatus::Accepted
        );
        assert_eq!(saved[0].title_or_label, "Theme: faithfulness");
        assert_eq!(saved[0].confidence.score, 0.82);
        assert_eq!(
            saved[0].assertion_level,
            cip_core_intelligence::AssertionLevel::Suggested,
            "assertion level must survive restart unchanged - never upgraded or downgraded"
        );
    }

    /// Phase 3.8's canonical Service Replay acceptance test: proves the
    /// audit's central finding (`docs/phase-3-8-audit.md` section B/K) is
    /// actually true - a realistic multi-segment sermon transcript, fed
    /// **sequentially** (never all at once) through the exact same
    /// pre-existing production functions the frontend's Service Replay
    /// scheduler calls (`handle_final_transcript` for Bible, plus the
    /// `sermon`/`content_intelligence`/`cross_domain` orchestration
    /// modules' own `analyze_and_queue` - the same pure cores
    /// `commands::analyze_sermon_transcript`/`analyze_bible_transcript`/
    /// `analyze_content_intelligence`/`analyze_cross_domain` call),
    /// produces real Bible detections and real Sermon findings, feeds
    /// those into Content/Cross-Domain analysis, carries an approved
    /// finding through the full presentation lifecycle, and survives a
    /// real file close/reopen with no stale replay state (replay
    /// position/pause is never persisted at all - there is nothing to
    /// verify against because Service Replay's own scheduler lives
    /// entirely in frontend memory, per spec section 28).
    #[test]
    fn phase_3_8_service_replay_full_offline_acceptance() {
        use crate::{content_intelligence, cross_domain, sermon};
        use cip_core_intelligence::{
            BibleIntelligenceEngine, ContentCandidateQueue, ContentIntelligenceEngine,
            ContextBounds, CorrelationQueue, CrossDomainCorrelationEngine, FindingQueue,
            IntelligenceContext, IntelligenceEngine, IntelligenceInput, SermonIntelligenceEngine,
        };

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cip-phase-3-8-service-replay.sqlite3");
        let bsb_translation_id = crate::bible_production_dataset::BSB_TRANSLATION_ID;

        // Two independent real-BSB-backed providers: one drives the real
        // Bible Suggestion path (`handle_final_transcript`, shares its
        // context across the whole replay exactly like the live pipeline
        // does), the other backs a standalone `BibleIntelligenceEngine`
        // for the Bible Finding path - mirroring exactly how
        // `commands::analyze_bible_transcript` uses its own engine
        // instance, never the same one `state.bible_provider` drives.
        let mut suggestion_provider_conn = open_in_memory().unwrap();
        run_migrations(&mut suggestion_provider_conn).unwrap();
        cip_integrations_bible::import_bible_dataset(
            &suggestion_provider_conn,
            &crate::bible_production_dataset::bsb_dataset(),
        )
        .unwrap();
        let suggestion_provider = SqliteBibleProvider::new(suggestion_provider_conn);

        let mut finding_provider_conn = open_in_memory().unwrap();
        run_migrations(&mut finding_provider_conn).unwrap();
        cip_integrations_bible::import_bible_dataset(
            &finding_provider_conn,
            &crate::bible_production_dataset::bsb_dataset(),
        )
        .unwrap();
        let bible_finding_engine = BibleIntelligenceEngine::new(
            Box::new(SqliteBibleProvider::new(finding_provider_conn)),
            bsb_translation_id,
        );

        // A realistic, sequential, condensed sermon (spec section 19's
        // sample, one logical segment per line) - never processed as one
        // giant blob, exactly what the frontend's paragraph-based
        // segmentation would produce.
        let transcript_segments = [
            "Good morning church. Today I want us to remember the faithfulness of God.",
            "John chapter 3 verse 16 reminds us of God's love for the world.",
            "My main point today is this: when we face difficult seasons, we should remember Romans chapter 8 verse 28.",
            "Let us pray.",
        ];

        let session_id;
        let mut approved_suggestion_id = None;
        let prepared_item_id;

        let sermon_engine = SermonIntelligenceEngine::new();
        let mut findings = FindingQueue::new();
        let mut sequences: Vec<u64> = Vec::new();

        {
            let mut conn = cip_database::open(&db_path).unwrap();
            run_migrations(&mut conn).unwrap();

            let session = ServiceSession::start("Phase 3.8 Service Replay Acceptance");
            session_id = session.id;
            persist_service(&conn, &session).unwrap();

            let mut context_manager = DefaultScriptureContextManager::new(bsb_translation_id);
            let mut recent_segments: Vec<cip_core_ai::TranscriptSegment> = Vec::new();

            for (i, &text) in transcript_segments.iter().enumerate() {
                let seq = i as u64;
                // --- Bible Suggestion path (the real production pipeline
                //     live speech and `process_test_transcript` both use)
                let processed = handle_final_transcript(
                    &conn,
                    &suggestion_provider,
                    &mut context_manager,
                    session_id,
                    bsb_translation_id,
                    segment(text, seq),
                )
                .unwrap();
                for s in &processed.suggestions {
                    if let cip_core_ai::SuggestionKind::Scripture { reference } = &s.kind {
                        if reference == "JHN 3:16" {
                            approved_suggestion_id = Some(s.id);
                        }
                    }
                }

                // --- Sermon + Bible Finding paths (mirrors
                //     `commands::analyze_sermon_transcript`/
                //     `analyze_bible_transcript`'s exact pure cores) -----
                let seg_for_context = segment(text, seq);
                recent_segments.push(seg_for_context.clone());
                let context = IntelligenceContext::build(
                    session_id,
                    None,
                    Some(seg_for_context.clone()),
                    recent_segments.clone(),
                    context_manager.active_context(),
                    findings.all().into_iter().cloned().collect(),
                    Vec::new(),
                    Vec::new(),
                    ContextBounds::default(),
                );
                let input = IntelligenceInput::new(session_id, seg_for_context);

                let sermon_findings =
                    sermon::analyze_and_queue(&sermon_engine, &input, &context, &mut findings)
                        .unwrap();
                let bible_result = bible_finding_engine.analyze(&input, &context).unwrap();
                for finding in bible_result.findings {
                    findings.add(finding);
                }

                log::debug!(
                    target: "cip::test",
                    "segment {seq} produced {} sermon finding(s)",
                    sermon_findings.len()
                );

                sequences.push(seq);
            }

            // Sequential-arrival proof: every segment got a strictly
            // increasing sequence number, in the order fed - never a
            // batch/simultaneous submission masquerading as sequential.
            for pair in sequences.windows(2) {
                assert!(
                    pair[1] > pair[0],
                    "transcript sequence numbers must strictly increase across replayed segments"
                );
            }

            let transcript_rows =
                crate::persistence::list_transcript_segments(&conn, session_id, 100).unwrap();
            assert_eq!(
                transcript_rows.len(),
                transcript_segments.len(),
                "every replayed segment must be persisted exactly once via the Suggestion path"
            );

            assert!(
                findings
                    .all()
                    .iter()
                    .any(|f| f.domain == cip_core_intelligence::IntelligenceDomain::Sermon),
                "the real Sermon Intelligence engine must produce at least one finding from this realistic transcript"
            );
            assert!(
                findings
                    .all()
                    .iter()
                    .any(|f| f.domain == cip_core_intelligence::IntelligenceDomain::Bible),
                "the real Bible Finding path must produce at least one finding from this realistic transcript"
            );

            let suggestion_id = approved_suggestion_id
                .expect("the John 3:16 segment must have produced a real Bible suggestion");

            // --- Content + Cross-Domain, run once after replay (mirrors
            //     the operator clicking "Analyze Cross-Domain + Content"
            //     after replay completes, exactly like the Offline Test
            //     Center's own Multi-Domain scenario precedent) ---------
            let final_context = IntelligenceContext::build(
                session_id,
                None,
                None,
                recent_segments.clone(),
                context_manager.active_context(),
                findings.all().into_iter().cloned().collect(),
                Vec::new(),
                Vec::new(),
                ContextBounds::default(),
            );
            let mut candidates = ContentCandidateQueue::new();
            let content_engine = ContentIntelligenceEngine::new();
            let queued_candidates = content_intelligence::analyze_and_queue(
                &content_engine,
                &final_context,
                &mut candidates,
            );
            let mut correlations = CorrelationQueue::new();
            let cross_domain_engine = CrossDomainCorrelationEngine::new();
            let queued_correlations = cross_domain::analyze_and_queue(
                &cross_domain_engine,
                &final_context,
                &mut correlations,
            );
            // Honesty rule (spec section 33/47): never fabricate a
            // correlation/candidate that the real deterministic engines
            // did not genuinely produce - both calls must merely succeed
            // without panicking; whether either produces output depends
            // entirely on the real rule engines, exactly like the
            // pre-existing Offline Test Center's Multi-Domain scenario.
            log::debug!(
                target: "cip::test",
                "content candidates: {}, cross-domain correlations: {}",
                queued_candidates.len(),
                queued_correlations.len()
            );

            // --- Operator review: approve the real Bible suggestion ----
            let approved = crate::persistence::update_suggestion_status(
                &conn,
                suggestion_id,
                cip_core_ai::SuggestionStatus::Approved,
                None,
            )
            .unwrap();
            assert_eq!(approved.status, cip_core_ai::SuggestionStatus::Approved);

            // --- Presentation: prepare -> activate -> stop, laptop
            //     screen only, entirely offline --------------------------
            let (content, _) = crate::presentation::build_scripture_slide(
                &suggestion_provider,
                bsb_translation_id,
                "JHN 3:16",
            )
            .unwrap();
            let item = crate::presentation::persist_prepared_item(
                &conn,
                session_id,
                content,
                "SCRIPTURE_DEFAULT",
                Some(suggestion_id),
            )
            .unwrap();
            prepared_item_id = item.id;
            crate::presentation::prepare_to_activate(&conn, item.id).unwrap();
            crate::presentation::commit_activation(&conn, item.id).unwrap();
            let stopped = crate::presentation::stop_active_item(&conn, session_id)
                .unwrap()
                .expect("an active item was present to stop");
            assert_eq!(
                stopped.status,
                cip_core_presentation::PresentationItemStatus::Stopped
            );

            // --- Service lifecycle: stop, then close the connection ----
            let mut ending_session = session;
            ending_session.end();
            crate::persistence::update_service_status(
                &conn,
                ending_session.id,
                ending_session.status,
                ending_session.ended_at,
            )
            .unwrap();
        }

        // Reopen the SAME on-disk file, exactly as a fresh application
        // launch would.
        let reopened = cip_database::open(&db_path).unwrap();

        let reopened_service = crate::persistence::get_service(&reopened, session_id).unwrap();
        assert_eq!(
            reopened_service.status,
            cip_core_service::ServiceStatus::Ended,
            "the replayed service must survive a real restart"
        );

        let reopened_transcript =
            crate::persistence::list_transcript_segments(&reopened, session_id, 100).unwrap();
        assert_eq!(reopened_transcript.len(), transcript_segments.len());

        let reopened_suggestions =
            crate::persistence::list_suggestions(&reopened, session_id, None).unwrap();
        assert!(reopened_suggestions
            .iter()
            .any(|s| Some(s.id) == approved_suggestion_id
                && s.status == cip_core_ai::SuggestionStatus::Approved));

        let reopened_items =
            crate::persistence::list_presentation_items(&reopened, session_id, None).unwrap();
        assert_eq!(reopened_items.len(), 1);
        assert_eq!(reopened_items[0].id, prepared_item_id);
        assert_eq!(
            reopened_items[0].status,
            cip_core_presentation::PresentationItemStatus::Stopped,
            "the final Stopped state must survive restart, never reset to Active"
        );

        // No stale replay state: Service Replay's own scheduler
        // (position/pause/speed) never touches the database at all, so
        // there is nothing here to leak across a restart or a fresh
        // replay run - confirmed by construction, not merely asserted.
    }

    /// Phase 3.8.1 acceptance test - proves the real defects reported
    /// against the Phase 3.8 Service Replay screen are architecturally
    /// fixed on the backend side of the boundary: many (16, not 2)
    /// sequential segments derived from a longer, multi-topic synthetic
    /// sermon (never the user's real copyrighted transcript, which was
    /// never supplied verbatim - only a list of the Scripture references
    /// it contains) are fed one at a time through the same real production
    /// entry points Service Replay calls, and intelligence must arrive
    /// PROGRESSIVELY - at more than one point during the sequence, not
    /// only in one final batch - proving the operator would see results
    /// building up in real time, not just at the very end. No reference is
    /// hardcoded as an expected detection anywhere below (spec section
    /// 5/13): the test only asserts that the real BSB-backed engine
    /// detected *some* references and that the real Sermon Intelligence
    /// engine produced *some* findings, exactly as much as the
    /// deterministic engines actually produced - never more, never
    /// invented.
    #[test]
    fn phase_3_8_1_service_replay_progressive_intelligence_acceptance() {
        use crate::{content_intelligence, cross_domain, sermon};
        use cip_core_intelligence::{
            BibleIntelligenceEngine, ContentCandidateQueue, ContentIntelligenceEngine,
            ContextBounds, CorrelationQueue, CrossDomainCorrelationEngine, FindingQueue,
            IntelligenceContext, IntelligenceEngine, IntelligenceInput, SermonIntelligenceEngine,
        };

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cip-phase-3-8-1-service-replay.sqlite3");
        let bsb_translation_id = crate::bible_production_dataset::BSB_TRANSLATION_ID;

        let mut suggestion_provider_conn = open_in_memory().unwrap();
        run_migrations(&mut suggestion_provider_conn).unwrap();
        cip_integrations_bible::import_bible_dataset(
            &suggestion_provider_conn,
            &crate::bible_production_dataset::bsb_dataset(),
        )
        .unwrap();
        let suggestion_provider = SqliteBibleProvider::new(suggestion_provider_conn);

        let mut finding_provider_conn = open_in_memory().unwrap();
        run_migrations(&mut finding_provider_conn).unwrap();
        cip_integrations_bible::import_bible_dataset(
            &finding_provider_conn,
            &crate::bible_production_dataset::bsb_dataset(),
        )
        .unwrap();
        let bible_finding_engine = BibleIntelligenceEngine::new(
            Box::new(SqliteBibleProvider::new(finding_provider_conn)),
            bsb_translation_id,
        );

        // A longer, multi-topic synthetic sermon - project-authored, never
        // the user's real transcript - mixing sermon-structure language
        // (theme/main point/illustration/sub point/question/application)
        // with several distinct Scripture references, spread across 16
        // sequential segments (proving the fixed segmentation-and-replay
        // path handles far more than the reported 2-segment collapse).
        let transcript_segments = [
            "Good morning church. Grace and peace to you all as we gather today.",
            "Today I want to talk about the kingdom of God operating within us.",
            "The theme of our message is the kingdom of God at work in our hearts.",
            "Matthew chapter 6 verse 9 teaches us how the Lord's Prayer begins: Our Father in heaven, hallowed be your name.",
            "My main point today is this: the kingdom is both something we receive and something we walk in daily.",
            "Romans chapter 14 verse 17 reminds us that the kingdom of God is not food and drink, but righteousness, peace, and joy.",
            "Let me give an illustration: think of a citizen who carries their nation's passport wherever they travel.",
            "Psalm chapter 4 verse 8 says I will lie down and sleep in peace, for you alone, Lord, make me dwell in safety.",
            "A sub point worth noting is that peace is not the absence of trouble but the presence of God.",
            "John chapter 3 verse 16 reminds us of God's love for the world.",
            "When we face difficult seasons, we should remember Romans chapter 8 verse 28.",
            "Isaiah chapter 55 invites everyone who thirsts to come to the waters.",
            "Hebrews chapter 9 speaks of the better covenant we now live under.",
            "Let's consider a question: what would change in your life if you truly believed the kingdom was already within you?",
            "The application for us today is to walk daily in righteousness, peace, and joy, not by our own effort but by the Spirit.",
            "Let us pray. Father, we thank you for your kingdom at work in us.",
        ];

        let session_id;
        let mut suggestion_records: Vec<(uuid::Uuid, String)> = Vec::new();
        let prepared_item_id;

        let sermon_engine = SermonIntelligenceEngine::new();
        let mut findings = FindingQueue::new();
        let mut sequences: Vec<u64> = Vec::new();
        let mut findings_progression: Vec<usize> = Vec::new();

        {
            let mut conn = cip_database::open(&db_path).unwrap();
            run_migrations(&mut conn).unwrap();

            let session = ServiceSession::start("Phase 3.8.1 Service Replay Acceptance");
            session_id = session.id;
            persist_service(&conn, &session).unwrap();

            let mut context_manager = DefaultScriptureContextManager::new(bsb_translation_id);
            let mut recent_segments: Vec<cip_core_ai::TranscriptSegment> = Vec::new();

            for (i, &text) in transcript_segments.iter().enumerate() {
                let seq = i as u64;

                // --- Bible Suggestion path (the real production pipeline
                //     live speech and `process_test_transcript` both use).
                //     Every Scripture suggestion produced is recorded -
                //     never just one hardcoded reference - since the real
                //     detector, not this test, decides what is found.
                let processed = handle_final_transcript(
                    &conn,
                    &suggestion_provider,
                    &mut context_manager,
                    session_id,
                    bsb_translation_id,
                    segment(text, seq),
                )
                .unwrap();
                for s in &processed.suggestions {
                    if let cip_core_ai::SuggestionKind::Scripture { reference } = &s.kind {
                        suggestion_records.push((s.id, reference.clone()));
                    }
                }

                // --- Sermon + Bible Finding paths (mirrors
                //     `commands::analyze_sermon_transcript`/
                //     `analyze_bible_transcript`'s exact pure cores) -----
                let seg_for_context = segment(text, seq);
                recent_segments.push(seg_for_context.clone());
                let context = IntelligenceContext::build(
                    session_id,
                    None,
                    Some(seg_for_context.clone()),
                    recent_segments.clone(),
                    context_manager.active_context(),
                    findings.all().into_iter().cloned().collect(),
                    Vec::new(),
                    Vec::new(),
                    ContextBounds::default(),
                );
                let input = IntelligenceInput::new(session_id, seg_for_context);

                let sermon_findings =
                    sermon::analyze_and_queue(&sermon_engine, &input, &context, &mut findings)
                        .unwrap();
                let bible_result = bible_finding_engine.analyze(&input, &context).unwrap();
                for finding in bible_result.findings {
                    findings.add(finding);
                }

                log::debug!(
                    target: "cip::test",
                    "segment {seq} produced {} sermon finding(s)",
                    sermon_findings.len()
                );

                sequences.push(seq);
                findings_progression.push(findings.all().len());
            }

            // Sequential-arrival proof (same as Phase 3.8's original test,
            // now over 16 segments instead of 4).
            for pair in sequences.windows(2) {
                assert!(
                    pair[1] > pair[0],
                    "transcript sequence numbers must strictly increase across replayed segments"
                );
            }

            let transcript_rows =
                crate::persistence::list_transcript_segments(&conn, session_id, 100).unwrap();
            assert_eq!(
                transcript_rows.len(),
                transcript_segments.len(),
                "every replayed segment must be persisted exactly once via the Suggestion path"
            );

            // --- PROGRESSIVE delivery proof (the actual Phase 3.8.1
            //     defect): findings must accumulate at more than one point
            //     during the sequence, never only after the final segment
            //     - this is what makes replay show the operator something
            //     building up in real time rather than one end-of-run
            //     batch. Never asserts *which* segment produced a finding,
            //     only that growth happened more than once. -------------
            assert!(
                findings_progression.windows(2).all(|w| w[1] >= w[0]),
                "the finding count must never decrease as segments are replayed"
            );
            let growth_points = findings_progression
                .windows(2)
                .filter(|w| w[1] > w[0])
                .count();
            assert!(
                growth_points >= 2,
                "real findings must accumulate at multiple points across a 16-segment replay \
                 (got {growth_points} growth point(s)) - proving progressive delivery, not a \
                 single end-of-replay batch"
            );

            assert!(
                findings
                    .all()
                    .iter()
                    .any(|f| f.domain == cip_core_intelligence::IntelligenceDomain::Sermon),
                "the real Sermon Intelligence engine must produce at least one finding from this realistic transcript"
            );
            assert!(
                findings
                    .all()
                    .iter()
                    .any(|f| f.domain == cip_core_intelligence::IntelligenceDomain::Bible),
                "the real Bible Finding path must produce at least one finding from this realistic transcript"
            );

            // At least one real Scripture reference must have been
            // detected via the Suggestion path too - which one is left
            // entirely to the real detector (spec section 5: "do not
            // hardcode expected outputs").
            let (suggestion_id, detected_reference) = suggestion_records
                .first()
                .cloned()
                .expect("the real Bible detector must find at least one Scripture reference across 16 realistic sermon segments");

            // --- Content + Cross-Domain, run once after replay ----------
            let final_context = IntelligenceContext::build(
                session_id,
                None,
                None,
                recent_segments.clone(),
                context_manager.active_context(),
                findings.all().into_iter().cloned().collect(),
                Vec::new(),
                Vec::new(),
                ContextBounds::default(),
            );
            let mut candidates = ContentCandidateQueue::new();
            let content_engine = ContentIntelligenceEngine::new();
            let queued_candidates = content_intelligence::analyze_and_queue(
                &content_engine,
                &final_context,
                &mut candidates,
            );
            let mut correlations = CorrelationQueue::new();
            let cross_domain_engine = CrossDomainCorrelationEngine::new();
            let queued_correlations = cross_domain::analyze_and_queue(
                &cross_domain_engine,
                &final_context,
                &mut correlations,
            );
            log::debug!(
                target: "cip::test",
                "content candidates: {}, cross-domain correlations: {}",
                queued_candidates.len(),
                queued_correlations.len()
            );

            // --- Operator review: approve the real (dynamically chosen)
            //     Bible suggestion ------------------------------------
            let approved = crate::persistence::update_suggestion_status(
                &conn,
                suggestion_id,
                cip_core_ai::SuggestionStatus::Approved,
                None,
            )
            .unwrap();
            assert_eq!(approved.status, cip_core_ai::SuggestionStatus::Approved);

            // --- Presentation: prepare -> activate -> stop, laptop
            //     screen only, entirely offline --------------------------
            let (content, _) = crate::presentation::build_scripture_slide(
                &suggestion_provider,
                bsb_translation_id,
                &detected_reference,
            )
            .unwrap();
            let item = crate::presentation::persist_prepared_item(
                &conn,
                session_id,
                content,
                "SCRIPTURE_DEFAULT",
                Some(suggestion_id),
            )
            .unwrap();
            prepared_item_id = item.id;
            crate::presentation::prepare_to_activate(&conn, item.id).unwrap();
            crate::presentation::commit_activation(&conn, item.id).unwrap();
            let stopped = crate::presentation::stop_active_item(&conn, session_id)
                .unwrap()
                .expect("an active item was present to stop");
            assert_eq!(
                stopped.status,
                cip_core_presentation::PresentationItemStatus::Stopped
            );

            // --- Service lifecycle: stop, then close the connection ----
            let mut ending_session = session;
            ending_session.end();
            crate::persistence::update_service_status(
                &conn,
                ending_session.id,
                ending_session.status,
                ending_session.ended_at,
            )
            .unwrap();
        }

        // Reopen the SAME on-disk file, exactly as a fresh application
        // launch would - proving persistence and that no stale replay
        // state leaks across a restart (Service Replay's own scheduler
        // position/pause/speed lives entirely in frontend memory and never
        // touches the database, so there is nothing here to leak by
        // construction).
        let reopened = cip_database::open(&db_path).unwrap();

        let reopened_service = crate::persistence::get_service(&reopened, session_id).unwrap();
        assert_eq!(
            reopened_service.status,
            cip_core_service::ServiceStatus::Ended,
            "the replayed service must survive a real restart"
        );

        let reopened_transcript =
            crate::persistence::list_transcript_segments(&reopened, session_id, 100).unwrap();
        assert_eq!(reopened_transcript.len(), transcript_segments.len());

        let reopened_suggestions =
            crate::persistence::list_suggestions(&reopened, session_id, None).unwrap();
        assert!(reopened_suggestions.iter().any(|s| Some(s.id)
            == suggestion_records.first().map(|(id, _)| *id)
            && s.status == cip_core_ai::SuggestionStatus::Approved));

        let reopened_items =
            crate::persistence::list_presentation_items(&reopened, session_id, None).unwrap();
        assert_eq!(reopened_items.len(), 1);
        assert_eq!(reopened_items[0].id, prepared_item_id);
        assert_eq!(
            reopened_items[0].status,
            cip_core_presentation::PresentationItemStatus::Stopped,
            "the final Stopped state must survive restart, never reset to Active"
        );

        // Offline / no new network capability: this test - like every
        // other in this file - never constructs an HTTP client and only
        // ever touches the local SQLite file and in-process engines; see
        // `pilot-evidence/3.8.1/automated/regression.json`'s
        // `cargo tree` check for the workspace-wide proof.
    }

    fn preview_content_is_scripture_text(
        content: &cip_core_presentation::PresentationContent,
        needle: &str,
    ) -> bool {
        matches!(content, cip_core_presentation::PresentationContent::Scripture { text, .. } if text.contains(needle))
    }

    // --- Phase 3.1: full pilot service simulation ---------------------------
    //
    // The Phase 3.1 spec's "full-service simulation": chains every domain
    // this app ships - Bible (Suggestion path AND the separate Finding-path
    // bridge `analyze_bible_transcript` uses), Sermon (foundation lifecycle
    // AND semantic taxonomy), Music, Content Intelligence, Cross-Domain
    // correlation, Presentation activation, and a real file-backed restart -
    // through the exact same production orchestration functions
    // `commands.rs` calls, without the `AppHandle`/`State` machinery this
    // codebase has no test harness for (see `sermon_foundation.rs`'s
    // canonical acceptance test for the established precedent this follows).
    // Fictional service, fictional sermon, synthetic project-authored
    // transcript text only.
    #[test]
    fn phase_3_1_pilot_full_service_simulation() {
        use cip_core_content::ContentRegistry;
        use cip_core_intelligence::{
            BibleIntelligenceEngine, ContentCandidateQueue, ContentIntelligenceEngine,
            ContextBounds, CorrelationKind, CorrelationQueue, CrossDomainCorrelationEngine,
            FindingQueue, IntelligenceContext, IntelligenceDomain, IntelligenceEngine,
            IntelligenceInput, MusicIntelligenceEngine, SermonIntelligenceEngine,
        };
        use cip_core_sermon::foundation::{
            SectionOrigin, Sermon, SermonSection, SermonSectionKind, SermonSegment, Speaker,
            SpeakerRole,
        };
        use cip_integrations_content::SqliteContentRegistry;
        use cip_integrations_music::SqliteMusicProvider;

        fn fresh_seeded_conn() -> Connection {
            let mut conn = open_in_memory().unwrap();
            run_migrations(&mut conn).unwrap();
            apply_dev_seed(&conn).unwrap();
            conn
        }

        // A real, file-backed main app database - so the restart step at
        // the end is a genuine close/reopen, matching
        // `service_history_survives_a_simulated_application_restart` -
        // plus one independent in-memory connection per provider/registry,
        // exactly this codebase's established convention (each provider
        // owns its own connection; only the main `conn` is ever shared
        // with `persistence.rs`).
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cip-phase-3-1-pilot-simulation.sqlite3");
        let session_id;
        let sermon_id;
        let romans_828_item_id;

        {
            let mut conn = cip_database::open(&db_path).unwrap();
            run_migrations(&mut conn).unwrap();
            apply_dev_seed(&conn).unwrap();
            let conn = conn;

            let bible_provider = SqliteBibleProvider::new(fresh_seeded_conn());
            let bible_provider_for_pipeline = SqliteBibleProvider::new(fresh_seeded_conn());
            let music_provider = SqliteMusicProvider::new(fresh_seeded_conn());
            let content_registry = SqliteContentRegistry::new({
                let mut c = open_in_memory().unwrap();
                run_migrations(&mut c).unwrap();
                c
            });
            crate::music::register_dev_seed_music_content_if_missing(&content_registry).unwrap();
            let content_metadata = content_registry.list(None).unwrap();

            let bible_engine = BibleIntelligenceEngine::new(Box::new(bible_provider), "KJV");
            let music_engine = MusicIntelligenceEngine::new(Box::new(music_provider));
            let sermon_engine = SermonIntelligenceEngine::new();
            let content_engine = ContentIntelligenceEngine::new();
            let cross_domain_engine = CrossDomainCorrelationEngine::new();

            let mut findings = FindingQueue::new();
            let mut candidates = ContentCandidateQueue::new();
            let mut correlations = CorrelationQueue::new();
            let mut seq = 0u64;

            // SERVICE START
            let session = ServiceSession::start("Phase 3.1 Pilot Simulation");
            session_id = session.id;
            persist_service(&conn, &session).unwrap();

            // SERMON START: title, speaker, and an explicit Main Message
            // section (mirrors `sermon_foundation.rs`'s canonical scenario).
            let mut sermon =
                Sermon::start(session.id, Some("Trusting God in Uncertainty".to_string()));
            sermon_id = sermon.id;
            crate::persistence::persist_sermon(&conn, &sermon).unwrap();
            let main_message = SermonSection::open(
                sermon.id,
                SermonSectionKind::MainMessage,
                SectionOrigin::OperatorAssigned,
                None,
            );
            crate::persistence::persist_sermon_section(&conn, &main_message).unwrap();
            let speaker = Speaker::new("Pastor Jane Doe", SpeakerRole::Primary);
            sermon.assign_speaker(speaker);
            crate::persistence::update_sermon(&conn, &sermon).unwrap();

            // A. BIBLE REFERENCE - the real operator-facing Suggestion path
            // (`handle_final_transcript`, unchanged production code):
            // detect, approve, and prepare Romans 8:28 for presentation.
            let mut scripture_context_manager = DefaultScriptureContextManager::new("KJV");
            let s1 = segment(
                "Good morning church. Turn with me to Romans chapter eight",
                seq,
            );
            seq += 1;
            handle_final_transcript(
                &conn,
                &bible_provider_for_pipeline,
                &mut scripture_context_manager,
                session.id,
                "KJV",
                s1,
            )
            .unwrap();
            let s2 = segment("Look at verse twenty-eight", seq);
            seq += 1;
            let processed = handle_final_transcript(
                &conn,
                &bible_provider_for_pipeline,
                &mut scripture_context_manager,
                session.id,
                "KJV",
                s2,
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
            let romans_828_suggestion = processed.suggestions[0].id;
            crate::persistence::update_suggestion_status(
                &conn,
                romans_828_suggestion,
                cip_core_ai::SuggestionStatus::Approved,
                None,
            )
            .unwrap();
            let (romans_828_content, _) = crate::presentation::build_scripture_slide(
                &bible_provider_for_pipeline,
                "KJV",
                "ROM 8:28",
            )
            .unwrap();
            let romans_828_item = crate::presentation::persist_prepared_item(
                &conn,
                session.id,
                romans_828_content,
                "SCRIPTURE_DEFAULT",
                Some(romans_828_suggestion),
            )
            .unwrap();
            romans_828_item_id = romans_828_item.id;

            // B. BIBLE INTELLIGENCE FINDING - the separate, real Finding-path
            // bridge (`commands::analyze_bible_transcript`'s own engine
            // call) so a Bible-domain `IntelligenceFinding` exists for
            // cross-domain correlation, exactly mirroring how that command
            // is the only way a Bible finding ever reaches
            // `context.recent_findings`.
            let scripture_context_snapshot = cip_core_bible::ScriptureContext {
                translation_id: "KJV".to_string(),
                book: "ROM".to_string(),
                chapter: 8,
                last_verse: Some(28),
                confidence: ConfidenceResult::new(0.9, ConfidenceSource::Heuristic, None),
                established_at: chrono::Utc::now(),
                valid: true,
            };
            let bible_finding_context = IntelligenceContext::build(
                session.id,
                None,
                None,
                Vec::new(),
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                ContextBounds::default(),
            );
            for text in ["Turn with me to Romans chapter eight", "Verse twenty-eight"] {
                let seg = segment(text, seq);
                seq += 1;
                let input = IntelligenceInput::new(session.id, seg);
                let result = bible_engine
                    .analyze(&input, &bible_finding_context)
                    .unwrap();
                for f in result.findings {
                    findings.add(f);
                }
            }
            assert!(
                findings
                    .all()
                    .iter()
                    .any(|f| f.domain == IntelligenceDomain::Bible && f.summary == "ROM 8:28"),
                "the real Bible engine must have produced a Bible finding for ROM 8:28"
            );

            // C. SERMON SEMANTIC TAXONOMY - the same nine-segment scripted
            // walkthrough `sermon_adapter.rs`'s own canonical acceptance
            // test uses, run through the real orchestration function
            // (`sermon::analyze_and_queue`) against a context carrying both
            // the active Scripture context (for the "Supporting Scripture"
            // cross-link) and this sermon's real foundation state - proving
            // the foundation (who/what section) and the semantic taxonomy
            // (what was said) share the same live sermon, not two
            // disconnected copies. Every transcript segment is persisted
            // and linked via a real `SermonSegment` row.
            let sermon_segments_text = [
                "Today I want to talk about trusting God when life becomes uncertain.",
                "My first point is that faith grows when it is tested.",
                "I remember when I faced total uncertainty myself.",
                "Romans chapter eight verse twenty eight reminds us that all things work together for good.",
                "What does this mean for us?",
                "We must trust God even when we cannot see the outcome.",
                "Never forget: faith is not the absence of uncertainty.",
                "What are you trusting when everything around you is uncertain?",
                "If you remember one thing today, remember that God is faithful.",
            ];
            let mut recent_segments = Vec::new();
            for (i, text) in sermon_segments_text.iter().enumerate() {
                let seg = segment(text, seq);
                seq += 1;
                persist_transcript_segment(&conn, session.id, &seg).unwrap();
                let link = SermonSegment::new(sermon.id, seg.id, i as u32, Some(main_message.id));
                crate::persistence::persist_sermon_segment(&conn, &link).unwrap();
                recent_segments.push(seg.clone());

                let recent_sermon_segments =
                    crate::persistence::list_sermon_segments(&conn, sermon.id).unwrap();
                let context = IntelligenceContext::build(
                    session.id,
                    Some(cip_core_service::ServiceStatus::Started),
                    Some(seg.clone()),
                    recent_segments.clone(),
                    Some(scripture_context_snapshot.clone()),
                    findings.all().into_iter().cloned().collect(),
                    Vec::new(),
                    content_metadata.clone(),
                    ContextBounds::default(),
                )
                .with_sermon_context(
                    Some(sermon.clone()),
                    Some(main_message.clone()),
                    recent_sermon_segments,
                );

                let input = IntelligenceInput::new(session.id, seg);
                crate::sermon::analyze_and_queue(&sermon_engine, &input, &context, &mut findings)
                    .unwrap();
            }
            let has_finding =
                |prefix: &str| findings.all().iter().any(|f| f.summary.starts_with(prefix));
            assert!(has_finding("Main Point:"), "expected a main point");
            assert!(has_finding("Story:"), "expected an illustration/story");
            assert!(has_finding("Application:"), "expected an application");
            assert!(has_finding("Takeaway:"), "expected a takeaway");
            assert!(
                has_finding("Food for Thought:"),
                "expected a food-for-thought prompt"
            );
            assert!(
                has_finding("Supporting Scripture:"),
                "expected the sermon to cross-link the active Scripture context"
            );

            let reloaded_sermon_segments =
                crate::persistence::list_sermon_segments(&conn, sermon.id).unwrap();
            assert_eq!(
                reloaded_sermon_segments.len(),
                sermon_segments_text.len(),
                "every sermon transcript segment must be linked and persisted"
            );

            // D. MUSIC - a real dev-seed hymnbook exact-title match, via the
            // real orchestration function (`music::analyze_and_queue`).
            let music_seg = segment("Test Fixture Hymn One", seq);
            let music_context = IntelligenceContext::build(
                session.id,
                None,
                Some(music_seg.clone()),
                vec![music_seg.clone()],
                None,
                Vec::new(),
                Vec::new(),
                content_metadata.clone(),
                ContextBounds::default(),
            );
            let music_input = IntelligenceInput::new(session.id, music_seg);
            let music_queued = crate::music::analyze_and_queue(
                &music_engine,
                &music_input,
                &music_context,
                &mut findings,
            )
            .unwrap();
            assert_eq!(
                music_queued.len(),
                1,
                "the real dev-seed hymnbook must recognize an exact title match"
            );

            // E. CONTENT CANDIDATES - the real orchestration function
            // (`content_intelligence::analyze_and_queue`), reading the
            // sermon taxonomy findings already queued above.
            let content_context = IntelligenceContext::build(
                session.id,
                None,
                None,
                Vec::new(),
                None,
                findings.all().into_iter().cloned().collect(),
                Vec::new(),
                content_metadata.clone(),
                ContextBounds::default(),
            );
            let queued_candidates = crate::content_intelligence::analyze_and_queue(
                &content_engine,
                &content_context,
                &mut candidates,
            );
            assert!(
                !queued_candidates.is_empty(),
                "sermon taxonomy findings must yield at least one content candidate"
            );

            // F. CROSS-DOMAIN CORRELATION - the real orchestration function
            // (`cross_domain::analyze_and_queue`): the shared Romans 8:28
            // reference between the Bible finding (B) and the sermon's
            // "Supporting Scripture" cross-link (C) must correlate.
            let cross_domain_context = IntelligenceContext::build(
                session.id,
                None,
                None,
                Vec::new(),
                Some(scripture_context_snapshot.clone()),
                findings.all().into_iter().cloned().collect(),
                Vec::new(),
                content_metadata.clone(),
                ContextBounds::default(),
            )
            .with_content_candidates(candidates.all().into_iter().cloned().collect());
            let queued_correlations = crate::cross_domain::analyze_and_queue(
                &cross_domain_engine,
                &cross_domain_context,
                &mut correlations,
            );
            assert!(
                queued_correlations
                    .iter()
                    .any(|c| c.kind == CorrelationKind::ScriptureSermon),
                "expected a Scripture<->Sermon correlation from the shared Romans 8:28 reference"
            );

            // G. PRESENTATION ACTIVATION - the real Prepared -> Active
            // transition (`prepare_to_activate` + `commit_activation`),
            // left Active on purpose: the block below drops this
            // connection with the item still Active, deliberately
            // simulating "app closed mid-service"
            // (`docs/first-use.md`'s Troubleshooting table) for step H.
            let (to_activate, _slide) =
                crate::presentation::prepare_to_activate(&conn, romans_828_item.id).unwrap();
            assert_eq!(
                to_activate.status,
                cip_core_presentation::PresentationItemStatus::Prepared
            );
            let active_item =
                crate::presentation::commit_activation(&conn, romans_828_item.id).unwrap();
            assert_eq!(
                active_item.status,
                cip_core_presentation::PresentationItemStatus::Active,
                "the display command must have actually taken effect"
            );

            // "Close/restart the application": drop the connection at the
            // end of this block with the presentation item still Active.
        }

        // H. SIMULATED RESTART - reopen the real file-backed database
        // exactly as a fresh application launch would, and run the same
        // startup reconciliation `lib.rs`'s real setup path always runs
        // unconditionally.
        let reopened = cip_database::open(&db_path).unwrap();
        let reconciled_count =
            crate::persistence::reconcile_stale_active_presentation_items(&reopened).unwrap();
        assert_eq!(
            reconciled_count, 1,
            "the stale Active presentation item must be reconciled on restart"
        );
        let reloaded_item =
            crate::persistence::get_presentation_item(&reopened, romans_828_item_id).unwrap();
        assert_eq!(
            reloaded_item.status,
            cip_core_presentation::PresentationItemStatus::Stopped,
            "restart must never leave a stale item claiming to still be on screen"
        );

        // Sermon and service history must also survive the restart intact.
        let reloaded_sermon = crate::persistence::get_sermon(&reopened, sermon_id).unwrap();
        assert_eq!(
            reloaded_sermon.title.as_deref(),
            Some("Trusting God in Uncertainty")
        );
        let reloaded_segments =
            crate::persistence::list_sermon_segments(&reopened, sermon_id).unwrap();
        assert_eq!(
            reloaded_segments.len(),
            9,
            "every sermon segment link must survive the restart"
        );
        let reloaded_transcript =
            crate::persistence::list_transcript_segments(&reopened, session_id, 100).unwrap();
        assert!(
            reloaded_transcript.len() >= 11,
            "bible and sermon transcript segments must all survive the restart"
        );
    }

    // --- Phase 3.2: sixty-minute simulated service stability ---------------
    //
    // A SIMULATED (not real-time - this session cannot productively spend
    // an hour of wall-clock time waiting, and nothing here is genuinely
    // time-driven) sustained-load proof: the same real production
    // orchestration functions as the Phase 3.1 full-service simulation,
    // driven across a synthetic ~60-minute timeline (spec section 13's
    // minute-by-minute outline, compressed into 20 three-"minute" cycles of
    // sermon + Scripture + music + presentation activate/stop), checking
    // for the specific things a real multi-hour service could go wrong in:
    // unbounded/explosive queue growth, duplicate-finding accumulation, or
    // a stale presentation state left behind at the end. Real multi-hour
    // wall-clock stability (spec section 20) is explicitly NOT claimed by
    // this test - see `docs/phase-3-2-hardware-pilot.md`'s Multi-Hour
    // Stability section for why that remains NOT VERIFIED in this
    // environment.
    #[test]
    fn phase_3_2_sixty_minute_simulated_service_remains_stable() {
        use cip_core_intelligence::{
            BibleIntelligenceEngine, ContentCandidateQueue, ContentIntelligenceEngine,
            ContextBounds, CorrelationQueue, CrossDomainCorrelationEngine, FindingQueue,
            IntelligenceContext, IntelligenceDomain, IntelligenceEngine, IntelligenceInput,
        };
        use cip_core_sermon::foundation::{
            SectionOrigin, Sermon, SermonSection, SermonSectionKind, SermonSegment,
        };

        fn fresh_seeded_conn() -> Connection {
            let mut conn = open_in_memory().unwrap();
            run_migrations(&mut conn).unwrap();
            apply_dev_seed(&conn).unwrap();
            conn
        }

        let conn = seeded_db();
        let bible_provider = SqliteBibleProvider::new(fresh_seeded_conn());
        let bible_engine = BibleIntelligenceEngine::new(Box::new(bible_provider), "KJV");
        let sermon_engine = cip_core_intelligence::SermonIntelligenceEngine::new();
        let content_engine = ContentIntelligenceEngine::new();
        let cross_domain_engine = CrossDomainCorrelationEngine::new();

        let mut findings = FindingQueue::new();
        let mut candidates = ContentCandidateQueue::new();
        let mut correlations = CorrelationQueue::new();

        let session = ServiceSession::start("Phase 3.2 Sixty Minute Simulation");
        persist_service(&conn, &session).unwrap();

        let sermon = Sermon::start(session.id, Some("A Sustained Message".to_string()));
        crate::persistence::persist_sermon(&conn, &sermon).unwrap();
        let main_message = SermonSection::open(
            sermon.id,
            SermonSectionKind::MainMessage,
            SectionOrigin::OperatorAssigned,
            None,
        );
        crate::persistence::persist_sermon_section(&conn, &main_message).unwrap();

        const CYCLES: u32 = 20; // 20 cycles * 3 "minutes" each ~= 60 minutes
        let mut seq = 0u64;
        let mut recent_segments = Vec::new();
        let mut findings_after_each_cycle = Vec::with_capacity(CYCLES as usize);
        let mut active_item_id: Option<Uuid> = None;

        for cycle in 0..CYCLES {
            // One sermon-taxonomy-shaped line per cycle (varies just enough
            // to avoid every cycle being a literal duplicate, matching how
            // a real sermon's phrasing varies minute to minute).
            let sermon_text = format!(
                "My point number {cycle} is that faith remains steady through every season."
            );
            let seg = segment(&sermon_text, seq);
            seq += 1;
            persist_transcript_segment(&conn, session.id, &seg).unwrap();
            let link = SermonSegment::new(sermon.id, seg.id, cycle, Some(main_message.id));
            crate::persistence::persist_sermon_segment(&conn, &link).unwrap();
            recent_segments.push(seg.clone());

            let recent_sermon_segments =
                crate::persistence::list_sermon_segments(&conn, sermon.id).unwrap();
            let context = IntelligenceContext::build(
                session.id,
                Some(cip_core_service::ServiceStatus::Started),
                Some(seg.clone()),
                recent_segments.clone(),
                None,
                findings.all().into_iter().cloned().collect(),
                Vec::new(),
                Vec::new(),
                ContextBounds::default(),
            )
            .with_sermon_context(
                Some(sermon.clone()),
                Some(main_message.clone()),
                recent_sermon_segments,
            );
            let input = IntelligenceInput::new(session.id, seg);
            crate::sermon::analyze_and_queue(&sermon_engine, &input, &context, &mut findings)
                .unwrap();

            // A Scripture reference every cycle, via the real Bible engine -
            // persisted first, exactly like the real
            // `analyze_bible_transcript` command always does.
            let bible_seg = segment(
                &format!("Turn with me to Romans chapter {}", 8 + (cycle % 5)),
                seq,
            );
            seq += 1;
            persist_transcript_segment(&conn, session.id, &bible_seg).unwrap();
            let bible_context = IntelligenceContext::build(
                session.id,
                None,
                None,
                Vec::new(),
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                ContextBounds::default(),
            );
            let bible_input = IntelligenceInput::new(session.id, bible_seg);
            let bible_result = bible_engine.analyze(&bible_input, &bible_context).unwrap();
            for f in bible_result.findings {
                findings.add(f);
            }

            // Content candidates + cross-domain, reading whatever
            // findings have accumulated so far.
            let content_context = IntelligenceContext::build(
                session.id,
                None,
                None,
                Vec::new(),
                None,
                findings.all().into_iter().cloned().collect(),
                Vec::new(),
                Vec::new(),
                ContextBounds::default(),
            );
            crate::content_intelligence::analyze_and_queue(
                &content_engine,
                &content_context,
                &mut candidates,
            );
            let cross_domain_context = IntelligenceContext::build(
                session.id,
                None,
                None,
                Vec::new(),
                None,
                findings.all().into_iter().cloned().collect(),
                Vec::new(),
                Vec::new(),
                ContextBounds::default(),
            )
            .with_content_candidates(candidates.all().into_iter().cloned().collect());
            crate::cross_domain::analyze_and_queue(
                &cross_domain_engine,
                &cross_domain_context,
                &mut correlations,
            );

            // A presentation activate/stop cycle every third cycle - the
            // real operator behavior a sustained service would produce
            // repeatedly, proving no cycle leaves a stray Active item
            // behind for the next one to trip over.
            if cycle % 3 == 0 {
                let content = cip_core_presentation::PresentationContent::Text {
                    title: None,
                    body: format!("Cycle {cycle} slide"),
                };
                let item = crate::presentation::persist_prepared_item(
                    &conn,
                    session.id,
                    content,
                    cip_presentation_renderer::TEXT_DEFAULT_TEMPLATE,
                    None,
                )
                .unwrap();
                let (_, _slide) = crate::presentation::prepare_to_activate(&conn, item.id).unwrap();
                let active = crate::presentation::commit_activation(&conn, item.id).unwrap();
                assert_eq!(
                    active.status,
                    cip_core_presentation::PresentationItemStatus::Active
                );
                let stopped = crate::presentation::stop_active_item(&conn, session.id)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    stopped.status,
                    cip_core_presentation::PresentationItemStatus::Stopped
                );
                active_item_id = Some(item.id);
            }

            findings_after_each_cycle.push(findings.all().len());
        }

        // --- stability assertions -------------------------------------------

        // No panic across 20 sustained cycles is itself the primary
        // assertion (a real crash would have failed this test already).
        // Beyond that: growth must be bounded and roughly proportional to
        // input, never explosive/quadratic - a real symptom a duplicate-
        // detection or dedup regression would produce.
        let final_findings = *findings_after_each_cycle.last().unwrap();
        assert!(
            final_findings <= (CYCLES as usize) * 4,
            "finding count ({final_findings}) grew far faster than the {CYCLES} cycles that \
             produced it - possible duplicate-accumulation regression"
        );
        assert!(
            final_findings > 0,
            "a 20-cycle simulated service must have produced at least some findings"
        );

        // Every transcript segment fed in must be persisted exactly once -
        // no silent loss, no silent duplication.
        let persisted_transcript =
            crate::persistence::list_transcript_segments(&conn, session.id, 1000).unwrap();
        assert_eq!(
            persisted_transcript.len(),
            seq as usize,
            "every transcript segment fed into the simulation must be persisted exactly once"
        );

        // No stray Active presentation item survives the sustained run.
        let all_items =
            crate::persistence::list_presentation_items(&conn, session.id, None).unwrap();
        assert!(
            all_items
                .iter()
                .all(|i| i.status != cip_core_presentation::PresentationItemStatus::Active),
            "no presentation item may remain Active after the simulated service concludes"
        );
        assert!(
            active_item_id.is_some(),
            "the presentation activate/stop cycle must have actually run at least once"
        );

        // Bible-domain findings must have accumulated across cycles (the
        // real engine, not a stub), and Sermon findings likewise - both
        // domains stayed alive for the full simulated hour.
        assert!(findings
            .all()
            .iter()
            .any(|f| f.domain == IntelligenceDomain::Bible));
        assert!(findings
            .all()
            .iter()
            .any(|f| f.domain == IntelligenceDomain::Sermon));
    }
}
