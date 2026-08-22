//! Test-only in-memory `MusicProvider`, matching the established pattern
//! (`core/bible::fixtures::FakeBibleProvider`) - a crate-local fixture
//! rather than a dev-dependency, avoiding the two-incompatible-builds
//! problem documented in `docs/bible-datasets.md`.

#![cfg(test)]

use std::collections::HashMap;

use crate::normalize::normalize_for_matching;
use crate::provider::{MusicProvider, MusicProviderError};
use crate::song::{LyricLine, Song, SongSection};

#[derive(Default)]
pub struct FakeMusicProvider {
    songs: HashMap<(String, String), Song>, // (content_id, song_id) -> Song
    sections: HashMap<(String, String), Vec<SongSection>>,
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

    pub fn add_sections(&mut self, content_id: &str, song_id: &str, sections: Vec<SongSection>) {
        self.sections
            .insert((content_id.to_string(), song_id.to_string()), sections);
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
                    && s.aliases
                        .iter()
                        .any(|a| normalize_for_matching(a) == normalized_alias)
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
        content_id: &str,
        song_id: &str,
    ) -> Result<Vec<SongSection>, MusicProviderError> {
        Ok(self
            .sections
            .get(&(content_id.to_string(), song_id.to_string()))
            .cloned()
            .unwrap_or_default())
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
