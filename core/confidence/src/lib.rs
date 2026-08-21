//! Confidence scoring contract shared across CIP's domains.
//!
//! Any subsystem that produces an uncertain result derived from AI or
//! heuristic inference (scripture detection, speech transcription, AI
//! suggestions) reports that uncertainty through [`ConfidenceResult`] so the
//! rest of the system - and, ultimately, the human operator - can apply a
//! single consistent policy for what to auto-accept, what to surface for
//! review, and what to discard.
//!
//! This crate defines the contract only. It has no knowledge of Bible text,
//! audio, or presentation - it is deliberately the most dependency-free
//! crate in the workspace so every other domain can depend on it without
//! creating a cycle.

use serde::{Deserialize, Serialize};

/// Where a confidence value originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceSource {
    /// Produced by pattern/heuristic matching (e.g. regex scripture detection).
    Heuristic,
    /// Produced by a machine learning model (speech, classifier, embeddings).
    Model,
    /// Produced or overridden by a human operator.
    Human,
}

/// A coarse, human-meaningful bucket for a raw confidence score.
///
/// UI code should generally branch on this rather than the raw `f32`, so the
/// thresholds live in exactly one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    Low,
    Medium,
    High,
}

impl ConfidenceLevel {
    /// Bucket a raw `0.0..=1.0` score into a [`ConfidenceLevel`].
    ///
    /// Thresholds are intentionally conservative for Phase 1: anything below
    /// 0.5 is `Low`, 0.5..0.8 is `Medium`, and 0.8+ is `High`. These will be
    /// revisited once real detectors produce calibrated scores.
    pub fn from_score(score: f32) -> Self {
        if score >= 0.8 {
            ConfidenceLevel::High
        } else if score >= 0.5 {
            ConfidenceLevel::Medium
        } else {
            ConfidenceLevel::Low
        }
    }
}

/// The result of any confidence-scored inference in the system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceResult {
    /// Raw score in the `0.0..=1.0` range. Callers must clamp before
    /// constructing this via [`ConfidenceResult::new`].
    pub score: f32,
    pub level: ConfidenceLevel,
    pub source: ConfidenceSource,
    /// Optional short machine/human-readable reason for the score
    /// (e.g. "exact regex match", "low audio SNR").
    pub reason: Option<String>,
}

impl ConfidenceResult {
    pub fn new(score: f32, source: ConfidenceSource, reason: impl Into<Option<String>>) -> Self {
        let clamped = score.clamp(0.0, 1.0);
        Self {
            score: clamped,
            level: ConfidenceLevel::from_score(clamped),
            source,
            reason: reason.into(),
        }
    }

    /// Whether this result is confident enough to act on without human
    /// confirmation. Phase 1 policy: only `High` auto-applies.
    pub fn is_auto_acceptable(&self) -> bool {
        self.level == ConfidenceLevel::High
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_scores_into_expected_levels() {
        assert_eq!(ConfidenceLevel::from_score(0.95), ConfidenceLevel::High);
        assert_eq!(ConfidenceLevel::from_score(0.65), ConfidenceLevel::Medium);
        assert_eq!(ConfidenceLevel::from_score(0.1), ConfidenceLevel::Low);
    }

    #[test]
    fn clamps_out_of_range_scores() {
        let over = ConfidenceResult::new(1.5, ConfidenceSource::Heuristic, None);
        let under = ConfidenceResult::new(-0.5, ConfidenceSource::Heuristic, None);
        assert_eq!(over.score, 1.0);
        assert_eq!(under.score, 0.0);
    }

    #[test]
    fn only_high_confidence_is_auto_acceptable() {
        let high = ConfidenceResult::new(0.9, ConfidenceSource::Model, None);
        let medium = ConfidenceResult::new(0.6, ConfidenceSource::Model, None);
        assert!(high.is_auto_acceptable());
        assert!(!medium.is_auto_acceptable());
    }
}
