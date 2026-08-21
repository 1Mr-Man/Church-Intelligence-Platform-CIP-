//! Local-first SQLite foundation.
//!
//! CIP has no required cloud database, no Supabase dependency, and no
//! external database server requirement: every desktop install owns a
//! single SQLite file on local disk (see `apps/desktop/src-tauri/src/config.rs`
//! for how its path is resolved). This crate is the only place that opens a
//! connection to it or runs schema migrations.

mod connection;
mod migrations;
pub mod seed;

pub use connection::{open, open_in_memory};
pub use migrations::{run_migrations, AppliedMigration};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("failed to prepare database directory: {0}")]
    Io(String),
    #[error("failed to open database connection: {0}")]
    Connection(String),
    #[error("migration failed: {0}")]
    Migration(String),
}
