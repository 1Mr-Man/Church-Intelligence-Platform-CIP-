//! Sermon Intelligence orchestration (Phase 2.3) - the sermon-domain
//! counterpart to `music.rs`. Deliberately Tauri-agnostic (plain
//! `&dyn IntelligenceEngine`/domain types, no `AppHandle`/`State`),
//! matching `content.rs`/`presentation.rs`/`music.rs`.
//!
//! Unlike Music/Bible, `SermonIntelligenceEngine` needs no database
//! connection or external provider at all - it is pure, deterministic,
//! in-process logic (`cip_core_sermon`) with nothing to configure. See
//! `docs/sermon-intelligence.md`.
//!
//! ## Two separate engine instances, on purpose
//!
//! `state::AppState.sermon_engine` (the instance every Sermon Tauri
//! command actually reads/writes) is a *different* `SermonIntelligenceEngine`
//! instance from the one `register_sermon_engine` puts into
//! `intelligence_registry`. This mirrors Phase 2.2's
//! `AppState.acoustic_music_engine` vs. `intelligence_registry`'s own
//! Music registration: the registry's copy exists only so
//! `get_intelligence_capabilities`/`IntelligenceEngineRegistry::analyze_all`
//! see a real, `Available` Sermon engine for architecture-level diagnostics
//! and failure-isolation symmetry with Bible/Music - nothing in this app
//! ever calls `intelligence_registry.resolve(IntelligenceDomain::Sermon)`
//! from a live command. `AppState.sermon_engine` is the actual,
//! accumulating-state instance every real transcript segment goes
//! through, so its `snapshot()` (theme/state/structure) stays consistent
//! across calls - a trait object resolved from the registry cannot be
//! downcast back to call `SermonIntelligenceEngine`'s own inherent
//! `snapshot()` method, the same limitation that motivated the acoustic
//! engine's dedicated field.

use cip_core_intelligence::{
    FindingQueue, IntelligenceContext, IntelligenceEngine, IntelligenceEngineRegistry,
    IntelligenceError, IntelligenceFinding, IntelligenceInput, QueueAddOutcome,
    SermonIntelligenceEngine,
};

/// Registers a `SermonIntelligenceEngine` into an already-built
/// `IntelligenceEngineRegistry` - mirrors `music::register_music_engine`.
/// Takes no external dependencies (unlike Bible/Music's provider-backed
/// registration) since the Sermon engine needs none.
pub fn register_sermon_engine(
    registry: &mut IntelligenceEngineRegistry,
) -> Result<(), IntelligenceError> {
    registry.register(Box::new(SermonIntelligenceEngine::new()))
}

/// Calls `engine.analyze` and queues any genuinely new findings - the
/// plain, directly-testable core of `commands::analyze_sermon_transcript`.
/// Kept as its own copy of `music::analyze_and_queue`'s identical logic
/// (per this codebase's established per-domain-module convention: each
/// orchestration module is self-contained) rather than a cross-module
/// call. Has no way to create a `PresentationItem` or any other side
/// effect - it only ever calls `IntelligenceEngine::analyze` and
/// `FindingQueue::add`.
pub fn analyze_and_queue(
    engine: &dyn IntelligenceEngine,
    input: &IntelligenceInput,
    context: &IntelligenceContext,
    findings: &mut FindingQueue,
) -> Result<Vec<IntelligenceFinding>, IntelligenceError> {
    let result = engine.analyze(input, context)?;
    let mut queued = Vec::new();
    for finding in result.findings {
        if findings.add(finding.clone()) == QueueAddOutcome::Added {
            queued.push(finding);
        }
    }
    Ok(queued)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use cip_core_intelligence::{ContextBounds, IntelligenceDomain};
    use uuid::Uuid;

    fn segment(text: &str) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence: 0,
            text: text.to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(1.0, ConfidenceSource::Human, None),
            start_ms: 0,
            end_ms: 0,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    fn input_and_context(service_id: Uuid, text: &str) -> (IntelligenceInput, IntelligenceContext) {
        let seg = segment(text);
        let context = IntelligenceContext::build(
            service_id,
            None,
            Some(seg.clone()),
            vec![seg.clone()],
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );
        (IntelligenceInput::new(service_id, seg), context)
    }

    #[test]
    fn register_sermon_engine_makes_it_resolvable_by_domain() {
        let mut registry = IntelligenceEngineRegistry::new();
        register_sermon_engine(&mut registry).unwrap();
        assert!(registry.resolve(IntelligenceDomain::Sermon).is_some());
    }

    #[test]
    fn analyze_and_queue_queues_a_main_point_finding() {
        let engine = SermonIntelligenceEngine::new();
        let (input, context) = input_and_context(
            Uuid::new_v4(),
            "My first point is that faith comes by hearing.",
        );
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();

        assert!(queued.iter().any(|f| f.summary.starts_with("Main Point:")));
        assert!(queued
            .iter()
            .all(|f| f.domain == IntelligenceDomain::Sermon));
    }

    #[test]
    fn analyze_and_queue_does_not_duplicate_a_repeated_identical_call() {
        let engine = SermonIntelligenceEngine::new();
        let mut findings = FindingQueue::new();
        let service_id = Uuid::new_v4();

        let (input1, context1) =
            input_and_context(service_id, "My first point is that faith comes by hearing.");
        let first = analyze_and_queue(&engine, &input1, &context1, &mut findings).unwrap();
        assert!(!first.is_empty());

        let (input2, context2) =
            input_and_context(service_id, "My first point is that faith comes by hearing.");
        let second = analyze_and_queue(&engine, &input2, &context2, &mut findings).unwrap();
        assert!(
            second.is_empty(),
            "an identical repeated segment must not duplicate an already-pending finding"
        );
    }

    /// Operator-workflow proof (spec section 54): a finding queued from a
    /// real `analyze_and_queue` call can be accepted through the ordinary
    /// `FindingQueue` lifecycle, and doing so changes only its own status -
    /// `sermon.rs` has no dependency on `cip_core_presentation` at all (see
    /// this module's imports), so there is no code path here capable of
    /// creating a `PresentationItem`.
    #[test]
    fn a_queued_sermon_finding_can_be_accepted_and_only_changes_its_own_status() {
        let engine = SermonIntelligenceEngine::new();
        let (input, context) = input_and_context(
            Uuid::new_v4(),
            "My first point is that faith comes by hearing.",
        );
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();
        let id = queued[0].id;
        let pending_before = findings.pending().len();

        findings.accept(id).unwrap();
        assert_eq!(
            findings.get(id).unwrap().status,
            cip_core_intelligence::FindingStatus::Accepted
        );
        assert_eq!(
            findings.pending().len(),
            pending_before - 1,
            "an accepted finding is no longer pending operator review"
        );
    }

    #[test]
    fn a_queued_sermon_finding_can_be_rejected_and_stays_out_of_pending() {
        let engine = SermonIntelligenceEngine::new();
        let (input, context) = input_and_context(
            Uuid::new_v4(),
            "My first point is that faith comes by hearing.",
        );
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();
        let id = queued[0].id;
        let pending_before = findings.pending().len();

        findings.reject(id).unwrap();
        assert_eq!(
            findings.get(id).unwrap().status,
            cip_core_intelligence::FindingStatus::Rejected
        );
        assert_eq!(findings.pending().len(), pending_before - 1);
    }

    #[test]
    fn plain_prose_produces_no_findings_and_no_error() {
        let engine = SermonIntelligenceEngine::new();
        let (input, context) = input_and_context(Uuid::new_v4(), "Good morning, church.");
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();
        assert!(queued.is_empty());
    }
}
