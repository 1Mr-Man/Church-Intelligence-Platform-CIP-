-- Phase 4.4 (True Semantic Bible Search): storage for precomputed verse
-- embeddings, so semantic search never re-embeds the whole Bible at query
-- time (offline, real-time detection during a live service needs this to
-- be a lookup, not a batch inference job).
--
-- verse_id is bible_verses' own AUTOINCREMENT primary key, not the
-- (translation_id, book_code, chapter_number, verse_number) tuple - this
-- table is purely an index into that existing table, never a second
-- source of truth for what a verse's text is.
--
-- model_id identifies which embedding model produced a row (e.g.
-- "all-MiniLM-L6-v2") and is part of the primary key alongside verse_id:
-- switching embedding models (or upgrading one) must never silently mix
-- vectors from two different models in one similarity comparison - old
-- rows for a since-abandoned model_id simply stop being selected, not
-- deleted (no destructive migration needed on a model change). dimensions
-- is stored redundantly alongside the vector itself (rather than only
-- implied by model_id) so a mismatched-dimension read can be rejected with
-- one integer comparison, never a partial dot-product.
--
-- embedding is a raw little-endian f32 BLOB (4 * dimensions bytes) -
-- cheapest possible encoding for values only ever read back into a
-- Vec<f32> in-process, never queried by SQL itself.
--
-- No FOREIGN KEY to bible_verses: bible_verses' own PRIMARY KEY is
-- AUTOINCREMENT INTEGER with no natural key CIP can safely assume is
-- stable across a dataset re-import (see docs/bible-datasets.md) - exactly
-- the same reasoning scripture_detections already applies to its own
-- verse references (it stores the human-readable reference, not a verse
-- FK). A stale verse_id after a re-import is simply treated as
-- "not yet embedded" and silently skipped by similarity search, never a
-- constraint violation.
CREATE TABLE bible_verse_embeddings (
    verse_id   INTEGER NOT NULL,
    model_id   TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    embedding  BLOB NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (verse_id, model_id)
);
CREATE INDEX idx_bible_verse_embeddings_model ON bible_verse_embeddings(model_id);
