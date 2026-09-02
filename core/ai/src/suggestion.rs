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
///
/// `service_id` scopes every suggestion to the live service it was
/// produced during, matching `PresentationItem::service_id` and the
/// `ai_suggestions.service_id` column already defined in the Phase 1.0
/// schema (`database/migrations/0001_initial_schema.sql`) - the schema
/// always expected this linkage; Phase 1.1 is the first caller that
/// actually needs to construct a `Suggestion` from a live service, so it
/// adds the field the schema already assumed existed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: Uuid,
    pub service_id: Uuid,
    pub kind: SuggestionKind,
    pub status: SuggestionStatus,
    pub confidence: ConfidenceResult,
    pub created_at: DateTime<Utc>,
    /// The transcript segment this suggestion was produced from, if known
    /// (Phase 1.3 traceability - "what did the pastor say"). `None` for a
    /// suggestion with no single originating segment (e.g. an operator's
    /// manually resolved ambiguous reference isn't tied to exactly one).
    pub transcript_segment_id: Option<Uuid>,
    /// The transcript substring that produced this suggestion, denormalized
    /// so the link survives even if the referenced segment is ever gone.
    pub source_text: Option<String>,
    /// How many times this suggestion's reference was independently
    /// redetected while still `Pending`, within the live pipeline's
    /// suggestion-dedup window (Phase 5.2, "temporal confirmation") -
    /// corroborating evidence for a heuristic (`Paraphrase`/`Semantic`)
    /// guess, not a reason a second suggestion was ever created. Always `0`
    /// at construction; only the desktop-layer pipeline
    /// (`apps/desktop/src-tauri/src/persistence.rs::confirm_suggestion`)
    /// ever increments it, since `core` has no notion of "redetected" -
    /// that requires already-persisted history this crate doesn't see.
    pub confirmation_count: u32,
}

impl Suggestion {
    pub fn new(service_id: Uuid, kind: SuggestionKind, confidence: ConfidenceResult) -> Self {
        Self {
            id: Uuid::new_v4(),
            service_id,
            kind,
            status: SuggestionStatus::Pending,
            confidence,
            created_at: Utc::now(),
            transcript_segment_id: None,
            source_text: None,
            confirmation_count: 0,
        }
    }

    /// Attach the transcript segment this suggestion came from. Called by
    /// the Tauri-layer pipeline (`apps/desktop/src-tauri/src/pipeline.rs`),
    /// which is the one place that has both the `Suggestion` the Bible
    /// Intelligence Core produced and the `TranscriptSegment` it produced
    /// it from - `core/service`'s pipeline itself only ever sees segment
    /// *text*, not segment identity, so it cannot set this itself.
    pub fn with_source(
        mut self,
        transcript_segment_id: Uuid,
        source_text: impl Into<String>,
    ) -> Self {
        self.transcript_segment_id = Some(transcript_segment_id);
        self.source_text = Some(source_text.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_confidence::ConfidenceSource;

    #[test]
    fn new_suggestions_always_start_pending() {
        let suggestion = Suggestion::new(
            Uuid::new_v4(),
            SuggestionKind::Scripture {
                reference: "ROM 8:28".into(),
            },
            ConfidenceResult::new(0.95, ConfidenceSource::Heuristic, None),
        );
        assert_eq!(suggestion.status, SuggestionStatus::Pending);
    }

    #[test]
    fn new_suggestions_start_with_zero_confirmations() {
        let suggestion = Suggestion::new(
            Uuid::new_v4(),
            SuggestionKind::Scripture {
                reference: "ROM 8:28".into(),
            },
            ConfidenceResult::new(0.8, ConfidenceSource::Heuristic, None),
        );
        assert_eq!(suggestion.confirmation_count, 0);
    }
}
