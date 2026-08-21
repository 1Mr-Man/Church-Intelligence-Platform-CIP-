use chrono::{DateTime, Utc};
use cip_core_confidence::ConfidenceResult;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What kind of thing a suggestion proposes. Kept as an open string-backed
/// enum (`#[non_exhaustive]`) rather than a small fixed set, since Phase 2+
/// domains (song, sermon) will add their own suggestion kinds without
/// needing to modify this core type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
#[non_exhaustive]
pub enum SuggestionKind {
    Scripture { reference: String },
    Other { label: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionStatus {
    Pending,
    Approved,
    Edited,
    Rejected,
}

/// An AI-produced proposal awaiting human review. Every suggestion is
/// human-controlled by construction: it starts `Pending` and only a human
/// action (mirrored by the `SUGGESTION_APPROVED` / `SUGGESTION_EDITED` /
/// `SUGGESTION_REJECTED` events) moves it out of that state - nothing in
/// `core` auto-applies a suggestion on its own, regardless of confidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: Uuid,
    pub kind: SuggestionKind,
    pub status: SuggestionStatus,
    pub confidence: ConfidenceResult,
    pub created_at: DateTime<Utc>,
}

impl Suggestion {
    pub fn new(kind: SuggestionKind, confidence: ConfidenceResult) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            status: SuggestionStatus::Pending,
            confidence,
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_confidence::ConfidenceSource;

    #[test]
    fn new_suggestions_always_start_pending() {
        let suggestion = Suggestion::new(
            SuggestionKind::Scripture {
                reference: "ROM 8:28".into(),
            },
            ConfidenceResult::new(0.95, ConfidenceSource::Heuristic, None),
        );
        assert_eq!(suggestion.status, SuggestionStatus::Pending);
    }
}
