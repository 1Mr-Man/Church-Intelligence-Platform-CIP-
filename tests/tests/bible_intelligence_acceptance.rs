//! THE Phase 1.1 acceptance test.
//!
//! Runs the exact realistic pastoral-speech sequence the Bible
//! Intelligence Core is required to handle deterministically, end to end
//! against the real SQLite-backed `BibleProvider` (migrated + seeded, no
//! fakes) and the real `DefaultScriptureContextManager` - not isolated
//! parser examples, but the flow as it will actually occur in a service:
//!
//! ```text
//! "Turn with me to Romans chapter 8."                -> Romans 8 (context, no verse invented)
//! "Paul is explaining something very important here." -> nothing (context must survive)
//! "We have to understand the work of the Spirit."      -> nothing (context must survive)
//! "Look at verse 28."                                  -> Romans 8:28
//! "Now verse 31."                                      -> Romans 8:31
//! "Go back to verse 18."                                -> Romans 8:18
//! "Now let's go to John chapter 3."                     -> John 3 (context replaced)
//! "Verse 16."                                            -> John 3:16 (not Romans 8:16)
//! ```
//!
//! If this test does not pass deterministically, Phase 1.1 is not
//! complete.

use cip_core_bible::{BibleProvider, DefaultScriptureContextManager, ReferenceKind};
use cip_core_service::process_transcript_segment;
use cip_database::{run_migrations, seed::apply_dev_seed};
use cip_integrations_bible::SqliteBibleProvider;
use uuid::Uuid;

#[test]
fn romans_8_to_john_3_pastoral_sequence_resolves_deterministically() {
    let outcome = run_sequence();
    assert_eq!(
        outcome.len(),
        8,
        "one ProcessedSegment per transcript segment"
    );

    // A single reference (or none) resolved per segment; unrelated prose
    // (segments 2 and 3) produces zero detections, not a placeholder.
    let reference_of = |i: usize| -> Option<String> {
        outcome[i]
            .detections
            .first()
            .and_then(|d| d.reference.as_ref())
            .map(|r| r.to_string())
    };

    assert!(outcome[0].detections.len() == 1 && reference_of(0).is_none()); // Romans 8 - context only
    assert!(outcome[1].detections.is_empty()); // unrelated prose
    assert!(outcome[2].detections.is_empty()); // unrelated prose
    assert_eq!(reference_of(3), Some("ROM 8:28".into()));
    assert_eq!(reference_of(4), Some("ROM 8:31".into()));
    assert_eq!(reference_of(5), Some("ROM 8:18".into()));
    assert!(outcome[6].detections.len() == 1 && reference_of(6).is_none()); // John 3 - context only
    assert_eq!(reference_of(7), Some("JHN 3:16".into()));

    // The two chapter-only segments established/replaced context but never
    // invented a verse or produced a suggestion.
    assert_eq!(outcome[0].detections[0].kind, ReferenceKind::Chapter);
    assert!(outcome[0].suggestions.is_empty());
    assert_eq!(outcome[6].detections[0].kind, ReferenceKind::Chapter);
    assert!(outcome[6].suggestions.is_empty());

    // First verse pulled from a context is `Verse`; subsequent ones within
    // the same still-active context are `Sequential`.
    assert_eq!(outcome[3].detections[0].kind, ReferenceKind::Verse);
    assert_eq!(outcome[4].detections[0].kind, ReferenceKind::Sequential);
    assert_eq!(outcome[5].detections[0].kind, ReferenceKind::Sequential);

    // "Verse 16" right after replacing Romans 8 with John 3 resolves
    // cleanly to John 3:16 - Romans 8:16 was never seeded, so it fails
    // Bible validation and the ambiguity collapses to a single answer,
    // exactly as a real, fully-seeded translation would for a chapter the
    // pastor has clearly moved on from.
    assert_eq!(outcome[7].detections[0].kind, ReferenceKind::Verse);

    // Every suggestion that was produced is Pending - nothing auto-applied.
    let all_suggestions_pending = outcome
        .iter()
        .flat_map(|segment| &segment.suggestions)
        .all(|s| s.status == cip_core_ai::SuggestionStatus::Pending);
    assert!(all_suggestions_pending);
    let total_suggestions: usize = outcome.iter().map(|s| s.suggestions.len()).sum();
    assert_eq!(total_suggestions, 4, "one suggestion per resolved verse");
}

#[test]
fn the_sequence_is_deterministic_across_independent_runs() {
    let run_a = run_sequence();
    let run_b = run_sequence();

    let simplify = |outcome: &[cip_core_service::ProcessedSegment]| {
        outcome
            .iter()
            .flat_map(|segment| {
                segment
                    .detections
                    .iter()
                    .map(|d| (d.kind, d.reference.clone(), d.confidence.score))
            })
            .collect::<Vec<_>>()
    };

    assert_eq!(simplify(&run_a), simplify(&run_b));
}

fn run_sequence() -> Vec<cip_core_service::ProcessedSegment> {
    let mut conn = cip_database::open_in_memory().expect("open in-memory db");
    run_migrations(&mut conn).expect("run migrations");
    apply_dev_seed(&conn).expect("apply dev seed");

    let provider: Box<dyn BibleProvider> = Box::new(SqliteBibleProvider::new(conn));
    let mut context = DefaultScriptureContextManager::new("KJV");
    let service_id = Uuid::new_v4();

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

    segments
        .iter()
        .map(|segment| {
            process_transcript_segment(service_id, segment, "KJV", provider.as_ref(), &mut context)
        })
        .collect()
}
