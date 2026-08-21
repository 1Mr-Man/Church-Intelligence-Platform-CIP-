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
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: AppConfig,
        db: rusqlite::Connection,
        bible_provider: Box<dyn BibleProvider>,
        audio_engine: Box<dyn AudioEngine>,
        speech_engine: Box<dyn SpeechEngine>,
    ) -> Self {
        Self {
            config,
            db: Mutex::new(db),
            bible_provider,
            context_manager: Mutex::new(DefaultScriptureContextManager::new(
                DEFAULT_TRANSLATION_ID,
            )),
            audio_engine: Mutex::new(audio_engine),
            speech_engine: Mutex::new(speech_engine),
            active_service: Mutex::new(None),
            transcript_sequence: AtomicU64::new(0),
            audio_error: Mutex::new(None),
            speech_error: Mutex::new(None),
        }
    }
}
