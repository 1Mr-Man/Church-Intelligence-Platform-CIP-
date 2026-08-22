//! Local SQLite-backed [`MusicProvider`] (Phase 2.1) - the music-domain
//! counterpart to `integrations/bible::SqliteBibleProvider`. Every query
//! is scoped by `content_id`, matching `core/music::MusicProvider`'s
//! contract that a song id/number is never looked up without naming
//! which dataset it means.

use std::sync::Mutex;

use cip_core_music::{
    LyricLine, MusicProvider, MusicProviderError, SectionKind, Song, SongSection, SongStatus,
    SongType,
};
use rusqlite::{params, Connection, OptionalExtension};

pub mod importer;
pub use importer::{
    import_music_dataset, ImportError, ImportReport, LyricInput, MusicDatasetInput, SectionInput,
    SongInput,
};

pub struct SqliteMusicProvider {
    conn: Mutex<Connection>,
}

impl SqliteMusicProvider {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }
}

fn song_type_from_str(value: &str) -> SongType {
    match value {
        "hymn" => SongType::Hymn,
        "worship_song" => SongType::WorshipSong,
        "chorus" => SongType::Chorus,
        "gospel_song" => SongType::GospelSong,
        "psalm" => SongType::Psalm,
        "anthem" => SongType::Anthem,
        "spiritual_song" => SongType::SpiritualSong,
        _ => SongType::Other,
    }
}

pub(crate) fn song_type_to_str(value: SongType) -> &'static str {
    match value {
        SongType::Hymn => "hymn",
        SongType::WorshipSong => "worship_song",
        SongType::Chorus => "chorus",
        SongType::GospelSong => "gospel_song",
        SongType::Psalm => "psalm",
        SongType::Anthem => "anthem",
        SongType::SpiritualSong => "spiritual_song",
        SongType::Other => "other",
    }
}

fn section_kind_from_str(value: &str) -> SectionKind {
    match value {
        "verse" => SectionKind::Verse,
        "chorus" => SectionKind::Chorus,
        "bridge" => SectionKind::Bridge,
        "refrain" => SectionKind::Refrain,
        "stanza" => SectionKind::Stanza,
        "intro" => SectionKind::Intro,
        "outro" => SectionKind::Outro,
        _ => SectionKind::Other,
    }
}

pub(crate) fn section_kind_to_str(value: SectionKind) -> &'static str {
    match value {
        SectionKind::Verse => "verse",
        SectionKind::Chorus => "chorus",
        SectionKind::Bridge => "bridge",
        SectionKind::Refrain => "refrain",
        SectionKind::Stanza => "stanza",
        SectionKind::Intro => "intro",
        SectionKind::Outro => "outro",
        SectionKind::Other => "other",
    }
}

fn status_from_str(value: &str) -> SongStatus {
    match value {
        "disabled" => SongStatus::Disabled,
        _ => SongStatus::Enabled,
    }
}

const SONG_COLUMNS: &str =
    "id, content_id, title, normalized_title, song_type, language, number, author, composer, status";

fn row_to_song(row: &rusqlite::Row<'_>) -> rusqlite::Result<Song> {
    Ok(Song {
        id: row.get(0)?,
        content_id: row.get(1)?,
        title: row.get(2)?,
        normalized_title: row.get(3)?,
        aliases: Vec::new(), // populated separately - see `with_aliases`
        song_type: song_type_from_str(&row.get::<_, String>(4)?),
        language: row.get(5)?,
        number: row.get(6)?,
        author: row.get(7)?,
        composer: row.get(8)?,
        status: status_from_str(&row.get::<_, String>(9)?),
    })
}

impl SqliteMusicProvider {
    fn with_aliases(&self, conn: &Connection, mut song: Song) -> Result<Song, MusicProviderError> {
        let mut stmt = conn
            .prepare("SELECT alias FROM music_aliases WHERE content_id = ?1 AND song_id = ?2 ORDER BY alias")
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        let aliases = stmt
            .query_map(params![song.content_id, song.id], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        song.aliases = aliases;
        Ok(song)
    }
}

impl MusicProvider for SqliteMusicProvider {
    fn list_datasets(&self) -> Result<Vec<String>, MusicProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("music provider connection poisoned");
        let mut stmt = conn
            .prepare("SELECT DISTINCT content_id FROM music_songs ORDER BY content_id")
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| MusicProviderError::Storage(e.to_string()))
    }

    fn get_song(
        &self,
        content_id: &str,
        song_id: &str,
    ) -> Result<Option<Song>, MusicProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("music provider connection poisoned");
        let song = conn
            .query_row(
                &format!(
                    "SELECT {SONG_COLUMNS} FROM music_songs WHERE content_id = ?1 AND id = ?2"
                ),
                params![content_id, song_id],
                row_to_song,
            )
            .optional()
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        song.map(|s| self.with_aliases(&conn, s)).transpose()
    }

    fn search_title(
        &self,
        content_id: &str,
        normalized_title: &str,
    ) -> Result<Vec<Song>, MusicProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("music provider connection poisoned");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {SONG_COLUMNS} FROM music_songs WHERE content_id = ?1 AND normalized_title = ?2"
            ))
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        let songs = stmt
            .query_map(params![content_id, normalized_title], row_to_song)
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        songs
            .into_iter()
            .map(|s| self.with_aliases(&conn, s))
            .collect()
    }

    fn search_alias(
        &self,
        content_id: &str,
        normalized_alias: &str,
    ) -> Result<Vec<Song>, MusicProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("music provider connection poisoned");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {SONG_COLUMNS} FROM music_songs s
                 WHERE content_id = ?1 AND EXISTS (
                     SELECT 1 FROM music_aliases a
                     WHERE a.content_id = s.content_id AND a.song_id = s.id AND a.normalized_alias = ?2
                 )"
            ))
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        let songs = stmt
            .query_map(params![content_id, normalized_alias], row_to_song)
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        songs
            .into_iter()
            .map(|s| self.with_aliases(&conn, s))
            .collect()
    }

    fn search_number(
        &self,
        content_id: &str,
        number: &str,
    ) -> Result<Option<Song>, MusicProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("music provider connection poisoned");
        let song = conn
            .query_row(
                &format!(
                    "SELECT {SONG_COLUMNS} FROM music_songs WHERE content_id = ?1 AND number = ?2"
                ),
                params![content_id, number],
                row_to_song,
            )
            .optional()
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        song.map(|s| self.with_aliases(&conn, s)).transpose()
    }

    fn search_lyrics(
        &self,
        content_id: &str,
        normalized_phrase: &str,
    ) -> Result<Vec<LyricLine>, MusicProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("music provider connection poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT song_id, section_id, sequence, text, normalized_text FROM music_lyrics
                 WHERE content_id = ?1 AND normalized_text LIKE '%' || ?2 || '%'",
            )
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![content_id, normalized_phrase], |row| {
                Ok(LyricLine {
                    song_id: row.get(0)?,
                    section_id: row.get(1)?,
                    sequence: row.get::<_, i64>(2)? as u32,
                    text: row.get(3)?,
                    normalized_text: row.get(4)?,
                })
            })
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| MusicProviderError::Storage(e.to_string()))
    }

    fn get_sections(
        &self,
        content_id: &str,
        song_id: &str,
    ) -> Result<Vec<SongSection>, MusicProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("music provider connection poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT id, song_id, kind, sequence FROM music_sections
                 WHERE content_id = ?1 AND song_id = ?2 ORDER BY sequence",
            )
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![content_id, song_id], |row| {
                Ok(SongSection {
                    id: row.get(0)?,
                    song_id: row.get(1)?,
                    kind: section_kind_from_str(&row.get::<_, String>(2)?),
                    sequence: row.get::<_, i64>(3)? as u32,
                })
            })
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| MusicProviderError::Storage(e.to_string()))
    }

    fn get_lyrics(
        &self,
        content_id: &str,
        song_id: &str,
    ) -> Result<Vec<LyricLine>, MusicProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("music provider connection poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT song_id, section_id, sequence, text, normalized_text FROM music_lyrics
                 WHERE content_id = ?1 AND song_id = ?2 ORDER BY sequence",
            )
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![content_id, song_id], |row| {
                Ok(LyricLine {
                    song_id: row.get(0)?,
                    section_id: row.get(1)?,
                    sequence: row.get::<_, i64>(2)? as u32,
                    text: row.get(3)?,
                    normalized_text: row.get(4)?,
                })
            })
            .map_err(|e| MusicProviderError::Storage(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| MusicProviderError::Storage(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_database::{open_in_memory, run_migrations};

    fn migrated_conn() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn seed_one_song(conn: &Connection) {
        conn.execute(
            "INSERT INTO music_songs (id, content_id, title, normalized_title, song_type, language, number, author, composer, status)
             VALUES ('s1', 'music:test', 'Test Song', 'test song', 'hymn', 'en', '42', 'A. Author', NULL, 'enabled')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO music_aliases (content_id, song_id, alias, normalized_alias)
             VALUES ('music:test', 's1', 'Alt Title', 'alt title')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO music_lyrics (content_id, song_id, section_id, sequence, text, normalized_text)
             VALUES ('music:test', 's1', NULL, 0, 'A line of lyric text', 'a line of lyric text')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn get_song_returns_the_row_with_its_aliases() {
        let conn = migrated_conn();
        seed_one_song(&conn);
        let provider = SqliteMusicProvider::new(conn);

        let song = provider.get_song("music:test", "s1").unwrap().unwrap();
        assert_eq!(song.title, "Test Song");
        assert_eq!(song.aliases, vec!["Alt Title".to_string()]);
        assert_eq!(song.author.as_deref(), Some("A. Author"));
        assert_eq!(song.composer, None);
    }

    #[test]
    fn search_title_finds_by_normalized_title() {
        let conn = migrated_conn();
        seed_one_song(&conn);
        let provider = SqliteMusicProvider::new(conn);
        let songs = provider.search_title("music:test", "test song").unwrap();
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].id, "s1");
    }

    #[test]
    fn search_alias_finds_by_normalized_alias() {
        let conn = migrated_conn();
        seed_one_song(&conn);
        let provider = SqliteMusicProvider::new(conn);
        let songs = provider.search_alias("music:test", "alt title").unwrap();
        assert_eq!(songs[0].id, "s1");
    }

    #[test]
    fn search_number_is_scoped_by_dataset() {
        let conn = migrated_conn();
        seed_one_song(&conn);
        let provider = SqliteMusicProvider::new(conn);
        assert!(provider
            .search_number("music:test", "42")
            .unwrap()
            .is_some());
        assert!(provider
            .search_number("music:other-dataset", "42")
            .unwrap()
            .is_none());
    }

    #[test]
    fn search_lyrics_matches_a_substring() {
        let conn = migrated_conn();
        seed_one_song(&conn);
        let provider = SqliteMusicProvider::new(conn);
        let lines = provider
            .search_lyrics("music:test", "line of lyric")
            .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].song_id, "s1");
    }

    #[test]
    fn missing_song_returns_none_not_an_error() {
        let conn = migrated_conn();
        let provider = SqliteMusicProvider::new(conn);
        assert!(provider
            .get_song("music:test", "nonexistent")
            .unwrap()
            .is_none());
    }

    #[test]
    fn dataset_isolation_prevents_cross_dataset_lookup() {
        let conn = migrated_conn();
        seed_one_song(&conn);
        let provider = SqliteMusicProvider::new(conn);
        assert!(provider
            .get_song("music:other-dataset", "s1")
            .unwrap()
            .is_none());
    }

    #[test]
    fn list_datasets_reports_every_distinct_content_id() {
        let conn = migrated_conn();
        seed_one_song(&conn);
        let provider = SqliteMusicProvider::new(conn);
        assert_eq!(
            provider.list_datasets().unwrap(),
            vec!["music:test".to_string()]
        );
    }
}
