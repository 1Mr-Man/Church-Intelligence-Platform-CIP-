//! Test-only fixtures shared across this crate's test modules.
//!
//! `cip_core_bible::fixtures::FakeBibleProvider` is `#[cfg(test)]`-gated
//! and private to `core/bible` itself - the same cross-crate
//! dev-dependency limitation documented in `docs/bible-datasets.md`
//! (two incompatible builds of `cip_core_bible` in the dependency graph)
//! applies here too, so this crate keeps its own lightweight in-crate
//! fixture instead, matching the established pattern.

#![cfg(test)]

use std::collections::HashMap;

use cip_core_bible::{
    BibleBook, BibleChapter, BibleProvider, BibleProviderError, BibleTranslation, BibleVerse,
    ScriptureReference, Testament,
};

pub struct FakeBibleProvider {
    verses: HashMap<(String, String, u32, u32), String>,
}

impl FakeBibleProvider {
    pub fn new(entries: &[(&str, u32, u32, &str)]) -> Self {
        let mut verses = HashMap::new();
        for (book, chapter, verse, text) in entries {
            verses.insert(
                ("KJV".to_string(), book.to_string(), *chapter, *verse),
                text.to_string(),
            );
        }
        Self { verses }
    }

    /// Romans 8 (18, 28, 29, 30, 31) and John 3:16 - the same standard
    /// fixture used throughout `core/service::bible_intelligence`'s tests.
    pub fn kjv_fixture() -> Self {
        Self::new(&[
            (
                "ROM",
                8,
                18,
                "For I reckon that the sufferings of this present time...",
            ),
            (
                "ROM",
                8,
                28,
                "And we know that all things work together for good...",
            ),
            (
                "ROM",
                8,
                29,
                "For whom he did foreknow, he also did predestinate...",
            ),
            (
                "ROM",
                8,
                30,
                "Moreover whom he did predestinate, them he also called...",
            ),
            ("ROM", 8, 31, "What shall we then say to these things?..."),
            ("JHN", 3, 16, "For God so loved the world..."),
        ])
    }
}

impl BibleProvider for FakeBibleProvider {
    fn list_translations(&self) -> Result<Vec<BibleTranslation>, BibleProviderError> {
        Ok(vec![])
    }

    fn get_book(
        &self,
        _translation_id: &str,
        book_code: &str,
    ) -> Result<Option<BibleBook>, BibleProviderError> {
        Ok(Some(BibleBook {
            code: book_code.to_string(),
            name: book_code.to_string(),
            testament: Testament::New,
            chapter_count: 999,
            order: 0,
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
        _query: &str,
        _translation_id: &str,
    ) -> Result<Vec<BibleVerse>, BibleProviderError> {
        Ok(vec![])
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
