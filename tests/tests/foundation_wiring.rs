//! Proves the Phase 1 foundation is wired together correctly: a service
//! session is started, a scripture reference is resolved through the real
//! SQLite-backed `BibleProvider`, wrapped in a `Suggestion`, approved into a
//! `PresentationItem`, and handed to a `Renderer` - crossing every domain
//! boundary the architecture defines, using only the public contracts each
//! domain exposes.

use cip_ai_speech::NullSpeechEngine;
use cip_core_ai::{SpeechEngine, Suggestion, SuggestionKind, SuggestionStatus};
use cip_core_bible::{BibleProvider, ScriptureReference};
use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use cip_core_presentation::{PresentationContent, PresentationItem};
use cip_core_service::{ServiceSession, ServiceStatus};
use cip_database::{run_migrations, seed::apply_dev_seed};
use cip_integrations_bible::SqliteBibleProvider;
use cip_presentation_renderer::{NullRenderer, Renderer};

#[test]
fn a_scripture_reference_flows_from_provider_to_rendered_presentation_item() {
    // ServiceSession: the service domain's top-level unit of work.
    let session = ServiceSession::start("Integration Test Service");
    assert_eq!(session.status, ServiceStatus::Started);

    // SQLite foundation: migrate an in-memory database and seed it with a
    // handful of verses (not a full Bible dataset).
    let mut conn = cip_database::open_in_memory().expect("open in-memory db");
    run_migrations(&mut conn).expect("run migrations");
    apply_dev_seed(&conn).expect("apply dev seed");

    // Bible domain: resolve a verse through the provider/adaptor contract.
    let provider = SqliteBibleProvider::new(conn);
    let reference = ScriptureReference::single("KJV", "ROM", 8, 28);
    let verse = provider
        .get_verse(&reference)
        .expect("query should succeed")
        .expect("seeded verse should exist");

    // AI domain: wrap the detection in a Suggestion, starting Pending.
    let mut suggestion = Suggestion::new(
        session.id,
        SuggestionKind::Scripture {
            reference: reference.to_string(),
        },
        ConfidenceResult::new(0.92, ConfidenceSource::Heuristic, "exact match".to_string()),
    );
    assert_eq!(suggestion.status, SuggestionStatus::Pending);
    assert!(suggestion.confidence.is_auto_acceptable());

    // Human control: nothing auto-applies a suggestion - the flow only
    // proceeds once it is explicitly approved.
    suggestion.status = SuggestionStatus::Approved;

    // Presentation domain: an approved suggestion becomes a queued item.
    let item = PresentationItem::prepare(
        session.id,
        PresentationContent::Scripture {
            reference: reference.to_string(),
            translation_id: "KJV".to_string(),
            text: verse.text.clone(),
        },
    );

    // presentation/renderer: the rendering subsystem never sees the
    // suggestion or the provider - only the finished PresentationItem.
    let mut renderer = NullRenderer::default();
    renderer.render(&item).expect("render should succeed");
    assert_eq!(renderer.last_rendered.as_ref(), Some(&item));

    // Speech domain: SpeechEngine is wireable as a trait object even though
    // no real transcription backend exists yet.
    let speech_engine: Box<dyn SpeechEngine> = Box::new(NullSpeechEngine);
    assert!(!speech_engine.is_ready());
}
