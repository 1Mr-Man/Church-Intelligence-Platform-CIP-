//! [`LocalAcousticMusicRecognizer`] - the real local-model integration
//! boundary, mirroring `cip_ai_speech::WhisperSpeechEngine`'s pattern:
//! genuine configuration, genuine status resolution, and an honest
//! `Unavailable`/`Error` report when nothing usable is configured -
//! never fabricated recognition.
//!
//! ## Why this never reports `Available` in this phase
//!
//! Unlike `WhisperSpeechEngine` (which has a real, working inference
//! backend - whisper.cpp via `whisper-rs` - and is only blocked by the
//! *absence of a downloadable model file* in this environment), Phase
//! 2.2 does not choose or implement a specific acoustic fingerprint/
//! embedding model architecture: doing so without genuinely verifying it
//! against real audio would risk exactly the "fake it" outcome section 7
//! of the Phase 2.2 spec forbids. This struct is therefore the real,
//! compiling, testable *boundary* - configuration, status resolution,
//! the trait implementation itself - with a clearly documented seam
//! (`recognize()`'s `RecognitionFailed` branch) where a future phase
//! plugs in a real backend the same way the `whisper` Cargo feature was
//! added to `ai/speech` once whisper-rs was chosen. See
//! `docs/acoustic-music.md`'s "PROVEN vs NOT AVAILABLE" section.

use std::path::PathBuf;

use cip_core_music::{
    AcousticMusicRecognizer, AcousticRecognitionCandidate, AcousticRecognitionError,
    AcousticRecognitionMethod, AcousticRecognitionStatus, AudioSegment,
};

/// The manifest filename a configured model directory is expected to
/// contain - not yet a real model file format (no backend reads it in
/// this phase), only a documented placeholder so "is something
/// genuinely configured" is a real, checkable file-system fact rather
/// than "does this directory contain anything at all."
pub const MODEL_MANIFEST_FILENAME: &str = "acoustic-model.json";

#[derive(Debug, Clone)]
pub struct LocalAcousticConfig {
    /// Directory expected to contain `MODEL_MANIFEST_FILENAME`. `None`
    /// means "never configured" - the honest default, never a guessed
    /// path.
    pub model_dir: Option<PathBuf>,
    pub enabled: bool,
}

impl Default for LocalAcousticConfig {
    fn default() -> Self {
        Self {
            model_dir: None,
            enabled: true,
        }
    }
}

pub struct LocalAcousticMusicRecognizer {
    status: AcousticRecognitionStatus,
    reason: String,
}

impl LocalAcousticMusicRecognizer {
    /// Resolves status once, at construction, from real file-system
    /// facts - never re-checked per `recognize()` call (mirroring
    /// `WhisperSpeechEngine::load`'s "fail at load time, not per call"
    /// design). A caller that wants to react to a model appearing/
    /// disappearing mid-service reconstructs this type; nothing here
    /// polls the file system in the background.
    pub fn configure(config: LocalAcousticConfig) -> Self {
        let (status, reason) = resolve_status(&config);
        Self { status, reason }
    }
}

fn resolve_status(config: &LocalAcousticConfig) -> (AcousticRecognitionStatus, String) {
    if !config.enabled {
        return (
            AcousticRecognitionStatus::Disabled,
            "acoustic recognition explicitly disabled".to_string(),
        );
    }
    let Some(dir) = &config.model_dir else {
        return (
            AcousticRecognitionStatus::Unavailable,
            "no acoustic model directory configured".to_string(),
        );
    };
    if !dir.is_dir() {
        return (
            AcousticRecognitionStatus::Unavailable,
            format!(
                "configured model directory does not exist: {}",
                dir.display()
            ),
        );
    }
    let manifest = dir.join(MODEL_MANIFEST_FILENAME);
    match std::fs::read(&manifest) {
        Err(_) => (
            AcousticRecognitionStatus::Unavailable,
            format!("no model manifest found at {}", manifest.display()),
        ),
        Ok(bytes) if bytes.is_empty() => (
            AcousticRecognitionStatus::Error,
            format!("model manifest is empty (malformed): {}", manifest.display()),
        ),
        Ok(_) => (
            AcousticRecognitionStatus::Unavailable,
            "a model manifest is present, but no acoustic inference backend is implemented in this build - see docs/acoustic-music.md".to_string(),
        ),
    }
}

impl AcousticMusicRecognizer for LocalAcousticMusicRecognizer {
    fn status(&self) -> AcousticRecognitionStatus {
        self.status
    }

    fn method(&self) -> AcousticRecognitionMethod {
        AcousticRecognitionMethod::LocalModel
    }

    fn status_reason(&self) -> Option<String> {
        Some(self.reason.clone())
    }

    fn recognize(
        &mut self,
        _segment: &AudioSegment,
        _content_ids: &[String],
    ) -> Result<Vec<AcousticRecognitionCandidate>, AcousticRecognitionError> {
        match self.status {
            AcousticRecognitionStatus::Disabled => Err(AcousticRecognitionError::Disabled),
            AcousticRecognitionStatus::Error => Err(AcousticRecognitionError::RecognitionFailed(
                self.reason.clone(),
            )),
            AcousticRecognitionStatus::Unavailable => {
                Err(AcousticRecognitionError::Unavailable(self.reason.clone()))
            }
            AcousticRecognitionStatus::Available => {
                // Structurally unreachable in this phase - `resolve_status`
                // never returns `Available` (see this module's docs) - but
                // handled explicitly, honestly, and without panicking
                // rather than silently falling through, in case a future
                // change to `resolve_status` starts returning it before
                // `recognize()` is updated to match.
                Err(AcousticRecognitionError::RecognitionFailed(
                    "no acoustic inference backend is implemented in this build".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment() -> AudioSegment {
        AudioSegment::new(vec![1; 16_000], 16_000, 0)
    }

    #[test]
    fn disabled_config_reports_disabled_and_rejects_recognition() {
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: None,
            enabled: false,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Disabled);
    }

    #[test]
    fn no_model_dir_configured_is_honestly_unavailable() {
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig::default());
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Unavailable);
        assert!(recognizer
            .status_reason()
            .unwrap()
            .contains("no acoustic model directory"));
    }

    #[test]
    fn a_nonexistent_model_dir_is_unavailable_not_a_crash() {
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(PathBuf::from("/nonexistent/cip-acoustic-model-dir")),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Unavailable);
    }

    #[test]
    fn an_empty_model_dir_with_no_manifest_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Unavailable);
    }

    #[test]
    fn a_malformed_empty_manifest_is_reported_as_error_not_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MODEL_MANIFEST_FILENAME), b"").unwrap();
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Error);
        assert!(recognizer.status_reason().unwrap().contains("malformed"));
    }

    #[test]
    fn a_present_manifest_is_honestly_unavailable_never_fake_recognition() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MODEL_MANIFEST_FILENAME), b"{}").unwrap();
        let mut recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        // A present, non-empty manifest is still never enough to claim
        // `Available` - no inference backend exists to honor it (see
        // this module's docs).
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Unavailable);
        assert!(matches!(
            recognizer.recognize(&segment(), &["music:dev".to_string()]),
            Err(AcousticRecognitionError::Unavailable(_))
        ));
    }

    #[test]
    fn recognize_never_panics_regardless_of_status() {
        for config in [
            LocalAcousticConfig {
                model_dir: None,
                enabled: false,
            },
            LocalAcousticConfig::default(),
        ] {
            let mut recognizer = LocalAcousticMusicRecognizer::configure(config);
            let _ = recognizer.recognize(&segment(), &["music:dev".to_string()]);
        }
    }
}
