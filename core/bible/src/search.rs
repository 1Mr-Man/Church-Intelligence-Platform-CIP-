//! Local Bible search (Phase 1.5) - the minimum `SearchEngine`-adjacent
//! implementation `core/search::SearchEngine` leaves as a contract for
//! each domain to fill in for itself (see that crate's docs). This is the
//! Bible domain's fill-in: translation-aware, entirely offline (built only
//! on `BibleProvider` + this crate's own reference detection - no network,
//! no new search infrastructure), dispatching a typed query to whichever
//! lookup actually answers it:
//!
//! - `"Romans 8:28"`     -> a single verse (`get_verse`)
//! - `"Romans 8:28-31"`  -> a verse range (`get_verse_range`)
//! - `"Romans 8"`        -> a whole chapter (`get_chapter`)
//! - anything else       -> free-text search (`BibleProvider::search`)
//!
//! A query is only ever treated as a reference if it parses as *one*,
//! covering the whole (normalized) input - `"tell me about Romans 8:28"`
//! falls through to free text rather than silently discarding the rest of
//! the sentence, since guessing which part of a longer query was "the
//! reference" is exactly the kind of guess this system avoids elsewhere.

use crate::book_alias::canonicalize_book;
use crate::detection::{detect_candidates, ReferenceKind};
use crate::normalize::normalize_text;
use crate::provider::{BibleProvider, BibleProviderError};
use crate::range::{get_verse_range, VerseRangeError};
use crate::reference::ScriptureReference;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use thiserror::Error;

/// One Bible search result - enough for the UI to render and act on
/// without touching a raw database row (section 18).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BibleSearchResult {
    pub translation_id: String,
    pub book: String,
    pub chapter: u32,
    pub verse: u32,
    /// Display form, e.g. `"ROM 8:28"` - matches `ScriptureReference`'s
    /// own `Display` impl.
    pub reference: String,
    pub text: String,
    /// `Some(1.0)` for an exact reference/chapter match (there is nothing
    /// to rank), `None` for a free-text match - `BibleProvider::search`'s
    /// `LIKE`-based lookup has no real ranking signal, so this is left
    /// honestly absent rather than a fabricated score.
    pub relevance: Option<f32>,
}

#[derive(Debug, Error)]
pub enum BibleSearchError {
    #[error("search query was empty")]
    EmptyQuery,
    #[error("invalid verse range: {start}-{end} (start must not be after end)")]
    InvalidRange { start: u32, end: u32 },
    #[error("bible provider error: {0}")]
    Provider(String),
}

impl From<BibleProviderError> for BibleSearchError {
    fn from(e: BibleProviderError) -> Self {
        BibleSearchError::Provider(e.to_string())
    }
}

impl From<VerseRangeError> for BibleSearchError {
    fn from(e: VerseRangeError) -> Self {
        match e {
            VerseRangeError::InvalidRange { start, end } => {
                BibleSearchError::InvalidRange { start, end }
            }
            other => BibleSearchError::Provider(other.to_string()),
        }
    }
}

fn to_result(
    translation_id: &str,
    reference: &ScriptureReference,
    text: String,
) -> BibleSearchResult {
    BibleSearchResult {
        translation_id: translation_id.to_string(),
        book: reference.book.clone(),
        chapter: reference.chapter,
        verse: reference.verse_start,
        reference: reference.to_string(),
        text,
        relevance: Some(1.0),
    }
}

// `"Romans 8:28-31"` - a book, chapter, and inclusive verse range. Tried
// before whole-query reference detection since `detect_candidates` has no
// range shape at all (it's tuned for spoken text, where ranges aren't
// said this way).
static RANGE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(.+?)\s+(\d{1,3})\s*:\s*(\d{1,3})\s*-\s*(\d{1,3})$").unwrap()
});

/// Dispatches `query` per the module docs. `translation_id` scopes every
/// lookup - a search never mixes text from a different translation into
/// the result set (section 17).
pub fn search_bible(
    provider: &dyn BibleProvider,
    translation_id: &str,
    query: &str,
) -> Result<Vec<BibleSearchResult>, BibleSearchError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(BibleSearchError::EmptyQuery);
    }

    if let Some(captures) = RANGE_PATTERN.captures(trimmed) {
        if let Some(book) = canonicalize_book(&captures[1]) {
            let chapter: u32 = captures[2].parse().unwrap_or(0);
            let verse_start: u32 = captures[3].parse().unwrap_or(0);
            let verse_end: u32 = captures[4].parse().unwrap_or(0);
            let verses = get_verse_range(
                provider,
                translation_id,
                book.code,
                chapter,
                verse_start,
                verse_end,
            )?;
            return Ok(verses
                .into_iter()
                .map(|v| to_result(translation_id, &v.reference, v.text))
                .collect());
        }
    }

    let normalized = normalize_text(trimmed);
    let candidates = detect_candidates(&normalized);
    if candidates.len() == 1
        && candidates[0]
            .raw_text
            .trim()
            .eq_ignore_ascii_case(&normalized)
    {
        let candidate = &candidates[0];
        match candidate.kind {
            ReferenceKind::Direct => {
                if let (Some(book), Some(chapter), Some(verse)) = (
                    candidate.partial.book.as_deref(),
                    candidate.partial.chapter,
                    candidate.partial.verse_start,
                ) {
                    let reference =
                        ScriptureReference::single(translation_id, book, chapter, verse);
                    if let Some(verse) = provider.get_verse(&reference)? {
                        return Ok(vec![to_result(
                            translation_id,
                            &verse.reference,
                            verse.text,
                        )]);
                    }
                    return Ok(Vec::new());
                }
            }
            ReferenceKind::Chapter => {
                if let (Some(book), Some(chapter)) =
                    (candidate.partial.book.as_deref(), candidate.partial.chapter)
                {
                    if let Some(chapter_data) =
                        provider.get_chapter(translation_id, book, chapter)?
                    {
                        return Ok(chapter_data
                            .verses
                            .into_iter()
                            .map(|v| to_result(translation_id, &v.reference, v.text))
                            .collect());
                    }
                    return Ok(Vec::new());
                }
            }
            _ => {}
        }
    }

    let matches = provider.search(trimmed, translation_id)?;
    Ok(matches
        .into_iter()
        .map(|v| BibleSearchResult {
            translation_id: translation_id.to_string(),
            book: v.reference.book.clone(),
            chapter: v.reference.chapter,
            verse: v.reference.verse_start,
            reference: v.reference.to_string(),
            text: v.text,
            relevance: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::FakeBibleProvider;

    #[test]
    fn empty_query_is_rejected() {
        let provider = FakeBibleProvider::kjv_fixture();
        assert!(matches!(
            search_bible(&provider, "KJV", "   "),
            Err(BibleSearchError::EmptyQuery)
        ));
    }

    #[test]
    fn a_direct_reference_query_returns_exactly_that_verse() {
        let provider = FakeBibleProvider::kjv_fixture();
        let results = search_bible(&provider, "KJV", "Romans 8:28").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].reference, "ROM 8:28");
        assert_eq!(results[0].relevance, Some(1.0));
        assert!(results[0]
            .text
            .contains("all things work together for good"));
    }

    #[test]
    fn a_chapter_query_returns_every_verse_in_canonical_order() {
        let provider = FakeBibleProvider::kjv_fixture();
        let results = search_bible(&provider, "KJV", "Romans 8").unwrap();
        let verses: Vec<u32> = results.iter().map(|r| r.verse).collect();
        assert_eq!(verses, vec![18, 28, 29, 30, 31]);
    }

    #[test]
    fn a_range_query_returns_the_requested_verses_only() {
        let provider = FakeBibleProvider::kjv_fixture();
        let results = search_bible(&provider, "KJV", "Romans 8:28-31").unwrap();
        let verses: Vec<u32> = results.iter().map(|r| r.verse).collect();
        assert_eq!(verses, vec![28, 29, 30, 31]);
    }

    #[test]
    fn an_inverted_range_query_is_rejected_explicitly() {
        let provider = FakeBibleProvider::kjv_fixture();
        let err = search_bible(&provider, "KJV", "Romans 8:31-28").unwrap_err();
        assert!(matches!(err, BibleSearchError::InvalidRange { .. }));
    }

    #[test]
    fn free_text_query_falls_back_to_provider_search() {
        let provider = FakeBibleProvider::kjv_fixture();
        let results = search_bible(&provider, "KJV", "all things work together").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].reference, "ROM 8:28");
        assert_eq!(
            results[0].relevance, None,
            "free text has no fabricated ranking"
        );
    }

    #[test]
    fn a_reference_embedded_in_a_longer_sentence_is_treated_as_free_text() {
        // Deliberately does not guess which part of a longer query was
        // "the reference" - falls through to text search, which correctly
        // finds nothing for this nonsense phrase in the fixture.
        let provider = FakeBibleProvider::kjv_fixture();
        let results = search_bible(&provider, "KJV", "tell me about Romans 8:28 please").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_is_scoped_to_the_requested_translation_only() {
        let provider = FakeBibleProvider::kjv_fixture();
        // A second translation with different text for the same verse -
        // a search scoped to KJV must never surface it.
        let niv_entries = FakeBibleProvider::new("NIV", &[("ROM", 8, 28, "a different rendering")]);
        let kjv_results = search_bible(&provider, "KJV", "Romans 8:28").unwrap();
        assert_eq!(kjv_results[0].translation_id, "KJV");
        assert!(kjv_results[0]
            .text
            .contains("all things work together for good"));

        let niv_results = search_bible(&niv_entries, "NIV", "Romans 8:28").unwrap();
        assert_eq!(niv_results[0].translation_id, "NIV");
        assert_eq!(niv_results[0].text, "a different rendering");
    }

    #[test]
    fn requesting_an_unavailable_translation_finds_nothing_rather_than_falling_back() {
        let provider = FakeBibleProvider::kjv_fixture();
        let results = search_bible(&provider, "NIV", "Romans 8:28").unwrap();
        assert!(
            results.is_empty(),
            "must not silently substitute KJV for NIV"
        );
    }
}
