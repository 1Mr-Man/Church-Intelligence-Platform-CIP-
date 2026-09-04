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
//! buffer at once - it does not natively stream interim results the way a
//! true streaming ASR model would. This engine buffers incoming audio
//! toward one **final** inference pass per window (see "Window
//! boundaries" below), and - Phase 24.2 - additionally runs one **real**
//! (never fabricated) interim decode partway through a longer window, so
//! an operator watching a multi-second sentence in progress sees text
//! sooner than waiting for the window to close. See "Interim decoding"
//! below and `docs/live-speech.md`'s "Interim vs. final" section.
//!
//! ## Window boundaries (Phase 21: VAD-triggered flush)
//!
//! A window closes - and inference runs - at whichever of these happens
//! first:
//!
//! 1. **A genuine pause is detected** ([`should_flush_early`]): once at
//!    least [`MIN_VAD_FLUSH_SAMPLES`] (1.5s) has buffered *and* the
//!    trailing [`TRAILING_SILENCE_SAMPLES`] (0.4s) of that buffer is quiet
//!    enough to be a real phrase/sentence-level pause (not just an
//!    ordinary between-word gap). This is the primary mechanism: it means
//!    most windows now end where the speaker actually paused, rather than
//!    at an arbitrary fixed sample count that has no relationship to the
//!    words being spoken - the single biggest source of mid-word cutoffs
//!    in the original fixed-3s design.
//! 2. **The fixed [`CHUNK_SAMPLES`] cap** (3s) - unchanged from before this
//!    phase, and still hit whenever speech runs on with no detectable
//!    pause (a long, continuous sentence). This remains the hard latency
//!    bound and safety net: worst case, behavior is identical to the
//!    pre-Phase-21 engine.
//!
//! Deliberately **still not** implemented: audio-overlapping windows (each
//! window also decoding a shared slice of the previous one's tail for
//! extra left-context). That technique needs some way to reconcile the
//! text produced from the shared audio decoded twice - real streaming ASR
//! systems solve this with token-timestamp-based stitching, which this
//! container has no real, timing-sensitive audio to validate against.
//! VAD-triggered flush was chosen instead because it addresses the same
//! root cause (a boundary landing mid-word) without ever risking duplicated
//! transcript text: a window boundary chosen at a detected pause has, by
//! definition, no word left hanging across it. See `docs/phase-21-audit.md`
//! for the full reasoning. Interim decoding (below) is a different, safer
//! technique that does not touch window boundaries at all, which is why it
//! was buildable in this container while overlapping windows still is not.
//!
//! ## Interim decoding (Phase 24.2)
//!
//! Once a window has buffered at least [`MIN_VAD_FLUSH_SAMPLES`] (1.5s)
//! without yet closing (no pause detected, hard cap not yet reached), this
//! engine runs one additional, genuine whisper.cpp `full()` pass over
//! *only the audio buffered so far* and emits its result as an interim
//! (`is_final: false`) [`TranscriptSegment`] - sharing the same `id` the
//! window's eventual final segment will carry, per that field's own
//! contract. This never fabricates text: it is exactly the same real
//! inference call the final decode uses, just run early on a shorter
//! prefix of the same audio. At most one interim decode is attempted per
//! window (see `window_interim_decoded`) - bounding the worst-case extra
//! cost to one additional `full()` pass per window, never unbounded
//! polling. Interim segments are never persisted and never reach the
//! Bible Intelligence Core - `apps/desktop/src-tauri/src/commands.rs`'s
//! real-time worker already gated on `is_final` before this phase (see
//! `docs/live-speech.md`'s "Interim vs. final" section), so this required
//! zero changes outside this file.
//!
//! Honest limitation: a mid-buffer decode has less audio context than the
//! eventual final decode of the complete window, so an interim segment's
//! text can legitimately differ from (and is sometimes less accurate
//! than) the final segment that later replaces it - this is inherent to
//! decoding a shorter prefix, not a defect. An interim segment is always
//! superseded by its window's final segment; nothing downstream ever
//! treats an interim segment as authoritative.
//!
//! ## Language (Phase 12)
//!
//! Defaults to `"en"`, preserving this engine's exact pre-Phase-12
//! behavior (whisper.cpp's own C-level default) for any caller that
//! never touches [`SpeechEngine::set_language`]. See
//! `cip_ai_speech::SUPPORTED_LANGUAGES` for the real, verified set of
//! languages CIP offers (and why Igbo is deliberately not among them)
//! and `docs/phase-12-audit.md` for the full evidence. Every real
//! inference pass reads back the language whisper.cpp actually used via
//! `full_lang_id_from_state`, so `TranscriptSegment.language` is always
//! an honest report of what happened, never a blind echo of the request -
//! this matters most for `"auto"`, where the two can genuinely differ.

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

/// Minimum buffered duration before an early, pause-triggered flush
/// ([`should_flush_early`]) is even considered - keeps an ordinary
/// between-word gap occurring almost immediately from fragmenting the
/// buffer into a window too short for whisper.cpp to transcribe well, and
/// guarantees every window carries at least this much real audio context.
/// Documented reasoning, not empirically calibrated against real
/// speech-timing data - matches every other threshold in this codebase
/// (`SILENCE_RMS_THRESHOLD`, `MIN_PARAPHRASE_SCORE`, ...).
///
/// Phase 24.2: also the earliest point a window's one allotted interim
/// decode is attempted (see the module docs' "Interim decoding" section) -
/// deliberately reused rather than a second constant: a window can only
/// still be buffering at this point if `should_flush_early` did *not* also
/// fire on the same `feed_audio` call, so an interim decode and a genuine
/// VAD flush never compete for the same window in the same call.
const MIN_VAD_FLUSH_SAMPLES: usize = (SAMPLE_RATE_HZ as usize * 3) / 2; // 1.5s

/// How much trailing near-silence in the buffer counts as a genuine
/// phrase/sentence-level pause worth flushing early on, rather than an
/// ordinary between-word gap (typically well under this in normal speech).
/// See [`has_trailing_pause`].
const TRAILING_SILENCE_SAMPLES: usize = (SAMPLE_RATE_HZ as usize * 2) / 5; // 0.4s

/// Whether the trailing [`TRAILING_SILENCE_SAMPLES`] of `buffer` are quiet
/// enough to be a genuine pause - the VAD-triggered-flush signal (Phase
/// 21). Returns `false` (never a false pause) when `buffer` is shorter
/// than the trailing window itself, since there isn't yet enough audio to
/// confirm a real pause happened; a short buffer simply keeps
/// accumulating toward either an early flush or the hard [`CHUNK_SAMPLES`]
/// cap.
fn has_trailing_pause(buffer: &[i16]) -> bool {
    if buffer.len() < TRAILING_SILENCE_SAMPLES {
        return false;
    }
    is_silence(&buffer[buffer.len() - TRAILING_SILENCE_SAMPLES..])
}

/// Whether `buffer` should be flushed to inference right now because a
/// natural pause was detected, rather than waiting for the fixed
/// [`CHUNK_SAMPLES`] cap - see the module docs' "Window boundaries"
/// section. Requires both a minimum buffered duration
/// ([`MIN_VAD_FLUSH_SAMPLES`]) and a genuine trailing pause
/// ([`has_trailing_pause`]). Does not itself check against `CHUNK_SAMPLES` -
/// the caller ([`WhisperSpeechEngine::feed_audio`]) checks the hard cap
/// separately and independently.
fn should_flush_early(buffer: &[i16]) -> bool {
    buffer.len() >= MIN_VAD_FLUSH_SAMPLES && has_trailing_pause(buffer)
}

/// Phase 24.2: whether the current window's one allotted interim decode
/// (see the module docs' "Interim decoding" section) should be attempted
/// right now, given the buffer's current length and whether this window
/// has already had its attempt. Deliberately mirrors [`should_flush_early`]'s
/// own style - a small, pure, independently-testable gate rather than
/// inline logic buried in `feed_audio`. Callers (`feed_audio`) only ever
/// reach this check once the hard-cap and VAD-flush checks have both
/// already failed for the current call, so an interim attempt and a
/// genuine flush never compete for the same window in the same call.
fn should_attempt_interim_decode(buffer_len: usize, already_decoded_this_window: bool) -> bool {
    !already_decoded_this_window && buffer_len >= MIN_VAD_FLUSH_SAMPLES
}

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

/// Whisper.cpp's own well-known non-speech placeholder captions - literal
/// strings that appear in the caption datasets Whisper was trained on
/// (YouTube-auto-caption-style annotations for non-speech audio), which the
/// model reproduces verbatim as ordinary decoded text when it is uncertain
/// about quiet, unclear, or non-speech-shaped audio. Confirmed against a
/// real Windows pilot session (see `docs/phase-14-audit.md`) where a quiet
/// room microphone produced exactly `"(speaking in foreign language)"`,
/// `"[BLANK_AUDIO]"`, `"[inaudible]"`, and `"[LAUGHTER]"` as if they were
/// real spoken content. Compared against [`normalize_for_placeholder_match`]'s
/// output, so bracket/parenthesis style, case, punctuation, and surrounding
/// whitespace never matter.
const NON_SPEECH_PLACEHOLDERS: &[&str] = &[
    "blank audio",
    "silence",
    "no audio",
    "no speech",
    "no speech detected",
    "inaudible",
    "speaking in foreign language",
    "foreign language",
    "unintelligible",
    "laughter",
    "laughing",
    "music",
    "music playing",
    "applause",
    "clapping",
    "background noise",
    "background music",
    "static",
    "buzzing",
    "silence in the video",
];

/// Lowercases `text` and keeps only ASCII letters/digits, collapsing every
/// other character (brackets, parentheses, underscores, punctuation,
/// whitespace) to single spaces - so `"[BLANK_AUDIO]"`,
/// `"(speaking in foreign language)"`, and `"[LAUGHTER]."` all normalize to
/// exactly the same form regardless of which bracket style or punctuation
/// whisper.cpp happened to wrap them in.
fn normalize_for_placeholder_match(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_space = true;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_space = false;
        } else if !last_was_space {
            out.push(' ');
            last_was_space = true;
        }
    }
    out.trim_end().to_string()
}

/// Whether `text` (already trimmed) is, in its entirety, one of
/// whisper.cpp's own known non-speech placeholder captions rather than
/// genuine transcribed speech - see [`NON_SPEECH_PLACEHOLDERS`]'s own docs.
/// Deliberately whole-string, not substring: a segment that mixes real
/// words with a bracketed tag (e.g. `"Amen. [BLANK_AUDIO]"`) is kept in
/// full rather than guessed at, matching this codebase's "never eat real
/// content" discipline (the same reasoning behind [`SILENCE_RMS_THRESHOLD`]
/// being conservative rather than aggressive).
fn is_non_speech_placeholder(text: &str) -> bool {
    let normalized = normalize_for_placeholder_match(text);
    !normalized.is_empty() && NON_SPEECH_PLACEHOLDERS.contains(&normalized.as_str())
}

pub struct WhisperSpeechEngine {
    ctx: WhisperContext,
    buffer: Vec<i16>,
    sequence: u64,
    elapsed_ms: u64,
    /// Phase 12: the language code (`"en"`/`"yo"`/`"ha"`/`"auto"` - see
    /// `cip_ai_speech::SUPPORTED_LANGUAGES`) the *next* inference pass
    /// should condition on - set via [`SpeechEngine::set_language`].
    /// Defaults to `"en"`, preserving this engine's exact pre-Phase-12
    /// behavior (whisper.cpp's own C-level default) for any caller that
    /// never touches the new setting.
    requested_language: String,
    /// Phase 3.8.7.3: whether the most recent `feed_audio` call actually
    /// ran `run_inference` - see `SpeechEngine::last_feed_triggered_inference`'s
    /// own docs for why a caller needs this.
    last_feed_triggered_inference: bool,
    /// Phase 5.3: whether the most recent fully-buffered window was
    /// classified as silence and skipped rather than fed to whisper.cpp -
    /// see `SpeechEngine::last_feed_was_silence`'s own docs.
    last_feed_was_silence: bool,
    /// Phase 14: whether the most recent real inference pass produced only
    /// one of whisper.cpp's own known non-speech placeholder captions - see
    /// `SpeechEngine::last_feed_was_non_speech_placeholder`'s own docs.
    last_feed_was_non_speech_placeholder: bool,
    /// Phase 21: whether the most recent `feed_audio` call ran inference
    /// because a natural pause was detected, rather than because the
    /// buffer hit the fixed `CHUNK_SAMPLES` cap - see
    /// `SpeechEngine::last_feed_was_vad_early_flush`'s own docs.
    last_feed_was_vad_early_flush: bool,
    /// Phase 24.2: the stable id assigned to the current window's segment -
    /// an interim decode and the final segment it settles into share this
    /// id, per `TranscriptSegment::id`'s own contract. `None` exactly when
    /// `buffer` is empty (no window in progress); set the moment a new
    /// window starts buffering (see `feed_audio`), cleared when the window
    /// closes (see `run_inference`/`discard_buffered_audio`).
    window_id: Option<Uuid>,
    /// Phase 24.2: whether the current window has already had its one
    /// allotted interim decode attempt (see the module docs' "Interim
    /// decoding" section) - bounds interim decoding to at most once per
    /// window regardless of how many more `feed_audio` calls arrive before
    /// the window closes for real.
    window_interim_decoded: bool,
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
            requested_language: "en".to_string(),
            last_feed_triggered_inference: false,
            last_feed_was_silence: false,
            last_feed_was_non_speech_placeholder: false,
            last_feed_was_vad_early_flush: false,
            window_id: None,
            window_interim_decoded: false,
        })
    }

    /// Whether the loaded model's vocabulary includes language tokens at
    /// all - `false` for an English-only (`ggml-*.en.bin`) model, which
    /// cannot honor any language selection other than English regardless
    /// of [`SpeechEngine::set_language`] (Phase 12). A thin wrapper over
    /// `whisper-rs`'s own `WhisperContext::is_multilingual`, called once
    /// at load time by `apps/desktop/src-tauri`'s `create_speech_engine`
    /// and surfaced honestly via `SpeechDiagnostics::model_is_multilingual`
    /// rather than letting an operator believe a language switch worked
    /// when the loaded model architecturally cannot do it.
    pub fn is_multilingual(&self) -> bool {
        self.ctx.is_multilingual()
    }

    /// Runs one real whisper.cpp `full()` pass over `audio_f32` and
    /// classifies the result. Shared by both [`Self::run_inference`] (the
    /// final decode) and [`Self::try_interim_decode`] (Phase 24.2's
    /// interim decode) - the exact same real inference call either way,
    /// never a second, fabricated code path. Deliberately takes `ctx`/
    /// `requested_language` as plain arguments rather than `&self`/
    /// `&mut self`: it must never touch `self.buffer`/`self.elapsed_ms`/
    /// `self.sequence` itself, since the two callers need those mutated at
    /// different, specific points relative to this call's own fallibility
    /// (see each caller's own comments for why).
    fn decode_pass(
        ctx: &WhisperContext,
        requested_language: &str,
        audio_f32: &[f32],
    ) -> Result<DecodeOutcome, SpeechEngineError> {
        let mut state = ctx
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
        // Phase 12: condition this pass on the currently-selected
        // language - `"auto"` is `whisper-rs`'s own documented literal
        // for auto-detection (equivalent to passing `None`), so no
        // special-casing is needed here.
        params.set_language(Some(requested_language));

        state
            .full(params, audio_f32)
            .map_err(|e| SpeechEngineError::TranscriptionFailed(e.to_string()))?;

        // Phase 12: read back the language whisper.cpp *actually* used
        // for this pass - correct whether it was forced or auto-detected -
        // rather than blindly echoing the requested setting, so an
        // Auto-detect selection produces an honest, real answer instead
        // of the literal string "auto".
        let detected_language = state
            .full_lang_id_from_state()
            .ok()
            .and_then(whisper_rs::get_lang_str)
            .map(str::to_string)
            .unwrap_or_else(|| requested_language.to_string());

        let num_segments = state
            .full_n_segments()
            .map_err(|e| SpeechEngineError::TranscriptionFailed(e.to_string()))?;

        let mut text = String::new();
        // Phase 14: average whisper.cpp's own real per-token decode
        // probability across every token in this pass - `full_get_token_prob`
        // is a genuine, implemented accessor (confirmed against the
        // vendored whisper-rs source; see docs/phase-14-audit.md), not the
        // hardcoded placeholder score this used to be. `prob_count` stays
        // 0 (and the fallback below applies) only if whisper.cpp produced
        // no tokens at all for a non-empty segment text, which should not
        // happen in practice but is handled rather than panicking.
        let mut prob_sum = 0.0_f64;
        let mut prob_count: usize = 0;
        for i in 0..num_segments {
            if let Ok(segment_text) = state.full_get_segment_text(i) {
                text.push_str(&segment_text);
            }
            if let Ok(n_tokens) = state.full_n_tokens(i) {
                for j in 0..n_tokens {
                    if let Ok(p) = state.full_get_token_prob(i, j) {
                        prob_sum += f64::from(p);
                        prob_count += 1;
                    }
                }
            }
        }
        let text = text.trim().to_string();
        if text.is_empty() {
            return Ok(DecodeOutcome::Empty);
        }

        // Phase 14: whisper.cpp's own known non-speech placeholder captions
        // (e.g. "[BLANK_AUDIO]", "(speaking in foreign language)") are not
        // genuine transcribed speech - discard them exactly like an empty
        // decode, rather than reporting them as if the congregation had
        // said them. See docs/phase-14-audit.md for the real-pilot evidence
        // that prompted this.
        if is_non_speech_placeholder(&text) {
            return Ok(DecodeOutcome::Placeholder);
        }

        let (confidence_score, confidence_note) = if prob_count > 0 {
            (
                (prob_sum / prob_count as f64) as f32,
                format!(
                    "whisper.cpp full() decode; averaged real per-token probability across {prob_count} token(s)"
                ),
            )
        } else {
            (
                0.75,
                "whisper.cpp full() decode; no tokens were readable for this segment, falling back to a neutral estimate"
                    .to_string(),
            )
        };

        Ok(DecodeOutcome::Text {
            text,
            confidence: ConfidenceResult::new(
                confidence_score,
                ConfidenceSource::Model,
                Some(confidence_note),
            ),
            language: detected_language,
        })
    }

    fn run_inference(&mut self) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        if self.buffer.is_empty() {
            self.last_feed_triggered_inference = false;
            self.last_feed_was_silence = false;
            self.last_feed_was_non_speech_placeholder = false;
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
            self.window_id = None;
            self.window_interim_decoded = false;
            self.last_feed_triggered_inference = false;
            self.last_feed_was_silence = true;
            self.last_feed_was_non_speech_placeholder = false;
            return Ok(vec![]);
        }
        self.last_feed_was_silence = false;
        self.last_feed_was_non_speech_placeholder = false;

        // Buffer/clock/window state is always consumed here, *before* the
        // fallible decode call below - exactly like the silence branch
        // above already had to be, so a decode error (whisper.cpp itself
        // failing) never leaves stale audio stuck in the buffer forever,
        // retried on every subsequent call. `audio_f32` owns its own copy,
        // so clearing `self.buffer` immediately after building it is safe.
        let audio_f32: Vec<f32> = self
            .buffer
            .iter()
            .map(|s| f32::from(*s) / f32::from(i16::MAX))
            .collect();
        self.elapsed_ms += duration_ms;
        self.buffer.clear();
        self.last_feed_triggered_inference = true;
        let window_id = self.window_id.take().unwrap_or_else(Uuid::new_v4);
        self.window_interim_decoded = false;

        let outcome = Self::decode_pass(&self.ctx, &self.requested_language, &audio_f32)?;

        match outcome {
            DecodeOutcome::Empty => Ok(vec![]),
            DecodeOutcome::Placeholder => {
                self.last_feed_was_non_speech_placeholder = true;
                Ok(vec![])
            }
            DecodeOutcome::Text {
                text,
                confidence,
                language,
            } => {
                let segment = TranscriptSegment {
                    id: window_id,
                    sequence: self.sequence,
                    text,
                    is_final: true,
                    confidence,
                    start_ms,
                    end_ms: self.elapsed_ms,
                    language: Some(language),
                    speaker_id: None,
                };
                self.sequence += 1;
                Ok(vec![segment])
            }
        }
    }

    /// Phase 24.2: attempts the current window's one allotted interim
    /// decode - see the module docs' "Interim decoding" section. Never
    /// touches `self.buffer`/`self.elapsed_ms`/`self.sequence`: the window
    /// is still accumulating audio toward its eventual final decode in
    /// `run_inference`, and must not be disturbed by this early peek.
    /// Marks the attempt as spent (`window_interim_decoded = true`)
    /// regardless of outcome - silence/a placeholder/empty text is never
    /// retried this window; the next real opportunity is the final decode
    /// when the window actually closes.
    fn try_interim_decode(&mut self) -> Result<Option<TranscriptSegment>, SpeechEngineError> {
        self.window_interim_decoded = true;

        if is_silence(&self.buffer) {
            self.last_feed_triggered_inference = false;
            self.last_feed_was_silence = true;
            self.last_feed_was_non_speech_placeholder = false;
            return Ok(None);
        }
        self.last_feed_was_silence = false;

        let audio_f32: Vec<f32> = self
            .buffer
            .iter()
            .map(|s| f32::from(*s) / f32::from(i16::MAX))
            .collect();
        self.last_feed_triggered_inference = true;

        let outcome = Self::decode_pass(&self.ctx, &self.requested_language, &audio_f32)?;
        self.last_feed_was_non_speech_placeholder = matches!(outcome, DecodeOutcome::Placeholder);

        match outcome {
            DecodeOutcome::Empty | DecodeOutcome::Placeholder => Ok(None),
            DecodeOutcome::Text {
                text,
                confidence,
                language,
            } => {
                let duration_so_far_ms =
                    (self.buffer.len() as u64 * 1000) / u64::from(SAMPLE_RATE_HZ);
                Ok(Some(TranscriptSegment {
                    id: self.window_id.expect(
                        "try_interim_decode is only ever called with a window already in progress",
                    ),
                    sequence: self.sequence,
                    text,
                    is_final: false,
                    confidence,
                    start_ms: self.elapsed_ms,
                    end_ms: self.elapsed_ms + duration_so_far_ms,
                    language: Some(language),
                    speaker_id: None,
                }))
            }
        }
    }
}

/// The three ways a real whisper.cpp `full()` decode pass can end, once
/// the pass itself has actually run (the VAD silence gate is checked
/// separately, before [`WhisperSpeechEngine::decode_pass`] is ever called -
/// see its callers).
enum DecodeOutcome {
    /// Genuine inference ran but decoded nothing (or nothing usable).
    Empty,
    /// Genuine inference ran and decoded one of whisper.cpp's own known
    /// non-speech placeholder captions - see [`NON_SPEECH_PLACEHOLDERS`].
    Placeholder,
    Text {
        text: String,
        confidence: ConfidenceResult,
        language: String,
    },
}

impl SpeechEngine for WhisperSpeechEngine {
    fn is_ready(&self) -> bool {
        true
    }

    fn feed_audio(&mut self, samples: &[i16]) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        // Phase 24.2: a new window starts the moment a previously-empty
        // buffer receives audio - allocate its stable id here, before
        // `samples` is appended, so both an interim decode and the final
        // segment it settles into share it (see `window_id`'s own docs).
        if self.buffer.is_empty() {
            self.window_id = Some(Uuid::new_v4());
            self.window_interim_decoded = false;
        }
        self.buffer.extend_from_slice(samples);
        // Phase 21: the hard cap is checked first and independently of
        // `should_flush_early` - a window that has already reached
        // `CHUNK_SAMPLES` flushes because of the cap, never mislabeled as
        // pause-triggered just because its trailing audio also happens to
        // be quiet.
        if self.buffer.len() >= CHUNK_SAMPLES {
            self.last_feed_was_vad_early_flush = false;
            self.run_inference()
        } else if should_flush_early(&self.buffer) {
            self.last_feed_was_vad_early_flush = true;
            self.run_inference()
        } else if should_attempt_interim_decode(self.buffer.len(), self.window_interim_decoded) {
            // Phase 24.2: still buffering (neither flush condition above
            // fired this call), but there's now enough audio for a
            // worthwhile interim peek - see the module docs' "Interim
            // decoding" section and `MIN_VAD_FLUSH_SAMPLES`'s own docs for
            // why an interim attempt and a genuine VAD flush never compete
            // for the same window in the same call.
            self.last_feed_was_vad_early_flush = false;
            Ok(self.try_interim_decode()?.into_iter().collect())
        } else {
            self.last_feed_triggered_inference = false;
            self.last_feed_was_silence = false;
            self.last_feed_was_non_speech_placeholder = false;
            self.last_feed_was_vad_early_flush = false;
            Ok(vec![])
        }
    }

    fn flush(&mut self) -> Result<Vec<TranscriptSegment>, SpeechEngineError> {
        // An explicit, caller-requested flush (e.g. on stop_listening) is
        // never a VAD-triggered one, regardless of what the buffer's
        // trailing audio looks like.
        self.last_feed_was_vad_early_flush = false;
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

    fn last_feed_was_non_speech_placeholder(&self) -> bool {
        self.last_feed_was_non_speech_placeholder
    }

    fn last_feed_was_vad_early_flush(&self) -> bool {
        self.last_feed_was_vad_early_flush
    }

    fn discard_buffered_audio(&mut self) {
        self.buffer.clear();
        self.window_id = None;
        self.window_interim_decoded = false;
    }

    fn set_language(&mut self, language: &str) {
        self.requested_language = language.to_string();
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

    // --- Phase 21: VAD-triggered early flush - has_trailing_pause and
    // should_flush_early, fully testable without a real model file ------

    fn tone(sample_count: usize) -> Vec<i16> {
        (0..sample_count)
            .map(|i| if i % 2 == 0 { i16::MAX } else { i16::MIN })
            .collect()
    }

    fn silence(sample_count: usize) -> Vec<i16> {
        vec![0i16; sample_count]
    }

    #[test]
    fn trailing_pause_never_fires_on_a_buffer_shorter_than_the_trailing_window() {
        let mut buffer = silence(TRAILING_SILENCE_SAMPLES - 1);
        assert!(
            !has_trailing_pause(&buffer),
            "too short to confirm a real pause"
        );
        buffer.clear();
        assert!(!has_trailing_pause(&buffer));
    }

    #[test]
    fn trailing_pause_is_false_when_the_buffer_ends_in_real_audio() {
        let buffer = tone(TRAILING_SILENCE_SAMPLES * 3);
        assert!(!has_trailing_pause(&buffer));
    }

    #[test]
    fn trailing_pause_is_true_when_the_buffer_ends_in_a_real_quiet_stretch() {
        let mut buffer = tone(TRAILING_SILENCE_SAMPLES * 3);
        buffer.extend(silence(TRAILING_SILENCE_SAMPLES));
        assert!(
            has_trailing_pause(&buffer),
            "speech followed by a genuine trailing pause must be detected"
        );
    }

    #[test]
    fn trailing_pause_ignores_silence_that_is_not_at_the_very_end() {
        // Silence up front, then real audio right up to the end - the
        // *trailing* window is what matters, not the buffer as a whole.
        let mut buffer = silence(TRAILING_SILENCE_SAMPLES * 3);
        buffer.extend(tone(TRAILING_SILENCE_SAMPLES));
        assert!(!has_trailing_pause(&buffer));
    }

    #[test]
    fn should_flush_early_requires_the_minimum_buffered_duration_even_with_a_real_pause() {
        // A pause occurring almost immediately (buffer well under
        // MIN_VAD_FLUSH_SAMPLES) must not fragment the window.
        let short_buffer = silence(TRAILING_SILENCE_SAMPLES + 100);
        assert!(short_buffer.len() < MIN_VAD_FLUSH_SAMPLES);
        assert!(has_trailing_pause(&short_buffer));
        assert!(!should_flush_early(&short_buffer));
    }

    #[test]
    fn should_flush_early_is_false_with_enough_audio_but_no_pause() {
        let buffer = tone(MIN_VAD_FLUSH_SAMPLES + TRAILING_SILENCE_SAMPLES);
        assert!(!should_flush_early(&buffer));
    }

    #[test]
    fn should_flush_early_fires_once_both_conditions_are_met() {
        let mut buffer = tone(MIN_VAD_FLUSH_SAMPLES);
        buffer.extend(silence(TRAILING_SILENCE_SAMPLES));
        assert!(should_flush_early(&buffer));
    }

    #[test]
    fn should_flush_early_never_fires_on_an_empty_buffer() {
        assert!(!should_flush_early(&[]));
    }

    // --- Phase 24.2: interim decoding's own pure gate -------------------

    #[test]
    fn interim_decode_does_not_fire_before_the_minimum_buffered_duration() {
        assert!(!should_attempt_interim_decode(
            MIN_VAD_FLUSH_SAMPLES - 1,
            false
        ));
    }

    #[test]
    fn interim_decode_fires_exactly_at_the_minimum_buffered_duration() {
        assert!(should_attempt_interim_decode(MIN_VAD_FLUSH_SAMPLES, false));
    }

    #[test]
    fn interim_decode_fires_past_the_minimum_buffered_duration_too() {
        assert!(should_attempt_interim_decode(
            MIN_VAD_FLUSH_SAMPLES + 1000,
            false
        ));
    }

    #[test]
    fn interim_decode_never_fires_twice_in_the_same_window() {
        // Plenty of buffered audio, but this window already had its one
        // allotted attempt - must not fire again regardless of how much
        // more audio has since accumulated.
        assert!(!should_attempt_interim_decode(CHUNK_SAMPLES, true));
    }

    // --- Phase 14: whisper.cpp's own non-speech placeholder captions,
    // recognized post-decode - see docs/phase-14-audit.md for the real
    // pilot session these exact strings came from ------------------------

    #[test]
    fn recognizes_every_placeholder_string_seen_on_a_real_windows_pilot() {
        // Verbatim from the real operator screenshots that prompted this
        // phase (docs/phase-14-audit.md).
        for text in [
            "[BLANK_AUDIO]",
            "(speaking in foreign language)",
            "[inaudible]",
            "[LAUGHTER]",
        ] {
            assert!(
                is_non_speech_placeholder(text),
                "{text:?} must be recognized as a known non-speech placeholder"
            );
        }
    }

    #[test]
    fn placeholder_matching_is_case_and_punctuation_insensitive() {
        for text in [
            "[blank_audio]",
            "Blank Audio",
            "[BLANK_AUDIO].",
            "  [BLANK_AUDIO]  ",
            "(BLANK_AUDIO)",
        ] {
            assert!(
                is_non_speech_placeholder(text),
                "{text:?} must still match regardless of case/bracket/whitespace style"
            );
        }
    }

    #[test]
    fn real_transcribed_speech_is_never_flagged_as_a_placeholder() {
        for text in [
            "Yeah, we did it. We did it.",
            "Turn with me to Romans chapter eight.",
            "I've got myself going on.",
            "",
            "   ",
        ] {
            assert!(
                !is_non_speech_placeholder(text),
                "{text:?} must never be discarded as a placeholder"
            );
        }
    }

    #[test]
    fn a_placeholder_tag_mixed_with_real_words_is_kept_in_full() {
        // Deliberately conservative: only a *pure* placeholder decode is
        // discarded - never guess at a segment that also contains real
        // words, matching this codebase's "never eat real content"
        // discipline (see SILENCE_RMS_THRESHOLD's own reasoning).
        assert!(!is_non_speech_placeholder("Amen. [BLANK_AUDIO]"));
        assert!(!is_non_speech_placeholder(
            "(speaking in foreign language) but then he said amen"
        ));
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
