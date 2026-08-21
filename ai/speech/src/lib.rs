//! `SpeechEngine` implementations.
//!
//! Phase 1 explicitly excludes real speech recognition. This crate ships
//! only [`NullSpeechEngine`], a stub that satisfies `cip_core_ai::SpeechEngine`
//! so the rest of the application (Tauri command wiring, event emission)
//! can be built and tested against a real trait object today. A local model
//! backend (e.g. whisper.cpp) will replace it without changing any caller,
//! since callers depend on the trait, not this crate.

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
