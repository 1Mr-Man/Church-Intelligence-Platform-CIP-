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

use std::collections::{HashMap, HashSet};

use cip_core_confidence::ConfidenceLevel;
use cip_core_content::{ContentStatus, ContentType};
use cip_core_music::{
    classify_continuity, fuse_acoustic_with_context, is_ambiguous, search_songs,
    AcousticRecognitionCandidate, AcousticRecognitionMethod, MatchThresholds, MatchType,
    MusicProvider, MusicQuery, SongContinuity, SongRecognitionCandidate,
};

use crate::context::IntelligenceContext;
use crate::domain::{AssertionLevel, FindingKind, IntelligenceDomain};
use crate::engine::{
    EngineCapability, EngineIdentity, IntelligenceEngine, IntelligenceError, IntelligenceInput,
    IntelligenceResult,
};
use crate::evidence::{EvidenceSource, IntelligenceProvenance};
use crate::finding::IntelligenceFinding;
use uuid::Uuid;

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

/// `MusicIntelligenceEngine` itself has no acoustic (audio-fingerprint)
/// recognition built in - `analyze()` only ever performs deterministic
/// title/alias/number/lyric matching, unchanged from Phase 2.1. This
/// free function is kept exactly as Phase 2.1 defined it (always
/// `false`) for that narrower claim, and is now distinct from *whether
/// an acoustic recognizer is configured at all*: Phase 2.2 adds a real
/// `AcousticMusicRecognizer` boundary (`cip_core_music::AcousticMusicRecognizer`)
/// with its own, per-instance, honestly-reported `status()` - see
/// [`analyze_acoustic`](MusicIntelligenceEngine::analyze_acoustic) and
/// `docs/acoustic-music.md`. Never read this function's `false` as "no
/// acoustic recognizer could ever be wired into this application" - it
/// only means "this specific translation engine does not itself perform
/// acoustic matching."
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

    /// The most recent `Music`-domain finding, converted into a single
    /// `MatchType::Contextual` candidate carrying *that finding's own*
    /// confidence - the "lyric/title side" of acoustic evidence fusion.
    /// Deliberately reuses the exact same single-most-recent-finding
    /// convention as [`Self::previous_song`] (rather than scanning several
    /// recent findings) so there is only one continuity/context
    /// extraction rule in this file, not two that could disagree. Never
    /// more than one element - `fuse_acoustic_with_context` only ever
    /// *strengthens* an acoustic candidate that already exists for this
    /// same song, it can never introduce one on its own.
    fn recent_music_context_evidence(
        context: &IntelligenceContext,
    ) -> Vec<SongRecognitionCandidate> {
        context
            .recent_findings
            .iter()
            .rev()
            .find(|f| f.domain == IntelligenceDomain::Music)
            .and_then(|f| {
                let content_id = f.provenance.content_id.clone()?;
                let song_id = f.evidence.iter().find_map(|e| match e {
                    EvidenceSource::Context { description }
                        if description.starts_with("song_id:") =>
                    {
                        Some(description.trim_start_matches("song_id:").to_string())
                    }
                    _ => None,
                })?;
                Some(SongRecognitionCandidate {
                    song_id,
                    match_type: MatchType::Contextual,
                    matched_text: f.summary.clone(),
                    confidence: f.confidence.clone(),
                    evidence: vec![format!("recent finding: {}", f.summary)],
                    source: content_id,
                    ranking: 0,
                    explanation: f.summary.clone(),
                })
            })
            .into_iter()
            .collect()
    }

    /// Acoustic-sourced counterpart to [`IntelligenceEngine::analyze`] -
    /// deliberately an inherent method, not part of the shared
    /// `IntelligenceEngine` trait: acoustic recognition operates on an
    /// `AudioSegment`, not a `TranscriptSegment`, so it does not fit
    /// `IntelligenceInput`'s shape, and adding it to the trait would force
    /// every other domain engine (Bible included) to grow an
    /// acoustic-shaped parameter it has no use for. The acoustic worker
    /// (`apps/desktop/src-tauri/src/acoustic.rs`) calls this directly, the
    /// same way it calls `feed_audio` on a `SpeechEngine` directly rather
    /// than through a shared trait.
    ///
    /// `acoustic_candidates` is independently re-filtered to the service's
    /// *currently enabled* Music datasets here, as defense in depth (Phase
    /// 2.2 rule: acoustic recognition must never resolve into a disabled
    /// or wrong dataset) - exactly like `analyze`'s own
    /// `enabled_music_datasets` check. Fusion, ambiguity handling,
    /// per-call finding bounds, and duplicate-song dedup all reuse the
    /// same policy `analyze` uses; this method exists only because the
    /// *input* shape differs, never because acoustic findings follow a
    /// different confidence or approval policy.
    pub fn analyze_acoustic(
        &self,
        service_id: Uuid,
        acoustic_candidates: &[AcousticRecognitionCandidate],
        context: &IntelligenceContext,
    ) -> Result<IntelligenceResult, IntelligenceError> {
        let content_ids = self.enabled_music_datasets(context);
        if content_ids.is_empty() {
            return Ok(IntelligenceResult::empty());
        }

        let scoped: Vec<AcousticRecognitionCandidate> = acoustic_candidates
            .iter()
            .filter(|c| content_ids.contains(&c.content_id))
            .cloned()
            .collect();
        if scoped.is_empty() {
            return Ok(IntelligenceResult::empty());
        }

        // Original acoustic metadata (segment id, method, duration) does
        // not survive `fuse_acoustic_with_context`'s conversion to
        // `SongRecognitionCandidate` - keep a side lookup so it can be
        // recovered when building each finding's `EvidenceSource::Acoustic`
        // entry. Mirrors `fuse_acoustic_with_context`'s own
        // dedup-keep-strongest rule so the recovered metadata always
        // matches the observation that actually won the fusion.
        let mut strongest_by_key: HashMap<(String, String), &AcousticRecognitionCandidate> =
            HashMap::new();
        for candidate in &scoped {
            let key = (candidate.content_id.clone(), candidate.song_id.clone());
            match strongest_by_key.get(&key) {
                Some(existing) if existing.confidence.score >= candidate.confidence.score => {}
                _ => {
                    strongest_by_key.insert(key, candidate);
                }
            }
        }

        let context_evidence = Self::recent_music_context_evidence(context);
        let fused = fuse_acoustic_with_context(&scoped, &context_evidence);
        if fused.is_empty() {
            return Ok(IntelligenceResult::empty());
        }

        let ambiguous = is_ambiguous(&fused, &self.thresholds);
        let take = if ambiguous {
            fused.len().min(MAX_FINDINGS_PER_CALL)
        } else {
            1
        };

        let top_score = fused[0].confidence.score;
        let mut seen_songs: HashSet<(String, String)> = HashSet::new();
        let mut findings = Vec::new();
        for candidate in fused.iter().take(take) {
            if ambiguous
                && (top_score - candidate.confidence.score) >= self.thresholds.ambiguity_margin
            {
                break;
            }
            let key = (candidate.source.clone(), candidate.song_id.clone());
            if !seen_songs.insert(key.clone()) {
                continue;
            }
            let source = strongest_by_key.get(&key).copied();
            findings.push(
                self.finding_for_acoustic_candidate(
                    service_id, context, candidate, source, ambiguous,
                ),
            );
        }

        Ok(IntelligenceResult::new(findings))
    }

    fn finding_for_acoustic_candidate(
        &self,
        service_id: Uuid,
        context: &IntelligenceContext,
        candidate: &SongRecognitionCandidate,
        source: Option<&AcousticRecognitionCandidate>,
        ambiguous: bool,
    ) -> IntelligenceFinding {
        let confidence = candidate.confidence.clone();
        let is_weak = confidence.level == ConfidenceLevel::Low;
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
                "Possible song (acoustic): {} (operator confirmation required)",
                candidate.matched_text
            )
        } else {
            match continuity {
                SongContinuity::ContinuingSameSong => {
                    format!("Continuing (acoustic): {}", candidate.explanation)
                }
                _ => candidate.explanation.clone(),
            }
        };

        let mut evidence: Vec<EvidenceSource> = Vec::new();
        if let Some(source) = source {
            evidence.push(EvidenceSource::Acoustic {
                segment_id: source.segment_id,
                method: acoustic_method_label(source.method),
                duration_ms: source.duration_ms,
            });
        }
        for reason in &candidate.evidence {
            evidence.push(EvidenceSource::Context {
                description: reason.clone(),
            });
        }
        // Same "carry the song id forward for the next call's continuity
        // check" convention `finding_for_candidate` uses - one evidence
        // trail mechanism, not two.
        evidence.push(EvidenceSource::Context {
            description: format!("song_id:{}", candidate.song_id),
        });

        IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Music,
            FindingKind::Music,
            assertion_level,
            confidence,
            summary,
            MUSIC_ENGINE_ID,
            MUSIC_ENGINE_VERSION,
        )
        .with_evidence(evidence)
        .with_provenance(IntelligenceProvenance::from_content(
            candidate.source.clone(),
        ))
    }
}

/// Human-readable, stable text for `EvidenceSource::Acoustic.method` -
/// intentionally distinct from `AcousticRecognitionMethod`'s own serde
/// representation (this crate must not depend on `serde_json` outside
/// tests), but using the same snake_case vocabulary so the two never
/// visually disagree.
fn acoustic_method_label(method: AcousticRecognitionMethod) -> String {
    match method {
        AcousticRecognitionMethod::LocalModel => "local_model",
        AcousticRecognitionMethod::ExternalProvider => "external_provider",
        AcousticRecognitionMethod::Test => "test",
        AcousticRecognitionMethod::None => "none",
    }
    .to_string()
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
            licensing_status: cip_core_content::LicensingStatus::Unknown,
            usage: cip_core_content::UsagePermissions::default(),
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
            segment_id: Uuid::new_v4(),
            duration_ms: 8_000,
            evidence: vec!["acoustic test evidence".to_string()],
        }
    }

    #[test]
    fn acoustic_only_candidate_produces_a_finding() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![],
            vec![],
        );
        let candidates = vec![acoustic_candidate("h1", "music:test-hymnbook", 0.8)];

        let result = engine
            .analyze_acoustic(service_id, &candidates, &context)
            .unwrap();
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].domain, IntelligenceDomain::Music);
        assert_eq!(
            result.findings[0].provenance.content_id.as_deref(),
            Some("music:test-hymnbook")
        );
    }

    #[test]
    fn acoustic_finding_carries_acoustic_evidence_with_segment_and_method() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![],
            vec![],
        );
        let candidate = acoustic_candidate("h1", "music:test-hymnbook", 0.8);
        let segment_id = candidate.segment_id;
        let candidates = vec![candidate];

        let result = engine
            .analyze_acoustic(service_id, &candidates, &context)
            .unwrap();
        let evidence = &result.findings[0].evidence;
        assert!(evidence.iter().any(|e| matches!(
            e,
            EvidenceSource::Acoustic {
                segment_id: sid,
                method,
                duration_ms,
            } if *sid == segment_id && method == "test" && *duration_ms == 8_000
        )));
    }

    #[test]
    fn no_acoustic_candidates_produce_no_findings() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![],
            vec![],
        );

        let result = engine.analyze_acoustic(service_id, &[], &context).unwrap();
        assert!(result.findings.is_empty());
    }

    #[test]
    fn a_disabled_dataset_never_resolves_acoustic_candidates() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let mut disabled = enabled_music_content("music:test-hymnbook");
        disabled.status = ContentStatus::Disabled;
        let context = context_with(service_id, vec![disabled], vec![], vec![]);
        let candidates = vec![acoustic_candidate("h1", "music:test-hymnbook", 0.9)];

        let result = engine
            .analyze_acoustic(service_id, &candidates, &context)
            .unwrap();
        assert!(
            result.findings.is_empty(),
            "a disabled dataset must never be resolved by acoustic recognition either"
        );
    }

    #[test]
    fn a_candidate_naming_a_dataset_outside_the_enabled_set_is_dropped() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![],
            vec![],
        );
        let candidates = vec![acoustic_candidate("h1", "music:some-other-dataset", 0.9)];

        let result = engine
            .analyze_acoustic(service_id, &candidates, &context)
            .unwrap();
        assert!(result.findings.is_empty());
    }

    #[test]
    fn ambiguous_acoustic_candidates_emit_both_for_operator_choice() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![],
            vec![],
        );
        let candidates = vec![
            acoustic_candidate("h1", "music:test-hymnbook", 0.81),
            acoustic_candidate("h2", "music:test-hymnbook", 0.80),
        ];

        let result = engine
            .analyze_acoustic(service_id, &candidates, &context)
            .unwrap();
        assert_eq!(
            result.findings.len(),
            2,
            "close candidates must be left for the operator to choose, never silently resolved"
        );
        assert!(result
            .findings
            .iter()
            .all(|f| f.summary.contains("operator confirmation required")));
    }

    #[test]
    fn recent_lyric_finding_for_the_same_song_strengthens_acoustic_confidence() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let content = vec![enabled_music_content("music:test-hymnbook")];

        // A genuine lyric-derived finding for "h1", produced the same way
        // live transcript-driven `analyze()` would produce one.
        let lyric_context = context_with(service_id, content.clone(), vec![], vec![]);
        let lyric_input = IntelligenceInput::new(service_id, segment("Test Hymn One", 0));
        let lyric_result = engine.analyze(&lyric_input, &lyric_context).unwrap();
        assert_eq!(lyric_result.findings.len(), 1);
        let lyric_finding = lyric_result.findings[0].clone();
        let lyric_score = lyric_finding.confidence.score;

        let acoustic_score = 0.55;
        let context_with_recent_finding =
            context_with(service_id, content, vec![], vec![lyric_finding]);
        let candidates = vec![acoustic_candidate(
            "h1",
            "music:test-hymnbook",
            acoustic_score,
        )];

        let result = engine
            .analyze_acoustic(service_id, &candidates, &context_with_recent_finding)
            .unwrap();
        assert_eq!(result.findings.len(), 1);
        let fused_score = result.findings[0].confidence.score;
        assert!(
            fused_score > acoustic_score && fused_score > lyric_score,
            "acoustic + corroborating lyric evidence for the same song must strengthen \
             confidence beyond either source alone (acoustic={acoustic_score}, lyric={lyric_score}, fused={fused_score})"
        );
    }

    #[test]
    fn a_recent_finding_for_a_different_song_never_invents_or_boosts_an_unrelated_candidate() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let content = vec![enabled_music_content("music:test-hymnbook")];

        let lyric_context = context_with(service_id, content.clone(), vec![], vec![]);
        let lyric_input = IntelligenceInput::new(service_id, segment("Test Hymn One", 0));
        let lyric_result = engine.analyze(&lyric_input, &lyric_context).unwrap();
        let lyric_finding = lyric_result.findings[0].clone();

        // Acoustic evidence names a *different* song ("h2") than the
        // recent lyric finding ("h1") - the recent finding must never
        // leak its confidence into an unrelated candidate.
        let acoustic_score = 0.6;
        let context_with_recent_finding =
            context_with(service_id, content, vec![], vec![lyric_finding]);
        let candidates = vec![acoustic_candidate(
            "h2",
            "music:test-hymnbook",
            acoustic_score,
        )];

        let result = engine
            .analyze_acoustic(service_id, &candidates, &context_with_recent_finding)
            .unwrap();
        assert_eq!(result.findings.len(), 1);
        assert_eq!(
            result.findings[0].confidence.score, acoustic_score,
            "unrelated recent evidence must not change this candidate's confidence at all"
        );
        assert_eq!(
            result.findings[0].provenance.content_id.as_deref(),
            Some("music:test-hymnbook")
        );
    }

    #[test]
    fn duplicate_acoustic_windows_for_the_same_song_do_not_inflate_confidence() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let context = context_with(
            service_id,
            vec![enabled_music_content("music:test-hymnbook")],
            vec![],
            vec![],
        );
        let candidates = vec![
            acoustic_candidate("h1", "music:test-hymnbook", 0.6),
            acoustic_candidate("h1", "music:test-hymnbook", 0.6),
        ];

        let result = engine
            .analyze_acoustic(service_id, &candidates, &context)
            .unwrap();
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].confidence.score, 0.6);
    }

    #[test]
    fn analyze_acoustic_is_deterministic_for_identical_input_and_context() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let content = vec![enabled_music_content("music:test-hymnbook")];

        let run = || {
            let context = context_with(service_id, content.clone(), vec![], vec![]);
            let candidates = vec![acoustic_candidate("h1", "music:test-hymnbook", 0.8)];
            engine
                .analyze_acoustic(service_id, &candidates, &context)
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
}
