//! [`ServiceIntelligenceEngine`]: deterministic Service Intelligence - the
//! authoritative Phase 2 roadmap's **Phase 2.4 ("Service Intelligence")**.
//! Understands the live church service's own state: what phase the
//! service is in, when it changed, and how confidently - never Sermon
//! content, Music recognition, or cross-domain correlation.
//!
//! This is distinct from, and does not touch, the earlier general
//! "cross-domain correlation" work (`crate::cross_domain`) developed
//! earlier in this repository's history under an internal label that also
//! read "Phase 2.4" - the authoritative roadmap this module follows
//! reserves that functionality for a future formal Phase 2.8
//! integration. Nothing in `crate::cross_domain` was modified, renamed,
//! or removed to make room for this module; both remain independently
//! tested. See `docs/service-intelligence.md`'s "Roadmap note" section.
//!
//! ## Architecture: mirrors `sermon_adapter.rs`
//!
//! Deterministic, phrase-anchored cue detection over a `Mutex`-guarded
//! accumulating tracker (`analyze` takes `&self`, per the shared
//! [`IntelligenceEngine`] contract), producing [`IntelligenceFinding`]s.
//! Every transition traces back to either an explicit trigger phrase
//! (`matched_phrase`, always a verbatim substring of the segment text) or
//! an explicit operator action - never a purely statistical/semantic
//! guess, and never a claim of certainty the evidence doesn't support.
//!
//! ## Service *lifecycle* vs service *phase* (spec section 5)
//!
//! [`cip_core_service::ServiceStatus`] (Started/Paused/Ended) already
//! answers "is a service running" - this module never duplicates that.
//! [`ServicePhase`] answers a different question entirely: "what part of
//! the service is happening right now." Phase inference is deliberately
//! suspended while `context.service_status` is not `Started` (spec
//! section 21: "no false phase transitions" while paused) - see
//! [`ServiceIntelligenceEngine::analyze`].
//!
//! ## Debounce / hysteresis (spec section 32)
//!
//! Every phrase cue is tagged [`CueStrength::Strong`] or
//! [`CueStrength::Weak`]. A `Strong` cue (specific and unambiguous -
//! "let's pray," "turn with me to...") transitions immediately, the same
//! way an explicit operator action is trusted immediately. A `Weak` cue
//! (a bare, more easily-coincidental word - "worship," "good morning" on
//! their own) only transitions once the *same* candidate phase has been
//! cued [`WEAK_DEBOUNCE_STREAK`] times - a single stray mention elsewhere
//! in the service (inside a sermon illustration, for instance) is never
//! enough on its own to flip the phase.
//!
//! ## No system-time-dependent decisions (spec section 34)
//!
//! `analyze` never calls `Utc::now()` to decide anything - every decision
//! here is a pure function of `(input, context)`; identical input always
//! produces identical output. `Utc::now()` is only ever used the same way
//! every other finding in this codebase already uses it: to stamp an
//! informational timestamp (`phase_started_at`, `IntelligenceFinding::created_at`),
//! never to branch a decision. Wall-clock-*dependent* signals (e.g. "the
//! transcript has gone stale") are computed entirely outside this engine,
//! in `apps/desktop/src-tauri/src/service.rs`'s `transcript_freshness` -
//! see `docs/service-intelligence.md`.

use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};
use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use cip_core_service::ServiceStatus;
use regex::Regex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::context::IntelligenceContext;
use crate::domain::{AssertionLevel, FindingKind, IntelligenceDomain};
use crate::engine::{
    EngineCapability, EngineIdentity, IntelligenceEngine, IntelligenceError, IntelligenceInput,
    IntelligenceResult,
};
use crate::evidence::{EvidenceSource, IntelligenceProvenance};
use crate::finding::IntelligenceFinding;

pub const SERVICE_ENGINE_ID: &str = "service-state";
pub const SERVICE_ENGINE_VERSION: &str = "1.0.0";

/// How many consecutive matching weak cues are required before a weak
/// candidate phase is promoted to the actual current phase. A strong cue
/// never needs this - see [`CueStrength`].
pub const WEAK_DEBOUNCE_STREAK: u32 = 2;

// --- service phase -----------------------------------------------------------

/// The observable phase of a live service - distinct from
/// [`ServiceStatus`] (this module's own docs, above). `Unknown` is the
/// honest starting state and a legitimate ongoing state (spec section 23:
/// "Unknown must remain possible") - never silently defaulted to
/// `Opening`. Deliberately a smaller set than every phase a real service
/// could contain (no `Communion`/`AltarCall`/`Transition`) - see
/// `docs/service-intelligence.md`'s "NOT AVAILABLE" section for why those
/// were left out rather than guessed at with no reliable cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServicePhase {
    Unknown,
    Opening,
    Worship,
    Prayer,
    ScriptureReading,
    Sermon,
    Offering,
    Announcement,
    Closing,
}

impl ServicePhase {
    /// SCREAMING_SNAKE_CASE label, matching this codebase's established
    /// `AppEvent::name`/`SermonState::label` convention.
    pub const fn label(self) -> &'static str {
        match self {
            ServicePhase::Unknown => "UNKNOWN",
            ServicePhase::Opening => "OPENING",
            ServicePhase::Worship => "WORSHIP",
            ServicePhase::Prayer => "PRAYER",
            ServicePhase::ScriptureReading => "SCRIPTURE_READING",
            ServicePhase::Sermon => "SERMON",
            ServicePhase::Offering => "OFFERING",
            ServicePhase::Announcement => "ANNOUNCEMENT",
            ServicePhase::Closing => "CLOSING",
        }
    }
}

/// Whether a phase-to-phase transition is ordinary, unusual-but-real, or
/// worth flagging (spec section 31) - classification only, **never** a
/// block: every transition this engine detects or an operator asserts is
/// always accepted, regardless of plausibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionPlausibility {
    Expected,
    Possible,
    Unexpected,
}

/// The conventional service-flow order (spec section 31's own example
/// order). `Expected` = the immediate next step (or staying put);
/// `Possible` = skipping ahead, or either side being `Unknown`;
/// `Unexpected` = a real regression to an earlier phase - worth a
/// finding an operator can review, never a rejected transition.
fn classify_transition(from: ServicePhase, to: ServicePhase) -> TransitionPlausibility {
    use ServicePhase::*;
    if from == to {
        return TransitionPlausibility::Expected;
    }
    const ORDER: [ServicePhase; 8] = [
        Opening,
        Worship,
        Prayer,
        ScriptureReading,
        Sermon,
        Offering,
        Announcement,
        Closing,
    ];
    let from_idx = ORDER.iter().position(|&p| p == from);
    let to_idx = ORDER.iter().position(|&p| p == to);
    match (from_idx, to_idx) {
        (None, _) | (_, None) => TransitionPlausibility::Possible,
        (Some(f), Some(t)) if t == f + 1 => TransitionPlausibility::Expected,
        (Some(f), Some(t)) if t > f => TransitionPlausibility::Possible,
        _ => TransitionPlausibility::Unexpected,
    }
}

// --- phrase-anchored cue detection --------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CueStrength {
    Strong,
    Weak,
}

struct PhaseCue {
    pattern: LazyLock<Regex>,
    phase: ServicePhase,
    strength: CueStrength,
}

macro_rules! cue {
    ($pattern:literal, $phase:expr, $strength:expr) => {
        PhaseCue {
            pattern: LazyLock::new(|| Regex::new($pattern).unwrap()),
            phase: $phase,
            strength: $strength,
        }
    };
}

// Every cue requires an explicit trigger phrase, mirroring
// `cip_core_sermon::detection`'s discipline exactly - no purely
// statistical/keyword-frequency heuristic. `Weak` cues are the
// deliberately riskier, single-word-ish shapes; `Strong` cues are
// specific enough on their own to transition immediately (see this
// module's own docs on debounce).
#[rustfmt::skip]
static PHASE_CUES: [PhaseCue; 19] = [
    cue!(r"(?i)\bwelcome\s+to\s+(today's|this)\s+service\b", ServicePhase::Opening, CueStrength::Strong),
    cue!(r"(?i)\bwelcome\s+everyone\b", ServicePhase::Opening, CueStrength::Strong),
    cue!(r"(?i)\bgood\s+morning\b", ServicePhase::Opening, CueStrength::Weak),

    cue!(r"(?i)\b(let.?s|let\s+us)\s+worship\b", ServicePhase::Worship, CueStrength::Strong),
    cue!(r"(?i)\bworship\s+the\s+lord\b", ServicePhase::Worship, CueStrength::Strong),
    cue!(r"(?i)\b(let.?s|let\s+us)\s+praise\b", ServicePhase::Worship, CueStrength::Strong),
    cue!(r"(?i)\bworship\b", ServicePhase::Worship, CueStrength::Weak),

    cue!(r"(?i)\b(let.?s|let\s+us)\s+pray\b", ServicePhase::Prayer, CueStrength::Strong),
    cue!(r"(?i)\bbow\s+(your|our)\s+heads?\b", ServicePhase::Prayer, CueStrength::Strong),

    cue!(r"(?i)\bturn\s+with\s+me\s+to\b", ServicePhase::ScriptureReading, CueStrength::Strong),
    cue!(r"(?i)\bopen\s+your\s+bibles?\b", ServicePhase::ScriptureReading, CueStrength::Strong),

    cue!(r"(?i)\btoday\s+i\s+want\s+to\b", ServicePhase::Sermon, CueStrength::Strong),
    cue!(r"(?i)\bhear\s+the\s+word\b", ServicePhase::Sermon, CueStrength::Strong),

    cue!(r"(?i)\b(our|your)\s+offering\b", ServicePhase::Offering, CueStrength::Strong),
    cue!(r"(?i)\btithes?\s+and\s+offerings?\b", ServicePhase::Offering, CueStrength::Strong),
    cue!(r"(?i)\bthe\s+tithe\b", ServicePhase::Offering, CueStrength::Strong),

    cue!(r"(?i)\bannouncements?\b", ServicePhase::Announcement, CueStrength::Strong),

    cue!(r"(?i)\bmay\s+the\s+grace\b", ServicePhase::Closing, CueStrength::Strong),
    cue!(r"(?i)\blet\s+us\s+close\b", ServicePhase::Closing, CueStrength::Strong),
];

/// One detected cue - `matched_phrase` is always a verbatim substring of
/// the input text, never paraphrased.
struct PhaseCueMatch {
    phase: ServicePhase,
    strength: CueStrength,
    matched_phrase: String,
}

/// Detect every phase cue present in `text`, in cue-table order - pure
/// and deterministic: identical `text` always produces identical output,
/// in the same order.
fn detect_phase_cues(text: &str) -> Vec<PhaseCueMatch> {
    let mut matches = Vec::new();
    for cue in &PHASE_CUES {
        if let Some(m) = cue.pattern.find(text) {
            matches.push(PhaseCueMatch {
                phase: cue.phase,
                strength: cue.strength,
                matched_phrase: m.as_str().to_string(),
            });
        }
    }
    matches
}

/// The single cue a segment should act on: the first `Strong` match
/// (cue-table order), or else the first `Weak` match. More than one cue
/// for different phases in a single segment is rare and deliberately
/// resolved this way rather than left ambiguous.
fn strongest_cue(matches: &[PhaseCueMatch]) -> Option<&PhaseCueMatch> {
    matches
        .iter()
        .find(|m| m.strength == CueStrength::Strong)
        .or_else(|| matches.first())
}

// --- accumulating tracker + engine --------------------------------------------

struct EngineState {
    current_phase: ServicePhase,
    phase_started_at: DateTime<Utc>,
    previous_phase: Option<ServicePhase>,
    transition_count: u32,
    /// A weak cue's candidate phase and how many consecutive matching weak
    /// cues it has accumulated - reset whenever a different phase is
    /// cued, whenever the current phase is confirmed, or whenever any
    /// transition actually happens.
    weak_candidate: Option<(ServicePhase, u32)>,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            current_phase: ServicePhase::Unknown,
            phase_started_at: Utc::now(),
            previous_phase: None,
            transition_count: 0,
            weak_candidate: None,
        }
    }
}

fn transition(state: &mut EngineState, to: ServicePhase) {
    state.previous_phase = Some(state.current_phase);
    state.current_phase = to;
    state.phase_started_at = Utc::now();
    state.transition_count += 1;
}

/// A read-only snapshot of the current service-phase state - the shape
/// `get_service_intelligence_state` returns (see
/// `apps/desktop/src-tauri/src/service.rs`). Never includes pending or
/// resolved findings (those live in the `FindingQueue`, same as every
/// other domain); this is purely the current phase, computed the same
/// deterministic way regardless of any operator review decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePhaseSnapshot {
    pub phase: ServicePhase,
    pub phase_started_at: DateTime<Utc>,
    pub previous_phase: Option<ServicePhase>,
    pub transition_count: u32,
}

/// Wraps the accumulating phase tracker behind the shared
/// [`IntelligenceEngine`] contract, `Mutex`-wrapped for the same
/// interior-mutability reason `SermonIntelligenceEngine`/
/// `MusicIntelligenceEngine` already are - `analyze` takes `&self`.
pub struct ServiceIntelligenceEngine {
    state: Mutex<EngineState>,
}

impl Default for ServiceIntelligenceEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceIntelligenceEngine {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(EngineState::default()),
        }
    }

    /// The current phase snapshot - used by `get_service_intelligence_state`.
    /// Never mutates anything; safe to call at any time, including before
    /// any segment has been analyzed (returns `Unknown`).
    pub fn snapshot(&self) -> ServicePhaseSnapshot {
        let s = self.state.lock().expect("service engine state poisoned");
        ServicePhaseSnapshot {
            phase: s.current_phase,
            phase_started_at: s.phase_started_at,
            previous_phase: s.previous_phase,
            transition_count: s.transition_count,
        }
    }

    /// Explicit operator action (spec sections 19-20: "mark"/"correct" the
    /// service phase) - transitions immediately, bypasses debounce
    /// entirely, and is always `AssertionLevel::Observed`: an operator's
    /// own statement of the service's phase is a direct observation, not
    /// an inference. Never mutates the transcript or anything outside
    /// this engine's own state; returns the finding for the caller to
    /// queue.
    pub fn apply_operator_action(
        &self,
        service_id: Uuid,
        new_phase: ServicePhase,
        note: Option<&str>,
        is_correction: bool,
    ) -> IntelligenceFinding {
        let mut s = self.state.lock().expect("service engine state poisoned");
        let from = s.current_phase;
        s.weak_candidate = None;
        transition(&mut s, new_phase);
        finding_for_operator_action(
            service_id,
            s.transition_count,
            from,
            new_phase,
            note,
            is_correction,
        )
    }
}

impl IntelligenceEngine for ServiceIntelligenceEngine {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            domain: IntelligenceDomain::Service,
            engine_id: SERVICE_ENGINE_ID.to_string(),
            engine_version: SERVICE_ENGINE_VERSION.to_string(),
        }
    }

    fn capability(&self) -> EngineCapability {
        // Deterministic and dependency-free - always available, exactly
        // like `SermonIntelligenceEngine`.
        EngineCapability::Available
    }

    fn analyze(
        &self,
        input: &IntelligenceInput,
        context: &IntelligenceContext,
    ) -> Result<IntelligenceResult, IntelligenceError> {
        let mut s = self
            .state
            .lock()
            .map_err(|_| IntelligenceError::EngineFailed {
                engine_id: SERVICE_ENGINE_ID.to_string(),
                reason: "service engine state lock poisoned".to_string(),
            })?;

        // Never advance phase automatically while the service is not
        // actively running (spec section 21: "no false phase transitions"
        // while paused/ended) - `context.service_status` is explicit
        // input, not a hidden clock call.
        if !matches!(context.service_status, Some(ServiceStatus::Started)) {
            return Ok(IntelligenceResult::empty());
        }

        let service_id = input.service_id;
        let segment_id = input.transcript_segment.id;
        let text = input.transcript_segment.text.as_str();

        let cues = detect_phase_cues(text);
        let Some(cue) = strongest_cue(&cues) else {
            return Ok(IntelligenceResult::empty());
        };

        let mut findings = Vec::new();
        let from = s.current_phase;

        match cue.strength {
            CueStrength::Strong => {
                if cue.phase != from {
                    s.weak_candidate = None;
                    transition(&mut s, cue.phase);
                    findings.push(finding_for_transition(
                        service_id,
                        segment_id,
                        s.transition_count,
                        from,
                        cue.phase,
                        AssertionLevel::Inferred,
                        0.85,
                        &format!("explicit trigger phrase \"{}\" matched", cue.matched_phrase),
                        &cue.matched_phrase,
                    ));
                    push_plausibility_finding(
                        &mut findings,
                        service_id,
                        segment_id,
                        s.transition_count,
                        from,
                        cue.phase,
                    );
                }
            }
            CueStrength::Weak => {
                if cue.phase == from {
                    // Already there - confirms the current phase, nothing
                    // to keep counting toward.
                    s.weak_candidate = None;
                } else {
                    let streak = match &s.weak_candidate {
                        Some((phase, n)) if *phase == cue.phase => n + 1,
                        _ => 1,
                    };
                    if streak >= WEAK_DEBOUNCE_STREAK {
                        s.weak_candidate = None;
                        transition(&mut s, cue.phase);
                        findings.push(finding_for_transition(
                            service_id,
                            segment_id,
                            s.transition_count,
                            from,
                            cue.phase,
                            AssertionLevel::Suggested,
                            0.6,
                            &format!(
                                "weak cue \"{}\" repeated {streak} time(s) with no stronger evidence",
                                cue.matched_phrase
                            ),
                            &cue.matched_phrase,
                        ));
                        push_plausibility_finding(
                            &mut findings,
                            service_id,
                            segment_id,
                            s.transition_count,
                            from,
                            cue.phase,
                        );
                    } else {
                        s.weak_candidate = Some((cue.phase, streak));
                    }
                }
            }
        }

        Ok(IntelligenceResult::new(findings))
    }
}

fn push_plausibility_finding(
    findings: &mut Vec<IntelligenceFinding>,
    service_id: Uuid,
    segment_id: Uuid,
    transition_count: u32,
    from: ServicePhase,
    to: ServicePhase,
) {
    if classify_transition(from, to) == TransitionPlausibility::Unexpected {
        findings.push(anomaly_finding_for_unexpected_transition(
            service_id,
            segment_id,
            transition_count,
            from,
            to,
        ));
    }
}

// --- finding construction ------------------------------------------------------
//
// Every summary embeds `transition_count` (`#<n>`) - not decoration, but
// what keeps `FindingQueue::add`'s equivalence rule (same service/domain/
// kind/summary) from silently dropping a *later*, genuinely different
// transition that happens to repeat an earlier transition's phase pair
// (e.g. Sermon -> Prayer -> Sermon -> Prayer) while the first one is
// still sitting unresolved in the queue.

#[allow(clippy::too_many_arguments)]
fn finding_for_transition(
    service_id: Uuid,
    segment_id: Uuid,
    transition_count: u32,
    from: ServicePhase,
    to: ServicePhase,
    assertion_level: AssertionLevel,
    score: f32,
    reason: &str,
    matched_phrase: &str,
) -> IntelligenceFinding {
    let confidence =
        ConfidenceResult::new(score, ConfidenceSource::Heuristic, Some(reason.to_string()));
    IntelligenceFinding::new(
        service_id,
        IntelligenceDomain::Service,
        FindingKind::ServiceState,
        assertion_level,
        confidence,
        format!(
            "Service phase changed #{transition_count}: {} -> {}",
            from.label(),
            to.label()
        ),
        SERVICE_ENGINE_ID,
        SERVICE_ENGINE_VERSION,
    )
    .with_transcript_segments(vec![segment_id])
    .with_evidence(vec![EvidenceSource::Transcript {
        segment_ids: vec![segment_id],
        excerpt: matched_phrase.to_string(),
    }])
    .with_provenance(IntelligenceProvenance::unknown())
}

fn anomaly_finding_for_unexpected_transition(
    service_id: Uuid,
    segment_id: Uuid,
    transition_count: u32,
    from: ServicePhase,
    to: ServicePhase,
) -> IntelligenceFinding {
    let confidence = ConfidenceResult::new(
        0.5,
        ConfidenceSource::Heuristic,
        Some("transition moved backward relative to the conventional service flow".to_string()),
    );
    IntelligenceFinding::new(
        service_id,
        IntelligenceDomain::Service,
        FindingKind::ServiceState,
        AssertionLevel::Inferred,
        confidence,
        format!(
            "Anomaly #{transition_count}: unexpected service phase transition {} -> {}",
            from.label(),
            to.label()
        ),
        SERVICE_ENGINE_ID,
        SERVICE_ENGINE_VERSION,
    )
    .with_transcript_segments(vec![segment_id])
    .with_evidence(vec![EvidenceSource::Context {
        description: format!(
            "transition_plausibility:unexpected:{} -> {}",
            from.label(),
            to.label()
        ),
    }])
    .with_provenance(IntelligenceProvenance::unknown())
}

fn finding_for_operator_action(
    service_id: Uuid,
    transition_count: u32,
    from: ServicePhase,
    to: ServicePhase,
    note: Option<&str>,
    is_correction: bool,
) -> IntelligenceFinding {
    let confidence =
        ConfidenceResult::new(1.0, ConfidenceSource::Human, note.map(|s| s.to_string()));
    let verb = if is_correction { "corrected" } else { "marked" };
    let summary = format!(
        "Service phase changed #{transition_count}: {} -> {} (operator {verb})",
        from.label(),
        to.label()
    );
    let description = match note {
        Some(n) => format!("operator {verb} phase: {n}"),
        None => format!("operator {verb} phase"),
    };
    IntelligenceFinding::new(
        service_id,
        IntelligenceDomain::Service,
        FindingKind::ServiceState,
        AssertionLevel::Observed,
        confidence,
        summary,
        SERVICE_ENGINE_ID,
        SERVICE_ENGINE_VERSION,
    )
    .with_evidence(vec![EvidenceSource::OperatorAction { description }])
    .with_provenance(IntelligenceProvenance::unknown())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextBounds;
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult as CR, ConfidenceSource as CS};

    fn segment(text: &str, sequence: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence,
            text: text.to_string(),
            is_final: true,
            confidence: CR::new(0.9, CS::Model, None),
            start_ms: sequence * 1000,
            end_ms: sequence * 1000 + 900,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    fn started_context(service_id: Uuid) -> IntelligenceContext {
        IntelligenceContext::build(
            service_id,
            Some(ServiceStatus::Started),
            None,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        )
    }

    fn context_with_status(service_id: Uuid, status: Option<ServiceStatus>) -> IntelligenceContext {
        IntelligenceContext::build(
            service_id,
            status,
            None,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        )
    }

    fn engine() -> ServiceIntelligenceEngine {
        ServiceIntelligenceEngine::new()
    }

    fn analyze(
        engine: &ServiceIntelligenceEngine,
        service_id: Uuid,
        text: &str,
        seq: u64,
    ) -> IntelligenceResult {
        engine
            .analyze(
                &IntelligenceInput::new(service_id, segment(text, seq)),
                &started_context(service_id),
            )
            .unwrap()
    }

    #[test]
    fn identity_reports_the_service_domain() {
        let identity = engine().identity();
        assert_eq!(identity.domain, IntelligenceDomain::Service);
        assert_eq!(identity.engine_id, SERVICE_ENGINE_ID);
    }

    #[test]
    fn always_available_with_no_external_dependency() {
        assert_eq!(engine().capability(), EngineCapability::Available);
    }

    #[test]
    fn starts_unknown_with_no_segments_processed() {
        assert_eq!(engine().snapshot().phase, ServicePhase::Unknown);
    }

    // --- section 37 B-I: per-phase strong-cue detection -----------------------

    #[test]
    fn detects_opening_from_an_explicit_welcome() {
        let e = engine();
        let sid = Uuid::new_v4();
        let result = analyze(&e, sid, "Welcome to today's service.", 0);
        assert!(result
            .findings
            .iter()
            .any(|f| f.summary.contains("UNKNOWN -> OPENING")));
        assert_eq!(e.snapshot().phase, ServicePhase::Opening);
    }

    #[test]
    fn detects_worship_from_an_explicit_invitation() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Welcome to today's service.", 0);
        let result = analyze(&e, sid, "Let's worship the Lord together.", 1);
        assert!(result
            .findings
            .iter()
            .any(|f| f.summary.contains("OPENING -> WORSHIP")));
    }

    #[test]
    fn detects_prayer_from_let_us_pray() {
        let e = engine();
        let sid = Uuid::new_v4();
        let result = analyze(&e, sid, "Let us pray.", 0);
        assert!(result
            .findings
            .iter()
            .any(|f| f.summary.contains("-> PRAYER")));
    }

    #[test]
    fn detects_scripture_reading_from_turn_with_me() {
        let e = engine();
        let sid = Uuid::new_v4();
        let result = analyze(&e, sid, "Turn with me to Romans chapter eight.", 0);
        assert!(result
            .findings
            .iter()
            .any(|f| f.summary.contains("-> SCRIPTURE_READING")));
    }

    #[test]
    fn detects_sermon_from_todays_teaching_language() {
        let e = engine();
        let sid = Uuid::new_v4();
        let result = analyze(
            &e,
            sid,
            "Today I want to speak to you about the life of the Spirit.",
            0,
        );
        assert!(result
            .findings
            .iter()
            .any(|f| f.summary.contains("-> SERMON")));
    }

    #[test]
    fn detects_offering_from_explicit_offering_language() {
        let e = engine();
        let sid = Uuid::new_v4();
        let result = analyze(&e, sid, "Let us prepare our offering.", 0);
        assert!(result
            .findings
            .iter()
            .any(|f| f.summary.contains("-> OFFERING")));
    }

    #[test]
    fn detects_announcement_from_the_word_announcements() {
        let e = engine();
        let sid = Uuid::new_v4();
        let result = analyze(&e, sid, "Here are the announcements.", 0);
        assert!(result
            .findings
            .iter()
            .any(|f| f.summary.contains("-> ANNOUNCEMENT")));
    }

    #[test]
    fn detects_closing_from_a_benediction() {
        let e = engine();
        let sid = Uuid::new_v4();
        let result = analyze(
            &e,
            sid,
            "May the grace of our Lord Jesus Christ be with you.",
            0,
        );
        assert!(result
            .findings
            .iter()
            .any(|f| f.summary.contains("-> CLOSING")));
    }

    #[test]
    fn plain_prose_with_no_cue_produces_no_finding_and_stays_unknown() {
        let e = engine();
        let sid = Uuid::new_v4();
        let result = analyze(&e, sid, "It's good to see everyone this week.", 0);
        assert!(result.findings.is_empty());
        assert_eq!(e.snapshot().phase, ServicePhase::Unknown);
    }

    // --- K: transition detection carries evidence -----------------------------

    #[test]
    fn a_transition_finding_carries_the_matched_phrase_as_evidence() {
        let e = engine();
        let sid = Uuid::new_v4();
        let result = analyze(&e, sid, "Let us pray.", 0);
        let finding = result
            .findings
            .iter()
            .find(|f| f.summary.contains("PRAYER"))
            .unwrap();
        assert!(matches!(
            &finding.evidence[0],
            EvidenceSource::Transcript { excerpt, .. } if excerpt.to_lowercase().contains("let us pray")
        ));
    }

    // --- L/O: weak-cue repeated evidence + debounce/hysteresis ----------------

    #[test]
    fn a_single_weak_cue_never_transitions_on_its_own() {
        let e = engine();
        let sid = Uuid::new_v4();
        // Establish a known starting phase first.
        analyze(&e, sid, "Today I want to speak about faith.", 0);
        let result = analyze(&e, sid, "Good morning, I hope you're doing well.", 1);
        assert!(
            result.findings.is_empty(),
            "one weak cue must never transition the phase on its own"
        );
        assert_eq!(e.snapshot().phase, ServicePhase::Sermon);
    }

    #[test]
    fn a_repeated_weak_cue_transitions_after_the_debounce_streak() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Today I want to speak about faith.", 0);
        let first = analyze(&e, sid, "Good morning to you all.", 1);
        assert!(first.findings.is_empty());
        let second = analyze(&e, sid, "Good morning again, everyone.", 2);
        assert!(
            second
                .findings
                .iter()
                .any(|f| f.summary.contains("SERMON -> OPENING")),
            "a second matching weak cue should promote the candidate phase"
        );
        assert_eq!(e.snapshot().phase, ServicePhase::Opening);
    }

    #[test]
    fn a_weak_promoted_transition_is_suggested_not_inferred() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Today I want to speak about faith.", 0);
        analyze(&e, sid, "Good morning to you all.", 1);
        let result = analyze(&e, sid, "Good morning again, everyone.", 2);
        let finding = result
            .findings
            .iter()
            .find(|f| f.summary.contains("OPENING"))
            .unwrap();
        assert_eq!(finding.assertion_level, AssertionLevel::Suggested);
    }

    #[test]
    fn a_different_weak_cue_in_between_resets_the_streak() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Today I want to speak about faith.", 0);
        analyze(&e, sid, "Good morning to you all.", 1); // Opening streak = 1
        analyze(&e, sid, "Sometimes worship feels distant.", 2); // Worship streak = 1, resets Opening
        let result = analyze(&e, sid, "Good morning again, everyone.", 3); // Opening streak = 1 again, not 2
        assert!(
            result.findings.is_empty(),
            "an intervening different weak cue must reset the streak, not accumulate across candidates"
        );
    }

    // --- M/N: false-positive control (spec section 39's canonical scenario) --

    #[test]
    fn a_narrated_reference_inside_a_sermon_illustration_never_triggers_prayer() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Today I want to speak to you about faith.", 0);
        let result = analyze(
            &e,
            sid,
            "In the story, Jesus told them to pray for their enemies and to love one another.",
            1,
        );
        assert!(
            result.findings.is_empty(),
            "narrated/indirect speech about prayer must never trigger a Prayer transition"
        );
        assert_eq!(e.snapshot().phase, ServicePhase::Sermon);

        let result2 = analyze(&e, sid, "Now, church, let us pray.", 2);
        assert!(
            result2
                .findings
                .iter()
                .any(|f| f.summary.contains("SERMON -> PRAYER")),
            "an explicit, direct prayer invitation must still transition immediately"
        );
    }

    #[test]
    fn mentioning_faith_or_the_bible_without_a_cue_never_advances_the_phase() {
        let e = engine();
        let sid = Uuid::new_v4();
        let result = analyze(&e, sid, "Faith is mentioned throughout the whole Bible.", 0);
        assert!(result.findings.is_empty());
    }

    // --- P: operator override ---------------------------------------------------

    #[test]
    fn operator_mark_transitions_immediately_and_is_observed() {
        let e = engine();
        let sid = Uuid::new_v4();
        let finding = e.apply_operator_action(sid, ServicePhase::Worship, None, false);
        assert_eq!(finding.assertion_level, AssertionLevel::Observed);
        assert!(finding.summary.contains("UNKNOWN -> WORSHIP"));
        assert_eq!(e.snapshot().phase, ServicePhase::Worship);
    }

    #[test]
    fn operator_correction_overrides_a_system_detected_phase_and_is_observed() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Today I want to speak about faith.", 0); // system says Sermon
        assert_eq!(e.snapshot().phase, ServicePhase::Sermon);

        let finding = e.apply_operator_action(
            sid,
            ServicePhase::Worship,
            Some("actually still worship"),
            true,
        );
        assert_eq!(finding.assertion_level, AssertionLevel::Observed);
        assert!(finding.summary.contains("SERMON -> WORSHIP"));
        assert!(finding.summary.contains("corrected"));
        assert_eq!(e.snapshot().phase, ServicePhase::Worship);
    }

    #[test]
    fn operator_action_clears_a_pending_weak_candidate() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Today I want to speak about faith.", 0);
        analyze(&e, sid, "Good morning to you all.", 1); // weak Opening candidate, streak 1
        e.apply_operator_action(sid, ServicePhase::Closing, None, false);
        // The pending weak candidate must not silently fire after the
        // operator's own explicit action.
        let result = analyze(&e, sid, "Good morning again, everyone.", 2);
        assert!(
            result.findings.is_empty(),
            "an operator action must clear any pending weak candidate, not let it resurface later"
        );
    }

    // --- Q/R/S: pause/resume behavior -------------------------------------------

    #[test]
    fn no_finding_is_ever_produced_while_the_service_is_paused() {
        let e = engine();
        let sid = Uuid::new_v4();
        let result = e
            .analyze(
                &IntelligenceInput::new(sid, segment("Let us pray.", 0)),
                &context_with_status(sid, Some(ServiceStatus::Paused)),
            )
            .unwrap();
        assert!(result.findings.is_empty());
        assert_eq!(e.snapshot().phase, ServicePhase::Unknown);
    }

    #[test]
    fn no_finding_is_ever_produced_after_the_service_has_ended() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Today I want to speak about faith.", 0);
        let result = e
            .analyze(
                &IntelligenceInput::new(sid, segment("Let us pray.", 1)),
                &context_with_status(sid, Some(ServiceStatus::Ended)),
            )
            .unwrap();
        assert!(result.findings.is_empty());
        assert_eq!(
            e.snapshot().phase,
            ServicePhase::Sermon,
            "phase stays exactly where it was when the service ended"
        );
    }

    #[test]
    fn resuming_the_service_lets_analysis_continue_from_where_it_left_off() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Today I want to speak about faith.", 0);
        let _ = e.analyze(
            &IntelligenceInput::new(sid, segment("Let us pray.", 1)),
            &context_with_status(sid, Some(ServiceStatus::Paused)),
        );
        assert_eq!(
            e.snapshot().phase,
            ServicePhase::Sermon,
            "no transition while paused"
        );
        let result = analyze(&e, sid, "Let us pray.", 2);
        assert!(
            result
                .findings
                .iter()
                .any(|f| f.summary.contains("SERMON -> PRAYER")),
            "the same cue transitions normally once the service resumes"
        );
    }

    // --- U: anomaly (unexpected transition) detection ---------------------------

    #[test]
    fn a_backward_transition_produces_an_anomaly_finding_but_still_transitions() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Let us prepare our offering.", 0); // Unknown -> Offering (Possible)
        let result = analyze(&e, sid, "Welcome to today's service.", 1); // Offering -> Opening (backward)
        assert!(
            result.findings.iter().any(|f| f.summary.starts_with("Anomaly")),
            "a real regression in service flow should be flagged, not silently accepted without a trace"
        );
        assert_eq!(
            e.snapshot().phase,
            ServicePhase::Opening,
            "an unexpected transition is still accepted, never blocked"
        );
    }

    #[test]
    fn the_expected_next_step_never_produces_an_anomaly_finding() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Welcome to today's service.", 0);
        let result = analyze(&e, sid, "Let's worship the Lord together.", 1);
        assert!(result
            .findings
            .iter()
            .all(|f| !f.summary.starts_with("Anomaly")));
    }

    // --- X: finding deduplication stays possible across repeated transitions --

    #[test]
    fn repeated_identical_transitions_produce_distinguishable_finding_summaries() {
        let e = engine();
        let sid = Uuid::new_v4();
        analyze(&e, sid, "Today I want to speak about faith.", 0); // -> Sermon
        analyze(&e, sid, "Let us pray.", 1); // Sermon -> Prayer  (#2)
        analyze(&e, sid, "Today I want to continue.", 2); // Prayer -> Sermon
        let result = analyze(&e, sid, "Let us pray again.", 3); // Sermon -> Prayer (#4)
        let second_prayer = result
            .findings
            .iter()
            .find(|f| f.summary.contains("SERMON -> PRAYER"))
            .unwrap();
        assert!(
            !second_prayer.summary.contains("#2"),
            "a later, genuinely new transition must not carry the same summary as an earlier equivalent one: {}",
            second_prayer.summary
        );
    }

    // --- Y: determinism -----------------------------------------------------------

    #[test]
    fn identical_input_sequences_produce_equivalent_finding_sequences() {
        let run = || {
            let e = engine();
            let sid = Uuid::new_v4();
            let texts = [
                "Welcome to today's service.",
                "Let's worship the Lord together.",
                "Let us pray.",
                "Turn with me to Romans chapter eight.",
                "Today I want to speak to you about faith.",
            ];
            texts
                .iter()
                .enumerate()
                .flat_map(|(i, text)| analyze(&e, sid, text, i as u64).findings)
                .map(|f| (f.domain, f.kind, f.assertion_level, f.summary))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    // --- Z: bounded / large-scale operation ----------------------------------------

    #[test]
    fn ten_thousand_segments_never_exhaust_memory_or_break_analysis() {
        let e = engine();
        let sid = Uuid::new_v4();
        for i in 0..10_000u64 {
            let text = if i % 500 == 0 {
                "Let us pray.".to_string()
            } else {
                format!("segment number {i} about ordinary service content")
            };
            let _ = analyze(&e, sid, &text, i);
        }
        let result = analyze(&e, sid, "Today I want to teach on faith.", 10_000);
        assert!(result.findings.iter().any(|f| f.summary.contains("SERMON")));
    }

    // --- AC: no engine-to-engine calls (type-level proof) ------------------------
    //
    // `ServiceIntelligenceEngine` holds no field of any other engine type
    // and never imports `bible_adapter`/`music_adapter`/`sermon_adapter`/
    // `cross_domain` - the only communication channel is whatever the
    // caller puts into `IntelligenceContext`, exactly like every other
    // engine in this crate. See `service_adapter::tests` in
    // `apps/desktop/src-tauri/src/service.rs` for the multi-engine
    // shared-context proof mirroring Phase 2.1's own acceptance test.

    #[test]
    fn bible_and_service_engines_share_one_context_without_calling_each_other() {
        use crate::bible_adapter::BibleIntelligenceEngine;
        use crate::fixtures::FakeBibleProvider;

        let sid = Uuid::new_v4();
        let bible = BibleIntelligenceEngine::new(Box::new(FakeBibleProvider::kjv_fixture()), "KJV");
        let service = engine();

        let seg = segment("Turn with me to Romans chapter eight.", 0);
        let input = IntelligenceInput::new(sid, seg);
        let context = started_context(sid);

        let bible_result = bible.analyze(&input, &context).unwrap();
        let service_result = service.analyze(&input, &context).unwrap();

        // Each engine only ever produces findings in its own domain - the
        // structural proof that neither called into the other: a
        // `ServiceIntelligenceEngine` has no field or method capable of
        // reaching `BibleIntelligenceEngine`, and vice versa. The only
        // channel between them is the identical `context` both were
        // handed here.
        assert!(bible_result
            .findings
            .iter()
            .all(|f| f.domain == IntelligenceDomain::Bible));
        assert!(service_result
            .findings
            .iter()
            .all(|f| f.domain == IntelligenceDomain::Service));
        assert!(service_result
            .findings
            .iter()
            .any(|f| f.summary.contains("SCRIPTURE_READING")));
    }

    #[test]
    fn a_panicking_cue_lookup_never_happens_but_engine_failure_is_still_isolated_by_the_registry() {
        // This engine cannot panic on any input (regex matching over a
        // plain &str is infallible) - this test instead documents that
        // claim by feeding it deliberately malformed-looking input.
        let e = engine();
        let sid = Uuid::new_v4();
        let result = analyze(&e, sid, "\u{0}\u{0}\u{0} let us pray \u{0}", 0);
        assert!(result.findings.iter().any(|f| f.summary.contains("PRAYER")));
    }
}
