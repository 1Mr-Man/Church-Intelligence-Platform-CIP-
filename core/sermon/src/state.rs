//! Lightweight sermon-state classification (spec section 29): "a
//! classification of current message structure. It must not become a
//! rigid state machine that prevents pastors from moving freely." Every
//! call to [`infer_state`] re-derives the state fresh from whatever was
//! most recently detected - there is no persistent state machine object
//! here, no illegal-transition guard, and nothing that could ever "lock"
//! a service into a phase it has already moved past.

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
        SermonElementKind::Theme
        | SermonElementKind::ScriptureReference
        | SermonElementKind::ScriptureQuotation
        | SermonElementKind::Definition
        | SermonElementKind::KeyStatement
        | SermonElementKind::Declaration
        | SermonElementKind::Question
        | SermonElementKind::Reflection
        | SermonElementKind::Transition => SermonState::Teaching,
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
