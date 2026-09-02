//! Local SQLite-backed [`OperatorAccountStore`]. Mirrors
//! `integrations/content::SqliteContentRegistry`'s shape: its own
//! connection, `Mutex`-guarded for interior mutability behind a shared
//! `&self`.

use chrono::{DateTime, Utc};
use cip_core_access::{AccessError, OperatorAccount, OperatorAccountStore, Role};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Mutex;

pub struct SqliteOperatorAccountStore {
    conn: Mutex<Connection>,
}

impl SqliteOperatorAccountStore {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }
}

fn role_str(role: Role) -> &'static str {
    match role {
        Role::Admin => "admin",
        Role::Operator => "operator",
    }
}

fn role_from_str(value: &str) -> Role {
    // Unrecognized/absent values fail closed to `Operator`, never the
    // more privileged `Admin` - a row this crate cannot positively
    // identify as Admin must never be treated as Admin.
    match value {
        "admin" => Role::Admin,
        _ => Role::Operator,
    }
}

const ROW_COLUMNS: &str = "id, display_name, role, pin_hash, pin_salt, created_at";

fn row_to_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperatorAccount> {
    let role_raw: String = row.get(2)?;
    let created_at_raw: String = row.get(5)?;
    Ok(OperatorAccount {
        id: row.get(0)?,
        display_name: row.get(1)?,
        role: role_from_str(&role_raw),
        pin_hash: row.get(3)?,
        pin_salt: row.get(4)?,
        created_at: DateTime::parse_from_rfc3339(&created_at_raw)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    })
}

impl OperatorAccountStore for SqliteOperatorAccountStore {
    fn list(&self) -> Result<Vec<OperatorAccount>, AccessError> {
        let conn = self
            .conn
            .lock()
            .expect("operator account connection poisoned");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {ROW_COLUMNS} FROM operator_accounts ORDER BY created_at"
            ))
            .map_err(|e| AccessError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map([], row_to_account)
            .map_err(|e| AccessError::Storage(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AccessError::Storage(e.to_string()))
    }

    fn get(&self, id: &str) -> Result<Option<OperatorAccount>, AccessError> {
        let conn = self
            .conn
            .lock()
            .expect("operator account connection poisoned");
        conn.query_row(
            &format!("SELECT {ROW_COLUMNS} FROM operator_accounts WHERE id = ?1"),
            params![id],
            row_to_account,
        )
        .optional()
        .map_err(|e| AccessError::Storage(e.to_string()))
    }

    fn get_by_display_name(
        &self,
        display_name: &str,
    ) -> Result<Option<OperatorAccount>, AccessError> {
        let conn = self
            .conn
            .lock()
            .expect("operator account connection poisoned");
        conn.query_row(
            &format!("SELECT {ROW_COLUMNS} FROM operator_accounts WHERE display_name = ?1"),
            params![display_name],
            row_to_account,
        )
        .optional()
        .map_err(|e| AccessError::Storage(e.to_string()))
    }

    fn create(&self, account: &OperatorAccount) -> Result<(), AccessError> {
        if account.id.trim().is_empty() || account.display_name.trim().is_empty() {
            return Err(AccessError::InvalidInput(
                "operator account id and display name must not be empty".to_string(),
            ));
        }
        let conn = self
            .conn
            .lock()
            .expect("operator account connection poisoned");
        conn.execute(
            "INSERT INTO operator_accounts (id, display_name, role, pin_hash, pin_salt, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                account.id,
                account.display_name,
                role_str(account.role),
                account.pin_hash,
                account.pin_salt,
                account.created_at.to_rfc3339(),
            ],
        )
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                AccessError::DuplicateDisplayName(account.display_name.clone())
            } else {
                AccessError::Storage(e.to_string())
            }
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migrated_store() -> SqliteOperatorAccountStore {
        let mut conn = cip_database::open_in_memory().unwrap();
        cip_database::run_migrations(&mut conn).unwrap();
        SqliteOperatorAccountStore::new(conn)
    }

    fn sample_account(id: &str, name: &str, role: Role) -> OperatorAccount {
        OperatorAccount {
            id: id.to_string(),
            display_name: name.to_string(),
            role,
            pin_hash: "hash".to_string(),
            pin_salt: "salt".to_string(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn creates_and_retrieves_an_account_by_id_and_display_name() {
        let store = migrated_store();
        store
            .create(&sample_account("op-1", "Pastor Sam", Role::Admin))
            .unwrap();

        let by_id = store.get("op-1").unwrap().unwrap();
        assert_eq!(by_id.display_name, "Pastor Sam");
        assert_eq!(by_id.role, Role::Admin);

        let by_name = store.get_by_display_name("Pastor Sam").unwrap().unwrap();
        assert_eq!(by_name.id, "op-1");
    }

    #[test]
    fn get_returns_none_for_an_unknown_id() {
        let store = migrated_store();
        assert!(store.get("nope").unwrap().is_none());
    }

    #[test]
    fn list_returns_every_account_oldest_first() {
        let store = migrated_store();
        store
            .create(&sample_account("op-1", "First", Role::Admin))
            .unwrap();
        store
            .create(&sample_account("op-2", "Second", Role::Operator))
            .unwrap();

        let all = store.list().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].display_name, "First");
        assert_eq!(all[1].display_name, "Second");
    }

    #[test]
    fn duplicate_display_names_are_rejected() {
        let store = migrated_store();
        store
            .create(&sample_account("op-1", "Same Name", Role::Admin))
            .unwrap();
        let result = store.create(&sample_account("op-2", "Same Name", Role::Operator));
        assert!(matches!(result, Err(AccessError::DuplicateDisplayName(_))));
    }

    #[test]
    fn rejects_an_account_with_an_empty_id_or_name() {
        let store = migrated_store();
        assert!(matches!(
            store.create(&sample_account("", "Someone", Role::Admin)),
            Err(AccessError::InvalidInput(_))
        ));
        assert!(matches!(
            store.create(&sample_account("op-1", "", Role::Admin)),
            Err(AccessError::InvalidInput(_))
        ));
    }

    #[test]
    fn role_round_trips_through_sqlite_for_both_variants() {
        let store = migrated_store();
        store
            .create(&sample_account("op-admin", "Admin One", Role::Admin))
            .unwrap();
        store
            .create(&sample_account(
                "op-operator",
                "Operator One",
                Role::Operator,
            ))
            .unwrap();

        assert_eq!(store.get("op-admin").unwrap().unwrap().role, Role::Admin);
        assert_eq!(
            store.get("op-operator").unwrap().unwrap().role,
            Role::Operator
        );
    }
}
