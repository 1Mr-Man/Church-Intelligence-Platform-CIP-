use crate::DatabaseError;
use rusqlite::Connection;

const DEV_SEED: &str = include_str!("../seeds/dev_seed.sql");

/// Apply the development/test seed data (a handful of verses, one service)
/// on top of an already-migrated database. Never called automatically -
/// Phase 1 explicitly does not ship a populated Bible dataset.
pub fn apply_dev_seed(conn: &Connection) -> Result<(), DatabaseError> {
    conn.execute_batch(DEV_SEED)
        .map_err(|e| DatabaseError::Migration(format!("dev seed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{connection::open_in_memory, migrations::run_migrations};

    #[test]
    fn dev_seed_applies_cleanly_after_migrations() {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        apply_dev_seed(&conn).unwrap();

        let verse_count: i64 = conn
            .query_row("SELECT count(*) FROM bible_verses", [], |row| row.get(0))
            .unwrap();
        assert_eq!(verse_count, 3);

        let text: String = conn
            .query_row(
                "SELECT text FROM bible_verses WHERE book_code = 'ROM' AND chapter_number = 8 AND verse_number = 28",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(text.starts_with("And we know that all things work together for good"));
    }
}
