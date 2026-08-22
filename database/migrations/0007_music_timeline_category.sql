-- Phase 2.1: extend audit_events.category to accept 'music'.
--
-- Music Intelligence timeline entries (a finding detected, accepted, or
-- rejected - see apps/desktop/src-tauri/src/commands.rs's music section)
-- are a genuine, permanent new caller of the service timeline, unlike
-- Content Registry management (which timeline.rs deliberately maps onto
-- the existing 'app' bucket, documented there as "no current caller").
-- SQLite has no ALTER TABLE for CHECK constraints, so the table is
-- recreated with the extended constraint and its rows copied across -
-- nothing else references audit_events by foreign key, so this is safe.

CREATE TABLE audit_events_new (
    id         TEXT PRIMARY KEY,
    service_id TEXT REFERENCES services(id) ON DELETE SET NULL,
    event_name TEXT NOT NULL,
    category   TEXT NOT NULL
                   CHECK (category IN (
                       'app', 'database', 'audio', 'speech', 'bible',
                       'ai', 'presentation', 'music', 'network', 'security', 'error'
                   )),
    payload    TEXT, -- JSON, optional event-specific detail
    created_at TEXT NOT NULL
);

INSERT INTO audit_events_new (id, service_id, event_name, category, payload, created_at)
    SELECT id, service_id, event_name, category, payload, created_at FROM audit_events;

DROP TABLE audit_events;
ALTER TABLE audit_events_new RENAME TO audit_events;

CREATE INDEX idx_audit_events_service_id ON audit_events(service_id);
CREATE INDEX idx_audit_events_category ON audit_events(category);
-- Recreates the index 0003_service_operations.sql added - DROP TABLE
-- above dropped it along with the old table.
CREATE INDEX idx_audit_events_service_created ON audit_events(service_id, created_at);
