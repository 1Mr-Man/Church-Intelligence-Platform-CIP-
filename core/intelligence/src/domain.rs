//! The domain/kind/status/assertion/priority axes every [`crate::IntelligenceFinding`]
//! is classified along. Kept as four small, independent enums rather than
//! one combined type, because each answers a different question and they
//! vary independently - see each type's own docs for its question.

use cip_core_confidence::{ConfidenceLevel, ConfidenceResult};
use serde::{Deserialize, Serialize};

/// *Which intelligence domain produced this?* A closed enum (not
/// `#[non_exhaustive]`): Phase 2.0 fixes the *shape* future engines will
/// occupy without implementing them, mirroring how `core/content::ContentType`
/// was grown - a new domain is a deliberate new variant here, not something
/// added by an external plugin.
///
/// `Bible` is the only domain with a real engine behind it in Phase 2.0
/// (see [`crate::bible_adapter::BibleIntelligenceEngine`]). `Music`,
/// `Sermon`, and `Content` exist so the architecture has somewhere to put
/// those engines later; nothing in this codebase implements them yet -
/// see `docs/intelligence-architecture.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceDomain {
    Bible,
    Music,
    Service,
    Sermon,
    Content,
    CrossDomain,
}

/// *What kind of thing was found?* Deliberately a separate axis from
/// [`IntelligenceDomain`] - "which engine produced this" and "what shape of
/// thing is this" vary independently (e.g. a future `CrossDomainEngine`
/// finding is still, structurally, a `Correlation`-kind finding; a
/// `BibleEngine` finding is always `Scripture`-kind). Do not merge these
/// two enums into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    Scripture,
    Music,
    ServiceState,
    Sermon,
    Content,
    Correlation,
}

/// The lifecycle a finding moves through under **explicit** operator
/// action - deliberately a separate state machine from
/// `cip_core_presentation::PresentationItemStatus`. Accepting a finding is
/// not the same event as preparing a presentation item; see
/// `docs/intelligence-architecture.md`'s "human-in-the-loop boundary"
/// section for why these are never merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatus {
    Detected,
    Reviewed,
    Accepted,
    Rejected,
    Expired,
}

/// The mandatory epistemic-state distinction (Phase 2.0 spec section 8):
/// CIP must never present one of these as if it were another.
///
/// ```text
/// OBSERVED   - what was actually said (the transcript text itself).
/// INFERRED   - a state CIP derived from what was said (e.g. an active
///              Scripture context established from "Romans chapter eight").
/// SUGGESTED  - a specific proposal CIP is putting forward for human review
///              (e.g. the concrete reference "Romans 8:28").
/// GENERATED  - synthesized content (e.g. a reflection or summary) - not
///              implemented in Phase 2.0, but the architecture reserves
///              this level so a future engine cannot silently collapse it
///              into `Suggested`.
/// ```
///
/// A finding's `assertion_level` must always reflect which of these it
/// actually is - never upgraded to make it look more certain than it is,
/// never downgraded to hide what it actually claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionLevel {
    Observed,
    Inferred,
    Suggested,
    Generated,
}

/// How urgently a finding needs operator attention - **not** a restatement
/// of confidence. A high-confidence, low-urgency finding (e.g. a
/// well-established Scripture suggestion) can rank below a
/// lower-confidence, service-critical one - see [`baseline_priority`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntelligencePriority {
    Low,
    Normal,
    High,
    Critical,
}

/// A deterministic, non-ML baseline priority. Confidence sets the floor
/// (a `Low`-confidence finding is never more than `Normal` priority, since
/// low certainty should not itself demand urgent attention); `kind` can
/// raise it from there - `ServiceState` and `Correlation` findings describe
/// something about the service itself, not just a candidate piece of
/// content, so they start at `High` regardless of confidence bucket. No
/// randomness, no learned weights - see Phase 2.0 spec section 13.
///
/// This is a starting point only; nothing in this crate prevents an
/// operator or a later phase from overriding it.
pub fn baseline_priority(kind: FindingKind, confidence: &ConfidenceResult) -> IntelligencePriority {
    if confidence.level == ConfidenceLevel::Low {
        return IntelligencePriority::Low;
    }
    match kind {
        FindingKind::ServiceState | FindingKind::Correlation => IntelligencePriority::High,
        FindingKind::Scripture
        | FindingKind::Music
        | FindingKind::Sermon
        | FindingKind::Content => IntelligencePriority::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_confidence::ConfidenceSource;

    #[test]
    fn domain_and_kind_serialize_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&IntelligenceDomain::CrossDomain).unwrap(),
            "\"cross_domain\""
        );
        assert_eq!(
            serde_json::to_string(&FindingKind::ServiceState).unwrap(),
            "\"service_state\""
        );
    }

    #[test]
    fn assertion_level_round_trips_through_json() {
        for level in [
            AssertionLevel::Observed,
            AssertionLevel::Inferred,
            AssertionLevel::Suggested,
            AssertionLevel::Generated,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let back: AssertionLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(back, level);
        }
    }

    #[test]
    fn priority_is_not_a_restatement_of_confidence() {
        // Finding A: high confidence, ordinary content kind -> Normal.
        let high_confidence = ConfidenceResult::new(0.95, ConfidenceSource::Heuristic, None);
        let a = baseline_priority(FindingKind::Scripture, &high_confidence);

        // Finding B: lower (but not Low-bucket) confidence, service-state
        // kind -> High. Lower confidence, higher priority - proves the two
        // axes are independent.
        let medium_confidence = ConfidenceResult::new(0.75, ConfidenceSource::Heuristic, None);
        let b = baseline_priority(FindingKind::ServiceState, &medium_confidence);

        assert_eq!(a, IntelligencePriority::Normal);
        assert_eq!(b, IntelligencePriority::High);
        assert!(
            b > a,
            "lower-confidence service-state finding outranks a higher-confidence content one"
        );
    }

    #[test]
    fn low_confidence_never_exceeds_normal_priority_regardless_of_kind() {
        let low = ConfidenceResult::new(0.2, ConfidenceSource::Heuristic, None);
        assert_eq!(
            baseline_priority(FindingKind::ServiceState, &low),
            IntelligencePriority::Low
        );
    }

    #[test]
    fn priority_ordering_is_total_and_ascending() {
        assert!(IntelligencePriority::Low < IntelligencePriority::Normal);
        assert!(IntelligencePriority::Normal < IntelligencePriority::High);
        assert!(IntelligencePriority::High < IntelligencePriority::Critical);
    }
}
