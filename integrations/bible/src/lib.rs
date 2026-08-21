//! Local SQLite-backed [`BibleProvider`]. This is the only Phase 1
//! adapter: it proves the provider/adaptor boundary works end to end
//! against the real schema without requiring any network-backed provider
//! to exist yet. A future `integrations/bible` addition (e.g. an online
//! translation API) would live alongside this as another struct
//! implementing the same trait - `core/bible` and the UI would not change.

use cip_core_bible::{
    BibleBook, BibleChapter, BibleProvider, BibleProviderError, BibleTranslation, BibleVerse,
    ScriptureReference, Testament,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Mutex;

pub struct SqliteBibleProvider {
    conn: Mutex<Connection>,
}

impl SqliteBibleProvider {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }
}

fn testament_from_str(value: &str) -> Testament {
    match value {
        "new" => Testament::New,
        _ => Testament::Old,
    }
}

impl BibleProvider for SqliteBibleProvider {
    fn list_translations(&self) -> Result<Vec<BibleTranslation>, BibleProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("bible provider connection poisoned");
        let mut stmt = conn
            .prepare("SELECT id, name, abbreviation, language, is_local FROM bible_translations")
            .map_err(|e| BibleProviderError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(BibleTranslation {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    abbreviation: row.get(2)?,
                    language: row.get(3)?,
                    is_local: row.get::<_, i64>(4)? != 0,
                })
            })
            .map_err(|e| BibleProviderError::Storage(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| BibleProviderError::Storage(e.to_string()))
    }

    fn get_book(
        &self,
        translation_id: &str,
        book_code: &str,
    ) -> Result<Option<BibleBook>, BibleProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("bible provider connection poisoned");
        conn.query_row(
            "SELECT code, name, testament, chapter_count, book_order
             FROM bible_books WHERE translation_id = ?1 AND code = ?2",
            params![translation_id, book_code],
            |row| {
                Ok(BibleBook {
                    code: row.get(0)?,
                    name: row.get(1)?,
                    testament: testament_from_str(&row.get::<_, String>(2)?),
                    chapter_count: row.get(3)?,
                    order: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| BibleProviderError::Storage(e.to_string()))
    }

    fn get_chapter(
        &self,
        translation_id: &str,
        book_code: &str,
        chapter: u32,
    ) -> Result<Option<BibleChapter>, BibleProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("bible provider connection poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT verse_number, text FROM bible_verses
                 WHERE translation_id = ?1 AND book_code = ?2 AND chapter_number = ?3
                 ORDER BY verse_number",
            )
            .map_err(|e| BibleProviderError::Storage(e.to_string()))?;
        let verses = stmt
            .query_map(params![translation_id, book_code, chapter], |row| {
                let verse_number: u32 = row.get(0)?;
                let text: String = row.get(1)?;
                Ok(BibleVerse {
                    reference: ScriptureReference::single(
                        translation_id,
                        book_code,
                        chapter,
                        verse_number,
                    ),
                    text,
                })
            })
            .map_err(|e| BibleProviderError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| BibleProviderError::Storage(e.to_string()))?;

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
        let conn = self
            .conn
            .lock()
            .expect("bible provider connection poisoned");
        conn.query_row(
            "SELECT text FROM bible_verses
             WHERE translation_id = ?1 AND book_code = ?2 AND chapter_number = ?3 AND verse_number = ?4",
            params![
                reference.translation_id,
                reference.book,
                reference.chapter,
                reference.verse_start
            ],
            |row| {
                Ok(BibleVerse {
                    reference: reference.clone(),
                    text: row.get(0)?,
                })
            },
        )
        .optional()
        .map_err(|e| BibleProviderError::Storage(e.to_string()))
    }

    fn search(
        &self,
        query: &str,
        translation_id: &str,
    ) -> Result<Vec<BibleVerse>, BibleProviderError> {
        let conn = self
            .conn
            .lock()
            .expect("bible provider connection poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT book_code, chapter_number, verse_number, text FROM bible_verses
                 WHERE translation_id = ?1 AND text LIKE ?2
                 ORDER BY book_code, chapter_number, verse_number",
            )
            .map_err(|e| BibleProviderError::Storage(e.to_string()))?;
        let like_pattern = format!("%{query}%");
        let rows = stmt
            .query_map(params![translation_id, like_pattern], |row| {
                let book: String = row.get(0)?;
                let chapter: u32 = row.get(1)?;
                let verse: u32 = row.get(2)?;
                Ok(BibleVerse {
                    reference: ScriptureReference::single(translation_id, book, chapter, verse),
                    text: row.get(3)?,
                })
            })
            .map_err(|e| BibleProviderError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| BibleProviderError::Storage(e.to_string()));
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_database::{run_migrations, seed::apply_dev_seed};

    fn seeded_provider() -> SqliteBibleProvider {
        let mut conn = cip_database::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        apply_dev_seed(&conn).unwrap();
        SqliteBibleProvider::new(conn)
    }

    #[test]
    fn lists_the_seeded_translation() {
        let provider = seeded_provider();
        let translations = provider.list_translations().unwrap();
        assert_eq!(translations.len(), 1);
        assert_eq!(translations[0].id, "KJV");
    }

    #[test]
    fn resolves_a_single_verse_end_to_end_through_the_provider_trait() {
        let provider = seeded_provider();
        let provider: &dyn BibleProvider = &provider;
        let reference = ScriptureReference::single("KJV", "ROM", 8, 28);
        let verse = provider
            .get_verse(&reference)
            .unwrap()
            .expect("verse should exist");
        assert!(verse.text.contains("all things work together for good"));
    }

    #[test]
    fn search_finds_verses_by_substring() {
        let provider = seeded_provider();
        let results = provider.search("God so loved", "KJV").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].reference.book, "JHN");
    }

    #[test]
    fn missing_verse_returns_none_not_an_error() {
        let provider = seeded_provider();
        let reference = ScriptureReference::single("KJV", "ROM", 8, 999);
        assert!(provider.get_verse(&reference).unwrap().is_none());
    }
}
