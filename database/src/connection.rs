use crate::DatabaseError;
use rusqlite::Connection;
use std::path::Path;

/// Open (creating if necessary) a SQLite database at `path` with the
/// pragmas CIP relies on for a local-first, single-writer desktop app.
///
/// - `foreign_keys = ON`: SQLite disables FK enforcement by default; every
///   migration in this workspace assumes it is on.
/// - `journal_mode = WAL`: lets the UI keep reading while a background
///   write (e.g. an incoming transcript segment) is in flight.
pub fn open(path: &Path) -> Result<Connection, DatabaseError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| DatabaseError::Io(e.to_string()))?;
    }
    let conn = Connection::open(path).map_err(|e| DatabaseError::Connection(e.to_string()))?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
        .map_err(|e| DatabaseError::Connection(e.to_string()))?;
    Ok(conn)
}

/// Open an in-memory database. Used by tests and anywhere else that needs a
/// throwaway database without touching disk.
pub fn open_in_memory() -> Result<Connection, DatabaseError> {
    let conn =
        Connection::open_in_memory().map_err(|e| DatabaseError::Connection(e.to_string()))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|e| DatabaseError::Connection(e.to_string()))?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Phase 3.1 failure-injection gap #19: a genuinely unopenable database
    /// path (here, a path that is actually a directory - the same shape of
    /// failure real disk/permission problems on a pilot machine take, and
    /// one that reproduces deterministically regardless of which user CIP
    /// runs as) must be reported as a clean [`DatabaseError::Connection`],
    /// never a panic.
    #[test]
    fn an_unopenable_database_path_is_reported_as_a_connection_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!(
            "cip-unopenable-db-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        // `dir` itself is a directory, not a file SQLite can open - a real,
        // reachable failure shape (a stale directory left behind by a
        // previous crash, a misconfigured `CIP_...` path, etc.).
        let result = open(&dir);

        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            matches!(result, Err(DatabaseError::Connection(_))),
            "opening a directory as a database file must fail cleanly, got {result:?}"
        );
    }
}
