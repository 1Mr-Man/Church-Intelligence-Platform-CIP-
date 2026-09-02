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

/// What CIP actually knows about a content item's right to be stored and
/// redistributed - the hard safety gate a bulk text importer (the Bible
/// production dataset milestone) checks before writing anything, so a
/// translation with uncertain rights (NIV, ESV, NASB, ...) can never enter
/// the production dataset by accident. Distinct from the free-text
/// `license`/`distribution` fields below, which record *what the source
/// said*; this field records *what CIP has concluded from that*, and only
/// ever moves away from [`LicensingStatus::Unknown`] on deliberate,
/// evidence-backed classification at the call site that registers the
/// content - never inferred, never guessed, never silently upgraded from
/// `Unknown` to a permissive status. See `docs/bible-production-dataset.md`
/// for the evidence standard applied to the first dataset that used this
/// gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LicensingStatus {
    /// Independently verified to be in the public domain (e.g. an
    /// explicit, dated public-domain dedication from the source/publisher).
    VerifiedPublicDomain,
    /// Independently verified to carry an explicit, permissive
    /// redistribution license (e.g. CC0, a stated "free to copy and
    /// distribute" grant) that is not itself a public-domain dedication.
    VerifiedRedistributable,
    /// CIP (or its operator/church) holds an explicit license/agreement
    /// with the rights holder permitting this specific distribution -
    /// reserved for a future real licensing agreement; nothing in this
    /// milestone sets it.
    LicensedForCip,
    /// The default, honest starting point for every content item: no
    /// redistribution determination has been made. A bulk importer MUST
    /// refuse to write production content while this is the status.
    Unknown,
    /// Explicitly known to be under restrictive/unclear copyright that
    /// does not permit CIP's redistribution (e.g. a mainstream commercial
    /// translation with no license on file) - stronger than `Unknown`:
    /// this is a deliberate "never import this" marker, not just "not yet
    /// checked."
    Restricted,
}

impl LicensingStatus {
    /// Whether a bulk importer may write content carrying this status into
    /// a production dataset table. Only a status backed by actual evidence
    /// clears the gate - `Unknown` and `Restricted` never do, regardless of
    /// how confident the caller feels.
    pub fn permits_bulk_import(self) -> bool {
        matches!(
            self,
            LicensingStatus::VerifiedPublicDomain
                | LicensingStatus::VerifiedRedistributable
                | LicensingStatus::LicensedForCip
        )
    }
}

/// Per-use-case licensing permissions for one content item - a finer
/// grain than [`LicensingStatus`], which only governs whether a dataset
/// may be bulk-imported at all. Once a translation has cleared that
/// coarse admission gate, `UsagePermissions` governs what CIP is actually
/// allowed to *do* with it: a real license can permit projection and
/// offline storage while forbidding AI/ML processing, or permit
/// non-commercial use only - distinctions `LicensingStatus` alone cannot
/// express (see `docs/bible-translation-registry.md` and
/// `docs/bible-translation-licensing-roadmap.md`).
///
/// Every field defaults to `None` - "not yet determined" - via
/// `#[derive(Default)]`. `None` must never be treated as permissive by
/// any caller: the `permits_*` helpers below all read `Some(true)` as the
/// only "yes," identical to `LicensingStatus`'s own "never assume
/// permissive" doctrine. `Some(false)` records an explicit denial,
/// distinct from "unknown."
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePermissions {
    /// The actual rights holder/publisher of record (e.g. "Biblica",
    /// "Thomas Nelson / HarperCollins Christian Publishing") - distinct
    /// from `ContentMetadata::publisher`, which may record a distributor
    /// rather than the underlying rights holder.
    pub rights_holder: Option<String>,
    /// Which platform/route this content was (or will be) obtained
    /// through (e.g. "direct publisher license", "API.Bible", "YouVersion
    /// Platform", "public domain dataset").
    pub source_provider: Option<String>,
    pub source_url: Option<String>,
    /// The exact attribution text a license requires CIP to display, if
    /// any - `None` means no requirement is known, never "none required."
    pub attribution_text: Option<String>,
    pub license_start: Option<DateTime<Utc>>,
    pub license_expiry: Option<DateTime<Utc>>,
    pub distribution_allowed: Option<bool>,
    pub offline_storage_allowed: Option<bool>,
    pub projection_allowed: Option<bool>,
    pub api_allowed: Option<bool>,
    pub commercial_allowed: Option<bool>,
    /// Whether this content's text may be processed by any local AI/ML
    /// model (e.g. embedding generation) - the one permission this phase
    /// actually enforces (see `commands::ensure_ai_processing_permitted`).
    pub ai_processing_allowed: Option<bool>,
    /// Whether this content's text may be sent into an LLM prompt
    /// specifically (a stricter sub-case some real licenses distinguish
    /// from AI/ML processing in general, e.g. embedding vs. generative
    /// use) - recorded for future enforcement; nothing in this codebase
    /// sends Bible text into an LLM prompt today.
    pub llm_prompt_allowed: Option<bool>,
    /// Whether this content's text may be used to train or fine-tune a
    /// model - recorded for future enforcement; CIP does no model
    /// training today.
    pub training_allowed: Option<bool>,
}

impl UsagePermissions {
    pub fn permits_distribution(&self) -> bool {
        self.distribution_allowed == Some(true)
    }

    pub fn permits_offline_storage(&self) -> bool {
        self.offline_storage_allowed == Some(true)
    }

    pub fn permits_projection(&self) -> bool {
        self.projection_allowed == Some(true)
    }

    pub fn permits_api(&self) -> bool {
        self.api_allowed == Some(true)
    }

    pub fn permits_commercial_use(&self) -> bool {
        self.commercial_allowed == Some(true)
    }

    pub fn permits_ai_processing(&self) -> bool {
        self.ai_processing_allowed == Some(true)
    }

    pub fn permits_llm_prompt(&self) -> bool {
        self.llm_prompt_allowed == Some(true)
    }

    pub fn permits_training(&self) -> bool {
        self.training_allowed == Some(true)
    }
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
    /// What CIP has independently concluded about this item's right to be
    /// stored/redistributed - see [`LicensingStatus`]'s docs. Defaults to
    /// `Unknown` at every call site that has no real evidence; a bulk
    /// importer gates on this, never on the free-text `license` field
    /// above (which only records what a source *said*, not what CIP has
    /// verified).
    pub licensing_status: LicensingStatus,
    /// Fine-grained, per-use-case licensing permissions - see
    /// [`UsagePermissions`]'s own docs. Defaults to
    /// `UsagePermissions::default()` (every field `None`, i.e. "not yet
    /// determined") at every call site with no real evidence, matching
    /// `licensing_status`'s own honesty discipline.
    #[serde(default)]
    pub usage: UsagePermissions,
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
            licensing_status: LicensingStatus::Unknown,
            usage: UsagePermissions::default(),
        }
    }

    #[test]
    fn usage_permissions_default_to_unknown_and_permit_nothing() {
        let usage = UsagePermissions::default();
        assert!(!usage.permits_distribution());
        assert!(!usage.permits_offline_storage());
        assert!(!usage.permits_projection());
        assert!(!usage.permits_api());
        assert!(!usage.permits_commercial_use());
        assert!(!usage.permits_ai_processing());
        assert!(!usage.permits_llm_prompt());
        assert!(!usage.permits_training());
    }

    #[test]
    fn usage_permissions_explicit_false_is_distinct_from_unknown_but_still_denies() {
        let usage = UsagePermissions {
            ai_processing_allowed: Some(false),
            ..Default::default()
        };
        assert!(!usage.permits_ai_processing());
        assert_eq!(usage.ai_processing_allowed, Some(false));
    }

    #[test]
    fn usage_permissions_only_explicit_true_permits() {
        let usage = UsagePermissions {
            ai_processing_allowed: Some(true),
            offline_storage_allowed: Some(true),
            ..Default::default()
        };
        assert!(usage.permits_ai_processing());
        assert!(usage.permits_offline_storage());
        assert!(!usage.permits_commercial_use());
    }

    #[test]
    fn only_evidence_backed_licensing_statuses_permit_bulk_import() {
        assert!(LicensingStatus::VerifiedPublicDomain.permits_bulk_import());
        assert!(LicensingStatus::VerifiedRedistributable.permits_bulk_import());
        assert!(LicensingStatus::LicensedForCip.permits_bulk_import());
        assert!(!LicensingStatus::Unknown.permits_bulk_import());
        assert!(!LicensingStatus::Restricted.permits_bulk_import());
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
