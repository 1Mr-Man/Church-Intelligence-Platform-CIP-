//! Operator account/role domain contract (Phase 10) - the answer to
//! `docs/phase-4-master-plan-gap-audit.md`'s "Church/user roles &
//! permissions" gap: *"No multi-user model exists at all - the app has
//! no login, no user table, no role enforcement."*
//!
//! CIP is a single local desktop installation, not a multi-tenant SaaS
//! product (spec section 33/34: fully offline, one SQLite file per
//! church). "Multi-user" here means separate human operators of the
//! same installation across services/seasons - a tech lead or pastor who
//! configures licensing/OBS-vMix credentials/AI model files, and a
//! rotating cast of Sunday volunteers who run the live service without
//! touching those settings. See `docs/roles-permissions.md` and
//! `docs/phase-10-audit.md` for the full design record.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// A closed, two-value role set - matching this codebase's convention
/// for closed enums that model exactly what's needed now (`ContentType`,
/// `LicensingStatus`), not a general RBAC framework. A future role (e.g.
/// a read-only "Viewer") is a reasonable later addition, deliberately
/// not designed here - nothing in this phase needs it yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// May perform configuration/licensing/credential actions in
    /// addition to everything an `Operator` can do - see
    /// `docs/roles-permissions.md`'s exact command list.
    Admin,
    /// Day-to-day live-service operation only - search, display,
    /// approve/reject AI suggestions, start/pause/resume a service, and
    /// every other command not explicitly Admin-gated.
    Operator,
}

/// One local operator account. `pin_hash`/`pin_salt` are the only
/// persisted secret material - never sent to the frontend (see
/// `commands::OperatorAccountSummaryDto`, which deliberately omits
/// them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorAccount {
    pub id: String,
    pub display_name: String,
    pub role: Role,
    pub pin_hash: String,
    pub pin_salt: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum AccessError {
    #[error("operator account not found: {0}")]
    NotFound(String),
    #[error("an operator account named {0:?} already exists")]
    DuplicateDisplayName(String),
    #[error("invalid operator account data: {0}")]
    InvalidInput(String),
    #[error("access denied: {0}")]
    Forbidden(String),
    #[error("operator account storage error: {0}")]
    Storage(String),
}

/// The provider/adaptor contract for operator account storage -
/// implementations live in `integrations/access` (a local SQLite-backed
/// one, mirroring `ContentRegistry`/`BibleProvider`'s own trait/impl
/// split).
pub trait OperatorAccountStore: Send + Sync {
    /// Every account, in no particular guaranteed order beyond what the
    /// implementation documents - callers that need a stable order sort
    /// themselves.
    fn list(&self) -> Result<Vec<OperatorAccount>, AccessError>;

    fn get(&self, id: &str) -> Result<Option<OperatorAccount>, AccessError>;

    fn get_by_display_name(
        &self,
        display_name: &str,
    ) -> Result<Option<OperatorAccount>, AccessError>;

    /// Inserts a new account. Callers (see `create_operator_account` in
    /// `apps/desktop/src-tauri/src/access.rs`) are responsible for the
    /// bootstrap/role-authorization decision *before* calling this - this
    /// trait method itself only ever performs the storage write.
    fn create(&self, account: &OperatorAccount) -> Result<(), AccessError>;
}

/// A cryptographically random-enough salt for local PIN hashing, reusing
/// `uuid::Uuid::new_v4`'s own randomness source (already a workspace
/// dependency) rather than adding a new `rand` dependency for a single
/// call site.
pub fn generate_salt() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// `base64(sha256(salt || pin))` - the exact algorithm shape
/// `cip_integrations_obs::compute_auth_response` already uses for
/// obs-websocket auth (Phase 8), reused here rather than adding a new
/// dependency (`bcrypt`/`argon2`) for a threat model that doesn't need
/// their offline-brute-force resistance: this protects a PIN on a single
/// local desktop machine, not a networked credential (see
/// `docs/phase-10-audit.md`'s design-choice #2 for the full rationale).
pub fn hash_pin(pin: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(pin.as_bytes());
    base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        hasher.finalize(),
    )
}

pub fn verify_pin(pin: &str, salt: &str, expected_hash: &str) -> bool {
    hash_pin(pin, salt) == expected_hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_pin_is_deterministic_for_the_same_pin_and_salt() {
        assert_eq!(hash_pin("1234", "abc"), hash_pin("1234", "abc"));
    }

    #[test]
    fn hash_pin_differs_across_salts_for_the_same_pin() {
        assert_ne!(hash_pin("1234", "salt-a"), hash_pin("1234", "salt-b"));
    }

    #[test]
    fn hash_pin_differs_across_pins_for_the_same_salt() {
        assert_ne!(hash_pin("1234", "salt"), hash_pin("5678", "salt"));
    }

    #[test]
    fn verify_pin_accepts_the_correct_pin_and_rejects_a_wrong_one() {
        let salt = generate_salt();
        let hash = hash_pin("4242", &salt);
        assert!(verify_pin("4242", &salt, &hash));
        assert!(!verify_pin("0000", &salt, &hash));
    }

    #[test]
    fn generate_salt_produces_distinct_values() {
        assert_ne!(generate_salt(), generate_salt());
    }

    #[test]
    fn role_round_trips_through_json_as_snake_case() {
        let admin = serde_json::to_string(&Role::Admin).unwrap();
        assert_eq!(admin, "\"admin\"");
        let operator = serde_json::to_string(&Role::Operator).unwrap();
        assert_eq!(operator, "\"operator\"");
        assert_eq!(
            serde_json::from_str::<Role>("\"admin\"").unwrap(),
            Role::Admin
        );
    }
}
