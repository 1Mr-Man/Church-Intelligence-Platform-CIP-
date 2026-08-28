-- Phase 2.7.1 (Content Intelligence Operationalization): durable storage
-- for an operator's *accepted* ContentCandidate, closing the gap the
-- audit (docs/phase-2-7-1-audit.md section E) found - ContentCandidate
-- previously lived only in AppState::content_candidate_queue, an
-- in-memory Mutex, so an accepted candidate did not survive the service
-- ending, let alone an application restart.
--
-- `payload` stores the complete `ContentCandidate` as JSON, verbatim -
-- the same convention `ai_suggestions.payload`/`presentation_items.content`
-- already use for a variable-shaped Rust type - so provenance, evidence,
-- confidence, and assertion level are preserved byte-for-byte, never
-- re-derived from this table. `candidate_type` is duplicated out of the
-- payload as its own column (matching `presentation_items.content_type`'s
-- precedent) purely so a row's kind is visible without parsing JSON.
--
-- Only ever written once, at the moment `accept_content_candidate`
-- succeeds (mirroring `saved_scriptures`: an explicit, operator-initiated
-- "save," never an automatic write on detection). Rejected and merely
-- pending candidates are never persisted here.
CREATE TABLE saved_content_candidates (
    id             TEXT PRIMARY KEY,
    service_id     TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    candidate_type TEXT NOT NULL,
    payload        TEXT NOT NULL, -- JSON: the full ContentCandidate
    created_at     TEXT NOT NULL
);
CREATE INDEX idx_saved_content_candidates_service_id ON saved_content_candidates(service_id);
