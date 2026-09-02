//! Operator account / role orchestration (Phase 10). Deliberately Tauri-
//! agnostic (plain `&dyn OperatorAccountStore`/domain types, no
//! `AppHandle`/`State`), mirroring `content.rs`/`production.rs`/
//! `music.rs` - see their docs for why (this project's own no-`tauri::test`-
//! harness discipline: the pure core is tested directly, the thin
//! `#[tauri::command]` wrapper is not separately tested).

use chrono::Utc;
use cip_core_access::{
    hash_pin, verify_pin, AccessError, OperatorAccount, OperatorAccountStore, Role,
};

/// The currently-logged-in operator for this running process -
/// `AppState.current_operator` holds `Option<OperatorSession>`,
/// in-memory only, cleared on restart (see `docs/phase-10-audit.md`'s
/// design choice #3 - identical precedent to `screen_route_modes`).
#[derive(Debug, Clone, PartialEq)]
pub struct OperatorSession {
    pub id: String,
    pub display_name: String,
    pub role: Role,
}

impl From<&OperatorAccount> for OperatorSession {
    fn from(account: &OperatorAccount) -> Self {
        Self {
            id: account.id.clone(),
            display_name: account.display_name.clone(),
            role: account.role,
        }
    }
}

const MIN_PIN_LEN: usize = 4;

/// Refuses to proceed unless `current` is a logged-in Admin. Deliberately
/// the same fail-closed shape as `commands::ensure_ai_processing_permitted`
/// (Phase 9): no session, or a session that isn't Admin, is refused -
/// never inferred, never defaulted to permissive.
pub fn ensure_admin(current: &Option<OperatorSession>) -> Result<(), String> {
    match current {
        Some(session) if session.role == Role::Admin => Ok(()),
        Some(session) => Err(format!(
            "{} is logged in as Operator - this action requires an Admin account",
            session.display_name
        )),
        None => Err("no operator is logged in - log in first".to_string()),
    }
}

/// Creates a new operator account. Bootstrap rule (design choice #4,
/// `docs/phase-10-audit.md`): with zero accounts in the store, this
/// requires no login at all and the new account is always Admin
/// regardless of `requested_role` - there is no other way for the very
/// first account to ever become an Admin. Once at least one account
/// exists, the caller must already be a logged-in Admin.
pub fn create_operator_account(
    store: &dyn OperatorAccountStore,
    current: &Option<OperatorSession>,
    display_name: &str,
    pin: &str,
    requested_role: Role,
) -> Result<OperatorAccount, AccessError> {
    let display_name = display_name.trim();
    if display_name.is_empty() {
        return Err(AccessError::InvalidInput(
            "display name must not be empty".to_string(),
        ));
    }
    if pin.len() < MIN_PIN_LEN {
        return Err(AccessError::InvalidInput(format!(
            "PIN must be at least {MIN_PIN_LEN} characters"
        )));
    }

    let existing = store.list()?;
    let role = if existing.is_empty() {
        Role::Admin
    } else {
        ensure_admin(current).map_err(AccessError::Forbidden)?;
        requested_role
    };

    let salt = cip_core_access::generate_salt();
    let account = OperatorAccount {
        id: uuid::Uuid::new_v4().to_string(),
        display_name: display_name.to_string(),
        role,
        pin_hash: hash_pin(pin, &salt),
        pin_salt: salt,
        created_at: Utc::now(),
    };
    store.create(&account)?;
    Ok(account)
}

/// Verifies `pin` against `account_id`'s stored hash and returns the
/// resulting session on success. Never distinguishes "no such account"
/// from "wrong PIN" in its error text - see this function's own test for
/// why (not leaking which account ids exist to an unauthenticated
/// caller).
pub fn login(
    store: &dyn OperatorAccountStore,
    account_id: &str,
    pin: &str,
) -> Result<OperatorSession, AccessError> {
    let account = store.get(account_id)?;
    match account {
        Some(account) if verify_pin(pin, &account.pin_salt, &account.pin_hash) => {
            Ok(OperatorSession::from(&account))
        }
        _ => Err(AccessError::Forbidden(
            "incorrect account or PIN".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_database::{open_in_memory, run_migrations};
    use cip_integrations_access::SqliteOperatorAccountStore;

    fn migrated_store() -> SqliteOperatorAccountStore {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        SqliteOperatorAccountStore::new(conn)
    }

    #[test]
    fn ensure_admin_refuses_when_nobody_is_logged_in() {
        assert!(ensure_admin(&None).is_err());
    }

    #[test]
    fn ensure_admin_refuses_an_operator_role_session() {
        let session = OperatorSession {
            id: "op-1".to_string(),
            display_name: "Alex".to_string(),
            role: Role::Operator,
        };
        assert!(ensure_admin(&Some(session)).is_err());
    }

    #[test]
    fn ensure_admin_permits_an_admin_role_session() {
        let session = OperatorSession {
            id: "op-1".to_string(),
            display_name: "Pastor Sam".to_string(),
            role: Role::Admin,
        };
        assert!(ensure_admin(&Some(session)).is_ok());
    }

    #[test]
    fn the_first_account_ever_created_becomes_admin_with_no_login_required() {
        let store = migrated_store();
        let account =
            create_operator_account(&store, &None, "Pastor Sam", "4242", Role::Operator).unwrap();
        assert_eq!(
            account.role,
            Role::Admin,
            "the very first account must become Admin even though Operator was requested"
        );
    }

    #[test]
    fn creating_a_second_account_without_a_logged_in_admin_is_refused() {
        let store = migrated_store();
        create_operator_account(&store, &None, "Pastor Sam", "4242", Role::Admin).unwrap();

        let result = create_operator_account(&store, &None, "Alex", "1111", Role::Operator);
        assert!(matches!(result, Err(AccessError::Forbidden(_))));
    }

    #[test]
    fn creating_a_second_account_as_a_logged_in_operator_is_refused() {
        let store = migrated_store();
        create_operator_account(&store, &None, "Pastor Sam", "4242", Role::Admin).unwrap();

        let operator_session = Some(OperatorSession {
            id: "someone-else".to_string(),
            display_name: "Alex".to_string(),
            role: Role::Operator,
        });
        let result =
            create_operator_account(&store, &operator_session, "Jordan", "1111", Role::Operator);
        assert!(matches!(result, Err(AccessError::Forbidden(_))));
    }

    #[test]
    fn creating_a_second_account_as_a_logged_in_admin_succeeds_with_the_requested_role() {
        let store = migrated_store();
        let admin =
            create_operator_account(&store, &None, "Pastor Sam", "4242", Role::Admin).unwrap();
        let admin_session = Some(OperatorSession::from(&admin));

        let second =
            create_operator_account(&store, &admin_session, "Alex", "1111", Role::Operator)
                .unwrap();
        assert_eq!(second.role, Role::Operator);
    }

    #[test]
    fn create_operator_account_rejects_a_pin_shorter_than_the_minimum() {
        let store = migrated_store();
        let result = create_operator_account(&store, &None, "Pastor Sam", "12", Role::Admin);
        assert!(matches!(result, Err(AccessError::InvalidInput(_))));
    }

    #[test]
    fn create_operator_account_rejects_an_empty_display_name() {
        let store = migrated_store();
        let result = create_operator_account(&store, &None, "   ", "4242", Role::Admin);
        assert!(matches!(result, Err(AccessError::InvalidInput(_))));
    }

    #[test]
    fn login_succeeds_with_the_correct_pin_and_returns_the_right_role() {
        let store = migrated_store();
        let account =
            create_operator_account(&store, &None, "Pastor Sam", "4242", Role::Admin).unwrap();

        let session = login(&store, &account.id, "4242").unwrap();
        assert_eq!(session.display_name, "Pastor Sam");
        assert_eq!(session.role, Role::Admin);
    }

    #[test]
    fn login_fails_with_an_incorrect_pin() {
        let store = migrated_store();
        let account =
            create_operator_account(&store, &None, "Pastor Sam", "4242", Role::Admin).unwrap();

        assert!(login(&store, &account.id, "0000").is_err());
    }

    #[test]
    fn login_fails_for_an_unknown_account_id_with_the_same_error_shape_as_a_wrong_pin() {
        let store = migrated_store();
        create_operator_account(&store, &None, "Pastor Sam", "4242", Role::Admin).unwrap();

        let wrong_pin = login(&store, "some-real-id-would-go-here", "4242");
        assert!(matches!(wrong_pin, Err(AccessError::Forbidden(_))));
    }
}
