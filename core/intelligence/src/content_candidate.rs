//! [`ContentCandidate`]: the foundation for Content Intelligence (Phase
//! 2.7, per the authoritative Phase 2 roadmap). A typed record that an
//! already-detected [`crate::IntelligenceFinding`] appears suitable as a
//! future content opportunity - never a second detection, never final
//! copy.
//!
//! Deliberately its own type, not folded into [`crate::IntelligenceFinding`],
//! mirroring [`crate::IntelligenceCorrelation`]'s own precedent exactly
//! (see `correlation.rs`'s module docs): a candidate's meaning - "this
//! detected information appears suitable for a particular future content
//! purpose" - is structurally different from a finding's meaning - "this
//! was detected." See `docs/content-intelligence.md`.
//!
//! ## Confidence vs. content potential (spec rules 9/10/21)
//!
//! [`ContentCandidate::confidence`] reuses [`cip_core_confidence::ConfidenceResult`]
//! unchanged - it is always inherited from the source finding, never
//! recomputed, and it still means exactly what it always has ("how
//! certain is this fact"). [`ContentCandidate::content_potential`] is a
//! deliberately separate `f32` dimension ("how suitable does this appear
//! as a future content opportunity") - see `content_intelligence.rs`'s
//! `content_potential_for` for the deterministic, confidence-independent
//! formula. A highly-confident finding does not automatically score high
//! content potential, and vice versa; tests in `content_intelligence.rs`
//! prove the two dimensions vary independently.

use chrono::{DateTime, Utc};
use cip_core_confidence::ConfidenceResult;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{AssertionLevel, FindingStatus};
use crate::evidence::{EvidenceSource, IntelligenceProvenance};

/// A closed, minimal taxonomy of future content opportunity types -
/// justified entirely by real Phase 2.6 Sermon Intelligence finding
/// categories (spec section 33's own recommended initial mapping), not
/// implemented speculatively. See `content_intelligence.rs`'s
/// `candidate_type_for_finding` for the exact, documented source mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentCandidateType {
    /// From a Sermon `Theme:` finding.
    Theme,
    /// From a Sermon `Main Point:`/`Sub-Point:` finding.
    Teaching,
    /// From a Sermon `Application:` finding.
    Reflection,
    /// From a Sermon `Takeaway:` finding.
    Takeaway,
    /// From a Sermon `Food for Thought:` finding.
    FoodForThought,
    /// From a Sermon `Key Statement:` finding - the only type held to the
    /// verbatim-quote integrity rule (spec section 14); see
    /// `content_intelligence.rs`'s eligibility filter.
    Quote,
    /// From a Sermon `Question:` finding.
    DiscussionQuestion,
    /// From a Sermon `Supporting Scripture:` cross-link finding - never
    /// re-derived from Scripture text directly (spec section 15); the
    /// reference/translation stays exactly what the source finding
    /// already carried.
    ScriptureReflection,
    /// From a Sermon `Illustration:`/`Story:`/`Example:` finding.
    Illustration,
}

impl ContentCandidateType {
    /// SCREAMING_SNAKE_CASE label, matching this codebase's established
    /// `CorrelationKind::label`/`SermonElementKind::label` convention -
    /// used for deterministic sort tiebreaks and human-readable display.
    pub const fn label(self) -> &'static str {
        match self {
            ContentCandidateType::Theme => "THEME",
            ContentCandidateType::Teaching => "TEACHING",
            ContentCandidateType::Reflection => "REFLECTION",
            ContentCandidateType::Takeaway => "TAKEAWAY",
            ContentCandidateType::FoodForThought => "FOOD_FOR_THOUGHT",
            ContentCandidateType::Quote => "QUOTE",
            ContentCandidateType::DiscussionQuestion => "DISCUSSION_QUESTION",
            ContentCandidateType::ScriptureReflection => "SCRIPTURE_REFLECTION",
            ContentCandidateType::Illustration => "ILLUSTRATION",
        }
    }
}

/// A future content opportunity CIP has structured from an already-proven
/// finding - never final copy (spec rule 2). `title_or_label`/
/// `working_concept` are deterministic, unpolished labels/paraphrase
/// scaffolding, never marketing prose - see `content_intelligence.rs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentCandidate {
    pub id: Uuid,
    pub service_id: Uuid,
    /// The active `Sermon`'s id (Phase 2.5 foundation) this candidate was
    /// derived under, if any - `None` when the source finding carried no
    /// `sermon_id`. Never fabricated.
    pub sermon_id: Option<Uuid>,
    /// The finding(s) this candidate was derived from - always at least
    /// one (spec section 12: "do not create candidates that cannot
    /// explain their source").
    pub source_finding_ids: Vec<Uuid>,
    pub candidate_type: ContentCandidateType,
    /// A short, deterministic working label (e.g. `"Theme: faith"`) -
    /// never polished marketing copy (spec section 13).
    pub title_or_label: String,
    /// A short, deterministic working concept derived from the source
    /// finding's own summary - for `Quote` candidates, the exact verbatim
    /// transcript excerpt; for every other type, the source finding's own
    /// summary text, never paraphrased into new prose.
    pub working_concept: String,
    /// Inherited from the source finding - never upgraded to look more
    /// certain merely because a finding becomes a candidate (spec rule 8).
    pub assertion_level: AssertionLevel,
    /// Reused [`FindingStatus`] exactly - a candidate lifecycle distinct
    /// from the source finding's own status but never a second enum (spec
    /// section 11).
    pub status: FindingStatus,
    /// Reused unchanged from the source finding - see this module's own
    /// docs on why this is never recomputed or replaced.
    pub confidence: ConfidenceResult,
    /// A separate 0.0-1.0 dimension: "how suitable this finding appears as
    /// a future content opportunity" - explicitly NOT a truth-confidence
    /// score (spec rules 9/10/21). See `content_intelligence.rs`'s
    /// `content_potential_for`.
    pub content_potential: f32,
    pub evidence: Vec<EvidenceSource>,
    pub provenance: IntelligenceProvenance,
    pub engine_id: String,
    pub engine_version: String,
    pub created_at: DateTime<Utc>,
}

impl ContentCandidate {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        service_id: Uuid,
        sermon_id: Option<Uuid>,
        source_finding_ids: Vec<Uuid>,
        candidate_type: ContentCandidateType,
        title_or_label: impl Into<String>,
        working_concept: impl Into<String>,
        assertion_level: AssertionLevel,
        confidence: ConfidenceResult,
        content_potential: f32,
        engine_id: impl Into<String>,
        engine_version: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            service_id,
            sermon_id,
            source_finding_ids,
            candidate_type,
            title_or_label: title_or_label.into(),
            working_concept: working_concept.into(),
            assertion_level,
            status: FindingStatus::Detected,
            confidence,
            content_potential: content_potential.clamp(0.0, 1.0),
            evidence: Vec::new(),
            provenance: IntelligenceProvenance::unknown(),
            engine_id: engine_id.into(),
            engine_version: engine_version.into(),
            created_at: Utc::now(),
        }
    }

    pub fn with_evidence(mut self, evidence: Vec<EvidenceSource>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn with_provenance(mut self, provenance: IntelligenceProvenance) -> Self {
        self.provenance = provenance;
        self
    }

    pub fn review(&mut self) {
        if self.status == FindingStatus::Detected {
            self.status = FindingStatus::Reviewed;
        }
    }

    /// Explicit operator acceptance of the content *opportunity* (spec
    /// section 11/35) - never publishes, schedules, or creates a
    /// `PresentationItem`; this changes only `status`, exactly like
    /// `IntelligenceFinding::accept`.
    pub fn accept(&mut self) {
        self.status = FindingStatus::Accepted;
    }

    pub fn reject(&mut self) {
        self.status = FindingStatus::Rejected;
    }

    /// Deterministic duplicate-equivalence rule, mirroring
    /// [`crate::IntelligenceCorrelation::is_equivalent_to`] exactly: same
    /// service, same candidate type, same *set* of source finding ids
    /// (order-independent) is the same candidate, regardless of id/
    /// timestamp/confidence/content_potential.
    pub fn is_equivalent_to(&self, other: &ContentCandidate) -> bool {
        if self.service_id != other.service_id || self.candidate_type != other.candidate_type {
            return false;
        }
        let mut a = self.source_finding_ids.clone();
        let mut b = other.source_finding_ids.clone();
        a.sort_unstable();
        b.sort_unstable();
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_confidence::ConfidenceSource;

    fn confidence() -> ConfidenceResult {
        ConfidenceResult::new(0.9, ConfidenceSource::Heuristic, None)
    }

    fn candidate(
        service_id: Uuid,
        candidate_type: ContentCandidateType,
        source_ids: Vec<Uuid>,
    ) -> ContentCandidate {
        ContentCandidate::new(
            service_id,
            None,
            source_ids,
            candidate_type,
            "Theme: faith",
            "faith",
            AssertionLevel::Inferred,
            confidence(),
            0.5,
            "content-intelligence",
            "0.1.0",
        )
    }

    #[test]
    fn new_candidate_starts_detected_with_unknown_provenance() {
        let c = candidate(
            Uuid::new_v4(),
            ContentCandidateType::Theme,
            vec![Uuid::new_v4()],
        );
        assert_eq!(c.status, FindingStatus::Detected);
        assert_eq!(c.provenance.content_id, None);
    }

    #[test]
    fn content_potential_is_clamped_to_zero_one() {
        let mut c = candidate(
            Uuid::new_v4(),
            ContentCandidateType::Theme,
            vec![Uuid::new_v4()],
        );
        c.content_potential = 5.0;
        // clamp happens in `new`, so construct directly to prove the
        // constructor itself clamps out-of-range input.
        let clamped = ContentCandidate::new(
            c.service_id,
            None,
            c.source_finding_ids.clone(),
            c.candidate_type,
            "x",
            "y",
            AssertionLevel::Inferred,
            confidence(),
            5.0,
            "content-intelligence",
            "0.1.0",
        );
        assert_eq!(clamped.content_potential, 1.0);
        let negative = ContentCandidate::new(
            c.service_id,
            None,
            c.source_finding_ids,
            c.candidate_type,
            "x",
            "y",
            AssertionLevel::Inferred,
            confidence(),
            -5.0,
            "content-intelligence",
            "0.1.0",
        );
        assert_eq!(negative.content_potential, 0.0);
    }

    #[test]
    fn accept_and_reject_change_only_status() {
        let mut c = candidate(
            Uuid::new_v4(),
            ContentCandidateType::Takeaway,
            vec![Uuid::new_v4()],
        );
        let concept_before = c.working_concept.clone();
        c.accept();
        assert_eq!(c.status, FindingStatus::Accepted);
        assert_eq!(c.working_concept, concept_before);

        let mut r = candidate(
            Uuid::new_v4(),
            ContentCandidateType::Takeaway,
            vec![Uuid::new_v4()],
        );
        r.reject();
        assert_eq!(r.status, FindingStatus::Rejected);
    }

    #[test]
    fn review_moves_detected_to_reviewed_and_never_resurrects_a_rejected_candidate() {
        let mut c = candidate(
            Uuid::new_v4(),
            ContentCandidateType::Quote,
            vec![Uuid::new_v4()],
        );
        c.review();
        assert_eq!(c.status, FindingStatus::Reviewed);
        c.reject();
        c.review();
        assert_eq!(c.status, FindingStatus::Rejected);
    }

    #[test]
    fn equivalence_ignores_id_timestamp_confidence_content_potential_and_source_id_order() {
        let service_id = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let mut x = candidate(service_id, ContentCandidateType::Theme, vec![a, b]);
        x.content_potential = 0.9;
        let mut y = candidate(service_id, ContentCandidateType::Theme, vec![b, a]);
        y.content_potential = 0.1;
        assert!(x.is_equivalent_to(&y));
    }

    #[test]
    fn different_type_or_different_service_is_never_equivalent() {
        let a = Uuid::new_v4();
        let x = candidate(Uuid::new_v4(), ContentCandidateType::Theme, vec![a]);
        let y = candidate(Uuid::new_v4(), ContentCandidateType::Theme, vec![a]);
        assert!(
            !x.is_equivalent_to(&y),
            "different service_id is never equivalent"
        );

        let service_id = Uuid::new_v4();
        let p = candidate(service_id, ContentCandidateType::Theme, vec![a]);
        let q = candidate(service_id, ContentCandidateType::Takeaway, vec![a]);
        assert!(
            !p.is_equivalent_to(&q),
            "different candidate_type is never equivalent"
        );
    }

    #[test]
    fn candidate_serializes_with_camel_case_fields_and_snake_case_type() {
        let c = candidate(
            Uuid::new_v4(),
            ContentCandidateType::ScriptureReflection,
            vec![Uuid::new_v4()],
        );
        let json = serde_json::to_value(&c).unwrap();
        assert!(json.get("sourceFindingIds").is_some());
        assert!(json.get("titleOrLabel").is_some());
        assert!(json.get("workingConcept").is_some());
        assert!(json.get("contentPotential").is_some());
        assert!(json.get("assertionLevel").is_some());
        assert!(
            json.get("sermonId").is_some(),
            "present as null, not omitted"
        );
        assert_eq!(json["candidateType"], "scripture_reflection");
    }

    #[test]
    fn type_labels_are_screaming_snake_case_and_distinct() {
        let all = [
            ContentCandidateType::Theme,
            ContentCandidateType::Teaching,
            ContentCandidateType::Reflection,
            ContentCandidateType::Takeaway,
            ContentCandidateType::FoodForThought,
            ContentCandidateType::Quote,
            ContentCandidateType::DiscussionQuestion,
            ContentCandidateType::ScriptureReflection,
            ContentCandidateType::Illustration,
        ];
        let mut labels: Vec<&str> = all.iter().map(|t| t.label()).collect();
        let before = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), before);
        for label in labels {
            assert_eq!(label, label.to_uppercase());
        }
    }
}
