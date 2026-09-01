//! Phase 4.4: semantic (embedding-based) Bible search - the SQLite-backed
//! [`cip_core_bible::VerseEmbeddingStore`] implementation, plus the verse
//! embedding generation routine an operator triggers explicitly via the
//! `generate_verse_embeddings` command (see `commands.rs`). Deliberately
//! Tauri-agnostic (plain `rusqlite` + domain types, no `AppHandle`) -
//! matches `persistence.rs`'s own discipline so this stays directly
//! unit-testable without a running app.
//!
//! `bible_verse_embeddings` stores each vector as a raw little-endian
//! `f32` `BLOB` (see migration `0013_bible_verse_embeddings.sql`) - encoded
//! and decoded here, at the only boundary that needs to know that detail.

use chrono::Utc;
use cip_core_ai::EmbeddingEngine;
use cip_core_bible::{
    ScriptureReference, VerseEmbedding, VerseEmbeddingError, VerseEmbeddingStore,
};
use rusqlite::{params, Connection};
use std::sync::Mutex;
use thiserror::Error;

fn decode_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn encode_embedding(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Reads `bible_verse_embeddings` (joined against `bible_verses` to
/// reconstruct each row's [`ScriptureReference`]) through its own dedicated
/// `Mutex<Connection>` - mirroring `SqliteBibleProvider`'s exact shape
/// (`Mutex`, not a bare `Connection`, because `rusqlite::Connection` is
/// `!Sync` and [`VerseEmbeddingStore`] requires `Sync`) and mirroring
/// `AppState`'s established "every independent read path gets its own
/// connection" discipline (see e.g. `music_provider`/`acoustic_music_engine`'s
/// own docs) - the live pipeline can hold `AppState::db`'s lock for the
/// rest of a segment's persistence while this store independently reads
/// embeddings, with no risk of the two ever deadlocking each other. A
/// distinct type from `bible_provider` (not an extra trait method on
/// `BibleProvider` itself) because embeddings are an independent, optional
/// capability, exactly as [`VerseEmbeddingStore`]'s own doc comment
/// explains.
pub struct SqliteVerseEmbeddingStore {
    conn: Mutex<Connection>,
}

impl SqliteVerseEmbeddingStore {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// Exposes the underlying connection for
    /// `generate_verse_embeddings_for_translation`'s own per-verse
    /// lock/unlock writes - kept as one connection (not two) so generation
    /// and lookup never race against different SQLite handles for the same
    /// table.
    pub fn connection(&self) -> &Mutex<Connection> {
        &self.conn
    }
}

impl VerseEmbeddingStore for SqliteVerseEmbeddingStore {
    fn verse_embeddings(
        &self,
        translation_id: &str,
        model_id: &str,
    ) -> Result<Vec<VerseEmbedding>, VerseEmbeddingError> {
        let conn = self
            .conn
            .lock()
            .expect("verse embedding store connection poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT v.book_code, v.chapter_number, v.verse_number, e.embedding
                 FROM bible_verse_embeddings e
                 JOIN bible_verses v ON v.id = e.verse_id
                 WHERE v.translation_id = ?1 AND e.model_id = ?2",
            )
            .map_err(|e| VerseEmbeddingError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![translation_id, model_id], move |row| {
                let book: String = row.get(0)?;
                let chapter: u32 = row.get(1)?;
                let verse: u32 = row.get(2)?;
                let bytes: Vec<u8> = row.get(3)?;
                Ok(VerseEmbedding {
                    reference: ScriptureReference::single(translation_id, book, chapter, verse),
                    vector: decode_embedding(&bytes),
                })
            })
            .map_err(|e| VerseEmbeddingError::Storage(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| VerseEmbeddingError::Storage(e.to_string()))
    }
}

#[derive(Debug, Error)]
pub enum EmbeddingGenerationError {
    #[error("database error: {0}")]
    Database(String),
}

/// One verse still needing an embedding, read from `bible_verses` directly
/// (never through `BibleProvider`, since generation needs every verse of a
/// translation in one pass, not the chapter/reference-scoped shape that
/// trait offers).
struct PendingVerse {
    verse_id: i64,
    reference: String,
    text: String,
}

/// Result of one `generate_verse_embeddings` run - always returned, even on
/// partial failure, so an operator sees real numbers rather than only a
/// pass/fail. `total_verses` is `attempted + already_embedded`, so an
/// operator can tell "nothing left to do" (`attempted == 0`) apart from
/// "the translation itself is empty" (`total_verses == 0`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingGenerationSummary {
    pub total_verses: u64,
    /// Verses that already had a row for this exact `model_id` before this
    /// run started - skipped, never re-embedded (idempotent/resumable: an
    /// interrupted run can simply be re-triggered).
    pub already_embedded: u64,
    pub attempted: u64,
    pub succeeded: u64,
    /// One entry per verse whose `EmbeddingEngine::embed` call itself
    /// returned an error (a real, individual model failure) - `reference`
    /// plus the engine's own error text, never silently dropped.
    pub failures: Vec<EmbeddingFailure>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingFailure {
    pub reference: String,
    pub reason: String,
}

/// Embeds every verse of `translation_id` not already embedded under
/// `engine.model_id()`, persisting each as it completes. Holds
/// `conn_mutex`'s lock only briefly per verse (once to read the pending
/// list, once per verse to write its row) rather than for the whole run -
/// `engine.embed` itself (the expensive part, one CPU-bound forward pass
/// per verse) always runs with the lock released, so this deliberately
/// long-running operation never blocks the live pipeline's own database
/// access for its full duration, only for each brief read/write.
///
/// Never fatal on a single verse's failure: a verse whose `embed` call
/// errors is recorded in the returned summary's `failures` and skipped,
/// exactly like every other "one bad candidate never blocks the rest of
/// the batch" rule in this codebase (see `bible_intelligence`'s validation
/// discipline). Only a real database error aborts the whole run early.
pub fn generate_verse_embeddings_for_translation(
    conn_mutex: &Mutex<Connection>,
    engine: &dyn EmbeddingEngine,
    translation_id: &str,
) -> Result<EmbeddingGenerationSummary, EmbeddingGenerationError> {
    let model_id = engine.model_id().to_string();

    let (pending, already_embedded) = {
        let conn = conn_mutex.lock().expect("db connection poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT v.id, v.book_code, v.chapter_number, v.verse_number, v.text
                 FROM bible_verses v
                 WHERE v.translation_id = ?1
                   AND NOT EXISTS (
                       SELECT 1 FROM bible_verse_embeddings e
                       WHERE e.verse_id = v.id AND e.model_id = ?2
                   )",
            )
            .map_err(|e| EmbeddingGenerationError::Database(e.to_string()))?;
        let pending: Vec<PendingVerse> = stmt
            .query_map(params![translation_id, model_id], |row| {
                let verse_id: i64 = row.get(0)?;
                let book: String = row.get(1)?;
                let chapter: u32 = row.get(2)?;
                let verse: u32 = row.get(3)?;
                let text: String = row.get(4)?;
                Ok(PendingVerse {
                    verse_id,
                    reference: format!("{book} {chapter}:{verse}"),
                    text,
                })
            })
            .map_err(|e| EmbeddingGenerationError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| EmbeddingGenerationError::Database(e.to_string()))?;

        let total_verses: u64 =
            conn.query_row(
                "SELECT count(*) FROM bible_verses WHERE translation_id = ?1",
                params![translation_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| EmbeddingGenerationError::Database(e.to_string()))? as u64;
        let already_embedded = total_verses.saturating_sub(pending.len() as u64);
        (pending, already_embedded)
    };

    let mut succeeded = 0u64;
    let mut failures = Vec::new();

    for verse in &pending {
        match engine.embed(&verse.text) {
            Ok(vector) => {
                let encoded = encode_embedding(&vector);
                let conn = conn_mutex.lock().expect("db connection poisoned");
                conn.execute(
                    "INSERT INTO bible_verse_embeddings
                        (verse_id, model_id, dimensions, embedding, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (verse_id, model_id) DO UPDATE SET
                        dimensions = excluded.dimensions,
                        embedding = excluded.embedding,
                        created_at = excluded.created_at",
                    params![
                        verse.verse_id,
                        model_id,
                        vector.len() as i64,
                        encoded,
                        Utc::now().to_rfc3339(),
                    ],
                )
                .map_err(|e| EmbeddingGenerationError::Database(e.to_string()))?;
                succeeded += 1;
            }
            Err(e) => failures.push(EmbeddingFailure {
                reference: verse.reference.clone(),
                reason: e.to_string(),
            }),
        }
    }

    Ok(EmbeddingGenerationSummary {
        total_verses: already_embedded + pending.len() as u64,
        already_embedded,
        attempted: pending.len() as u64,
        succeeded,
        failures,
    })
}

/// How many of `translation_id`'s verses already have an embedding under
/// `model_id` - the coverage figure `get_embedding_capabilities` reports,
/// computed the same way `generate_verse_embeddings_for_translation` itself
/// determines what's left to do.
pub fn embedding_coverage(
    conn: &Connection,
    translation_id: &str,
    model_id: &str,
) -> Result<(u64, u64), EmbeddingGenerationError> {
    let total_verses: u64 =
        conn.query_row(
            "SELECT count(*) FROM bible_verses WHERE translation_id = ?1",
            params![translation_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| EmbeddingGenerationError::Database(e.to_string()))? as u64;
    let embedded: u64 =
        conn.query_row(
            "SELECT count(*) FROM bible_verse_embeddings e
             JOIN bible_verses v ON v.id = e.verse_id
             WHERE v.translation_id = ?1 AND e.model_id = ?2",
            params![translation_id, model_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| EmbeddingGenerationError::Database(e.to_string()))? as u64;
    Ok((embedded, total_verses))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_ai::EmbeddingEngineError;
    use cip_database::{run_migrations, seed::apply_dev_seed};

    struct FixedEmbeddingEngine {
        model_id: String,
        dimensions: usize,
    }

    impl EmbeddingEngine for FixedEmbeddingEngine {
        fn is_ready(&self) -> bool {
            true
        }
        fn model_id(&self) -> &str {
            &self.model_id
        }
        fn dimensions(&self) -> usize {
            self.dimensions
        }
        fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingEngineError> {
            // Deterministic, content-independent stand-in for a real
            // model - real inference is not available in this
            // environment (see docs/phase-4-4-semantic-bible-search.md).
            // Only used to prove the storage/round-trip plumbing here.
            Ok(vec![text.len() as f32 % 7.0; self.dimensions])
        }
    }

    fn seeded_db() -> Mutex<Connection> {
        let mut conn = cip_database::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        apply_dev_seed(&conn).unwrap();
        Mutex::new(conn)
    }

    #[test]
    fn generation_embeds_every_verse_of_the_translation_exactly_once() {
        let conn = seeded_db();
        let engine = FixedEmbeddingEngine {
            model_id: "test-model".to_string(),
            dimensions: 4,
        };
        let summary = generate_verse_embeddings_for_translation(&conn, &engine, "KJV").unwrap();

        assert!(summary.attempted > 0);
        assert_eq!(summary.succeeded, summary.attempted);
        assert!(summary.failures.is_empty());
        assert_eq!(summary.already_embedded, 0);
        assert_eq!(summary.total_verses, summary.attempted);
    }

    #[test]
    fn a_second_run_is_a_no_op_because_every_verse_is_already_embedded() {
        let conn = seeded_db();
        let engine = FixedEmbeddingEngine {
            model_id: "test-model".to_string(),
            dimensions: 4,
        };
        let first = generate_verse_embeddings_for_translation(&conn, &engine, "KJV").unwrap();
        let second = generate_verse_embeddings_for_translation(&conn, &engine, "KJV").unwrap();

        assert_eq!(second.attempted, 0);
        assert_eq!(second.already_embedded, first.succeeded);
    }

    #[test]
    fn a_different_model_id_re_embeds_independently_of_the_first() {
        let conn = seeded_db();
        let engine_a = FixedEmbeddingEngine {
            model_id: "model-a".to_string(),
            dimensions: 4,
        };
        let engine_b = FixedEmbeddingEngine {
            model_id: "model-b".to_string(),
            dimensions: 4,
        };
        generate_verse_embeddings_for_translation(&conn, &engine_a, "KJV").unwrap();
        let summary_b = generate_verse_embeddings_for_translation(&conn, &engine_b, "KJV").unwrap();

        assert!(
            summary_b.attempted > 0,
            "a different model_id must never be treated as already covered"
        );
        assert_eq!(summary_b.already_embedded, 0);
    }

    fn seeded_store() -> SqliteVerseEmbeddingStore {
        let mut conn = cip_database::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        apply_dev_seed(&conn).unwrap();
        SqliteVerseEmbeddingStore::new(conn)
    }

    #[test]
    fn the_store_returns_generated_embeddings_round_tripped_correctly() {
        let store = seeded_store();
        let engine = FixedEmbeddingEngine {
            model_id: "test-model".to_string(),
            dimensions: 4,
        };
        generate_verse_embeddings_for_translation(store.connection(), &engine, "KJV").unwrap();

        let embeddings = store.verse_embeddings("KJV", "test-model").unwrap();
        assert!(!embeddings.is_empty());
        for e in &embeddings {
            assert_eq!(e.vector.len(), 4);
            assert_eq!(e.reference.translation_id, "KJV");
        }
    }

    #[test]
    fn the_store_returns_nothing_for_an_unconfigured_model_id() {
        let store = seeded_store();
        let engine = FixedEmbeddingEngine {
            model_id: "test-model".to_string(),
            dimensions: 4,
        };
        generate_verse_embeddings_for_translation(store.connection(), &engine, "KJV").unwrap();

        let embeddings = store.verse_embeddings("KJV", "some-other-model").unwrap();
        assert!(embeddings.is_empty());
    }

    #[test]
    fn embedding_coverage_reports_embedded_and_total_counts() {
        let conn = seeded_db();
        let engine = FixedEmbeddingEngine {
            model_id: "test-model".to_string(),
            dimensions: 4,
        };
        let summary = generate_verse_embeddings_for_translation(&conn, &engine, "KJV").unwrap();

        let (embedded, total) =
            embedding_coverage(&conn.lock().unwrap(), "KJV", "test-model").unwrap();
        assert_eq!(embedded, summary.succeeded);
        assert_eq!(total, summary.total_verses);
    }

    #[test]
    fn embedding_coverage_is_zero_for_an_unconfigured_model_id() {
        let conn = seeded_db();
        let engine = FixedEmbeddingEngine {
            model_id: "test-model".to_string(),
            dimensions: 4,
        };
        generate_verse_embeddings_for_translation(&conn, &engine, "KJV").unwrap();

        let (embedded, total) =
            embedding_coverage(&conn.lock().unwrap(), "KJV", "some-other-model").unwrap();
        assert_eq!(embedded, 0);
        assert!(total > 0);
    }
}
