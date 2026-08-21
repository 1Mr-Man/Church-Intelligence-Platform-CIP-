-- Phase 1.4 presentation foundation. Additive only - every existing
-- presentation_items row remains valid with these new columns simply NULL.
--
-- presentation_items gains the same kind of traceability ai_suggestions and
-- scripture_detections already have back to their source:
--   * source_suggestion_id - the ai_suggestions row this item was prepared
--     from, when it came from the automatic detection + operator-approval
--     path rather than manual creation. ON DELETE SET NULL, matching the
--     existing transcript_segment_id FK pattern from migration 0003, so a
--     presentation item is never lost if its source suggestion is.
--   * template              - the rendering template applied when this item
--     was prepared (e.g. "SCRIPTURE_DEFAULT"), so the item's on-screen
--     layout is reconstructable/auditable after the fact.
--
-- An index on source_suggestion_id supports the "which presentation, if
-- any, came from this suggestion" lookup the operator workspace needs when
-- showing suggestion -> presentation linkage.

ALTER TABLE presentation_items ADD COLUMN source_suggestion_id TEXT REFERENCES ai_suggestions(id) ON DELETE SET NULL;
ALTER TABLE presentation_items ADD COLUMN template TEXT;

CREATE INDEX IF NOT EXISTS idx_presentation_items_source_suggestion ON presentation_items(source_suggestion_id);
