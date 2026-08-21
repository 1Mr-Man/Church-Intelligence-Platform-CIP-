-- Phase 1 foundation schema.
--
-- Conventions:
--   * Primary keys for domain-event rows (services, transcript_segments,
--     scripture_detections, ai_suggestions, presentation_items,
--     audit_events) are TEXT UUIDs generated in application code, so a row
--     can be created and referenced before it is ever persisted.
--   * Bible content tables use surrogate INTEGER ids since they are
--     provider-populated, never user-created.
--   * Timestamps are ISO-8601 TEXT (UTC), matching chrono's default
--     `DateTime<Utc>` serialization - avoids SQLite's limited native
--     date/time typing.
--   * confidence_score/confidence_level mirror `cip-core-confidence`'s
--     `ConfidenceResult` so nothing needs to be recomputed when a row is
--     loaded back from disk.

PRAGMA foreign_keys = ON;

CREATE TABLE services (
    id         TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    status     TEXT NOT NULL CHECK (status IN ('started', 'paused', 'ended')),
    started_at TEXT NOT NULL,
    ended_at   TEXT
);

CREATE TABLE transcript_segments (
    id                TEXT PRIMARY KEY,
    service_id        TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    text              TEXT NOT NULL,
    is_final          INTEGER NOT NULL CHECK (is_final IN (0, 1)),
    confidence_score  REAL NOT NULL,
    confidence_level  TEXT NOT NULL CHECK (confidence_level IN ('low', 'medium', 'high')),
    start_ms          INTEGER NOT NULL,
    end_ms            INTEGER NOT NULL,
    created_at        TEXT NOT NULL
);
CREATE INDEX idx_transcript_segments_service_id ON transcript_segments(service_id);

CREATE TABLE bible_translations (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    abbreviation TEXT NOT NULL,
    language     TEXT NOT NULL,
    is_local     INTEGER NOT NULL DEFAULT 1 CHECK (is_local IN (0, 1))
);

CREATE TABLE bible_books (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    translation_id TEXT NOT NULL REFERENCES bible_translations(id) ON DELETE CASCADE,
    code           TEXT NOT NULL,
    name           TEXT NOT NULL,
    testament      TEXT NOT NULL CHECK (testament IN ('old', 'new')),
    chapter_count  INTEGER NOT NULL,
    book_order     INTEGER NOT NULL,
    UNIQUE (translation_id, code)
);

CREATE TABLE bible_chapters (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    translation_id TEXT NOT NULL,
    book_code      TEXT NOT NULL,
    chapter_number INTEGER NOT NULL,
    verse_count    INTEGER NOT NULL,
    FOREIGN KEY (translation_id, book_code) REFERENCES bible_books(translation_id, code) ON DELETE CASCADE,
    UNIQUE (translation_id, book_code, chapter_number)
);

CREATE TABLE bible_verses (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    translation_id TEXT NOT NULL,
    book_code      TEXT NOT NULL,
    chapter_number INTEGER NOT NULL,
    verse_number   INTEGER NOT NULL,
    text           TEXT NOT NULL,
    FOREIGN KEY (translation_id, book_code, chapter_number)
        REFERENCES bible_chapters(translation_id, book_code, chapter_number) ON DELETE CASCADE,
    UNIQUE (translation_id, book_code, chapter_number, verse_number)
);
CREATE INDEX idx_bible_verses_lookup ON bible_verses(translation_id, book_code, chapter_number);

CREATE TABLE scripture_detections (
    id                    TEXT PRIMARY KEY,
    service_id            TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    transcript_segment_id TEXT REFERENCES transcript_segments(id) ON DELETE SET NULL,
    reference             TEXT NOT NULL,
    translation_id        TEXT NOT NULL,
    confidence_score      REAL NOT NULL,
    confidence_level      TEXT NOT NULL CHECK (confidence_level IN ('low', 'medium', 'high')),
    status                TEXT NOT NULL DEFAULT 'detected'
                               CHECK (status IN ('detected', 'confirmed', 'rejected', 'updated')),
    detected_at           TEXT NOT NULL
);
CREATE INDEX idx_scripture_detections_service_id ON scripture_detections(service_id);

CREATE TABLE ai_suggestions (
    id                TEXT PRIMARY KEY,
    service_id        TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    kind              TEXT NOT NULL,
    payload           TEXT NOT NULL, -- JSON, shape depends on `kind`
    status            TEXT NOT NULL DEFAULT 'pending'
                          CHECK (status IN ('pending', 'approved', 'edited', 'rejected')),
    confidence_score  REAL NOT NULL,
    confidence_level  TEXT NOT NULL CHECK (confidence_level IN ('low', 'medium', 'high')),
    created_at        TEXT NOT NULL
);
CREATE INDEX idx_ai_suggestions_service_id ON ai_suggestions(service_id);

CREATE TABLE presentation_items (
    id           TEXT PRIMARY KEY,
    service_id   TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    content_type TEXT NOT NULL,
    content      TEXT NOT NULL, -- JSON, shape depends on `content_type`
    status       TEXT NOT NULL DEFAULT 'prepared'
                     CHECK (status IN ('prepared', 'active', 'stopped')),
    created_at   TEXT NOT NULL
);
CREATE INDEX idx_presentation_items_service_id ON presentation_items(service_id);

CREATE TABLE audit_events (
    id         TEXT PRIMARY KEY,
    service_id TEXT REFERENCES services(id) ON DELETE SET NULL,
    event_name TEXT NOT NULL,
    category   TEXT NOT NULL
                   CHECK (category IN (
                       'app', 'database', 'audio', 'speech', 'bible',
                       'ai', 'presentation', 'network', 'security', 'error'
                   )),
    payload    TEXT, -- JSON, optional event-specific detail
    created_at TEXT NOT NULL
);
CREATE INDEX idx_audit_events_service_id ON audit_events(service_id);
CREATE INDEX idx_audit_events_category ON audit_events(category);
