//! Managed Tauri application state.
//!
//! One `AppState` is constructed during `setup` (see `lib.rs`) and handed to
//! every command via `tauri::State`. It holds the resolved config and the
//! single SQLite connection - CIP is a single-writer desktop app, so one
//! mutex-guarded connection is the right level of concurrency control for
//! Phase 1, rather than a connection pool sized for a multi-client server.

use crate::config::AppConfig;
use cip_core_bible::BibleProvider;
use std::sync::Mutex;

pub struct AppState {
    pub config: AppConfig,
    pub db: Mutex<rusqlite::Connection>,
    pub bible_provider: Box<dyn BibleProvider>,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        db: rusqlite::Connection,
        bible_provider: Box<dyn BibleProvider>,
    ) -> Self {
        Self {
            config,
            db: Mutex::new(db),
            bible_provider,
        }
    }
}
