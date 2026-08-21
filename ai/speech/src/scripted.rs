//! [`ScriptedSpeechEngine`] - a deterministic test/demo adapter at the
//! `SpeechEngine` boundary.
//!
//! Real audio in, real inference out isn't reliably testable in every
//! environment (no microphone, no downloadable model, CI without a large
//! model cached). `ScriptedSpeechEngine` implements the real `SpeechEngine`
//! trait but ignores the audio bytes it's fed, instead emitting a
//! pre-programmed script of transcript segments - one `Final` segment per
//! non-empty `feed_audio` call. It exists to test the
//! `AudioEngine -> SpeechEngine -> Bible Intelligence Core` wiring end to
//! end without a microphone or model. It is not, and must never become, a
//! substitute for validating a real speech backend - see the `whisper`
//! module (behind the `whisper` feature) and `docs/live-speech.md`.

use cip_core_ai::{SpeechEngine, SpeechEngineError, TranscriptSegment};
use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use std::collections::VecDeque;
use uuid::Uuid;

pub struct ScriptedSpeechEngine {
    script: VecDeque<String>,
    sequence: u64,
    elapsed_ms: u64,
}

impl ScriptedSpeechEngine {
    pub fn new(lines: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            script: lines.into_iter().map(Into::into).collect(),
            sequence: 0,
            elapsed_ms: 0,
        }
    }
}

impl SpeechEngine for ScriptedSpeechEngine {
    fn is_ready(&self) -> bool {
        true
    }

    /// Ignores `samples` beyond checking it's non-empty - one scripted line
    /// is emitted per call that receives audio, simulating "enough audio
    /// for one utterance arrived."
    fn feed_audio(&mut self, samples: &[i16]) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        if samples.is_empty() {
            return Ok(vec![]);
        }
        let Some(text) = self.script.pop_front() else {
            return Ok(vec![]);
        };

        let start_ms = self.elapsed_ms;
        let duration_ms = 500 + (text.len() as u64) * 30;
        self.elapsed_ms += duration_ms;

        let segment = TranscriptSegment {
            id: Uuid::new_v4(),
            sequence: self.sequence,
            text,
            is_final: true,
            confidence: ConfidenceResult::new(
                0.95,
                ConfidenceSource::Model,
                Some("scripted test adapter, not a real transcription".to_string()),
            ),
            start_ms,
            end_ms: self.elapsed_ms,
            language: Some("en".to_string()),
            speaker_id: None,
        };
        self.sequence += 1;
        Ok(vec![segment])
    }

    fn flush(&mut self) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_one_final_segment_per_scripted_line_in_order() {
        let mut engine = ScriptedSpeechEngine::new(["hello", "world"]);

        let first = engine.feed_audio(&[1, 2, 3]).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].text, "hello");
        assert!(first[0].is_final);
        assert_eq!(first[0].sequence, 0);

        let second = engine.feed_audio(&[1, 2, 3]).unwrap();
        assert_eq!(second[0].text, "world");
        assert_eq!(second[0].sequence, 1);
        assert!(second[0].start_ms >= first[0].end_ms);

        let exhausted = engine.feed_audio(&[1, 2, 3]).unwrap();
        assert!(exhausted.is_empty());
    }

    #[test]
    fn empty_audio_produces_no_segment() {
        let mut engine = ScriptedSpeechEngine::new(["hello"]);
        assert!(engine.feed_audio(&[]).unwrap().is_empty());
    }

    #[test]
    fn is_always_ready() {
        assert!(ScriptedSpeechEngine::new(Vec::<&str>::new()).is_ready());
    }
}
