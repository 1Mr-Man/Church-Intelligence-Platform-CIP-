//! A reusable local Bible dataset importer (Phase 1.5).
//!
//! Accepts a structured, already-parsed [`BibleDatasetInput`] - **never**
//! raw SQL, and never a file path this crate reads itself (see
//! `docs/bible-datasets.md`'s security note: the desktop app reads the
//! file client-side and hands this crate already-parsed JSON, so nothing
//! here ever trusts an arbitrary filesystem path). Validates every row,
//! skips (never silently repairs) anything invalid, and is idempotent: a
//! second import of the same dataset inserts nothing new.
//!
//! ## Never silently overwriting existing content
//!
//! Every insert uses `INSERT OR IGNORE` - if a `(translation, book,
//! chapter, verse)` row already exists, the existing text is left exactly
//! as it is, even if this dataset's text for that verse differs. A
//! changed dataset (different checksum) still only *adds* what's
//! missing; the caller decides what to do about pre-existing content that
//! conflicts (see the module docs' rationale in `docs/bible-datasets.md`).

use cip_core_bible::book_alias::canonicalize_book;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationInput {
    pub id: String,
    pub name: String,
    pub abbreviation: String,
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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerseInput {
    /// A book code, name, or known alias (e.g. `"ROM"`, `"Romans"`,
    /// `"Rom."`) - canonicalized against the same 66-book catalog
    /// `core/bible::book_alias` uses everywhere else, so the importer
    /// never introduces a second, conflicting book list.
    pub book: String,
    pub chapter: u32,
    pub verse: u32,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BibleDatasetInput {
    pub translation: TranslationInput,
    pub verses: Vec<VerseInput>,
}

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("invalid translation metadata: {0}")]
    InvalidTranslationMetadata(String),
    #[error("database error: {0}")]
    Database(String),
}

impl From<rusqlite::Error> for ImportError {
    fn from(e: rusqlite::Error) -> Self {
        ImportError::Database(e.to_string())
    }
}

/// A deterministic report of what one import call actually did - every
/// number here is derived from the dataset and the database's own
/// `changes()` count, never hard-coded (section 8).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub translation_id: String,
    pub dataset_version: String,
    /// Distinct valid books in the dataset.
    pub books: usize,
    /// Distinct valid (book, chapter) pairs in the dataset.
    pub chapters: usize,
    /// Total verse rows in the input, valid and invalid.
    pub verses_total: usize,
    /// Rows newly inserted by this call.
    pub imported: usize,
    /// Valid rows that already existed (idempotent re-import).
    pub already_present: usize,
    /// Rows rejected by validation (see `errors` for why each one).
    pub invalid: usize,
    pub errors: Vec<String>,
    /// A deterministic hash of translation id + every (book, chapter,
    /// verse, text) row, sorted - identical input always produces the
    /// same checksum, so a re-import of unchanged content is detectable
    /// without depending on row insertion order.
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

fn compute_checksum(translation_id: &str, rows: &[(String, u32, u32, String)]) -> String {
    let mut sorted = rows.to_vec();
    sorted.sort();
    let mut buf = String::new();
    buf.push_str(translation_id);
    buf.push('\n');
    for (book, chapter, verse, text) in &sorted {
        buf.push_str(&format!("{book}|{chapter}|{verse}|{text}\n"));
    }
    format!("{:016x}", fnv1a_hash(buf.as_bytes()))
}

/// Imports `dataset` into `conn`'s `bible_translations`/`bible_books`/
/// `bible_chapters`/`bible_verses` tables. Validates every row and skips
/// (with a reported reason) anything invalid rather than aborting the
/// whole import - only invalid *translation* metadata is fatal, since
/// nothing can be attributed to a translation that doesn't have a real
/// id/name.
pub fn import_bible_dataset(
    conn: &Connection,
    dataset: &BibleDatasetInput,
) -> Result<ImportReport, ImportError> {
    let translation = &dataset.translation;
    if translation.id.trim().is_empty()
        || translation.name.trim().is_empty()
        || translation.abbreviation.trim().is_empty()
        || translation.language.trim().is_empty()
    {
        return Err(ImportError::InvalidTranslationMetadata(
            "translation id, name, abbreviation, and language must all be non-empty".to_string(),
        ));
    }
    if translation.dataset_version.trim().is_empty() {
        return Err(ImportError::InvalidTranslationMetadata(
            "dataset_version must not be empty".to_string(),
        ));
    }

    let mut errors = Vec::new();
    let mut valid_rows: Vec<(String, u32, u32, String)> = Vec::new();
    let mut seen_in_dataset: BTreeSet<(String, u32, u32)> = BTreeSet::new();

    for row in &dataset.verses {
        let Some(book) = canonicalize_book(&row.book) else {
            errors.push(format!("invalid book: {:?}", row.book));
            continue;
        };
        if row.chapter == 0 {
            errors.push(format!("{}: invalid chapter number 0", book.name));
            continue;
        }
        if row.verse == 0 {
            errors.push(format!(
                "{} {}: invalid verse number 0",
                book.name, row.chapter
            ));
            continue;
        }
        if row.text.trim().is_empty() {
            errors.push(format!(
                "{} {}:{}: missing verse text",
                book.name, row.chapter, row.verse
            ));
            continue;
        }
        let key = (book.code.to_string(), row.chapter, row.verse);
        if !seen_in_dataset.insert(key) {
            errors.push(format!(
                "{} {}:{}: duplicate verse within this dataset",
                book.name, row.chapter, row.verse
            ));
            continue;
        }
        valid_rows.push((
            book.code.to_string(),
            row.chapter,
            row.verse,
            row.text.clone(),
        ));
    }

    let checksum = compute_checksum(&translation.id, &valid_rows);

    conn.execute(
        "INSERT OR IGNORE INTO bible_translations (id, name, abbreviation, language, is_local)
         VALUES (?1, ?2, ?3, ?4, 1)",
        params![
            translation.id,
            translation.name,
            translation.abbreviation,
            translation.language
        ],
    )?;

    let books: BTreeSet<&str> = valid_rows.iter().map(|(b, ..)| b.as_str()).collect();
    for book_code in &books {
        let Some(canonical) = cip_core_bible::book_alias::book_by_code(book_code) else {
            continue;
        };
        let chapter_count = valid_rows
            .iter()
            .filter(|(b, ..)| b == book_code)
            .map(|(_, c, ..)| *c)
            .collect::<BTreeSet<_>>()
            .len() as u32;
        let order = cip_core_bible::book_alias::BOOKS
            .iter()
            .position(|b| b.code == canonical.code)
            .unwrap_or(0) as u32;
        let testament = match canonical.testament {
            cip_core_bible::Testament::Old => "old",
            cip_core_bible::Testament::New => "new",
        };
        conn.execute(
            "INSERT OR IGNORE INTO bible_books
                (translation_id, code, name, testament, chapter_count, book_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                translation.id,
                canonical.code,
                canonical.name,
                testament,
                chapter_count,
                order
            ],
        )?;
    }

    let chapters: BTreeSet<(&str, u32)> = valid_rows
        .iter()
        .map(|(b, c, ..)| (b.as_str(), *c))
        .collect();
    for (book_code, chapter) in &chapters {
        let verse_count = valid_rows
            .iter()
            .filter(|(b, c, ..)| b == book_code && c == chapter)
            .count() as u32;
        conn.execute(
            "INSERT OR IGNORE INTO bible_chapters
                (translation_id, book_code, chapter_number, verse_count)
             VALUES (?1, ?2, ?3, ?4)",
            params![translation.id, book_code, chapter, verse_count],
        )?;
    }

    let mut imported = 0usize;
    let mut already_present = 0usize;
    for (book_code, chapter, verse, text) in &valid_rows {
        let changed = conn.execute(
            "INSERT OR IGNORE INTO bible_verses
                (translation_id, book_code, chapter_number, verse_number, text)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![translation.id, book_code, chapter, verse, text],
        )?;
        if changed > 0 {
            imported += 1;
        } else {
            already_present += 1;
        }
    }

    Ok(ImportReport {
        translation_id: translation.id.clone(),
        dataset_version: translation.dataset_version.clone(),
        books: books.len(),
        chapters: chapters.len(),
        verses_total: dataset.verses.len(),
        imported,
        already_present,
        invalid: errors.len(),
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

    fn small_dataset() -> BibleDatasetInput {
        BibleDatasetInput {
            translation: TranslationInput {
                id: "TEST".to_string(),
                name: "Test Translation".to_string(),
                abbreviation: "TST".to_string(),
                language: "en".to_string(),
                publisher: None,
                copyright: None,
                license: Some("public domain".to_string()),
                distribution: Some("public domain".to_string()),
                dataset_version: "1.0".to_string(),
            },
            verses: vec![
                VerseInput {
                    book: "Romans".to_string(),
                    chapter: 8,
                    verse: 28,
                    text: "And we know that all things work together for good...".to_string(),
                },
                VerseInput {
                    book: "ROM".to_string(),
                    chapter: 8,
                    verse: 29,
                    text: "For whom he did foreknow...".to_string(),
                },
                VerseInput {
                    book: "John".to_string(),
                    chapter: 3,
                    verse: 16,
                    text: "For God so loved the world...".to_string(),
                },
            ],
        }
    }

    #[test]
    fn imports_a_clean_dataset_and_reports_actual_counts() {
        let conn = migrated_conn();
        let report = import_bible_dataset(&conn, &small_dataset()).unwrap();

        assert_eq!(report.translation_id, "TEST");
        assert_eq!(report.books, 2);
        assert_eq!(report.chapters, 2);
        assert_eq!(report.verses_total, 3);
        assert_eq!(report.imported, 3);
        assert_eq!(report.already_present, 0);
        assert_eq!(report.invalid, 0);
        assert!(report.errors.is_empty());
        assert!(!report.checksum.is_empty());

        let stored: i64 = conn
            .query_row(
                "SELECT count(*) FROM bible_verses WHERE translation_id = 'TEST'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, 3);
    }

    #[test]
    fn reimporting_the_identical_dataset_creates_no_duplicate_rows() {
        let conn = migrated_conn();
        import_bible_dataset(&conn, &small_dataset()).unwrap();
        let second = import_bible_dataset(&conn, &small_dataset()).unwrap();

        assert_eq!(second.imported, 0, "nothing new to insert the second time");
        assert_eq!(second.already_present, 3);
        assert_eq!(
            second.checksum,
            import_bible_dataset(&conn, &small_dataset())
                .unwrap()
                .checksum
        );

        let stored: i64 = conn
            .query_row(
                "SELECT count(*) FROM bible_verses WHERE translation_id = 'TEST'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, 3, "re-import must never duplicate rows");
    }

    #[test]
    fn rejects_an_unknown_book_without_aborting_the_rest_of_the_import() {
        let conn = migrated_conn();
        let mut dataset = small_dataset();
        dataset.verses.push(VerseInput {
            book: "Frobnicate".to_string(),
            chapter: 1,
            verse: 1,
            text: "not a real book".to_string(),
        });

        let report = import_bible_dataset(&conn, &dataset).unwrap();
        assert_eq!(report.imported, 3, "the other valid rows still import");
        assert_eq!(report.invalid, 1);
        assert!(report.errors[0].contains("invalid book"));
    }

    #[test]
    fn rejects_zero_chapter_and_verse_numbers() {
        let conn = migrated_conn();
        let dataset = BibleDatasetInput {
            translation: small_dataset().translation,
            verses: vec![
                VerseInput {
                    book: "ROM".into(),
                    chapter: 0,
                    verse: 1,
                    text: "x".into(),
                },
                VerseInput {
                    book: "ROM".into(),
                    chapter: 1,
                    verse: 0,
                    text: "x".into(),
                },
            ],
        };
        let report = import_bible_dataset(&conn, &dataset).unwrap();
        assert_eq!(report.imported, 0);
        assert_eq!(report.invalid, 2);
    }

    #[test]
    fn rejects_missing_verse_text() {
        let conn = migrated_conn();
        let dataset = BibleDatasetInput {
            translation: small_dataset().translation,
            verses: vec![VerseInput {
                book: "ROM".into(),
                chapter: 8,
                verse: 28,
                text: "   ".into(),
            }],
        };
        let report = import_bible_dataset(&conn, &dataset).unwrap();
        assert_eq!(report.imported, 0);
        assert_eq!(report.invalid, 1);
        assert!(report.errors[0].contains("missing verse text"));
    }

    #[test]
    fn rejects_a_duplicate_verse_within_the_same_dataset() {
        let conn = migrated_conn();
        let dataset = BibleDatasetInput {
            translation: small_dataset().translation,
            verses: vec![
                VerseInput {
                    book: "ROM".into(),
                    chapter: 8,
                    verse: 28,
                    text: "first".into(),
                },
                VerseInput {
                    book: "ROM".into(),
                    chapter: 8,
                    verse: 28,
                    text: "second, duplicate".into(),
                },
            ],
        };
        let report = import_bible_dataset(&conn, &dataset).unwrap();
        assert_eq!(report.imported, 1, "only the first occurrence is imported");
        assert_eq!(report.invalid, 1);
        assert!(report.errors[0].contains("duplicate verse within this dataset"));
    }

    #[test]
    fn rejects_invalid_translation_metadata_before_touching_the_database() {
        let conn = migrated_conn();
        let mut dataset = small_dataset();
        dataset.translation.id = String::new();

        let err = import_bible_dataset(&conn, &dataset).unwrap_err();
        assert!(matches!(err, ImportError::InvalidTranslationMetadata(_)));

        let count: i64 = conn
            .query_row("SELECT count(*) FROM bible_verses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn checksum_is_deterministic_and_changes_with_content() {
        let conn = migrated_conn();
        let report_a = import_bible_dataset(&conn, &small_dataset()).unwrap();

        let mut different = small_dataset();
        different.translation.id = "TEST2".to_string();
        different.verses[0].text = "a different rendering entirely".to_string();
        let report_b = import_bible_dataset(&conn, &different).unwrap();

        assert_ne!(report_a.checksum, report_b.checksum);
    }

    #[test]
    fn a_book_name_and_its_code_canonicalize_to_the_same_stored_row() {
        let conn = migrated_conn();
        // "Romans" and "ROM" both appear in small_dataset() for chapter 8 -
        // they must canonicalize to the same book code, not two books.
        let report = import_bible_dataset(&conn, &small_dataset()).unwrap();
        assert_eq!(report.books, 2);

        let rom_books: i64 = conn
            .query_row(
                "SELECT count(*) FROM bible_books WHERE translation_id = 'TEST' AND code = 'ROM'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rom_books, 1);
    }
}
