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
use cip_core_ai::SpeechEngine;
use cip_core_bible::{BibleProvider, DefaultScriptureContextManager};
use cip_core_content::ContentRegistry;
use cip_core_intelligence::{
    CorrelationQueue, FindingQueue, IntelligenceEngineRegistry, MusicIntelligenceEngine,
    SermonIntelligenceEngine,
};
use cip_core_music::{AcousticMusicRecognizer, CurrentSong, MusicProvider};
use cip_core_service::{AudioEngine, ServiceSession};
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

/// The one Bible translation Phase 1.2 operates against. Not user-facing
/// configuration yet - `core/bible::BibleProvider::list_translations` is
/// already multi-translation-capable; wiring a picker through is future
/// UI work, not a Bible Intelligence Core limitation.
pub const DEFAULT_TRANSLATION_ID: &str = "KJV";

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
            acoustic_music_engine,
            acoustic_recognizer: Mutex::new(acoustic_recognizer),
            current_song: Mutex::new(None),
            sermon_engine: SermonIntelligenceEngine::new(),
            correlation_queue: Mutex::new(CorrelationQueue::new()),
        }
    }
}
