//! Cross-Domain Intelligence orchestration (Phase 2.4) - the correlation-layer
//! counterpart to `music.rs`/`sermon.rs`. Deliberately Tauri-agnostic (plain
//! functions over domain types, no `AppHandle`/`State`), matching the
//! established per-domain orchestration-module pattern.
//!
//! `CrossDomainCorrelationEngine` (`core/intelligence::cross_domain`) is not
//! an `IntelligenceEngine` and is never registered into
//! `IntelligenceEngineRegistry` (see that type's own module docs) - it reads
//! an already-built `IntelligenceContext` (in particular,
//! `context.recent_findings`, spanning every domain) and produces
//! [`IntelligenceCorrelation`]s, never findings. This module's job is only
//! to call it and queue the result into `AppState.correlation_queue`,
//! exactly mirroring `music::analyze_and_queue`/`sermon::analyze_and_queue`'s
//! shape for a different element type.

use cip_core_intelligence::{
    CorrelationQueue, CorrelationQueueAddOutcome, CrossDomainCorrelationEngine,
    IntelligenceContext, IntelligenceCorrelation,
};

/// Run the correlation rule engine against `context` and queue any
/// genuinely new correlations - the plain, directly-testable core of
/// `commands::analyze_cross_domain`. Has no way to create a
/// `PresentationItem`, mutate a source finding, or call another engine: it
/// only ever calls `CrossDomainCorrelationEngine::analyze` and
/// `CorrelationQueue::add`.
pub fn analyze_and_queue(
    engine: &CrossDomainCorrelationEngine,
    context: &IntelligenceContext,
    correlations: &mut CorrelationQueue,
) -> Vec<IntelligenceCorrelation> {
    let produced = engine.analyze(context);
    let mut queued = Vec::new();
    for correlation in produced {
        if correlations.add(correlation.clone()) == CorrelationQueueAddOutcome::Added {
            queued.push(correlation);
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

    fn finding(
        service_id: Uuid,
        domain: IntelligenceDomain,
        kind: FindingKind,
        summary: &str,
        segment_ids: Vec<Uuid>,
    ) -> IntelligenceFinding {
        let mut f = IntelligenceFinding::new(
            service_id,
            domain,
            kind,
            AssertionLevel::Inferred,
            ConfidenceResult::new(0.9, ConfidenceSource::Heuristic, None),
            summary,
            "test_engine",
            "1.0",
        );
        f.transcript_segment_ids = segment_ids;
        f
    }

    #[test]
    fn analyze_and_queue_queues_a_scripture_sermon_correlation_from_real_findings() {
        let service_id = Uuid::new_v4();
        let seg = segment("for it is the power of God unto salvation", 0);

        let bible_finding = finding(
            service_id,
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            "ROM 1:16",
            vec![seg.id],
        );
        let sermon_finding = finding(
            service_id,
            IntelligenceDomain::Sermon,
            FindingKind::Sermon,
            "Supporting Scripture: ROM 1:16",
            vec![seg.id],
        );

        let context = IntelligenceContext::build(
            service_id,
            None,
            Some(seg.clone()),
            vec![seg],
            None,
            vec![bible_finding, sermon_finding],
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );

        let engine = CrossDomainCorrelationEngine::new();
        let mut correlations = CorrelationQueue::new();
        let queued = analyze_and_queue(&engine, &context, &mut correlations);

        assert!(!queued.is_empty());
        assert!(queued
            .iter()
            .any(|c| c.kind == cip_core_intelligence::CorrelationKind::ScriptureSermon));
        assert_eq!(correlations.pending().len(), queued.len());
    }

    #[test]
    fn analyze_and_queue_does_not_duplicate_a_repeated_identical_call() {
        let service_id = Uuid::new_v4();
        let seg = segment("for it is the power of God unto salvation", 0);

        let bible_finding = finding(
            service_id,
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            "ROM 1:16",
            vec![seg.id],
        );
        let sermon_finding = finding(
            service_id,
            IntelligenceDomain::Sermon,
            FindingKind::Sermon,
            "Supporting Scripture: ROM 1:16",
            vec![seg.id],
        );

        let context = IntelligenceContext::build(
            service_id,
            None,
            Some(seg.clone()),
            vec![seg],
            None,
            vec![bible_finding, sermon_finding],
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );

        let engine = CrossDomainCorrelationEngine::new();
        let mut correlations = CorrelationQueue::new();
        let first = analyze_and_queue(&engine, &context, &mut correlations);
        assert!(!first.is_empty());

        let second = analyze_and_queue(&engine, &context, &mut correlations);
        assert!(
            second.is_empty(),
            "an identical repeated analysis must not duplicate an already-pending correlation"
        );
    }

    #[test]
    fn analyze_and_queue_with_no_findings_yields_no_correlations_and_no_error() {
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

        let engine = CrossDomainCorrelationEngine::new();
        let mut correlations = CorrelationQueue::new();
        let queued = analyze_and_queue(&engine, &context, &mut correlations);
        assert!(queued.is_empty());
        assert!(correlations.is_empty());
    }

    /// Operator-workflow proof: a correlation queued from a real
    /// `analyze_and_queue` call can be reviewed/dismissed through the
    /// ordinary `CorrelationQueue` lifecycle, and doing so changes only its
    /// own status - `cross_domain.rs` has no dependency on
    /// `cip_core_presentation` at all, so there is no code path here
    /// capable of creating a `PresentationItem`.
    #[test]
    fn a_queued_correlation_can_be_dismissed_and_only_changes_its_own_status() {
        let service_id = Uuid::new_v4();
        let seg = segment("for it is the power of God unto salvation", 0);
        let bible_finding = finding(
            service_id,
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            "ROM 1:16",
            vec![seg.id],
        );
        let sermon_finding = finding(
            service_id,
            IntelligenceDomain::Sermon,
            FindingKind::Sermon,
            "Supporting Scripture: ROM 1:16",
            vec![seg.id],
        );
        let context = IntelligenceContext::build(
            service_id,
            None,
            Some(seg.clone()),
            vec![seg],
            None,
            vec![bible_finding, sermon_finding],
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );

        let engine = CrossDomainCorrelationEngine::new();
        let mut correlations = CorrelationQueue::new();
        let queued = analyze_and_queue(&engine, &context, &mut correlations);
        let id = queued[0].id;
        let pending_before = correlations.pending().len();

        correlations.dismiss(id).unwrap();
        assert_eq!(
            correlations.get(id).unwrap().status,
            cip_core_intelligence::FindingStatus::Rejected
        );
        assert_eq!(correlations.pending().len(), pending_before - 1);
    }
}
