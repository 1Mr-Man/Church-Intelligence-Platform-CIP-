//! Content Intelligence orchestration (Phase 2.7, per the authoritative
//! Phase 2 roadmap) - the `ContentCandidate` counterpart to
//! `cross_domain.rs`. Deliberately Tauri-agnostic (plain functions over
//! domain types, no `AppHandle`/`State`), matching the established
//! per-domain orchestration-module pattern.
//!
//! `ContentIntelligenceEngine` (`core/intelligence::content_intelligence`)
//! is not an `IntelligenceEngine` and is never registered into
//! `IntelligenceEngineRegistry` (see that type's own module docs, which
//! mirror `CrossDomainCorrelationEngine`'s identical precedent) - it reads
//! an already-built `IntelligenceContext` (in particular,
//! `context.recent_findings`) and produces [`ContentCandidate`]s, never
//! findings. This module's job is only to call it and queue the result
//! into `AppState.content_candidate_queue`, exactly mirroring
//! `cross_domain::analyze_and_queue`'s shape for a different element type.

use cip_core_intelligence::{
    ContentCandidate, ContentCandidateQueue, ContentCandidateQueueAddOutcome,
    ContentIntelligenceEngine, IntelligenceContext,
};

/// Run the content-intelligence layer against `context` and queue any
/// genuinely new candidates - the plain, directly-testable core of
/// `commands::analyze_content_intelligence`. Has no way to create a
/// `PresentationItem`, mutate a source finding, or call another engine: it
/// only ever calls `ContentIntelligenceEngine::analyze` and
/// `ContentCandidateQueue::add`.
pub fn analyze_and_queue(
    engine: &ContentIntelligenceEngine,
    context: &IntelligenceContext,
    candidates: &mut ContentCandidateQueue,
) -> Vec<ContentCandidate> {
    let produced = engine.analyze(context);
    let mut queued = Vec::new();
    for candidate in produced {
        if candidates.add(candidate.clone()) == ContentCandidateQueueAddOutcome::Added {
            queued.push(candidate);
        }
    }
    queued
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use cip_core_intelligence::{
        AssertionLevel, ContextBounds, FindingKind, IntelligenceDomain, IntelligenceFinding,
    };
    use uuid::Uuid;

    fn segment(text: &str, sequence: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence,
            text: text.to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(1.0, ConfidenceSource::Human, None),
            start_ms: 0,
            end_ms: 0,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    fn theme_finding(service_id: Uuid, segment_id: Uuid) -> IntelligenceFinding {
        let mut f = IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Sermon,
            FindingKind::Sermon,
            AssertionLevel::Inferred,
            ConfidenceResult::new(0.8, ConfidenceSource::Heuristic, None),
            "Theme: faith and obedience",
            "sermon-core",
            "0.1.0",
        );
        f.transcript_segment_ids = vec![segment_id];
        f.evidence = vec![cip_core_intelligence::EvidenceSource::Context {
            description: "accumulated theme evidence".to_string(),
        }];
        f
    }

    #[test]
    fn analyze_and_queue_queues_a_theme_candidate_from_a_real_finding() {
        let service_id = Uuid::new_v4();
        let seg = segment("faith is central", 0);
        let finding = theme_finding(service_id, seg.id);

        let context = IntelligenceContext::build(
            service_id,
            None,
            Some(seg.clone()),
            vec![seg],
            None,
            vec![finding],
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );

        let engine = ContentIntelligenceEngine::new();
        let mut candidates = ContentCandidateQueue::new();
        let queued = analyze_and_queue(&engine, &context, &mut candidates);

        assert!(!queued.is_empty());
        assert_eq!(
            queued[0].candidate_type,
            cip_core_intelligence::ContentCandidateType::Theme
        );
        assert_eq!(candidates.pending().len(), queued.len());
    }

    #[test]
    fn analyze_and_queue_does_not_duplicate_a_repeated_identical_call() {
        let service_id = Uuid::new_v4();
        let seg = segment("faith is central", 0);
        let finding = theme_finding(service_id, seg.id);

        let context = IntelligenceContext::build(
            service_id,
            None,
            Some(seg.clone()),
            vec![seg],
            None,
            vec![finding],
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );

        let engine = ContentIntelligenceEngine::new();
        let mut candidates = ContentCandidateQueue::new();
        let first = analyze_and_queue(&engine, &context, &mut candidates);
        assert!(!first.is_empty());

        let second = analyze_and_queue(&engine, &context, &mut candidates);
        assert!(
            second.is_empty(),
            "an identical repeated analysis must not duplicate an already-pending candidate"
        );
    }

    #[test]
    fn analyze_and_queue_with_no_findings_yields_no_candidates_and_no_error() {
        let service_id = Uuid::new_v4();
        let seg = segment("Good morning, church.", 0);
        let context = IntelligenceContext::build(
            service_id,
            None,
            Some(seg.clone()),
            vec![seg],
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );

        let engine = ContentIntelligenceEngine::new();
        let mut candidates = ContentCandidateQueue::new();
        let queued = analyze_and_queue(&engine, &context, &mut candidates);
        assert!(queued.is_empty());
        assert!(candidates.is_empty());
    }

    /// Operator-workflow proof: a candidate queued from a real
    /// `analyze_and_queue` call can be accepted/rejected through the
    /// ordinary `ContentCandidateQueue` lifecycle, and doing so changes
    /// only its own status - `content_intelligence.rs` has no dependency
    /// on `cip_core_presentation` at all, so there is no code path here
    /// capable of creating a `PresentationItem`.
    #[test]
    fn a_queued_candidate_can_be_accepted_and_only_changes_its_own_status() {
        let service_id = Uuid::new_v4();
        let seg = segment("faith is central", 0);
        let finding = theme_finding(service_id, seg.id);
        let context = IntelligenceContext::build(
            service_id,
            None,
            Some(seg.clone()),
            vec![seg],
            None,
            vec![finding],
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );

        let engine = ContentIntelligenceEngine::new();
        let mut candidates = ContentCandidateQueue::new();
        let queued = analyze_and_queue(&engine, &context, &mut candidates);
        let id = queued[0].id;
        let pending_before = candidates.pending().len();

        candidates.accept(id).unwrap();
        assert_eq!(
            candidates.get(id).unwrap().status,
            cip_core_intelligence::FindingStatus::Accepted
        );
        assert_eq!(candidates.pending().len(), pending_before - 1);
    }

    #[test]
    fn a_queued_candidate_can_be_rejected_and_stays_out_of_pending() {
        let service_id = Uuid::new_v4();
        let seg = segment("faith is central", 0);
        let finding = theme_finding(service_id, seg.id);
        let context = IntelligenceContext::build(
            service_id,
            None,
            Some(seg.clone()),
            vec![seg],
            None,
            vec![finding],
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );

        let engine = ContentIntelligenceEngine::new();
        let mut candidates = ContentCandidateQueue::new();
        let queued = analyze_and_queue(&engine, &context, &mut candidates);
        let id = queued[0].id;

        candidates.reject(id).unwrap();
        assert_eq!(
            candidates.get(id).unwrap().status,
            cip_core_intelligence::FindingStatus::Rejected
        );
        assert!(candidates.pending().is_empty());
    }
}
