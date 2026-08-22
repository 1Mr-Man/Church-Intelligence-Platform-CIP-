//! The `MusicProvider` contract - the music-domain equivalent of
//! `core/bible::BibleProvider`. `core` depends only on this trait;
//! `integrations/music` supplies a concrete (SQLite-backed) implementation.
//! Every lookup is explicitly scoped to one `content_id` (a Content
//! Registry dataset id, e.g. `"music:dev-hymnbook"`) - nothing in this
//! trait can look up a song number, title, or alias without naming which
//! dataset it means (Phase 2.1 spec section 57: two datasets can both
//! have a song "120" that are entirely different songs).

use thiserror::Error;

use crate::song::{LyricLine, Song, SongSection};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MusicProviderError {
    #[error("song not found: {0}")]
    MusicNotFound(String),
    #[error("music dataset unavailable: {0}")]
    DatasetUnavailable(String),
    #[error("music provider storage error: {0}")]
    Storage(String),
}

pub trait MusicProvider: Send + Sync {
    /// Every dataset (Content Registry id) this provider currently knows
    /// about - the caller (the matcher/engine) filters this against the
    /// Content Registry's enabled/disabled status before searching, so
    /// this trait itself does not need to know about enablement.
    fn list_datasets(&self) -> Result<Vec<String>, MusicProviderError>;

    fn get_song(&self, content_id: &str, song_id: &str)
        -> Result<Option<Song>, MusicProviderError>;

    fn search_title(
        &self,
        content_id: &str,
        normalized_title: &str,
    ) -> Result<Vec<Song>, MusicProviderError>;

    fn search_alias(
        &self,
        content_id: &str,
        normalized_alias: &str,
    ) -> Result<Vec<Song>, MusicProviderError>;

    fn search_number(
        &self,
        content_id: &str,
        number: &str,
    ) -> Result<Option<Song>, MusicProviderError>;

    /// Lines (from any song in this dataset) whose normalized text
    /// contains `normalized_phrase` as a substring. Returned lines carry
    /// enough (`song_id`/`section_id`/`sequence`) for the matcher to
    /// detect consecutive-line matches without a second lookup.
    fn search_lyrics(
        &self,
        content_id: &str,
        normalized_phrase: &str,
    ) -> Result<Vec<LyricLine>, MusicProviderError>;

    fn get_sections(
        &self,
        content_id: &str,
        song_id: &str,
    ) -> Result<Vec<SongSection>, MusicProviderError>;

    fn get_lyrics(
        &self,
        content_id: &str,
        song_id: &str,
    ) -> Result<Vec<LyricLine>, MusicProviderError>;
}
