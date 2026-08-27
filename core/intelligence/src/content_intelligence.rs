//! [`ContentIntelligenceEngine`]: the Content Intelligence layer (Phase
//! 2.7, per the authoritative Phase 2 roadmap). Reads already-produced
//! findings out of a shared [`IntelligenceContext`] and structures
//! [`ContentCandidate`]s from them - it never calls `BibleIntelligenceEngine`,
//! `MusicIntelligenceEngine`, `ServiceIntelligenceEngine`,
//! `SermonIntelligenceEngine`, or `CrossDomainCorrelationEngine` directly,
//! and it never mutates a source finding, the transcript, the active
//! Scripture context, or any presentation state. See
//! `docs/content-intelligence.md` for the full taxonomy, mapping table,
//! and design rationale.
//!
//! ## Why this is not an `IntelligenceEngine`
//!
//! [`crate::IntelligenceEngine::analyze`] returns `Vec<IntelligenceFinding>` -
//! a content candidate is a structurally different value
//! ([`ContentCandidate`], deliberately not folded into `IntelligenceFinding`;
//! see `content_candidate.rs`'s module docs), so this engine does not
//! implement the shared trait and is not registered into
//! `IntelligenceEngineRegistry`. This mirrors `CrossDomainCorrelationEngine`'s
//! own, identical precedent (`cross_domain.rs`'s module docs) exactly - the
//! same reasoning applies here without alteration.
//!
//! ## Source-to-content mapping (spec section 9/33)
//!
//! Every mapping is an explicit `(&str summary prefix, ContentCandidateType)`
//! pair in [`SUMMARY_PREFIX_MAPPINGS`] - never an opaque heuristic. Only
//! `IntelligenceDomain::Sermon`/`FindingKind::Sermon` findings are mapped in
//! this initial phase (the only domain with a real, structured taxonomy to
//! draw from as of Phase 2.6); Bible/Music/Service findings are not yet
//! mapped - see `docs/content-intelligence.md`'s "NOT AVAILABLE" section.

use std::collections::HashSet;

use uuid::Uuid;

use crate::content_candidate::{ContentCandidate, ContentCandidateType};
use crate::context::IntelligenceContext;
use crate::domain::{AssertionLevel, FindingKind, FindingStatus, IntelligenceDomain};
use crate::engine::IntelligenceError;
use crate::evidence::EvidenceSource;
use crate::finding::IntelligenceFinding;

pub const CONTENT_ENGINE_ID: &str = "content-intelligence";
pub const CONTENT_ENGINE_VERSION: &str = "0.1.0";

/// Explicit, documented source-to-content mapping (spec section 9): the
/// summary prefix every `FindingKind::Sermon` finding already carries
/// (Phase 2.6's own summary-prefix convention) maps to exactly one
/// [`ContentCandidateType`]. Every prefix not listed here (`Definition:`,
/// `Declaration:`, `Prayer Point:`, `Summary:`, `Reflection:`,
/// `Transition:`, `Possible Conclusion:`, `Structural Transition (section):`,
/// and every `Sermon foundation:`-prefixed structural finding) is
/// deliberately not mapped - see `docs/content-intelligence.md` for why
/// each was excluded.
const SUMMARY_PREFIX_MAPPINGS: &[(&str, ContentCandidateType)] = &[
    ("Theme: ", ContentCandidateType::Theme),
    ("Main Point: ", ContentCandidateType::Teaching),
    ("Sub-Point: ", ContentCandidateType::Teaching),
    ("Application: ", ContentCandidateType::Reflection),
    ("Takeaway: ", ContentCandidateType::Takeaway),
    ("Food for Thought: ", ContentCandidateType::FoodForThought),
    ("Key Statement: ", ContentCandidateType::Quote),
    ("Question: ", ContentCandidateType::DiscussionQuestion),
    (
        "Supporting Scripture: ",
        ContentCandidateType::ScriptureReflection,
    ),
    ("Illustration: ", ContentCandidateType::Illustration),
    ("Story: ", ContentCandidateType::Illustration),
    ("Example: ", ContentCandidateType::Illustration),
];

/// Which [`ContentCandidateType`] (if any) `finding` maps to, plus the
/// summary text with its prefix stripped. `None` for any finding whose
/// domain/kind isn't `Sermon`, or whose summary matches none of
/// [`SUMMARY_PREFIX_MAPPINGS`].
fn candidate_type_for_finding(
    finding: &IntelligenceFinding,
) -> Option<(ContentCandidateType, &str)> {
    if finding.domain != IntelligenceDomain::Sermon || finding.kind != FindingKind::Sermon {
        return None;
    }
    for (prefix, candidate_type) in SUMMARY_PREFIX_MAPPINGS {
        if let Some(rest) = finding.summary.strip_prefix(prefix) {
            return Some((*candidate_type, rest));
        }
    }
    None
}

/// Deterministic eligibility filter (spec section 10): a finding must be
/// unresolved-or-accepted (never `Rejected`/`Expired`), never `Generated`,
/// and carry at least one piece of evidence to explain itself. Documented
/// thresholds, no magic numbers.
fn is_eligible(finding: &IntelligenceFinding) -> bool {
    matches!(
        finding.status,
        FindingStatus::Detected | FindingStatus::Reviewed | FindingStatus::Accepted
    ) && finding.assertion_level != AssertionLevel::Generated
        && !finding.evidence.is_empty()
}

/// Quote-integrity rule (spec section 14): a `Quote` candidate may only be
/// built from a finding carrying verbatim `EvidenceSource::Transcript`
/// evidence - never from a purely `Context`/inferential evidence entry, so
/// a paraphrase can never be presented as an exact quotation.
fn quote_is_verbatim(finding: &IntelligenceFinding) -> bool {
    finding
        .evidence
        .iter()
        .any(|e| matches!(e, EvidenceSource::Transcript { .. }))
}

/// The exact verbatim transcript excerpt backing a `Quote` candidate -
/// `None` should never occur once [`quote_is_verbatim`] has already
/// gated the call site, but this stays a fallible lookup rather than an
/// `unwrap`, so a future refactor cannot introduce a panic here.
fn verbatim_excerpt(finding: &IntelligenceFinding) -> Option<&str> {
    finding.evidence.iter().find_map(|e| match e {
        EvidenceSource::Transcript { excerpt, .. } => Some(excerpt.as_str()),
        _ => None,
    })
}

/// Deterministic content-potential formula (spec rules 9/10/21) -
/// explicitly independent of [`ConfidenceResult`]: a `type_weight` fixed
/// per [`ContentCandidateType`] (structural/content-type suitability,
/// never derived from how *certain* the underlying fact is) plus a small
/// evidence-count bonus (capped at 0.15, so a finding with many evidence
/// entries never dominates purely on volume). Both factors are documented
/// here, not hidden behind unexplained numbers.
fn content_potential_for(candidate_type: ContentCandidateType, evidence_count: usize) -> f32 {
    let type_weight: f32 = match candidate_type {
        ContentCandidateType::Quote => 0.60,
        ContentCandidateType::Theme => 0.55,
        ContentCandidateType::Takeaway => 0.55,
        ContentCandidateType::ScriptureReflection => 0.50,
        ContentCandidateType::Teaching => 0.45,
        ContentCandidateType::Reflection => 0.40,
        ContentCandidateType::FoodForThought => 0.40,
        ContentCandidateType::DiscussionQuestion => 0.35,
        ContentCandidateType::Illustration => 0.35,
    };
    let evidence_bonus = (evidence_count as f32 * 0.05).min(0.15);
    (type_weight + evidence_bonus).clamp(0.0, 1.0)
}

/// A short, deterministic label - truncates on a character boundary,
/// never mid-codepoint, and never adds invented words.
fn short_label(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}

fn build_candidate(
    finding: &IntelligenceFinding,
    candidate_type: ContentCandidateType,
    remainder: &str,
) -> ContentCandidate {
    let remainder = remainder.trim();
    let working_concept = if candidate_type == ContentCandidateType::Quote {
        verbatim_excerpt(finding).unwrap_or(remainder).to_string()
    } else {
        remainder.to_string()
    };
    let title_or_label = format!("{}: {}", candidate_type.label(), short_label(remainder, 60));
    let content_potential = content_potential_for(candidate_type, finding.evidence.len());

    ContentCandidate::new(
        finding.service_id,
        finding.sermon_id,
        vec![finding.id],
        candidate_type,
        title_or_label,
        working_concept,
        finding.assertion_level,
        finding.confidence.clone(),
        content_potential,
        CONTENT_ENGINE_ID,
        CONTENT_ENGINE_VERSION,
    )
    .with_evidence(finding.evidence.clone())
    .with_provenance(finding.provenance.clone())
}

/// Deterministic duplicate suppression (spec section 19), mirroring
/// `cross_domain::dedup`'s exact hash-keyed approach: keep only the first
/// occurrence of each equivalence class
/// ([`ContentCandidate::is_equivalent_to`]). O(n), not the naive O(n^2)
/// "scan every already-kept candidate."
fn dedup(candidates: &mut Vec<ContentCandidate>) {
    let mut seen: HashSet<(Uuid, ContentCandidateType, Vec<Uuid>)> =
        HashSet::with_capacity(candidates.len());
    candidates.retain(|candidate| {
        let mut ids = candidate.source_finding_ids.clone();
        ids.sort_unstable();
        let key = (candidate.service_id, candidate.candidate_type, ids);
        seen.insert(key)
    });
}

/// Deterministic ordering (spec section 11/20): content potential
/// descending, then type label, then sorted source finding ids, then id
/// as a final stable tiebreak. Never depends on hash-iteration order.
fn sort_deterministically(candidates: &mut [ContentCandidate]) {
    candidates.sort_by(|a, b| {
        b.content_potential
            .total_cmp(&a.content_potential)
            .then_with(|| a.candidate_type.label().cmp(b.candidate_type.label()))
            .then_with(|| {
                let mut a_ids = a.source_finding_ids.clone();
                a_ids.sort_unstable();
                let mut b_ids = b.source_finding_ids.clone();
                b_ids.sort_unstable();
                a_ids.cmp(&b_ids)
            })
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// The Phase 2.7 content-intelligence layer. Stateless (no fields yet -
/// reserved for future configurable thresholds, mirroring
/// `CrossDomainCorrelationEngine`'s own shape); safe to construct fresh
/// per call or hold for a service's lifetime.
#[derive(Debug, Default, Clone, Copy)]
pub struct ContentIntelligenceEngine;

impl ContentIntelligenceEngine {
    pub fn new() -> Self {
        Self
    }

    /// Structure every eligible finding in `context.recent_findings` into
    /// a candidate. Bounded by construction: `recent_findings` is already
    /// bounded by `IntelligenceContext::build` (spec rule 12), and this
    /// method never queries anything beyond what `context` already
    /// carries. Deterministic for identical input (spec rule 11): running
    /// this twice against equivalent contexts produces equivalent
    /// candidates (ignoring `id`/`created_at`). Never panics on empty or
    /// degraded input - an ineligible/unmapped finding is silently
    /// skipped, never a reason to error.
    pub fn analyze(&self, context: &IntelligenceContext) -> Vec<ContentCandidate> {
        let mut candidates = Vec::new();
        for finding in &context.recent_findings {
            if !is_eligible(finding) {
                continue;
            }
            let Some((candidate_type, remainder)) = candidate_type_for_finding(finding) else {
                continue;
            };
            if candidate_type == ContentCandidateType::Quote && !quote_is_verbatim(finding) {
                // Never fabricate a quote from a non-verbatim finding.
                continue;
            }
            candidates.push(build_candidate(finding, candidate_type, remainder));
        }
        dedup(&mut candidates);
        sort_deterministically(&mut candidates);
        candidates
    }
}

// --- storage -------------------------------------------------------------

/// What happened when a candidate was added - mirrors
/// [`crate::CorrelationQueueAddOutcome`] exactly, kept as its own type for
/// the same reason: `FindingQueue`/`CorrelationQueue` are hard-typed to
/// their own element types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentCandidateQueueAddOutcome {
    Added,
    /// An equivalent candidate ([`ContentCandidate::is_equivalent_to`])
    /// was already queued and not yet resolved; the new one was discarded
    /// rather than creating an uncontrolled duplicate.
    DuplicateIgnored,
}

/// In-memory queue of content candidates awaiting operator review - the
/// `ContentCandidate` counterpart to [`crate::FindingQueue`]/
/// [`crate::CorrelationQueue`]. No new database table (spec section 27's
/// explicit default): a candidate is derived from a finding that already
/// has its own provenance/persistence story, so nothing here needs to
/// survive a restart - exactly `CorrelationQueue`'s own precedent, see
/// `docs/content-intelligence.md`'s persistence-decision section.
#[derive(Default)]
pub struct ContentCandidateQueue {
    candidates: Vec<ContentCandidate>,
}

impl ContentCandidateQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a candidate, unless an equivalent one is already queued with a
    /// status that hasn't been resolved yet (`Detected`/`Reviewed`) -
    /// mirrors `FindingQueue::add`/`CorrelationQueue::add`'s exact dedup
    /// policy.
    pub fn add(&mut self, candidate: ContentCandidate) -> ContentCandidateQueueAddOutcome {
        let is_duplicate = self.candidates.iter().any(|existing| {
            matches!(
                existing.status,
                FindingStatus::Detected | FindingStatus::Reviewed
            ) && existing.is_equivalent_to(&candidate)
        });
        if is_duplicate {
            return ContentCandidateQueueAddOutcome::DuplicateIgnored;
        }
        self.candidates.push(candidate);
        ContentCandidateQueueAddOutcome::Added
    }

    /// Candidates still awaiting an operator decision
    /// (`Detected`/`Reviewed`), ordered by content potential (highest
    /// first), then id as a stable tiebreak.
    pub fn pending(&self) -> Vec<&ContentCandidate> {
        let mut pending: Vec<&ContentCandidate> = self
            .candidates
            .iter()
            .filter(|c| matches!(c.status, FindingStatus::Detected | FindingStatus::Reviewed))
            .collect();
        pending.sort_by(|a, b| {
            b.content_potential
                .total_cmp(&a.content_potential)
                .then(a.id.cmp(&b.id))
        });
        pending
    }

    fn find_mut(&mut self, id: Uuid) -> Result<&mut ContentCandidate, IntelligenceError> {
        self.candidates
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(IntelligenceError::ContentCandidateNotFound(id))
    }

    pub fn review(&mut self, id: Uuid) -> Result<(), IntelligenceError> {
        self.find_mut(id)?.review();
        Ok(())
    }

    /// Explicit operator acceptance (spec section 11/35) - changes only
    /// this candidate's own status; has no way to publish, schedule, or
    /// create a `PresentationItem`.
    pub fn accept(&mut self, id: Uuid) -> Result<(), IntelligenceError> {
        self.find_mut(id)?.accept();
        Ok(())
    }

    pub fn reject(&mut self, id: Uuid) -> Result<(), IntelligenceError> {
        self.find_mut(id)?.reject();
        Ok(())
    }

    pub fn get(&self, id: Uuid) -> Option<&ContentCandidate> {
        self.candidates.iter().find(|c| c.id == id)
    }

    /// Every candidate ever added, regardless of status, oldest first.
    pub fn all(&self) -> Vec<&ContentCandidate> {
        self.candidates.iter().collect()
    }

    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextBounds;
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult as CR, ConfidenceSource as CS};

    fn segment(text: &str, sequence: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence,
            text: text.to_string(),
            is_final: true,
            confidence: CR::new(0.9, CS::Model, None),
            start_ms: sequence * 1000,
            end_ms: sequence * 1000 + 900,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    fn sermon_finding(
        service_id: Uuid,
        summary: &str,
        assertion_level: AssertionLevel,
        segment_ids: Vec<Uuid>,
        excerpt: Option<&str>,
    ) -> IntelligenceFinding {
        let confidence = CR::new(0.85, CS::Heuristic, None);
        let mut finding = IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Sermon,
            FindingKind::Sermon,
            assertion_level,
            confidence,
            summary,
            "sermon-core",
            "0.1.0",
        )
        .with_transcript_segments(segment_ids.clone());
        finding.evidence = if let Some(excerpt) = excerpt {
            vec![EvidenceSource::Transcript {
                segment_ids,
                excerpt: excerpt.to_string(),
            }]
        } else {
            vec![EvidenceSource::Context {
                description: "test evidence".to_string(),
            }]
        };
        finding
    }

    fn context_with(service_id: Uuid, findings: Vec<IntelligenceFinding>) -> IntelligenceContext {
        let seg = segment("placeholder", 0);
        IntelligenceContext::build(
            service_id,
            None,
            Some(seg.clone()),
            vec![seg],
            None,
            findings,
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        )
    }

    fn engine() -> ContentIntelligenceEngine {
        ContentIntelligenceEngine::new()
    }

    // --- mapping tests -------------------------------------------------

    #[test]
    fn theme_finding_maps_to_a_theme_candidate() {
        let service_id = Uuid::new_v4();
        let f = sermon_finding(
            service_id,
            "Theme: faith and obedience",
            AssertionLevel::Inferred,
            vec![Uuid::new_v4()],
            None,
        );
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].candidate_type, ContentCandidateType::Theme);
        assert_eq!(candidates[0].working_concept, "faith and obedience");
    }

    #[test]
    fn main_point_and_sub_point_map_to_teaching() {
        let service_id = Uuid::new_v4();
        let a = sermon_finding(
            service_id,
            "Main Point: faith comes by hearing",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("faith comes by hearing"),
        );
        let b = sermon_finding(
            service_id,
            "Sub-Point: hearing requires attention",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("hearing requires attention"),
        );
        let candidates = engine().analyze(&context_with(service_id, vec![a, b]));
        assert_eq!(candidates.len(), 2);
        assert!(candidates
            .iter()
            .all(|c| c.candidate_type == ContentCandidateType::Teaching));
    }

    #[test]
    fn application_maps_to_reflection() {
        let service_id = Uuid::new_v4();
        let f = sermon_finding(
            service_id,
            "Application: you need to trust God this week",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("you need to trust God this week"),
        );
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert_eq!(
            candidates[0].candidate_type,
            ContentCandidateType::Reflection
        );
    }

    #[test]
    fn takeaway_maps_to_takeaway() {
        let service_id = Uuid::new_v4();
        let f = sermon_finding(
            service_id,
            "Takeaway: God is faithful",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("God is faithful"),
        );
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert_eq!(candidates[0].candidate_type, ContentCandidateType::Takeaway);
    }

    #[test]
    fn food_for_thought_maps_to_food_for_thought() {
        let service_id = Uuid::new_v4();
        let f = sermon_finding(
            service_id,
            "Food for Thought: what are you trusting?",
            AssertionLevel::Inferred,
            vec![Uuid::new_v4()],
            None,
        );
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert_eq!(
            candidates[0].candidate_type,
            ContentCandidateType::FoodForThought
        );
    }

    #[test]
    fn key_statement_maps_to_quote_and_carries_the_verbatim_excerpt() {
        let service_id = Uuid::new_v4();
        let f = sermon_finding(
            service_id,
            "Key Statement: faith is not the absence of uncertainty",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("faith is not the absence of uncertainty"),
        );
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert_eq!(candidates[0].candidate_type, ContentCandidateType::Quote);
        assert_eq!(
            candidates[0].working_concept,
            "faith is not the absence of uncertainty"
        );
    }

    #[test]
    fn question_maps_to_discussion_question() {
        let service_id = Uuid::new_v4();
        let f = sermon_finding(
            service_id,
            "Question: what does this mean for us?",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("what does this mean for us?"),
        );
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert_eq!(
            candidates[0].candidate_type,
            ContentCandidateType::DiscussionQuestion
        );
    }

    #[test]
    fn supporting_scripture_maps_to_scripture_reflection() {
        let service_id = Uuid::new_v4();
        let f = sermon_finding(
            service_id,
            "Supporting Scripture: ROM 10:17",
            AssertionLevel::Inferred,
            vec![Uuid::new_v4()],
            None,
        );
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert_eq!(
            candidates[0].candidate_type,
            ContentCandidateType::ScriptureReflection
        );
        assert_eq!(candidates[0].working_concept, "ROM 10:17");
    }

    #[test]
    fn illustration_story_and_example_all_map_to_illustration() {
        let service_id = Uuid::new_v4();
        let a = sermon_finding(
            service_id,
            "Illustration: imagine a farmer sowing seed",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("imagine a farmer sowing seed"),
        );
        let b = sermon_finding(
            service_id,
            "Story: there was a man who planted a seed",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("there was a man who planted a seed"),
        );
        let c = sermon_finding(
            service_id,
            "Example: for instance, a farmer",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("for instance, a farmer"),
        );
        let candidates = engine().analyze(&context_with(service_id, vec![a, b, c]));
        assert_eq!(candidates.len(), 3);
        assert!(candidates
            .iter()
            .all(|cand| cand.candidate_type == ContentCandidateType::Illustration));
    }

    #[test]
    fn unmapped_summary_prefixes_produce_no_candidate() {
        let service_id = Uuid::new_v4();
        let unmapped = [
            "Definition: faith is not merely believing",
            "Declaration: I declare this house blessed",
            "Prayer Point: let's pray together",
            "Summary: to summarize",
            "Reflection: what would you do?",
            "Transition: INTRODUCTION -> MAIN_POINT",
            "Possible Conclusion: in conclusion",
            "Structural Transition (section): INTRODUCTION -> MAIN_MESSAGE",
            "Sermon foundation: sermon started",
        ];
        let findings: Vec<_> = unmapped
            .iter()
            .map(|s| {
                sermon_finding(
                    service_id,
                    s,
                    AssertionLevel::Observed,
                    vec![Uuid::new_v4()],
                    Some("evidence"),
                )
            })
            .collect();
        let candidates = engine().analyze(&context_with(service_id, findings));
        assert!(candidates.is_empty());
    }

    #[test]
    fn non_sermon_findings_are_never_mapped() {
        let service_id = Uuid::new_v4();
        let mut f = IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            CR::new(0.9, CS::Heuristic, None),
            "Theme: faith",
            "bible",
            "1.0",
        );
        f.evidence = vec![EvidenceSource::Context {
            description: "test".to_string(),
        }];
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert!(candidates.is_empty());
    }

    // --- eligibility tests -----------------------------------------------

    #[test]
    fn rejected_finding_never_becomes_a_candidate() {
        let service_id = Uuid::new_v4();
        let mut f = sermon_finding(
            service_id,
            "Theme: faith",
            AssertionLevel::Inferred,
            vec![Uuid::new_v4()],
            None,
        );
        f.status = FindingStatus::Rejected;
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert!(candidates.is_empty());
    }

    #[test]
    fn expired_finding_never_becomes_a_candidate() {
        let service_id = Uuid::new_v4();
        let mut f = sermon_finding(
            service_id,
            "Theme: faith",
            AssertionLevel::Inferred,
            vec![Uuid::new_v4()],
            None,
        );
        f.status = FindingStatus::Expired;
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert!(candidates.is_empty());
    }

    #[test]
    fn accepted_finding_is_still_eligible() {
        let service_id = Uuid::new_v4();
        let mut f = sermon_finding(
            service_id,
            "Theme: faith",
            AssertionLevel::Inferred,
            vec![Uuid::new_v4()],
            None,
        );
        f.status = FindingStatus::Accepted;
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn missing_evidence_produces_no_candidate() {
        let service_id = Uuid::new_v4();
        let mut f = sermon_finding(
            service_id,
            "Theme: faith",
            AssertionLevel::Inferred,
            vec![Uuid::new_v4()],
            None,
        );
        f.evidence.clear();
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert!(candidates.is_empty());
    }

    #[test]
    fn generated_assertion_level_is_never_eligible() {
        let service_id = Uuid::new_v4();
        let f = sermon_finding(
            service_id,
            "Theme: faith",
            AssertionLevel::Generated,
            vec![Uuid::new_v4()],
            None,
        );
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert!(candidates.is_empty());
    }

    // --- quote integrity ---------------------------------------------------

    #[test]
    fn a_key_statement_with_only_context_evidence_never_becomes_a_quote() {
        let service_id = Uuid::new_v4();
        let f = sermon_finding(
            service_id,
            "Key Statement: never forget this",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            None, // Context evidence only, no verbatim transcript excerpt
        );
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert!(
            candidates.is_empty(),
            "a Quote candidate must never be built without verbatim Transcript evidence"
        );
    }

    // --- assertion-level preservation --------------------------------------

    #[test]
    fn assertion_level_is_inherited_unchanged_and_never_upgraded() {
        let service_id = Uuid::new_v4();
        let observed = sermon_finding(
            service_id,
            "Main Point: faith comes by hearing",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("faith comes by hearing"),
        );
        let inferred = sermon_finding(
            service_id,
            "Theme: faith",
            AssertionLevel::Inferred,
            vec![Uuid::new_v4()],
            None,
        );
        let candidates = engine().analyze(&context_with(service_id, vec![observed, inferred]));
        let teaching = candidates
            .iter()
            .find(|c| c.candidate_type == ContentCandidateType::Teaching)
            .unwrap();
        let theme = candidates
            .iter()
            .find(|c| c.candidate_type == ContentCandidateType::Theme)
            .unwrap();
        assert_eq!(teaching.assertion_level, AssertionLevel::Observed);
        assert_eq!(theme.assertion_level, AssertionLevel::Inferred);
    }

    #[test]
    fn no_candidate_is_ever_generated() {
        let service_id = Uuid::new_v4();
        let f = sermon_finding(
            service_id,
            "Takeaway: God is faithful",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("God is faithful"),
        );
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert!(candidates
            .iter()
            .all(|c| c.assertion_level != AssertionLevel::Generated));
    }

    // --- confidence vs content potential independence -----------------------

    #[test]
    fn content_potential_is_independent_of_confidence() {
        let service_id = Uuid::new_v4();
        // High confidence, low-content-potential type (DiscussionQuestion).
        let mut low_potential_high_confidence = sermon_finding(
            service_id,
            "Question: what does this mean for us?",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("what does this mean for us?"),
        );
        low_potential_high_confidence.confidence = CR::new(0.99, CS::Heuristic, None);

        // Low confidence, high-content-potential type (Quote).
        let mut high_potential_low_confidence = sermon_finding(
            service_id,
            "Key Statement: never forget",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("never forget"),
        );
        high_potential_low_confidence.confidence = CR::new(0.2, CS::Heuristic, None);

        let candidates = engine().analyze(&context_with(
            service_id,
            vec![low_potential_high_confidence, high_potential_low_confidence],
        ));
        let question = candidates
            .iter()
            .find(|c| c.candidate_type == ContentCandidateType::DiscussionQuestion)
            .unwrap();
        let quote = candidates
            .iter()
            .find(|c| c.candidate_type == ContentCandidateType::Quote)
            .unwrap();

        assert!(question.confidence.score > quote.confidence.score);
        assert!(
            quote.content_potential > question.content_potential,
            "a lower-confidence Quote can still outrank a higher-confidence DiscussionQuestion \
             on content potential - the two dimensions are independent"
        );
    }

    // --- traceability --------------------------------------------------

    #[test]
    fn every_candidate_traces_back_to_its_source_finding_id() {
        let service_id = Uuid::new_v4();
        let f = sermon_finding(
            service_id,
            "Theme: faith",
            AssertionLevel::Inferred,
            vec![Uuid::new_v4()],
            None,
        );
        let finding_id = f.id;
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert_eq!(candidates[0].source_finding_ids, vec![finding_id]);
        assert_eq!(candidates[0].evidence.len(), 1);
    }

    #[test]
    fn sermon_id_is_inherited_from_the_source_finding() {
        let service_id = Uuid::new_v4();
        let sermon_id = Uuid::new_v4();
        let mut f = sermon_finding(
            service_id,
            "Takeaway: God is faithful",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("God is faithful"),
        );
        f.sermon_id = Some(sermon_id);
        let candidates = engine().analyze(&context_with(service_id, vec![f]));
        assert_eq!(candidates[0].sermon_id, Some(sermon_id));
    }

    // --- deduplication ---------------------------------------------------

    #[test]
    fn repeated_identical_finding_produces_one_candidate() {
        let service_id = Uuid::new_v4();
        let shared_id = Uuid::new_v4();
        let mut findings = Vec::new();
        for _ in 0..100 {
            let mut f = sermon_finding(
                service_id,
                "Theme: faith",
                AssertionLevel::Inferred,
                vec![Uuid::new_v4()],
                None,
            );
            f.id = shared_id; // same source finding id repeated
            findings.push(f);
        }
        let candidates = engine().analyze(&context_with(service_id, findings));
        assert_eq!(
            candidates.len(),
            1,
            "100 repeats of the identical source finding must never produce 100 candidates"
        );
    }

    // --- determinism / ordering -------------------------------------------

    #[test]
    fn identical_context_produces_identical_candidate_sequences() {
        let service_id = Uuid::new_v4();
        let a = sermon_finding(
            service_id,
            "Theme: faith",
            AssertionLevel::Inferred,
            vec![Uuid::new_v4()],
            None,
        );
        let b = sermon_finding(
            service_id,
            "Key Statement: never forget",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("never forget"),
        );
        let context = context_with(service_id, vec![a, b]);
        let run = || {
            engine()
                .analyze(&context)
                .into_iter()
                .map(|c| (c.candidate_type, c.working_concept, c.content_potential))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn ordering_is_stable_when_content_potential_ties() {
        let service_id = Uuid::new_v4();
        let a = sermon_finding(
            service_id,
            "Main Point: first point",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("first point"),
        );
        let b = sermon_finding(
            service_id,
            "Sub-Point: second point",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("second point"),
        );
        let context = context_with(service_id, vec![a, b]);
        // `ContentCandidate::id` is freshly generated on every `analyze()`
        // call by design (mirrors every other engine's own determinism
        // convention - see `identical_context_produces_identical_candidate_sequences`
        // above), so this compares the deterministic *source* identity
        // (candidate_type + source_finding_ids) each run resolves to,
        // rather than the inherently-fresh generated id.
        let run = || {
            engine()
                .analyze(&context)
                .into_iter()
                .map(|c| (c.candidate_type, c.source_finding_ids))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            run(),
            run(),
            "tied content_potential must still sort to the same order every run"
        );
    }

    // --- empty / bounded input -----------------------------------------

    #[test]
    fn empty_context_produces_no_candidates_and_never_panics() {
        let service_id = Uuid::new_v4();
        let candidates = engine().analyze(&context_with(service_id, Vec::new()));
        assert!(candidates.is_empty());
    }

    #[test]
    fn ten_thousand_findings_never_produce_unbounded_output() {
        let service_id = Uuid::new_v4();
        // context.recent_findings is already bounded to
        // DEFAULT_MAX_RECENT_FINDINGS by IntelligenceContext::build, so
        // even a 10,000-entry input list never reaches analyze() whole.
        let findings: Vec<_> = (0..10_000)
            .map(|i| {
                sermon_finding(
                    service_id,
                    &format!("Theme: concept {i}"),
                    AssertionLevel::Inferred,
                    vec![Uuid::new_v4()],
                    None,
                )
            })
            .collect();
        let context = context_with(service_id, findings);
        assert!(context.recent_findings.len() <= 20);
        let candidates = engine().analyze(&context);
        assert!(candidates.len() <= 20);
    }

    // --- failure isolation (type-level: no engine-to-engine calls) --------

    #[test]
    fn engine_never_depends_on_another_engine_type_level() {
        // Type-level proof: this module's imports carry no reference to
        // BibleIntelligenceEngine/MusicIntelligenceEngine/
        // ServiceIntelligenceEngine/SermonIntelligenceEngine/
        // CrossDomainCorrelationEngine - see this file's own `use`
        // statements. This test exists so a future edit that adds such an
        // import triggers an explicit, documented decision rather than a
        // silent one.
        let engine = ContentIntelligenceEngine::new();
        let _ = engine.analyze(&context_with(Uuid::new_v4(), Vec::new()));
    }

    #[test]
    fn speaker_provenance_flows_through_without_being_invented() {
        // Phase 2.6 attaches speaker attribution as `provenance.note` on
        // the source finding when (and only when) an operator explicitly
        // assigned one. This proves that note is carried forward
        // unchanged - never invented, never dropped - and that a finding
        // with no such note produces a candidate whose note is also
        // `None`.
        let service_id = Uuid::new_v4();
        let mut with_speaker = sermon_finding(
            service_id,
            "Main Point: faith comes by hearing",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("faith comes by hearing"),
        );
        with_speaker.provenance.note = Some("speaker: Pastor Jane Doe (PRIMARY)".to_string());
        let candidates = engine().analyze(&context_with(service_id, vec![with_speaker]));
        assert_eq!(
            candidates[0].provenance.note.as_deref(),
            Some("speaker: Pastor Jane Doe (PRIMARY)")
        );

        let service_id2 = Uuid::new_v4();
        let without_speaker = sermon_finding(
            service_id2,
            "Main Point: faith comes by hearing",
            AssertionLevel::Observed,
            vec![Uuid::new_v4()],
            Some("faith comes by hearing"),
        );
        let candidates2 = engine().analyze(&context_with(service_id2, vec![without_speaker]));
        assert_eq!(
            candidates2[0].provenance.note, None,
            "unknown speaker must remain unknown, never invented"
        );
    }

    // --- ContentCandidateQueue --------------------------------------------

    #[test]
    fn queue_add_queues_a_new_candidate() {
        let mut queue = ContentCandidateQueue::new();
        let c = ContentCandidate::new(
            Uuid::new_v4(),
            None,
            vec![Uuid::new_v4()],
            ContentCandidateType::Theme,
            "Theme: faith",
            "faith",
            AssertionLevel::Inferred,
            CR::new(0.7, CS::Heuristic, None),
            0.5,
            CONTENT_ENGINE_ID,
            CONTENT_ENGINE_VERSION,
        );
        assert_eq!(queue.add(c), ContentCandidateQueueAddOutcome::Added);
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn queue_rejects_a_duplicate_pending_candidate() {
        let mut queue = ContentCandidateQueue::new();
        let service_id = Uuid::new_v4();
        let source_id = Uuid::new_v4();
        let make = || {
            ContentCandidate::new(
                service_id,
                None,
                vec![source_id],
                ContentCandidateType::Theme,
                "Theme: faith",
                "faith",
                AssertionLevel::Inferred,
                CR::new(0.7, CS::Heuristic, None),
                0.5,
                CONTENT_ENGINE_ID,
                CONTENT_ENGINE_VERSION,
            )
        };
        queue.add(make());
        assert_eq!(
            queue.add(make()),
            ContentCandidateQueueAddOutcome::DuplicateIgnored
        );
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn queue_accept_and_reject_change_only_status_and_remove_from_pending() {
        let mut queue = ContentCandidateQueue::new();
        let c = ContentCandidate::new(
            Uuid::new_v4(),
            None,
            vec![Uuid::new_v4()],
            ContentCandidateType::Takeaway,
            "Takeaway: x",
            "x",
            AssertionLevel::Observed,
            CR::new(0.9, CS::Heuristic, None),
            0.6,
            CONTENT_ENGINE_ID,
            CONTENT_ENGINE_VERSION,
        );
        let id = c.id;
        queue.add(c);
        assert_eq!(queue.pending().len(), 1);
        queue.accept(id).unwrap();
        assert_eq!(queue.get(id).unwrap().status, FindingStatus::Accepted);
        assert!(queue.pending().is_empty());

        let mut queue2 = ContentCandidateQueue::new();
        let r = ContentCandidate::new(
            Uuid::new_v4(),
            None,
            vec![Uuid::new_v4()],
            ContentCandidateType::Takeaway,
            "Takeaway: y",
            "y",
            AssertionLevel::Observed,
            CR::new(0.9, CS::Heuristic, None),
            0.6,
            CONTENT_ENGINE_ID,
            CONTENT_ENGINE_VERSION,
        );
        let rid = r.id;
        queue2.add(r);
        queue2.reject(rid).unwrap();
        assert_eq!(queue2.get(rid).unwrap().status, FindingStatus::Rejected);
    }

    /// Phase 3.0: before `list_accepted_content_candidates` existed
    /// (`apps/desktop/src-tauri/src/commands.rs`), an accepted candidate's
    /// text was reachable only through `pending()` (which deliberately
    /// excludes it) - `all()` was the one accessor that could ever surface
    /// it again, but had no test proving it actually still carries the
    /// original working text after acceptance. This is the invariant that
    /// command depends on.
    #[test]
    fn an_accepted_candidate_remains_retrievable_via_all_with_its_text_intact() {
        let mut queue = ContentCandidateQueue::new();
        let c = ContentCandidate::new(
            Uuid::new_v4(),
            None,
            vec![Uuid::new_v4()],
            ContentCandidateType::Quote,
            "Quote: grace upon grace",
            "grace upon grace",
            AssertionLevel::Observed,
            CR::new(0.9, CS::Heuristic, None),
            0.6,
            CONTENT_ENGINE_ID,
            CONTENT_ENGINE_VERSION,
        );
        let id = c.id;
        queue.add(c);
        queue.accept(id).unwrap();

        assert!(
            queue.pending().is_empty(),
            "accepted candidates must leave the pending queue"
        );
        let saved = queue
            .all()
            .into_iter()
            .find(|candidate| candidate.id == id)
            .expect("an accepted candidate must remain findable via all()");
        assert_eq!(saved.status, FindingStatus::Accepted);
        assert_eq!(saved.working_concept, "grace upon grace");
        assert_eq!(saved.title_or_label, "Quote: grace upon grace");
    }

    #[test]
    fn queue_unknown_id_reports_not_found() {
        let mut queue = ContentCandidateQueue::new();
        assert!(matches!(
            queue.accept(Uuid::new_v4()),
            Err(IntelligenceError::ContentCandidateNotFound(_))
        ));
    }

    // --- canonical Phase 2.7 acceptance scenario ---------------------------

    /// The canonical Phase 2.7 acceptance scenario (spec section 37): a
    /// theme finding (already proven real by Phase 2.6) becomes a content
    /// candidate, the operator accepts it, and every invariant the spec
    /// demands holds - traceability, no presentation side effect, no
    /// engine-to-engine call, source finding/transcript untouched.
    #[test]
    fn canonical_phase_2_7_acceptance_scenario() {
        let service_id = Uuid::new_v4();
        let sermon_id = Uuid::new_v4();
        let segment_id = Uuid::new_v4();

        // SERMON INTELLIGENCE already produced this finding (Phase 2.6) -
        // Content Intelligence only ever reads it, never re-derives it.
        let mut theme_finding = sermon_finding(
            service_id,
            "Theme: trusting God during difficulty",
            AssertionLevel::Inferred,
            vec![segment_id],
            None,
        );
        theme_finding.sermon_id = Some(sermon_id);
        let original_finding = theme_finding.clone();

        let context = context_with(service_id, vec![theme_finding]);
        let engine = ContentIntelligenceEngine::new();
        let mut queue = ContentCandidateQueue::new();

        let produced = engine.analyze(&context);
        assert_eq!(produced.len(), 1);
        let mut queued = Vec::new();
        for candidate in produced {
            if queue.add(candidate.clone()) == ContentCandidateQueueAddOutcome::Added {
                queued.push(candidate);
            }
        }
        let candidate = &queued[0];

        // Traceability: service_id/sermon_id/source_finding_ids intact.
        assert_eq!(candidate.service_id, service_id);
        assert_eq!(candidate.sermon_id, Some(sermon_id));
        assert_eq!(
            candidate.source_finding_ids,
            vec![context.recent_findings[0].id]
        );
        assert_eq!(candidate.candidate_type, ContentCandidateType::Theme);

        // Assertion level inherited, never upgraded to Generated.
        assert_eq!(candidate.assertion_level, AssertionLevel::Inferred);

        // OPERATOR REVIEW -> ACCEPT.
        queue.accept(candidate.id).unwrap();
        let accepted = queue.get(candidate.id).unwrap();
        assert_eq!(accepted.status, FindingStatus::Accepted);

        // Source finding is never mutated by any of this.
        assert_eq!(context.recent_findings[0], original_finding);

        // NO presentation, NO publication: type-level, this module has no
        // dependency on `cip_core_presentation` at all - nothing here
        // could construct a `PresentationItem` even if it wanted to.
    }
}
