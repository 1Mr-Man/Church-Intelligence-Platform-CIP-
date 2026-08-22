//! Deterministic, offline song recognition (Phase 2.1 spec sections
//! 9-10, 56). No network, no machine learning, no randomness - every
//! confidence value here is a documented formula over the query and what
//! [`MusicProvider`] returned.
//!
//! ## Ranking hierarchy
//!
//! Base confidence by [`MatchType`], before any distinctiveness
//! modulation (spec section 56's literal ordering):
//!
//! | Match type | Base confidence |
//! | --- | --- |
//! | `ExplicitTitle` | 0.97 |
//! | `Alias` | 0.93 |
//! | `SongNumber` | 0.90 |
//! | `MultipleLyricLines` | 0.85 |
//! | `ExactLyric` (single line) | up to 0.80, modulated by distinctiveness |
//! | `PartialLyric` | up to 0.55, modulated by distinctiveness |
//! | `Contextual` | 0.35 (continuity only - see `continuity.rs`) |
//! | `Acoustic` | not from this table - the recognizer's own reported score, fused per `crate::fusion` (Phase 2.2) |
//!
//! Title/alias/number/multi-line matches are already structural
//! (exact-equality or exact-adjacency), so they are not modulated by
//! distinctiveness. A single-line lyric match is modulated because a
//! generic phrase ("Lord we praise you") is much weaker evidence than a
//! distinctive one ("Great is Thy faithfulness, morning by morning new
//! mercies I see") - see [`distinctiveness`].
//!
//! Candidates are always sorted `(confidence desc, song_id asc)` -
//! ranking never depends on `HashMap`/`HashSet` iteration order.

use std::collections::HashSet;

use cip_core_confidence::{ConfidenceResult, ConfidenceSource};

use crate::candidate::{MatchType, SongRecognitionCandidate};
use crate::normalize::{normalize_for_matching, word_count};
use crate::provider::{MusicProvider, MusicProviderError};
use crate::song::LyricLine;

/// A recognition query, dispatched by shape - mirrors
/// `core/bible::search::search_bible`'s reference/chapter/range/free-text
/// dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MusicQuery {
    Title(String),
    Number(String),
    Lyric(String),
    /// Multiple lines, oldest first, most recently spoken/sung last -
    /// checked for exact consecutive adjacency within one song (spec
    /// section 9E).
    LyricSequence(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchThresholds {
    /// Candidates scoring below this are discarded entirely - never
    /// returned, never counted toward ambiguity.
    pub minimum_confidence: f32,
    /// If the top two candidates' confidence differ by less than this,
    /// [`is_ambiguous`] reports `true` - "highest score wins" is never
    /// used when candidates are this close (spec section 26).
    pub ambiguity_margin: f32,
}

impl Default for MatchThresholds {
    fn default() -> Self {
        Self {
            minimum_confidence: 0.25,
            ambiguity_margin: 0.08,
        }
    }
}

/// A generic phrase shared across many songs is weaker evidence than a
/// long, distinctive one found in exactly one song - deterministic, no
/// machine learning. `word_count` rewards longer phrases (a phrase is
/// "fully distinctive" at 8+ words); `songs_matched` penalizes a phrase
/// shared across many songs. Bounded to `0.05..=1.0` so a match is never
/// worth literally nothing (the fact that it matched at all is still
/// evidence) nor treated as more certain than a structural match.
pub fn distinctiveness(word_count: usize, songs_matched: usize) -> f32 {
    let length_factor = (word_count as f32 / 8.0).min(1.0);
    let uniqueness_factor = 1.0 / (songs_matched.max(1) as f32);
    (0.4 * length_factor + 0.6 * uniqueness_factor).clamp(0.05, 1.0)
}

fn base_confidence(match_type: MatchType) -> f32 {
    match match_type {
        MatchType::ExplicitTitle => 0.97,
        MatchType::Alias => 0.93,
        MatchType::SongNumber => 0.90,
        MatchType::MultipleLyricLines => 0.85,
        MatchType::ExactLyric => 0.80,
        MatchType::PartialLyric => 0.55,
        MatchType::Contextual => 0.35,
        // Acoustic candidates never go through this fixed table - their
        // confidence comes directly from the recognizer's own reported
        // score (see `crate::fusion`), since "how sure the recognizer is"
        // is a continuous signal a fixed base constant would flatten.
        // This arm exists only so the match stays exhaustive.
        MatchType::Acoustic => 0.0,
    }
}

fn structural_candidate(
    song_id: &str,
    content_id: &str,
    match_type: MatchType,
    matched_text: &str,
    evidence: String,
) -> SongRecognitionCandidate {
    let score = base_confidence(match_type);
    SongRecognitionCandidate {
        song_id: song_id.to_string(),
        match_type,
        matched_text: matched_text.to_string(),
        confidence: ConfidenceResult::new(
            score,
            ConfidenceSource::Heuristic,
            Some(evidence.clone()),
        ),
        evidence: vec![evidence.clone()],
        source: content_id.to_string(),
        ranking: 0, // assigned after sorting
        explanation: evidence,
    }
}

fn lyric_candidate(
    song_id: &str,
    content_id: &str,
    match_type: MatchType,
    matched_text: &str,
    distinctiveness_score: f32,
    songs_matched: usize,
) -> SongRecognitionCandidate {
    let score =
        (base_confidence(match_type) * (0.5 + 0.5 * distinctiveness_score)).clamp(0.05, 0.97);
    let mut evidence = vec![format!("Matched lyric phrase \"{matched_text}\"")];
    if songs_matched > 1 {
        evidence.push(format!(
            "Phrase also appears in {} other song(s) - weaker evidence",
            songs_matched - 1
        ));
    }
    let explanation = if songs_matched > 1 {
        format!(
            "{} song(s) match the phrase \"{matched_text}\"",
            songs_matched
        )
    } else {
        format!("Matched distinctive lyric phrase \"{matched_text}\"")
    };
    SongRecognitionCandidate {
        song_id: song_id.to_string(),
        match_type,
        matched_text: matched_text.to_string(),
        confidence: ConfidenceResult::new(
            score,
            ConfidenceSource::Heuristic,
            Some(explanation.clone()),
        ),
        evidence,
        source: content_id.to_string(),
        ranking: 0,
        explanation,
    }
}

fn search_title_or_alias(
    provider: &dyn MusicProvider,
    content_ids: &[String],
    title: &str,
) -> Result<Vec<SongRecognitionCandidate>, MusicProviderError> {
    let normalized = normalize_for_matching(title);
    let mut out = Vec::new();
    for content_id in content_ids {
        for song in provider.search_title(content_id, &normalized)? {
            out.push(structural_candidate(
                &song.id,
                content_id,
                MatchType::ExplicitTitle,
                title,
                "Exact title match".to_string(),
            ));
        }
        for song in provider.search_alias(content_id, &normalized)? {
            out.push(structural_candidate(
                &song.id,
                content_id,
                MatchType::Alias,
                title,
                "Matched alias".to_string(),
            ));
        }
    }
    Ok(out)
}

fn search_number(
    provider: &dyn MusicProvider,
    content_ids: &[String],
    number: &str,
) -> Result<Vec<SongRecognitionCandidate>, MusicProviderError> {
    let mut out = Vec::new();
    for content_id in content_ids {
        if let Some(song) = provider.search_number(content_id, number)? {
            out.push(structural_candidate(
                &song.id,
                content_id,
                MatchType::SongNumber,
                number,
                format!("Matched song number {number} in {content_id}"),
            ));
        }
    }
    Ok(out)
}

fn search_single_lyric(
    provider: &dyn MusicProvider,
    content_ids: &[String],
    phrase: &str,
) -> Result<Vec<SongRecognitionCandidate>, MusicProviderError> {
    let normalized_phrase = normalize_for_matching(phrase);
    let words = word_count(&normalized_phrase);
    if words == 0 {
        return Ok(Vec::new());
    }

    let mut raw: Vec<(String, LyricLine)> = Vec::new();
    for content_id in content_ids {
        for line in provider.search_lyrics(content_id, &normalized_phrase)? {
            raw.push((content_id.clone(), line));
        }
    }

    let distinct_songs: HashSet<(String, String)> = raw
        .iter()
        .map(|(cid, line)| (cid.clone(), line.song_id.clone()))
        .collect();
    let songs_matched = distinct_songs.len();
    let d = distinctiveness(words, songs_matched);

    let mut per_song_best: Vec<(String, String, MatchType)> = Vec::new();
    for (content_id, song_id) in &distinct_songs {
        let exact = raw.iter().any(|(cid, l)| {
            cid == content_id && &l.song_id == song_id && l.normalized_text == normalized_phrase
        });
        let match_type = if exact {
            MatchType::ExactLyric
        } else {
            MatchType::PartialLyric
        };
        per_song_best.push((content_id.clone(), song_id.clone(), match_type));
    }

    Ok(per_song_best
        .into_iter()
        .map(|(content_id, song_id, match_type)| {
            lyric_candidate(&song_id, &content_id, match_type, phrase, d, songs_matched)
        })
        .collect())
}

/// Multi-line matching (spec section 9E): all of `lines`, in order, must
/// map to strictly consecutive `sequence` values within the same song
/// (and, when present, the same section) for a `MultipleLyricLines`
/// candidate. If the lines don't line up consecutively in any song, this
/// falls back to treating the *last* (most recent) line as an ordinary
/// single-phrase lyric query - a partial match is still useful evidence
/// even when full-sequence adjacency isn't found.
fn search_lyric_sequence(
    provider: &dyn MusicProvider,
    content_ids: &[String],
    lines: &[String],
) -> Result<Vec<SongRecognitionCandidate>, MusicProviderError> {
    if lines.len() < 2 {
        return match lines.first() {
            Some(only) => search_single_lyric(provider, content_ids, only),
            None => Ok(Vec::new()),
        };
    }

    let normalized_lines: Vec<String> = lines.iter().map(|l| normalize_for_matching(l)).collect();
    let mut matches_per_line: Vec<Vec<(String, LyricLine)>> = Vec::new();
    for normalized in &normalized_lines {
        let mut found = Vec::new();
        for content_id in content_ids {
            for line in provider.search_lyrics(content_id, normalized)? {
                if line.normalized_text == *normalized {
                    found.push((content_id.clone(), line));
                }
            }
        }
        matches_per_line.push(found);
    }

    // A song/section is a consecutive-sequence candidate if the first
    // line's match has a same-song/section successor for every
    // subsequent queried line, at increasing sequence.
    let mut consecutive_songs: HashSet<(String, String)> = HashSet::new();
    if let Some(first_matches) = matches_per_line.first() {
        for (content_id, first_line) in first_matches {
            let mut expected_sequence = first_line.sequence;
            let mut all_found = true;
            for later_matches in &matches_per_line[1..] {
                expected_sequence += 1;
                let found_next = later_matches.iter().any(|(cid, l)| {
                    cid == content_id
                        && l.song_id == first_line.song_id
                        && l.section_id == first_line.section_id
                        && l.sequence == expected_sequence
                });
                if !found_next {
                    all_found = false;
                    break;
                }
            }
            if all_found {
                consecutive_songs.insert((content_id.clone(), first_line.song_id.clone()));
            }
        }
    }

    if consecutive_songs.is_empty() {
        return search_single_lyric(provider, content_ids, lines.last().unwrap());
    }

    let combined_text = lines.join(" / ");
    let mut songs: Vec<(String, String)> = consecutive_songs.into_iter().collect();
    songs.sort();
    Ok(songs
        .into_iter()
        .map(|(content_id, song_id)| {
            let evidence = format!("Matched {} consecutive lyric lines", lines.len());
            SongRecognitionCandidate {
                song_id,
                match_type: MatchType::MultipleLyricLines,
                matched_text: combined_text.clone(),
                confidence: ConfidenceResult::new(
                    base_confidence(MatchType::MultipleLyricLines),
                    ConfidenceSource::Heuristic,
                    Some(evidence.clone()),
                ),
                evidence: vec![evidence.clone()],
                source: content_id,
                ranking: 0,
                explanation: evidence,
            }
        })
        .collect())
}

/// Search across `content_ids` (already filtered to enabled datasets by
/// the caller - see Phase 2.1 spec section 22) for `query`, returning
/// every candidate at or above `thresholds.minimum_confidence`, ranked
/// deterministically (confidence descending, `song_id` ascending as a
/// tiebreak - never hash-map order). Never auto-selects a single winner;
/// see [`is_ambiguous`] for the caller-facing ambiguity classification.
pub fn search_songs(
    provider: &dyn MusicProvider,
    content_ids: &[String],
    query: &MusicQuery,
    thresholds: &MatchThresholds,
) -> Result<Vec<SongRecognitionCandidate>, MusicProviderError> {
    let mut candidates = match query {
        MusicQuery::Title(title) => search_title_or_alias(provider, content_ids, title)?,
        MusicQuery::Number(number) => search_number(provider, content_ids, number)?,
        MusicQuery::Lyric(phrase) => search_single_lyric(provider, content_ids, phrase)?,
        MusicQuery::LyricSequence(lines) => search_lyric_sequence(provider, content_ids, lines)?,
    };

    candidates.retain(|c| c.confidence.score >= thresholds.minimum_confidence);
    candidates.sort_by(|a, b| {
        b.confidence
            .score
            .total_cmp(&a.confidence.score)
            .then_with(|| a.song_id.cmp(&b.song_id))
    });
    for (i, c) in candidates.iter_mut().enumerate() {
        c.ranking = i as u32;
    }
    Ok(candidates)
}

/// Whether the top two ranked `candidates` are too close to call - the
/// caller must not auto-select in that case (spec section 26).
pub fn is_ambiguous(candidates: &[SongRecognitionCandidate], thresholds: &MatchThresholds) -> bool {
    match candidates {
        [first, second, ..] => {
            (first.confidence.score - second.confidence.score) < thresholds.ambiguity_margin
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinctiveness_rewards_long_phrases_in_one_song() {
        let long_unique = distinctiveness(8, 1);
        let short_common = distinctiveness(2, 10);
        assert!(long_unique > short_common);
    }

    #[test]
    fn distinctiveness_stays_within_bounds() {
        assert!(distinctiveness(0, 1) >= 0.05);
        assert!(distinctiveness(100, 1) <= 1.0);
    }

    #[test]
    fn is_ambiguous_true_when_top_two_are_close() {
        let thresholds = MatchThresholds::default();
        let candidates = vec![
            SongRecognitionCandidate {
                song_id: "a".to_string(),
                match_type: MatchType::ExactLyric,
                matched_text: "x".to_string(),
                confidence: ConfidenceResult::new(0.76, ConfidenceSource::Heuristic, None),
                evidence: vec![],
                source: "music:dev".to_string(),
                ranking: 0,
                explanation: String::new(),
            },
            SongRecognitionCandidate {
                song_id: "b".to_string(),
                match_type: MatchType::ExactLyric,
                matched_text: "x".to_string(),
                confidence: ConfidenceResult::new(0.73, ConfidenceSource::Heuristic, None),
                evidence: vec![],
                source: "music:dev".to_string(),
                ranking: 1,
                explanation: String::new(),
            },
        ];
        assert!(is_ambiguous(&candidates, &thresholds));
    }

    #[test]
    fn is_ambiguous_false_with_a_clear_margin() {
        let thresholds = MatchThresholds::default();
        let candidates = vec![
            SongRecognitionCandidate {
                song_id: "a".to_string(),
                match_type: MatchType::ExplicitTitle,
                matched_text: "x".to_string(),
                confidence: ConfidenceResult::new(0.97, ConfidenceSource::Heuristic, None),
                evidence: vec![],
                source: "music:dev".to_string(),
                ranking: 0,
                explanation: String::new(),
            },
            SongRecognitionCandidate {
                song_id: "b".to_string(),
                match_type: MatchType::PartialLyric,
                matched_text: "x".to_string(),
                confidence: ConfidenceResult::new(0.3, ConfidenceSource::Heuristic, None),
                evidence: vec![],
                source: "music:dev".to_string(),
                ranking: 1,
                explanation: String::new(),
            },
        ];
        assert!(!is_ambiguous(&candidates, &thresholds));
    }

    #[test]
    fn is_ambiguous_false_with_zero_or_one_candidate() {
        let thresholds = MatchThresholds::default();
        assert!(!is_ambiguous(&[], &thresholds));
    }
}
