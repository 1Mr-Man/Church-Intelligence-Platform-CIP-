//! [`SermonSection`]: a deterministic, timestamped span within a
//! [`crate::foundation::Sermon`] - "the message was in its Introduction
//! from 10:02 to 10:05, then Main Message from 10:05 onward." Never a
//! semantic classification of *what was said* (that is
//! `cip_core_sermon::state::SermonState`'s job, a Phase 2.6-equivalent
//! concern) - only a structural span an operator explicitly opened, or the
//! system opened on an unambiguous lifecycle boundary.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A fixed, deterministic taxonomy of structural sermon sections. Closed
/// (no open-ended "other") for the same reason
/// `cip_core_sermon::SermonElementKind` is closed - every assignment must
/// be one of a documented set, never a free-text guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SermonSectionKind {
    Introduction,
    ScriptureReading,
    MainMessage,
    Illustration,
    Prayer,
    AltarCall,
    Conclusion,
}

impl SermonSectionKind {
    pub const fn label(self) -> &'static str {
        match self {
            SermonSectionKind::Introduction => "INTRODUCTION",
            SermonSectionKind::ScriptureReading => "SCRIPTURE_READING",
            SermonSectionKind::MainMessage => "MAIN_MESSAGE",
            SermonSectionKind::Illustration => "ILLUSTRATION",
            SermonSectionKind::Prayer => "PRAYER",
            SermonSectionKind::AltarCall => "ALTAR_CALL",
            SermonSectionKind::Conclusion => "CONCLUSION",
        }
    }
}

/// How a [`SermonSection`] assignment was established - the data model's
/// explicit answer to "was this a human decision, a deterministic system
/// boundary, or an inference?" (spec's own required distinction).
/// `Inferred` is reserved, exactly like `AssertionLevel::Generated` is
/// reserved in `core/intelligence` - nothing in Phase 2.5 produces it. See
/// `docs/sermon-foundation.md`'s "NOT AVAILABLE" section: inferring a
/// section from vague transcript language is explicitly out of scope here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionOrigin {
    /// An operator explicitly chose this section.
    OperatorAssigned,
    /// A deterministic, unambiguous system boundary (e.g. the
    /// `Introduction` section that opens automatically when a sermon
    /// starts) - never a judgment call about transcript content.
    SystemBoundary,
    /// Reserved for a future phase's semantic section inference - never
    /// produced by anything in this crate today.
    Inferred,
}

/// One open-or-closed span. `ended_at` is `None` while this section is
/// still the sermon's current one; assigning a new section closes the
/// previous one with an explicit timestamp rather than deleting or
/// rewriting it (spec's "do not delete previous section history").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonSection {
    pub id: Uuid,
    pub sermon_id: Uuid,
    pub kind: SermonSectionKind,
    pub origin: SectionOrigin,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub note: Option<String>,
}

impl SermonSection {
    pub fn open(
        sermon_id: Uuid,
        kind: SermonSectionKind,
        origin: SectionOrigin,
        note: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            sermon_id,
            kind,
            origin,
            started_at: Utc::now(),
            ended_at: None,
            note,
        }
    }

    pub fn is_open(&self) -> bool {
        self.ended_at.is_none()
    }

    pub fn close(&mut self) {
        if self.ended_at.is_none() {
            self.ended_at = Some(Utc::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newly_opened_section_has_no_end_time() {
        let section = SermonSection::open(
            Uuid::new_v4(),
            SermonSectionKind::Introduction,
            SectionOrigin::SystemBoundary,
            None,
        );
        assert!(section.is_open());
        assert!(section.ended_at.is_none());
    }

    #[test]
    fn closing_a_section_sets_ended_at_and_it_is_no_longer_open() {
        let mut section = SermonSection::open(
            Uuid::new_v4(),
            SermonSectionKind::MainMessage,
            SectionOrigin::OperatorAssigned,
            None,
        );
        section.close();
        assert!(!section.is_open());
        assert!(section.ended_at.is_some());
    }

    #[test]
    fn closing_an_already_closed_section_does_not_move_its_end_time() {
        let mut section = SermonSection::open(
            Uuid::new_v4(),
            SermonSectionKind::Prayer,
            SectionOrigin::OperatorAssigned,
            None,
        );
        section.close();
        let first_close = section.ended_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        section.close();
        assert_eq!(
            section.ended_at, first_close,
            "close() must be idempotent, never re-timestamping"
        );
    }

    #[test]
    fn kind_labels_are_screaming_snake_case_and_distinct() {
        let all = [
            SermonSectionKind::Introduction,
            SermonSectionKind::ScriptureReading,
            SermonSectionKind::MainMessage,
            SermonSectionKind::Illustration,
            SermonSectionKind::Prayer,
            SermonSectionKind::AltarCall,
            SermonSectionKind::Conclusion,
        ];
        let mut labels: Vec<&str> = all.iter().map(|k| k.label()).collect();
        let before = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), before);
        for label in labels {
            assert_eq!(label, label.to_uppercase());
        }
    }

    #[test]
    fn section_serializes_with_camel_case_fields_and_snake_case_enums() {
        let section = SermonSection::open(
            Uuid::new_v4(),
            SermonSectionKind::AltarCall,
            SectionOrigin::OperatorAssigned,
            Some("moved early".to_string()),
        );
        let json = serde_json::to_value(&section).unwrap();
        assert_eq!(json["kind"], "altar_call");
        assert_eq!(json["origin"], "operator_assigned");
        assert!(json.get("sermonId").is_some());
        assert!(json.get("startedAt").is_some());
    }
}
