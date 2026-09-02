-- Phase 13 (Church Knowledge Base / Cross-Sermon Analytics): durable
-- storage for an operator's *accepted* Sermon Intelligence findings,
-- closing the gap docs/phase-13-audit.md found - IntelligenceFinding
-- previously lived only in AppState.intelligence_findings, an in-memory
-- Mutex constructed once at startup and never persisted, so no finding
-- survived an application restart, making genuine cross-sermon analytics
-- (as opposed to cross-sermon-within-one-continuous-run) impossible.
--
-- Mirrors saved_content_candidates (0011) exactly: `payload` stores the
-- complete IntelligenceFinding as JSON verbatim, so provenance, evidence,
-- confidence, and assertion level are preserved byte-for-byte, never
-- re-derived from this table. `element_label` duplicates a value derived
-- from `summary`'s established text-prefix convention (see
-- sermon_knowledge_base.rs) purely so it is groupable without parsing
-- JSON, matching `saved_content_candidates.candidate_type`'s own
-- precedent.
--
-- Only ever written once, at the moment `accept_sermon_finding` succeeds
-- (mirroring `saved_content_candidates`: an explicit, operator-initiated
-- "accept," never an automatic write on detection). Detected/Reviewed/
-- Rejected findings are never persisted here.
--
-- `sermon_id` is nullable because IntelligenceFinding.sermon_id itself is
-- Option<Uuid> - a finding accepted with no active sermon context is
-- still real, operator-confirmed history, it just cannot be grouped by
-- sermon for cross-sermon analytics (an honest, documented limitation).
CREATE TABLE saved_sermon_findings (
    id            TEXT PRIMARY KEY,
    service_id    TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    sermon_id     TEXT REFERENCES sermons(id) ON DELETE SET NULL,
    element_label TEXT NOT NULL,
    summary       TEXT NOT NULL,
    payload       TEXT NOT NULL, -- JSON: the full IntelligenceFinding
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_saved_sermon_findings_service_id ON saved_sermon_findings(service_id);
CREATE INDEX idx_saved_sermon_findings_sermon_id ON saved_sermon_findings(sermon_id);
CREATE INDEX idx_saved_sermon_findings_element_label ON saved_sermon_findings(element_label);
