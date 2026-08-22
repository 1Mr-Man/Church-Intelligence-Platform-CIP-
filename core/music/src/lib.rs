//! Music domain (Phase 2.1): the `Song`/lyric content model,
//! `MusicProvider` contract, and deterministic offline title/alias/
//! number/lyric recognition. The music-domain counterpart to
//! `core/bible` - same provider/adaptor split, same "never guess, always
//! return evidence" discipline.
//!
//! This crate implements **deterministic lyric/title/number matching**
//! only - not machine learning, not acoustic fingerprinting. See
//! `docs/music-intelligence.md` for what's implemented vs. deferred, and
//! `matcher.rs` for the documented, explainable ranking formula.
//!
//! Deliberately dependency-light: `serde`/`thiserror`/`cip-core-confidence`
//! only. No Tauri, no SQLite, no audio library, no network client - see
//! `integrations/music` for the SQLite-backed `MusicProvider`
//! implementation, and `ai/speech`-analogous future crates for any real
//! acoustic recognizer.

mod candidate;
mod continuity;
#[cfg(test)]
mod fixtures;
pub mod matcher;
#[cfg(test)]
mod matcher_tests;
pub mod normalize;
mod provider;
mod song;

pub use candidate::{MatchType, SongRecognitionCandidate};
pub use continuity::{classify_continuity, SongContinuity};
pub use matcher::{distinctiveness, is_ambiguous, search_songs, MatchThresholds, MusicQuery};
pub use provider::{MusicProvider, MusicProviderError};
pub use song::{LyricLine, SectionKind, Song, SongSection, SongStatus, SongType};
