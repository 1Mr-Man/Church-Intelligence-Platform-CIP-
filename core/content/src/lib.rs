//! Content Registry domain: a general, source-agnostic answer to "what
//! local content exists?" - Phase 1.5.
//!
//! Every dataset CIP can use locally (a Bible translation today; music,
//! sermon media, or reference material in later phases) gets one
//! [`ContentMetadata`] row, regardless of which domain-specific tables
//! (e.g. `bible_translations`/`bible_verses`) actually hold its content.
//! This lets a future engine ask "what local content exists?" through
//! [`ContentRegistry`] without coupling to any specific domain's schema -
//! mirroring the same provider/adaptor separation `BibleProvider` already
//! established for Bible content specifically (`core/bible`).
//!
//! Phase 1.5 only *populates* the `Bible` category; `Music`/`Service`/
//! `Media`/`Reference` exist here as the closed set future phases will
//! populate, not as implemented engines.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Which conceptual category a piece of content belongs to. A closed enum
/// (not `#[non_exhaustive]`) because Phase 1.5 needs the *shape* of future
/// categories to exist for extensibility, but is not designing an
/// externally-extensible plugin system - a new category is a new variant
/// here, the same way `SearchResultSource` (`core/search`) is grown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Bible,
    Music,
    Service,
    Media,
    Reference,
}

/// Whether a registered content item participates in normal
/// selection/search. Disabling never deletes the underlying content (see
/// module docs on historical traceability) - it only hides it going
/// forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentStatus {
    Enabled,
    Disabled,
}

/// Provenance/licensing metadata for one locally-installed content item.
///
/// Every field that describes a real-world fact CIP cannot independently
/// verify (`publisher`/`copyright`/`license`/`distribution`) is
/// `Option<String>`: `None` means *unknown*, recorded honestly rather than
/// guessed. Nothing in this crate or its callers may invent a value for
/// one of these fields - see `docs/bible-datasets.md`'s licensing section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMetadata {
    /// Stable id, unique across every content item regardless of type.
    /// Convention: `"<type>:<domain-id>"` (e.g. `"bible:KJV"`) - enforced
    /// by convention at the call site, not by this crate, so a future
    /// content type is free to choose its own scheme.
    pub id: String,
    pub content_type: ContentType,
    pub name: String,
    /// A simple, deterministic dataset identity (e.g. `"1.0"`) - not a
    /// migration/versioning system, just "which version of this dataset
    /// is installed."
    pub version: String,
    pub language: String,
    /// Where this content came from (e.g. `"user-provided"`,
    /// `"development fixture"`) - free text, not a licensing claim.
    pub source: String,
    pub publisher: Option<String>,
    pub copyright: Option<String>,
    pub license: Option<String>,
    /// Distribution permission/status (e.g. `"public domain"`,
    /// `"permission granted by publisher"`) - `None` means unknown, never
    /// assumed permissive.
    pub distribution: Option<String>,
    pub imported_at: DateTime<Utc>,
    /// A content-derived hash, where practical, so a re-import of
    /// identical content is detectable. `None` when not computed.
    pub checksum: Option<String>,
    pub status: ContentStatus,
}

#[derive(Debug, Error)]
pub enum ContentRegistryError {
    #[error("content not found: {0}")]
    NotFound(String),
    #[error("invalid content metadata: {0}")]
    InvalidMetadata(String),
    #[error("content registry storage error: {0}")]
    Storage(String),
}

/// The provider/adaptor contract for "what local content exists?" -
/// implementations live in `integrations/*` (a local SQLite-backed one
/// first, per the approved local-first architecture), never in `core`.
pub trait ContentRegistry: Send + Sync {
    /// List registered content, optionally filtered to one [`ContentType`].
    fn list(
        &self,
        content_type: Option<ContentType>,
    ) -> Result<Vec<ContentMetadata>, ContentRegistryError>;

    fn get(&self, content_id: &str) -> Result<Option<ContentMetadata>, ContentRegistryError>;

    /// Register (or update the metadata for) a content item. An upsert on
    /// the metadata row only - this never touches the actual content the
    /// metadata describes (e.g. Bible verse text), which has its own,
    /// separate never-silently-overwritten discipline (see
    /// `docs/bible-datasets.md`).
    fn register(&self, metadata: &ContentMetadata) -> Result<(), ContentRegistryError>;

    fn set_enabled(&self, content_id: &str, enabled: bool) -> Result<(), ContentRegistryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct InMemoryRegistry {
        items: Mutex<HashMap<String, ContentMetadata>>,
    }

    impl InMemoryRegistry {
        fn new() -> Self {
            Self {
                items: Mutex::new(HashMap::new()),
            }
        }
    }

    impl ContentRegistry for InMemoryRegistry {
        fn list(
            &self,
            content_type: Option<ContentType>,
        ) -> Result<Vec<ContentMetadata>, ContentRegistryError> {
            Ok(self
                .items
                .lock()
                .unwrap()
                .values()
                .filter(|m| content_type.map_or(true, |t| m.content_type == t))
                .cloned()
                .collect())
        }

        fn get(&self, content_id: &str) -> Result<Option<ContentMetadata>, ContentRegistryError> {
            Ok(self.items.lock().unwrap().get(content_id).cloned())
        }

        fn register(&self, metadata: &ContentMetadata) -> Result<(), ContentRegistryError> {
            self.items
                .lock()
                .unwrap()
                .insert(metadata.id.clone(), metadata.clone());
            Ok(())
        }

        fn set_enabled(&self, content_id: &str, enabled: bool) -> Result<(), ContentRegistryError> {
            let mut items = self.items.lock().unwrap();
            let item = items
                .get_mut(content_id)
                .ok_or_else(|| ContentRegistryError::NotFound(content_id.to_string()))?;
            item.status = if enabled {
                ContentStatus::Enabled
            } else {
                ContentStatus::Disabled
            };
            Ok(())
        }
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
        }
    }

    #[test]
    fn registers_and_retrieves_content_with_unknown_licensing_left_unknown() {
        let registry = InMemoryRegistry::new();
        registry.register(&unknown_kjv()).unwrap();

        let loaded = registry.get("bible:KJV").unwrap().unwrap();
        assert_eq!(
            loaded.publisher, None,
            "unknown metadata must stay None, never guessed"
        );
        assert_eq!(loaded.license, None);
    }

    #[test]
    fn get_returns_none_for_unregistered_content() {
        let registry = InMemoryRegistry::new();
        assert!(registry.get("bible:NIV").unwrap().is_none());
    }

    #[test]
    fn list_filters_by_content_type() {
        let registry = InMemoryRegistry::new();
        registry.register(&unknown_kjv()).unwrap();

        assert_eq!(registry.list(Some(ContentType::Bible)).unwrap().len(), 1);
        assert_eq!(registry.list(Some(ContentType::Music)).unwrap().len(), 0);
        assert_eq!(registry.list(None).unwrap().len(), 1);
    }

    #[test]
    fn set_enabled_toggles_status_without_deleting_the_item() {
        let registry = InMemoryRegistry::new();
        registry.register(&unknown_kjv()).unwrap();

        registry.set_enabled("bible:KJV", false).unwrap();
        assert_eq!(
            registry.get("bible:KJV").unwrap().unwrap().status,
            ContentStatus::Disabled
        );

        registry.set_enabled("bible:KJV", true).unwrap();
        assert_eq!(
            registry.get("bible:KJV").unwrap().unwrap().status,
            ContentStatus::Enabled
        );
    }

    #[test]
    fn set_enabled_reports_not_found_for_unregistered_content() {
        let registry = InMemoryRegistry::new();
        assert!(matches!(
            registry.set_enabled("bible:NIV", false),
            Err(ContentRegistryError::NotFound(_))
        ));
    }
}
