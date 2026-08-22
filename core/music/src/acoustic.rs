//! The acoustic recognition boundary (Phase 2.2) - the audio-fingerprint/
//! embedding-based counterpart to lyric/title matching. Deliberately a
//! *contract*, not an implementation: this module defines what an
//! acoustic recognizer looks like from `core/music`'s point of view -
//! `integrations/music-acoustic` supplies concrete implementations
//! (`NullAcousticMusicRecognizer`, `ScriptedAcousticMusicRecognizer`,
//! `LocalAcousticMusicRecognizer`), mirroring exactly how
//! `core/ai::SpeechEngine` is a contract and `ai/speech` supplies
//! `NullSpeechEngine`/`ScriptedSpeechEngine`/`WhisperSpeechEngine`.
//!
//! ## What this module does NOT do
//!
//! It does not perform recognition itself, does not know about any
//! specific model architecture or vendor, does not touch Tauri, SQLite,
//! or an audio capture library (`cpal`) - see `docs/acoustic-music.md`'s
//! architecture section. It does not pretend acoustic recognition works
//! when no recognizer is configured: [`AcousticRecognitionStatus::Unavailable`]
//! is a first-class, honestly-reported state, not an error to work around.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use cip_core_confidence::ConfidenceResult;

/// Whether an [`AcousticMusicRecognizer`] is even worth calling right now -
/// mirrors `cip_core_intelligence::EngineCapability`'s four-way
/// distinction (not installed / installed but broken / installed but off),
/// applied to one recognizer instance rather than a whole engine. Kept as
/// its own type (not reusing `EngineCapability` directly) because
/// `core/music` must not depend on `core/intelligence` - the dependency
/// runs the other way (see `docs/architecture.md#boundaries`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcousticRecognitionStatus {
    Available,
    /// No model/provider is configured, or a configured one could not be
    /// loaded (missing file, unreadable, malformed) - the honest default
    /// when nothing has been set up. Never silently treated as "works
    /// anyway."
    Unavailable,
    /// Explicitly turned off by configuration, distinct from `Unavailable`
    /// (which means "not working"), so an operator/diagnostic UI can tell
    /// "never configured" apart from "configured but off."
    Disabled,
    /// Was `Available` but the last recognition attempt failed (e.g. the
    /// audio device disappeared mid-service). Distinct from `Unavailable`
    /// the same way `AppState::audio_error` is distinct from "no device" -
    /// see `apps/desktop/src-tauri/src/commands.rs`'s `AudioStatusKind`.
    Error,
}

/// How a recognizer is implemented - never a specific vendor/model name,
/// only the *kind* of thing it is. Exists so the architecture can support
/// multiple future implementations (a fingerprint matcher, an embedding
/// model, a future external provider) without `MusicIntelligenceEngine` or
/// any caller needing to know which one is behind the trait object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcousticRecognitionMethod {
    /// A model/algorithm running entirely on this machine, no network.
    LocalModel,
    /// Reserved for a future, explicitly-configured external service -
    /// nothing in this codebase constructs this today (Phase 2.2 does not
    /// hard-code a commercial acoustic recognition provider).
    ExternalProvider,
    /// A deterministic scripted/test adapter - never real recognition.
    Test,
    /// No recognizer is configured at all (the `Null` default).
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalQuality {
    /// No usable signal at all (empty buffer, or RMS indistinguishable
    /// from zero).
    Silence,
    /// Real signal present, but shorter than
    /// [`AcousticAnalysisConfig::min_duration_ms`] - too little audio to
    /// attempt recognition against.
    TooShort,
    /// Long enough, but the signal level is below
    /// [`AcousticAnalysisConfig::min_rms`] - present but too quiet/noisy
    /// to be worth an expensive recognition call. Deliberately *not* the
    /// same bucket as `Silence`: this is real audio, just weak, and the
    /// threshold is a documented constant, not a guess about what "quiet
    /// worship" should sound like (see this module's docs and
    /// `docs/acoustic-music.md`'s signal-quality section for why the
    /// default is conservative rather than aggressive).
    LowQuality,
    /// Long enough and loud enough to attempt recognition.
    Ready,
}

/// One bounded window of raw audio ready for acoustic analysis - the
/// music-domain counterpart to `cip_core_ai::TranscriptSegment`, produced
/// by [`AudioSegmenter`] from a stream of `AudioChunk`s
/// (`cip_core_service::AudioChunk`, mono PCM16 - this module reuses that
/// exact sample format rather than inventing another one).
///
/// `started_at_ms` is on the same audio-relative clock convention
/// `TranscriptSegment::start_ms`/`end_ms` already use (milliseconds since
/// capture started, not wall-clock time) - kept consistent rather than
/// introducing a second time convention, and it avoids `core/music`
/// needing a `chrono` dependency it otherwise has no use for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSegment {
    pub id: Uuid,
    pub samples: Vec<i16>,
    pub sample_rate_hz: u32,
    /// Always `1` today - `AudioChunk` is already normalized to mono by
    /// the `AudioEngine` boundary (see `cip_core_service::AudioChunk`'s
    /// docs). Carried explicitly anyway so a future multi-channel source
    /// does not require a breaking field addition.
    pub channels: u16,
    pub started_at_ms: u64,
    pub duration_ms: u64,
}

impl AudioSegment {
    pub fn new(samples: Vec<i16>, sample_rate_hz: u32, started_at_ms: u64) -> Self {
        let duration_ms = if sample_rate_hz == 0 {
            0
        } else {
            (samples.len() as u64 * 1000) / u64::from(sample_rate_hz)
        };
        Self {
            id: Uuid::new_v4(),
            samples,
            sample_rate_hz,
            channels: 1,
            started_at_ms,
            duration_ms,
        }
    }
}

/// Tunable thresholds for segmentation and the signal-quality gate - a
/// plain, documented config struct (mirroring `matcher::MatchThresholds`'s
/// pattern) rather than magic numbers scattered across the worker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcousticAnalysisConfig {
    /// Length of one analysis window handed to the recognizer.
    pub window_ms: u64,
    /// How much consecutive windows overlap - long enough that a song
    /// transition happening right at a window boundary is not missed
    /// entirely, short enough that windows are not almost-duplicates.
    pub overlap_ms: u64,
    /// Below this, a segment is `TooShort` even if otherwise loud enough -
    /// there is not enough audio yet to be worth recognizing.
    pub min_duration_ms: u64,
    /// Minimum root-mean-square sample magnitude (on the `i16` scale,
    /// `0..=32767`) for a segment to be `Ready` rather than `LowQuality`.
    /// Deliberately conservative (see [`SignalQuality::LowQuality`]'s
    /// docs) - this must never reject genuinely quiet-but-real singing,
    /// only silence/near-silence.
    pub min_rms: f32,
    /// Minimum wall-clock-equivalent gap (on the same audio-relative
    /// clock as `AudioSegment::started_at_ms`) between two recognition
    /// *attempts* - bounds how often the (potentially expensive)
    /// recognizer is actually called, independent of how many windows
    /// the segmenter produces.
    pub minimum_recognition_interval_ms: u64,
}

impl Default for AcousticAnalysisConfig {
    fn default() -> Self {
        Self {
            window_ms: 8_000,
            overlap_ms: 2_000,
            min_duration_ms: 3_000,
            min_rms: 80.0,
            minimum_recognition_interval_ms: 5_000,
        }
    }
}

/// Deterministic RMS-based signal-quality gate - see [`SignalQuality`]'s
/// variants for what each outcome means. Pure and cheap by design: this
/// must run before every (comparatively expensive) recognizer call, never
/// after.
pub fn assess_signal_quality(
    segment: &AudioSegment,
    config: &AcousticAnalysisConfig,
) -> SignalQuality {
    if segment.samples.is_empty() {
        return SignalQuality::Silence;
    }
    let rms = root_mean_square(&segment.samples);
    if rms < 1.0 {
        return SignalQuality::Silence;
    }
    if segment.duration_ms < config.min_duration_ms {
        return SignalQuality::TooShort;
    }
    if rms < config.min_rms {
        return SignalQuality::LowQuality;
    }
    SignalQuality::Ready
}

fn root_mean_square(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_of_squares: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    ((sum_of_squares / samples.len() as f64).sqrt()) as f32
}

/// One acoustic recognition result - the music-domain counterpart to a
/// lyric/title [`crate::SongRecognitionCandidate`], carrying the
/// acoustic-specific facts a text match has no equivalent for (which
/// audio segment produced it, when, how long the analysis took). Kept as
/// its own type rather than folding these fields onto
/// `SongRecognitionCandidate` itself, since a title/alias match has no
/// audio segment/duration at all - see `crate::fusion` for how the two
/// meet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcousticRecognitionCandidate {
    pub song_id: String,
    /// The dataset (Content Registry id) this candidate's song belongs
    /// to. Every acoustic result is dataset-scoped exactly like lyric/
    /// title matches are (Phase 2.1 spec section 57's rule, unchanged in
    /// spirit here): a recognizer is asked to search within
    /// `content_ids` and must never resolve outside them.
    pub content_id: String,
    pub confidence: ConfidenceResult,
    pub method: AcousticRecognitionMethod,
    pub segment_id: Uuid,
    pub duration_ms: u64,
    /// Human-readable reasons, mirroring `SongRecognitionCandidate::evidence`.
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AcousticRecognitionError {
    /// The recognizer is not currently usable - carries the same reason
    /// `status()` would report, so a caller never has to reconcile two
    /// different explanations of "why not."
    Unavailable(String),
    Disabled,
    /// Recognition was attempted and failed (not "found nothing" - that
    /// is `Ok(vec![])`; this is a real failure, e.g. a corrupt model
    /// state).
    RecognitionFailed(String),
}

impl std::fmt::Display for AcousticRecognitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcousticRecognitionError::Unavailable(reason) => {
                write!(f, "acoustic recognizer unavailable: {reason}")
            }
            AcousticRecognitionError::Disabled => write!(f, "acoustic recognizer disabled"),
            AcousticRecognitionError::RecognitionFailed(reason) => {
                write!(f, "acoustic recognition failed: {reason}")
            }
        }
    }
}

impl std::error::Error for AcousticRecognitionError {}

/// The provider/adaptor contract for acoustic (audio-fingerprint/
/// embedding) song recognition - the acoustic-domain counterpart to
/// `cip_core_ai::SpeechEngine`. `core/music` depends only on this trait;
/// `integrations/music-acoustic` supplies concrete implementations.
///
/// A recognizer must never mutate service state, approve a finding,
/// prepare a presentation, project a slide, write SQLite directly, or
/// touch Tauri - it only ever answers "given this audio, which songs
/// (if any) does it resemble, in these datasets."
pub trait AcousticMusicRecognizer: Send + Sync {
    fn status(&self) -> AcousticRecognitionStatus;
    fn method(&self) -> AcousticRecognitionMethod;

    /// A short, human-readable explanation of `status()` - e.g. "no
    /// model directory configured", "model manifest is empty" - for an
    /// honest diagnostic UI (never a placeholder/generic string when a
    /// real reason is known). Default `None`: a recognizer with an
    /// obvious status (like the always-`Unavailable` `Null` default)
    /// need not override this.
    fn status_reason(&self) -> Option<String> {
        None
    }

    /// Attempt recognition against `segment`, scoped to `content_ids`
    /// (already filtered to enabled Music datasets by the caller - the
    /// recognizer itself has no concept of "enabled," mirroring
    /// `MusicProvider`'s dataset-scoping discipline). Returns zero, one,
    /// or many candidates - never guesses a single "best" answer; ranking
    /// and ambiguity are the caller's job (see `crate::matcher::is_ambiguous`).
    fn recognize(
        &mut self,
        segment: &AudioSegment,
        content_ids: &[String],
    ) -> Result<Vec<AcousticRecognitionCandidate>, AcousticRecognitionError>;
}

/// A bounded, deterministic accumulator turning a stream of raw mono
/// PCM16 chunks into [`AudioSegment`] windows - the audio-domain
/// counterpart to how a `SpeechEngine` buffers samples internally
/// (compare `WhisperSpeechEngine`'s `CHUNK_SAMPLES` buffering in
/// `ai/speech`), but exposed here as its own type since segmentation is
/// genuinely reusable logic, not an implementation detail of one
/// recognizer.
///
/// Bounded by construction: the internal buffer never grows past
/// `config.window_ms` worth of samples (older samples are dropped, never
/// accumulated without limit) - "tolerate dropped chunks" is a design
/// requirement, not a bug, since real-time audio that cannot be analyzed
/// in time is by definition stale.
pub struct AudioSegmenter {
    config: AcousticAnalysisConfig,
    buffer: Vec<i16>,
    sample_rate_hz: u32,
    elapsed_ms: u64,
    next_window_start_ms: u64,
}

impl AudioSegmenter {
    pub fn new(config: AcousticAnalysisConfig) -> Self {
        Self {
            config,
            buffer: Vec::new(),
            sample_rate_hz: 0,
            elapsed_ms: 0,
            next_window_start_ms: 0,
        }
    }

    /// Feed one chunk of raw mono PCM16 audio. Returns zero or one
    /// [`AudioSegment`] - one exactly when enough audio has accumulated
    /// for a full `window_ms` window since the last one was emitted.
    /// `sample_rate_hz` is trusted from the caller (mirroring
    /// `AudioChunk`'s own contract) and is not expected to change
    /// mid-capture; if it does, the buffer is reset rather than mixing
    /// two sample rates together.
    pub fn push(&mut self, samples: &[i16], sample_rate_hz: u32) -> Option<AudioSegment> {
        if sample_rate_hz == 0 {
            return None;
        }
        if self.sample_rate_hz != 0 && self.sample_rate_hz != sample_rate_hz {
            self.buffer.clear();
        }
        self.sample_rate_hz = sample_rate_hz;
        self.buffer.extend_from_slice(samples);
        self.elapsed_ms += (samples.len() as u64 * 1000) / u64::from(sample_rate_hz);

        let window_samples = (self.config.window_ms * u64::from(sample_rate_hz) / 1000) as usize;
        if window_samples == 0 || self.buffer.len() < window_samples {
            // Bounded even while still filling the first window - never
            // accumulate more than one window's worth before the first
            // emission.
            let max_buffer = window_samples.max(1);
            if self.buffer.len() > max_buffer {
                let excess = self.buffer.len() - max_buffer;
                self.buffer.drain(0..excess);
            }
            return None;
        }

        let window: Vec<i16> = self.buffer[..window_samples].to_vec();
        let overlap_samples = (self.config.overlap_ms * u64::from(sample_rate_hz) / 1000) as usize;
        let advance = window_samples.saturating_sub(overlap_samples).max(1);
        let advance = advance.min(self.buffer.len());
        self.buffer.drain(0..advance);

        let segment = AudioSegment::new(window, sample_rate_hz, self.next_window_start_ms);
        self.next_window_start_ms += (advance as u64 * 1000) / u64::from(sample_rate_hz);
        Some(segment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn silent_samples(n: usize) -> Vec<i16> {
        vec![0; n]
    }

    fn loud_samples(n: usize) -> Vec<i16> {
        vec![10_000; n]
    }

    #[test]
    fn empty_segment_is_silence() {
        let segment = AudioSegment::new(vec![], 16_000, 0);
        assert_eq!(
            assess_signal_quality(&segment, &AcousticAnalysisConfig::default()),
            SignalQuality::Silence
        );
    }

    #[test]
    fn all_zero_samples_are_silence_regardless_of_duration() {
        let segment = AudioSegment::new(silent_samples(16_000 * 5), 16_000, 0);
        assert_eq!(
            assess_signal_quality(&segment, &AcousticAnalysisConfig::default()),
            SignalQuality::Silence
        );
    }

    #[test]
    fn loud_but_too_short_is_too_short_not_ready() {
        let segment = AudioSegment::new(loud_samples(1_000), 16_000, 0);
        assert!(segment.duration_ms < AcousticAnalysisConfig::default().min_duration_ms);
        assert_eq!(
            assess_signal_quality(&segment, &AcousticAnalysisConfig::default()),
            SignalQuality::TooShort
        );
    }

    #[test]
    fn quiet_but_long_enough_is_low_quality() {
        let quiet: Vec<i16> = vec![5; 16_000 * 4];
        let segment = AudioSegment::new(quiet, 16_000, 0);
        assert_eq!(
            assess_signal_quality(&segment, &AcousticAnalysisConfig::default()),
            SignalQuality::LowQuality
        );
    }

    #[test]
    fn loud_and_long_enough_is_ready() {
        let segment = AudioSegment::new(loud_samples(16_000 * 4), 16_000, 0);
        assert_eq!(
            assess_signal_quality(&segment, &AcousticAnalysisConfig::default()),
            SignalQuality::Ready
        );
    }

    #[test]
    fn segmenter_emits_nothing_until_a_full_window_accumulates() {
        let mut segmenter = AudioSegmenter::new(AcousticAnalysisConfig::default());
        // One window is 8000ms at 16kHz = 128,000 samples.
        assert!(segmenter.push(&loud_samples(16_000), 16_000).is_none());
        assert!(segmenter.push(&loud_samples(16_000), 16_000).is_none());
    }

    #[test]
    fn segmenter_emits_a_window_once_enough_audio_has_accumulated() {
        let mut segmenter = AudioSegmenter::new(AcousticAnalysisConfig::default());
        let window_samples = 8_000 * 16_000 / 1000; // window_ms * rate / 1000
        let segment = segmenter
            .push(&loud_samples(window_samples), 16_000)
            .expect("a full window must emit a segment");
        assert_eq!(segment.samples.len(), window_samples);
        assert_eq!(segment.duration_ms, 8_000);
    }

    #[test]
    fn segmenter_never_grows_the_buffer_unboundedly_while_filling() {
        let mut segmenter = AudioSegmenter::new(AcousticAnalysisConfig::default());
        for _ in 0..50 {
            segmenter.push(&loud_samples(16_000), 16_000);
        }
        // One window's worth (128,000 samples) is the most that should
        // ever be buffered while waiting for the first emission.
        let window_samples = 8_000 * 16_000 / 1000;
        assert!(segmenter.buffer.len() <= window_samples);
    }

    #[test]
    fn segmenter_advances_by_window_minus_overlap_producing_overlapping_windows() {
        let mut segmenter = AudioSegmenter::new(AcousticAnalysisConfig::default());
        let window_samples = 8_000 * 16_000 / 1000;
        let first = segmenter
            .push(&loud_samples(window_samples), 16_000)
            .unwrap();
        assert_eq!(first.started_at_ms, 0);

        // advance = (window_ms - overlap_ms) = 6000ms worth of samples
        let advance_samples = 6_000 * 16_000 / 1000;
        let second = segmenter
            .push(&loud_samples(advance_samples), 16_000)
            .expect("enough new audio to fill the next overlapping window");
        assert_eq!(second.started_at_ms, 6_000);
    }

    #[test]
    fn a_zero_sample_rate_is_ignored_rather_than_dividing_by_zero() {
        let mut segmenter = AudioSegmenter::new(AcousticAnalysisConfig::default());
        assert!(segmenter.push(&loud_samples(1_000), 0).is_none());
    }

    #[test]
    fn audio_segment_duration_is_derived_from_sample_count_and_rate() {
        let segment = AudioSegment::new(loud_samples(16_000), 16_000, 0);
        assert_eq!(segment.duration_ms, 1_000);
    }

    #[test]
    fn statuses_and_methods_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&AcousticRecognitionStatus::Unavailable).unwrap(),
            "\"unavailable\""
        );
        assert_eq!(
            serde_json::to_string(&AcousticRecognitionMethod::LocalModel).unwrap(),
            "\"local_model\""
        );
    }

    #[test]
    fn candidate_serializes_camel_case() {
        let candidate = AcousticRecognitionCandidate {
            song_id: "s1".to_string(),
            content_id: "music:dev-hymnbook".to_string(),
            confidence: ConfidenceResult::new(
                0.8,
                cip_core_confidence::ConfidenceSource::Model,
                None,
            ),
            method: AcousticRecognitionMethod::Test,
            segment_id: Uuid::nil(),
            duration_ms: 8_000,
            evidence: vec!["scripted match".to_string()],
        };
        let json = serde_json::to_value(&candidate).unwrap();
        assert_eq!(json["songId"], "s1");
        assert_eq!(json["contentId"], "music:dev-hymnbook");
        assert_eq!(json["durationMs"], 8000);
    }
}
