-- Phase 2.5 sermon intelligence foundation (per the authoritative Phase 2
-- roadmap). Additive only - every earlier row/table/constraint remains
-- valid unchanged.
--
-- Persistence is justified here (unlike Service Intelligence's Phase 2.4
-- phase-tracking, which stays in-memory) for the same reasons
-- `services` itself is persisted: sermon history/auditability, and a
-- restart must not lose the record of what sermon happened, who spoke,
-- and which transcript segments belonged to it. See
-- docs/sermon-foundation.md's "Persistence decision" section.
--
-- Mirrors `services`' own precedent exactly: a row here durably records
-- what happened, but `AppState.active_sermon` is *not* automatically
-- restored into the live session on app restart (the same is true of
-- `AppState.active_service` today) - restart recovery means "the history
-- is not lost," not "a live session resumes unattended."

CREATE TABLE sermons (
    id         TEXT PRIMARY KEY,
    service_id TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    title      TEXT,
    speaker_id TEXT,
    speaker_name TEXT,
    speaker_role TEXT CHECK (speaker_role IN ('primary', 'guest')),
    status     TEXT NOT NULL
                   CHECK (status IN ('planned', 'active', 'paused', 'ended', 'cancelled')),
    started_at TEXT,
    ended_at   TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_sermons_service_id ON sermons(service_id);

CREATE TABLE sermon_sections (
    id         TEXT PRIMARY KEY,
    sermon_id  TEXT NOT NULL REFERENCES sermons(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL
                   CHECK (kind IN (
                       'introduction', 'scripture_reading', 'main_message',
                       'illustration', 'prayer', 'altar_call', 'conclusion'
                   )),
    origin     TEXT NOT NULL CHECK (origin IN ('operator_assigned', 'system_boundary', 'inferred')),
    started_at TEXT NOT NULL,
    ended_at   TEXT,
    note       TEXT
);
CREATE INDEX idx_sermon_sections_sermon_id ON sermon_sections(sermon_id);

CREATE TABLE sermon_segments (
    id                    TEXT PRIMARY KEY,
    sermon_id             TEXT NOT NULL REFERENCES sermons(id) ON DELETE CASCADE,
    transcript_segment_id TEXT NOT NULL REFERENCES transcript_segments(id) ON DELETE CASCADE,
    sequence              INTEGER NOT NULL,
    section_id            TEXT REFERENCES sermon_sections(id) ON DELETE SET NULL,
    linked_at             TEXT NOT NULL,
    UNIQUE (sermon_id, transcript_segment_id)
);
CREATE INDEX idx_sermon_segments_sermon_id ON sermon_segments(sermon_id);
CREATE INDEX idx_sermon_segments_transcript_segment_id ON sermon_segments(transcript_segment_id);
