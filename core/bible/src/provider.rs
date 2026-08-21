use crate::reference::ScriptureReference;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BibleTranslation {
    pub id: String,
    pub name: String,
    pub abbreviation: String,
    pub language: String,
    /// True for translations stored fully in the local database; false for
    /// translations that require a network-backed integration to resolve
    /// (internet-enhanced, never internet-required).
    pub is_local: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BibleBook {
    pub code: String,
    pub name: String,
    pub testament: Testament,
    pub chapter_count: u32,
    pub order: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Testament {
    Old,
    New,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BibleVerse {
    pub reference: ScriptureReference,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BibleChapter {
    pub book: String,
    pub chapter: u32,
    pub verses: Vec<BibleVerse>,
}

#[derive(Debug, Error)]
pub enum BibleProviderError {
    #[error("translation not found: {0}")]
    TranslationNotFound(String),
    #[error("reference not found: {0}")]
    ReferenceNotFound(String),
    #[error("provider unavailable: {0}")]
    Unavailable(String),
    #[error("underlying storage error: {0}")]
    Storage(String),
}

/// The provider/adaptor contract for anything that can serve Bible content.
///
/// Core domain code and the UI depend only on this trait, never on a
/// concrete source. `integrations/bible` supplies implementations (a local
/// SQLite-backed provider first, network-backed providers later) so the
/// Bible engine stays decoupled from both the UI and any single data
/// source, per the approved architecture.
///
/// Implementations MUST be local-first: a provider that requires network
/// access must report itself via `BibleTranslation::is_local = false` and
/// must not be the only provider registered for offline use.
pub trait BibleProvider: Send + Sync {
    fn list_translations(&self) -> Result<Vec<BibleTranslation>, BibleProviderError>;

    fn get_book(
        &self,
        translation_id: &str,
        book_code: &str,
    ) -> Result<Option<BibleBook>, BibleProviderError>;

    fn get_chapter(
        &self,
        translation_id: &str,
        book_code: &str,
        chapter: u32,
    ) -> Result<Option<BibleChapter>, BibleProviderError>;

    fn get_verse(
        &self,
        reference: &ScriptureReference,
    ) -> Result<Option<BibleVerse>, BibleProviderError>;

    /// Free-text search within a single translation. Ranking/relevance is an
    /// implementation detail of the provider (`core/search` composes results
    /// across providers).
    fn search(
        &self,
        query: &str,
        translation_id: &str,
    ) -> Result<Vec<BibleVerse>, BibleProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptyProvider;

    impl BibleProvider for EmptyProvider {
        fn list_translations(&self) -> Result<Vec<BibleTranslation>, BibleProviderError> {
            Ok(vec![])
        }
        fn get_book(&self, _t: &str, _b: &str) -> Result<Option<BibleBook>, BibleProviderError> {
            Ok(None)
        }
        fn get_chapter(
            &self,
            _t: &str,
            _b: &str,
            _c: u32,
        ) -> Result<Option<BibleChapter>, BibleProviderError> {
            Ok(None)
        }
        fn get_verse(
            &self,
            _r: &ScriptureReference,
        ) -> Result<Option<BibleVerse>, BibleProviderError> {
            Ok(None)
        }
        fn search(&self, _q: &str, _t: &str) -> Result<Vec<BibleVerse>, BibleProviderError> {
            Ok(vec![])
        }
    }

    #[test]
    fn a_minimal_provider_satisfies_the_trait_object_contract() {
        let provider: Box<dyn BibleProvider> = Box::new(EmptyProvider);
        assert_eq!(provider.list_translations().unwrap().len(), 0);
    }
}
