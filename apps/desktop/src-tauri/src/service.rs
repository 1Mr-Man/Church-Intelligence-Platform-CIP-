//! Service Intelligence orchestration - the service-phase-domain
//! counterpart to `music.rs`/`sermon.rs`. Deliberately Tauri-agnostic
//! (plain `&dyn IntelligenceEngine`/domain types, no `AppHandle`/`State`),
//! matching `content.rs`/`presentation.rs`/`music.rs`/`sermon.rs`.
//!
//! Unlike Music/Bible, `ServiceIntelligenceEngine` needs no database
//! connection or external provider at all - it is pure, deterministic,
//! in-process logic (`cip_core_intelligence::service_adapter`) with
//! nothing to configure. See `docs/service-intelligence.md`.
//!
//! ## Two separate engine instances, on purpose
//!
//! `state::AppState.service_engine` (the instance every Service Tauri
//! command actually reads/writes) is a *different* `ServiceIntelligenceEngine`
//! instance from the one `register_service_engine` puts into
//! `intelligence_registry`. This mirrors Phase 2.3's
//! `AppState.sermon_engine` vs. `intelligence_registry`'s own separate
//! Sermon registration exactly: the registry's copy exists only so
//! `get_intelligence_capabilities`/`IntelligenceEngineRegistry::analyze_all`
//! see a real, `Available` Service engine for architecture-level
//! diagnostics and failure-isolation symmetry with Bible/Music/Sermon -
//! nothing in this app ever calls
//! `intelligence_registry.resolve(IntelligenceDomain::Service)` from a
//! live command. `AppState.service_engine` is the actual, accumulating-
//! state instance every real transcript segment goes through, so its
//! `snapshot()` (current phase/transition history) stays consistent
//! across calls.
//!
//! ## Transcript freshness: computed here, not inside the engine
//!
//! `cip_core_intelligence::service_adapter::ServiceIntelligenceEngine::analyze`
//! deliberately never calls `Utc::now()` to make a decision (spec section
//! 34) - it is a pure function of `(input, context)`. "Has the transcript
//! gone stale" is a genuinely wall-clock-dependent question, so it is
//! answered here instead, in the Tauri orchestration layer, as a plain
//! function of two explicitly-passed timestamps ([`transcript_freshness`]) -
//! never a hidden clock call baked into the deterministic core engine.

use chrono::{DateTime, Utc};
use cip_core_intelligence::{
    FindingQueue, IntelligenceDomain, IntelligenceEngine, IntelligenceEngineRegistry,
    IntelligenceError, IntelligenceFinding, IntelligenceInput, ServiceIntelligenceEngine,
    ServicePhase,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How long the transcript may go without a new final segment before it
/// is considered stale (spec section 41's acceptance scenario) - long
/// enough to absorb an ordinary pause for reflection or a long
/// congregational reading, short enough to still be a meaningful signal
/// within a live service.
pub const TRANSCRIPT_STALE_AFTER_SECONDS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum TranscriptFreshness {
    /// No transcript segment has been received yet this service.
    Unknown,
    Fresh,
    /// `seconds_since` since the last final transcript segment was
    /// received - never itself a reason to end or pause the service (spec
    /// section 41: "do NOT automatically end the service"). The
    /// container's `rename_all` only renames the variant tag itself, not
    /// this variant's own field - it needs its own `rename_all` to
    /// produce `secondsSince` in JSON, matching every other IPC struct in
    /// this codebase.
    #[serde(rename_all = "camelCase")]
    Stale {
        seconds_since: i64,
    },
}

/// Pure function of two explicit timestamps - never calls `Utc::now()`
/// itself, so it is exactly as deterministic/testable as everything in
/// `cip_core_intelligence::service_adapter`, even though what it *answers*
/// is inherently wall-clock-dependent. The real caller
/// (`commands::get_service_intelligence_state`) supplies `now` from
/// `Utc::now()` at the moment of the command call; tests supply an
/// explicit value.
pub fn transcript_freshness(
    last_transcript_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> TranscriptFreshness {
    let Some(last) = last_transcript_at else {
        return TranscriptFreshness::Unknown;
    };
    let elapsed = (now - last).num_seconds().max(0);
    if elapsed >= TRANSCRIPT_STALE_AFTER_SECONDS {
        TranscriptFreshness::Stale {
            seconds_since: elapsed,
        }
    } else {
        TranscriptFreshness::Fresh
    }
}

/// Registers a `ServiceIntelligenceEngine` into an already-built
/// `IntelligenceEngineRegistry` - mirrors `sermon::register_sermon_engine`.
/// Takes no external dependencies since the Service engine needs none.
pub fn register_service_engine(
    registry: &mut IntelligenceEngineRegistry,
) -> Result<(), IntelligenceError> {
    registry.register(Box::new(ServiceIntelligenceEngine::new()))
}

/// Calls `engine.analyze` and queues any genuinely new findings - the
/// plain, directly-testable core of `commands::analyze_service_transcript`.
/// Kept as its own copy of `music::analyze_and_queue`'s identical logic
/// (per this codebase's established per-domain-module convention). Has no
/// way to create a `PresentationItem` or any other side effect - it only
/// ever calls `IntelligenceEngine::analyze` and `FindingQueue::add`.
pub fn analyze_and_queue(
    engine: &dyn IntelligenceEngine,
    input: &IntelligenceInput,
    context: &cip_core_intelligence::IntelligenceContext,
    findings: &mut FindingQueue,
) -> Result<Vec<IntelligenceFinding>, IntelligenceError> {
    let result = engine.analyze(input, context)?;
    let mut queued = Vec::new();
    for finding in result.findings {
        if findings.add(finding.clone()) == cip_core_intelligence::QueueAddOutcome::Added {
            queued.push(finding);
        }
    }
    Ok(queued)
}

/// Parse a phase name (matching [`ServicePhase::label`], case-insensitive)
/// from operator input - the one place a plain string from the frontend
/// becomes a real `ServicePhase`, so a typo/unknown value is rejected with
/// a clear error rather than silently becoming `Unknown`.
pub fn parse_service_phase(value: &str) -> Option<ServicePhase> {
    let normalized = value.trim().to_uppercase().replace([' ', '-'], "_");
    [
        ServicePhase::Unknown,
        ServicePhase::Opening,
        ServicePhase::Worship,
        ServicePhase::Prayer,
        ServicePhase::ScriptureReading,
        ServicePhase::Sermon,
        ServicePhase::Offering,
        ServicePhase::Announcement,
        ServicePhase::Closing,
    ]
    .into_iter()
    .find(|&phase| phase.label() == normalized)
}

/// Apply an explicit operator action (mark or correct) and queue the
/// resulting finding - the plain, directly-testable core of
/// `commands::mark_service_phase`/`commands::correct_service_phase`.
/// `is_correction` only changes the finding's summary/evidence wording
/// (spec sections 19-20 distinguish "mark" from "correct" in intent, not
/// in mechanism - both transition immediately and are always `Observed`).
pub fn apply_operator_action(
    engine: &ServiceIntelligenceEngine,
    service_id: Uuid,
    new_phase: ServicePhase,
    note: Option<&str>,
    is_correction: bool,
    findings: &mut FindingQueue,
) -> IntelligenceFinding {
    let finding = engine.apply_operator_action(service_id, new_phase, note, is_correction);
    findings.add(finding.clone());
    finding
}

/// Service-domain findings whose summary marks them as an anomaly
/// (`"Anomaly #<n>: ..."`, from `service_adapter::anomaly_finding_for_unexpected_transition`) -
/// the filter behind `commands::list_service_anomalies`. Anomalies reuse
/// the exact same `FindingQueue`/`FindingStatus` lifecycle every other
/// finding kind already has (spec section 27's persistence-reuse
/// preference): "acknowledging" one is nothing more than
/// `FindingQueue::accept`, no bespoke anomaly-tracking system needed.
pub fn is_anomaly_finding(finding: &IntelligenceFinding) -> bool {
    finding.domain == IntelligenceDomain::Service && finding.summary.starts_with("Anomaly")
}

/// Service-domain findings that represent an actual phase transition
/// (`"Service phase changed #<n>: ..."`) - the filter behind
/// `commands::list_service_transitions`, distinct from anomaly findings.
pub fn is_transition_finding(finding: &IntelligenceFinding) -> bool {
    finding.domain == IntelligenceDomain::Service
        && finding.summary.starts_with("Service phase changed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use cip_core_intelligence::{ContextBounds, IntelligenceContext};
    use cip_core_service::ServiceStatus;

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
            Some(ServiceStatus::Started),
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
    fn register_service_engine_makes_it_resolvable_by_domain() {
        let mut registry = IntelligenceEngineRegistry::new();
        register_service_engine(&mut registry).unwrap();
        assert!(registry.resolve(IntelligenceDomain::Service).is_some());
    }

    #[test]
    fn analyze_and_queue_queues_a_phase_transition_finding() {
        let engine = ServiceIntelligenceEngine::new();
        let (input, context) = input_and_context(Uuid::new_v4(), "Let us pray.");
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();

        assert!(queued.iter().any(|f| f.summary.contains("-> PRAYER")));
        assert!(queued
            .iter()
            .all(|f| f.domain == IntelligenceDomain::Service));
    }

    #[test]
    fn analyze_and_queue_does_not_duplicate_a_repeated_identical_call() {
        let engine = ServiceIntelligenceEngine::new();
        let mut findings = FindingQueue::new();
        let service_id = Uuid::new_v4();

        let (input1, context1) = input_and_context(service_id, "Let us pray.");
        let first = analyze_and_queue(&engine, &input1, &context1, &mut findings).unwrap();
        assert!(!first.is_empty());

        // The engine is already in Prayer, so the identical cue is now a
        // no-op (no *change*) - proving no duplicate spam from a repeated
        // segment saying the same thing while already in that phase.
        let (input2, context2) = input_and_context(service_id, "Let us pray.");
        let second = analyze_and_queue(&engine, &input2, &context2, &mut findings).unwrap();
        assert!(second.is_empty());
    }

    #[test]
    fn plain_prose_produces_no_findings_and_no_error() {
        let engine = ServiceIntelligenceEngine::new();
        let (input, context) =
            input_and_context(Uuid::new_v4(), "Good afternoon, everyone here today.");
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();
        assert!(queued.is_empty());
    }

    #[test]
    fn parse_service_phase_accepts_labels_case_insensitively_and_rejects_garbage() {
        assert_eq!(parse_service_phase("sermon"), Some(ServicePhase::Sermon));
        assert_eq!(
            parse_service_phase("SCRIPTURE_READING"),
            Some(ServicePhase::ScriptureReading)
        );
        assert_eq!(
            parse_service_phase("scripture reading"),
            Some(ServicePhase::ScriptureReading)
        );
        assert_eq!(parse_service_phase("not-a-real-phase"), None);
    }

    #[test]
    fn apply_operator_action_queues_an_observed_finding() {
        let engine = ServiceIntelligenceEngine::new();
        let mut findings = FindingQueue::new();
        let service_id = Uuid::new_v4();
        let finding = apply_operator_action(
            &engine,
            service_id,
            ServicePhase::Worship,
            None,
            false,
            &mut findings,
        );
        assert_eq!(
            finding.assertion_level,
            cip_core_intelligence::AssertionLevel::Observed
        );
        assert_eq!(findings.pending().len(), 1);
    }

    /// Operator-workflow proof (mirrors `music.rs`/`sermon.rs`'s own
    /// equivalent test): an operator action here has no dependency on
    /// `cip_core_presentation` at all, so there is no code path in this
    /// module capable of creating a `PresentationItem`.
    #[test]
    fn a_queued_transition_can_be_accepted_and_only_changes_its_own_status() {
        let engine = ServiceIntelligenceEngine::new();
        let (input, context) = input_and_context(Uuid::new_v4(), "Let us pray.");
        let mut findings = FindingQueue::new();
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();
        let id = queued[0].id;

        findings.accept(id).unwrap();
        assert_eq!(
            findings.get(id).unwrap().status,
            cip_core_intelligence::FindingStatus::Accepted
        );
        assert!(findings.pending().is_empty());
    }

    #[test]
    fn is_transition_finding_and_is_anomaly_finding_partition_service_findings() {
        let engine = ServiceIntelligenceEngine::new();
        let sid = Uuid::new_v4();
        let mut findings = FindingQueue::new();
        let (input1, context1) = input_and_context(sid, "Let us prepare our offering.");
        analyze_and_queue(&engine, &input1, &context1, &mut findings).unwrap();
        let (input2, context2) = input_and_context(sid, "Welcome to today's service.");
        analyze_and_queue(&engine, &input2, &context2, &mut findings).unwrap();

        let all = findings.all();
        assert!(all.iter().any(|f| is_transition_finding(f)));
        assert!(all.iter().any(|f| is_anomaly_finding(f)));
        assert!(all
            .iter()
            .all(|f| !(is_transition_finding(f) && is_anomaly_finding(f))));
    }

    // --- transcript_freshness ------------------------------------------------

    #[test]
    fn no_prior_transcript_is_unknown_freshness() {
        assert_eq!(
            transcript_freshness(None, Utc::now()),
            TranscriptFreshness::Unknown
        );
    }

    #[test]
    fn a_recent_transcript_is_fresh() {
        let now = Utc::now();
        let last = now - chrono::Duration::seconds(5);
        assert_eq!(
            transcript_freshness(Some(last), now),
            TranscriptFreshness::Fresh
        );
    }

    #[test]
    fn a_transcript_older_than_the_threshold_is_stale_but_never_ends_the_service() {
        let now = Utc::now();
        let last = now - chrono::Duration::seconds(42);
        assert_eq!(
            transcript_freshness(Some(last), now),
            TranscriptFreshness::Stale { seconds_since: 42 }
        );
        // The type itself has no lifecycle-mutating capability - `Stale`
        // is only ever a value returned to a caller, never something that
        // can call `end_service`/`pause_service` on its own.
    }

    #[test]
    fn recovery_after_a_new_segment_returns_to_fresh() {
        let now = Utc::now();
        let just_now = now - chrono::Duration::seconds(1);
        assert_eq!(
            transcript_freshness(Some(just_now), now),
            TranscriptFreshness::Fresh
        );
    }

    #[test]
    fn transcript_freshness_has_no_way_to_touch_service_lifecycle() {
        // Type-level proof (spec section 41: "do NOT automatically end the
        // service" while the transcript is stale): `transcript_freshness`
        // takes no `AppState`/`ServiceSession` reference of any kind and
        // returns a plain value - there is no code path here capable of
        // pausing or ending a service, however stale the transcript gets.
        let now = Utc::now();
        let very_stale = now - chrono::Duration::seconds(3600);
        assert_eq!(
            transcript_freshness(Some(very_stale), now),
            TranscriptFreshness::Stale {
                seconds_since: 3600
            }
        );
    }

    // --- canonical full-service acceptance scenario (spec section 38) --------
    //
    // A short, project-authored synthetic transcript (never copyrighted
    // sermon content) walking through every phase this module recognizes,
    // in order, asserting every expected transition fires and the engine
    // ends in the expected final phase - mirrors
    // `sermon_adapter::tests::phase_2_3_canonical_sermon_acceptance_scenario`'s
    // shape.

    #[test]
    fn canonical_full_service_phase_acceptance_scenario() {
        let engine = ServiceIntelligenceEngine::new();
        let service_id = Uuid::new_v4();
        let mut findings = FindingQueue::new();

        let segments = [
            (
                "Good morning everyone. Welcome to today's service.",
                "OPENING",
            ),
            ("Let's worship the Lord together.", "WORSHIP"),
            ("Let us pray.", "PRAYER"),
            ("Turn with me to Romans chapter eight.", "SCRIPTURE_READING"),
            (
                "Today I want to speak to you about the life of the Spirit.",
                "SERMON",
            ),
            ("Let us prepare our offering.", "OFFERING"),
            ("Here are the announcements.", "ANNOUNCEMENT"),
            (
                "May the grace of our Lord Jesus Christ be with you all.",
                "CLOSING",
            ),
        ];

        let mut all_findings = Vec::new();
        for (text, _) in &segments {
            let (input, context) = input_and_context(service_id, text);
            let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();
            all_findings.extend(queued);
        }

        for (_, expected_phase) in &segments {
            assert!(
                all_findings
                    .iter()
                    .any(|f| f.summary.contains(&format!("-> {expected_phase}"))),
                "expected a transition into {expected_phase} somewhere in the canonical scenario"
            );
        }

        assert_eq!(
            engine.snapshot().phase,
            cip_core_intelligence::ServicePhase::Closing,
            "the engine must end the scenario in the final expected phase"
        );
        assert_eq!(
            engine.snapshot().transition_count,
            segments.len() as u32,
            "every segment in this scenario should have produced exactly one ordinary transition"
        );

        // Every transition in this canonical, well-ordered scenario is the
        // conventional next step - never flagged as an anomaly.
        assert!(
            all_findings.iter().all(|f| !is_anomaly_finding(f)),
            "a well-ordered service must never produce an anomaly finding"
        );

        // No finding may ever be `Generated` - Service Intelligence infers
        // phase from evidence, it never synthesizes content.
        assert!(all_findings
            .iter()
            .all(|f| f.assertion_level != cip_core_intelligence::AssertionLevel::Generated));

        // No code path here has any way to create a `PresentationItem` -
        // `service.rs` has no dependency on `cip_core_presentation` at
        // all (see this module's imports).
        assert!(all_findings
            .iter()
            .all(|f| f.status == cip_core_intelligence::FindingStatus::Detected));
    }

    /// Operator-correction acceptance scenario (spec section 40): the
    /// system detects Sermon, the operator corrects it to Worship, and the
    /// correction supersedes (rejects) the prior pending transition
    /// finding without deleting it - fully auditable via `findings.all()`.
    #[test]
    fn operator_correction_acceptance_scenario() {
        let engine = ServiceIntelligenceEngine::new();
        let service_id = Uuid::new_v4();
        let mut findings = FindingQueue::new();

        let (input, context) =
            input_and_context(service_id, "Today I want to speak to you about faith.");
        let queued = analyze_and_queue(&engine, &input, &context, &mut findings).unwrap();
        let system_detected_id = queued[0].id;
        assert_eq!(
            findings.get(system_detected_id).unwrap().status,
            cip_core_intelligence::FindingStatus::Detected
        );

        // Operator correction: reject the superseded finding, queue a new
        // Observed one.
        let superseded: Vec<Uuid> = findings
            .pending()
            .iter()
            .filter(|f| f.service_id == service_id && is_transition_finding(f))
            .map(|f| f.id)
            .collect();
        for id in superseded {
            findings.reject(id).unwrap();
        }
        let correction = apply_operator_action(
            &engine,
            service_id,
            cip_core_intelligence::ServicePhase::Worship,
            Some("actually still worship"),
            true,
            &mut findings,
        );

        assert_eq!(
            findings.get(system_detected_id).unwrap().status,
            cip_core_intelligence::FindingStatus::Rejected,
            "the superseded system-detected finding is rejected, never deleted"
        );
        assert_eq!(
            correction.assertion_level,
            cip_core_intelligence::AssertionLevel::Observed
        );
        assert_eq!(
            engine.snapshot().phase,
            cip_core_intelligence::ServicePhase::Worship
        );
        assert_eq!(
            findings.all().len(),
            2,
            "both the original detection and the correction remain in the full history"
        );
    }
}

#[cfg(test)]
mod ipc_shape_tests {
    use super::*;

    /// `TranscriptFreshness` is the one type in this module sent over IPC
    /// (via `commands::ServiceIntelligenceSummary`) - this locks in the
    /// camelCase field naming `domain/service.ts`'s `TranscriptFreshness`
    /// mirror expects, matching every other IPC struct in this codebase.
    #[test]
    fn transcript_freshness_serializes_with_camel_case_fields() {
        let stale = TranscriptFreshness::Stale { seconds_since: 42 };
        let json = serde_json::to_value(stale).unwrap();
        assert_eq!(json["status"], "stale");
        assert_eq!(json["secondsSince"], 42);
        assert!(json.get("seconds_since").is_none());
    }
}
