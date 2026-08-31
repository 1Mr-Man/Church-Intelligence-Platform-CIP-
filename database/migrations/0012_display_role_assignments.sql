-- Phase 3.10.2 (Display Registry): persists which physical monitor is
-- assigned which presentation role (PROJECTOR/STAGE/CONFIDENCE/LOBBY/
-- OPERATOR/UNASSIGNED - see display_registry.rs). Global, not
-- service-scoped: which monitor plays which role is a property of the
-- machine CIP is running on, not any one church service - matching
-- saved_scriptures' own "deliberately not tied to service_id" precedent
-- (0010_saved_scriptures.sql).
--
-- monitor_id is not a real OS-issued identifier - Tauri's monitor API
-- exposes no such thing. It is CIP's own best-effort stable identifier
-- (the monitor's OS-reported name when one exists, otherwise a
-- position+resolution fingerprint - see display_registry::compute_monitor_id),
-- so an assignment can silently stop matching if an unnamed monitor's
-- position/resolution changes between sessions. This is a real, honestly
-- documented limitation, not a defect - see docs/phase-3-10-2-display-registry.md.
--
-- Deliberately an upsert table (ON CONFLICT DO UPDATE, in persistence.rs),
-- not an append-only log like every other table in this schema: a role
-- assignment has "current value" semantics (re-assigning replaces the
-- prior assignment), not "one row per event" semantics.
CREATE TABLE display_role_assignments (
    monitor_id TEXT PRIMARY KEY,
    role       TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
