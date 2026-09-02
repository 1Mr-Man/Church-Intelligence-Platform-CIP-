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

/// Voice-activity RMS floor (Phase 5.3) below which a fully-buffered ~3s
/// window is treated as silence and whisper.cpp's real inference pass is
/// skipped entirely - the window's elapsed-time bookkeeping still advances
/// exactly as if inference had run (see [`WhisperSpeechEngine::run_inference`]),
/// so later segments' `start_ms`/`end_ms` are never distorted; only the
/// (expensive, and on slow hardware potentially hallucination-prone -
/// see `docs/phase-5-3-audio-vad.md`) whisper.cpp call itself is avoided.
///
/// Deliberately conservative (near the noise floor of an unmuted,
/// otherwise-quiet microphone) rather than tuned to filter quiet speech -
/// skipping a window that actually contained soft-but-real speech is a
/// far worse failure than occasionally spending one inference pass on
/// true silence. Documented, not empirically calibrated against real
/// hardware, matching every other threshold in this codebase
/// (`MIN_PARAPHRASE_SCORE`, `MIN_SEMANTIC_SIMILARITY`,
/// `CONFIRMATION_SCORE_BONUS`) - revisit once real operator feedback from
/// a live sanctuary environment exists.
const SILENCE_RMS_THRESHOLD: f32 = 0.01;

/// RMS energy of a PCM16 buffer, `0.0..=1.0` - the same formula
/// `integrations/audio`'s own input-level meter uses, duplicated here
/// rather than adding a new cross-crate dependency (`ai/speech` has no
/// existing dependency on `integrations/audio`) since this is a small,
/// pure, independently-testable function.
fn rms_level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples
        .iter()
        .map(|s| (f64::from(*s) / f64::from(i16::MAX)).powi(2))
        .sum();
    ((sum_sq / samples.len() as f64).sqrt() as f32).clamp(0.0, 1.0)
}

/// Whether `samples` is quiet enough to be treated as silence - see
/// [`SILENCE_RMS_THRESHOLD`]'s own docs for the conservative reasoning
/// behind the cutoff.
fn is_silence(samples: &[i16]) -> bool {
    rms_level(samples) < SILENCE_RMS_THRESHOLD
}

pub struct WhisperSpeechEngine {
    ctx: WhisperContext,
    buffer: Vec<i16>,
    sequence: u64,
    elapsed_ms: u64,
    language: Option<String>,
    /// Phase 3.8.7.3: whether the most recent `feed_audio` call actually
    /// ran `run_inference` - see `SpeechEngine::last_feed_triggered_inference`'s
    /// own docs for why a caller needs this.
    last_feed_triggered_inference: bool,
    /// Phase 5.3: whether the most recent fully-buffered window was
    /// classified as silence and skipped rather than fed to whisper.cpp -
    /// see `SpeechEngine::last_feed_was_silence`'s own docs.
    last_feed_was_silence: bool,
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
            last_feed_triggered_inference: false,
            last_feed_was_silence: false,
        })
    }

    fn run_inference(&mut self) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        if self.buffer.is_empty() {
            self.last_feed_triggered_inference = false;
            self.last_feed_was_silence = false;
            return Ok(vec![]);
        }

        let duration_ms = (self.buffer.len() as u64 * 1000) / u64::from(SAMPLE_RATE_HZ);
        let start_ms = self.elapsed_ms;

        // Phase 5.3 (VAD gating): a near-silent window still advances the
        // clock and is still cleared, exactly as if inference had run - it
        // just never reaches whisper.cpp, avoiding both a wasted (on slow
        // hardware, expensive) inference pass and the hallucination risk
        // of running Whisper on near-empty audio. See
        // `SILENCE_RMS_THRESHOLD`'s own docs for why the cutoff is
        // deliberately conservative.
        if is_silence(&self.buffer) {
            self.elapsed_ms += duration_ms;
            self.buffer.clear();
            self.last_feed_triggered_inference = false;
            self.last_feed_was_silence = true;
            return Ok(vec![]);
        }
        self.last_feed_was_silence = false;

        let audio_f32: Vec<f32> = self
            .buffer
            .iter()
            .map(|s| f32::from(*s) / f32::from(i16::MAX))
            .collect();
        self.elapsed_ms += duration_ms;
        self.buffer.clear();
        self.last_feed_triggered_inference = true;

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| SpeechEngineError::TranscriptionFailed(e.to_string()))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // whisper.cpp's own default (used if this is never called) is
        // `min(4, hardware_concurrency())` - a conservative cap that
        // leaves real parallelism on the table on any machine with more
        // than 4 logical cores. This already-idle worker thread (see
        // `spawn_speech_worker` in `apps/desktop/src-tauri`) is the only
        // thing running inference, so using every available core is safe;
        // capped at 8 to avoid diminishing-returns oversubscription
        // overhead on very high-core-count machines.
        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(8) as std::ffi::c_int;
        params.set_n_threads(n_threads);

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
            self.last_feed_triggered_inference = false;
            self.last_feed_was_silence = false;
            Ok(vec![])
        }
    }

    fn flush(&mut self) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        self.run_inference()
    }

    fn required_sample_rate_hz(&self) -> Option<u32> {
        Some(SAMPLE_RATE_HZ)
    }

    fn last_feed_triggered_inference(&self) -> bool {
        self.last_feed_triggered_inference
    }

    fn last_feed_was_silence(&self) -> bool {
        self.last_feed_was_silence
    }

    fn discard_buffered_audio(&mut self) {
        self.buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // --- Phase 5.3 VAD gating: the pure classification functions, fully
    // testable without a real model file or `WhisperContext` -----------

    #[test]
    fn silence_has_near_zero_rms() {
        let silence = vec![0i16; SAMPLE_RATE_HZ as usize];
        assert!(rms_level(&silence) < 0.001);
        assert!(is_silence(&silence));
    }

    #[test]
    fn a_full_scale_tone_is_never_classified_as_silence() {
        // A square wave alternating between full positive and negative
        // scale - deliberately loud, not a realistic voice waveform, but
        // enough to prove the RMS floor doesn't false-positive on any
        // genuinely energetic signal.
        let tone: Vec<i16> = (0..SAMPLE_RATE_HZ)
            .map(|i| if i % 2 == 0 { i16::MAX } else { i16::MIN })
            .collect();
        assert!(rms_level(&tone) > 0.9);
        assert!(!is_silence(&tone));
    }

    #[test]
    fn low_level_room_noise_just_above_the_floor_is_not_silence() {
        // A quiet but real signal (5% of full scale) - well above
        // SILENCE_RMS_THRESHOLD (0.01), proving the gate does not
        // over-trigger on quiet-but-genuine audio, only on near-total
        // silence.
        let quiet: Vec<i16> = (0..SAMPLE_RATE_HZ)
            .map(|i| {
                let amplitude = (f32::from(i16::MAX) * 0.05) as i16;
                if i % 2 == 0 {
                    amplitude
                } else {
                    -amplitude
                }
            })
            .collect();
        assert!(
            !is_silence(&quiet),
            "a quiet-but-real signal must never be misclassified as silence"
        );
    }

    #[test]
    fn rms_level_of_an_empty_buffer_is_zero_not_a_panic() {
        assert_eq!(rms_level(&[]), 0.0);
        assert!(is_silence(&[]));
    }

    /// The one thing about this engine that's fully verifiable without a
    /// real model file: a missing model is reported cleanly, never
    /// downloaded, never panics.
    #[test]
    fn missing_model_file_is_reported_as_model_not_found() {
        let result = WhisperSpeechEngine::load(&PathBuf::from("/nonexistent/ggml-tiny.en.bin"));
        assert!(matches!(result, Err(SpeechEngineError::ModelNotFound(_))));
    }

    /// Phase 3.1 failure-injection gap #5: a model path that exists but is
    /// not a real ggml/gguf model (a corrupt download, a truncated
    /// transfer, or a file of the wrong type pointed at by
    /// `CIP_WHISPER_MODEL_PATH`) must be reported as a clean, actionable
    /// error - never a panic or a silent hang - distinct from the
    /// "no file at all" case above.
    #[test]
    fn corrupt_model_file_is_reported_as_transcription_failed_not_a_panic() {
        let path =
            std::env::temp_dir().join(format!("cip-corrupt-model-test-{}.bin", std::process::id()));
        std::fs::write(&path, b"this is not a real ggml/gguf whisper model file").unwrap();

        let result = WhisperSpeechEngine::load(&path);

        let _ = std::fs::remove_file(&path);

        match result {
            Err(SpeechEngineError::TranscriptionFailed(_)) => {}
            Err(other) => panic!(
                "a corrupt-but-present model file must fail with TranscriptionFailed, got {other:?}"
            ),
            Ok(_) => panic!("a corrupt-but-present model file must never load successfully"),
        }
    }
}
