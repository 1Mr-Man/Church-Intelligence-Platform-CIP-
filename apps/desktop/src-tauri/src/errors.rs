//! Top-level application error type.
//!
//! Every fallible Tauri command returns `Result<T, AppError>`. `AppError`
//! wraps the lower-level errors each domain already defines (rather than
//! flattening them into strings early) and adds a [`LogCategory`] so the
//! command dispatch layer can log consistently before turning the error
//! into whatever the frontend sees.

use crate::config::ConfigError;
use crate::logging::LogCategory;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Database(#[from] cip_database::DatabaseError),
    #[error(transparent)]
    BibleProvider(#[from] cip_core_bible::BibleProviderError),
}

impl AppError {
    pub fn category(&self) -> LogCategory {
        match self {
            AppError::Config(_) => LogCategory::App,
            AppError::Database(_) => LogCategory::Database,
            AppError::BibleProvider(_) => LogCategory::Bible,
        }
    }
}

/// Tauri commands must return a `Serialize` error type; `AppError`'s
/// variants carry non-serializable inner errors (e.g. `rusqlite::Error`
/// wrapped in `thiserror`), so this serializes as a plain message string.
/// The full error (with source chain) is still logged via `category()`
/// before this conversion happens - nothing is lost, only flattened for
/// the IPC boundary.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_errors_are_categorized_under_database() {
        let err = AppError::Database(cip_database::DatabaseError::Connection("boom".into()));
        assert_eq!(err.category(), LogCategory::Database);
    }

    #[test]
    fn bible_provider_errors_are_categorized_under_bible() {
        let err = AppError::BibleProvider(cip_core_bible::BibleProviderError::Unavailable(
            "boom".into(),
        ));
        assert_eq!(err.category(), LogCategory::Bible);
    }
}
