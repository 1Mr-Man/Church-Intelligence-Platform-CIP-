//! Music content/dataset orchestration (Phase 2.1) - the music-domain
//! counterpart to `content.rs`. Deliberately Tauri-agnostic (plain
//! `&Connection`/`&dyn ContentRegistry`/domain types, no `AppHandle`/
//! `State`), matching `content.rs`/`presentation.rs`/`intelligence.rs`.

use chrono::Utc;
use cip_core_content::{
    ContentMetadata, ContentRegistry, ContentRegistryError, ContentStatus, ContentType,
};
use cip_core_intelligence::{
    FindingQueue, IntelligenceContext, IntelligenceEngine, IntelligenceEngineRegistry,
    IntelligenceError, IntelligenceFinding, IntelligenceInput, MusicIntelligenceEngine,
    QueueAddOutcome,
};
use cip_core_music::MusicProvider;
use cip_integrations_music::{import_music_dataset, ImportError, ImportReport, MusicDatasetInput};
use rusqlite::Connection;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MusicError {
    #[error(transparent)]
    Import(#[from] ImportError),
    #[error(transparent)]
    Registry(#[from] ContentRegistryError),
}

/// Registers the three dev-seeded music fixture datasets
/// (`database/seeds/dev_seed.sql`) with the Content Registry - mirroring
/// `content::register_dev_seed_content_if_missing` exactly, including its
/// "no-op if already registered, never overwrite" discipline.
///
/// `music:dev-disabled-set` is registered `Disabled` from the start,
/// deliberately - a fixture proving "a disabled dataset is never used for
/// automatic recognition" (Phase 2.1 spec section 22) needs a disabled
/// dataset to exist in the first place, and honestly recording it as
/// starting disabled (rather than enabling then immediately disabling
/// it) is the more truthful representation.
pub fn register_dev_seed_music_content_if_missing(
    registry: &dyn ContentRegistry,
) -> Result<(), ContentRegistryError> {
    let datasets = [
        (
            "music:dev-hymnbook",
            "Test Fixture Hymnbook",
            ContentStatus::Enabled,
        ),
        (
            "music:dev-worship-set",
            "Test Fixture Worship Set",
            ContentStatus::Enabled,
        ),
        (
            "music:dev-disabled-set",
            "Test Fixture Disabled Set",
            ContentStatus::Disabled,
        ),
    ];
    for (id, name, status) in datasets {
        if registry.get(id)?.is_some() {
            continue;
        }
        registry.register(&ContentMetadata {
            id: id.to_string(),
            content_type: ContentType::Music,
            name: name.to_string(),
            version: "dev-fixture".to_string(),
            language: "en".to_string(),
            source: "development fixture".to_string(),
            publisher: None,
            copyright: None,
            license: None,
            distribution: None,
            imported_at: Utc::now(),
            checksum: None,
            status,
        })?;
    }
    Ok(())
}

/// Imports a music dataset and registers/updates its Content Registry
/// metadata in one call - mirrors `content::import_and_register` exactly,
/// including preserving an existing registration's enabled/disabled
/// status across a re-import.
pub fn import_and_register_music(
    conn: &Connection,
    registry: &dyn ContentRegistry,
    dataset: &MusicDatasetInput,
) -> Result<ImportReport, MusicError> {
    let report = import_music_dataset(conn, dataset)?;

    let status = registry
        .get(&dataset.content_id)?
        .map(|existing| existing.status)
        .unwrap_or(ContentStatus::Enabled);

    registry.register(&ContentMetadata {
        id: dataset.content_id.clone(),
        content_type: ContentType::Music,
        name: dataset.name.clone(),
        version: dataset.dataset_version.clone(),
        language: dataset.language.clone(),
        source: "user-provided import".to_string(),
        publisher: dataset.publisher.clone(),
        copyright: dataset.copyright.clone(),
        license: dataset.license.clone(),
        distribution: dataset.distribution.clone(),
        imported_at: Utc::now(),
        checksum: Some(report.checksum.clone()),
        status,
    })?;

    Ok(report)
}

/// Registers the real `MusicIntelligenceEngine` into an already-built
/// `IntelligenceEngineRegistry` (see `intelligence::build_registry`) -
/// kept as a separate step, not folded into `build_registry` itself, so
/// `core/intelligence`'s registry construction never has to know about
/// `cip_integrations_music` directly.
pub fn register_music_engine(
    registry: &mut IntelligenceEngineRegistry,
    provider: Box<dyn MusicProvider>,
) -> Result<(), IntelligenceError> {
    registry.register(Box::new(MusicIntelligenceEngine::new(provider)))
}

/// Calls `engine.analyze` and queues any genuinely new findings - the
/// plain, directly-testable core of `commands::analyze_music_transcript`.
/// Mirrors `pipeline::handle_final_transcript`'s split: the Tauri command
/// itself only handles persisting the transcript segment, building the
/// real `IntelligenceContext`, and IPC/event concerns, while the decision
/// logic worth testing in isolation (call the engine, then queue only
/// what `FindingQueue::add` accepts as non-duplicate) lives here. This
/// function has no way to create a `PresentationItem` or any other
/// side effect - it only ever calls `IntelligenceEngine::analyze` and
/// `FindingQueue::add`.
pub fn analyze_and_queue(
    engine: &dyn IntelligenceEngine,
    input: &IntelligenceInput,
    context: &IntelligenceContext,
    findings: &mut FindingQueue,
) -> Result<Vec<IntelligenceFinding>, IntelligenceError> {
    let result = engine.analyze(input, context)?;
    let mut queued = Vec::new();
    for finding in result.findings {
        if findings.add(finding.clone()) == QueueAddOutcome::Added {
            queued.push(finding);
        }
    }
    Ok(queued)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use cip_database::{open_in_memory, run_migrations, seed::apply_dev_seed};
    use cip_integrations_content::SqliteContentRegistry;
    use cip_integrations_music::{LyricInput, SectionInput, SongInput, SqliteMusicProvider};
    use uuid::Uuid;

    fn migrated_conn() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn registry() -> SqliteContentRegistry {
        SqliteContentRegistry::new(migrated_conn())
    }

    #[test]
    fn registers_all_three_dev_seed_datasets_with_expected_status() {
        let reg = registry();
        register_dev_seed_music_content_if_missing(&reg).unwrap();

        let hymnbook = reg.get("music:dev-hymnbook").unwrap().unwrap();
        assert_eq!(hymnbook.status, ContentStatus::Enabled);
        assert_eq!(hymnbook.publisher, None);

        let disabled = reg.get("music:dev-disabled-set").unwrap().unwrap();
        assert_eq!(disabled.status, ContentStatus::Disabled);
    }

    #[test]
    fn registering_twice_never_overwrites_an_existing_entry() {
        let reg = registry();
        register_dev_seed_music_content_if_missing(&reg).unwrap();
        reg.set_enabled("music:dev-hymnbook", false).unwrap();

        register_dev_seed_music_content_if_missing(&reg).unwrap();

        assert_eq!(
            reg.get("music:dev-hymnbook").unwrap().unwrap().status,
            ContentStatus::Disabled,
            "a no-op registration must never silently re-enable disabled content"
        );
    }

    fn small_dataset() -> MusicDatasetInput {
        MusicDatasetInput {
            content_id: "music:test".to_string(),
            name: "Test Dataset".to_string(),
            language: "en".to_string(),
            publisher: Some("Test Publisher".to_string()),
            copyright: None,
            license: Some("public domain".to_string()),
            distribution: Some("public domain".to_string()),
            dataset_version: "1.0".to_string(),
            songs: vec![SongInput {
                id: "s1".to_string(),
                title: "Test Song".to_string(),
                aliases: vec![],
                song_type: "hymn".to_string(),
                language: "en".to_string(),
                number: None,
                author: None,
                composer: None,
                sections: vec![SectionInput {
                    id: "v1".to_string(),
                    kind: "verse".to_string(),
                    sequence: 0,
                }],
                lyrics: vec![LyricInput {
                    section_id: Some("v1".to_string()),
                    sequence: 0,
                    text: "A line".to_string(),
                }],
            }],
        }
    }

    #[test]
    fn import_and_register_populates_both_music_tables_and_the_registry() {
        let conn = migrated_conn();
        let reg = SqliteContentRegistry::new(migrated_conn());
        let report = import_and_register_music(&conn, &reg, &small_dataset()).unwrap();
        assert_eq!(report.songs_imported, 1);

        let metadata = reg.get("music:test").unwrap().unwrap();
        assert_eq!(metadata.publisher.as_deref(), Some("Test Publisher"));
        assert_eq!(metadata.checksum.as_deref(), Some(report.checksum.as_str()));
        assert_eq!(metadata.status, ContentStatus::Enabled);
    }

    #[test]
    fn reimporting_music_preserves_a_previously_disabled_status() {
        let conn = migrated_conn();
        let reg = SqliteContentRegistry::new(migrated_conn());
        import_and_register_music(&conn, &reg, &small_dataset()).unwrap();
        reg.set_enabled("music:test", false).unwrap();

        import_and_register_music(&conn, &reg, &small_dataset()).unwrap();

        assert_eq!(
            reg.get("music:test").unwrap().unwrap().status,
            ContentStatus::Disabled
        );
    }

    // --- `analyze_and_queue` operator-workflow + degradation tests --------
    //
    // Unlike `core/intelligence::music_adapter`'s own tests (a
    // `FakeMusicProvider` fixture), these exercise the real
    // `SqliteMusicProvider` against the real dev-seed fixture data
    // (`database/seeds/dev_seed.sql`) and the real `SqliteContentRegistry`
    // - proving the full orchestration path (not just the engine in
    // isolation) end to end, matching `pipeline.rs`'s tests doing the same
    // for Bible.

    fn seeded_music_conn() -> Connection {
        let conn = migrated_conn();
        apply_dev_seed(&conn).unwrap();
        conn
    }

    fn segment(text: &str) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence: 0,
            text: text.to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(1.0, ConfidenceSource::Human, None),
            start_ms: 0,
            end_ms: 0,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    /// Builds a real `IntelligenceInput`/`IntelligenceContext` pair for
    /// `text` under `service_id`, with `content_metadata` as the only
    /// context content - real enough for `analyze_and_queue` to exercise
    /// its full dataset-enablement filtering against genuine Content
    /// Registry rows. Callers that need two calls to be recognized as
    /// "the same service" (e.g. duplicate-detection) must pass the same
    /// `service_id` to both.
    fn input_and_context(
        service_id: Uuid,
        text: &str,
        content_metadata: Vec<ContentMetadata>,
    ) -> (IntelligenceInput, IntelligenceContext) {
        let seg = segment(text);
        let context = IntelligenceContext::build(
            service_id,
            None,
            Some(seg.clone()),
            vec![seg.clone()],
            None,
            Vec::new(),
            Vec::new(),
            content_metadata,
            cip_core_intelligence::ContextBounds::default(),
        );
        (IntelligenceInput::new(service_id, seg), context)
    }

    #[test]
    fn analyze_and_queue_recognizes_a_real_dev_seed_song_by_exact_title() {
        let provider = SqliteMusicProvider::new(seeded_music_conn());
        let engine = MusicIntelligenceEngine::new(Box::new(provider));

        let reg = registry();
        register_dev_seed_music_content_if_missing(&reg).unwrap();
        let content_metadata = reg.list(None).unwrap();

        let (input, context) =
            input_and_context(Uuid::new_v4(), "Test Fixture Hymn One", content_metadata);
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();

        assert_eq!(queued.len(), 1);
        assert_eq!(
            queued[0].domain,
            cip_core_intelligence::IntelligenceDomain::Music
        );
        assert_eq!(
            queued[0].provenance.content_id.as_deref(),
            Some("music:dev-hymnbook"),
            "the real dev-seed hymnbook dataset, not a synthetic fixture, produced this finding"
        );
        assert_eq!(findings.pending().len(), 1);
    }

    /// Hard degradation requirement (Phase 2.1 spec section 22): even an
    /// exact title match in a disabled dataset must never be searched.
    /// `music:dev-disabled-set` is registered `Disabled` from the start
    /// (see `register_dev_seed_music_content_if_missing`'s docs) and its
    /// dev-seeded song "Disabled Fixture Song" is a real row in the same
    /// database the enabled hymnbook's rows live in - so this proves the
    /// filtering, not just an empty table.
    #[test]
    fn analyze_and_queue_never_searches_a_disabled_dataset_even_on_an_exact_title() {
        let provider = SqliteMusicProvider::new(seeded_music_conn());
        let engine = MusicIntelligenceEngine::new(Box::new(provider));

        let reg = registry();
        register_dev_seed_music_content_if_missing(&reg).unwrap();
        let content_metadata = reg.list(None).unwrap();
        assert!(
            content_metadata
                .iter()
                .any(|m| m.id == "music:dev-disabled-set" && m.status == ContentStatus::Disabled),
            "test premise: the disabled dataset must actually be registered disabled"
        );

        let (input, context) =
            input_and_context(Uuid::new_v4(), "Disabled Fixture Song", content_metadata);
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();

        assert!(
            queued.is_empty(),
            "an exact title match in a disabled dataset must never become a finding"
        );
        assert!(findings.is_empty());
    }

    /// Missing-content degradation: no Music content registered at all
    /// (a fresh install before any dataset import) must behave exactly
    /// like "no match" - no error, no panic, an empty queue.
    #[test]
    fn analyze_and_queue_with_no_registered_music_content_yields_no_findings_and_no_error() {
        let provider = SqliteMusicProvider::new(seeded_music_conn());
        let engine = MusicIntelligenceEngine::new(Box::new(provider));

        let (input, context) =
            input_and_context(Uuid::new_v4(), "Test Fixture Hymn One", Vec::new());
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();

        assert!(queued.is_empty());
        assert!(findings.is_empty());
    }

    /// Empty-dataset degradation: a dataset registered and enabled in the
    /// Content Registry but with no imported songs (an empty in-memory
    /// database, not the dev-seed one) must also degrade to "no match,"
    /// never an error.
    #[test]
    fn analyze_and_queue_against_an_empty_dataset_yields_no_findings_and_no_error() {
        let provider = SqliteMusicProvider::new(migrated_conn());
        let engine = MusicIntelligenceEngine::new(Box::new(provider));

        let reg = registry();
        register_dev_seed_music_content_if_missing(&reg).unwrap();
        let content_metadata = reg.list(None).unwrap();

        let (input, context) =
            input_and_context(Uuid::new_v4(), "Test Fixture Hymn One", content_metadata);
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();

        assert!(queued.is_empty());
    }

    /// Operator-workflow proof: a finding queued from a real `analyze_and_queue`
    /// call can be accepted through the ordinary `FindingQueue` lifecycle,
    /// and doing so changes only its own status - `music.rs` has no
    /// dependency on `cip_core_presentation` at all (see this module's
    /// imports), so there is no code path here capable of creating a
    /// `PresentationItem`.
    #[test]
    fn a_queued_finding_can_be_accepted_and_only_changes_its_own_status() {
        let provider = SqliteMusicProvider::new(seeded_music_conn());
        let engine = MusicIntelligenceEngine::new(Box::new(provider));

        let reg = registry();
        register_dev_seed_music_content_if_missing(&reg).unwrap();
        let content_metadata = reg.list(None).unwrap();

        let (input, context) =
            input_and_context(Uuid::new_v4(), "Test Fixture Hymn One", content_metadata);
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();
        let id = queued[0].id;

        findings.accept(id).unwrap();
        assert_eq!(
            findings.get(id).unwrap().status,
            cip_core_intelligence::FindingStatus::Accepted
        );
        assert!(
            findings.pending().is_empty(),
            "an accepted finding is no longer pending operator review"
        );
    }

    /// A second, textually-identical call must not duplicate a still-
    /// pending finding - `FindingQueue::add`'s own equivalence rule,
    /// exercised here through the real orchestration path.
    #[test]
    fn analyze_and_queue_does_not_duplicate_a_repeated_identical_call() {
        let provider = SqliteMusicProvider::new(seeded_music_conn());
        let engine = MusicIntelligenceEngine::new(Box::new(provider));

        let reg = registry();
        register_dev_seed_music_content_if_missing(&reg).unwrap();
        let content_metadata = reg.list(None).unwrap();

        let mut findings = FindingQueue::new();
        let service_id = Uuid::new_v4();
        let (input1, context1) = input_and_context(
            service_id,
            "Test Fixture Hymn One",
            content_metadata.clone(),
        );
        let first = analyze_and_queue(&engine, &input1, &context1, &mut findings).unwrap();
        assert_eq!(first.len(), 1);

        let (input2, context2) =
            input_and_context(service_id, "Test Fixture Hymn One", content_metadata);
        let second = analyze_and_queue(&engine, &input2, &context2, &mut findings).unwrap();
        assert!(
            second.is_empty(),
            "a duplicate finding for the same service/domain/kind/summary is discarded, not re-queued"
        );
        assert_eq!(findings.pending().len(), 1);
    }
}
