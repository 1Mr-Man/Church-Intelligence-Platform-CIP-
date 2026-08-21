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
}
