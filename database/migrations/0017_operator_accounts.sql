-- Phase 10: Church/User Roles & Permissions - the local operator account
-- table docs/phase-4-master-plan-gap-audit.md's own cross-cutting audit
-- named as entirely missing ("no login, no user table, no role
-- enforcement"). One row per local operator; pin_hash/pin_salt are the
-- only persisted secret material (see cip_core_access::hash_pin) and are
-- never sent to the frontend (commands::OperatorAccountSummaryDto omits
-- them). display_name is unique so an operator picking their own name
-- from a login list is unambiguous.

CREATE TABLE operator_accounts (
    id           TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    role         TEXT NOT NULL CHECK (role IN ('admin', 'operator')),
    pin_hash     TEXT NOT NULL,
    pin_salt     TEXT NOT NULL,
    created_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_operator_accounts_display_name ON operator_accounts(display_name);
