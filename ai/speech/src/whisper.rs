//! [`WhisperSpeechEngine`] - a real local speech backend behind the
//! `SpeechEngine` trait, using [whisper-rs](https://github.com/tazz4843/whisper-rs)
//! (MIT-licensed Rust bindings to [whisper.cpp](https://github.com/ggerganov/whisper.cpp),
//! also MIT-licensed). Both are vendored/fetched from crates.io and compile
//! fully offline - no network access is needed to *build* this engine.
//!
//! **Running** it requires a local ggml/gguf Whisper model file, which is
//! not bundled with CIP (see `docs/live-speech.md` for licensing and how to
//! obtain one) and, in this development environment, could not be
//! downloaded to verify end-to-end transcription: the standard model host
//! (`huggingface.co`) is blocked by this environment's egress policy. That
//! is a documented environmental limitation, not a defect in this code -
//! [`WhisperSpeechEngine::load`] correctly reports
//! [`SpeechEngineError::ModelNotFound`] when no model file is present,
//! which is exactly the state a real installation without a configured
//! model would also be in.
//!
//! ## Design
//!
//! whisper.cpp's `full()` call is synchronous and processes a complete
//! buffer at once - it does not natively stream interim results. This
//! engine buffers incoming audio and runs one inference pass per ~3 seconds
//! of audio (or on [`flush`](WhisperSpeechEngine::flush)), always emitting
//! `is_final: true` segments. It does **not** fabricate interim segments -
//! see `docs/live-speech.md`'s "Interim vs. final" section for what a
//! future engine with true streaming support would change.

use cip_core_ai::{SpeechEngine, SpeechEngineError, TranscriptSegment};
use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use std::path::Path;
use uuid::Uuid;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const SAMPLE_RATE_HZ: u32 = 16_000;
/// Run inference once this many samples have buffered (~3s at 16kHz) -
/// bounds latency without pretending whisper.cpp's synchronous API is a
/// true low-latency streaming interface.
const CHUNK_SAMPLES: usize = SAMPLE_RATE_HZ as usize * 3;

pub struct WhisperSpeechEngine {
    ctx: WhisperContext,
    buffer: Vec<i16>,
    sequence: u64,
    elapsed_ms: u64,
    language: Option<String>,
}

impl WhisperSpeechEngine {
    /// Load a local ggml/gguf Whisper model from `model_path`.
    ///
    /// Never downloads anything and never panics: a missing file is
    /// reported as [`SpeechEngineError::ModelNotFound`], matching the
    /// "speech model not installed" state the rest of the application
    /// (see `apps/desktop/src-tauri`'s engine-selection policy) treats as
    /// non-fatal.
    pub fn load(model_path: &Path) -> Result<Self, SpeechEngineError> {
        if !model_path.is_file() {
            return Err(SpeechEngineError::ModelNotFound(
                model_path.display().to_string(),
            ));
        }
        let ctx = WhisperContext::new_with_params(
            &model_path.to_string_lossy(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| SpeechEngineError::TranscriptionFailed(e.to_string()))?;
        Ok(Self {
            ctx,
            buffer: Vec::with_capacity(CHUNK_SAMPLES),
            sequence: 0,
            elapsed_ms: 0,
            language: None,
        })
    }

    fn run_inference(&mut self) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        if self.buffer.is_empty() {
            return Ok(vec![]);
        }

        let audio_f32: Vec<f32> = self
            .buffer
            .iter()
            .map(|s| f32::from(*s) / f32::from(i16::MAX))
            .collect();
        let duration_ms = (self.buffer.len() as u64 * 1000) / u64::from(SAMPLE_RATE_HZ);
        let start_ms = self.elapsed_ms;
        self.elapsed_ms += duration_ms;
        self.buffer.clear();

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| SpeechEngineError::TranscriptionFailed(e.to_string()))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full(params, &audio_f32)
            .map_err(|e| SpeechEngineError::TranscriptionFailed(e.to_string()))?;

        let num_segments = state
            .full_n_segments()
            .map_err(|e| SpeechEngineError::TranscriptionFailed(e.to_string()))?;

        let mut text = String::new();
        for i in 0..num_segments {
            if let Ok(segment_text) = state.full_get_segment_text(i) {
                text.push_str(&segment_text);
            }
        }
        let text = text.trim().to_string();
        if text.is_empty() {
            return Ok(vec![]);
        }

        let segment = TranscriptSegment {
            id: Uuid::new_v4(),
            sequence: self.sequence,
            text,
            is_final: true,
            confidence: ConfidenceResult::new(
                0.75,
                ConfidenceSource::Model,
                Some(
                    "whisper.cpp full() decode; no per-token confidence exposed by this API"
                        .to_string(),
                ),
            ),
            start_ms,
            end_ms: self.elapsed_ms,
            language: self.language.clone(),
            speaker_id: None,
        };
        self.sequence += 1;
        Ok(vec![segment])
    }
}

impl SpeechEngine for WhisperSpeechEngine {
    fn is_ready(&self) -> bool {
        true
    }

    fn feed_audio(&mut self, samples: &[i16]) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        self.buffer.extend_from_slice(samples);
        if self.buffer.len() >= CHUNK_SAMPLES {
            self.run_inference()
        } else {
            Ok(vec![])
        }
    }

    fn flush(&mut self) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        self.run_inference()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The one thing about this engine that's fully verifiable without a
    /// real model file: a missing model is reported cleanly, never
    /// downloaded, never panics.
    #[test]
    fn missing_model_file_is_reported_as_model_not_found() {
        let result = WhisperSpeechEngine::load(&PathBuf::from("/nonexistent/ggml-tiny.en.bin"));
        assert!(matches!(result, Err(SpeechEngineError::ModelNotFound(_))));
    }
}
