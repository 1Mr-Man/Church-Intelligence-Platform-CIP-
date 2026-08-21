//! Application configuration.
//!
//! Resolves the local data directory, database path, model directory, and
//! log directory CIP needs to run - all local filesystem paths, since CIP
//! is local-first and has no required cloud service to configure. No
//! secrets are read or stored here; a future networked integration
//! (`integrations/web`) is responsible for its own credential storage, out
//! of scope for Phase 1.

use serde::Serialize;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppEnvironment {
    Development,
    Test,
    Production,
}

impl AppEnvironment {
    /// Resolved from the `CIP_ENV` environment variable
    /// (`development` | `test` | `production`, case-insensitive), falling
    /// back to `Development` for debug builds and `Production` for release
    /// builds when unset.
    pub fn resolve() -> Self {
        match std::env::var("CIP_ENV")
            .ok()
            .as_deref()
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("development") => AppEnvironment::Development,
            Some("test") => AppEnvironment::Test,
            Some("production") => AppEnvironment::Production,
            _ if cfg!(debug_assertions) => AppEnvironment::Development,
            _ => AppEnvironment::Production,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to resolve app data directory: {0}")]
    DataDir(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub environment: AppEnvironment,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub model_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl AppConfig {
    /// Resolve configuration from Tauri's app data directory (the real
    /// startup path - see `lib.rs`'s `setup` hook).
    pub fn resolve(app: &tauri::AppHandle) -> Result<Self, ConfigError> {
        use tauri::Manager;
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| ConfigError::DataDir(e.to_string()))?;
        Ok(Self::from_data_dir(data_dir))
    }

    /// Build configuration from an explicit base directory, bypassing the
    /// Tauri runtime entirely. Used by tests and any tooling that needs an
    /// `AppConfig` without a running app.
    pub fn from_data_dir(data_dir: PathBuf) -> Self {
        Self {
            environment: AppEnvironment::resolve(),
            database_path: data_dir.join("cip.sqlite3"),
            model_dir: data_dir.join("models"),
            log_dir: data_dir.join("logs"),
            data_dir,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_all_paths_from_a_single_data_dir() {
        let config = AppConfig::from_data_dir(PathBuf::from("/tmp/cip-test"));
        assert_eq!(
            config.database_path,
            PathBuf::from("/tmp/cip-test/cip.sqlite3")
        );
        assert_eq!(config.model_dir, PathBuf::from("/tmp/cip-test/models"));
        assert_eq!(config.log_dir, PathBuf::from("/tmp/cip-test/logs"));
    }
}
