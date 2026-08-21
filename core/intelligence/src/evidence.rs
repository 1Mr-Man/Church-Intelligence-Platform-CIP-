//! Evidence (*why* a finding was produced) and provenance (*where its
//! underlying content traces back to*) - two related but distinct
//! questions, kept as separate types per Phase 2.0 spec sections 9-10.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One contributing piece of evidence behind a finding. A tagged enum
/// (extensible by adding variants) rather than one struct with dozens of
/// unrelated optional fields - each variant carries only the data that
/// kind of evidence actually has. A finding typically carries one, but the
/// list can hold more (e.g. a correlation's evidence names several
/// contributing findings).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EvidenceSource {
    /// Derived from transcript text - the most common evidence kind for a
    /// Phase 2.0 finding.
    Transcript {
        segment_ids: Vec<Uuid>,
        excerpt: String,
    },
    /// Derived from installed content, referenced by its Content Registry
    /// id (Phase 1.5 convention, e.g. `"bible:KJV"`).
    Content { content_id: String },
    /// Derived from the currently active context (e.g. "active Scripture
    /// context was Romans 8").
    Context { description: String },
    /// Derived from timing/ordering information.
    Temporal { description: String },
    /// Derived from another finding this or an earlier engine produced.
    AnotherFinding { finding_id: Uuid },
    /// Derived from a service lifecycle event (start/pause/end, etc).
    ServiceEvent { description: String },
    /// Derived from an explicit operator action.
    OperatorAction { description: String },
}

/// Where a finding's underlying content traces back to. Deliberately thin:
/// this crate does not re-implement the Phase 1.5 Content Registry's
/// licensing model, it only *references* it by id - see
/// `cip_core_content::ContentMetadata` for the actual publisher/copyright/
/// license/distribution facts. `content_id` is `None` when a finding has
/// no content-registry-backed source (e.g. a purely transcript/context-
/// derived inference).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceProvenance {
    pub content_id: Option<String>,
    /// Free-text provenance detail beyond the content id (e.g. "operator
    /// override", "derived from finding <id>"). Never a licensing claim -
    /// see `ContentMetadata` for that.
    pub note: Option<String>,
}

impl IntelligenceProvenance {
    pub fn unknown() -> Self {
        Self {
            content_id: None,
            note: None,
        }
    }

    pub fn from_content(content_id: impl Into<String>) -> Self {
        Self {
            content_id: Some(content_id.into()),
            note: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_variants_serialize_with_a_kind_tag() {
        let evidence = EvidenceSource::Transcript {
            segment_ids: vec![Uuid::nil()],
            excerpt: "Romans chapter eight".to_string(),
        };
        let json = serde_json::to_value(&evidence).unwrap();
        assert_eq!(json["kind"], "transcript");
        assert_eq!(json["excerpt"], "Romans chapter eight");
    }

    #[test]
    fn unknown_provenance_has_no_content_id_and_is_never_guessed() {
        let provenance = IntelligenceProvenance::unknown();
        assert_eq!(provenance.content_id, None);
        assert_eq!(provenance.note, None);
    }

    #[test]
    fn provenance_from_content_carries_the_registry_id() {
        let provenance = IntelligenceProvenance::from_content("bible:KJV");
        assert_eq!(provenance.content_id.as_deref(), Some("bible:KJV"));
    }

    #[test]
    fn evidence_round_trips_through_json() {
        let evidence = EvidenceSource::AnotherFinding {
            finding_id: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&evidence).unwrap();
        let back: EvidenceSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, evidence);
    }
}
