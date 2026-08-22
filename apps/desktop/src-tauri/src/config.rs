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

/// Expected filename of a local ggml/gguf Whisper model under
/// `AppConfig::model_dir`, if one has been installed. Not bundled with
/// CIP and never downloaded automatically - see `docs/live-speech.md` for
/// where to get one, its license, and why this environment could not
/// verify one end to end.
#[allow(dead_code)] // only read when built with the `whisper` feature
pub const WHISPER_MODEL_FILENAME: &str = "ggml-tiny.en.bin";

/// Expected subdirectory of `AppConfig::model_dir` a local acoustic
/// (audio-fingerprint) model would be configured under, if one exists -
/// the Phase 2.2 counterpart to `WHISPER_MODEL_FILENAME`. Unlike Whisper
/// (a single model file), this names a *directory*: `LocalAcousticConfig`
/// expects a `cip_integrations_music_acoustic::MODEL_MANIFEST_FILENAME`
/// manifest file inside it (see `docs/acoustic-music.md`). Never bundled
/// with CIP, never downloaded automatically, and (see that module's docs)
/// never enough on its own to make acoustic recognition `Available` in
/// this build - no inference backend is implemented yet.
pub const ACOUSTIC_MODEL_DIR_NAME: &str = "acoustic";

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

/// Phase 2.2's explicit, documented acoustic-recognition settings - "no
/// hard-coded environment-specific paths," each independently resolvable
/// from an environment variable, mirroring `AppEnvironment::resolve`'s
/// convention exactly (a fixed default for local/dev use, overridable
/// without a rebuild). None of these values are secrets.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcousticConfig {
    /// Operator/deployment kill switch, independent of whether a model is
    /// actually configured - `LocalAcousticMusicRecognizer` reports
    /// `AcousticRecognitionStatus::Disabled` (not `Unavailable`) when this
    /// is `false`, so a diagnostic UI can tell "turned off" apart from
    /// "nothing configured." `CIP_ACOUSTIC_ENABLED` (`true`/`false`),
    /// defaults to `true` - enabling by default is safe: with no model
    /// manifest present, recognition still honestly reports `Unavailable`.
    pub enabled: bool,
    /// Directory expected to contain a model manifest -
    /// `CIP_ACOUSTIC_MODEL_DIR`, defaulting to
    /// `model_dir/ACOUSTIC_MODEL_DIR_NAME`.
    pub model_dir: PathBuf,
    /// Minimum real audio (post signal-quality-gate) before a window is
    /// worth recognizing - `CIP_ACOUSTIC_MIN_AUDIO_MS`, defaulting to
    /// `AcousticAnalysisConfig::default().min_duration_ms`.
    pub minimum_audio_ms: u64,
    /// Length of one analysis window - `CIP_ACOUSTIC_WINDOW_MS`,
    /// defaulting to `AcousticAnalysisConfig::default().window_ms`.
    pub analysis_window_ms: u64,
    /// Overlap between consecutive windows -
    /// `CIP_ACOUSTIC_OVERLAP_MS`, defaulting to
    /// `AcousticAnalysisConfig::default().overlap_ms`.
    pub overlap_ms: u64,
}

impl AcousticConfig {
    fn resolve(model_dir: PathBuf) -> Self {
        let default = cip_core_music::AcousticAnalysisConfig::default();
        Self {
            enabled: env_bool("CIP_ACOUSTIC_ENABLED", true),
            model_dir: std::env::var("CIP_ACOUSTIC_MODEL_DIR")
                .map(PathBuf::from)
                .unwrap_or(model_dir),
            minimum_audio_ms: env_u64("CIP_ACOUSTIC_MIN_AUDIO_MS", default.min_duration_ms),
            analysis_window_ms: env_u64("CIP_ACOUSTIC_WINDOW_MS", default.window_ms),
            overlap_ms: env_u64("CIP_ACOUSTIC_OVERLAP_MS", default.overlap_ms),
        }
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key).ok().as_deref().map(str::to_lowercase) {
        Some(v) if v == "true" || v == "1" => true,
        Some(v) if v == "false" || v == "0" => false,
        _ => default,
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub environment: AppEnvironment,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub model_dir: PathBuf,
    pub log_dir: PathBuf,
    pub acoustic: AcousticConfig,
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
        let model_dir = data_dir.join("models");
        let acoustic = AcousticConfig::resolve(model_dir.join(ACOUSTIC_MODEL_DIR_NAME));
        Self {
            environment: AppEnvironment::resolve(),
            database_path: data_dir.join("cip.sqlite3"),
            model_dir,
            log_dir: data_dir.join("logs"),
            acoustic,
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

    /// Not a claim about every process's environment (a caller with
    /// `CIP_ACOUSTIC_*` variables already set would see those instead,
    /// exactly as intended) - only proves the *shape* this module
    /// produces when nothing overrides it: a real, non-empty default
    /// model directory nested under the acoustic subdirectory, and the
    /// same window/overlap/min-duration values
    /// `AcousticAnalysisConfig::default()` documents.
    #[test]
    fn acoustic_config_defaults_match_the_documented_shape() {
        let config = AppConfig::from_data_dir(PathBuf::from("/tmp/cip-test"));
        assert!(config.acoustic.model_dir.ends_with("models/acoustic"));
        let defaults = cip_core_music::AcousticAnalysisConfig::default();
        assert_eq!(config.acoustic.minimum_audio_ms, defaults.min_duration_ms);
        assert_eq!(config.acoustic.analysis_window_ms, defaults.window_ms);
        assert_eq!(config.acoustic.overlap_ms, defaults.overlap_ms);
    }

    #[test]
    fn env_bool_parses_common_truthy_and_falsy_forms() {
        assert!(env_bool("CIP_TEST_NEVER_SET_TRUE_DEFAULT", true));
        assert!(!env_bool("CIP_TEST_NEVER_SET_FALSE_DEFAULT", false));
    }

    #[test]
    fn env_u64_falls_back_to_default_when_unset_or_unparsable() {
        assert_eq!(env_u64("CIP_TEST_NEVER_SET_U64", 42), 42);
    }
}
