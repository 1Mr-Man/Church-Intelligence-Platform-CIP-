-- Phase 9: Bible Translation Registry v2 - fine-grained, per-use-case
-- licensing permissions (cip_core_content::UsagePermissions), distinct
-- from the existing licensing_status admission gate. Every new column is
-- nullable and defaults to NULL - "not yet determined," never a
-- permissive assumption. Boolean permissions are stored as INTEGER
-- (0/1/NULL); NULL and 0 are both "not permitted," but only NULL means
-- "unknown" - a caller that cares about the distinction reads the raw
-- value, while every enforcement point in this codebase only ever asks
-- "is this exactly 1 (true)?" and treats anything else as not permitted.

ALTER TABLE content_registry ADD COLUMN rights_holder TEXT;
ALTER TABLE content_registry ADD COLUMN source_provider TEXT;
ALTER TABLE content_registry ADD COLUMN source_url TEXT;
ALTER TABLE content_registry ADD COLUMN attribution_text TEXT;
ALTER TABLE content_registry ADD COLUMN license_start TEXT;
ALTER TABLE content_registry ADD COLUMN license_expiry TEXT;
ALTER TABLE content_registry ADD COLUMN distribution_allowed INTEGER;
ALTER TABLE content_registry ADD COLUMN offline_storage_allowed INTEGER;
ALTER TABLE content_registry ADD COLUMN projection_allowed INTEGER;
ALTER TABLE content_registry ADD COLUMN api_allowed INTEGER;
ALTER TABLE content_registry ADD COLUMN commercial_allowed INTEGER;
ALTER TABLE content_registry ADD COLUMN ai_processing_allowed INTEGER;
ALTER TABLE content_registry ADD COLUMN llm_prompt_allowed INTEGER;
ALTER TABLE content_registry ADD COLUMN training_allowed INTEGER;
