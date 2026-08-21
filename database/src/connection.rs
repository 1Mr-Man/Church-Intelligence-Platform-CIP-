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
