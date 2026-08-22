//! Evidence-based sermon theme tracking (spec sections 14-15, 26).
//!
//! "Do NOT equate the most frequent word with the sermon theme" - a
//! candidate theme requires *both* repeated mention across a bounded
//! window *and* at least one structural mention (the concept appearing in
//! an explicit main point, key statement, definition, or declaration).
//! Repetition alone only ever produces supporting evidence, never a
//! candidate on its own.

use crate::taxonomy::SermonElementKind;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Bounded window of recent segments' concepts kept for theme evidence -
/// matches the order of magnitude `IntelligenceContext`'s
/// `DEFAULT_MAX_RECENT_TRANSCRIPT_SEGMENTS` already uses elsewhere in this
/// codebase, so a 10,000-segment service still keeps this tracker's memory
/// bounded (spec section 57).
pub const DEFAULT_THEME_WINDOW: usize = 40;

/// A concept must be repeated at least this many times across the window
/// before it may contribute to a theme candidate at all.
pub const DEFAULT_REPETITION_THRESHOLD: u32 = 3;

/// ...and mentioned in at least this many structurally-significant
/// segments (a main point, key statement, definition, or declaration) -
/// the "use repetition as supporting evidence, not the sole basis" rule.
pub const DEFAULT_STRUCTURAL_THRESHOLD: u32 = 1;

/// Small, deliberately non-exhaustive stopword/generic-church-language
/// list - "do not introduce a heavyweight NLP dependency merely for
/// this" (spec section 26). Kept as a plain slice, not a crate dependency.
const IGNORED_WORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "these", "those", "from", "have", "has", "had",
    "were", "was", "are", "you", "your", "yours", "they", "them", "then", "than", "when", "what",
    "which", "because", "about", "into", "unto", "shall", "will", "would", "should", "could",
    "been", "being", "there", "here", "today", "morning", "brothers", "sisters", "church", "amen",
    "lord", "god", "jesus", "christ", "just", "like", "know", "think", "want", "going", "come",
    "let", "lets", "very", "really", "some", "one", "two", "three", "four", "five", "not", "but",
    "all", "can", "our", "his", "her", "she", "he", "who", "how", "why", "now",
];

/// Structural detections that count toward a concept's "structural
/// mention" score - the same handful of kinds spec section 14 treats as
/// evidence of an explicit topic statement or sermon structure.
fn is_structural_signal(kind: SermonElementKind) -> bool {
    matches!(
        kind,
        SermonElementKind::MainPoint
            | SermonElementKind::KeyStatement
            | SermonElementKind::Definition
            | SermonElementKind::Declaration
    )
}

fn extract_concepts(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 4)
        .map(|w| w.to_lowercase())
        .filter(|w| !IGNORED_WORDS.contains(&w.as_str()))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeCandidate {
    /// The theme label - one concept, or two joined as `"X and Y"` when a
    /// second concept's evidence is close to the leading one (theme
    /// evolution, spec section 15).
    pub label: String,
    pub confidence: f64,
    pub repetition_count: u32,
    pub structural_mentions: u32,
}

struct ConceptEvidence {
    repetition_count: u32,
    structural_mentions: u32,
}

/// Tracks concept evidence across a bounded recent window and derives a
/// candidate theme label from it. Never unbounded: `window` never exceeds
/// `window_size` entries regardless of how many segments are recorded.
pub struct ThemeTracker {
    window: VecDeque<Vec<String>>,
    window_size: usize,
    structural_hits: HashMap<String, u32>,
    repetition_threshold: u32,
    structural_threshold: u32,
}

impl Default for ThemeTracker {
    fn default() -> Self {
        Self::new(
            DEFAULT_THEME_WINDOW,
            DEFAULT_REPETITION_THRESHOLD,
            DEFAULT_STRUCTURAL_THRESHOLD,
        )
    }
}

impl ThemeTracker {
    pub fn new(window_size: usize, repetition_threshold: u32, structural_threshold: u32) -> Self {
        Self {
            window: VecDeque::with_capacity(window_size),
            window_size,
            structural_hits: HashMap::new(),
            repetition_threshold,
            structural_threshold,
        }
    }

    /// Record one segment's text plus whatever structural kinds were
    /// detected in it (from [`crate::detection::detect_elements`]) - the
    /// caller decides what counts as "structural" for this call by
    /// passing the detection kinds it already computed, so this tracker
    /// never re-runs detection itself.
    pub fn record(&mut self, text: &str, detected_kinds: &[SermonElementKind]) {
        let concepts = extract_concepts(text);

        if self.window.len() == self.window_size {
            if let Some(oldest) = self.window.pop_front() {
                // Structural hits are cumulative for the service, not
                // windowed - once a concept has been raised in an explicit
                // point/definition, that evidence doesn't expire the way
                // raw repetition does.
                let _ = oldest;
            }
        }

        if detected_kinds.iter().any(|k| is_structural_signal(*k)) {
            for concept in &concepts {
                *self.structural_hits.entry(concept.clone()).or_insert(0) += 1;
            }
        }

        self.window.push_back(concepts);
    }

    fn evidence(&self) -> HashMap<String, ConceptEvidence> {
        let mut counts: HashMap<String, u32> = HashMap::new();
        for concepts in &self.window {
            for concept in concepts {
                *counts.entry(concept.clone()).or_insert(0) += 1;
            }
        }
        counts
            .into_iter()
            .map(|(concept, repetition_count)| {
                let structural_mentions = self.structural_hits.get(&concept).copied().unwrap_or(0);
                (
                    concept,
                    ConceptEvidence {
                        repetition_count,
                        structural_mentions,
                    },
                )
            })
            .collect()
    }

    /// The current best theme candidate, or `None` when no concept yet
    /// meets both the repetition and structural thresholds - "do not
    /// pretend certainty too early" (spec section 15).
    pub fn current_candidate(&self) -> Option<ThemeCandidate> {
        let evidence = self.evidence();
        let mut qualifying: Vec<(&String, &ConceptEvidence)> = evidence
            .iter()
            .filter(|(_, e)| {
                e.repetition_count >= self.repetition_threshold
                    && e.structural_mentions >= self.structural_threshold
            })
            .collect();
        if qualifying.is_empty() {
            return None;
        }
        qualifying.sort_by(|a, b| {
            let score_a = a.1.repetition_count + a.1.structural_mentions * 3;
            let score_b = b.1.repetition_count + b.1.structural_mentions * 3;
            score_b.cmp(&score_a).then_with(|| a.0.cmp(b.0)) // deterministic tiebreak
        });

        let (top_concept, top_evidence) = qualifying[0];
        let top_score = top_evidence.repetition_count + top_evidence.structural_mentions * 3;

        let label = if let Some((second_concept, second_evidence)) = qualifying.get(1) {
            let second_score =
                second_evidence.repetition_count + second_evidence.structural_mentions * 3;
            if top_score > 0 && (second_score as f64) / (top_score as f64) >= 0.6 {
                format!("{top_concept} and {second_concept}")
            } else {
                top_concept.clone()
            }
        } else {
            top_concept.clone()
        };

        let confidence = (0.5
            + 0.05 * top_evidence.repetition_count as f64
            + 0.1 * top_evidence.structural_mentions as f64)
            .min(0.95);

        Some(ThemeCandidate {
            label,
            confidence,
            repetition_count: top_evidence.repetition_count,
            structural_mentions: top_evidence.structural_mentions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repetition_alone_never_produces_a_candidate() {
        let mut tracker = ThemeTracker::default();
        for _ in 0..10 {
            tracker.record("Faith faith faith is mentioned again and again.", &[]);
        }
        assert!(
            tracker.current_candidate().is_none(),
            "repetition without any structural mention must never become a theme candidate"
        );
    }

    #[test]
    fn structural_mention_plus_repetition_produces_a_candidate() {
        let mut tracker = ThemeTracker::default();
        tracker.record(
            "My first point is that faith comes by hearing.",
            &[SermonElementKind::MainPoint],
        );
        tracker.record("Faith is central to everything we do.", &[]);
        tracker.record("We must walk by faith, not by sight.", &[]);

        let candidate = tracker
            .current_candidate()
            .expect("repeated concept with a structural mention should be a candidate");
        assert_eq!(candidate.label, "faith");
        assert!(candidate.confidence > 0.5 && candidate.confidence < 1.0);
    }

    #[test]
    fn theme_evolves_as_a_close_second_concept_gains_evidence() {
        let mut tracker = ThemeTracker::default();
        tracker.record(
            "My first point is that faith comes by hearing.",
            &[SermonElementKind::MainPoint],
        );
        tracker.record("Faith is central to everything we do.", &[]);
        tracker.record("We must walk by faith, not by sight.", &[]);
        let early = tracker.current_candidate().unwrap();
        assert_eq!(early.label, "faith");

        tracker.record(
            "The second thing is that obedience must follow faith.",
            &[SermonElementKind::MainPoint],
        );
        tracker.record("Obedience is not optional for the believer.", &[]);
        tracker.record("True obedience requires faith and obedience together.", &[]);

        let later = tracker.current_candidate().unwrap();
        assert!(
            later.label.contains("faith") && later.label.contains("obedience"),
            "a close second concept should combine into the theme label: {}",
            later.label
        );
    }

    #[test]
    fn window_never_grows_unbounded_across_ten_thousand_segments() {
        let mut tracker = ThemeTracker::new(40, 3, 1);
        for i in 0..10_000 {
            tracker.record(&format!("segment number {i} about faith and grace"), &[]);
        }
        assert!(tracker.window.len() <= 40);
    }

    #[test]
    fn generic_stopwords_never_become_a_theme_candidate() {
        let mut tracker = ThemeTracker::default();
        for _ in 0..10 {
            tracker.record(
                "This is that and this with that and this today.",
                &[SermonElementKind::MainPoint],
            );
        }
        assert!(tracker.current_candidate().is_none());
    }
}
