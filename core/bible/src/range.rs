//! Verse-range retrieval (Phase 1.5) - `"Romans 8:28-31"` -> verses 28
//! through 31, in canonical order.
//!
//! Deliberately a free function over `&dyn BibleProvider` rather than a
//! new `BibleProvider` trait method: it's entirely composable from the
//! existing `get_chapter`, so adding it as a trait method would force
//! every current and future implementation to duplicate the same
//! filter/validate logic for no benefit.

use crate::provider::{BibleProvider, BibleProviderError, BibleVerse};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VerseRangeError {
    /// `verse_start > verse_end` - e.g. "Romans 8:31-28" - reported
    /// explicitly rather than silently reversed or truncated.
    #[error("invalid verse range: {start}-{end} (start must not be after end)")]
    InvalidRange { start: u32, end: u32 },
    #[error("chapter not found: {translation_id} {book} {chapter}")]
    ChapterNotFound {
        translation_id: String,
        book: String,
        chapter: u32,
    },
    #[error("bible provider error: {0}")]
    Provider(String),
}

impl From<BibleProviderError> for VerseRangeError {
    fn from(e: BibleProviderError) -> Self {
        VerseRangeError::Provider(e.to_string())
    }
}

/// Retrieves verses `verse_start..=verse_end` of one book/chapter, in
/// canonical verse order. Rejects an inverted range (`start > end`)
/// explicitly instead of returning an empty or reordered result.
pub fn get_verse_range(
    provider: &dyn BibleProvider,
    translation_id: &str,
    book: &str,
    chapter: u32,
    verse_start: u32,
    verse_end: u32,
) -> Result<Vec<BibleVerse>, VerseRangeError> {
    if verse_start > verse_end {
        return Err(VerseRangeError::InvalidRange {
            start: verse_start,
            end: verse_end,
        });
    }

    let chapter_data = provider
        .get_chapter(translation_id, book, chapter)?
        .ok_or_else(|| VerseRangeError::ChapterNotFound {
            translation_id: translation_id.to_string(),
            book: book.to_string(),
            chapter,
        })?;

    Ok(chapter_data
        .verses
        .into_iter()
        .filter(|v| v.reference.verse_start >= verse_start && v.reference.verse_start <= verse_end)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::FakeBibleProvider;

    #[test]
    fn retrieves_a_verse_range_in_canonical_order() {
        let provider = FakeBibleProvider::kjv_fixture();
        let verses = get_verse_range(&provider, "KJV", "ROM", 8, 28, 31).unwrap();
        let numbers: Vec<u32> = verses.iter().map(|v| v.reference.verse_start).collect();
        assert_eq!(numbers, vec![28, 29, 30, 31]);
    }

    #[test]
    fn a_single_verse_range_returns_one_verse() {
        let provider = FakeBibleProvider::kjv_fixture();
        let verses = get_verse_range(&provider, "KJV", "ROM", 8, 28, 28).unwrap();
        assert_eq!(verses.len(), 1);
        assert_eq!(verses[0].reference.verse_start, 28);
    }

    #[test]
    fn rejects_an_inverted_range_explicitly() {
        let provider = FakeBibleProvider::kjv_fixture();
        let err = get_verse_range(&provider, "KJV", "ROM", 8, 31, 28).unwrap_err();
        assert_eq!(err, VerseRangeError::InvalidRange { start: 31, end: 28 });
    }

    #[test]
    fn reports_chapter_not_found_rather_than_an_empty_vec() {
        let provider = FakeBibleProvider::kjv_fixture();
        let err = get_verse_range(&provider, "KJV", "ROM", 999, 1, 5).unwrap_err();
        assert!(matches!(err, VerseRangeError::ChapterNotFound { .. }));
    }
}
