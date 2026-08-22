//! Deterministic evidence fusion (Phase 2.2): combining acoustic
//! recognition with recent lyric/title-derived evidence into one ranked
//! candidate list, without creating a second finding/confidence system.
//! No averaging - see [`fuse_acoustic_with_context`]'s docs for the
//! documented, explainable combination formula.
//!
//! This module also defines [`CurrentSong`]/[`CurrentSongState`] - the
//! "what is authoritative right now" concept a live service needs, kept
//! deliberately small: a plain data type, not a second state machine.
//! Deriving/mutating it (from the operator's accept/reject/clear actions)
//! is the caller's job - see `apps/desktop/src-tauri/src/music.rs`.

use std::collections::HashMap;

use cip_core_confidence::{ConfidenceResult, ConfidenceSource};

use crate::acoustic::{AcousticRecognitionCandidate, AcousticRecognitionMethod};
use crate::candidate::{MatchType, SongRecognitionCandidate};

/// Combine two independent confidence estimates for the *same* claim into
/// one, using the "noisy-or" combination: the probability that at least
/// one of two independent pieces of evidence is correct is higher than
/// either alone, but never simply their sum or average (which can exceed
/// 1.0 or under-reward agreement). Clamped to `0.99`, never `1.0` - fused
/// heuristic evidence is never asserted as absolute certainty, matching
/// this codebase's "never claim more than the evidence supports"
/// discipline.
fn noisy_or(a: f32, b: f32) -> f32 {
    (1.0 - (1.0 - a) * (1.0 - b)).min(0.99)
}

/// `(content_id, song_id)` - the key every fusion/dedup step groups
/// candidates by, matching `SongRecognitionCandidate::source`/`song_id`.
type SongKey = (String, String);

fn key_of(candidate: &SongRecognitionCandidate) -> SongKey {
    (candidate.source.clone(), candidate.song_id.clone())
}

/// Collapse duplicate candidates for the same song (e.g. two overlapping
/// acoustic windows both recognizing the same song) to the single
/// highest-confidence one - evidence must never be inflated just because
/// the same underlying signal was observed more than once.
fn dedup_keep_strongest(
    candidates: Vec<SongRecognitionCandidate>,
) -> Vec<SongRecognitionCandidate> {
    let mut by_key: HashMap<SongKey, SongRecognitionCandidate> = HashMap::new();
    for candidate in candidates {
        let key = key_of(&candidate);
        match by_key.get(&key) {
            Some(existing) if existing.confidence.score >= candidate.confidence.score => {}
            _ => {
                by_key.insert(key, candidate);
            }
        }
    }
    by_key.into_values().collect()
}

fn acoustic_to_song_candidate(
    candidate: &AcousticRecognitionCandidate,
) -> SongRecognitionCandidate {
    let method_label = match candidate.method {
        AcousticRecognitionMethod::LocalModel => "local model",
        AcousticRecognitionMethod::ExternalProvider => "external provider",
        AcousticRecognitionMethod::Test => "test adapter",
        AcousticRecognitionMethod::None => "none",
    };
    let explanation = format!("Acoustic match ({method_label})");
    SongRecognitionCandidate {
        song_id: candidate.song_id.clone(),
        match_type: MatchType::Acoustic,
        matched_text: explanation.clone(),
        confidence: candidate.confidence.clone(),
        evidence: candidate.evidence.clone(),
        source: candidate.content_id.clone(),
        ranking: 0,
        explanation,
    }
}

/// Fuse fresh acoustic candidates with `context_evidence` (typically a
/// small number of `MatchType::Contextual` candidates the caller built
/// from recent Music findings already in `IntelligenceContext.recent_findings`
/// - see `core/intelligence::music_adapter`).
///
/// The result **never contains a song that was not already present in
/// `acoustic`** - context evidence may only strengthen an existing
/// acoustic candidate, never introduce a new one on its own (Phase 2.2
/// spec section 15: "continuity must never create a song from nothing").
/// A context entry with no matching acoustic candidate is simply dropped.
///
/// When a song has evidence in both lists, the two confidences are
/// combined via [`noisy_or`] (never averaged) and every evidence string
/// from both sides is preserved (deduplicated), so the fused candidate
/// stays fully auditable. Acoustic-only candidates pass through
/// unchanged. Duplicate acoustic candidates for the same song (e.g.
/// overlapping analysis windows) are collapsed to the strongest one
/// *before* fusion, so repeated observation of the same evidence can
/// never inflate confidence on its own.
///
/// Output is sorted deterministically `(confidence desc, song_id asc)`
/// with `ranking` reassigned, exactly like `matcher::search_songs` - the
/// same list can be fed straight into `matcher::is_ambiguous`.
pub fn fuse_acoustic_with_context(
    acoustic: &[AcousticRecognitionCandidate],
    context_evidence: &[SongRecognitionCandidate],
) -> Vec<SongRecognitionCandidate> {
    let acoustic_candidates: Vec<SongRecognitionCandidate> =
        acoustic.iter().map(acoustic_to_song_candidate).collect();
    let deduped = dedup_keep_strongest(acoustic_candidates);

    let context_by_key: HashMap<SongKey, &SongRecognitionCandidate> =
        context_evidence.iter().map(|c| (key_of(c), c)).collect();

    let mut fused: Vec<SongRecognitionCandidate> = deduped
        .into_iter()
        .map(|primary| match context_by_key.get(&key_of(&primary)) {
            Some(secondary) => fuse_pair(primary, secondary),
            None => primary,
        })
        .collect();

    fused.sort_by(|a, b| {
        b.confidence
            .score
            .total_cmp(&a.confidence.score)
            .then_with(|| a.song_id.cmp(&b.song_id))
    });
    for (i, c) in fused.iter_mut().enumerate() {
        c.ranking = i as u32;
    }
    fused
}

fn fuse_pair(
    primary: SongRecognitionCandidate,
    secondary: &SongRecognitionCandidate,
) -> SongRecognitionCandidate {
    let fused_score = noisy_or(primary.confidence.score, secondary.confidence.score);
    let confidence = ConfidenceResult::new(
        fused_score,
        ConfidenceSource::Model,
        Some(format!(
            "acoustic + {} corroborate the same song",
            match secondary.match_type {
                MatchType::Contextual => "recent lyric recognition",
                _ => "independent evidence",
            }
        )),
    );

    let mut evidence = primary.evidence.clone();
    for item in &secondary.evidence {
        if !evidence.contains(item) {
            evidence.push(item.clone());
        }
    }

    let explanation = format!(
        "{} - corroborated ({})",
        primary.explanation, secondary.explanation
    );

    SongRecognitionCandidate {
        confidence,
        evidence,
        explanation,
        ..primary
    }
}

/// The three-way distinction a live service's operator UI needs beyond
/// "a finding exists": what's currently authoritative
/// (`ConfirmedSong`, set only by an explicit operator accept), what's
/// merely pending (`CandidateSong`), and whether a pending candidate
/// disagrees with what's confirmed (`PossibleTransition`). Deliberately
/// small - no sixth/seventh state; see `docs/acoustic-music.md`'s
/// "avoiding state explosion" note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurrentSongState {
    NoSong,
    CandidateSong,
    ConfirmedSong,
    PossibleTransition,
}

/// The song a human operator has explicitly confirmed as currently being
/// sung - never set automatically, regardless of acoustic/lyric
/// confidence (Phase 2.2 rule 16/section 14). Derived from an accepted
/// Music finding's evidence, or explicitly cleared; see
/// `apps/desktop/src-tauri/src/music.rs::compute_current_song_after_accept`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSong {
    pub content_id: String,
    pub song_id: String,
    pub confidence: ConfidenceResult,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_confidence::ConfidenceSource;
    use uuid::Uuid;

    fn acoustic_candidate(
        song_id: &str,
        content_id: &str,
        score: f32,
    ) -> AcousticRecognitionCandidate {
        AcousticRecognitionCandidate {
            song_id: song_id.to_string(),
            content_id: content_id.to_string(),
            confidence: ConfidenceResult::new(score, ConfidenceSource::Model, None),
            method: AcousticRecognitionMethod::Test,
            segment_id: Uuid::nil(),
            duration_ms: 8_000,
            evidence: vec!["acoustic match".to_string()],
        }
    }

    fn contextual_candidate(
        song_id: &str,
        content_id: &str,
        score: f32,
    ) -> SongRecognitionCandidate {
        SongRecognitionCandidate {
            song_id: song_id.to_string(),
            match_type: MatchType::Contextual,
            matched_text: "recent finding".to_string(),
            confidence: ConfidenceResult::new(score, ConfidenceSource::Model, None),
            evidence: vec!["recently recognized from lyrics".to_string()],
            source: content_id.to_string(),
            ranking: 0,
            explanation: "Recently recognized from lyrics".to_string(),
        }
    }

    #[test]
    fn acoustic_only_candidate_passes_through_with_acoustic_match_type() {
        let fused = fuse_acoustic_with_context(&[acoustic_candidate("s1", "music:dev", 0.7)], &[]);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].match_type, MatchType::Acoustic);
        assert_eq!(fused[0].confidence.score, 0.7);
    }

    #[test]
    fn agreeing_context_evidence_strengthens_confidence_beyond_either_alone() {
        let acoustic = acoustic_candidate("s1", "music:dev", 0.6);
        let context = contextual_candidate("s1", "music:dev", 0.6);
        let fused = fuse_acoustic_with_context(&[acoustic], &[context]);
        assert_eq!(fused.len(), 1);
        assert!(
            fused[0].confidence.score > 0.6,
            "corroborating evidence must strengthen confidence beyond either source alone"
        );
        assert!(
            fused[0].confidence.score < 1.0,
            "fused heuristic evidence must never claim absolute certainty"
        );
    }

    #[test]
    fn fusion_is_not_a_simple_average() {
        // noisy_or(0.9, 0.9) = 1 - 0.1*0.1 = 0.99, far above the average (0.9).
        let acoustic = acoustic_candidate("s1", "music:dev", 0.9);
        let context = contextual_candidate("s1", "music:dev", 0.9);
        let fused = fuse_acoustic_with_context(&[acoustic], &[context]);
        assert!((fused[0].confidence.score - 0.99).abs() < 0.001);
    }

    #[test]
    fn context_evidence_never_invents_a_candidate_not_already_acoustic() {
        let context_only = contextual_candidate("s1", "music:dev", 0.95);
        let fused = fuse_acoustic_with_context(&[], &[context_only]);
        assert!(
            fused.is_empty(),
            "context/continuity evidence alone must never create a song finding"
        );
    }

    #[test]
    fn conflicting_songs_remain_distinguishable_not_merged() {
        let fused = fuse_acoustic_with_context(
            &[
                acoustic_candidate("s1", "music:dev", 0.7),
                acoustic_candidate("s2", "music:dev", 0.65),
            ],
            &[],
        );
        assert_eq!(fused.len(), 2);
        let ids: Vec<&str> = fused.iter().map(|c| c.song_id.as_str()).collect();
        assert!(ids.contains(&"s1"));
        assert!(ids.contains(&"s2"));
    }

    #[test]
    fn duplicate_acoustic_evidence_for_the_same_song_does_not_inflate_confidence() {
        let fused = fuse_acoustic_with_context(
            &[
                acoustic_candidate("s1", "music:dev", 0.6),
                acoustic_candidate("s1", "music:dev", 0.6),
            ],
            &[],
        );
        assert_eq!(
            fused.len(),
            1,
            "duplicate observations of the same song must collapse, not stack"
        );
        assert_eq!(fused[0].confidence.score, 0.6);
    }

    #[test]
    fn duplicate_dedup_keeps_the_strongest_observation() {
        let fused = fuse_acoustic_with_context(
            &[
                acoustic_candidate("s1", "music:dev", 0.4),
                acoustic_candidate("s1", "music:dev", 0.8),
            ],
            &[],
        );
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].confidence.score, 0.8);
    }

    #[test]
    fn dataset_isolation_is_preserved_through_fusion() {
        let fused = fuse_acoustic_with_context(
            &[
                acoustic_candidate("120", "music:dataset-a", 0.7),
                acoustic_candidate("120", "music:dataset-b", 0.7),
            ],
            &[contextual_candidate("120", "music:dataset-a", 0.7)],
        );
        assert_eq!(fused.len(), 2);
        let a = fused
            .iter()
            .find(|c| c.source == "music:dataset-a")
            .unwrap();
        let b = fused
            .iter()
            .find(|c| c.source == "music:dataset-b")
            .unwrap();
        assert!(
            a.confidence.score > b.confidence.score,
            "only the matching dataset's candidate may be strengthened by context evidence"
        );
    }

    #[test]
    fn output_is_sorted_deterministically_with_ranking_assigned() {
        let fused = fuse_acoustic_with_context(
            &[
                acoustic_candidate("s2", "music:dev", 0.5),
                acoustic_candidate("s1", "music:dev", 0.9),
            ],
            &[],
        );
        assert_eq!(fused[0].song_id, "s1");
        assert_eq!(fused[0].ranking, 0);
        assert_eq!(fused[1].song_id, "s2");
        assert_eq!(fused[1].ranking, 1);
    }

    #[test]
    fn fusion_is_deterministic_for_identical_input() {
        let run = || {
            fuse_acoustic_with_context(
                &[
                    acoustic_candidate("s1", "music:dev", 0.7),
                    acoustic_candidate("s2", "music:dev", 0.65),
                ],
                &[contextual_candidate("s1", "music:dev", 0.5)],
            )
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn evidence_from_both_sources_is_merged_and_deduplicated() {
        let acoustic = acoustic_candidate("s1", "music:dev", 0.6);
        let context = contextual_candidate("s1", "music:dev", 0.6);
        let fused = fuse_acoustic_with_context(&[acoustic], &[context]);
        assert!(fused[0].evidence.contains(&"acoustic match".to_string()));
        assert!(fused[0]
            .evidence
            .contains(&"recently recognized from lyrics".to_string()));
    }
}
