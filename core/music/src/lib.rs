//! Music domain (Phase 2.1, extended in Phase 2.2): the `Song`/lyric
//! content model, `MusicProvider` contract, deterministic offline title/
//! alias/number/lyric recognition, and the acoustic recognition boundary
//! and evidence fusion. The music-domain counterpart to `core/bible`,
//! using the same provider/adaptor split and the same "never guess,
//! always return evidence" discipline.
//!
//! Phase 2.1 implemented **deterministic lyric/title/number matching**
//! only. Phase 2.2 adds the `AcousticMusicRecognizer` *contract* (not an
//! implementation - see `integrations/music-acoustic`) and a deterministic
//! fusion policy for combining acoustic evidence with recent
//! lyric-derived evidence. See `docs/music-intelligence.md` and
//! `docs/acoustic-music.md` for what's implemented vs. deferred, and
//! `matcher.rs`/`fusion.rs` for the documented, explainable formulas.
//!
//! Deliberately dependency-light: `serde`/`thiserror`/`cip-core-confidence`/
//! `uuid` only. No Tauri, no SQLite, no audio capture library (`cpal`), no
//! network client, no specific acoustic model/vendor - see
//! `integrations/music` for the SQLite-backed `MusicProvider`
//! implementation and `integrations/music-acoustic` for concrete
//! `AcousticMusicRecognizer` implementations.

mod acoustic;
mod candidate;
mod continuity;
#[cfg(test)]
mod fixtures;
mod fusion;
pub mod matcher;
#[cfg(test)]
mod matcher_tests;
pub mod normalize;
mod provider;
mod song;

pub use acoustic::{
    assess_signal_quality, AcousticAnalysisConfig, AcousticMusicRecognizer,
    AcousticRecognitionCandidate, AcousticRecognitionError, AcousticRecognitionMethod,
    AcousticRecognitionStatus, AudioSegment, AudioSegmenter, SignalQuality,
};
pub use candidate::{MatchType, SongRecognitionCandidate};
pub use continuity::{classify_continuity, SongContinuity};
pub use fusion::{fuse_acoustic_with_context, CurrentSong, CurrentSongState};
pub use matcher::{distinctiveness, is_ambiguous, search_songs, MatchThresholds, MusicQuery};
pub use provider::{MusicProvider, MusicProviderError};
pub use song::{LyricLine, SectionKind, Song, SongSection, SongStatus, SongType};
