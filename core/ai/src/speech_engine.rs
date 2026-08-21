use cip_core_confidence::ConfidenceResult;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One incremental piece of transcribed speech. `is_final` distinguishes a
/// still-changing partial hypothesis from a settled segment, mirroring how
/// streaming speech recognizers typically report results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub text: String,
    pub is_final: bool,
    pub confidence: ConfidenceResult,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Error)]
pub enum SpeechEngineError {
    #[error("speech engine not initialized")]
    NotInitialized,
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("transcription failed: {0}")]
    TranscriptionFailed(String),
}

/// The provider/adaptor contract for speech-to-text. `core` depends only on
/// this trait; `ai/speech` supplies concrete implementations (a local model
/// first - no cloud dependency is required for CIP to function offline).
///
/// Implementations consume raw audio frames (format/source is an
/// implementation detail negotiated with whatever `AudioEngine` is active)
/// and emit `TranscriptSegment`s. Phase 1 defines this contract only; no
/// implementation is wired in yet.
pub trait SpeechEngine: Send + Sync {
    fn is_ready(&self) -> bool;

    /// Feed one chunk of raw audio (implementation-defined encoding) and
    /// receive zero or more transcript segments produced so far.
    fn feed_audio(&mut self, samples: &[i16]) -> Result<Vec<TranscriptSegment>, SpeechEngineError>;

    /// Flush any buffered audio and finalize the current segment.
    fn flush(&mut self) -> Result<Vec<TranscriptSegment>, SpeechEngineError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_confidence::ConfidenceSource;

    struct NullSpeechEngine;

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
            Ok(vec![])
        }
    }

    #[test]
    fn not_ready_engine_reports_not_initialized() {
        let mut engine = NullSpeechEngine;
        assert!(!engine.is_ready());
        assert!(matches!(
            engine.feed_audio(&[]),
            Err(SpeechEngineError::NotInitialized)
        ));
    }

    #[test]
    fn transcript_segment_carries_a_confidence_result() {
        let segment = TranscriptSegment {
            text: "Romans eight".into(),
            is_final: true,
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Model, None),
            start_ms: 0,
            end_ms: 500,
        };
        assert!(segment.confidence.is_auto_acceptable());
    }
}
