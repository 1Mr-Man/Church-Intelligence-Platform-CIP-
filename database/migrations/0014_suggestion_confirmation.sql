-- Phase 5.2 (Temporal Confirmation / Sliding Re-Score): ai_suggestions
-- gains confirmation_count - how many times a heuristic (Paraphrase or
-- Semantic) suggestion's own reference was independently redetected while
-- the suggestion was still pending, within the existing suggestion-dedup
-- window (see apps/desktop/src-tauri/src/pipeline.rs's
-- SUGGESTION_DEDUP_WINDOW_SECONDS). Repetition is treated as corroborating
-- evidence for a single-shot heuristic guess, never as a reason to create
-- a second suggestion (dedup still suppresses that) - so this never
-- floods the queue; it only makes an already-surfaced suggestion's
-- confidence more honest.
--
-- Defaults to 0 for every pre-existing row - nothing is retroactively
-- reclassified as "confirmed" by this migration alone.
ALTER TABLE ai_suggestions
    ADD COLUMN confirmation_count INTEGER NOT NULL DEFAULT 0;
