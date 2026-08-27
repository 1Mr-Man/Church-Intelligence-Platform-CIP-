//! Acoustic (audio-fingerprint) recognition orchestration (Phase 2.2) -
//! the acoustic-domain counterpart to `music.rs`. Deliberately
//! Tauri-agnostic (plain functions/types, no `AppHandle`/`State`, matching
//! `content.rs`/`music.rs`/`presentation.rs`) so segmentation, the
//! signal-quality gate, rate limiting, recognition, and fusion-and-queue
//! are all directly unit-testable without a real microphone, a background
//! thread, or Tauri running at all.
//!
//! The actual background-thread/channel wiring that feeds this module
//! from live audio lives in `commands.rs` (`spawn_acoustic_worker`,
//! called from `start_listening`'s sink closure alongside the existing
//! speech-engine feed) - mirroring how `pipeline.rs` stays Tauri-agnostic
//! while `commands.rs::handle_audio_chunk` does the actual calling. See
//! `docs/acoustic-music.md`.

use cip_core_intelligence::{
    FindingQueue, IntelligenceContext, IntelligenceError, IntelligenceFinding,
    MusicIntelligenceEngine, QueueAddOutcome,
};
use cip_core_music::{
    assess_signal_quality, AcousticAnalysisConfig, AcousticMusicRecognizer,
    AcousticRecognitionError, AcousticRecognitionMethod, AcousticRecognitionStatus, AudioSegment,
    AudioSegmenter, SignalQuality,
};
use thiserror::Error;
use uuid::Uuid;

/// Bounded queue depth for the channel feeding the acoustic worker thread
/// from the audio capture thread - Phase 2.2's "never unbounded async/
/// task queues" rule, applied at the channel boundary the same way
/// `AudioSegmenter`'s own internal buffer is bounded. If the worker falls
/// behind, the sender (`commands.rs`'s sink closure) uses `try_send` and
/// drops the newest chunk rather than blocking the audio capture thread
/// or growing without limit - real-time audio that cannot be analyzed in
/// time is stale anyway.
pub const ACOUSTIC_CHANNEL_CAPACITY: usize = 8;

/// Whether/why acoustic recognition can currently run - part of
/// `commands::LiveStatus.acoustic_status` (reusing the existing
/// `get_live_status` diagnostic command rather than adding a dedicated
/// `get_acoustic_music_status` one - "reuse existing commands where they
/// already provide the behavior"). Deliberately plain data (not a bare
/// re-exported `AcousticRecognitionStatus`) so `reason` travels with it in
/// one serializable value, matching how `AudioEngineStatus`/
/// `SpeechEngineStatus`-shaped diagnostics already work elsewhere in this
/// app.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcousticEngineStatus {
    pub status: AcousticRecognitionStatus,
    pub method: AcousticRecognitionMethod,
    pub reason: Option<String>,
}

pub fn describe_status(recognizer: &dyn AcousticMusicRecognizer) -> AcousticEngineStatus {
    AcousticEngineStatus {
        status: recognizer.status(),
        method: recognizer.method(),
        reason: recognizer.status_reason(),
    }
}

/// Every currently-enabled Music dataset visible in `context` - derived
/// from the same `IntelligenceContext.content_metadata` already built for
/// this call (mirroring `commands::enabled_music_content_ids`, but
/// working from an already-built context instead of a fresh
/// `ContentRegistry` call, since the acoustic worker builds one context
/// per analysis pass anyway). Passed to the recognizer as the
/// `content_ids` its trait contract requires be pre-scoped to enabled
/// datasets; `MusicIntelligenceEngine::analyze_acoustic` independently
/// re-checks the same thing as defense in depth.
pub fn enabled_music_dataset_ids(context: &IntelligenceContext) -> Vec<String> {
    context
        .content_metadata
        .iter()
        .filter(|m| {
            m.content_type == cip_core_content::ContentType::Music
                && m.status == cip_core_content::ContentStatus::Enabled
        })
        .map(|m| m.id.clone())
        .collect()
}

/// Everything the acoustic worker needs to remember between chunks - owned
/// exclusively by one worker (thread or otherwise), never shared behind a
/// `Mutex` the way `AppState`'s other fields are, since only the worker
/// itself ever touches it (see `commands.rs::spawn_acoustic_worker`).
pub struct AcousticWorkerState {
    segmenter: AudioSegmenter,
    config: AcousticAnalysisConfig,
    last_recognition_at_ms: Option<u64>,
}

impl AcousticWorkerState {
    pub fn new(config: AcousticAnalysisConfig) -> Self {
        Self {
            segmenter: AudioSegmenter::new(config),
            config,
            last_recognition_at_ms: None,
        }
    }

    /// Feed one chunk of raw mono PCM16 audio, returning a segment exactly
    /// when the segmenter has accumulated a full window - a thin,
    /// testable wrapper so this type's own tests do not need to reach
    /// into `AudioSegmenter` directly.
    pub fn ingest(&mut self, samples: &[i16], sample_rate_hz: u32) -> Option<AudioSegment> {
        self.segmenter.push(samples, sample_rate_hz)
    }

    /// Whether `segment` is both quality-gated `Ready` *and* far enough
    /// past the last attempted recognition to respect
    /// `AcousticAnalysisConfig::minimum_recognition_interval_ms` - the two
    /// independent reasons a window might be skipped without ever calling
    /// the (comparatively expensive) recognizer, combined into one
    /// deterministic decision. Does not itself record an attempt - see
    /// `record_recognition_attempt`, called only once the caller has
    /// actually gone on to call the recognizer.
    pub fn should_attempt_recognition(&self, segment: &AudioSegment) -> bool {
        if assess_signal_quality(segment, &self.config) != SignalQuality::Ready {
            return false;
        }
        match self.last_recognition_at_ms {
            None => true,
            Some(last) => {
                segment.started_at_ms.saturating_sub(last)
                    >= self.config.minimum_recognition_interval_ms
            }
        }
    }

    pub fn record_recognition_attempt(&mut self, segment: &AudioSegment) {
        self.last_recognition_at_ms = Some(segment.started_at_ms);
    }
}

#[derive(Debug, Error)]
pub enum AcousticAnalysisError {
    #[error(transparent)]
    Recognizer(#[from] AcousticRecognitionError),
    #[error(transparent)]
    Engine(#[from] IntelligenceError),
}

/// One full acoustic-analysis pass over an already rate/quality-gated
/// `segment`: recognizer -> `MusicIntelligenceEngine::analyze_acoustic`
/// fusion -> queue only genuinely new findings. Mirrors
/// `music::analyze_and_queue`'s split exactly (call the engine, then queue
/// only what `FindingQueue::add` accepts as non-duplicate) so acoustic and
/// lyric findings share one dedup/queuing discipline, not two. Never
/// prepares or projects anything - this function has no way to reach
/// `cip_core_presentation` at all (see this module's imports).
#[allow(clippy::too_many_arguments)]
pub fn recognize_fuse_and_queue(
    recognizer: &mut dyn AcousticMusicRecognizer,
    engine: &MusicIntelligenceEngine,
    segment: &AudioSegment,
    content_ids: &[String],
    service_id: Uuid,
    context: &IntelligenceContext,
    findings: &mut FindingQueue,
) -> Result<Vec<IntelligenceFinding>, AcousticAnalysisError> {
    let candidates = recognizer.recognize(segment, content_ids)?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let result = engine.analyze_acoustic(service_id, &candidates, context)?;
    let mut queued = Vec::new();
    for finding in result.findings {
        if findings.add(finding.clone()) == QueueAddOutcome::Added {
            queued.push(finding);
        }
    }
    Ok(queued)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cip_core_content::{ContentMetadata, ContentStatus, ContentType};
    use cip_core_intelligence::ContextBounds;
    use cip_core_music::{
        AcousticRecognitionCandidate, AcousticRecognitionMethod as Method, MusicProvider,
    };
    use cip_integrations_music_acoustic::{
        NullAcousticMusicRecognizer, ScriptedAcousticMusicRecognizer, ScriptedAcousticStep,
    };

    fn loud_samples(n: usize) -> Vec<i16> {
        vec![10_000; n]
    }

    fn window_samples() -> usize {
        // AcousticAnalysisConfig::default().window_ms (8000) at 16kHz.
        8_000 * 16_000 / 1000
    }

    #[test]
    fn worker_state_emits_nothing_until_a_full_window_accumulates() {
        let mut worker = AcousticWorkerState::new(AcousticAnalysisConfig::default());
        assert!(worker.ingest(&loud_samples(16_000), 16_000).is_none());
    }

    #[test]
    fn worker_state_emits_a_segment_once_a_window_is_full() {
        let mut worker = AcousticWorkerState::new(AcousticAnalysisConfig::default());
        let segment = worker
            .ingest(&loud_samples(window_samples()), 16_000)
            .expect("a full window must emit a segment");
        assert_eq!(segment.duration_ms, 8_000);
    }

    #[test]
    fn a_loud_full_window_should_attempt_recognition_on_the_first_call() {
        let worker = AcousticWorkerState::new(AcousticAnalysisConfig::default());
        let segment = AudioSegment::new(loud_samples(window_samples()), 16_000, 0);
        assert!(worker.should_attempt_recognition(&segment));
    }

    #[test]
    fn a_silent_window_never_triggers_a_recognition_attempt() {
        let worker = AcousticWorkerState::new(AcousticAnalysisConfig::default());
        let segment = AudioSegment::new(vec![0; window_samples()], 16_000, 0);
        assert!(!worker.should_attempt_recognition(&segment));
    }

    #[test]
    fn a_too_short_window_never_triggers_a_recognition_attempt() {
        let worker = AcousticWorkerState::new(AcousticAnalysisConfig::default());
        let segment = AudioSegment::new(loud_samples(1_000), 16_000, 0);
        assert!(!worker.should_attempt_recognition(&segment));
    }

    #[test]
    fn rate_limiting_blocks_a_second_attempt_too_soon_after_the_first() {
        let mut worker = AcousticWorkerState::new(AcousticAnalysisConfig::default());
        let first = AudioSegment::new(loud_samples(window_samples()), 16_000, 0);
        assert!(worker.should_attempt_recognition(&first));
        worker.record_recognition_attempt(&first);

        // minimum_recognition_interval_ms defaults to 5_000; this segment
        // starts only 1_000ms later.
        let mut too_soon = AudioSegment::new(loud_samples(window_samples()), 16_000, 0);
        too_soon.started_at_ms = 1_000;
        assert!(!worker.should_attempt_recognition(&too_soon));
    }

    #[test]
    fn rate_limiting_allows_a_second_attempt_once_the_interval_has_passed() {
        let mut worker = AcousticWorkerState::new(AcousticAnalysisConfig::default());
        let first = AudioSegment::new(loud_samples(window_samples()), 16_000, 0);
        worker.record_recognition_attempt(&first);

        let mut later = AudioSegment::new(loud_samples(window_samples()), 16_000, 0);
        later.started_at_ms = 6_000;
        assert!(worker.should_attempt_recognition(&later));
    }

    fn enabled_music_content(id: &str) -> ContentMetadata {
        ContentMetadata {
            id: id.to_string(),
            content_type: ContentType::Music,
            name: "Test".to_string(),
            version: "dev-fixture".to_string(),
            language: "en".to_string(),
            source: "test fixture".to_string(),
            publisher: None,
            copyright: None,
            license: None,
            distribution: None,
            imported_at: Utc::now(),
            checksum: None,
            status: ContentStatus::Enabled,
            licensing_status: cip_core_content::LicensingStatus::Unknown,
        }
    }

    fn context_with(content_metadata: Vec<ContentMetadata>) -> IntelligenceContext {
        IntelligenceContext::build(
            Uuid::new_v4(),
            None,
            None,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            content_metadata,
            ContextBounds::default(),
        )
    }

    #[test]
    fn enabled_music_dataset_ids_filters_to_enabled_music_content() {
        let mut disabled = enabled_music_content("music:off");
        disabled.status = ContentStatus::Disabled;
        let context = context_with(vec![enabled_music_content("music:on"), disabled]);
        let ids = enabled_music_dataset_ids(&context);
        assert_eq!(ids, vec!["music:on".to_string()]);
    }

    fn empty_provider() -> impl MusicProvider {
        let mut conn = cip_database::open_in_memory().unwrap();
        cip_database::run_migrations(&mut conn).unwrap();
        cip_integrations_music::SqliteMusicProvider::new(conn)
    }

    fn acoustic_candidate(
        song_id: &str,
        content_id: &str,
        score: f32,
    ) -> AcousticRecognitionCandidate {
        AcousticRecognitionCandidate {
            song_id: song_id.to_string(),
            content_id: content_id.to_string(),
            confidence: cip_core_confidence::ConfidenceResult::new(
                score,
                cip_core_confidence::ConfidenceSource::Model,
                None,
            ),
            method: Method::Test,
            segment_id: Uuid::nil(),
            duration_ms: 8_000,
            evidence: vec!["test evidence".to_string()],
        }
    }

    #[test]
    fn recognize_fuse_and_queue_queues_a_finding_for_a_scripted_candidate() {
        let engine = MusicIntelligenceEngine::new(Box::new(empty_provider()));
        let mut recognizer = ScriptedAcousticMusicRecognizer::new(vec![
            ScriptedAcousticStep::Candidates(vec![acoustic_candidate("h1", "music:test", 0.8)]),
        ]);
        let context = context_with(vec![enabled_music_content("music:test")]);
        let segment = AudioSegment::new(loud_samples(window_samples()), 16_000, 0);
        let mut findings = FindingQueue::new();

        let queued = recognize_fuse_and_queue(
            &mut recognizer,
            &engine,
            &segment,
            &["music:test".to_string()],
            context.service_id,
            &context,
            &mut findings,
        )
        .unwrap();

        assert_eq!(queued.len(), 1);
        assert_eq!(findings.pending().len(), 1);
    }

    #[test]
    fn recognize_fuse_and_queue_produces_no_findings_when_the_recognizer_finds_nothing() {
        let engine = MusicIntelligenceEngine::new(Box::new(empty_provider()));
        let mut recognizer =
            ScriptedAcousticMusicRecognizer::new(vec![ScriptedAcousticStep::NoResult]);
        let context = context_with(vec![enabled_music_content("music:test")]);
        let segment = AudioSegment::new(loud_samples(window_samples()), 16_000, 0);
        let mut findings = FindingQueue::new();

        let queued = recognize_fuse_and_queue(
            &mut recognizer,
            &engine,
            &segment,
            &["music:test".to_string()],
            context.service_id,
            &context,
            &mut findings,
        )
        .unwrap();

        assert!(queued.is_empty());
        assert!(findings.is_empty());
    }

    #[test]
    fn recognize_fuse_and_queue_surfaces_an_unavailable_recognizer_as_an_error_never_a_fake_result()
    {
        let engine = MusicIntelligenceEngine::new(Box::new(empty_provider()));
        let mut recognizer = NullAcousticMusicRecognizer;
        let context = context_with(vec![enabled_music_content("music:test")]);
        let segment = AudioSegment::new(loud_samples(window_samples()), 16_000, 0);
        let mut findings = FindingQueue::new();

        let result = recognize_fuse_and_queue(
            &mut recognizer,
            &engine,
            &segment,
            &["music:test".to_string()],
            context.service_id,
            &context,
            &mut findings,
        );

        assert!(matches!(
            result,
            Err(AcousticAnalysisError::Recognizer(
                AcousticRecognitionError::Unavailable(_)
            ))
        ));
        assert!(findings.is_empty());
    }

    #[test]
    fn recognize_fuse_and_queue_surfaces_a_recognizer_failure_never_silently_swallowed() {
        let engine = MusicIntelligenceEngine::new(Box::new(empty_provider()));
        let mut recognizer =
            ScriptedAcousticMusicRecognizer::new(vec![ScriptedAcousticStep::Error(
                "simulated model crash".to_string(),
            )]);
        let context = context_with(vec![enabled_music_content("music:test")]);
        let segment = AudioSegment::new(loud_samples(window_samples()), 16_000, 0);
        let mut findings = FindingQueue::new();

        let result = recognize_fuse_and_queue(
            &mut recognizer,
            &engine,
            &segment,
            &["music:test".to_string()],
            context.service_id,
            &context,
            &mut findings,
        );

        assert!(matches!(
            result,
            Err(AcousticAnalysisError::Recognizer(
                AcousticRecognitionError::RecognitionFailed(_)
            ))
        ));
    }

    #[test]
    fn describe_status_reports_the_recognizers_own_status_and_reason() {
        let recognizer = NullAcousticMusicRecognizer;
        let status = describe_status(&recognizer);
        assert_eq!(status.status, AcousticRecognitionStatus::Unavailable);
        assert_eq!(status.method, Method::None);
        assert!(status.reason.is_some());
    }

    // --- operator workflow + song transition (Phase 2.2 acceptance) --------
    //
    // Exercises the same sequence `commands::accept_music_finding`/
    // `commands::spawn_acoustic_worker` perform, without needing
    // `AppHandle`/`State` machinery (this crate has no `tauri::test`
    // harness - see `commands.rs`'s own note on this) - a detected
    // finding is never presented/current until an operator explicitly
    // accepts it, and accepting one song never silently promotes a later,
    // merely-detected candidate for a different song.

    #[test]
    fn operator_accept_is_the_only_way_a_current_song_is_derived() {
        let engine = MusicIntelligenceEngine::new(Box::new(empty_provider()));
        let mut recognizer = ScriptedAcousticMusicRecognizer::new(vec![
            ScriptedAcousticStep::Candidates(vec![acoustic_candidate("h1", "music:test", 0.85)]),
        ]);
        let context = context_with(vec![enabled_music_content("music:test")]);
        let segment = AudioSegment::new(loud_samples(window_samples()), 16_000, 0);
        let mut findings = FindingQueue::new();

        let queued = recognize_fuse_and_queue(
            &mut recognizer,
            &engine,
            &segment,
            &["music:test".to_string()],
            context.service_id,
            &context,
            &mut findings,
        )
        .unwrap();
        assert_eq!(queued.len(), 1);
        let id = queued[0].id;

        // Merely detected - not yet current.
        assert!(
            crate::music::current_song_from_finding(&queued[0]).is_some(),
            "a genuine acoustic finding always carries derivable song evidence"
        );
        assert_eq!(
            findings.get(id).unwrap().status,
            cip_core_intelligence::FindingStatus::Detected
        );

        // Only the explicit accept (mirroring `commands::accept_music_finding`)
        // makes this song "current."
        findings.accept(id).unwrap();
        let accepted = findings.get(id).unwrap();
        let current = crate::music::current_song_from_finding(accepted)
            .expect("an accepted acoustic finding derives a CurrentSong");
        assert_eq!(current.content_id, "music:test");
        assert_eq!(current.song_id, "h1");
    }

    #[test]
    fn a_later_candidate_for_a_different_song_never_silently_becomes_current() {
        let engine = MusicIntelligenceEngine::new(Box::new(empty_provider()));
        let content = vec![enabled_music_content("music:test")];
        let mut findings = FindingQueue::new();

        // Window 1: "h1" detected and operator-accepted - this is the only
        // song ever explicitly confirmed in this test.
        let mut first_recognizer = ScriptedAcousticMusicRecognizer::new(vec![
            ScriptedAcousticStep::Candidates(vec![acoustic_candidate("h1", "music:test", 0.9)]),
        ]);
        let context1 = context_with(content.clone());
        let segment1 = AudioSegment::new(loud_samples(window_samples()), 16_000, 0);
        let first = recognize_fuse_and_queue(
            &mut first_recognizer,
            &engine,
            &segment1,
            &["music:test".to_string()],
            context1.service_id,
            &context1,
            &mut findings,
        )
        .unwrap();
        let first_id = first[0].id;
        findings.accept(first_id).unwrap();
        let current_song = crate::music::current_song_from_finding(findings.get(first_id).unwrap())
            .expect("accepted finding derives a current song");
        assert_eq!(current_song.song_id, "h1");

        // Window 2: acoustic evidence now names a *different* song ("h2") -
        // a real song transition. It is queued as its own new `Detected`
        // finding; nothing here ever mutates `current_song` on its own -
        // that mutation only happens inside `commands::accept_music_finding`,
        // which this test deliberately never calls for "h2".
        let mut second_recognizer = ScriptedAcousticMusicRecognizer::new(vec![
            ScriptedAcousticStep::Candidates(vec![acoustic_candidate("h2", "music:test", 0.9)]),
        ]);
        let context2 = context_with(content);
        let segment2 = AudioSegment::new(loud_samples(window_samples()), 16_000, 6_000);
        let second = recognize_fuse_and_queue(
            &mut second_recognizer,
            &engine,
            &segment2,
            &["music:test".to_string()],
            context2.service_id,
            &context2,
            &mut findings,
        )
        .unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(
            second[0].status,
            cip_core_intelligence::FindingStatus::Detected
        );

        // `current_song` (derived only from what was actually accepted)
        // still names "h1" - the transition was detected, never
        // auto-declared current.
        assert_eq!(current_song.song_id, "h1");
        assert_eq!(
            findings.pending().len(),
            1,
            "the transition candidate is the only thing still awaiting operator review"
        );
    }
}
