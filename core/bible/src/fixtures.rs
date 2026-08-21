//! Test-only fixtures shared across `core/bible`'s own unit tests
//! (`range`, `search`, `integrity`). Not a real `BibleProvider` adapter -
//! those live in `integrations/bible`; this crate's own tests use a
//! simple in-memory fake so they never need a real SQLite connection or a
//! dependency back on `integrations/bible` (which itself depends on this
//! crate - a real adapter here would be a dev-dependency cycle Cargo
//! cannot resolve across two different builds of the same crate).

#![cfg(test)]

use crate::provider::{
    BibleBook, BibleChapter, BibleProvider, BibleProviderError, BibleTranslation, BibleVerse,
    Testament,
};
use crate::reference::ScriptureReference;
use std::collections::BTreeMap;

/// An in-memory `BibleProvider` keyed on (translation, book, chapter,
/// verse). `chapter_count`/`book_order` are set generously since these
/// tests only care about verse content, not full book metadata.
pub struct FakeBibleProvider {
    verses: BTreeMap<(String, String, u32, u32), String>,
    translations: Vec<BibleTranslation>,
}

impl FakeBibleProvider {
    pub fn new(translation_id: &str, entries: &[(&str, u32, u32, &str)]) -> Self {
        let mut verses = BTreeMap::new();
        for (book, chapter, verse, text) in entries {
            verses.insert(
                (
                    translation_id.to_string(),
                    book.to_string(),
                    *chapter,
                    *verse,
                ),
                text.to_string(),
            );
        }
        Self {
            verses,
            translations: vec![BibleTranslation {
                id: translation_id.to_string(),
                name: translation_id.to_string(),
                abbreviation: translation_id.to_string(),
                language: "en".to_string(),
                is_local: true,
            }],
        }
    }

    /// Romans 8:18,28-31 and John 3:16 under `"KJV"` - the standard small
    /// fixture most of this module's tests reuse.
    pub fn kjv_fixture() -> Self {
        Self::new(
            "KJV",
            &[
                ("ROM", 8, 18, "For I reckon that the sufferings..."),
                (
                    "ROM",
                    8,
                    28,
                    "And we know that all things work together for good...",
                ),
                ("ROM", 8, 29, "For whom he did foreknow..."),
                ("ROM", 8, 30, "Moreover whom he did predestinate..."),
                ("ROM", 8, 31, "What shall we then say to these things?"),
                ("JHN", 3, 16, "For God so loved the world..."),
            ],
        )
    }
}

impl BibleProvider for FakeBibleProvider {
    fn list_translations(&self) -> Result<Vec<BibleTranslation>, BibleProviderError> {
        Ok(self.translations.clone())
    }

    fn get_book(
        &self,
        translation_id: &str,
        book_code: &str,
    ) -> Result<Option<BibleBook>, BibleProviderError> {
        let has_book = self
            .verses
            .keys()
            .any(|(t, b, _, _)| t == translation_id && b == book_code);
        // Real `order` (its position in the canonical catalog) rather
        // than a fixed dummy value, so fixture-based tests can exercise
        // `integrity::check_bible_integrity`'s book-ordering check
        // meaningfully.
        Ok(has_book.then(|| BibleBook {
            code: book_code.to_string(),
            name: book_code.to_string(),
            testament: Testament::New,
            chapter_count: 999,
            order: crate::book_alias::BOOKS
                .iter()
                .position(|b| b.code == book_code)
                .map(|i| i as u32)
                .unwrap_or(0),
        }))
    }

    fn get_chapter(
        &self,
        translation_id: &str,
        book_code: &str,
        chapter: u32,
    ) -> Result<Option<BibleChapter>, BibleProviderError> {
        let verses: Vec<BibleVerse> = self
            .verses
            .iter()
            .filter(|((t, b, c, _), _)| t == translation_id && b == book_code && *c == chapter)
            .map(|((_, b, c, v), text)| BibleVerse {
                reference: ScriptureReference::single(translation_id, b, *c, *v),
                text: text.clone(),
            })
            .collect();
        if verses.is_empty() {
            Ok(None)
        } else {
            Ok(Some(BibleChapter {
                book: book_code.to_string(),
                chapter,
                verses,
            }))
        }
    }

    fn get_verse(
        &self,
        reference: &ScriptureReference,
    ) -> Result<Option<BibleVerse>, BibleProviderError> {
        let key = (
            reference.translation_id.clone(),
            reference.book.clone(),
            reference.chapter,
            reference.verse_start,
        );
        Ok(self.verses.get(&key).map(|text| BibleVerse {
            reference: reference.clone(),
            text: text.clone(),
        }))
    }

    fn search(
        &self,
        query: &str,
        translation_id: &str,
    ) -> Result<Vec<BibleVerse>, BibleProviderError> {
        let needle = query.to_lowercase();
        Ok(self
            .verses
            .iter()
            .filter(|((t, _, _, _), text)| {
                t == translation_id && text.to_lowercase().contains(&needle)
            })
            .map(|((_, b, c, v), text)| BibleVerse {
                reference: ScriptureReference::single(translation_id, b, *c, *v),
                text: text.clone(),
            })
            .collect())
    }

    fn list_chapters(
        &self,
        translation_id: &str,
        book_code: &str,
    ) -> Result<Vec<u32>, BibleProviderError> {
        let mut chapters: Vec<u32> = self
            .verses
            .keys()
            .filter(|(t, b, _, _)| t == translation_id && b == book_code)
            .map(|(_, _, c, _)| *c)
            .collect();
        chapters.sort_unstable();
        chapters.dedup();
        Ok(chapters)
    }
}
