//! [`IntelligenceCorrelation`]: the foundation for cross-domain
//! correlation (Phase 2.0 spec section 30). Deliberately minimal - no
//! graph algorithms, just a typed record that two or more findings are
//! related, with its own evidence/confidence, ready for a future
//! `CrossDomainEngine` to construct.

use chrono::{DateTime, Utc};
use cip_core_confidence::ConfidenceResult;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::evidence::EvidenceSource;

/// What kind of relationship a correlation asserts between its source
/// findings. `Other(String)` keeps this extensible without a schema
/// change, matching `cip_core_ai::SuggestionKind`'s `Other` escape hatch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum CorrelationKind {
    /// The source findings occurred near the same point in the transcript.
    TemporalProximity,
    /// The source findings were produced against the same active context.
    SharedContext,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceCorrelation {
    pub id: Uuid,
    pub source_finding_ids: Vec<Uuid>,
    pub kind: CorrelationKind,
    pub confidence: ConfidenceResult,
    pub evidence: Vec<EvidenceSource>,
    pub created_at: DateTime<Utc>,
}

impl IntelligenceCorrelation {
    pub fn new(
        source_finding_ids: Vec<Uuid>,
        kind: CorrelationKind,
        confidence: ConfidenceResult,
        evidence: Vec<EvidenceSource>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            source_finding_ids,
            kind,
            confidence,
            evidence,
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_confidence::ConfidenceSource;

    #[test]
    fn correlation_carries_every_source_finding_id() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let correlation = IntelligenceCorrelation::new(
            vec![a, b],
            CorrelationKind::TemporalProximity,
            ConfidenceResult::new(0.7, ConfidenceSource::Heuristic, None),
            Vec::new(),
        );
        assert_eq!(correlation.source_finding_ids, vec![a, b]);
    }

    #[test]
    fn correlation_serializes_with_camel_case_fields() {
        let correlation = IntelligenceCorrelation::new(
            vec![Uuid::new_v4()],
            CorrelationKind::Other("custom".to_string()),
            ConfidenceResult::new(0.5, ConfidenceSource::Heuristic, None),
            Vec::new(),
        );
        let json = serde_json::to_value(&correlation).unwrap();
        assert!(json.get("sourceFindingIds").is_some());
        assert_eq!(json["kind"]["kind"], "other");
        assert_eq!(json["kind"]["detail"], "custom");
    }
}
