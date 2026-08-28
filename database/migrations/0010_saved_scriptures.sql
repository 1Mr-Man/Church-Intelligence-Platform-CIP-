-- Phase 3.6 (Church Knowledge Libraries): saved/reusable Scripture
-- references, for the Bible Library's "Save" action.
--
-- Deliberately NOT the same thing as `scripture_detections` (an automatic,
-- service-scoped detection record) or `ai_suggestions`/`presentation_items`
-- (service-scoped, one-shot review/display queues). A saved scripture is a
-- standalone, church-wide bookmark an operator creates on purpose from the
-- Bible Library, meant to be found again in a *future* service, so it is
-- intentionally NOT tied to any one service_id - the audit in
-- docs/phase-3-6-church-libraries.md confirmed no existing table already
-- serves this "reusable, cross-service bookmark" purpose (the closest
-- candidate, `ScriptureContextManager`'s in-memory recent-references
-- deque, is bounded, per-session, and never persisted).
--
-- verse_end is nullable: a saved item may be a single verse (verse_end
-- NULL) or a verse range (verse_end set), matching how Bible Library
-- search/browse already represents both.
CREATE TABLE saved_scriptures (
    id            TEXT PRIMARY KEY,
    translation_id TEXT NOT NULL,
    book          TEXT NOT NULL,
    chapter       INTEGER NOT NULL,
    verse_start   INTEGER NOT NULL,
    verse_end     INTEGER,
    reference_display TEXT NOT NULL, -- e.g. "ROM 8:28" or "ROM 8:28-30"
    note          TEXT,
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_saved_scriptures_created ON saved_scriptures(created_at);
