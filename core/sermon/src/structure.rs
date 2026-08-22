//! Incremental sermon structure tracking (spec section 11, 37): an ordered,
//! append-only list of main points, each optionally carrying nested
//! sub-points. History is never rewritten - a later point never erases an
//! earlier one; refining the *theme* is [`crate::theme::ThemeTracker`]'s
//! job, not this module's. Hierarchy is expressed with plain sequence
//! numbers, never a general graph ("do not build a general graph
//! database", spec section 37).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SermonSubPoint {
    pub sequence: u32,
    pub raw_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SermonPoint {
    pub sequence: u32,
    /// The exact transcript text that stated this point - never
    /// paraphrased.
    pub raw_text: String,
    pub sub_points: Vec<SermonSubPoint>,
}

/// Append-only structure tracker. `record_main_point`/`record_sub_point`
/// are the only ways points enter it; nothing here ever removes or edits a
/// previously recorded point (spec section 11: "must not rewrite history
/// silently").
#[derive(Debug, Default)]
pub struct SermonStructure {
    points: Vec<SermonPoint>,
}

impl SermonStructure {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new main point, becoming the current point. Returns its
    /// assigned sequence number.
    pub fn record_main_point(&mut self, raw_text: &str) -> u32 {
        let sequence = self.points.len() as u32 + 1;
        self.points.push(SermonPoint {
            sequence,
            raw_text: raw_text.to_string(),
            sub_points: Vec::new(),
        });
        sequence
    }

    /// Attach a sub-point to the current main point. If no main point has
    /// been recorded yet, the sub-point is dropped rather than inventing a
    /// parent - "avoid inventing hierarchy when evidence is weak" (spec
    /// section 13). Returns whether it was attached.
    pub fn record_sub_point(&mut self, raw_text: &str) -> bool {
        let Some(current) = self.points.last_mut() else {
            return false;
        };
        let sequence = current.sub_points.len() as u32 + 1;
        current.sub_points.push(SermonSubPoint {
            sequence,
            raw_text: raw_text.to_string(),
        });
        true
    }

    /// The most recently recorded main point - "becomes current," while
    /// every earlier point remains reachable via [`Self::points`] (spec
    /// section 52: "do not rewrite Point 1").
    pub fn current_point(&self) -> Option<&SermonPoint> {
        self.points.last()
    }

    pub fn points(&self) -> &[SermonPoint] {
        &self.points
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_two_main_points_keeps_both_and_advances_current() {
        let mut structure = SermonStructure::new();
        structure.record_main_point("Faith comes by hearing.");
        assert_eq!(
            structure.current_point().unwrap().raw_text,
            "Faith comes by hearing."
        );

        structure.record_main_point("Faith must be exercised.");
        assert_eq!(structure.points().len(), 2);
        assert_eq!(
            structure.current_point().unwrap().raw_text,
            "Faith must be exercised.",
            "the second point becomes current"
        );
        assert_eq!(
            structure.points()[0].raw_text,
            "Faith comes by hearing.",
            "the first point is never rewritten"
        );
    }

    #[test]
    fn sub_points_attach_to_the_current_main_point() {
        let mut structure = SermonStructure::new();
        structure.record_main_point("Faith comes by hearing.");
        assert!(structure.record_sub_point("Hearing requires attention."));
        assert!(structure.record_sub_point("Attention requires stillness."));

        assert_eq!(structure.current_point().unwrap().sub_points.len(), 2);
    }

    #[test]
    fn a_sub_point_with_no_main_point_yet_is_never_invented_a_parent() {
        let mut structure = SermonStructure::new();
        assert!(!structure.record_sub_point("Under this point..."));
        assert!(structure.points().is_empty());
    }

    #[test]
    fn sub_points_do_not_leak_into_a_later_main_points_list() {
        let mut structure = SermonStructure::new();
        structure.record_main_point("Point one.");
        structure.record_sub_point("Sub of one.");
        structure.record_main_point("Point two.");

        assert_eq!(structure.points()[0].sub_points.len(), 1);
        assert!(structure.points()[1].sub_points.is_empty());
    }
}
