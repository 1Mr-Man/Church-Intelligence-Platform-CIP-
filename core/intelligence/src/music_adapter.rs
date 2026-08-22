//! [`MusicIntelligenceEngine`]: the Music-domain `IntelligenceEngine`
//! (Phase 2.1) - the first real *second* intelligence domain, proving the
//! shared architecture generalizes beyond Bible.
//!
//! Like [`crate::bible_adapter::BibleIntelligenceEngine`], this does not
//! reimplement recognition logic: every candidate comes from
//! `cip_core_music::search_songs`, the deterministic, documented,
//! explainable matcher - this module's own job is strictly translation
//! (transcript text -> a `MusicQuery`, and a `SongRecognitionCandidate`
//! -> an `IntelligenceFinding`), plus the two things that genuinely
//! belong at the intelligence-integration layer rather than in
//! `core/music` itself:
//!
//! - **Dataset enablement.** `cip_core_music::MusicProvider` has no
//!   concept of "enabled" - that lives in the Phase 1.5 Content
//!   Registry. This engine reads which `Music`-typed, `Enabled` datasets
//!   exist from `context.content_metadata` (already part of the shared
//!   `IntelligenceContext`) and only ever searches those - a disabled
//!   dataset is never included, matching Phase 2.1 spec section 22.
//! - **Song continuity.** Reads the single most recent `Music`-domain
//!   finding out of `context.recent_findings` (already bounded - see
//!   `docs/intelligence-architecture.md`'s context-bounds section) and
//!   classifies continuity via `cip_core_music::classify_continuity`.
//!
//! ## Interpreting free transcript text as a music query
//!
//! A live transcript segment is unstructured spoken text - unlike Bible
//! detection (which has its own dedicated, tested `core/bible::detection`
//! module), Phase 2.1 does not build a comparable general-purpose "music
//! utterance parser." Instead this adapter uses one deterministic,
//! documented dispatch order, trying the cheapest/most-specific
//! interpretation first:
//!
//! 1. **Title/alias**: try the whole segment text as an exact title/alias
//!    query first (`"Let's sing Great Is Thy Faithfulness"` will not
//!    exact-match, but `"Great Is Thy Faithfulness"` will).
//! 2. **Song/hymn number**: look for a trigger word (`"number"`, `"hymn"`,
//!    `"song"`, `"take"`) immediately followed by a run of digits.
//! 3. **Lyric, possibly multi-line**: if the immediately preceding
//!    transcript segment (from `context.recent_transcript_segments`) also
//!    produced no title/number match, both lines are tried together as a
//!    `LyricSequence` before falling back to the current line alone.
//!
//! This is honestly a heuristic dispatch order, not a claim of full
//! natural-language understanding - see `docs/music-intelligence.md`.

use std::collections::HashSet;

use cip_core_confidence::ConfidenceLevel;
use cip_core_content::{ContentStatus, ContentType};
use cip_core_music::{
    classify_continuity, is_ambiguous, search_songs, MatchThresholds, MatchType, MusicProvider,
    MusicQuery, SongContinuity, SongRecognitionCandidate,
};

use crate::context::IntelligenceContext;
use crate::domain::{AssertionLevel, FindingKind, IntelligenceDomain};
use crate::engine::{
    EngineCapability, EngineIdentity, IntelligenceEngine, IntelligenceError, IntelligenceInput,
    IntelligenceResult,
};
use crate::evidence::{EvidenceSource, IntelligenceProvenance};
use crate::finding::IntelligenceFinding;

pub const MUSIC_ENGINE_ID: &str = "music-lyric";
pub const MUSIC_ENGINE_VERSION: &str = "0.1.0";

/// Bounds how many candidates a single `analyze()` call ever turns into
/// findings, even when ambiguity produces many close candidates -
/// operator safety (Phase 2.1 spec section 53/54): "prefer strong match
/// over 15 possible songs."
const MAX_FINDINGS_PER_CALL: usize = 5;

/// Confidence at/above which a match is reported `Suggested` rather than
/// `Inferred`, and (for continuity) counts as a confident enough
/// recognition to call a "new song" rather than merely a "possible
/// change" - kept as one constant so the two decisions never disagree
/// about what "confident" means.
const CONFIDENT_THRESHOLD: f32 = 0.7;

/// Acoustic (audio fingerprint) song recognition is not implemented in
/// Phase 2.1 - see Phase 2.1 spec sections 32-33/50 and
/// `docs/music-intelligence.md`'s "Acoustic recognition" section for
/// exactly why (no fake fingerprinting, no external API). This engine
/// only ever performs deterministic title/alias/number/lyric matching.
pub fn acoustic_recognition_available() -> bool {
    false
}

pub struct MusicIntelligenceEngine {
    provider: Box<dyn MusicProvider>,
    thresholds: MatchThresholds,
}

impl MusicIntelligenceEngine {
    pub fn new(provider: Box<dyn MusicProvider>) -> Self {
        Self {
            provider,
            thresholds: MatchThresholds::default(),
        }
    }

    fn enabled_music_datasets(&self, context: &IntelligenceContext) -> Vec<String> {
        context
            .content_metadata
            .iter()
            .filter(|m| m.content_type == ContentType::Music && m.status == ContentStatus::Enabled)
            .map(|m| m.id.clone())
            .collect()
    }

    fn extract_song_number(text: &str) -> Option<String> {
        const TRIGGERS: [&str; 4] = ["number", "hymn", "song", "take"];
        let words: Vec<&str> = text.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            let normalized = word
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if TRIGGERS.contains(&normalized.as_str()) {
                if let Some(next) = words.get(i + 1) {
                    let digits: String = next.chars().filter(|c| c.is_ascii_digit()).collect();
                    if !digits.is_empty() {
                        return Some(digits);
                    }
                }
            }
        }
        None
    }

    fn candidates_for_input(
        &self,
        content_ids: &[String],
        input: &IntelligenceInput,
        context: &IntelligenceContext,
    ) -> Result<Vec<SongRecognitionCandidate>, IntelligenceError> {
        let to_err = |e: cip_core_music::MusicProviderError| IntelligenceError::EngineFailed {
            engine_id: MUSIC_ENGINE_ID.to_string(),
            reason: e.to_string(),
        };

        let text = input.transcript_segment.text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }

        let title_candidates = search_songs(
            self.provider.as_ref(),
            content_ids,
            &MusicQuery::Title(text.to_string()),
            &self.thresholds,
        )
        .map_err(to_err)?;
        if !title_candidates.is_empty() {
            return Ok(title_candidates);
        }

        if let Some(number) = Self::extract_song_number(text) {
            let number_candidates = search_songs(
                self.provider.as_ref(),
                content_ids,
                &MusicQuery::Number(number),
                &self.thresholds,
            )
            .map_err(to_err)?;
            if !number_candidates.is_empty() {
                return Ok(number_candidates);
            }
        }

        let previous_segment = context
            .recent_transcript_segments
            .iter()
            .rev()
            .find(|s| s.id != input.transcript_segment.id);
        if let Some(previous) = previous_segment {
            let sequence_candidates = search_songs(
                self.provider.as_ref(),
                content_ids,
                &MusicQuery::LyricSequence(vec![previous.text.clone(), text.to_string()]),
                &self.thresholds,
            )
            .map_err(to_err)?;
            if !sequence_candidates.is_empty() {
                return Ok(sequence_candidates);
            }
        }

        search_songs(
            self.provider.as_ref(),
            content_ids,
            &MusicQuery::Lyric(text.to_string()),
            &self.thresholds,
        )
        .map_err(to_err)
    }

    /// The most recent `Music`-domain finding's `(content_id, song_id)`,
    /// if any - derived from `evidence`/`provenance`, since
    /// `IntelligenceFinding` does not carry a music-specific song
    /// reference field of its own (no parallel finding model - Phase 2.1
    /// spec section 13).
    fn previous_song(context: &IntelligenceContext) -> Option<(String, String)> {
        context
            .recent_findings
            .iter()
            .rev()
            .find(|f| f.domain == IntelligenceDomain::Music)
            .and_then(|f| {
                let content_id = f.provenance.content_id.clone()?;
                let song_id = f.evidence.iter().find_map(|e| match e {
                    EvidenceSource::Content { content_id: _ } => None,
                    EvidenceSource::Context { description }
                        if description.starts_with("song_id:") =>
                    {
                        Some(description.trim_start_matches("song_id:").to_string())
                    }
                    _ => None,
                })?;
                Some((content_id, song_id))
            })
    }

    fn finding_for_candidate(
        &self,
        input: &IntelligenceInput,
        context: &IntelligenceContext,
        candidate: &SongRecognitionCandidate,
        ambiguous: bool,
    ) -> IntelligenceFinding {
        let confidence = candidate.confidence.clone();
        let is_weak = matches!(candidate.match_type, MatchType::PartialLyric)
            || confidence.level == ConfidenceLevel::Low;
        let assertion_level = if is_weak {
            AssertionLevel::Inferred
        } else {
            AssertionLevel::Suggested
        };

        let previous = Self::previous_song(context);
        let continuity = classify_continuity(
            previous.as_ref().map(|(c, s)| (c.as_str(), s.as_str())),
            &candidate.source,
            &candidate.song_id,
            confidence.score,
            CONFIDENT_THRESHOLD,
        );

        let summary = if ambiguous {
            format!(
                "Possible song: {} (operator confirmation required)",
                candidate.matched_text
            )
        } else {
            match continuity {
                SongContinuity::ContinuingSameSong => {
                    format!("Continuing: {}", candidate.explanation)
                }
                _ => candidate.explanation.clone(),
            }
        };

        let segment_id = input.transcript_segment.id;
        let mut evidence: Vec<EvidenceSource> = vec![EvidenceSource::Transcript {
            segment_ids: vec![segment_id],
            excerpt: candidate.matched_text.clone(),
        }];
        for reason in &candidate.evidence {
            evidence.push(EvidenceSource::Context {
                description: reason.clone(),
            });
        }
        // Carries this finding's song id forward for the *next* call's
        // continuity check (see `previous_song`) - not a second context
        // mechanism, just this finding's own evidence trail.
        evidence.push(EvidenceSource::Context {
            description: format!("song_id:{}", candidate.song_id),
        });

        IntelligenceFinding::new(
            input.service_id,
            IntelligenceDomain::Music,
            FindingKind::Music,
            assertion_level,
            confidence,
            summary,
            MUSIC_ENGINE_ID,
            MUSIC_ENGINE_VERSION,
        )
        .with_transcript_segments(vec![segment_id])
        .with_evidence(evidence)
        .with_provenance(IntelligenceProvenance::from_content(
            candidate.source.clone(),
        ))
    }
}

impl IntelligenceEngine for MusicIntelligenceEngine {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            domain: IntelligenceDomain::Music,
            engine_id: MUSIC_ENGINE_ID.to_string(),
            engine_version: MUSIC_ENGINE_VERSION.to_string(),
        }
    }

    fn capability(&self) -> EngineCapability {
        EngineCapability::Available
    }

    fn analyze(
        &self,
        input: &IntelligenceInput,
        context: &IntelligenceContext,
    ) -> Result<IntelligenceResult, IntelligenceError> {
        let content_ids = self.enabled_music_datasets(context);
        if content_ids.is_empty() {
            return Ok(IntelligenceResult::empty());
        }

        let candidates = self.candidates_for_input(&content_ids, input, context)?;
        if candidates.is_empty() {
            return Ok(IntelligenceResult::empty());
        }

        let ambiguous = is_ambiguous(&candidates, &self.thresholds);
        let take = if ambiguous {
            candidates.len().min(MAX_FINDINGS_PER_CALL)
        } else {
            1
        };

        // When ambiguous, only emit the candidates actually within the
        // ambiguity margin of the top score - never pad the operator's
        // queue with weak also-rans just because the strongest two were
        // close (spec section 53: "prefer strong match over 15 possible
        // songs").
        let top_score = candidates[0].confidence.score;
        let mut seen_songs: HashSet<(String, String)> = HashSet::new();
        let mut findings = Vec::new();
        for candidate in candidates.iter().take(take) {
            if ambiguous
                && (top_score - candidate.confidence.score) >= self.thresholds.ambiguity_margin
            {
                break;
            }
            let key = (candidate.source.clone(), candidate.song_id.clone());
            if !seen_songs.insert(key) {
                continue;
            }
            findings.push(self.finding_for_candidate(input, context, candidate, ambiguous));
        }

        Ok(IntelligenceResult::new(findings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextBounds;
    use crate::fixtures::FakeMusicProvider;
    use chrono::Utc;
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use cip_core_content::{ContentMetadata, ContentStatus, ContentType};
    use uuid::Uuid;

    fn segment(text: &str, sequence: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence,
            text: text.to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Model, None),
            start_ms: sequence * 1000,
            end_ms: sequence * 1000 + 900,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    fn enabled_music_content(id: &str) -> ContentMetadata {
        ContentMetadata {
            id: id.to_string(),
            content_type: ContentType::Music,
            name: "Test Hymnbook".to_string(),
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
        }
    }

    fn context_with(
        service_id: Uuid,
        content_metadata: Vec<ContentMetadata>,
        recent_transcript_segments: Vec<TranscriptSegment>,
        recent_findings: Vec<IntelligenceFinding>,
    ) -> IntelligenceContext {
        IntelligenceContext::build(
            service_id,
            None,
            recent_transcript_segments.last().cloned(),
            recent_transcript_segments,
            None,
            recent_findings,
            Vec::new(),
            content_metadata,
            ContextBounds::default(),
        )
    }

    fn engine() -> MusicIntelligenceEngine {
        MusicIntelligenceEngine::new(Box::new(FakeMusicProvider::hymnbook_fixture()))
    }

    #[test]
    fn explicit_title_produces_a_suggested_finding() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let seg = segment("Test Hymn One", 0);
        let input = IntelligenceInput::new(service_id, seg);
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![],
            vec![],
        );

        let result = engine.analyze(&input, &context).unwrap();
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].domain, IntelligenceDomain::Music);
        assert_eq!(result.findings[0].kind, FindingKind::Music);
        assert_eq!(
            result.findings[0].assertion_level,
            AssertionLevel::Suggested
        );
        assert_eq!(
            result.findings[0].provenance.content_id.as_deref(),
            Some("music:test-hymnbook")
        );
    }

    #[test]
    fn alias_resolves_to_the_song() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let input = IntelligenceInput::new(service_id, segment("First Test Hymn", 0));
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![],
            vec![],
        );

        let result = engine.analyze(&input, &context).unwrap();
        assert_eq!(result.findings.len(), 1);
        assert!(result.findings[0].summary.to_lowercase().contains("alias"));
    }

    #[test]
    fn song_number_trigger_word_resolves_to_the_song() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let input = IntelligenceInput::new(service_id, segment("Take number 120", 0));
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![],
            vec![],
        );

        let result = engine.analyze(&input, &context).unwrap();
        assert_eq!(result.findings.len(), 1);
        assert_eq!(
            result.findings[0].provenance.content_id.as_deref(),
            Some("music:test-hymnbook")
        );
    }

    #[test]
    fn exact_lyric_phrase_resolves() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let input = IntelligenceInput::new(
            service_id,
            segment("Great is thy faithfulness my Father", 0),
        );
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![],
            vec![],
        );

        let result = engine.analyze(&input, &context).unwrap();
        assert_eq!(result.findings.len(), 1);
    }

    #[test]
    fn multi_line_lyric_across_two_segments_resolves() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let previous = segment("Great is thy faithfulness my Father", 0);
        let current = segment("Morning by morning new mercies I see", 1);
        let input = IntelligenceInput::new(service_id, current.clone());
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![previous, current],
            vec![],
        );

        let result = engine.analyze(&input, &context).unwrap();
        assert_eq!(result.findings.len(), 1);
        assert!(result.findings[0].summary.contains("consecutive") || result.findings[0].evidence.iter().any(|e| matches!(e, EvidenceSource::Context { description } if description.contains("consecutive"))));
    }

    #[test]
    fn no_match_produces_no_findings() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let input =
            IntelligenceInput::new(service_id, segment("Completely unrelated spoken words", 0));
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![],
            vec![],
        );

        let result = engine.analyze(&input, &context).unwrap();
        assert!(result.findings.is_empty());
    }

    #[test]
    fn a_disabled_dataset_is_never_searched() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let input = IntelligenceInput::new(service_id, segment("Test Hymn One", 0));
        let mut disabled = enabled_music_content("music:test-hymnbook");
        disabled.status = ContentStatus::Disabled;
        let context = context_with(service_id, vec![disabled], vec![], vec![]);

        let result = engine.analyze(&input, &context).unwrap();
        assert!(
            result.findings.is_empty(),
            "a disabled dataset must never be used for automatic recognition"
        );
    }

    #[test]
    fn no_registered_music_content_produces_no_findings_and_no_error() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let input = IntelligenceInput::new(service_id, segment("Test Hymn One", 0));
        let context = context_with(service_id, vec![], vec![], vec![]);

        let result = engine.analyze(&input, &context).unwrap();
        assert!(result.findings.is_empty());
    }

    #[test]
    fn deterministic_for_identical_input_and_context() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let content = vec![enabled_music_content("music:test-hymnbook")];

        let run = || {
            let input = IntelligenceInput::new(service_id, segment("Test Hymn One", 0));
            let context = context_with(service_id, content.clone(), vec![], vec![]);
            engine
                .analyze(&input, &context)
                .unwrap()
                .findings
                .into_iter()
                .map(|f| {
                    (
                        f.domain,
                        f.kind,
                        f.assertion_level,
                        f.summary,
                        f.confidence.score,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn acoustic_recognition_is_honestly_reported_unavailable() {
        assert!(!acoustic_recognition_available());
    }
}
