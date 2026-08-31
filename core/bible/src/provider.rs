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

    /// The chapter numbers actually present for one book, ascending -
    /// e.g. `[3]` for a dev fixture that only has John chapter 3, `1..=21`
    /// for a complete book. Added in Phase 1.5 so a dataset integrity
    /// checker (`crate::integrity`) can enumerate what's actually stored
    /// without guessing at or hard-coding a canonical chapter count -
    /// nothing in `core/bible` invents Bible structure it wasn't given.
    fn list_chapters(
        &self,
        translation_id: &str,
        book_code: &str,
    ) -> Result<Vec<u32>, BibleProviderError>;

    /// Finds verses that share significant vocabulary with `query_text`,
    /// for paraphrase-style Scripture detection (an operator paraphrases a
    /// verse without a formal citation) - see `crate::paraphrase`'s module
    /// docs for exactly what this can and cannot detect (lexical/keyword
    /// overlap, not semantic/neural understanding).
    ///
    /// Default implementation: unions [`search`](Self::search) results for
    /// each of `query_text`'s distinct significant (stemmed, stopword-
    /// filtered) words, so any `BibleProvider` gets this for free from its
    /// existing substring search - no new indexing infrastructure required.
    /// `limit` is an approximate cap on how many candidate verses are
    /// gathered before returning, not a guarantee of the best `limit`
    /// matches - a provider backed by a real database is free to override
    /// this with a faster or better-ranked implementation. Either way,
    /// callers must not assume any particular ordering: the caller (the
    /// paraphrase-scoring pipeline) re-scores every returned verse itself
    /// rather than trusting retrieval order.
    fn find_similar_verses(
        &self,
        translation_id: &str,
        query_text: &str,
        limit: usize,
    ) -> Result<Vec<BibleVerse>, BibleProviderError> {
        let words = crate::paraphrase::significant_words(query_text);
        let cap = limit.max(1).saturating_mul(5);

        let mut searched: Vec<&str> = Vec::new();
        let mut seen_refs: Vec<String> = Vec::new();
        let mut results = Vec::new();

        for word in &words {
            if searched.contains(&word.as_str()) {
                continue;
            }
            searched.push(word.as_str());

            for verse in self.search(word, translation_id)? {
                let key = verse.reference.to_string();
                if !seen_refs.contains(&key) {
                    seen_refs.push(key);
                    results.push(verse);
                }
            }
            if results.len() >= cap {
                break;
            }
        }

        Ok(results)
    }
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
        fn list_chapters(&self, _t: &str, _b: &str) -> Result<Vec<u32>, BibleProviderError> {
            Ok(vec![])
        }
    }

    #[test]
    fn a_minimal_provider_satisfies_the_trait_object_contract() {
        let provider: Box<dyn BibleProvider> = Box::new(EmptyProvider);
        assert_eq!(provider.list_translations().unwrap().len(), 0);
    }

    #[test]
    fn find_similar_verses_default_impl_retrieves_via_significant_words() {
        let provider = crate::fixtures::FakeBibleProvider::kjv_fixture();
        let results = provider
            .find_similar_verses("KJV", "all things work together for good", 5)
            .unwrap();
        assert!(
            results
                .iter()
                .any(|v| v.reference.book == "ROM" && v.reference.verse_start == 28),
            "expected Romans 8:28 among the retrieved candidates: {results:?}"
        );
    }

    #[test]
    fn find_similar_verses_finds_nothing_for_vocabulary_absent_from_the_dataset() {
        let provider = crate::fixtures::FakeBibleProvider::kjv_fixture();
        let results = provider
            .find_similar_verses("KJV", "spreadsheet quarterly revenue forecast", 5)
            .unwrap();
        assert!(results.is_empty());
    }
}
