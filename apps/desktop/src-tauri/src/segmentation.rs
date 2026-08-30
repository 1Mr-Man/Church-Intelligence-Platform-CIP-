//! Phase 3.8.7.5 Part A - bounded transcript segmentation.
//!
//! Whisper's own buffering window (`ai/speech/src/whisper.rs::CHUNK_SAMPLES`,
//! 3.0s) already produces one "final" [`TranscriptSegment`] roughly every
//! 3 seconds of real-time audio - a latency/memory bound on whisper.cpp's
//! synchronous inference API, not a meaningful unit of speech. Routing
//! Bible/Sermon/Service/Music analysis off of every ~3s fragment
//! independently would mean each engine sees a sentence chopped into
//! several pieces, and would triple-or-more the database/event volume
//! for no analytical benefit.
//!
//! [`TranscriptSegmenter`] sits between Whisper's raw per-window output and
//! the rest of the live pipeline: it concatenates consecutive raw segments'
//! text into one bounded logical window (target ~15s, per the operator's
//! own 12-20s spec) and hands back a single, complete [`TranscriptSegment`]
//! once that window closes - the unit everything downstream (Bible
//! detection, the Live Intelligence Router, persistence, the frontend
//! transcript display) actually operates on.
//!
//! Deliberately does **not** attempt pause/silence-based early flushing:
//! `AudioEngine`/`WhisperSpeechEngine` expose no voice-activity signal
//! today (Whisper's buffer fills at a fixed audio-time cadence regardless
//! of whether the speaker is talking or silent - see
//! `docs/phase-3-8-7-5-audit.md`), and inventing one here would mean
//! guessing at a boundary CIP cannot actually detect. A fixed, honest
//! time-window is the only trigger implemented this phase; a real
//! pause-aware "hybrid segmenter" remains a distinct, larger, future
//! design (it would need new evidence from the audio/Whisper layer this
//! phase deliberately does not touch - Phase 3.8.7.3 finally stabilized
//! that layer, and the operator's own instruction is not to modify it
//! again without evidence requiring it).
//!
//! Deliberately Tauri-agnostic (plain domain types, no `AppHandle`/`State`),
//! matching `pipeline.rs`/`persistence.rs`'s own discipline, and owned
//! exclusively by one `spawn_speech_worker` thread for the lifetime of one
//! listening session - never shared, never behind a `Mutex` (mirrors
//! `acoustic::AcousticWorkerState`'s "owns its own state exclusively"
//! pattern).

use cip_core_ai::TranscriptSegment;
use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use uuid::Uuid;

/// Target accumulated-audio span (milliseconds) before a window closes -
/// the middle of the operator's requested 12-20s band. Given Whisper's
/// own fixed ~3.0s emission cadence (`CHUNK_SAMPLES`), a window closes
/// the first time its span reaches this value, which in practice lands
/// between 15s and ~18s - comfortably inside the requested band without
/// a separate, redundant "max" constant (span only ever grows in ~3s
/// steps under this phase's unchanged Whisper buffering window).
const SEGMENT_TARGET_WINDOW_MS: u64 = 15_000;

/// Accumulates consecutive raw, already-final `TranscriptSegment`s (each
/// one Whisper's own ~3s buffering window) into one bounded logical
/// segment.
pub struct TranscriptSegmenter {
    buffer: String,
    first_start_ms: Option<u64>,
    last_end_ms: u64,
    confidence_sum: f32,
    confidence_count: u32,
    language: Option<String>,
    speaker_id: Option<String>,
}

impl TranscriptSegmenter {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            first_start_ms: None,
            last_end_ms: 0,
            confidence_sum: 0.0,
            confidence_count: 0,
            language: None,
            speaker_id: None,
        }
    }

    /// Appends one raw finalized segment's text. Returns `Some` with a
    /// completed, bounded logical segment once the accumulated span
    /// reaches [`SEGMENT_TARGET_WINDOW_MS`] - `None` while still
    /// accumulating (the common case, since each raw segment only covers
    /// ~3s).
    pub fn push(&mut self, raw: &TranscriptSegment) -> Option<TranscriptSegment> {
        let trimmed = raw.text.trim();
        if !trimmed.is_empty() {
            if self.buffer.is_empty() {
                self.first_start_ms = Some(raw.start_ms);
            } else {
                self.buffer.push(' ');
            }
            self.buffer.push_str(trimmed);
        }
        self.last_end_ms = raw.end_ms;
        self.confidence_sum += raw.confidence.score;
        self.confidence_count += 1;
        if raw.language.is_some() {
            self.language = raw.language.clone();
        }
        if raw.speaker_id.is_some() {
            self.speaker_id = raw.speaker_id.clone();
        }

        let span_ms = self
            .last_end_ms
            .saturating_sub(self.first_start_ms.unwrap_or(self.last_end_ms));
        if span_ms >= SEGMENT_TARGET_WINDOW_MS {
            self.flush()
        } else {
            None
        }
    }

    /// Force-closes whatever is currently buffered, regardless of span -
    /// used when listening stops mid-window, so the last few seconds of
    /// real speech are never silently dropped. `None` if nothing is
    /// buffered (a clean stop right at a window boundary).
    pub fn flush_remaining(&mut self) -> Option<TranscriptSegment> {
        if self.buffer.trim().is_empty() {
            self.reset();
            None
        } else {
            self.flush()
        }
    }

    /// Discards whatever is currently buffered without producing a
    /// segment - used when the speech worker's own overload/backlog-drain
    /// logic (Phase 3.8.7.3) discards stale queued audio. Without this,
    /// text accumulated just before an overload gap could be spliced onto
    /// unrelated text arriving after recovery, the same discontinuous-
    /// buffer problem `SpeechEngine::discard_buffered_audio` exists to
    /// prevent one layer down.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.first_start_ms = None;
        self.last_end_ms = 0;
        self.confidence_sum = 0.0;
        self.confidence_count = 0;
        self.language = None;
        self.speaker_id = None;
    }

    fn flush(&mut self) -> Option<TranscriptSegment> {
        if self.buffer.trim().is_empty() {
            self.reset();
            return None;
        }
        let avg_confidence = if self.confidence_count > 0 {
            self.confidence_sum / self.confidence_count as f32
        } else {
            0.0
        };
        let segment = TranscriptSegment {
            id: Uuid::new_v4(),
            // Overwritten by the caller from `AppState.transcript_sequence`
            // (the single, shared, correctly-ordered counter) - mirrors
            // exactly how the pre-segmentation code already assigned
            // sequence numbers to each raw final segment.
            sequence: 0,
            text: std::mem::take(&mut self.buffer),
            is_final: true,
            confidence: ConfidenceResult::new(
                avg_confidence,
                ConfidenceSource::Model,
                Some("averaged across an accumulated speech segment".to_string()),
            ),
            start_ms: self.first_start_ms.unwrap_or(self.last_end_ms),
            end_ms: self.last_end_ms,
            language: self.language.clone(),
            speaker_id: self.speaker_id.clone(),
        };
        self.reset();
        Some(segment)
    }
}

impl Default for TranscriptSegmenter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str, start_ms: u64, end_ms: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence: 0,
            text: text.to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(0.8, ConfidenceSource::Model, None),
            start_ms,
            end_ms,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    #[test]
    fn stays_open_below_the_target_window() {
        let mut seg = TranscriptSegmenter::new();
        assert!(seg.push(&raw("Open your Bible", 0, 3_000)).is_none());
        assert!(seg
            .push(&raw("to Matthew chapter six", 3_000, 6_000))
            .is_none());
        assert!(seg.push(&raw("verse nine", 6_000, 9_000)).is_none());
    }

    #[test]
    fn flushes_once_the_target_window_is_reached_and_concatenates_text_in_order() {
        let mut seg = TranscriptSegmenter::new();
        assert!(seg.push(&raw("Open your Bible", 0, 3_000)).is_none());
        assert!(seg
            .push(&raw("to Matthew chapter six", 3_000, 6_000))
            .is_none());
        assert!(seg.push(&raw("verse nine", 6_000, 9_000)).is_none());
        assert!(seg.push(&raw("about prayer", 9_000, 12_000)).is_none());
        let flushed = seg
            .push(&raw("and fasting today", 12_000, 15_000))
            .expect("span has now reached the 15s target");
        assert_eq!(
            flushed.text,
            "Open your Bible to Matthew chapter six verse nine about prayer and fasting today"
        );
        assert_eq!(flushed.start_ms, 0);
        assert_eq!(flushed.end_ms, 15_000);
        assert!(flushed.is_final);
    }

    #[test]
    fn a_new_window_starts_clean_after_a_flush_no_reprocessing_of_old_text() {
        let mut seg = TranscriptSegmenter::new();
        for i in 0..5 {
            seg.push(&raw("chunk", i * 3_000, (i + 1) * 3_000));
        }
        // Next chunk starts a fresh window - the flushed text must never
        // reappear.
        assert!(seg
            .push(&raw("brand new sentence", 15_000, 18_000))
            .is_none());
        let flushed = seg.flush_remaining().unwrap();
        assert_eq!(flushed.text, "brand new sentence");
    }

    #[test]
    fn whitespace_only_raw_segments_never_produce_a_flushed_segment_on_their_own() {
        let mut seg = TranscriptSegmenter::new();
        assert!(seg.push(&raw("   ", 0, 3_000)).is_none());
        assert!(seg.flush_remaining().is_none());
    }

    #[test]
    fn flush_remaining_returns_none_when_nothing_is_buffered() {
        let mut seg = TranscriptSegmenter::new();
        assert!(seg.flush_remaining().is_none());
    }

    #[test]
    fn flush_remaining_force_closes_a_short_partial_window_on_stop() {
        let mut seg = TranscriptSegmenter::new();
        seg.push(&raw("Let us pray", 0, 3_000));
        let flushed = seg
            .flush_remaining()
            .expect("a short but real partial window must not be silently dropped on stop");
        assert_eq!(flushed.text, "Let us pray");
        assert_eq!(flushed.end_ms, 3_000);
    }

    #[test]
    fn reset_discards_buffered_text_without_producing_a_segment() {
        let mut seg = TranscriptSegmenter::new();
        seg.push(&raw("stale text before an overload gap", 0, 3_000));
        seg.reset();
        assert!(seg.flush_remaining().is_none());
        // The next window starts clean - no splice of pre-overload text.
        assert!(seg
            .push(&raw("fresh text after recovery", 100_000, 103_000))
            .is_none());
        let flushed = seg.flush_remaining().unwrap();
        assert_eq!(flushed.text, "fresh text after recovery");
    }

    #[test]
    fn averages_confidence_across_every_accumulated_raw_segment() {
        let mut seg = TranscriptSegmenter::new();
        let a = TranscriptSegment {
            confidence: ConfidenceResult::new(1.0, ConfidenceSource::Model, None),
            ..raw("high confidence", 0, 3_000)
        };
        let b = TranscriptSegment {
            confidence: ConfidenceResult::new(0.5, ConfidenceSource::Model, None),
            ..raw("lower confidence", 3_000, 6_000)
        };
        seg.push(&a);
        seg.push(&b);
        let flushed = seg.flush_remaining().unwrap();
        assert!((flushed.confidence.score - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn a_single_raw_segment_at_or_past_the_target_flushes_immediately() {
        // A backlog-recovery scenario: one raw segment alone already spans
        // the whole target window - must still flush, not wait for a
        // second push that may never come at the right size.
        let mut seg = TranscriptSegmenter::new();
        let flushed = seg
            .push(&raw("a long recovered segment", 0, 15_000))
            .expect("a single segment already at the target span must flush immediately");
        assert_eq!(flushed.text, "a long recovered segment");
    }
}
