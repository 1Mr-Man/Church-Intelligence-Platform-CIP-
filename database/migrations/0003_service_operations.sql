-- Phase 1.3 live service intelligence & operator workflow. Additive only -
-- every Phase 1.0/1.1/1.2 row remains valid with these new columns simply
-- NULL, and no existing column, table, or constraint is modified.
--
-- ai_suggestions gains what's needed to trace a suggestion back to the
-- transcript segment that produced it (Phase 1.3's "transcript + Scripture
-- linking" requirement) - the same traceability scripture_detections
-- already had via transcript_segment_id/source_text (migration 0002):
--   * transcript_segment_id - the originating segment, so the operator can
--     see "what did the pastor say" next to a suggestion in the queue.
--     ON DELETE SET NULL, matching scripture_detections' own FK, so a
--     suggestion is never lost if its source segment ever is.
--   * source_text           - the raw transcript substring, so the link
--     survives even if the referenced segment is unavailable.
--
-- The service timeline (Phase 1.3 section 7/8/32) reuses the existing
-- audit_events table rather than introducing a second event/timeline
-- table - this index is the only schema change it needs, for the
-- service-scoped, chronological queries the Live Church Brain's timeline
-- view runs.

ALTER TABLE ai_suggestions ADD COLUMN transcript_segment_id TEXT REFERENCES transcript_segments(id) ON DELETE SET NULL;
ALTER TABLE ai_suggestions ADD COLUMN source_text TEXT;

CREATE INDEX IF NOT EXISTS idx_audit_events_service_created ON audit_events(service_id, created_at);
