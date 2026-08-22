//! [`IntelligenceFinding`]: the one thing every intelligence engine
//! produces. Everything else in this crate exists to build, bound, or
//! route these.

use crate::domain::{
    AssertionLevel, FindingKind, FindingStatus, IntelligenceDomain, IntelligencePriority,
};
use crate::evidence::{EvidenceSource, IntelligenceProvenance};
use chrono::{DateTime, Utc};
use cip_core_confidence::ConfidenceResult;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Something an intelligence engine has detected, inferred, or suggested -
/// never something it has decided or acted on. See the module docs on
/// [`crate::domain::AssertionLevel`] for the epistemic-state distinction
/// every finding must honestly represent, and
/// [`FindingStatus`](crate::domain::FindingStatus) for the human-review
/// lifecycle this struct's `status` field moves through.
///
/// Deliberately not coupled to SQLite or any storage row type - this is a
/// pure domain value, constructed and returned by engines, never read back
/// from a database table in Phase 2.0 (see `docs/intelligence-architecture.md`'s
/// persistence-decision section).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceFinding {
    pub id: Uuid,
    pub service_id: Uuid,
    pub domain: IntelligenceDomain,
    pub kind: FindingKind,
    pub assertion_level: AssertionLevel,
    pub status: FindingStatus,
    pub priority: IntelligencePriority,
    pub confidence: ConfidenceResult,
    /// Short human-readable description (e.g. `"ROM 8:28"`,
    /// `"Active Scripture Context: ROM 8"`) - domain-specific detail beyond
    /// this belongs in `evidence`, not invented fields on this struct.
    pub summary: String,
    /// The transcript segment(s) this finding traces back to, if any -
    /// support for multiple, since a finding can depend on more than one
    /// segment (Phase 2.0 spec section 17).
    pub transcript_segment_ids: Vec<Uuid>,
    /// The `Sermon` (Phase 2.5 foundation) this finding was produced while
    /// active, if any - `None` both when no sermon is active and for every
    /// domain besides Sermon, which never has one to attach (Phase 2.6
    /// spec section 58, "Phase 2.8 handoff": correlation needs a stable
    /// sermon id on every sermon-domain finding). Never guessed - set only
    /// from `IntelligenceContext::active_sermon`, exactly the same
    /// "unknown stays unknown" discipline `provenance` already follows.
    pub sermon_id: Option<Uuid>,
    pub evidence: Vec<EvidenceSource>,
    pub provenance: IntelligenceProvenance,
    pub engine_id: String,
    pub engine_version: String,
    pub created_at: DateTime<Utc>,
}

impl IntelligenceFinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        service_id: Uuid,
        domain: IntelligenceDomain,
        kind: FindingKind,
        assertion_level: AssertionLevel,
        confidence: ConfidenceResult,
        summary: impl Into<String>,
        engine_id: impl Into<String>,
        engine_version: impl Into<String>,
    ) -> Self {
        let priority = crate::domain::baseline_priority(kind, &confidence);
        Self {
            id: Uuid::new_v4(),
            service_id,
            domain,
            kind,
            assertion_level,
            status: FindingStatus::Detected,
            priority,
            confidence,
            summary: summary.into(),
            transcript_segment_ids: Vec::new(),
            sermon_id: None,
            evidence: Vec::new(),
            provenance: IntelligenceProvenance::unknown(),
            engine_id: engine_id.into(),
            engine_version: engine_version.into(),
            created_at: Utc::now(),
        }
    }

    pub fn with_transcript_segments(mut self, ids: Vec<Uuid>) -> Self {
        self.transcript_segment_ids = ids;
        self
    }

    /// Attach the active `Sermon`'s id (Phase 2.5 foundation) this finding
    /// was produced under - additive, exactly like `with_evidence`/
    /// `with_provenance`; never a required constructor argument, so every
    /// non-Sermon engine's existing `IntelligenceFinding::new(...)` call
    /// remains valid, unmodified source.
    pub fn with_sermon_id(mut self, sermon_id: Uuid) -> Self {
        self.sermon_id = Some(sermon_id);
        self
    }

    pub fn with_evidence(mut self, evidence: Vec<EvidenceSource>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn with_provenance(mut self, provenance: IntelligenceProvenance) -> Self {
        self.provenance = provenance;
        self
    }

    /// Operator has looked at this finding, but not yet accepted or
    /// rejected it. A no-op (not an error) if already past `Detected` -
    /// review is informational, not a strict gate.
    pub fn review(&mut self) {
        if self.status == FindingStatus::Detected {
            self.status = FindingStatus::Reviewed;
        }
    }

    /// Explicit human acceptance. Never called automatically - see Phase
    /// 2.0 spec section 36 ("no automatic actions"). This changes only
    /// `status`; it has no way to create a `PresentationItem` or any other
    /// side effect - that composition, if any, is the caller's job.
    pub fn accept(&mut self) {
        self.status = FindingStatus::Accepted;
    }

    pub fn reject(&mut self) {
        self.status = FindingStatus::Rejected;
    }

    pub fn expire(&mut self) {
        if matches!(
            self.status,
            FindingStatus::Detected | FindingStatus::Reviewed
        ) {
            self.status = FindingStatus::Expired;
        }
    }

    /// Deterministic duplicate-equivalence rule (Phase 2.0 spec section
    /// 32): two findings from the same service, domain, kind, and summary
    /// are considered the same finding, regardless of id/timestamp/
    /// confidence. Bounded and simple by design - not a fuzzy-matching
    /// system.
    pub fn is_equivalent_to(&self, other: &IntelligenceFinding) -> bool {
        self.service_id == other.service_id
            && self.domain == other.domain
            && self.kind == other.kind
            && self.summary == other.summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_confidence::ConfidenceSource;

    fn confidence() -> ConfidenceResult {
        ConfidenceResult::new(0.9, ConfidenceSource::Heuristic, None)
    }

    #[test]
    fn new_findings_always_start_detected() {
        let finding = IntelligenceFinding::new(
            Uuid::new_v4(),
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            confidence(),
            "ROM 8:28",
            "bible",
            "1.0",
        );
        assert_eq!(finding.status, FindingStatus::Detected);
        assert_eq!(
            finding.provenance.content_id, None,
            "unknown provenance until set"
        );
    }

    #[test]
    fn lifecycle_moves_through_review_then_accept() {
        let mut finding = IntelligenceFinding::new(
            Uuid::new_v4(),
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            confidence(),
            "ROM 8:28",
            "bible",
            "1.0",
        );
        finding.review();
        assert_eq!(finding.status, FindingStatus::Reviewed);
        finding.accept();
        assert_eq!(finding.status, FindingStatus::Accepted);
    }

    #[test]
    fn accepting_a_finding_has_no_presentation_side_effect() {
        // Type-level proof: `IntelligenceFinding` has no field capable of
        // representing a `PresentationItem` or a "prepared"/"active"
        // state - accepting it can only ever change `status` on this
        // struct.
        let mut finding = IntelligenceFinding::new(
            Uuid::new_v4(),
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            confidence(),
            "ROM 8:28",
            "bible",
            "1.0",
        );
        finding.accept();
        assert_eq!(finding.status, FindingStatus::Accepted);
    }

    #[test]
    fn expire_only_applies_from_detected_or_reviewed() {
        let mut accepted = IntelligenceFinding::new(
            Uuid::new_v4(),
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            confidence(),
            "ROM 8:28",
            "bible",
            "1.0",
        );
        accepted.accept();
        accepted.expire();
        assert_eq!(
            accepted.status,
            FindingStatus::Accepted,
            "an accepted finding is never silently expired"
        );
    }

    #[test]
    fn equivalence_ignores_id_timestamp_and_confidence() {
        let service_id = Uuid::new_v4();
        let a = IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            confidence(),
            "ROM 8:28",
            "bible",
            "1.0",
        );
        let b = IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            ConfidenceResult::new(0.4, ConfidenceSource::Heuristic, None),
            "ROM 8:28",
            "bible",
            "1.0",
        );
        assert!(a.is_equivalent_to(&b));
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn different_summary_is_not_equivalent() {
        let service_id = Uuid::new_v4();
        let a = IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            confidence(),
            "ROM 8:28",
            "bible",
            "1.0",
        );
        let b = IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            confidence(),
            "ROM 8:31",
            "bible",
            "1.0",
        );
        assert!(!a.is_equivalent_to(&b));
    }

    #[test]
    fn sermon_id_is_none_until_explicitly_attached() {
        let finding = IntelligenceFinding::new(
            Uuid::new_v4(),
            IntelligenceDomain::Sermon,
            FindingKind::Sermon,
            AssertionLevel::Observed,
            confidence(),
            "Main Point: faith comes by hearing",
            "sermon-core",
            "0.1.0",
        );
        assert_eq!(finding.sermon_id, None);

        let sermon_id = Uuid::new_v4();
        let with_sermon = finding.with_sermon_id(sermon_id);
        assert_eq!(with_sermon.sermon_id, Some(sermon_id));
    }

    #[test]
    fn finding_serializes_with_camel_case_fields() {
        let finding = IntelligenceFinding::new(
            Uuid::new_v4(),
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            confidence(),
            "ROM 8:28",
            "bible",
            "1.0",
        );
        let json = serde_json::to_value(&finding).unwrap();
        assert!(json.get("assertionLevel").is_some());
        assert!(json.get("engineVersion").is_some());
        assert!(json.get("serviceId").is_some());
        assert!(
            json.get("sermonId").is_some(),
            "present as null, not omitted"
        );
        assert!(json["sermonId"].is_null());
    }
}
