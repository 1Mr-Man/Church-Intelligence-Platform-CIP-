-- Phase 24.3 (true dual-tier Whisper): links a quality-tier
-- re-transcription back to the fast-tier `transcript_segments` row it
-- corrects. Deliberately does *not* mutate `transcript_segments` in place -
-- see `pipeline.rs`'s own "never edit a record after the fact" principle,
-- already applied to `scripture_detections`. Instead the quality tier's
-- output is persisted as its own ordinary `transcript_segments` row (via
-- the same `persist_transcript_segment` every other final segment uses),
-- and this table records only the link between the two - the original's
-- text as the operator actually saw it live remains untouched.
--
-- `original_segment_id`/`corrected_segment_id` are both real
-- `transcript_segments` rows by the time this table's row is written -
-- the caller (`commands::spawn_quality_worker`) always persists the
-- corrected segment first, then this link - so both foreign keys are
-- always satisfiable under `PRAGMA foreign_keys = ON`.
CREATE TABLE transcript_corrections (
    id                   TEXT PRIMARY KEY,
    original_segment_id  TEXT NOT NULL REFERENCES transcript_segments(id) ON DELETE CASCADE,
    corrected_segment_id TEXT NOT NULL REFERENCES transcript_segments(id) ON DELETE CASCADE,
    created_at           TEXT NOT NULL
);
CREATE INDEX idx_transcript_corrections_original_segment_id ON transcript_corrections(original_segment_id);
CREATE INDEX idx_transcript_corrections_corrected_segment_id ON transcript_corrections(corrected_segment_id);
