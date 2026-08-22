//! A reusable local music dataset importer (Phase 2.1) - the music-domain
//! counterpart to `integrations/bible::importer`. Accepts a structured,
//! already-parsed [`MusicDatasetInput`] - never raw SQL, never a file
//! path this crate reads itself. Validates every record, skips (never
//! silently repairs) anything invalid, and is idempotent: a second
//! import of the same dataset inserts nothing new.
//!
//! ## Never silently overwriting existing content
//!
//! Every insert uses `INSERT OR IGNORE` - a re-import leaves existing
//! rows exactly as they are, even if this dataset's text for them
//! differs, matching `integrations/bible::importer`'s discipline exactly
//! (see `docs/bible-datasets.md`'s rationale, which applies unchanged
//! here).

use std::collections::BTreeSet;

use cip_core_music::{SectionKind, SongType};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{section_kind_to_str, song_type_to_str};

fn normalize(text: &str) -> String {
    cip_core_music::normalize::normalize_for_matching(text)
}

fn parse_song_type_strict(value: &str) -> Option<SongType> {
    Some(match value {
        "hymn" => SongType::Hymn,
        "worship_song" => SongType::WorshipSong,
        "chorus" => SongType::Chorus,
        "gospel_song" => SongType::GospelSong,
        "psalm" => SongType::Psalm,
        "anthem" => SongType::Anthem,
        "spiritual_song" => SongType::SpiritualSong,
        "other" => SongType::Other,
        _ => return None,
    })
}

fn parse_section_kind_strict(value: &str) -> Option<SectionKind> {
    Some(match value {
        "verse" => SectionKind::Verse,
        "chorus" => SectionKind::Chorus,
        "bridge" => SectionKind::Bridge,
        "refrain" => SectionKind::Refrain,
        "stanza" => SectionKind::Stanza,
        "intro" => SectionKind::Intro,
        "outro" => SectionKind::Outro,
        "other" => SectionKind::Other,
        _ => return None,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionInput {
    pub id: String,
    pub kind: String,
    pub sequence: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricInput {
    #[serde(default)]
    pub section_id: Option<String>,
    pub sequence: u32,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongInput {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub song_type: String,
    pub language: String,
    #[serde(default)]
    pub number: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub composer: Option<String>,
    #[serde(default)]
    pub sections: Vec<SectionInput>,
    #[serde(default)]
    pub lyrics: Vec<LyricInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicDatasetInput {
    /// The Content Registry id this dataset will be (or already is)
    /// registered under, e.g. `"music:dev-hymnbook"` - provided by the
    /// caller, following the same `"<type>:<domain-id>"` convention
    /// Phase 1.5 established, never generated here.
    pub content_id: String,
    pub name: String,
    pub language: String,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub copyright: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub distribution: Option<String>,
    pub dataset_version: String,
    pub songs: Vec<SongInput>,
}

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("invalid dataset metadata: {0}")]
    InvalidDatasetMetadata(String),
    #[error("database error: {0}")]
    Database(String),
}

impl From<rusqlite::Error> for ImportError {
    fn from(e: rusqlite::Error) -> Self {
        ImportError::Database(e.to_string())
    }
}

/// A deterministic report of what one import call actually did - every
/// number derived from the dataset and the database's own `changes()`
/// count, never hard-coded (matching `docs/bible-datasets.md` section 8's
/// discipline).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub content_id: String,
    pub dataset_version: String,
    pub songs_total: usize,
    pub songs_imported: usize,
    pub songs_already_present: usize,
    pub songs_invalid: usize,
    pub lyric_lines_imported: usize,
    pub lyric_lines_already_present: usize,
    pub errors: Vec<String>,
    pub checksum: String,
}

fn fnv1a_hash(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Deterministic checksum over the whole dataset's canonical content -
/// content id, dataset version, and every song (sorted by id) with its
/// aliases (sorted), section ordering, and lyric ordering/text (sorted by
/// sequence) - so identical input always produces the same checksum
/// regardless of the order records appeared in the source file.
fn compute_checksum(dataset: &MusicDatasetInput, valid_songs: &[&SongInput]) -> String {
    let mut buf = String::new();
    buf.push_str(&dataset.content_id);
    buf.push('\n');
    buf.push_str(&dataset.dataset_version);
    buf.push('\n');

    let mut songs: Vec<&&SongInput> = valid_songs.iter().collect();
    songs.sort_by(|a, b| a.id.cmp(&b.id));
    for song in songs {
        buf.push_str(&format!("SONG|{}|{}\n", song.id, song.title));
        let mut aliases = song.aliases.clone();
        aliases.sort();
        for alias in &aliases {
            buf.push_str(&format!("ALIAS|{alias}\n"));
        }
        let mut sections = song.sections.clone();
        sections.sort_by_key(|s| s.sequence);
        for section in &sections {
            buf.push_str(&format!("SECTION|{}|{}\n", section.id, section.sequence));
        }
        let mut lyrics = song.lyrics.clone();
        lyrics.sort_by_key(|l| l.sequence);
        for lyric in &lyrics {
            buf.push_str(&format!("LYRIC|{}|{}\n", lyric.sequence, lyric.text));
        }
    }
    format!("{:016x}", fnv1a_hash(buf.as_bytes()))
}

/// Imports `dataset` into `conn`'s `music_songs`/`music_aliases`/
/// `music_sections`/`music_lyrics` tables. Only invalid *dataset-level*
/// metadata is fatal (aborts before touching the database); an invalid
/// individual song/section/lyric is skipped, reported, and never blocks
/// the rest of the import.
pub fn import_music_dataset(
    conn: &Connection,
    dataset: &MusicDatasetInput,
) -> Result<ImportReport, ImportError> {
    if dataset.content_id.trim().is_empty()
        || dataset.name.trim().is_empty()
        || dataset.language.trim().is_empty()
        || dataset.dataset_version.trim().is_empty()
    {
        return Err(ImportError::InvalidDatasetMetadata(
            "content_id, name, language, and dataset_version must all be non-empty".to_string(),
        ));
    }

    let mut errors = Vec::new();
    let mut valid_songs: Vec<&SongInput> = Vec::new();
    let mut seen_song_ids: BTreeSet<&str> = BTreeSet::new();

    for song in &dataset.songs {
        if song.id.trim().is_empty() || song.title.trim().is_empty() {
            errors.push(format!(
                "song {:?}: id and title must both be non-empty",
                song.id
            ));
            continue;
        }
        if parse_song_type_strict(&song.song_type).is_none() {
            errors.push(format!(
                "song {}: unknown song_type {:?}",
                song.id, song.song_type
            ));
            continue;
        }
        if song.language.trim().is_empty() {
            errors.push(format!("song {}: language must not be empty", song.id));
            continue;
        }
        if !seen_song_ids.insert(song.id.as_str()) {
            errors.push(format!(
                "song {}: duplicate song id within this dataset",
                song.id
            ));
            continue;
        }

        let mut section_ids: BTreeSet<&str> = BTreeSet::new();
        let mut sections_valid = true;
        for section in &song.sections {
            if section.id.trim().is_empty() || parse_section_kind_strict(&section.kind).is_none() {
                errors.push(format!(
                    "song {}: invalid section {:?}",
                    song.id, section.id
                ));
                sections_valid = false;
                continue;
            }
            if !section_ids.insert(section.id.as_str()) {
                errors.push(format!(
                    "song {}: duplicate section id {:?}",
                    song.id, section.id
                ));
                sections_valid = false;
            }
        }
        if !sections_valid {
            continue;
        }

        let mut lyric_sequences: BTreeSet<u32> = BTreeSet::new();
        let mut lyrics_valid = true;
        for lyric in &song.lyrics {
            if lyric.text.trim().is_empty() {
                errors.push(format!(
                    "song {} lyric {}: missing text",
                    song.id, lyric.sequence
                ));
                lyrics_valid = false;
                continue;
            }
            if !lyric_sequences.insert(lyric.sequence) {
                errors.push(format!(
                    "song {} lyric {}: duplicate sequence",
                    song.id, lyric.sequence
                ));
                lyrics_valid = false;
                continue;
            }
            if let Some(section_id) = &lyric.section_id {
                if !section_ids.contains(section_id.as_str()) {
                    errors.push(format!(
                        "song {} lyric {}: references unknown section {:?}",
                        song.id, lyric.sequence, section_id
                    ));
                    lyrics_valid = false;
                }
            }
        }
        if !lyrics_valid {
            continue;
        }

        let mut alias_seen: BTreeSet<String> = BTreeSet::new();
        for alias in &song.aliases {
            if !alias_seen.insert(normalize(alias)) {
                errors.push(format!("song {}: duplicate alias {:?}", song.id, alias));
            }
        }

        valid_songs.push(song);
    }

    let checksum = compute_checksum(dataset, &valid_songs);

    let mut songs_imported = 0usize;
    let mut songs_already_present = 0usize;
    let mut lyric_lines_imported = 0usize;
    let mut lyric_lines_already_present = 0usize;

    for song in &valid_songs {
        let changed = conn.execute(
            "INSERT OR IGNORE INTO music_songs
                (id, content_id, title, normalized_title, song_type, language, number, author, composer, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'enabled')",
            params![
                song.id,
                dataset.content_id,
                song.title,
                normalize(&song.title),
                song_type_to_str(parse_song_type_strict(&song.song_type).unwrap()),
                song.language,
                song.number,
                song.author,
                song.composer,
            ],
        )?;
        if changed > 0 {
            songs_imported += 1;
        } else {
            songs_already_present += 1;
        }

        let mut normalized_aliases: BTreeSet<String> = BTreeSet::new();
        for alias in &song.aliases {
            let normalized = normalize(alias);
            if !normalized_aliases.insert(normalized.clone()) {
                continue; // duplicate within this song, already reported above
            }
            conn.execute(
                "INSERT OR IGNORE INTO music_aliases (content_id, song_id, alias, normalized_alias)
                 VALUES (?1, ?2, ?3, ?4)",
                params![dataset.content_id, song.id, alias, normalized],
            )?;
        }

        for section in &song.sections {
            conn.execute(
                "INSERT OR IGNORE INTO music_sections (id, content_id, song_id, kind, sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    section.id,
                    dataset.content_id,
                    song.id,
                    section_kind_to_str(parse_section_kind_strict(&section.kind).unwrap()),
                    section.sequence,
                ],
            )?;
        }

        for lyric in &song.lyrics {
            let changed = conn.execute(
                "INSERT OR IGNORE INTO music_lyrics
                    (content_id, song_id, section_id, sequence, text, normalized_text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    dataset.content_id,
                    song.id,
                    lyric.section_id,
                    lyric.sequence,
                    lyric.text,
                    normalize(&lyric.text),
                ],
            )?;
            if changed > 0 {
                lyric_lines_imported += 1;
            } else {
                lyric_lines_already_present += 1;
            }
        }
    }

    Ok(ImportReport {
        content_id: dataset.content_id.clone(),
        dataset_version: dataset.dataset_version.clone(),
        songs_total: dataset.songs.len(),
        songs_imported,
        songs_already_present,
        songs_invalid: dataset.songs.len() - valid_songs.len(),
        lyric_lines_imported,
        lyric_lines_already_present,
        errors,
        checksum,
    })
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

    fn small_dataset() -> MusicDatasetInput {
        MusicDatasetInput {
            content_id: "music:test".to_string(),
            name: "Test Dataset".to_string(),
            language: "en".to_string(),
            publisher: None,
            copyright: None,
            license: Some("public domain".to_string()),
            distribution: Some("public domain".to_string()),
            dataset_version: "1.0".to_string(),
            songs: vec![SongInput {
                id: "s1".to_string(),
                title: "Test Song".to_string(),
                aliases: vec!["Alt Title".to_string()],
                song_type: "hymn".to_string(),
                language: "en".to_string(),
                number: Some("42".to_string()),
                author: None,
                composer: None,
                sections: vec![SectionInput {
                    id: "v1".to_string(),
                    kind: "verse".to_string(),
                    sequence: 0,
                }],
                lyrics: vec![
                    LyricInput {
                        section_id: Some("v1".to_string()),
                        sequence: 0,
                        text: "First lyric line".to_string(),
                    },
                    LyricInput {
                        section_id: Some("v1".to_string()),
                        sequence: 1,
                        text: "Second lyric line".to_string(),
                    },
                ],
            }],
        }
    }

    #[test]
    fn imports_a_clean_dataset_and_reports_actual_counts() {
        let conn = migrated_conn();
        let report = import_music_dataset(&conn, &small_dataset()).unwrap();

        assert_eq!(report.songs_total, 1);
        assert_eq!(report.songs_imported, 1);
        assert_eq!(report.songs_invalid, 0);
        assert_eq!(report.lyric_lines_imported, 2);
        assert!(report.errors.is_empty());
        assert!(!report.checksum.is_empty());

        let stored: i64 = conn
            .query_row("SELECT count(*) FROM music_songs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, 1);
    }

    #[test]
    fn reimporting_the_identical_dataset_creates_no_duplicate_rows() {
        let conn = migrated_conn();
        import_music_dataset(&conn, &small_dataset()).unwrap();
        let second = import_music_dataset(&conn, &small_dataset()).unwrap();

        assert_eq!(second.songs_imported, 0);
        assert_eq!(second.songs_already_present, 1);
        assert_eq!(second.lyric_lines_imported, 0);
        assert_eq!(second.lyric_lines_already_present, 2);

        let stored: i64 = conn
            .query_row("SELECT count(*) FROM music_songs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, 1, "re-import must never duplicate rows");
        let alias_count: i64 = conn
            .query_row("SELECT count(*) FROM music_aliases", [], |r| r.get(0))
            .unwrap();
        assert_eq!(alias_count, 1, "re-import must never duplicate aliases");
    }

    #[test]
    fn rejects_a_malformed_song_without_aborting_the_rest_of_the_import() {
        let conn = migrated_conn();
        let mut dataset = small_dataset();
        dataset.songs.push(SongInput {
            id: "bad".to_string(),
            title: "Bad Song".to_string(),
            aliases: vec![],
            song_type: "not_a_real_type".to_string(),
            language: "en".to_string(),
            number: None,
            author: None,
            composer: None,
            sections: vec![],
            lyrics: vec![],
        });

        let report = import_music_dataset(&conn, &dataset).unwrap();
        assert_eq!(
            report.songs_imported, 1,
            "the other valid song still imports"
        );
        assert_eq!(report.songs_invalid, 1);
        assert!(report.errors[0].contains("unknown song_type"));
    }

    #[test]
    fn rejects_a_lyric_referencing_an_unknown_section() {
        let conn = migrated_conn();
        let mut dataset = small_dataset();
        dataset.songs[0].lyrics.push(LyricInput {
            section_id: Some("nonexistent-section".to_string()),
            sequence: 2,
            text: "orphaned line".to_string(),
        });

        let report = import_music_dataset(&conn, &dataset).unwrap();
        assert_eq!(report.songs_invalid, 1);
        assert!(report.errors.iter().any(|e| e.contains("unknown section")));
    }

    #[test]
    fn rejects_a_duplicate_song_id_within_the_same_dataset() {
        let conn = migrated_conn();
        let mut dataset = small_dataset();
        let duplicate = dataset.songs[0].clone();
        dataset.songs.push(duplicate);

        let report = import_music_dataset(&conn, &dataset).unwrap();
        assert_eq!(report.songs_imported, 1);
        assert_eq!(report.songs_invalid, 1);
        assert!(report.errors[0].contains("duplicate song id"));
    }

    #[test]
    fn rejects_invalid_dataset_metadata_before_touching_the_database() {
        let conn = migrated_conn();
        let mut dataset = small_dataset();
        dataset.content_id = String::new();

        let err = import_music_dataset(&conn, &dataset).unwrap_err();
        assert!(matches!(err, ImportError::InvalidDatasetMetadata(_)));

        let count: i64 = conn
            .query_row("SELECT count(*) FROM music_songs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn checksum_is_deterministic_and_changes_with_content() {
        let conn = migrated_conn();
        let report_a = import_music_dataset(&conn, &small_dataset()).unwrap();

        let mut different = small_dataset();
        different.content_id = "music:test2".to_string();
        different.songs[0].title = "A Completely Different Title".to_string();
        let report_b = import_music_dataset(&conn, &different).unwrap();

        assert_ne!(report_a.checksum, report_b.checksum);
    }

    #[test]
    fn checksum_is_stable_across_repeated_imports_of_the_same_dataset() {
        let conn = migrated_conn();
        let report_a = import_music_dataset(&conn, &small_dataset()).unwrap();
        let report_b = import_music_dataset(&conn, &small_dataset()).unwrap();
        assert_eq!(report_a.checksum, report_b.checksum);
    }
}
