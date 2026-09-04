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
pub const WHISPER_MODEL_FILENAME: &str = "ggml-tiny.en.bin";

/// Phase 24.3 (true dual-tier Whisper): the expected filename of a second,
/// independent local ggml/gguf Whisper model - a larger, more accurate
/// model than [`WHISPER_MODEL_FILENAME`]'s fast/tiny default, run
/// concurrently to produce a slower, higher-quality re-transcription of
/// speech the fast tier already showed the operator. Never bundled with
/// CIP, never downloaded automatically, and entirely optional: an operator
/// who never installs a file at `whisper_quality_model_path` simply never
/// gets the quality tier - the fast tier alone keeps working exactly as it
/// always has (see `docs/phase-24-3-audit.md`). `base.en`, not `tiny.en`,
/// since a "quality" tier that defaulted to the same tiny-class model the
/// fast tier already uses would add real CPU cost for no accuracy gain.
pub const WHISPER_QUALITY_MODEL_FILENAME: &str = "ggml-base.en.bin";

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

/// Expected filename of the local embedding model weights under
/// `AppConfig::model_dir`, for Phase 4.4's semantic (meaning-based) Bible
/// search - the `cip-ai-embeddings` counterpart to `WHISPER_MODEL_FILENAME`.
/// Not bundled with CIP and never downloaded automatically - see
/// `docs/phase-4-4-semantic-bible-search.md` for where to get it, its
/// license, and why this environment could not verify one end to end.
pub const EMBEDDING_MODEL_FILENAME: &str = "model.safetensors";

/// Expected filename of the embedding model's tokenizer under
/// `AppConfig::model_dir`, alongside `EMBEDDING_MODEL_FILENAME`. Both files
/// come from the same Hugging Face model page and must be installed
/// together - a model file without its matching tokenizer (or vice versa)
/// cannot produce meaningful embeddings.
pub const EMBEDDING_TOKENIZER_FILENAME: &str = "tokenizer.json";

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
    /// Phase 3.0: the exact file `create_speech_engine` will try to load a
    /// local Whisper model from. Defaults to
    /// `model_dir/WHISPER_MODEL_FILENAME`, but - mirroring
    /// `AcousticConfig::model_dir`'s existing `CIP_ACOUSTIC_MODEL_DIR`
    /// precedent - is overridable via `CIP_WHISPER_MODEL_PATH` so an
    /// operator can point CIP at a model stored anywhere (a different
    /// filename, a shared/read-only location, a differently-quantized
    /// build) without rebuilding from source. Serialized to the frontend
    /// (via `get_app_config`) so a "speech unavailable" notice can name the
    /// exact path it looked for, never a vague "not configured."
    pub whisper_model_path: PathBuf,
    /// Phase 24.3: the exact file `create_quality_speech_engine` will try
    /// to load a second, independent Whisper model from - mirrors
    /// `whisper_model_path` exactly, including its `CIP_WHISPER_MODEL_PATH`
    /// precedent (`CIP_WHISPER_QUALITY_MODEL_PATH` here). Missing/invalid
    /// is never fatal: the quality tier is purely additive, see
    /// `docs/phase-24-3-audit.md`.
    pub whisper_quality_model_path: PathBuf,
    /// Phase 4.4: the exact file `create_embedding_engine` will try to load
    /// local embedding model weights from - mirrors `whisper_model_path`
    /// exactly, including its `CIP_EMBEDDING_MODEL_PATH` override.
    pub embedding_model_path: PathBuf,
    /// Phase 4.4: the exact file `create_embedding_engine` will try to load
    /// the embedding model's tokenizer from - `CIP_EMBEDDING_TOKENIZER_PATH`
    /// overridable, mirroring `embedding_model_path`.
    pub embedding_tokenizer_path: PathBuf,
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
        let whisper_model_path = std::env::var("CIP_WHISPER_MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| model_dir.join(WHISPER_MODEL_FILENAME));
        let whisper_quality_model_path = std::env::var("CIP_WHISPER_QUALITY_MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| model_dir.join(WHISPER_QUALITY_MODEL_FILENAME));
        let embedding_model_path = std::env::var("CIP_EMBEDDING_MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| model_dir.join(EMBEDDING_MODEL_FILENAME));
        let embedding_tokenizer_path = std::env::var("CIP_EMBEDDING_TOKENIZER_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| model_dir.join(EMBEDDING_TOKENIZER_FILENAME));
        Self {
            environment: AppEnvironment::resolve(),
            database_path: data_dir.join("cip.sqlite3"),
            model_dir,
            whisper_model_path,
            whisper_quality_model_path,
            embedding_model_path,
            embedding_tokenizer_path,
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

    /// Phase 3.0: `CIP_WHISPER_MODEL_PATH` must be able to point the
    /// speech engine at a model stored anywhere, not just
    /// `model_dir/WHISPER_MODEL_FILENAME` - the same override capability
    /// `CIP_ACOUSTIC_MODEL_DIR` already gives the acoustic recognizer.
    /// Serialized via `std::env::set_var`/`remove_var` around the call so
    /// this test cannot leak state into any other test in this binary
    /// (`cargo test` runs a crate's tests in one process, potentially in
    /// parallel threads, but never this one concurrently with itself).
    #[test]
    fn whisper_model_path_defaults_under_model_dir_when_unset() {
        // SAFETY: no other test in this crate reads or writes
        // CIP_WHISPER_MODEL_PATH, so removing it here cannot race.
        unsafe {
            std::env::remove_var("CIP_WHISPER_MODEL_PATH");
        }
        let config = AppConfig::from_data_dir(PathBuf::from("/tmp/cip-test-default-whisper"));
        assert_eq!(
            config.whisper_model_path,
            PathBuf::from("/tmp/cip-test-default-whisper/models/ggml-tiny.en.bin")
        );
    }

    #[test]
    fn whisper_model_path_honors_the_env_override() {
        // SAFETY: this test sets then immediately removes the var within
        // its own body, and no other test in this crate touches it.
        unsafe {
            std::env::set_var("CIP_WHISPER_MODEL_PATH", "/opt/models/my-whisper.bin");
        }
        let config = AppConfig::from_data_dir(PathBuf::from("/tmp/cip-test-override-whisper"));
        unsafe {
            std::env::remove_var("CIP_WHISPER_MODEL_PATH");
        }
        assert_eq!(
            config.whisper_model_path,
            PathBuf::from("/opt/models/my-whisper.bin"),
            "an operator-supplied path must be used verbatim, never merged with model_dir"
        );
    }

    /// Phase 24.3: mirrors `whisper_model_path_defaults_under_model_dir_when_unset`
    /// for the second, quality-tier model.
    #[test]
    fn whisper_quality_model_path_defaults_under_model_dir_when_unset() {
        // SAFETY: no other test in this crate reads or writes
        // CIP_WHISPER_QUALITY_MODEL_PATH, so removing it here cannot race.
        unsafe {
            std::env::remove_var("CIP_WHISPER_QUALITY_MODEL_PATH");
        }
        let config =
            AppConfig::from_data_dir(PathBuf::from("/tmp/cip-test-default-whisper-quality"));
        assert_eq!(
            config.whisper_quality_model_path,
            PathBuf::from("/tmp/cip-test-default-whisper-quality/models/ggml-base.en.bin")
        );
    }

    #[test]
    fn whisper_quality_model_path_honors_the_env_override() {
        // SAFETY: this test sets then immediately removes the var within
        // its own body, and no other test in this crate touches it.
        unsafe {
            std::env::set_var(
                "CIP_WHISPER_QUALITY_MODEL_PATH",
                "/opt/models/my-whisper-quality.bin",
            );
        }
        let config =
            AppConfig::from_data_dir(PathBuf::from("/tmp/cip-test-override-whisper-quality"));
        unsafe {
            std::env::remove_var("CIP_WHISPER_QUALITY_MODEL_PATH");
        }
        assert_eq!(
            config.whisper_quality_model_path,
            PathBuf::from("/opt/models/my-whisper-quality.bin"),
            "an operator-supplied path must be used verbatim, never merged with model_dir"
        );
    }

    /// Phase 4.4: mirrors `whisper_model_path_defaults_under_model_dir_when_unset`
    /// for the embedding model/tokenizer pair.
    #[test]
    fn embedding_paths_default_under_model_dir_when_unset() {
        // SAFETY: no other test in this crate reads or writes these two
        // vars, so removing them here cannot race.
        unsafe {
            std::env::remove_var("CIP_EMBEDDING_MODEL_PATH");
            std::env::remove_var("CIP_EMBEDDING_TOKENIZER_PATH");
        }
        let config = AppConfig::from_data_dir(PathBuf::from("/tmp/cip-test-default-embedding"));
        assert_eq!(
            config.embedding_model_path,
            PathBuf::from("/tmp/cip-test-default-embedding/models/model.safetensors")
        );
        assert_eq!(
            config.embedding_tokenizer_path,
            PathBuf::from("/tmp/cip-test-default-embedding/models/tokenizer.json")
        );
    }

    #[test]
    fn embedding_paths_honor_env_overrides() {
        // SAFETY: this test sets then immediately removes both vars within
        // its own body, and no other test in this crate touches them.
        unsafe {
            std::env::set_var(
                "CIP_EMBEDDING_MODEL_PATH",
                "/opt/models/my-embed.safetensors",
            );
            std::env::set_var("CIP_EMBEDDING_TOKENIZER_PATH", "/opt/models/my-tok.json");
        }
        let config = AppConfig::from_data_dir(PathBuf::from("/tmp/cip-test-override-embedding"));
        unsafe {
            std::env::remove_var("CIP_EMBEDDING_MODEL_PATH");
            std::env::remove_var("CIP_EMBEDDING_TOKENIZER_PATH");
        }
        assert_eq!(
            config.embedding_model_path,
            PathBuf::from("/opt/models/my-embed.safetensors")
        );
        assert_eq!(
            config.embedding_tokenizer_path,
            PathBuf::from("/opt/models/my-tok.json")
        );
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
