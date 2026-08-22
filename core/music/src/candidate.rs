//! [`SongRecognitionCandidate`]: one auditable, evidence-carrying
//! recognition result. Recognition never returns a bare `Song` - it
//! always returns candidates, so "why was this suggested" is answerable
//! without a second lookup (Phase 2.1 spec section 7: "a finding should
//! be auditable").

use serde::{Deserialize, Serialize};

use cip_core_confidence::ConfidenceResult;

/// How a candidate was matched. `Acoustic` (Phase 2.2) is now genuinely
/// constructed - by `crate::fusion`, from an
/// `AcousticRecognitionCandidate` a real `AcousticMusicRecognizer`
/// returned, never fabricated when no recognizer is configured (see
/// `docs/acoustic-music.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchType {
    ExplicitTitle,
    Alias,
    SongNumber,
    ExactLyric,
    MultipleLyricLines,
    PartialLyric,
    Contextual,
    Acoustic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongRecognitionCandidate {
    pub song_id: String,
    pub match_type: MatchType,
    /// The transcript/query text that produced this candidate.
    pub matched_text: String,
    pub confidence: ConfidenceResult,
    /// Human-readable reasons, e.g. `"Exact title match"`,
    /// `"Matched 2 consecutive lyric lines"` - never an opaque score
    /// alone.
    pub evidence: Vec<String>,
    /// The dataset (Content Registry id, e.g. `"music:dev-hymnbook"`)
    /// this candidate's song belongs to.
    pub source: String,
    /// 0-based rank within the result set this candidate was returned
    /// in, assigned by [`crate::matcher::search_songs`] after
    /// deterministic sorting - never hash-map iteration order.
    pub ranking: u32,
    /// A single synthesized human-readable explanation, suitable for
    /// direct display (e.g. `"Exact title match"`,
    /// `"Two songs match the phrase 'I will worship you'"`).
    pub explanation: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_confidence::ConfidenceSource;

    #[test]
    fn candidate_serializes_with_camel_case_fields() {
        let candidate = SongRecognitionCandidate {
            song_id: "s1".to_string(),
            match_type: MatchType::ExplicitTitle,
            matched_text: "Great Is Thy Faithfulness".to_string(),
            confidence: ConfidenceResult::new(0.97, ConfidenceSource::Heuristic, None),
            evidence: vec!["Exact title match".to_string()],
            source: "music:dev-hymnbook".to_string(),
            ranking: 0,
            explanation: "Exact title match".to_string(),
        };
        let json = serde_json::to_value(&candidate).unwrap();
        assert_eq!(json["songId"], "s1");
        assert_eq!(json["matchType"], "explicit_title");
    }
}
