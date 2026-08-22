//! [`IntelligenceCorrelation`]: the foundation for cross-domain
//! correlation (Phase 2.0 spec section 30, extended in Phase 2.4). A
//! typed record that two or more findings are related, with its own
//! evidence/confidence/provenance - never a graph structure, never a
//! second confidence system, never a second finding-status enum: the
//! lifecycle reuses [`crate::domain::FindingStatus`] exactly (see
//! [`IntelligenceCorrelation::review`]/[`IntelligenceCorrelation::dismiss`]),
//! matching this crate's "reuse, don't duplicate" discipline.
//!
//! Correlations are deliberately their own type, not folded into
//! [`crate::IntelligenceFinding`] (Phase 2.4 spec section 18): a
//! correlation's meaning - "these findings are related, here is why" - is
//! structurally different from a finding's meaning - "this was detected."
//! See `docs/cross-domain-intelligence.md` for the rule engine that
//! produces these.

use chrono::{DateTime, Utc};
use cip_core_confidence::ConfidenceResult;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{AssertionLevel, FindingStatus, IntelligenceDomain};
use crate::evidence::EvidenceSource;

/// What kind of relationship a correlation asserts between its source
/// findings. Closed, deterministic taxonomy (Phase 2.4 spec section 5) -
/// `Other(String)` keeps this extensible without a schema change, matching
/// `cip_core_ai::SuggestionKind`'s `Other` escape hatch. `TemporalProximity`
/// (Phase 2.0) is the same concept the Phase 2.4 spec calls
/// "TemporalAssociation" - reused under its original name rather than
/// adding a duplicate variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum CorrelationKind {
    /// The source findings occurred near the same point in the transcript,
    /// with no stronger evidence connecting them - always low confidence
    /// (Phase 2.4 spec section 12: "must not be promoted automatically to
    /// a strong semantic correlation").
    TemporalProximity,
    /// The source findings were produced against the same active context.
    SharedContext,
    /// A Sermon finding names the same Scripture reference (or chapter)
    /// as a Bible finding.
    ScriptureSermon,
    /// A Bible finding and a Music finding share a transcript segment -
    /// the only evidence strong enough to justify this kind (spec section
    /// 11's conservatism requirement).
    ScriptureMusic,
    /// A Sermon finding (typically a transition/conclusion/prayer signal)
    /// and a Music finding occur close together in the transcript.
    SermonMusic,
    /// A Sermon theme candidate and a Bible finding occur close together
    /// in the transcript.
    ThemeScripture,
    /// A Sermon theme candidate and a Music finding occur close together
    /// in the transcript - temporal proximity only, no semantic/lyric
    /// matching (see `docs/cross-domain-intelligence.md`'s NOT AVAILABLE
    /// section).
    ThemeMusic,
    /// A sermon conclusion/transition signal coincides with a
    /// service-lifecycle event (e.g. the service pausing/ending, or the
    /// derived sermon state itself changing).
    ServiceTransition,
    Other(String),
}

impl CorrelationKind {
    /// SCREAMING_SNAKE_CASE label, matching this codebase's established
    /// `AppEvent::name`/`SermonElementKind::label` convention - used for
    /// deterministic sort tiebreaks and human-readable display.
    pub fn label(&self) -> &'static str {
        match self {
            CorrelationKind::TemporalProximity => "TEMPORAL_PROXIMITY",
            CorrelationKind::SharedContext => "SHARED_CONTEXT",
            CorrelationKind::ScriptureSermon => "SCRIPTURE_SERMON",
            CorrelationKind::ScriptureMusic => "SCRIPTURE_MUSIC",
            CorrelationKind::SermonMusic => "SERMON_MUSIC",
            CorrelationKind::ThemeScripture => "THEME_SCRIPTURE",
            CorrelationKind::ThemeMusic => "THEME_MUSIC",
            CorrelationKind::ServiceTransition => "SERVICE_TRANSITION",
            CorrelationKind::Other(_) => "OTHER",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceCorrelation {
    pub id: Uuid,
    pub service_id: Uuid,
    pub source_finding_ids: Vec<Uuid>,
    /// Every [`IntelligenceDomain`] represented among `source_finding_ids`,
    /// letting a caller filter/display "what domains does this connect"
    /// without re-deriving it from the source findings themselves.
    pub domains: Vec<IntelligenceDomain>,
    pub kind: CorrelationKind,
    /// Always `Inferred` in Phase 2.4 - a correlation is always CIP's own
    /// derived judgment that two findings relate, never a verbatim
    /// observation and never generated content. See
    /// `docs/cross-domain-intelligence.md`'s assertion-level section.
    pub assertion_level: AssertionLevel,
    /// Operator-review lifecycle, reusing [`FindingStatus`] exactly - a
    /// correlation starts `Detected`, may move to `Reviewed`, and is
    /// `Rejected` when an operator dismisses it. `Accepted`/`Expired`
    /// are part of the reused enum but not driven by anything in Phase
    /// 2.4 (see `review`/`dismiss`).
    pub status: FindingStatus,
    pub confidence: ConfidenceResult,
    /// Short human-readable description of the relationship (e.g. "Sermon
    /// references ROM 10:17, matching Bible finding ROM 10:17") - never
    /// a vague claim like "these things seem related."
    pub summary: String,
    pub evidence: Vec<EvidenceSource>,
    /// Deterministic identity of the rule that produced this correlation
    /// (e.g. `"scripture_sermon_v1"`) - Phase 2.4 spec section 34/35's
    /// provenance/rule-versioning requirement. A plain string constant per
    /// rule, not a plugin system.
    pub rule_id: String,
    pub rule_version: String,
    pub created_at: DateTime<Utc>,
}

impl IntelligenceCorrelation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        service_id: Uuid,
        source_finding_ids: Vec<Uuid>,
        domains: Vec<IntelligenceDomain>,
        kind: CorrelationKind,
        assertion_level: AssertionLevel,
        confidence: ConfidenceResult,
        summary: impl Into<String>,
        rule_id: impl Into<String>,
        rule_version: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            service_id,
            source_finding_ids,
            domains,
            kind,
            assertion_level,
            status: FindingStatus::Detected,
            confidence,
            summary: summary.into(),
            evidence: Vec::new(),
            rule_id: rule_id.into(),
            rule_version: rule_version.into(),
            created_at: Utc::now(),
        }
    }

    pub fn with_evidence(mut self, evidence: Vec<EvidenceSource>) -> Self {
        self.evidence = evidence;
        self
    }

    /// Operator has looked at this correlation but not dismissed it - a
    /// no-op if already past `Detected`, mirroring
    /// `IntelligenceFinding::review`'s informational-only semantics.
    pub fn review(&mut self) {
        if self.status == FindingStatus::Detected {
            self.status = FindingStatus::Reviewed;
        }
    }

    /// Explicit operator dismissal (spec section 25's "dismiss
    /// correlation" action) - never automatic. This changes only
    /// `status`; it has no way to alter the source findings, the
    /// transcript, or the active Scripture context.
    pub fn dismiss(&mut self) {
        self.status = FindingStatus::Rejected;
    }

    /// Deterministic duplicate-equivalence rule (Phase 2.4 spec section
    /// 16): two correlations for the same service, same kind, and the
    /// same *set* of source finding ids (order-independent) are the same
    /// correlation, regardless of id/timestamp/confidence/evidence.
    pub fn is_equivalent_to(&self, other: &IntelligenceCorrelation) -> bool {
        if self.service_id != other.service_id || self.kind != other.kind {
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

    fn correlation(kind: CorrelationKind, ids: Vec<Uuid>) -> IntelligenceCorrelation {
        IntelligenceCorrelation::new(
            Uuid::new_v4(),
            ids,
            vec![IntelligenceDomain::Bible, IntelligenceDomain::Sermon],
            kind,
            AssertionLevel::Inferred,
            ConfidenceResult::new(0.7, ConfidenceSource::Heuristic, None),
            "test correlation",
            "test_rule_v1",
            "1.0",
        )
    }

    #[test]
    fn correlation_carries_every_source_finding_id() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = correlation(CorrelationKind::TemporalProximity, vec![a, b]);
        assert_eq!(c.source_finding_ids, vec![a, b]);
        assert_eq!(c.status, FindingStatus::Detected);
    }

    #[test]
    fn correlation_serializes_with_camel_case_fields() {
        let c = IntelligenceCorrelation::new(
            Uuid::new_v4(),
            vec![Uuid::new_v4()],
            vec![IntelligenceDomain::Music],
            CorrelationKind::Other("custom".to_string()),
            AssertionLevel::Inferred,
            ConfidenceResult::new(0.5, ConfidenceSource::Heuristic, None),
            "summary",
            "rule_v1",
            "1.0",
        );
        let json = serde_json::to_value(&c).unwrap();
        assert!(json.get("sourceFindingIds").is_some());
        assert!(json.get("ruleId").is_some());
        assert!(json.get("assertionLevel").is_some());
        assert_eq!(json["kind"]["kind"], "other");
        assert_eq!(json["kind"]["detail"], "custom");
    }

    #[test]
    fn review_moves_detected_to_reviewed_and_is_a_no_op_afterward() {
        let mut c = correlation(CorrelationKind::ScriptureSermon, vec![Uuid::new_v4()]);
        c.review();
        assert_eq!(c.status, FindingStatus::Reviewed);
        c.dismiss();
        c.review();
        assert_eq!(
            c.status,
            FindingStatus::Rejected,
            "review must never resurrect a dismissed correlation"
        );
    }

    #[test]
    fn dismiss_never_alters_source_finding_ids_or_evidence() {
        let ids = vec![Uuid::new_v4(), Uuid::new_v4()];
        let mut c = correlation(CorrelationKind::SermonMusic, ids.clone());
        c.dismiss();
        assert_eq!(c.status, FindingStatus::Rejected);
        assert_eq!(
            c.source_finding_ids, ids,
            "dismissal never rewrites provenance"
        );
    }

    #[test]
    fn equivalence_ignores_id_timestamp_confidence_and_ordering_of_source_ids() {
        let service_id = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let mut x = correlation(CorrelationKind::ScriptureSermon, vec![a, b]);
        x.service_id = service_id;
        let mut y = correlation(CorrelationKind::ScriptureSermon, vec![b, a]);
        y.service_id = service_id;
        assert!(
            x.is_equivalent_to(&y),
            "same kind + same finding-id set (any order) is the same correlation"
        );
    }

    #[test]
    fn different_kind_or_different_service_is_never_equivalent() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let x = correlation(CorrelationKind::ScriptureSermon, vec![a, b]);
        let y = correlation(CorrelationKind::SermonMusic, vec![a, b]);
        assert!(!x.is_equivalent_to(&y));
    }
}
