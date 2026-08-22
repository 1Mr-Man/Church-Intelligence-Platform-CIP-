//! Song continuity (Phase 2.1 spec section 25): distinguishing a brand
//! new recognition from "the worship leader is still on the same song."
//!
//! Deliberately does not introduce a second context/history mechanism -
//! the caller (`core/intelligence`'s `MusicIntelligenceEngine`) is
//! expected to pass in only the *single most recent* music finding from
//! `IntelligenceContext.recent_findings`, which is itself already bounded
//! (capped at 20 entries - see `docs/intelligence-architecture.md`'s
//! context-bounds section) - so recency is naturally bounded by
//! infrastructure this crate does not need to duplicate. A song is never
//! "active" forever: once it scrolls out of that bounded recent-findings
//! window, continuity classification simply has no `previous` to compare
//! against and reports `Unknown`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SongContinuity {
    /// No prior recognized song to compare against.
    Unknown,
    /// The current recognition names the same song (same dataset, same
    /// id) as the most recent prior one.
    ContinuingSameSong,
    /// A confident recognition of a *different* song than the prior one.
    NewSong,
    /// The current recognition differs from the prior one, but isn't
    /// confident enough to firmly call it a new song - worth surfacing to
    /// the operator as uncertain rather than asserted either way.
    PossibleSongChange,
}

/// `current_confidence` and `confident_threshold` are both raw
/// `0.0..=1.0` scores - `confident_threshold` should generally be the
/// same value the caller uses to decide `Suggested` vs `Inferred`
/// assertion level, so continuity and assertion level never disagree
/// about what counts as "confident."
pub fn classify_continuity(
    previous: Option<(&str, &str)>, // (content_id, song_id) of the most recent prior music finding
    current_content_id: &str,
    current_song_id: &str,
    current_confidence: f32,
    confident_threshold: f32,
) -> SongContinuity {
    match previous {
        None => SongContinuity::Unknown,
        Some((prev_content_id, prev_song_id)) => {
            if prev_content_id == current_content_id && prev_song_id == current_song_id {
                SongContinuity::ContinuingSameSong
            } else if current_confidence >= confident_threshold {
                SongContinuity::NewSong
            } else {
                SongContinuity::PossibleSongChange
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_prior_song_is_unknown_continuity() {
        assert_eq!(
            classify_continuity(None, "music:dev", "s1", 0.9, 0.7),
            SongContinuity::Unknown
        );
    }

    #[test]
    fn same_song_same_dataset_is_continuing() {
        assert_eq!(
            classify_continuity(Some(("music:dev", "s1")), "music:dev", "s1", 0.9, 0.7),
            SongContinuity::ContinuingSameSong
        );
    }

    #[test]
    fn same_song_id_in_a_different_dataset_is_not_continuing() {
        assert_ne!(
            classify_continuity(Some(("music:dev", "s1")), "music:other", "s1", 0.9, 0.7),
            SongContinuity::ContinuingSameSong
        );
    }

    #[test]
    fn a_confident_different_song_is_a_new_song() {
        assert_eq!(
            classify_continuity(Some(("music:dev", "s1")), "music:dev", "s2", 0.9, 0.7),
            SongContinuity::NewSong
        );
    }

    #[test]
    fn a_weak_different_song_is_only_a_possible_change() {
        assert_eq!(
            classify_continuity(Some(("music:dev", "s1")), "music:dev", "s2", 0.4, 0.7),
            SongContinuity::PossibleSongChange
        );
    }
}
