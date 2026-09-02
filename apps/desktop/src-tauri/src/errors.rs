//! Top-level application error type.
//!
//! Every fallible Tauri command returns `Result<T, AppError>`. `AppError`
//! wraps the lower-level errors each domain already defines (rather than
//! flattening them into strings early) and adds a [`LogCategory`] so the
//! command dispatch layer can log consistently before turning the error
//! into whatever the frontend sees.

use crate::config::ConfigError;
use crate::logging::LogCategory;
use crate::persistence::PersistError;
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
    #[error(transparent)]
    Persistence(#[from] PersistError),
    #[error(transparent)]
    AudioEngine(#[from] cip_core_service::AudioEngineError),
    #[error(transparent)]
    SpeechEngine(#[from] cip_core_ai::SpeechEngineError),
    #[error(transparent)]
    Presentation(#[from] crate::presentation::PresentationError),
    #[error(transparent)]
    Content(#[from] crate::content::ContentError),
    #[error(transparent)]
    ContentRegistry(#[from] cip_core_content::ContentRegistryError),
    #[error(transparent)]
    BibleSearch(#[from] cip_core_bible::BibleSearchError),
    #[error(transparent)]
    Music(#[from] crate::music::MusicError),
    #[error(transparent)]
    MusicProvider(#[from] cip_core_music::MusicProviderError),
    #[error(transparent)]
    Intelligence(#[from] cip_core_intelligence::IntelligenceError),
    #[error(transparent)]
    ContextBuild(#[from] crate::intelligence::ContextBuildError),
    #[error("no service is currently active - start one first")]
    NoActiveService,
    #[error("no sermon is currently active - start one first")]
    NoActiveSermon,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// Phase 10: a command requiring a specific operator role (usually
    /// Admin) was called without one - see `access::ensure_admin`.
    #[error("{0}")]
    Forbidden(String),
    /// Phase 11: the companion HTTP server could not be started.
    #[error(transparent)]
    Companion(#[from] crate::companion::CompanionError),
}

impl AppError {
    pub fn category(&self) -> LogCategory {
        match self {
            AppError::Config(_) => LogCategory::App,
            AppError::Database(_) => LogCategory::Database,
            AppError::BibleProvider(_) => LogCategory::Bible,
            AppError::Persistence(_) => LogCategory::Database,
            AppError::AudioEngine(_) => LogCategory::Audio,
            AppError::SpeechEngine(_) => LogCategory::Speech,
            AppError::Presentation(_) => LogCategory::Presentation,
            AppError::Content(_) => LogCategory::Content,
            AppError::ContentRegistry(_) => LogCategory::Content,
            AppError::BibleSearch(_) => LogCategory::Bible,
            AppError::Music(_) => LogCategory::Music,
            AppError::MusicProvider(_) => LogCategory::Music,
            AppError::Intelligence(_) => LogCategory::Music,
            AppError::ContextBuild(_) => LogCategory::Music,
            AppError::NoActiveService => LogCategory::App,
            AppError::NoActiveSermon => LogCategory::App,
            AppError::InvalidInput(_) => LogCategory::App,
            AppError::Forbidden(_) => LogCategory::Security,
            AppError::Companion(_) => LogCategory::Network,
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

    #[test]
    fn audio_engine_errors_are_categorized_under_audio() {
        let err = AppError::AudioEngine(cip_core_service::AudioEngineError::NoDevice);
        assert_eq!(err.category(), LogCategory::Audio);
    }

    #[test]
    fn speech_engine_errors_are_categorized_under_speech() {
        let err = AppError::SpeechEngine(cip_core_ai::SpeechEngineError::NotInitialized);
        assert_eq!(err.category(), LogCategory::Speech);
    }

    #[test]
    fn no_active_service_serializes_to_a_clear_message() {
        let err = AppError::NoActiveService;
        let value = serde_json::to_value(&err).unwrap();
        assert!(value
            .as_str()
            .unwrap()
            .contains("no service is currently active"));
    }
}
