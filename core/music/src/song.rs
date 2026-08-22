//! The Song content model - the music-domain equivalent of
//! `core/bible`'s `BibleBook`/`BibleVerse`, but for songs/hymns.

use serde::{Deserialize, Serialize};

use crate::normalize::normalize_for_matching;

/// Which broad category a song belongs to. Kept as a small closed enum,
/// matching how `cip_core_content::ContentType` is grown - a new category
/// is a deliberate new variant, not something a dataset can invent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SongType {
    Hymn,
    WorshipSong,
    Chorus,
    GospelSong,
    Psalm,
    Anthem,
    SpiritualSong,
    Other,
}

/// Whether a song participates in automatic recognition - mirrors
/// `cip_core_content::ContentStatus` but scoped to one song within a
/// dataset (a single mis-imported song can be disabled without disabling
/// its whole dataset). Disabling never deletes the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SongStatus {
    Enabled,
    Disabled,
}

/// One song/hymn, scoped to the dataset it was imported from.
///
/// `content_id` is the Phase 1.5 Content Registry id (e.g.
/// `"music:dev-hymnbook"`) this song's dataset is registered under - the
/// same provenance mechanism Bible content already uses, not a second
/// licensing system. `number` (a hymn/song number) is only ever
/// meaningful *within* `content_id` - two different datasets can both
/// have a song `"120"` that are entirely different songs (Phase 2.1 spec
/// section 57); nothing in this crate ever looks up a number without a
/// dataset.
///
/// Every field describing a real-world fact this crate cannot
/// independently verify (`author`/`composer`) is `Option<String>` -
/// `None` means honestly unknown, never guessed, matching
/// `ContentMetadata`'s discipline for the same reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub content_id: String,
    pub title: String,
    pub normalized_title: String,
    /// Alternate titles and known aliases, normalized for lookup. A
    /// single list rather than two separate fields - a hymnal alternate
    /// title and a colloquial alias serve the exact same lookup role.
    pub aliases: Vec<String>,
    pub song_type: SongType,
    pub language: String,
    pub number: Option<String>,
    pub author: Option<String>,
    pub composer: Option<String>,
    pub status: SongStatus,
}

impl Song {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        content_id: impl Into<String>,
        title: impl Into<String>,
        aliases: Vec<String>,
        song_type: SongType,
        language: impl Into<String>,
        number: Option<String>,
        author: Option<String>,
        composer: Option<String>,
    ) -> Self {
        let title = title.into();
        let normalized_title = normalize_for_matching(&title);
        Self {
            id: id.into(),
            content_id: content_id.into(),
            title,
            normalized_title,
            aliases,
            song_type,
            language: language.into(),
            number,
            author,
            composer,
            status: SongStatus::Enabled,
        }
    }

    /// Whether `needle` (already normalized) matches this song's title or
    /// any registered alias, normalized the same way.
    pub fn matches_title_or_alias(&self, normalized_needle: &str) -> bool {
        self.normalized_title == normalized_needle
            || self
                .aliases
                .iter()
                .any(|alias| normalize_for_matching(alias) == normalized_needle)
    }
}

/// Which structural role a [`SongSection`] plays. Not every song follows
/// the same structure - a song can have zero sections (see
/// `LyricLine.section_id`), one section, or many, in any order a real
/// hymnal/songbook actually uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Verse,
    Chorus,
    Bridge,
    Refrain,
    Stanza,
    Intro,
    Outro,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongSection {
    pub id: String,
    pub song_id: String,
    pub kind: SectionKind,
    /// Order within the song (e.g. verse 1 before the chorus before verse
    /// 2) - not globally unique, only ordered relative to this song's
    /// other sections.
    pub sequence: u32,
}

/// One line of lyric text. `section_id: None` is valid and expected for
/// a song this dataset didn't structure into named sections - the model
/// must not force every song into verse/chorus numbering it doesn't
/// actually have.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    pub song_id: String,
    pub section_id: Option<String>,
    /// Order within the song (or within its section, if it has one) -
    /// used to detect *consecutive* lines for multi-line matching.
    pub sequence: u32,
    pub text: String,
    pub normalized_text: String,
}

impl LyricLine {
    pub fn new(
        song_id: impl Into<String>,
        section_id: Option<String>,
        sequence: u32,
        text: impl Into<String>,
    ) -> Self {
        let text = text.into();
        let normalized_text = normalize_for_matching(&text);
        Self {
            song_id: song_id.into(),
            section_id,
            sequence,
            text,
            normalized_text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_song_derives_a_normalized_title() {
        let song = Song::new(
            "s1",
            "music:dev",
            "Great Is Thy Faithfulness",
            vec![],
            SongType::Hymn,
            "en",
            Some("120".to_string()),
            Some("Thomas Chisholm".to_string()),
            None,
        );
        assert_eq!(song.normalized_title, "great is thy faithfulness");
        assert_eq!(song.status, SongStatus::Enabled);
    }

    #[test]
    fn unknown_author_and_composer_stay_none_not_guessed() {
        let song = Song::new(
            "s1",
            "music:dev",
            "Test Song",
            vec![],
            SongType::Other,
            "en",
            None,
            None,
            None,
        );
        assert_eq!(song.author, None);
        assert_eq!(song.composer, None);
    }

    #[test]
    fn matches_title_or_alias_is_case_and_punctuation_insensitive() {
        let song = Song::new(
            "s1",
            "music:dev",
            "Great Is Thy Faithfulness",
            vec!["Morning by Morning".to_string()],
            SongType::Hymn,
            "en",
            None,
            None,
            None,
        );
        assert!(song.matches_title_or_alias(&normalize_for_matching("GREAT IS THY FAITHFULNESS!")));
        assert!(song.matches_title_or_alias(&normalize_for_matching("morning by morning")));
        assert!(!song.matches_title_or_alias(&normalize_for_matching("Blessed Assurance")));
    }

    #[test]
    fn lyric_line_without_a_section_is_valid() {
        let line = LyricLine::new("s1", None, 0, "A line with no section");
        assert!(line.section_id.is_none());
    }
}
