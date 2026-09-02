//! The Berean Standard Bible (BSB) production dataset - CIP's first
//! complete, 66-book, legally-documented Bible translation (the "real
//! Bible dataset" release-readiness milestone). See
//! `docs/bible-production-dataset.md` for the full selection/licensing
//! decision record and `docs/data/bible/BSB/BSB-LICENSE.md` for the
//! evidence chain.
//!
//! ## Why a compiled-in asset, not a runtime download
//!
//! CIP must work fully offline after installation (spec section 33/34):
//! Bible search/detection/presentation/display must never depend on
//! network access, and the application must never fetch Bible text at
//! runtime from Bible Hub, an API, or anywhere else. The dataset is
//! therefore acquired exactly once - during this development session, not
//! at any user's runtime - and checked into the repository as a plain
//! JSON asset (`database/datasets/bsb/bsb.json`, already in this
//! module's `cip_integrations_bible::BibleDatasetInput` shape), then
//! embedded into the compiled binary via `include_str!`. Every launch,
//! in every environment, imports it idempotently (see
//! `content::import_and_register`/`cip_integrations_bible::import_bible_dataset`):
//! the first launch writes it, every later launch finds it all already
//! present and writes nothing.

use cip_integrations_bible::BibleDatasetInput;

const BSB_DATASET_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../database/datasets/bsb/bsb.json"
));

/// The translation id this dataset installs under - the same id used in
/// `content::bible_content_id("BSB")` -> `"bible:BSB"` and passed as
/// `translationId` to any Bible command that wants the real production
/// dataset instead of `DEFAULT_TRANSLATION_ID`'s dev fixture.
pub const BSB_TRANSLATION_ID: &str = "BSB";

/// Parses the embedded dataset asset. Panics on malformed JSON,
/// deliberately: this is compiled-in, checked-in content this repository
/// controls, not user input - a parse failure here means the checked-in
/// asset itself is broken, a build-time defect that must fail loudly and
/// immediately, not a runtime condition any caller could recover from.
pub fn bsb_dataset() -> BibleDatasetInput {
    serde_json::from_str(BSB_DATASET_JSON)
        .expect("bundled BSB dataset asset (database/datasets/bsb/bsb.json) is not valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_asset_parses_and_declares_verified_public_domain_licensing() {
        let dataset = bsb_dataset();
        assert_eq!(dataset.translation.id, "BSB");
        assert_eq!(
            dataset.translation.licensing_status,
            "verified_public_domain"
        );
        assert!(!dataset.verses.is_empty());
    }

    /// Phase 9: BSB is public domain, so every usage permission is
    /// genuinely `true` (except `training_allowed`, deliberately left
    /// unset - CIP does no model training, and the CC0 dedication was
    /// never evaluated against that specific use case). This is the real
    /// evidence that satisfies `commands::ensure_ai_processing_permitted`
    /// for the one translation this codebase actually embeds - proving
    /// the Phase 9 gate is real, not merely passing a synthetic fixture.
    #[test]
    fn the_embedded_asset_declares_real_usage_permissions_including_ai_processing() {
        let dataset = bsb_dataset();
        assert_eq!(
            dataset.translation.usage.rights_holder.as_deref(),
            Some("Public Domain (CC0 1.0)")
        );
        assert!(dataset.translation.usage.permits_ai_processing());
        assert!(dataset.translation.usage.permits_distribution());
        assert!(dataset.translation.usage.permits_offline_storage());
        assert!(dataset.translation.usage.permits_projection());
        assert!(dataset.translation.usage.permits_commercial_use());
    }
}
