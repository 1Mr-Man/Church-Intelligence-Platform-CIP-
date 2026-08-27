//! Content Registry orchestration (Phase 1.5): composes the Bible dataset
//! importer (`cip_integrations_bible`) with the Content Registry
//! (`cip_core_content`/`cip_integrations_content`) into one call, and
//! registers the dev-seeded fixture's own metadata.
//!
//! Deliberately Tauri-agnostic (plain `&Connection`/`&dyn ContentRegistry`
//! and domain types, no `AppHandle`/`State`), mirroring `pipeline.rs` and
//! `presentation.rs` - see their docs for why.

use chrono::Utc;
use cip_core_content::{
    ContentMetadata, ContentRegistry, ContentRegistryError, ContentStatus, ContentType,
};
use cip_integrations_bible::{import_bible_dataset, BibleDatasetInput, ImportError, ImportReport};
use rusqlite::Connection;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ContentError {
    #[error(transparent)]
    Import(#[from] ImportError),
    #[error(transparent)]
    Registry(#[from] ContentRegistryError),
}

/// The `content_registry.id` convention this module uses for Bible
/// content - see `core/content`'s docs on the `"<type>:<domain-id>"`
/// scheme.
pub fn bible_content_id(translation_id: &str) -> String {
    format!("bible:{translation_id}")
}

/// Registers the dev-seeded fixture (`database/seeds/dev_seed.sql`'s KJV
/// rows) with the Content Registry, with every licensing field honestly
/// `UNKNOWN` (`None`) - the dev seed never recorded real provenance, so
/// nothing here invents any. A no-op if this id is already registered
/// (e.g. a real dataset was already imported over it, or this isn't the
/// first launch) - never overwrites an existing, possibly more complete,
/// registration.
pub fn register_dev_seed_content_if_missing(
    registry: &dyn ContentRegistry,
) -> Result<(), ContentRegistryError> {
    let id = bible_content_id("KJV");
    if registry.get(&id)?.is_some() {
        return Ok(());
    }
    registry.register(&ContentMetadata {
        id,
        content_type: ContentType::Bible,
        name: "King James Version".to_string(),
        version: "dev-fixture".to_string(),
        language: "en".to_string(),
        source: "development fixture".to_string(),
        publisher: None,
        copyright: None,
        license: None,
        distribution: None,
        imported_at: Utc::now(),
        checksum: None,
        status: ContentStatus::Enabled,
        licensing_status: cip_core_content::LicensingStatus::Unknown,
    })
}

/// Imports a Bible dataset and registers/updates its Content Registry
/// metadata in one call. Preserves an existing registration's `status`
/// (enabled/disabled) across a re-import - re-importing content a human
/// deliberately disabled must not silently re-enable it.
pub fn import_and_register(
    conn: &Connection,
    registry: &dyn ContentRegistry,
    dataset: &BibleDatasetInput,
) -> Result<ImportReport, ContentError> {
    // Bulk-import validation (including the licensing safety gate) has
    // already happened inside `import_bible_dataset` by the time this
    // returns `Ok` - a rejected dataset never reaches this line at all,
    // so the registry is only ever asked to register content that has
    // already cleared the gate (section 19: "do not mark enabled before
    // successful validation/import").
    let report = import_bible_dataset(conn, dataset)?;

    let id = bible_content_id(&report.translation_id);
    let status = registry
        .get(&id)?
        .map(|existing| existing.status)
        .unwrap_or(ContentStatus::Enabled);

    registry.register(&ContentMetadata {
        id,
        content_type: ContentType::Bible,
        name: dataset.translation.name.clone(),
        version: report.dataset_version.clone(),
        language: dataset.translation.language.clone(),
        source: "user-provided import".to_string(),
        publisher: dataset.translation.publisher.clone(),
        copyright: dataset.translation.copyright.clone(),
        license: dataset.translation.license.clone(),
        distribution: dataset.translation.distribution.clone(),
        imported_at: Utc::now(),
        checksum: Some(report.checksum.clone()),
        status,
        licensing_status: report.licensing_status,
    })?;

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_database::{open_in_memory, run_migrations};
    use cip_integrations_bible::{TranslationInput, VerseInput};
    use cip_integrations_content::SqliteContentRegistry;

    fn migrated_conn() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn registry() -> SqliteContentRegistry {
        SqliteContentRegistry::new(migrated_conn())
    }

    #[test]
    fn registers_the_dev_seed_fixture_with_unknown_licensing() {
        let reg = registry();
        register_dev_seed_content_if_missing(&reg).unwrap();

        let metadata = reg.get("bible:KJV").unwrap().unwrap();
        assert_eq!(metadata.name, "King James Version");
        assert_eq!(metadata.publisher, None);
        assert_eq!(metadata.license, None);
        assert_eq!(metadata.status, ContentStatus::Enabled);
    }

    #[test]
    fn registering_the_dev_seed_fixture_twice_does_not_overwrite_an_existing_entry() {
        let reg = registry();
        register_dev_seed_content_if_missing(&reg).unwrap();
        reg.set_enabled("bible:KJV", false).unwrap();

        register_dev_seed_content_if_missing(&reg).unwrap();

        assert_eq!(
            reg.get("bible:KJV").unwrap().unwrap().status,
            ContentStatus::Disabled,
            "a no-op registration must never silently re-enable disabled content"
        );
    }

    fn small_dataset() -> BibleDatasetInput {
        BibleDatasetInput {
            translation: TranslationInput {
                id: "TEST".to_string(),
                name: "Test Translation".to_string(),
                abbreviation: "TST".to_string(),
                language: "en".to_string(),
                publisher: Some("Test Publisher".to_string()),
                copyright: Some("Public Domain".to_string()),
                license: Some("public domain".to_string()),
                distribution: Some("public domain".to_string()),
                dataset_version: "1.0".to_string(),
                licensing_status: "verified_public_domain".to_string(),
            },
            verses: vec![VerseInput {
                book: "Romans".to_string(),
                chapter: 8,
                verse: 28,
                text: "And we know...".to_string(),
            }],
        }
    }

    #[test]
    fn import_and_register_populates_both_bible_tables_and_the_registry() {
        let conn = migrated_conn();
        let reg = SqliteContentRegistry::new(migrated_conn());
        // import_and_register writes Bible content into `conn` and
        // metadata into `reg` - two different connections here only
        // because the test wants to inspect each independently; in the
        // real app both point at the same on-disk file (see state.rs).
        let report = import_and_register(&conn, &reg, &small_dataset()).unwrap();
        assert_eq!(report.imported, 1);

        let metadata = reg.get("bible:TEST").unwrap().unwrap();
        assert_eq!(metadata.publisher.as_deref(), Some("Test Publisher"));
        assert_eq!(metadata.checksum.as_deref(), Some(report.checksum.as_str()));
        assert_eq!(metadata.status, ContentStatus::Enabled);
        assert_eq!(
            metadata.licensing_status,
            cip_core_content::LicensingStatus::VerifiedPublicDomain
        );
    }

    #[test]
    fn an_unverified_licensing_status_is_never_registered_and_never_enabled() {
        let conn = migrated_conn();
        let reg = SqliteContentRegistry::new(migrated_conn());
        let mut dataset = small_dataset();
        dataset.translation.licensing_status = "unknown".to_string();

        assert!(import_and_register(&conn, &reg, &dataset).is_err());
        assert!(
            reg.get("bible:TEST").unwrap().is_none(),
            "a rejected import must never reach the content registry at all"
        );
    }

    #[test]
    fn reimporting_preserves_a_previously_disabled_status() {
        let conn = migrated_conn();
        let reg = SqliteContentRegistry::new(migrated_conn());
        import_and_register(&conn, &reg, &small_dataset()).unwrap();
        reg.set_enabled("bible:TEST", false).unwrap();

        import_and_register(&conn, &reg, &small_dataset()).unwrap();

        assert_eq!(
            reg.get("bible:TEST").unwrap().unwrap().status,
            ContentStatus::Disabled
        );
    }

    /// The primary milestone test (spec section 44): the real, complete,
    /// checked-in BSB production dataset, imported through the exact same
    /// path the real app uses at startup, proves every acceptance
    /// criterion in one place - 66 books, valid structure, deterministic
    /// checksum, correct registry metadata, idempotent re-import,
    /// real-text search, translation isolation, presentation-ready - all
    /// against the real SQLite-backed provider, never a fixture.
    #[test]
    fn phase_real_bible_dataset_full_validation() {
        use cip_core_bible::{check_bible_integrity, search_bible, IntegrityStatus};
        use cip_integrations_bible::SqliteBibleProvider;

        let conn = migrated_conn();
        let reg = SqliteContentRegistry::new(migrated_conn());
        let dataset = crate::bible_production_dataset::bsb_dataset();

        // --- import + registry -------------------------------------------------
        let first = import_and_register(&conn, &reg, &dataset).unwrap();
        assert_eq!(
            first.translation_id,
            crate::bible_production_dataset::BSB_TRANSLATION_ID
        );
        assert_eq!(
            first.books, 66,
            "the real dataset must cover the complete 66-book canon"
        );
        assert_eq!(
            first.invalid, 0,
            "no row in the checked-in production dataset should be rejected"
        );
        assert!(
            first.imported > 30_000,
            "expected tens of thousands of real verses, got {}",
            first.imported
        );
        assert_eq!(first.already_present, 0);

        let metadata = reg.get("bible:BSB").unwrap().unwrap();
        assert_eq!(
            metadata.status,
            ContentStatus::Enabled,
            "dataset enabled only after successful validation/import"
        );
        assert_eq!(
            metadata.licensing_status,
            cip_core_content::LicensingStatus::VerifiedPublicDomain
        );
        assert_eq!(metadata.checksum.as_deref(), Some(first.checksum.as_str()));

        // --- idempotent re-import -----------------------------------------------
        let second = import_and_register(&conn, &reg, &dataset).unwrap();
        assert_eq!(
            second.imported, 0,
            "a second import of the same dataset must insert nothing new"
        );
        assert_eq!(second.already_present, first.imported);
        assert_eq!(
            second.checksum, first.checksum,
            "identical content must produce an identical checksum"
        );

        // --- a second translation, imported into the same database, for the
        //     translation-isolation check below - done here, on `conn`
        //     directly, before `conn` moves into the provider below.
        let mut kjv_dataset = dataset.clone();
        kjv_dataset.translation.id = "KJV".to_string();
        kjv_dataset.translation.abbreviation = "KJV".to_string();
        kjv_dataset.verses = vec![cip_integrations_bible::VerseInput {
            book: "JHN".to_string(),
            chapter: 3,
            verse: 16,
            text: "KJV WORDING - for testing isolation only".to_string(),
        }];
        cip_integrations_bible::import_bible_dataset(&conn, &kjv_dataset).unwrap();

        // --- 66-book structural integrity, against the real provider ------------
        let provider = SqliteBibleProvider::new(conn);
        let report = check_bible_integrity(&provider, "BSB").unwrap();
        assert_eq!(
            report.status,
            IntegrityStatus::Valid,
            "issues: {:?}",
            report.issues
        );
        assert_eq!(report.books_present, 66);
        assert_eq!(report.books_expected, 66);
        assert!(report.issues.is_empty());

        // --- exact verse lookup, using the real imported text --------------------
        let provider: &dyn cip_core_bible::BibleProvider = &provider;
        for (book, chapter, verse, expected_substring) in [
            ("GEN", 1, 1, "In the beginning"),
            ("JHN", 3, 16, "God so loved the world"),
            ("ROM", 8, 28, "God works all things together for the good"),
            ("ROM", 8, 31, "who can be against us"),
            ("PSA", 23, 1, "my shepherd"),
            ("REV", 22, 21, "grace of the Lord Jesus"),
            ("MAT", 1, 1, "genealogy of Jesus Christ"),
        ] {
            let reference = cip_core_bible::ScriptureReference::single("BSB", book, chapter, verse);
            let found = provider.get_verse(&reference).unwrap().unwrap_or_else(|| {
                panic!("{book} {chapter}:{verse} missing from the real imported dataset")
            });
            assert!(
                found.text.contains(expected_substring),
                "{book} {chapter}:{verse} = {:?}, expected it to contain {:?}",
                found.text,
                expected_substring
            );
        }

        // --- chapter, range, and free-text search --------------------------------
        let chapter_results = search_bible(provider, "BSB", "1 Corinthians 13").unwrap();
        assert_eq!(chapter_results.len(), 13, "1 Corinthians 13 has 13 verses");
        assert_eq!(chapter_results[0].verse, 1);

        let range_results = search_bible(provider, "BSB", "Romans 8:28-31").unwrap();
        assert_eq!(
            range_results.iter().map(|r| r.verse).collect::<Vec<_>>(),
            vec![28, 29, 30, 31]
        );
        assert!(
            search_bible(provider, "BSB", "Romans 8:31-28").is_err(),
            "an inverted range must be rejected, not silently reordered"
        );

        let free_text = search_bible(provider, "BSB", "shepherd").unwrap();
        assert!(free_text
            .iter()
            .any(|r| r.book == "PSA" && r.chapter == 23 && r.verse == 1));

        // invalid references never produce results
        assert!(search_bible(provider, "BSB", "Romans 8:999")
            .unwrap()
            .is_empty());
        assert!(search_bible(provider, "BSB", "Fakebook 1:1")
            .unwrap()
            .is_empty());

        // --- translation isolation: requesting BSB never returns KJV, and
        //     vice versa, even though both exist in the same database
        //     (the second translation was imported into `conn` above,
        //     before it moved into the provider) -----------------------------
        let bsb_john = provider
            .get_verse(&cip_core_bible::ScriptureReference::single(
                "BSB", "JHN", 3, 16,
            ))
            .unwrap()
            .unwrap();
        assert!(
            bsb_john.text.contains("one and only"),
            "BSB text must stay BSB text, not KJV's"
        );
        let kjv_john = provider
            .get_verse(&cip_core_bible::ScriptureReference::single(
                "KJV", "JHN", 3, 16,
            ))
            .unwrap()
            .unwrap();
        assert_eq!(kjv_john.text, "KJV WORDING - for testing isolation only");

        // --- disabled-dataset safety: is_translation_selectable is the real
        //     gate `search_bible`/presentation commands check -------------------
        let enabled_lookup = reg.get("bible:BSB").unwrap();
        assert!(crate::commands::is_translation_selectable(Ok(
            enabled_lookup.as_ref()
        )));
        reg.set_enabled("bible:BSB", false).unwrap();
        let disabled_lookup = reg.get("bible:BSB").unwrap();
        assert!(!crate::commands::is_translation_selectable(Ok(
            disabled_lookup.as_ref()
        )));
        reg.set_enabled("bible:BSB", true).unwrap();
        let reenabled_lookup = reg.get("bible:BSB").unwrap();
        assert!(crate::commands::is_translation_selectable(Ok(
            reenabled_lookup.as_ref()
        )));

        // --- presentation-ready: the real imported text survives unchanged
        //     through build_scripture_slide -----------------------------------
        let (content, slide) =
            crate::presentation::build_scripture_slide(provider, "BSB", "JHN 3:16").unwrap();
        let cip_core_presentation::PresentationContent::Scripture {
            translation_id,
            text,
            ..
        } = &content
        else {
            panic!("expected Scripture content");
        };
        assert_eq!(translation_id, "BSB");
        assert!(text.contains("one and only"));
        assert!(slide.body_lines.join(" ").contains("one and only"));
    }
}
