-- Phase 1.2 live speech pipeline: columns the Phase 1.0 schema didn't yet
-- need, because no real detector or speech engine existed to populate
-- them. All additive/nullable (SQLite's ALTER TABLE ADD COLUMN), so every
-- Phase 1.0/1.1 row remains valid with these columns simply NULL.
--
-- transcript_segments gains what a real SpeechEngine can now supply:
--   * sequence_number - the engine's own monotonic per-session ordering,
--     for reconstructing utterance order independent of start_ms/rowid.
--   * language        - BCP-47-ish tag when the engine reports one.
--   * speaker_id      - only populated by an engine that actually performs
--     speaker identification; never invented.
--
-- scripture_detections gains what's needed to reconstruct a detection
-- without re-parsing the source transcript:
--   * detection_type - the ReferenceKind ("direct_reference" /
--     "chapter_reference" / "verse_reference" / "sequential_reference")
--     that produced this row. Ambiguous/unresolved detections are still
--     never persisted here (see docs/live-speech.md) - only ones that
--     validated - so this column only ever holds one of those four.
--   * source_text    - the exact transcript substring that produced the
--     detection, for audit/diagnostics.

ALTER TABLE transcript_segments ADD COLUMN sequence_number INTEGER;
ALTER TABLE transcript_segments ADD COLUMN language TEXT;
ALTER TABLE transcript_segments ADD COLUMN speaker_id TEXT;

ALTER TABLE scripture_detections ADD COLUMN detection_type TEXT;
ALTER TABLE scripture_detections ADD COLUMN source_text TEXT;
