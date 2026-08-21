//! Tauri commands: the IPC surface the frontend calls.
//!
//! Deliberately minimal for Phase 1 - just enough to prove the frontend can
//! reach the config, the database, and the Bible provider through the real
//! managed state, without implementing any of the excluded features
//! (speech, sermon intelligence, presentation designer, etc).

use crate::config::AppConfig;
use crate::errors::AppError;
use crate::logging::LogCategory;
use crate::state::AppState;
use cip_core_bible::BibleTranslation;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub database_connected: bool,
    pub applied_migrations: usize,
    pub environment: crate::config::AppEnvironment,
}

/// Log an [`AppError`] under its category, then return it unchanged so
/// `?` in a command body both logs and propagates in one step.
fn log_and_return(err: AppError) -> AppError {
    log::error!(target: err.category().target(), "{err}");
    log::error!(target: LogCategory::Error.target(), "{err}");
    err
}

#[tauri::command]
pub fn get_app_config(state: State<'_, AppState>) -> AppConfig {
    state.config.clone()
}

#[tauri::command]
pub fn app_health_check(state: State<'_, AppState>) -> Result<HealthReport, AppError> {
    let db = state.db.lock().expect("db connection poisoned");
    let applied_migrations: usize = db
        .query_row("SELECT count(*) FROM schema_migrations", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|e| cip_database::DatabaseError::Migration(e.to_string()))
        .map_err(AppError::from)
        .map_err(log_and_return)? as usize;

    Ok(HealthReport {
        database_connected: true,
        applied_migrations,
        environment: state.config.environment,
    })
}

#[tauri::command]
pub fn list_bible_translations(
    state: State<'_, AppState>,
) -> Result<Vec<BibleTranslation>, AppError> {
    state
        .bible_provider
        .list_translations()
        .map_err(AppError::from)
        .map_err(log_and_return)
}
