//! `SpeechEngine` implementations.
//!
//! - [`NullSpeechEngine`] - the safe default: reports itself not ready,
//!   rejects audio. What the application uses whenever no real engine is
//!   configured/available, so "no speech model" is never fatal.
//! - [`ScriptedSpeechEngine`] - a deterministic test/demo adapter (see its
//!   module docs) for exercising the audio -> speech -> Bible Intelligence
//!   wiring without a microphone or model.
//! - `WhisperSpeechEngine` (module `whisper`, behind the `whisper` Cargo
//!   feature) - a real local backend using whisper-rs/whisper.cpp. Off by
//!   default because compiling vendored whisper.cpp costs real build time
//!   this crate shouldn't impose on every default build; see its module
//!   docs and `docs/live-speech.md` for the model-download blocker
//!   encountered in this development environment.
//!
//! All three satisfy the same `cip_core_ai::SpeechEngine` trait - callers
//! never know which one they're holding.

mod scripted;
#[cfg(feature = "whisper")]
mod whisper;

pub use scripted::ScriptedSpeechEngine;
#[cfg(feature = "whisper")]
pub use whisper::WhisperSpeechEngine;

use cip_core_ai::{SpeechEngine, SpeechEngineError, TranscriptSegment};

/// The transcription languages CIP actually offers, deliberately
/// unconditional on the `whisper` Cargo feature (so command-layer input
/// validation compiles and behaves identically in every build) even
/// though only `WhisperSpeechEngine` (behind that feature) ever honors
/// a non-default selection.
///
/// Every code here is a real, verified entry in `whisper.cpp`'s own
/// 100-language vocabulary table (`g_lang` in
/// `whisper-rs-sys`'s vendored `whisper.cpp/src/whisper.cpp`) - not a
/// guess. Yoruba (`yo`) and Hausa (`ha`) are both real, trained Whisper
/// languages. **Igbo is deliberately absent**: `whisper.cpp`'s
/// vocabulary has no Igbo entry at all (checked directly against the
/// vendored source, not assumed), so no CIP-side configuration could
/// make a Whisper model correctly condition on it - see
/// `docs/phase-12-audit.md`'s "Verifying the premise before building
/// anything" for the full evidence. `"auto"` is `whisper-rs`'s own
/// documented literal for language auto-detection (equivalent to
/// passing `None`).
pub const SUPPORTED_LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("yo", "Yoruba"),
    ("ha", "Hausa"),
    ("auto", "Auto-detect"),
];

/// Whether `code` is one of [`SUPPORTED_LANGUAGES`]'s entries - the
/// single validation point `apps/desktop/src-tauri`'s `set_speech_language`
/// command uses, so the allow-list is never duplicated.
pub fn is_supported_language(code: &str) -> bool {
    SUPPORTED_LANGUAGES.iter().any(|(c, _)| *c == code)
}

#[derive(Default)]
pub struct NullSpeechEngine;

impl SpeechEngine for NullSpeechEngine {
    fn is_ready(&self) -> bool {
        false
    }

    fn feed_audio(
        &mut self,
        _samples: &[i16],
    ) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        Err(SpeechEngineError::NotInitialized)
    }

    fn flush(&mut self) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_engine_reports_not_ready_and_rejects_audio() {
        let mut engine = NullSpeechEngine;
        assert!(!engine.is_ready());
        assert!(matches!(
            engine.feed_audio(&[0; 4]),
            Err(SpeechEngineError::NotInitialized)
        ));
    }

    #[test]
    fn null_engine_satisfies_the_trait_object_contract() {
        let engine: Box<dyn SpeechEngine> = Box::new(NullSpeechEngine);
        assert!(!engine.is_ready());
    }

    // --- Phase 12: multi-language Whisper - the allow-list itself ---------

    #[test]
    fn supported_languages_are_exactly_english_yoruba_hausa_and_auto() {
        let codes: Vec<&str> = SUPPORTED_LANGUAGES.iter().map(|(c, _)| *c).collect();
        assert_eq!(codes, vec!["en", "yo", "ha", "auto"]);
    }

    #[test]
    fn igbo_is_never_offered_as_a_supported_language() {
        // See SUPPORTED_LANGUAGES's own docs: whisper.cpp's vocabulary has
        // no Igbo entry at all, verified against its vendored source -
        // this is a hard model limitation, not an oversight, and this
        // test guards against it being silently added back without that
        // same verification.
        assert!(!is_supported_language("ig"));
    }

    #[test]
    fn is_supported_language_accepts_every_listed_code() {
        for (code, _) in SUPPORTED_LANGUAGES {
            assert!(is_supported_language(code), "{code} should be supported");
        }
    }

    #[test]
    fn is_supported_language_rejects_an_unknown_code() {
        assert!(!is_supported_language("xx"));
        assert!(!is_supported_language(""));
    }
}
