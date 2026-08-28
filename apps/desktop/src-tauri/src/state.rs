//! Managed Tauri application state.
//!
//! One `AppState` is constructed during `setup` (see `lib.rs`) and handed
//! to every command via `tauri::State`. It holds the resolved config, the
//! single SQLite connection, the `BibleProvider`, and - as of Phase 1.2 -
//! the live-service pieces: the audio/speech engines (each independently
//! swappable behind their trait, per the architecture), the Scripture
//! Context Manager, and whichever `ServiceSession` is currently active.
//!
//! Each piece that must be mutated from multiple call sites (the sync
//! Tauri command thread pool, and the audio engine's own capture thread
//! via its chunk sink) is behind its own `Mutex` - CIP is a single-writer
//! desktop app, so per-field locks are the right granularity: a slow
//! database write never blocks reading audio status, and vice versa (see
//! `docs/live-speech.md`'s concurrency section).

use crate::config::AppConfig;
use chrono::{DateTime, Utc};
use cip_core_ai::SpeechEngine;
use cip_core_bible::{BibleProvider, DefaultScriptureContextManager};
use cip_core_content::ContentRegistry;
use cip_core_intelligence::{
    ContentCandidateQueue, CorrelationQueue, FindingQueue, IntelligenceEngineRegistry,
    MusicIntelligenceEngine, SermonIntelligenceEngine, ServiceIntelligenceEngine,
};
use cip_core_music::{AcousticMusicRecognizer, CurrentSong, MusicProvider};
use cip_core_sermon::foundation::{Sermon, SermonSection};
use cip_core_service::{AudioEngine, ServiceSession};
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

/// The one Bible translation Phase 1.2 operates against. Not user-facing
/// configuration yet - `core/bible::BibleProvider::list_translations` is
/// already multi-translation-capable; wiring a picker through is future
/// UI work, not a Bible Intelligence Core limitation.
pub const DEFAULT_TRANSLATION_ID: &str = "KJV";

/// Phase 3.8.6: retains what `create_speech_engine` (startup model load)
/// and `handle_audio_chunk` (per-chunk inference) already observe about
/// the speech pipeline, instead of discarding it after logging - see
/// `commands::PilotDiagnostics`'s `speech` field for the operator-facing
/// view. Never a second source of truth: every field mirrors a real event
/// that already happened, using the same `SpeechEngineError` text
/// `WhisperSpeechEngine` already produces.
#[derive(Debug, Default, Clone)]
pub struct SpeechDiagnostics {
    /// Whether this binary was compiled with the `whisper` Cargo feature.
    pub feature_compiled: bool,
    /// Whether `create_speech_engine` attempted `WhisperSpeechEngine::load`
    /// at startup (only happens when `feature_compiled` is true).
    pub model_load_attempted: bool,
    /// Whether that attempt succeeded - distinct from "a file exists at
    /// the configured path" (`WhisperModelDiagnostic::Present`), which
    /// only proves the file is readable, not that it parsed as a valid
    /// ggml/gguf model.
    pub model_loaded: bool,
    /// The real error text from a failed load attempt, if any.
    pub model_load_error: Option<String>,
    /// Total `AudioChunk`s delivered to the speech engine so far this
    /// process (across every `start_listening` call, not just the
    /// current one - a simple running counter, not per-session).
    pub chunks_received: u64,
    pub last_chunk_sample_rate_hz: Option<u32>,
    pub last_chunk_sample_count: Option<usize>,
    /// `None` when the engine's `required_sample_rate_hz()` was `None`
    /// (no resampling needed) or matched the chunk's own rate already.
    pub last_resampled_sample_count: Option<usize>,
    pub inferences_attempted: u64,
    pub inferences_succeeded: u64,
    /// The real error text from the most recent `feed_audio` failure -
    /// mirrors `AppState::speech_error` but retained here even after a
    /// later success clears that field, so a diagnostics read always has
    /// something to show once at least one failure has occurred.
    pub last_error: Option<String>,
}

pub struct AppState {
    pub config: AppConfig,
    pub db: Mutex<rusqlite::Connection>,
    pub bible_provider: Box<dyn BibleProvider>,
    /// What local content exists (Phase 1.5) - a translation's own text
    /// still comes from `bible_provider`; this answers "what's
    /// installed, and under what license/version" (see
    /// `docs/content-registry.md`). Its own connection, like
    /// `bible_provider`'s, for the same reason: an independent read path
    /// that never contends with the primary `db` mutex.
    pub content_registry: Box<dyn ContentRegistry>,
    /// The Phase 2.0 intelligence engine registry - the Bible
    /// compatibility adapter and (Phase 2.1) the Music engine are
    /// registered here (see `intelligence.rs`/`music.rs`). Exercised by
    /// `get_intelligence_capabilities` and the manual `analyze_music_transcript`
    /// command; nothing in the live audio/speech transcript pipeline
    /// calls into it yet.
    pub intelligence_registry: IntelligenceEngineRegistry,
    /// Music's own read path (Phase 2.1), mirroring `bible_provider`'s
    /// dedicated connection.
    pub music_provider: Box<dyn MusicProvider>,
    /// In-memory queue of intelligence findings awaiting operator review
    /// (Phase 2.0's `FindingQueue`, first given a real writer in Phase
    /// 2.1 by `analyze_music_transcript`). Deliberately not persisted -
    /// see `docs/music-intelligence.md`'s persistence-decision section.
    pub intelligence_findings: Mutex<FindingQueue>,
    pub context_manager: Mutex<DefaultScriptureContextManager>,
    pub audio_engine: Mutex<Box<dyn AudioEngine>>,
    pub speech_engine: Mutex<Box<dyn SpeechEngine>>,
    pub active_service: Mutex<Option<ServiceSession>>,
    /// Monotonic counter for `TranscriptSegment.sequence`, shared across
    /// the real audio/speech pipeline and `process_test_transcript` so
    /// both paths order consistently within one service.
    pub transcript_sequence: AtomicU64,
    /// The last audio-engine failure, if any, since the last successful
    /// `start_listening`/chunk. `get_live_status` reports `AudioStatusKind::Error`
    /// while this is set, distinct from `Unavailable` (Phase 1.3's audio
    /// failure recovery - see `docs/live-service.md`). Cleared on the next
    /// successful audio operation, never automatically time-limited: an
    /// unresolved failure must stay visible until the operator retries.
    pub audio_error: Mutex<Option<String>>,
    /// Same as `audio_error`, for the speech engine (`SpeechStatusKind::Error`).
    pub speech_error: Mutex<Option<String>>,
    /// Phase 3.8.6: the retained diagnostic detail `speech_error` alone
    /// cannot carry (feature/model-load state, chunk/inference counters).
    /// See `SpeechDiagnostics`'s own docs.
    pub speech_diagnostics: Mutex<SpeechDiagnostics>,
    /// A dedicated `MusicIntelligenceEngine` instance for acoustic
    /// analysis (Phase 2.2), with its own `MusicProvider` connection -
    /// mirroring the existing "every independent read path gets its own
    /// connection" discipline (`music_provider` above, and
    /// `intelligence_registry`'s own separate Music engine registration in
    /// `lib.rs`). Kept separate from `intelligence_registry`'s trait-object
    /// registration because `analyze_acoustic` is a `MusicIntelligenceEngine`
    /// inherent method, not part of the shared `IntelligenceEngine` trait
    /// (see that method's own docs) - a `Box<dyn IntelligenceEngine>` looked
    /// up from the registry cannot be downcast back to call it.
    pub acoustic_music_engine: MusicIntelligenceEngine,
    /// The acoustic recognizer implementation this app is configured with -
    /// `NullAcousticMusicRecognizer` unless `AppConfig.acoustic` names a
    /// real, present model (see `lib.rs::create_acoustic_recognizer`).
    /// Mirrors `speech_engine`'s `Mutex<Box<dyn ...>>` shape exactly; "no
    /// acoustic recognizer configured" is never fatal, the same way "no
    /// speech model" is never fatal.
    pub acoustic_recognizer: Mutex<Box<dyn AcousticMusicRecognizer>>,
    /// The song a human operator has explicitly confirmed as currently
    /// being sung (Phase 2.2) - `None` until an operator accepts a Music
    /// finding, and cleared only by an explicit operator action
    /// (`clear_current_song`). Never set automatically by acoustic/lyric
    /// confidence alone, regardless of how high - see
    /// `cip_core_music::CurrentSong`'s own docs.
    pub current_song: Mutex<Option<CurrentSong>>,
    /// The real, stateful Sermon Intelligence engine (Phase 2.3) - every
    /// `analyze_sermon_transcript`/`get_sermon_state` call goes through
    /// this exact instance, so its accumulated theme/structure/state stays
    /// consistent across calls. A *separate* instance is also registered
    /// into `intelligence_registry` for diagnostics/failure-isolation
    /// symmetry only - see `sermon.rs`'s module docs for why these are
    /// deliberately not the same instance (mirrors `acoustic_music_engine`
    /// above).
    pub sermon_engine: SermonIntelligenceEngine,
    /// In-memory queue of cross-domain correlations awaiting operator
    /// review (Phase 2.4's `CorrelationQueue`) - the correlation
    /// counterpart to `intelligence_findings`, populated only by
    /// `commands::analyze_cross_domain`. Deliberately not persisted, for
    /// the same reason `intelligence_findings` isn't: a correlation is
    /// derived from findings that themselves already carry provenance, so
    /// nothing here needs to survive a restart (see
    /// `docs/cross-domain-intelligence.md`'s persistence-decision section).
    pub correlation_queue: Mutex<CorrelationQueue>,
    /// The real, stateful Service Intelligence engine (Phase 2.4, per the
    /// authoritative Phase 2 roadmap) - every `analyze_service_transcript`/
    /// `get_service_intelligence_state` call goes through this exact
    /// instance, so its accumulated phase/transition history stays
    /// consistent across calls. A *separate* instance is also registered
    /// into `intelligence_registry` for diagnostics/failure-isolation
    /// symmetry only - see `service.rs`'s module docs (mirrors
    /// `sermon_engine` above).
    pub service_engine: ServiceIntelligenceEngine,
    /// Wall-clock time the last **final** transcript segment was received
    /// from the real live audio/speech pipeline (Phase 2.4) - `None` until
    /// the first one arrives this service. Deliberately not touched by the
    /// manual/test-mode transcript commands (Music/Sermon/Bible/Service's
    /// own `analyze_*_transcript` harnesses): this field answers "is real
    /// live transcription actually still happening," which manual test
    /// input would only make misleading. See `service.rs::transcript_freshness`.
    pub last_transcript_at: Mutex<Option<DateTime<Utc>>>,
    /// The active Sermon Foundation entity (Phase 2.5, per the
    /// authoritative Phase 2 roadmap), if any - `None` when no sermon is
    /// currently being delivered. Durably persisted to the `sermons`
    /// table on every mutation (see `persistence.rs`'s sermon functions),
    /// but - exactly like `active_service` above - never automatically
    /// restored into this field on app restart; a restart loses the
    /// *live* session, never the historical record. See
    /// `docs/sermon-foundation.md`'s "Persistence decision" section.
    pub active_sermon: Mutex<Option<Sermon>>,
    /// The section currently open within `active_sermon`, if any - kept
    /// alongside `active_sermon` rather than re-queried from the database
    /// on every command, mirroring every other "current live thing" field
    /// in this struct.
    pub active_sermon_section: Mutex<Option<SermonSection>>,
    /// In-memory queue of content candidates awaiting operator review
    /// (Phase 2.7's `ContentCandidateQueue`, per the authoritative Phase 2
    /// roadmap) - the content-candidate counterpart to `intelligence_findings`/
    /// `correlation_queue`, populated only by
    /// `commands::analyze_content_intelligence`. Deliberately not
    /// persisted, for the same reason `correlation_queue` isn't: a
    /// candidate is derived from a finding that already carries its own
    /// provenance/persistence story, so nothing here needs to survive a
    /// restart (see `docs/content-intelligence.md`'s persistence-decision
    /// section).
    pub content_candidate_queue: Mutex<ContentCandidateQueue>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: AppConfig,
        db: rusqlite::Connection,
        bible_provider: Box<dyn BibleProvider>,
        content_registry: Box<dyn ContentRegistry>,
        intelligence_registry: IntelligenceEngineRegistry,
        music_provider: Box<dyn MusicProvider>,
        audio_engine: Box<dyn AudioEngine>,
        speech_engine: Box<dyn SpeechEngine>,
        speech_diagnostics: SpeechDiagnostics,
        acoustic_music_engine: MusicIntelligenceEngine,
        acoustic_recognizer: Box<dyn AcousticMusicRecognizer>,
    ) -> Self {
        Self {
            config,
            db: Mutex::new(db),
            bible_provider,
            content_registry,
            intelligence_registry,
            music_provider,
            intelligence_findings: Mutex::new(FindingQueue::new()),
            context_manager: Mutex::new(DefaultScriptureContextManager::new(
                DEFAULT_TRANSLATION_ID,
            )),
            audio_engine: Mutex::new(audio_engine),
            speech_engine: Mutex::new(speech_engine),
            active_service: Mutex::new(None),
            transcript_sequence: AtomicU64::new(0),
            audio_error: Mutex::new(None),
            speech_error: Mutex::new(None),
            speech_diagnostics: Mutex::new(speech_diagnostics),
            acoustic_music_engine,
            acoustic_recognizer: Mutex::new(acoustic_recognizer),
            current_song: Mutex::new(None),
            sermon_engine: SermonIntelligenceEngine::new(),
            correlation_queue: Mutex::new(CorrelationQueue::new()),
            service_engine: ServiceIntelligenceEngine::new(),
            last_transcript_at: Mutex::new(None),
            active_sermon: Mutex::new(None),
            active_sermon_section: Mutex::new(None),
            content_candidate_queue: Mutex::new(ContentCandidateQueue::new()),
        }
    }
}
