//! Local SQLite-backed [`ContentRegistry`]. Mirrors
//! `integrations/bible::SqliteBibleProvider`'s shape: its own connection,
//! `Mutex`-guarded for interior mutability behind a shared `&self`.

use chrono::{DateTime, Utc};
use cip_core_content::{
    ContentMetadata, ContentRegistry, ContentRegistryError, ContentStatus, LicensingStatus,
    UsagePermissions,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Mutex;

pub struct SqliteContentRegistry {
    conn: Mutex<Connection>,
}

impl SqliteContentRegistry {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }
}

fn content_type_str(value: cip_core_content::ContentType) -> &'static str {
    use cip_core_content::ContentType;
    match value {
        ContentType::Bible => "bible",
        ContentType::Music => "music",
        ContentType::Service => "service",
        ContentType::Media => "media",
        ContentType::Reference => "reference",
    }
}

fn content_type_from_str(value: &str) -> Option<cip_core_content::ContentType> {
    use cip_core_content::ContentType;
    match value {
        "bible" => Some(ContentType::Bible),
        "music" => Some(ContentType::Music),
        "service" => Some(ContentType::Service),
        "media" => Some(ContentType::Media),
        "reference" => Some(ContentType::Reference),
        _ => None,
    }
}

fn status_str(value: ContentStatus) -> &'static str {
    match value {
        ContentStatus::Enabled => "enabled",
        ContentStatus::Disabled => "disabled",
    }
}

fn licensing_status_str(value: LicensingStatus) -> &'static str {
    match value {
        LicensingStatus::VerifiedPublicDomain => "verified_public_domain",
        LicensingStatus::VerifiedRedistributable => "verified_redistributable",
        LicensingStatus::LicensedForCip => "licensed_for_cip",
        LicensingStatus::Unknown => "unknown",
        LicensingStatus::Restricted => "restricted",
    }
}

fn licensing_status_from_str(value: &str) -> LicensingStatus {
    match value {
        "verified_public_domain" => LicensingStatus::VerifiedPublicDomain,
        "verified_redistributable" => LicensingStatus::VerifiedRedistributable,
        "licensed_for_cip" => LicensingStatus::LicensedForCip,
        "restricted" => LicensingStatus::Restricted,
        // Unrecognized/absent values fail closed to `Unknown`, never a
        // permissive status - a row this crate cannot positively identify
        // as verified must never be treated as verified.
        _ => LicensingStatus::Unknown,
    }
}

const ROW_COLUMNS: &str = "id, content_type, name, version, language, source, publisher, \
     copyright, license, distribution, imported_at, checksum, status, licensing_status, \
     rights_holder, source_provider, source_url, attribution_text, license_start, \
     license_expiry, distribution_allowed, offline_storage_allowed, projection_allowed, \
     api_allowed, commercial_allowed, ai_processing_allowed, llm_prompt_allowed, training_allowed";

fn parse_optional_datetime(raw: Option<String>) -> Option<DateTime<Utc>> {
    raw.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
    })
}

fn row_to_metadata(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContentMetadata> {
    let content_type_raw: String = row.get(1)?;
    let status_raw: String = row.get(12)?;
    let licensing_status_raw: String = row.get(13)?;
    let imported_at_raw: String = row.get(10)?;
    let usage = UsagePermissions {
        rights_holder: row.get(14)?,
        source_provider: row.get(15)?,
        source_url: row.get(16)?,
        attribution_text: row.get(17)?,
        license_start: parse_optional_datetime(row.get(18)?),
        license_expiry: parse_optional_datetime(row.get(19)?),
        distribution_allowed: row.get(20)?,
        offline_storage_allowed: row.get(21)?,
        projection_allowed: row.get(22)?,
        api_allowed: row.get(23)?,
        commercial_allowed: row.get(24)?,
        ai_processing_allowed: row.get(25)?,
        llm_prompt_allowed: row.get(26)?,
        training_allowed: row.get(27)?,
    };
    Ok(ContentMetadata {
        id: row.get(0)?,
        content_type: content_type_from_str(&content_type_raw)
            .unwrap_or(cip_core_content::ContentType::Reference),
        name: row.get(2)?,
        version: row.get(3)?,
        language: row.get(4)?,
        source: row.get(5)?,
        publisher: row.get(6)?,
        copyright: row.get(7)?,
        license: row.get(8)?,
        distribution: row.get(9)?,
        imported_at: DateTime::parse_from_rfc3339(&imported_at_raw)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        checksum: row.get(11)?,
        status: if status_raw == "disabled" {
            ContentStatus::Disabled
        } else {
            ContentStatus::Enabled
        },
        licensing_status: licensing_status_from_str(&licensing_status_raw),
        usage,
    })
}

impl ContentRegistry for SqliteContentRegistry {
    fn list(
        &self,
        content_type: Option<cip_core_content::ContentType>,
    ) -> Result<Vec<ContentMetadata>, ContentRegistryError> {
        let conn = self
            .conn
            .lock()
            .expect("content registry connection poisoned");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {ROW_COLUMNS} FROM content_registry
                 WHERE (?1 IS NULL OR content_type = ?1)
                 ORDER BY id"
            ))
            .map_err(|e| ContentRegistryError::Storage(e.to_string()))?;
        let filter = content_type.map(content_type_str);
        let rows = stmt
            .query_map(params![filter], row_to_metadata)
            .map_err(|e| ContentRegistryError::Storage(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| ContentRegistryError::Storage(e.to_string()))
    }

    fn get(&self, content_id: &str) -> Result<Option<ContentMetadata>, ContentRegistryError> {
        let conn = self
            .conn
            .lock()
            .expect("content registry connection poisoned");
        conn.query_row(
            &format!("SELECT {ROW_COLUMNS} FROM content_registry WHERE id = ?1"),
            params![content_id],
            row_to_metadata,
        )
        .optional()
        .map_err(|e| ContentRegistryError::Storage(e.to_string()))
    }

    fn register(&self, metadata: &ContentMetadata) -> Result<(), ContentRegistryError> {
        if metadata.id.trim().is_empty() {
            return Err(ContentRegistryError::InvalidMetadata(
                "content id must not be empty".to_string(),
            ));
        }
        let conn = self
            .conn
            .lock()
            .expect("content registry connection poisoned");
        conn.execute(
            "INSERT INTO content_registry
                (id, content_type, name, version, language, source, publisher, copyright,
                 license, distribution, imported_at, checksum, status, licensing_status,
                 rights_holder, source_provider, source_url, attribution_text, license_start,
                 license_expiry, distribution_allowed, offline_storage_allowed,
                 projection_allowed, api_allowed, commercial_allowed, ai_processing_allowed,
                 llm_prompt_allowed, training_allowed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                     ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28)
             ON CONFLICT(id) DO UPDATE SET
                content_type = excluded.content_type,
                name = excluded.name,
                version = excluded.version,
                language = excluded.language,
                source = excluded.source,
                publisher = excluded.publisher,
                copyright = excluded.copyright,
                license = excluded.license,
                distribution = excluded.distribution,
                imported_at = excluded.imported_at,
                checksum = excluded.checksum,
                status = excluded.status,
                licensing_status = excluded.licensing_status,
                rights_holder = excluded.rights_holder,
                source_provider = excluded.source_provider,
                source_url = excluded.source_url,
                attribution_text = excluded.attribution_text,
                license_start = excluded.license_start,
                license_expiry = excluded.license_expiry,
                distribution_allowed = excluded.distribution_allowed,
                offline_storage_allowed = excluded.offline_storage_allowed,
                projection_allowed = excluded.projection_allowed,
                api_allowed = excluded.api_allowed,
                commercial_allowed = excluded.commercial_allowed,
                ai_processing_allowed = excluded.ai_processing_allowed,
                llm_prompt_allowed = excluded.llm_prompt_allowed,
                training_allowed = excluded.training_allowed",
            params![
                metadata.id,
                content_type_str(metadata.content_type),
                metadata.name,
                metadata.version,
                metadata.language,
                metadata.source,
                metadata.publisher,
                metadata.copyright,
                metadata.license,
                metadata.distribution,
                metadata.imported_at.to_rfc3339(),
                metadata.checksum,
                status_str(metadata.status),
                licensing_status_str(metadata.licensing_status),
                metadata.usage.rights_holder,
                metadata.usage.source_provider,
                metadata.usage.source_url,
                metadata.usage.attribution_text,
                metadata.usage.license_start.map(|dt| dt.to_rfc3339()),
                metadata.usage.license_expiry.map(|dt| dt.to_rfc3339()),
                metadata.usage.distribution_allowed,
                metadata.usage.offline_storage_allowed,
                metadata.usage.projection_allowed,
                metadata.usage.api_allowed,
                metadata.usage.commercial_allowed,
                metadata.usage.ai_processing_allowed,
                metadata.usage.llm_prompt_allowed,
                metadata.usage.training_allowed,
            ],
        )
        .map_err(|e| ContentRegistryError::Storage(e.to_string()))?;
        Ok(())
    }

    fn set_enabled(&self, content_id: &str, enabled: bool) -> Result<(), ContentRegistryError> {
        let conn = self
            .conn
            .lock()
            .expect("content registry connection poisoned");
        let changed = conn
            .execute(
                "UPDATE content_registry SET status = ?1 WHERE id = ?2",
                params![
                    status_str(if enabled {
                        ContentStatus::Enabled
                    } else {
                        ContentStatus::Disabled
                    }),
                    content_id
                ],
            )
            .map_err(|e| ContentRegistryError::Storage(e.to_string()))?;
        if changed == 0 {
            return Err(ContentRegistryError::NotFound(content_id.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_content::ContentType;

    fn migrated_registry() -> SqliteContentRegistry {
        let mut conn = cip_database::open_in_memory().unwrap();
        cip_database::run_migrations(&mut conn).unwrap();
        SqliteContentRegistry::new(conn)
    }

    fn unknown_kjv() -> ContentMetadata {
        ContentMetadata {
            id: "bible:KJV".to_string(),
            content_type: ContentType::Bible,
            name: "King James Version".to_string(),
            version: "1.0".to_string(),
            language: "en".to_string(),
            source: "development fixture".to_string(),
            publisher: None,
            copyright: None,
            license: None,
            distribution: None,
            imported_at: Utc::now(),
            checksum: None,
            status: ContentStatus::Enabled,
            licensing_status: LicensingStatus::Unknown,
            usage: cip_core_content::UsagePermissions::default(),
        }
    }

    #[test]
    fn usage_permissions_round_trip_through_sqlite_including_unset_fields() {
        let registry = migrated_registry();
        let mut metadata = unknown_kjv();
        metadata.id = "bible:BSB".to_string();
        metadata.usage = cip_core_content::UsagePermissions {
            rights_holder: Some("Public Domain (CC0 1.0)".to_string()),
            source_provider: Some("public domain dataset".to_string()),
            source_url: Some("https://github.com/lyteword/bsb".to_string()),
            attribution_text: None,
            license_start: None,
            license_expiry: None,
            distribution_allowed: Some(true),
            offline_storage_allowed: Some(true),
            projection_allowed: Some(true),
            api_allowed: Some(true),
            commercial_allowed: Some(true),
            ai_processing_allowed: Some(true),
            llm_prompt_allowed: Some(true),
            training_allowed: Some(false),
        };
        registry.register(&metadata).unwrap();

        let loaded = registry.get("bible:BSB").unwrap().unwrap();
        assert_eq!(
            loaded.usage.rights_holder.as_deref(),
            Some("Public Domain (CC0 1.0)")
        );
        assert!(loaded.usage.permits_ai_processing());
        assert!(loaded.usage.permits_distribution());
        assert_eq!(loaded.usage.training_allowed, Some(false));
        assert!(!loaded.usage.permits_training());
        assert_eq!(loaded.usage.attribution_text, None);
    }

    #[test]
    fn usage_permissions_default_to_unknown_when_never_set() {
        let registry = migrated_registry();
        registry.register(&unknown_kjv()).unwrap();

        let loaded = registry.get("bible:KJV").unwrap().unwrap();
        assert_eq!(loaded.usage.ai_processing_allowed, None);
        assert!(!loaded.usage.permits_ai_processing());
    }

    #[test]
    fn registers_and_round_trips_metadata_with_unknown_fields_preserved() {
        let registry = migrated_registry();
        registry.register(&unknown_kjv()).unwrap();

        let loaded = registry.get("bible:KJV").unwrap().unwrap();
        assert_eq!(loaded.name, "King James Version");
        assert_eq!(loaded.publisher, None);
        assert_eq!(loaded.status, ContentStatus::Enabled);
        assert_eq!(loaded.licensing_status, LicensingStatus::Unknown);
    }

    #[test]
    fn licensing_status_round_trips_through_every_variant() {
        let registry = migrated_registry();
        let variants = [
            LicensingStatus::VerifiedPublicDomain,
            LicensingStatus::VerifiedRedistributable,
            LicensingStatus::LicensedForCip,
            LicensingStatus::Unknown,
            LicensingStatus::Restricted,
        ];
        for (i, variant) in variants.iter().enumerate() {
            let mut metadata = unknown_kjv();
            metadata.id = format!("bible:TEST{i}");
            metadata.licensing_status = *variant;
            registry.register(&metadata).unwrap();
            let loaded = registry.get(&metadata.id).unwrap().unwrap();
            assert_eq!(loaded.licensing_status, *variant);
        }
    }

    #[test]
    fn register_is_an_upsert_not_a_duplicate_row() {
        let registry = migrated_registry();
        registry.register(&unknown_kjv()).unwrap();

        let mut updated = unknown_kjv();
        updated.version = "1.1".to_string();
        updated.checksum = Some("abc123".to_string());
        registry.register(&updated).unwrap();

        let all = registry.list(None).unwrap();
        assert_eq!(
            all.len(),
            1,
            "re-registering the same id must not duplicate rows"
        );
        assert_eq!(all[0].version, "1.1");
        assert_eq!(all[0].checksum.as_deref(), Some("abc123"));
    }

    #[test]
    fn rejects_metadata_with_an_empty_id() {
        let registry = migrated_registry();
        let mut bad = unknown_kjv();
        bad.id = String::new();
        assert!(matches!(
            registry.register(&bad),
            Err(ContentRegistryError::InvalidMetadata(_))
        ));
    }

    #[test]
    fn list_filters_by_content_type_and_get_returns_none_for_unknown() {
        let registry = migrated_registry();
        registry.register(&unknown_kjv()).unwrap();

        assert_eq!(registry.list(Some(ContentType::Bible)).unwrap().len(), 1);
        assert_eq!(registry.list(Some(ContentType::Music)).unwrap().len(), 0);
        assert!(registry.get("bible:NIV").unwrap().is_none());
    }

    #[test]
    fn set_enabled_toggles_status_and_disabled_content_is_never_deleted() {
        let registry = migrated_registry();
        registry.register(&unknown_kjv()).unwrap();

        registry.set_enabled("bible:KJV", false).unwrap();
        let loaded = registry.get("bible:KJV").unwrap().unwrap();
        assert_eq!(loaded.status, ContentStatus::Disabled);
        assert_eq!(
            loaded.name, "King James Version",
            "content itself is untouched"
        );
    }

    #[test]
    fn set_enabled_reports_not_found_for_an_unregistered_id() {
        let registry = migrated_registry();
        assert!(matches!(
            registry.set_enabled("bible:NIV", true),
            Err(ContentRegistryError::NotFound(_))
        ));
    }
}
