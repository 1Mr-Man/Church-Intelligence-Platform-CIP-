use crate::DatabaseError;
use rusqlite::{params, Connection, OptionalExtension};

/// A single migration, embedded into the binary at compile time so the
/// desktop app never depends on migration files being present on disk at
/// runtime.
struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "0001_initial_schema",
        sql: include_str!("../migrations/0001_initial_schema.sql"),
    },
    Migration {
        version: 2,
        name: "0002_live_speech_detail",
        sql: include_str!("../migrations/0002_live_speech_detail.sql"),
    },
    Migration {
        version: 3,
        name: "0003_service_operations",
        sql: include_str!("../migrations/0003_service_operations.sql"),
    },
    Migration {
        version: 4,
        name: "0004_presentation_traceability",
        sql: include_str!("../migrations/0004_presentation_traceability.sql"),
    },
    Migration {
        version: 5,
        name: "0005_content_registry",
        sql: include_str!("../migrations/0005_content_registry.sql"),
    },
    Migration {
        version: 6,
        name: "0006_music_content",
        sql: include_str!("../migrations/0006_music_content.sql"),
    },
    Migration {
        version: 7,
        name: "0007_music_timeline_category",
        sql: include_str!("../migrations/0007_music_timeline_category.sql"),
    },
];

/// A migration that was applied during this call to [`run_migrations`].
/// (Migrations already applied in a previous run are not included.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMigration {
    pub version: i64,
    pub name: String,
}

fn ensure_migrations_table(conn: &Connection) -> Result<(), DatabaseError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    INTEGER PRIMARY KEY,
            name       TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );",
    )
    .map_err(|e| DatabaseError::Migration(e.to_string()))
}

fn is_applied(conn: &Connection, version: i64) -> Result<bool, DatabaseError> {
    conn.query_row(
        "SELECT 1 FROM schema_migrations WHERE version = ?1",
        params![version],
        |_| Ok(()),
    )
    .optional()
    .map(|row| row.is_some())
    .map_err(|e| DatabaseError::Migration(e.to_string()))
}

/// Apply every migration in `MIGRATIONS` that has not already been applied
/// to `conn`, in ascending version order, each inside its own transaction.
/// Idempotent: calling this on an already-current database is a no-op.
pub fn run_migrations(conn: &mut Connection) -> Result<Vec<AppliedMigration>, DatabaseError> {
    ensure_migrations_table(conn)?;
    let mut applied = Vec::new();

    for migration in MIGRATIONS {
        if is_applied(conn, migration.version)? {
            continue;
        }

        let tx = conn
            .transaction()
            .map_err(|e| DatabaseError::Migration(e.to_string()))?;
        tx.execute_batch(migration.sql)
            .map_err(|e| DatabaseError::Migration(format!("{}: {e}", migration.name)))?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
            params![migration.version, migration.name],
        )
        .map_err(|e| DatabaseError::Migration(e.to_string()))?;
        tx.commit()
            .map_err(|e| DatabaseError::Migration(e.to_string()))?;

        applied.push(AppliedMigration {
            version: migration.version,
            name: migration.name.to_string(),
        });
    }

    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::open_in_memory;

    #[test]
    fn applies_all_migrations_to_a_fresh_database() {
        let mut conn = open_in_memory().unwrap();
        let applied = run_migrations(&mut conn).unwrap();
        assert_eq!(applied.len(), MIGRATIONS.len());

        let table_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'services'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 1);
    }

    #[test]
    fn phase_1_2_columns_exist_after_migration() {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let has_column = |table: &str, column: &str| -> bool {
            conn.prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .filter_map(Result::ok)
                .any(|name| name == column)
        };

        for column in ["sequence_number", "language", "speaker_id"] {
            assert!(
                has_column("transcript_segments", column),
                "missing transcript_segments.{column}"
            );
        }
        for column in ["detection_type", "source_text"] {
            assert!(
                has_column("scripture_detections", column),
                "missing scripture_detections.{column}"
            );
        }
    }

    #[test]
    fn phase_1_3_suggestion_source_columns_and_timeline_index_exist_after_migration() {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let has_column = |table: &str, column: &str| -> bool {
            conn.prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .filter_map(Result::ok)
                .any(|name| name == column)
        };
        for column in ["transcript_segment_id", "source_text"] {
            assert!(
                has_column("ai_suggestions", column),
                "missing ai_suggestions.{column}"
            );
        }

        let index_exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_audit_events_service_created'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count == 1)
            .unwrap();
        assert!(index_exists, "missing idx_audit_events_service_created");
    }

    #[test]
    fn phase_1_4_presentation_traceability_columns_and_index_exist_after_migration() {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let has_column = |table: &str, column: &str| -> bool {
            conn.prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .filter_map(Result::ok)
                .any(|name| name == column)
        };
        for column in ["source_suggestion_id", "template"] {
            assert!(
                has_column("presentation_items", column),
                "missing presentation_items.{column}"
            );
        }

        let index_exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_presentation_items_source_suggestion'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count == 1)
            .unwrap();
        assert!(
            index_exists,
            "missing idx_presentation_items_source_suggestion"
        );
    }

    #[test]
    fn phase_1_5_content_registry_table_and_index_exist_after_migration() {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'content_registry'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count == 1)
            .unwrap();
        assert!(exists, "expected table `content_registry` to exist");

        let index_exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_content_registry_type'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count == 1)
            .unwrap();
        assert!(index_exists, "missing idx_content_registry_type");
    }

    #[test]
    fn running_migrations_twice_is_a_no_op_the_second_time() {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        let second_pass = run_migrations(&mut conn).unwrap();
        assert!(second_pass.is_empty());
    }

    #[test]
    fn all_ten_phase_1_domain_tables_exist_after_migration() {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        const EXPECTED_TABLES: [&str; 10] = [
            "services",
            "transcript_segments",
            "bible_translations",
            "bible_books",
            "bible_chapters",
            "bible_verses",
            "scripture_detections",
            "ai_suggestions",
            "presentation_items",
            "audit_events",
        ];

        for table in EXPECTED_TABLES {
            let exists: bool = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![table],
                    |row| row.get::<_, i64>(0),
                )
                .map(|count| count == 1)
                .unwrap();
            assert!(exists, "expected table `{table}` to exist after migration");
        }
    }

    #[test]
    fn phase_2_1_music_tables_and_indexes_exist_after_migration() {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        const EXPECTED_TABLES: [&str; 4] = [
            "music_songs",
            "music_aliases",
            "music_sections",
            "music_lyrics",
        ];
        for table in EXPECTED_TABLES {
            let exists: bool = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![table],
                    |row| row.get::<_, i64>(0),
                )
                .map(|count| count == 1)
                .unwrap();
            assert!(exists, "expected table `{table}` to exist after migration");
        }

        const EXPECTED_INDEXES: [&str; 5] = [
            "idx_music_songs_normalized_title",
            "idx_music_songs_number",
            "idx_music_aliases_normalized",
            "idx_music_sections_song",
            "idx_music_lyrics_normalized",
        ];
        for index in EXPECTED_INDEXES {
            let exists: bool = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
                    params![index],
                    |row| row.get::<_, i64>(0),
                )
                .map(|count| count == 1)
                .unwrap();
            assert!(exists, "missing index `{index}`");
        }
    }

    #[test]
    fn music_foreign_keys_are_enforced() {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

        let result = conn.execute(
            "INSERT INTO music_lyrics (content_id, song_id, section_id, sequence, text, normalized_text)
             VALUES ('music:dev', 'nonexistent-song', NULL, 0, 'x', 'x')",
            [],
        );
        assert!(
            result.is_err(),
            "a lyric line referencing a nonexistent song must be rejected"
        );
    }
}
