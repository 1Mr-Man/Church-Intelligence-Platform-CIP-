//! [`SermonIntelligenceEngine`]: the `IntelligenceEngine` boundary over
//! `cip_core_sermon`'s pure detection/theme/structure logic (Phase 2.3).
//! Mirrors `bible_adapter.rs`'s split exactly: `cip_core_sermon` answers
//! "what sermon-shaped things are present in this text," this module
//! decides what any of that *means* epistemically (assertion level,
//! confidence, priority) and packages it as [`crate::IntelligenceFinding`]s.
//!
//! ## Observed vs inferred (spec section 7)
//!
//! - A phrase-anchored structural detection (main point, sub-point,
//!   definition, key statement, declaration, question, illustration/story/
//!   example, application, prayer point, summary, scripture quotation) is
//!   `Observed` - the trigger phrase is verbatim transcript text, so the
//!   *fact the pastor said this* is a direct observation, not a guess.
//! - A theme candidate, a scripture cross-link, a reflection
//!   classification, a transition, and a conclusion signal are all
//!   `Inferred` - each is CIP's own derived judgment about structure or
//!   purpose, never something claimed to be a verbatim statement.
//! - Nothing in this module ever produces `Generated` - see
//!   `docs/sermon-intelligence.md`.
//!
//! ## Scripture cross-linking (spec section 16/47)
//!
//! This module never detects Scripture references itself - it only reads
//! `IntelligenceContext::active_scripture_context` (populated by the Bible
//! Intelligence Engine, unchanged) and, when a main point is freshly
//! recorded while a Scripture context is active, emits one additional
//! `Inferred` finding naming that context as the point's likely supporting
//! Scripture. The actual reference/text always stays owned by the Bible
//! engine; this finding only records a structural relationship.

use std::sync::Mutex;

use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use cip_core_sermon::{
    detect_elements, infer_state, SermonDetection, SermonElementKind, SermonPoint, SermonState,
    SermonStructure, ThemeCandidate, ThemeTracker,
};
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

pub const SERMON_ENGINE_ID: &str = "sermon-core";
pub const SERMON_ENGINE_VERSION: &str = "0.1.0";

/// A read-only snapshot of everything [`SermonIntelligenceEngine`] has
/// derived so far - the shape `get_sermon_state` returns. Never includes
/// pending/rejected findings (those live in the `FindingQueue`, per the
/// existing operator-review architecture); this is purely the current
/// theme/state/structure, computed the same deterministic way regardless
/// of any operator review decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonStateSnapshot {
    pub state: SermonState,
    pub theme: Option<ThemeCandidate>,
    pub points: Vec<SermonPoint>,
}

struct EngineState {
    theme: ThemeTracker,
    structure: SermonStructure,
    last_state: SermonState,
    has_processed: bool,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            theme: ThemeTracker::default(),
            structure: SermonStructure::new(),
            last_state: SermonState::Introduction,
            has_processed: false,
        }
    }
}

/// Wraps `cip_core_sermon`'s trackers behind the shared
/// [`IntelligenceEngine`] contract, `Mutex`-wrapped for the same interior-
/// mutability reason `BibleIntelligenceEngine`/`MusicIntelligenceEngine`
/// already are - `analyze` takes `&self`.
pub struct SermonIntelligenceEngine {
    state: Mutex<EngineState>,
}

impl Default for SermonIntelligenceEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SermonIntelligenceEngine {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(EngineState::default()),
        }
    }

    /// The current theme/state/structure snapshot - used by
    /// `get_sermon_state` and by the manual test-mode UI. Never mutates
    /// anything; safe to call at any time, including before any segment
    /// has been analyzed.
    pub fn snapshot(&self) -> SermonStateSnapshot {
        let state = self.state.lock().expect("sermon engine state poisoned");
        SermonStateSnapshot {
            state: state.last_state,
            theme: state.theme.current_candidate(),
            points: state.structure.points().to_vec(),
        }
    }
}

impl IntelligenceEngine for SermonIntelligenceEngine {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            domain: IntelligenceDomain::Sermon,
            engine_id: SERMON_ENGINE_ID.to_string(),
            engine_version: SERMON_ENGINE_VERSION.to_string(),
        }
    }

    fn capability(&self) -> EngineCapability {
        // Deterministic and dependency-free - always available, unlike
        // Phase 2.2's acoustic engine (which depends on an optional local
        // model). See `docs/sermon-intelligence.md`'s offline section.
        EngineCapability::Available
    }

    fn analyze(
        &self,
        input: &IntelligenceInput,
        context: &IntelligenceContext,
    ) -> Result<IntelligenceResult, IntelligenceError> {
        let mut engine_state = self
            .state
            .lock()
            .map_err(|_| IntelligenceError::EngineFailed {
                engine_id: SERMON_ENGINE_ID.to_string(),
                reason: "sermon engine state lock poisoned".to_string(),
            })?;

        let service_id = input.service_id;
        let segment_id = input.transcript_segment.id;
        let text = input.transcript_segment.text.as_str();

        let detections = detect_elements(text);
        let kinds: Vec<SermonElementKind> = detections.iter().map(|d| d.kind).collect();

        engine_state.theme.record(text, &kinds);

        let mut findings: Vec<IntelligenceFinding> = Vec::new();
        let mut freshly_recorded_main_point = false;
        for detection in &detections {
            match detection.kind {
                SermonElementKind::MainPoint => {
                    engine_state.structure.record_main_point(text);
                    freshly_recorded_main_point = true;
                }
                SermonElementKind::SubPoint => {
                    engine_state.structure.record_sub_point(text);
                }
                _ => {}
            }
            findings.push(finding_for_detection(service_id, segment_id, detection));
        }

        if let Some(candidate) = engine_state.theme.current_candidate() {
            findings.push(finding_for_theme(service_id, segment_id, &candidate));
        }

        if freshly_recorded_main_point {
            if let Some(scripture_finding) =
                finding_for_scripture_cross_link(service_id, segment_id, context)
            {
                findings.push(scripture_finding);
            }
        }

        let new_state = infer_state(&kinds, engine_state.has_processed);
        engine_state.has_processed = true;
        if new_state != engine_state.last_state {
            findings.push(finding_for_transition(
                service_id,
                segment_id,
                engine_state.last_state,
                new_state,
            ));
            engine_state.last_state = new_state;
        }

        Ok(IntelligenceResult::new(findings))
    }
}

fn observed(score: f32, reason: &str) -> (AssertionLevel, ConfidenceResult) {
    (
        AssertionLevel::Observed,
        ConfidenceResult::new(score, ConfidenceSource::Heuristic, Some(reason.to_string())),
    )
}

fn inferred(score: f32, reason: &str) -> (AssertionLevel, ConfidenceResult) {
    (
        AssertionLevel::Inferred,
        ConfidenceResult::new(score, ConfidenceSource::Heuristic, Some(reason.to_string())),
    )
}

fn finding_for_detection(
    service_id: Uuid,
    segment_id: Uuid,
    detection: &SermonDetection,
) -> IntelligenceFinding {
    let raw = detection.raw_text.trim();
    let (assertion_level, confidence, summary) = match detection.kind {
        SermonElementKind::MainPoint => {
            let (a, c) = observed(0.9, "explicit main-point trigger phrase matched");
            (a, c, format!("Main Point: {raw}"))
        }
        SermonElementKind::SubPoint => {
            let (a, c) = observed(0.85, "explicit sub-point trigger phrase matched");
            (a, c, format!("Sub-Point: {raw}"))
        }
        SermonElementKind::ScriptureReference => {
            let (a, c) = inferred(0.6, "structural cross-link, no explicit trigger phrase");
            (a, c, format!("Supporting Scripture context: {raw}"))
        }
        SermonElementKind::ScriptureQuotation => {
            let quoted = detection.extracted_text.as_deref().unwrap_or(raw);
            let (a, c) = observed(0.95, "verbatim quotation marks present in transcript");
            (a, c, format!("Scripture Quotation: \"{quoted}\""))
        }
        SermonElementKind::Definition => {
            let (a, c) = observed(0.85, "explicit definition trigger phrase matched");
            (a, c, format!("Definition: {raw}"))
        }
        SermonElementKind::KeyStatement => {
            let (a, c) = observed(0.85, "explicit key-statement trigger phrase matched");
            (a, c, format!("Key Statement: {raw}"))
        }
        SermonElementKind::Declaration => {
            let (a, c) = observed(0.85, "explicit declaration trigger phrase matched");
            (a, c, format!("Declaration: {raw}"))
        }
        SermonElementKind::Question => {
            let (a, c) = observed(0.95, "question mark present in transcript");
            (a, c, format!("Question: {raw}"))
        }
        SermonElementKind::Illustration => {
            let (a, c) = observed(0.8, "explicit illustration trigger phrase matched");
            (a, c, format!("Illustration: {raw}"))
        }
        SermonElementKind::Story => {
            let (a, c) = observed(0.8, "explicit story trigger phrase matched");
            (a, c, format!("Story: {raw}"))
        }
        SermonElementKind::Example => {
            let (a, c) = observed(0.8, "explicit example trigger phrase matched");
            (a, c, format!("Example: {raw}"))
        }
        SermonElementKind::Application => {
            let (a, c) = observed(0.85, "explicit application trigger phrase matched");
            (a, c, format!("Application: {raw}"))
        }
        SermonElementKind::PrayerPoint => {
            let (a, c) = observed(0.85, "explicit prayer-point trigger phrase matched");
            (a, c, format!("Prayer Point: {raw}"))
        }
        SermonElementKind::Summary => {
            let (a, c) = observed(0.8, "explicit summary trigger phrase matched");
            (a, c, format!("Summary: {raw}"))
        }
        SermonElementKind::Reflection => {
            let (a, c) = inferred(
                0.6,
                "classification of a question's likely reflective purpose",
            );
            (a, c, format!("Reflection: {raw}"))
        }
        SermonElementKind::Transition => {
            let (a, c) = inferred(0.6, "structural transition signal");
            (a, c, format!("Transition: {raw}"))
        }
        SermonElementKind::Conclusion => {
            let (a, c) = inferred(0.65, "possible conclusion signal - sermon may continue");
            (a, c, format!("Possible Conclusion: {raw}"))
        }
        SermonElementKind::Theme => {
            // Themes are never produced per-detection - see
            // `finding_for_theme`, driven by `ThemeTracker` evidence
            // instead.
            let (a, c) = inferred(0.5, "unused path");
            (a, c, String::new())
        }
    };

    IntelligenceFinding::new(
        service_id,
        IntelligenceDomain::Sermon,
        FindingKind::Sermon,
        assertion_level,
        confidence,
        summary,
        SERMON_ENGINE_ID,
        SERMON_ENGINE_VERSION,
    )
    .with_transcript_segments(vec![segment_id])
    .with_evidence(vec![EvidenceSource::Transcript {
        segment_ids: vec![segment_id],
        excerpt: detection.raw_text.clone(),
    }])
    .with_provenance(IntelligenceProvenance::unknown())
}

fn finding_for_theme(
    service_id: Uuid,
    segment_id: Uuid,
    candidate: &ThemeCandidate,
) -> IntelligenceFinding {
    let confidence = ConfidenceResult::new(
        candidate.confidence as f32,
        ConfidenceSource::Heuristic,
        Some(format!(
            "repeated {} time(s) with {} structural mention(s)",
            candidate.repetition_count, candidate.structural_mentions
        )),
    );
    IntelligenceFinding::new(
        service_id,
        IntelligenceDomain::Sermon,
        FindingKind::Sermon,
        AssertionLevel::Inferred,
        confidence,
        format!("Theme: {}", candidate.label),
        SERMON_ENGINE_ID,
        SERMON_ENGINE_VERSION,
    )
    .with_transcript_segments(vec![segment_id])
    .with_evidence(vec![EvidenceSource::Context {
        description: format!(
            "accumulated theme evidence: {} repetition(s), {} structural mention(s)",
            candidate.repetition_count, candidate.structural_mentions
        ),
    }])
    .with_provenance(IntelligenceProvenance::unknown())
}

/// Cross-links a freshly recorded main point to the currently active
/// Scripture context (spec section 47) - never invents or duplicates the
/// reference itself, only names the structural relationship. Returns
/// `None` when there is no active Scripture context to link to.
fn finding_for_scripture_cross_link(
    service_id: Uuid,
    segment_id: Uuid,
    context: &IntelligenceContext,
) -> Option<IntelligenceFinding> {
    let scripture = context.active_scripture_context.as_ref()?;
    let reference_display = match scripture.last_verse {
        Some(verse) => format!("{} {}:{}", scripture.book, scripture.chapter, verse),
        None => format!("{} {}", scripture.book, scripture.chapter),
    };
    let confidence = ConfidenceResult::new(
        0.55,
        ConfidenceSource::Heuristic,
        Some("main point recorded while a Scripture context was active".to_string()),
    );
    Some(
        IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Sermon,
            FindingKind::Sermon,
            AssertionLevel::Inferred,
            confidence,
            format!("Supporting Scripture: {reference_display}"),
            SERMON_ENGINE_ID,
            SERMON_ENGINE_VERSION,
        )
        .with_transcript_segments(vec![segment_id])
        .with_evidence(vec![EvidenceSource::Context {
            description: format!("active_scripture_context:{reference_display}"),
        }])
        .with_provenance(IntelligenceProvenance::unknown()),
    )
}

fn finding_for_transition(
    service_id: Uuid,
    segment_id: Uuid,
    from: SermonState,
    to: SermonState,
) -> IntelligenceFinding {
    let confidence = ConfidenceResult::new(
        0.6,
        ConfidenceSource::Heuristic,
        Some("sermon state changed based on a new structural detection".to_string()),
    );
    IntelligenceFinding::new(
        service_id,
        IntelligenceDomain::Sermon,
        FindingKind::Sermon,
        AssertionLevel::Inferred,
        confidence,
        format!("Transition: {} -> {}", from.label(), to.label()),
        SERMON_ENGINE_ID,
        SERMON_ENGINE_VERSION,
    )
    .with_transcript_segments(vec![segment_id])
    .with_evidence(vec![EvidenceSource::Context {
        description: format!("sermon_state:{} -> {}", from.label(), to.label()),
    }])
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

    fn empty_context(service_id: Uuid) -> IntelligenceContext {
        IntelligenceContext::build(
            service_id,
            None,
            None,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        )
    }

    fn engine() -> SermonIntelligenceEngine {
        SermonIntelligenceEngine::new()
    }

    #[test]
    fn identity_reports_the_sermon_domain() {
        let identity = engine().identity();
        assert_eq!(identity.domain, IntelligenceDomain::Sermon);
        assert_eq!(identity.engine_id, SERMON_ENGINE_ID);
    }

    #[test]
    fn always_available_with_no_external_dependency() {
        assert_eq!(engine().capability(), EngineCapability::Available);
    }

    #[test]
    fn a_main_point_produces_an_observed_finding() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let result = engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("My first point is that faith comes by hearing.", 0),
                ),
                &empty_context(service_id),
            )
            .unwrap();

        let point = result
            .findings
            .iter()
            .find(|f| f.summary.starts_with("Main Point:"))
            .expect("expected a main point finding");
        assert_eq!(point.assertion_level, AssertionLevel::Observed);
        assert_eq!(point.domain, IntelligenceDomain::Sermon);
    }

    #[test]
    fn plain_prose_with_no_trigger_produces_no_structural_finding_and_no_error() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let result = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("Good morning, church.", 0)),
                &empty_context(service_id),
            )
            .unwrap();
        assert!(result
            .findings
            .iter()
            .all(|f| !f.summary.starts_with("Main Point:")));
    }

    #[test]
    fn false_positive_thanksgiving_never_becomes_a_theme() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        for i in 0..5 {
            engine
                .analyze(
                    &IntelligenceInput::new(service_id, segment("We thank God for today.", i)),
                    &empty_context(service_id),
                )
                .unwrap();
        }
        assert!(
            engine.snapshot().theme.is_none(),
            "repeated thanksgiving language with no structural evidence must never become a theme"
        );
    }

    #[test]
    fn theme_emerges_only_after_repetition_and_a_structural_mention() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("My first point is that faith comes by hearing.", 0),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("Faith is central to everything.", 1)),
                &empty_context(service_id),
            )
            .unwrap();
        let result = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("We must walk by faith daily.", 2)),
                &empty_context(service_id),
            )
            .unwrap();

        let theme = result
            .findings
            .iter()
            .find(|f| f.summary.starts_with("Theme:"))
            .expect("expected a theme finding once evidence accumulates");
        assert_eq!(theme.assertion_level, AssertionLevel::Inferred);
        assert!(theme.summary.contains("faith"));
    }

    #[test]
    fn structure_never_rewrites_an_earlier_point_when_a_second_point_arrives() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("My first point is faith comes by hearing.", 0),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("The second thing is faith must be exercised.", 1),
                ),
                &empty_context(service_id),
            )
            .unwrap();

        let snapshot = engine.snapshot();
        assert_eq!(snapshot.points.len(), 2);
        assert!(snapshot.points[0].raw_text.contains("comes by hearing"));
        assert!(snapshot.points[1].raw_text.contains("must be exercised"));
    }

    #[test]
    fn a_repeated_identical_segment_produces_an_equivalent_main_point_finding() {
        // Duplicate suppression itself is `FindingQueue::add`'s job (spec
        // section 36) - this proves the adapter's output is stable enough
        // for that dedup to work: the same input segment always produces
        // an equivalent (same domain/kind/summary) finding.
        let engine = engine();
        let service_id = Uuid::new_v4();
        let r1 = engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("My first point is faith comes by hearing.", 0),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        let r2 = engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("My first point is faith comes by hearing.", 1),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        let p1 = r1
            .findings
            .iter()
            .find(|f| f.summary.starts_with("Main Point:"))
            .unwrap();
        let p2 = r2
            .findings
            .iter()
            .find(|f| f.summary.starts_with("Main Point:"))
            .unwrap();
        assert!(p1.is_equivalent_to(p2));
    }

    #[test]
    fn a_refined_theme_is_not_equivalent_to_the_earlier_narrower_one() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("My first point is faith comes by hearing.", 0),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("Faith is central to everything.", 1)),
                &empty_context(service_id),
            )
            .unwrap();
        let early = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("We must walk by faith daily.", 2)),
                &empty_context(service_id),
            )
            .unwrap();
        let early_theme = early
            .findings
            .iter()
            .find(|f| f.summary.starts_with("Theme:"))
            .unwrap()
            .clone();

        engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("The second thing is that obedience must follow faith.", 3),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("Obedience is not optional for the believer.", 4),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        let later = engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("True obedience requires faith and obedience together.", 5),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        let later_theme = later
            .findings
            .iter()
            .find(|f| f.summary.starts_with("Theme:"))
            .unwrap();

        assert!(
            !early_theme.is_equivalent_to(later_theme),
            "a refined theme label must not collide with the earlier narrower one"
        );
    }

    #[test]
    fn a_main_point_never_creates_a_presentation_item_type() {
        // Type-level proof (spec section 54): `IntelligenceFinding` has no
        // field capable of representing a `PresentationItem`, and this
        // crate does not depend on `cip_core_presentation` at all.
        let engine = engine();
        let service_id = Uuid::new_v4();
        let result = engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("My first point is faith comes by hearing.", 0),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        assert!(result
            .findings
            .iter()
            .all(|f| f.status == crate::domain::FindingStatus::Detected));
    }

    #[test]
    fn ten_thousand_segments_never_exhaust_memory_or_break_analysis() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        for i in 0..10_000u64 {
            let text = if i % 500 == 0 {
                "My first point is that faith comes by hearing.".to_string()
            } else {
                format!("segment number {i} about ordinary sermon content")
            };
            let result = engine
                .analyze(
                    &IntelligenceInput::new(service_id, segment(&text, i)),
                    &empty_context(service_id),
                )
                .unwrap();
            let _ = result;
        }
        // Still operational and still produces sensible output afterward -
        // "remains operational" per spec section 57, not just "doesn't
        // crash."
        let result = engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("The second thing is that obedience must follow.", 10_000),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.summary.starts_with("Main Point:")));
    }

    // --- Phase 2.3: the canonical sermon acceptance scenario (spec
    // section 49) - a short, project-authored synthetic transcript
    // (never copyrighted sermon content), asserting every taxonomy
    // category the spec names is actually detected, and that no finding
    // ever contains text absent from the transcript that produced it.

    #[test]
    fn phase_2_3_canonical_sermon_acceptance_scenario() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let segments = [
            "Good morning church. Today we are looking at faith.",
            "My first point is that faith comes by hearing.",
            "Romans chapter ten verse seventeen says that faith comes by hearing, and hearing by the word of God.",
            "Faith is not merely believing; faith responds.",
            "Let me tell you a story. There was a man who planted a seed and waited patiently.",
            "The second thing is that faith must be exercised.",
            "What would change in your life if you truly believed this?",
            "This week, you need to act on what God has spoken to you.",
            "Let's pray together now.",
            "In conclusion, let us walk by faith daily.",
        ];

        let mut all_findings = Vec::new();
        for (i, text) in segments.iter().enumerate() {
            let result = engine
                .analyze(
                    &IntelligenceInput::new(service_id, segment(text, i as u64)),
                    &empty_context(service_id),
                )
                .unwrap();
            all_findings.extend(result.findings);
        }

        let has = |prefix: &str| all_findings.iter().any(|f| f.summary.starts_with(prefix));
        assert!(
            has("Main Point:"),
            "expected at least one main point finding"
        );
        assert!(
            all_findings
                .iter()
                .filter(|f| f.summary.starts_with("Main Point:"))
                .count()
                >= 2,
            "expected both main points to be detected"
        );
        assert!(has("Definition:"), "expected the definition to be detected");
        assert!(
            has("Story:"),
            "expected the illustration/story to be detected"
        );
        assert!(
            has("Question:"),
            "expected the reflective question to be detected"
        );
        assert!(has("Reflection:"), "expected the reflection classification");
        assert!(
            has("Application:"),
            "expected the application to be detected"
        );
        assert!(
            has("Prayer Point:"),
            "expected the prayer point to be detected"
        );
        assert!(
            has("Possible Conclusion:"),
            "expected the conclusion signal to be detected"
        );
        assert!(
            all_findings
                .iter()
                .any(|f| f.summary.starts_with("Theme:") && f.summary.contains("faith")),
            "expected a Faith theme candidate to emerge from accumulated evidence"
        );

        // No finding may contain fabricated text - every Transcript
        // evidence excerpt must be a verbatim substring of the segment
        // that produced it.
        for finding in &all_findings {
            for evidence in &finding.evidence {
                if let EvidenceSource::Transcript { excerpt, .. } = evidence {
                    assert!(
                        segments.iter().any(|s| s.contains(excerpt.as_str())),
                        "evidence excerpt {excerpt:?} was not verbatim transcript text"
                    );
                }
            }
        }

        // Never Generated - the whole point of Phase 2.3's assertion
        // discipline (spec section 7).
        assert!(all_findings
            .iter()
            .all(|f| f.assertion_level != AssertionLevel::Generated));
    }

    #[test]
    fn false_positive_thanksgiving_is_never_a_theme_and_a_bare_bible_mention_is_never_a_main_point()
    {
        let engine = engine();
        let service_id = Uuid::new_v4();

        let r1 = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("We thank God for today.", 0)),
                &empty_context(service_id),
            )
            .unwrap();
        assert!(r1.findings.iter().all(|f| !f.summary.starts_with("Theme:")));

        let r2 = engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("Faith is mentioned in the Bible many times.", 1),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        assert!(
            r2.findings
                .iter()
                .all(|f| !f.summary.starts_with("Main Point:")),
            "a bare mention of faith with no structural trigger must never become a main point"
        );

        let r3 = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("Turn with me to Romans 8.", 2)),
                &empty_context(service_id),
            )
            .unwrap();
        assert!(
            r3.findings.iter().all(|f| !f.summary.starts_with("Main Point:")),
            "the Bible engine owns Scripture detection - Sermon must never invent a main point from a bare reference"
        );
    }

    #[test]
    fn context_retention_across_an_unrelated_intervening_segment() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("My first point is faith.", 0)),
                &empty_context(service_id),
            )
            .unwrap();
        engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("Good morning, everyone here today.", 1),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        let r3 = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("This means we must act on it.", 2)),
                &empty_context(service_id),
            )
            .unwrap();
        // The point recorded at segment 0 is still the current point -
        // an unrelated intervening segment never displaces it.
        assert_eq!(engine.snapshot().points.len(), 1);
        assert!(engine.snapshot().points[0].raw_text.contains("faith"));
        let _ = r3;
    }

    #[test]
    fn context_replacement_keeps_point_one_historical_and_never_rewrites_it() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("My first point is faith.", 0)),
                &empty_context(service_id),
            )
            .unwrap();
        engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("The second thing is obedience.", 1)),
                &empty_context(service_id),
            )
            .unwrap();

        let snapshot = engine.snapshot();
        assert_eq!(
            snapshot.points.len(),
            2,
            "Point 1 must remain, not be replaced"
        );
        assert!(
            snapshot.points[0].raw_text.contains("faith"),
            "Point 1 is never rewritten"
        );
        assert!(
            snapshot.points[1].raw_text.contains("obedience"),
            "Point 2 becomes current"
        );
    }

    #[test]
    fn scripture_cross_link_only_fires_when_a_main_point_is_recorded_with_an_active_context() {
        use crate::context::ContextBounds as CB;
        let engine_with_context = engine();
        let service_id = Uuid::new_v4();
        let scripture_context = cip_core_bible::ScriptureContext {
            translation_id: "KJV".to_string(),
            book: "ROM".to_string(),
            chapter: 10,
            last_verse: Some(17),
            confidence: CR::new(0.9, CS::Heuristic, None),
            established_at: chrono::Utc::now(),
            valid: true,
        };
        let context_with_scripture = IntelligenceContext::build(
            service_id,
            None,
            None,
            Vec::new(),
            Some(scripture_context),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            CB::default(),
        );

        let result = engine_with_context
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("My first point is that faith comes by hearing.", 0),
                ),
                &context_with_scripture,
            )
            .unwrap();

        assert!(
            result
                .findings
                .iter()
                .any(|f| f.summary.starts_with("Supporting Scripture:")
                    && f.summary.contains("ROM 10:17")),
            "a main point recorded while a Scripture context is active should cross-link to it"
        );
        // No cross-link at all without an active context.
        let engine_without_context = engine();
        let result2 = engine_without_context
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("My first point is that faith comes by hearing.", 0),
                ),
                &empty_context(service_id),
            )
            .unwrap();
        assert!(result2
            .findings
            .iter()
            .all(|f| !f.summary.starts_with("Supporting Scripture:")));
    }

    #[test]
    fn identical_input_sequences_produce_equivalent_finding_sequences() {
        let run = || {
            let engine = engine();
            let service_id = Uuid::new_v4();
            let texts = [
                "My first point is faith comes by hearing.",
                "Let me tell you a story about a man I met.",
                "This week, you need to act on what God has spoken.",
            ];
            texts
                .iter()
                .enumerate()
                .flat_map(|(i, text)| {
                    engine
                        .analyze(
                            &IntelligenceInput::new(service_id, segment(text, i as u64)),
                            &empty_context(service_id),
                        )
                        .unwrap()
                        .findings
                        .into_iter()
                        .map(|f| (f.domain, f.kind, f.assertion_level, f.summary))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }
}
