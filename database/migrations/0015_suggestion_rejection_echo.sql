-- Phase 5.4 (Wrong-Verse Feedback Loop): ai_suggestions gains
-- rejection_echo_count - how many times a Rejected suggestion's own
-- reference was independently redetected (same Paraphrase/Semantic
-- category) within the existing suggestion-dedup window after the
-- operator rejected it, while that repeat was silently suppressed
-- exactly as before (see apps/desktop/src-tauri/src/pipeline.rs's
-- SUGGESTION_DEDUP_WINDOW_SECONDS).
--
-- Previously an operator's Reject decision had zero downstream effect: a
-- same-category repeat within the window was silently dropped with no
-- trace at all (dedup suppressed it, and confirm_suggestion's Pending-only
-- lookup found nothing to update). This column makes that already-existing
-- suppression observable - a purely additive counter, never a status/score
-- change, and never a reason to resurrect a decided suggestion back to
-- Pending.
--
-- Defaults to 0 for every pre-existing row - nothing is retroactively
-- reclassified as "echoed" by this migration alone.
ALTER TABLE ai_suggestions
    ADD COLUMN rejection_echo_count INTEGER NOT NULL DEFAULT 0;
