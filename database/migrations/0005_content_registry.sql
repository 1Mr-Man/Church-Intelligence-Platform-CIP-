-- Phase 1.5 content/dataset foundation.
--
-- content_registry answers "what local content exists?" across every
-- content category (Bible today; music/sermon/media/reference in later
-- phases), independent of whichever domain-specific tables (e.g.
-- bible_translations/bible_verses) actually hold the content itself. One
-- row per installed dataset, carrying the provenance/licensing metadata
-- those domain tables deliberately do not (see docs/bible-datasets.md):
-- publisher/copyright/license/distribution are nullable because "unknown"
-- is a real, honest value here - never guessed.
--
-- id follows the "<type>:<domain-id>" convention (e.g. "bible:KJV") by
-- application-level agreement, not a DB constraint, so future content
-- types can choose their own scheme without a schema change.

CREATE TABLE content_registry (
    id           TEXT PRIMARY KEY,
    content_type TEXT NOT NULL
                     CHECK (content_type IN ('bible', 'music', 'service', 'media', 'reference')),
    name         TEXT NOT NULL,
    version      TEXT NOT NULL,
    language     TEXT NOT NULL,
    source       TEXT NOT NULL,
    publisher    TEXT,
    copyright    TEXT,
    license      TEXT,
    distribution TEXT,
    imported_at  TEXT NOT NULL,
    checksum     TEXT,
    status       TEXT NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled'))
);
CREATE INDEX idx_content_registry_type ON content_registry(content_type);
