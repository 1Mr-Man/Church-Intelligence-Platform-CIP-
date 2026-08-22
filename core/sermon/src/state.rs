//! Lightweight sermon-state classification (spec section 29): "a
//! classification of current message structure. It must not become a
//! rigid state machine that prevents pastors from moving freely." Every
//! call to [`infer_state`] re-derives the state fresh from whatever was
//! most recently detected - there is no persistent state machine object
//! here, no illegal-transition guard, and nothing that could ever "lock"
//! a service into a phase it has already moved past.

use crate::foundation::section::SermonSectionKind;
use crate::taxonomy::SermonElementKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SermonState {
    Introduction,
    Teaching,
    MainPoint,
    Illustration,
    Application,
    Conclusion,
    Prayer,
    Unknown,
}

impl SermonState {
    pub const fn label(self) -> &'static str {
        match self {
            SermonState::Introduction => "INTRODUCTION",
            SermonState::Teaching => "TEACHING",
            SermonState::MainPoint => "MAIN_POINT",
            SermonState::Illustration => "ILLUSTRATION",
            SermonState::Application => "APPLICATION",
            SermonState::Conclusion => "CONCLUSION",
            SermonState::Prayer => "PRAYER",
            SermonState::Unknown => "UNKNOWN",
        }
    }
}

fn state_for_kind(kind: SermonElementKind) -> SermonState {
    match kind {
        SermonElementKind::MainPoint | SermonElementKind::SubPoint => SermonState::MainPoint,
        SermonElementKind::Illustration | SermonElementKind::Story | SermonElementKind::Example => {
            SermonState::Illustration
        }
        SermonElementKind::Application => SermonState::Application,
        SermonElementKind::Conclusion | SermonElementKind::Summary => SermonState::Conclusion,
        SermonElementKind::PrayerPoint => SermonState::Prayer,
        SermonElementKind::Takeaway => SermonState::Conclusion,
        SermonElementKind::Theme
        | SermonElementKind::ScriptureReference
        | SermonElementKind::ScriptureQuotation
        | SermonElementKind::Definition
        | SermonElementKind::KeyStatement
        | SermonElementKind::Declaration
        | SermonElementKind::Question
        | SermonElementKind::Reflection
        | SermonElementKind::FoodForThought
        | SermonElementKind::Transition => SermonState::Teaching,
    }
}

/// A conservative, read-only candidate mapping from the internal
/// [`SermonState`] classification onto the Phase 2.5 Sermon Foundation's
/// own [`SermonSectionKind`] taxonomy (Phase 2.6 spec section 14,
/// "STRUCTURAL TRANSITION DETECTION": "Reuse `SermonSectionKind` from
/// Phase 2.5"). Deliberately partial - `SermonState::Application` and
/// `SermonState::Unknown` have no honest single-section equivalent in the
/// foundation's closed taxonomy, so they map to `None` rather than
/// guessing one. This function only ever *suggests* a candidate section;
/// nothing here mutates persisted `SermonSection` state (that remains the
/// Sermon Foundation/operator's exclusive responsibility, per the same
/// spec section: "The operator/foundation layer remains responsible for
/// durable section state").
pub fn candidate_section_for_state(state: SermonState) -> Option<SermonSectionKind> {
    match state {
        SermonState::Introduction => Some(SermonSectionKind::Introduction),
        SermonState::Teaching | SermonState::MainPoint => Some(SermonSectionKind::MainMessage),
        SermonState::Illustration => Some(SermonSectionKind::Illustration),
        SermonState::Conclusion => Some(SermonSectionKind::Conclusion),
        SermonState::Prayer => Some(SermonSectionKind::Prayer),
        SermonState::Application | SermonState::Unknown => None,
    }
}

/// Derive the current sermon state from the most recent segment's detected
/// kinds (empty when a segment triggered no detector at all) and whether
/// any segment has been processed yet at all. `has_any_prior_segment`
/// distinguishes a genuinely fresh service (`Introduction`) from a run of
/// segments that simply matched no detector (`Unknown`, not a silent
/// reset to `Introduction`).
pub fn infer_state(
    most_recent_kinds: &[SermonElementKind],
    has_any_prior_segment: bool,
) -> SermonState {
    if let Some(kind) = most_recent_kinds.first() {
        return state_for_kind(*kind);
    }
    if has_any_prior_segment {
        SermonState::Unknown
    } else {
        SermonState::Introduction
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_segments_yet_is_introduction() {
        assert_eq!(infer_state(&[], false), SermonState::Introduction);
    }

    #[test]
    fn a_segment_with_no_detections_after_some_history_is_unknown_not_a_reset() {
        assert_eq!(infer_state(&[], true), SermonState::Unknown);
    }

    #[test]
    fn a_main_point_detection_moves_state_to_main_point() {
        assert_eq!(
            infer_state(&[SermonElementKind::MainPoint], true),
            SermonState::MainPoint
        );
    }

    #[test]
    fn a_story_detection_moves_state_to_illustration() {
        assert_eq!(
            infer_state(&[SermonElementKind::Story], true),
            SermonState::Illustration
        );
    }

    #[test]
    fn a_prayer_point_moves_state_to_prayer() {
        assert_eq!(
            infer_state(&[SermonElementKind::PrayerPoint], true),
            SermonState::Prayer
        );
    }

    #[test]
    fn a_takeaway_detection_moves_state_to_conclusion() {
        assert_eq!(
            infer_state(&[SermonElementKind::Takeaway], true),
            SermonState::Conclusion
        );
    }

    #[test]
    fn a_food_for_thought_detection_moves_state_to_teaching() {
        assert_eq!(
            infer_state(&[SermonElementKind::FoodForThought], true),
            SermonState::Teaching
        );
    }

    // --- candidate_section_for_state (Phase 2.6) -----------------------

    #[test]
    fn every_mapped_state_has_a_plausible_foundation_section_candidate() {
        assert_eq!(
            candidate_section_for_state(SermonState::Introduction),
            Some(SermonSectionKind::Introduction)
        );
        assert_eq!(
            candidate_section_for_state(SermonState::Teaching),
            Some(SermonSectionKind::MainMessage)
        );
        assert_eq!(
            candidate_section_for_state(SermonState::MainPoint),
            Some(SermonSectionKind::MainMessage)
        );
        assert_eq!(
            candidate_section_for_state(SermonState::Illustration),
            Some(SermonSectionKind::Illustration)
        );
        assert_eq!(
            candidate_section_for_state(SermonState::Conclusion),
            Some(SermonSectionKind::Conclusion)
        );
        assert_eq!(
            candidate_section_for_state(SermonState::Prayer),
            Some(SermonSectionKind::Prayer)
        );
    }

    #[test]
    fn states_with_no_honest_section_equivalent_map_to_none() {
        assert_eq!(candidate_section_for_state(SermonState::Application), None);
        assert_eq!(candidate_section_for_state(SermonState::Unknown), None);
    }

    #[test]
    fn state_can_move_freely_back_to_teaching_after_application() {
        // The pastor returning to teaching after an application is a
        // normal, always-legal transition - no state machine forbids it.
        assert_eq!(
            infer_state(&[SermonElementKind::Application], true),
            SermonState::Application
        );
        assert_eq!(
            infer_state(&[SermonElementKind::Definition], true),
            SermonState::Teaching
        );
    }
}
