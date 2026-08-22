//! [`SermonSegment`]: "which portion of the transcript belongs to this
//! sermon" - never "what does this portion mean." A thin linkage record,
//! not a copy: `transcript_segment_id` references the single canonical
//! `TranscriptSegment` this crate never duplicates (spec's
//! "SERMON → TRANSCRIPT RELATIONSHIP" section - the transcript itself
//! remains owned, and immutable, elsewhere).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One transcript segment's association with a sermon. `sequence` is the
/// order in which segments were linked to *this sermon specifically*
/// (distinct from the transcript segment's own service-wide sequence
/// number) - gapless and starting at 0 for a given sermon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonSegment {
    pub id: Uuid,
    pub sermon_id: Uuid,
    pub transcript_segment_id: Uuid,
    pub sequence: u32,
    /// Which section (if any) was open at the moment this segment was
    /// linked - `None` when no section was open, never guessed.
    pub section_id: Option<Uuid>,
    pub linked_at: DateTime<Utc>,
}

impl SermonSegment {
    pub fn new(
        sermon_id: Uuid,
        transcript_segment_id: Uuid,
        sequence: u32,
        section_id: Option<Uuid>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            sermon_id,
            transcript_segment_id,
            sequence,
            section_id,
            linked_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_link_never_duplicates_transcript_text() {
        // Type-level proof: `SermonSegment` has no `text`/`content` field -
        // the only way to know what was said is to follow
        // `transcript_segment_id` back to the canonical `TranscriptSegment`.
        let segment = SermonSegment::new(Uuid::new_v4(), Uuid::new_v4(), 0, None);
        let json = serde_json::to_value(&segment).unwrap();
        assert!(json.get("text").is_none());
        assert!(json.get("content").is_none());
    }

    #[test]
    fn sequence_and_section_are_carried_through_exactly() {
        let sermon_id = Uuid::new_v4();
        let transcript_id = Uuid::new_v4();
        let section_id = Uuid::new_v4();
        let segment = SermonSegment::new(sermon_id, transcript_id, 3, Some(section_id));
        assert_eq!(segment.sermon_id, sermon_id);
        assert_eq!(segment.transcript_segment_id, transcript_id);
        assert_eq!(segment.sequence, 3);
        assert_eq!(segment.section_id, Some(section_id));
    }

    #[test]
    fn a_segment_outside_any_open_section_carries_no_section_id() {
        let segment = SermonSegment::new(Uuid::new_v4(), Uuid::new_v4(), 0, None);
        assert!(segment.section_id.is_none());
    }

    #[test]
    fn segment_serializes_with_camel_case_fields() {
        let segment = SermonSegment::new(Uuid::new_v4(), Uuid::new_v4(), 1, None);
        let json = serde_json::to_value(&segment).unwrap();
        assert!(json.get("sermonId").is_some());
        assert!(json.get("transcriptSegmentId").is_some());
        assert!(json.get("linkedAt").is_some());
    }
}
