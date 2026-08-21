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
}

impl PresentationItem {
    pub fn prepare(service_id: Uuid, content: PresentationContent) -> Self {
        Self {
            id: Uuid::new_v4(),
            service_id,
            content,
            status: PresentationItemStatus::Prepared,
            created_at: Utc::now(),
        }
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
}
