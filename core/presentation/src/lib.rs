//! Presentation domain: the `PresentationItem` model.
//!
//! This crate defines *what* is shown, not *how* it's rendered - rendering
//! is `presentation/renderer`'s job, kept separate so the AI/suggestion
//! pipeline and the on-screen renderer never couple directly, per the
//! approved architecture.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "type"
)]
#[non_exhaustive]
pub enum PresentationContent {
    Scripture {
        reference: String,
        translation_id: String,
        text: String,
    },
    Text {
        title: Option<String>,
        body: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationItemStatus {
    /// Queued but not yet shown.
    Prepared,
    /// Currently on screen.
    Active,
    /// Was shown and has since been dismissed.
    Stopped,
}

/// A single item in the presentation queue (e.g. one verse, one slide of
/// text). `service` (a `ServiceSession` id) scopes every item to the live
/// service it belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationItem {
    pub id: Uuid,
    pub service_id: Uuid,
    pub content: PresentationContent,
    pub status: PresentationItemStatus,
    pub created_at: DateTime<Utc>,
    /// The `ai_suggestions` row this item was prepared from, when it came
    /// from the automatic detection path rather than manual creation.
    #[serde(default)]
    pub source_suggestion_id: Option<Uuid>,
    /// The rendering template used to prepare this item (e.g.
    /// `"SCRIPTURE_DEFAULT"`), when one was applied.
    #[serde(default)]
    pub template: Option<String>,
}

impl PresentationItem {
    pub fn prepare(service_id: Uuid, content: PresentationContent) -> Self {
        Self {
            id: Uuid::new_v4(),
            service_id,
            content,
            status: PresentationItemStatus::Prepared,
            created_at: Utc::now(),
            source_suggestion_id: None,
            template: None,
        }
    }

    /// Records which suggestion this item was prepared from (the automatic
    /// detection path). Manually-created items leave this unset.
    pub fn with_source_suggestion(mut self, suggestion_id: Uuid) -> Self {
        self.source_suggestion_id = Some(suggestion_id);
        self
    }

    /// Records which rendering template was applied to prepare this item.
    pub fn with_template(mut self, template: impl Into<String>) -> Self {
        self.template = Some(template.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_items_start_in_prepared_status() {
        let item = PresentationItem::prepare(
            Uuid::new_v4(),
            PresentationContent::Scripture {
                reference: "ROM 8:28".into(),
                translation_id: "KJV".into(),
                text: "And we know that all things work together for good...".into(),
            },
        );
        assert_eq!(item.status, PresentationItemStatus::Prepared);
    }

    #[test]
    fn source_suggestion_and_template_are_unset_unless_recorded() {
        let item = PresentationItem::prepare(
            Uuid::new_v4(),
            PresentationContent::Text {
                title: None,
                body: "hello".into(),
            },
        );
        assert_eq!(item.source_suggestion_id, None);
        assert_eq!(item.template, None);

        let suggestion_id = Uuid::new_v4();
        let item = item
            .with_source_suggestion(suggestion_id)
            .with_template("SCRIPTURE_DEFAULT");
        assert_eq!(item.source_suggestion_id, Some(suggestion_id));
        assert_eq!(item.template.as_deref(), Some("SCRIPTURE_DEFAULT"));
    }
}
