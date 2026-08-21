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
}
