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
use cip_core_music::{LyricLine, MusicProvider, MusicProviderError, Song, SongSection, SongType};

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

/// A minimal in-memory `MusicProvider`, matching the same
/// crate-local-fixture pattern as `FakeBibleProvider` above -
/// `cip_core_music::fixtures::FakeMusicProvider` is `#[cfg(test)]`-gated
/// and private to `core/music` itself, so this crate keeps its own.
#[derive(Default)]
pub struct FakeMusicProvider {
    songs: HashMap<(String, String), Song>,
    lyrics: HashMap<(String, String), Vec<LyricLine>>,
}

impl FakeMusicProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_song(&mut self, song: Song) {
        self.songs
            .insert((song.content_id.clone(), song.id.clone()), song);
    }

    pub fn add_lyrics(&mut self, content_id: &str, song_id: &str, lines: Vec<LyricLine>) {
        self.lyrics
            .insert((content_id.to_string(), song_id.to_string()), lines);
    }

    /// One dataset (`"music:test-hymnbook"`) with two songs: "Test Hymn
    /// One" (number "120", alias "First Test Hymn", a distinctive
    /// two-line lyric) and "Test Hymn Two" (number "121", unrelated
    /// lyrics) - the same shape used throughout `core/music`'s own
    /// matcher tests.
    pub fn hymnbook_fixture() -> Self {
        let mut provider = Self::new();
        let hymn_one = Song::new(
            "h1",
            "music:test-hymnbook",
            "Test Hymn One",
            vec!["First Test Hymn".to_string()],
            SongType::Hymn,
            "en",
            Some("120".to_string()),
            None,
            None,
        );
        provider.add_song(hymn_one);
        provider.add_lyrics(
            "music:test-hymnbook",
            "h1",
            vec![
                LyricLine::new("h1", None, 0, "Great is thy faithfulness my Father"),
                LyricLine::new("h1", None, 1, "Morning by morning new mercies I see"),
            ],
        );

        let hymn_two = Song::new(
            "h2",
            "music:test-hymnbook",
            "Test Hymn Two",
            vec![],
            SongType::Hymn,
            "en",
            Some("121".to_string()),
            None,
            None,
        );
        provider.add_song(hymn_two);
        provider.add_lyrics(
            "music:test-hymnbook",
            "h2",
            vec![LyricLine::new(
                "h2",
                None,
                0,
                "A completely unrelated lyric line",
            )],
        );

        provider
    }
}

impl MusicProvider for FakeMusicProvider {
    fn list_datasets(&self) -> Result<Vec<String>, MusicProviderError> {
        let mut ids: Vec<String> = self.songs.keys().map(|(c, _)| c.clone()).collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    fn get_song(
        &self,
        content_id: &str,
        song_id: &str,
    ) -> Result<Option<Song>, MusicProviderError> {
        Ok(self
            .songs
            .get(&(content_id.to_string(), song_id.to_string()))
            .cloned())
    }

    fn search_title(
        &self,
        content_id: &str,
        normalized_title: &str,
    ) -> Result<Vec<Song>, MusicProviderError> {
        Ok(self
            .songs
            .values()
            .filter(|s| s.content_id == content_id && s.normalized_title == normalized_title)
            .cloned()
            .collect())
    }

    fn search_alias(
        &self,
        content_id: &str,
        normalized_alias: &str,
    ) -> Result<Vec<Song>, MusicProviderError> {
        Ok(self
            .songs
            .values()
            .filter(|s| {
                s.content_id == content_id
                    && s.aliases.iter().any(|a| {
                        cip_core_music::normalize::normalize_for_matching(a) == normalized_alias
                    })
            })
            .cloned()
            .collect())
    }

    fn search_number(
        &self,
        content_id: &str,
        number: &str,
    ) -> Result<Option<Song>, MusicProviderError> {
        Ok(self
            .songs
            .values()
            .find(|s| s.content_id == content_id && s.number.as_deref() == Some(number))
            .cloned())
    }

    fn search_lyrics(
        &self,
        content_id: &str,
        normalized_phrase: &str,
    ) -> Result<Vec<LyricLine>, MusicProviderError> {
        let mut out = Vec::new();
        for ((cid, _), lines) in &self.lyrics {
            if cid != content_id {
                continue;
            }
            for line in lines {
                if line.normalized_text.contains(normalized_phrase) {
                    out.push(line.clone());
                }
            }
        }
        Ok(out)
    }

    fn get_sections(
        &self,
        _content_id: &str,
        _song_id: &str,
    ) -> Result<Vec<SongSection>, MusicProviderError> {
        Ok(Vec::new())
    }

    fn get_lyrics(
        &self,
        content_id: &str,
        song_id: &str,
    ) -> Result<Vec<LyricLine>, MusicProviderError> {
        Ok(self
            .lyrics
            .get(&(content_id.to_string(), song_id.to_string()))
            .cloned()
            .unwrap_or_default())
    }
}
