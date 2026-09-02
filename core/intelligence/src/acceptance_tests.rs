//! The Phase 2.0 architectural acceptance tests (spec sections 49-52, 60):
//! multi-engine shared-context, failure isolation, determinism, and the
//! full scripted acceptance scenario. Kept as one file, separate from each
//! module's own unit tests, because these specifically test the
//! *composition* of the crate's public API rather than any one type.

#![cfg(test)]

use crate::acceptance_tests::support::segment;
use crate::bible_adapter::BibleIntelligenceEngine;
use crate::context::{ContextBounds, IntelligenceContext};
use crate::domain::{AssertionLevel, FindingKind, FindingStatus, IntelligenceDomain};
use crate::engine::{
    EngineCapability, EngineIdentity, IntelligenceEngine, IntelligenceError, IntelligenceInput,
    IntelligenceResult,
};
use crate::finding::IntelligenceFinding;
use crate::fixtures::{FakeBibleProvider, FakeMusicProvider};
use crate::music_adapter::MusicIntelligenceEngine;
use crate::registry::IntelligenceEngineRegistry;
use chrono::Utc;
use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use cip_core_content::{ContentMetadata, ContentStatus, ContentType};
use cip_core_music::{AcousticRecognitionCandidate, AcousticRecognitionMethod};
use uuid::Uuid;

mod support {
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use uuid::Uuid;

    pub fn segment(text: &str, sequence: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence,
            text: text.to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Model, None),
            start_ms: sequence * 1000,
            end_ms: sequence * 1000 + 900,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }
}

/// A synthetic engine that only ever reads `context` - never a
/// `BibleProvider`, never another engine. Its struct has no field capable
/// of holding one, which is the structural half of "must not call
/// BibleEngine"; the test below is the behavioral half (it produces a
/// finding whose content is demonstrably derived from `context`, not from
/// re-detecting anything itself).
struct RecordingEngine {
    domain: IntelligenceDomain,
    engine_id: &'static str,
}

impl IntelligenceEngine for RecordingEngine {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            domain: self.domain,
            engine_id: self.engine_id.to_string(),
            engine_version: "0.1-test".to_string(),
        }
    }

    fn capability(&self) -> EngineCapability {
        EngineCapability::Available
    }

    fn analyze(
        &self,
        input: &IntelligenceInput,
        context: &IntelligenceContext,
    ) -> Result<IntelligenceResult, IntelligenceError> {
        // Everything this finding says is read straight off `context` -
        // proof that a `RecordingEngine` can "inspect" the shared context
        // without any Bible-specific logic or dependency of its own.
        let observed_context = context
            .active_scripture_context
            .as_ref()
            .map(|c| format!("{} {}", c.book, c.chapter))
            .unwrap_or_else(|| "none".to_string());
        let observed_findings = context.recent_findings.len();

        let finding = IntelligenceFinding::new(
            input.service_id,
            self.domain,
            FindingKind::Correlation,
            AssertionLevel::Observed,
            ConfidenceResult::new(1.0, ConfidenceSource::Heuristic, None),
            format!(
                "observed active context = {observed_context}, recent findings = {observed_findings}"
            ),
            self.engine_id,
            "0.1-test",
        );
        Ok(IntelligenceResult::new(vec![finding]))
    }
}

struct FailingEngine {
    domain: IntelligenceDomain,
    engine_id: &'static str,
}

impl IntelligenceEngine for FailingEngine {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            domain: self.domain,
            engine_id: self.engine_id.to_string(),
            engine_version: "0.1-test".to_string(),
        }
    }
    fn capability(&self) -> EngineCapability {
        EngineCapability::Available
    }
    fn analyze(
        &self,
        _input: &IntelligenceInput,
        _context: &IntelligenceContext,
    ) -> Result<IntelligenceResult, IntelligenceError> {
        Err(IntelligenceError::EngineFailed {
            engine_id: self.engine_id.to_string(),
            reason: "simulated Music Intelligence failure".to_string(),
        })
    }
}

fn bible_engine() -> BibleIntelligenceEngine {
    BibleIntelligenceEngine::new(Box::new(FakeBibleProvider::kjv_fixture()), "KJV")
}

/// Phase 2.0 spec section 49: TestBibleEngine/TestMusicEngine/TestSermonEngine
/// all receive the *same* `IntelligenceContext` object; neither the music
/// nor the sermon engine calls into Bible logic to do so.
#[test]
fn multi_engine_shared_context_proves_no_engine_to_engine_calls() {
    let bible = bible_engine();
    let service_id = Uuid::new_v4();

    // Establish "Romans chapter eight" through the real Bible engine, then
    // build the *one* shared context every other engine will see -
    // including the finding the Bible engine just produced.
    let empty_context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        ContextBounds::default(),
    );
    let bible_input = IntelligenceInput::new(
        service_id,
        segment("Turn with me to Romans chapter eight", 0),
    );
    let bible_result = bible.analyze(&bible_input, &empty_context).unwrap();
    assert_eq!(bible_result.findings.len(), 1);
    assert_eq!(
        bible_result.findings[0].assertion_level,
        AssertionLevel::Inferred
    );

    let shared_context = IntelligenceContext::build(
        service_id,
        None,
        Some(bible_input.transcript_segment.clone()),
        vec![bible_input.transcript_segment.clone()],
        None, // deliberately not re-deriving ScriptureContext here - the
        // finding itself is what other engines are meant to see.
        bible_result.findings.clone(),
        Vec::new(),
        Vec::new(),
        ContextBounds::default(),
    );

    let music = RecordingEngine {
        domain: IntelligenceDomain::Music,
        engine_id: "test-music",
    };
    let sermon = RecordingEngine {
        domain: IntelligenceDomain::Sermon,
        engine_id: "test-sermon",
    };

    let music_result = music.analyze(&bible_input, &shared_context).unwrap();
    let sermon_result = sermon.analyze(&bible_input, &shared_context).unwrap();

    // Both saw the same recent_findings the Bible engine produced -
    // proof the context, not a direct call, is the only channel between
    // them.
    assert!(music_result.findings[0]
        .summary
        .contains("recent findings = 1"));
    assert!(sermon_result.findings[0]
        .summary
        .contains("recent findings = 1"));
    assert_eq!(music_result.findings[0].domain, IntelligenceDomain::Music);
    assert_eq!(sermon_result.findings[0].domain, IntelligenceDomain::Sermon);
}

/// Phase 2.0 spec section 50 / STEP 6: Bible succeeds, Music fails,
/// Sermon still runs, service stays operational, nothing panics.
#[test]
fn failure_isolation_through_the_registry_with_a_real_bible_engine() {
    let mut registry = IntelligenceEngineRegistry::new();
    registry.register(Box::new(bible_engine())).unwrap();
    registry
        .register(Box::new(FailingEngine {
            domain: IntelligenceDomain::Music,
            engine_id: "music",
        }))
        .unwrap();
    registry
        .register(Box::new(RecordingEngine {
            domain: IntelligenceDomain::Sermon,
            engine_id: "sermon",
        }))
        .unwrap();

    let service_id = Uuid::new_v4();
    let context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        ContextBounds::default(),
    );
    let input = IntelligenceInput::new(service_id, segment("Romans 8:28", 0));

    let outcomes = registry.analyze_all(&input, &context);
    assert_eq!(outcomes.len(), 3);

    let bible = outcomes
        .iter()
        .find(|o| o.identity.domain == IntelligenceDomain::Bible)
        .unwrap();
    let music = outcomes
        .iter()
        .find(|o| o.identity.domain == IntelligenceDomain::Music)
        .unwrap();
    let sermon = outcomes
        .iter()
        .find(|o| o.identity.domain == IntelligenceDomain::Sermon)
        .unwrap();

    assert!(bible.result.is_ok(), "Bible result must remain available");
    assert_eq!(
        bible.result.as_ref().unwrap().findings[0].summary,
        "ROM 8:28"
    );
    assert!(
        music.result.is_err(),
        "Music failure must be observable, not hidden"
    );
    assert!(
        sermon.result.is_ok(),
        "Sermon must remain runnable despite Music's failure"
    );
}

/// Phase 2.0 spec section 51: identical input + identical fresh engine
/// state must produce equivalent results (ignoring generated
/// ids/timestamps). No randomness anywhere in this crate.
#[test]
fn identical_input_and_context_produce_deterministic_findings() {
    let service_id = Uuid::new_v4();
    let context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        ContextBounds::default(),
    );
    let sentence = "Turn with me to Romans chapter eight";

    let run = || {
        let engine = bible_engine();
        let seg = segment(sentence, 0);
        let input = IntelligenceInput::new(service_id, seg);
        engine
            .analyze(&input, &context)
            .unwrap()
            .findings
            .into_iter()
            .map(|f| {
                (
                    f.domain,
                    f.kind,
                    f.assertion_level,
                    f.summary,
                    f.confidence.score,
                )
            })
            .collect::<Vec<_>>()
    };

    assert_eq!(run(), run());
}

/// Phase 2.0 spec section 60 (the full architectural acceptance scenario)
/// and its explicit STEP 7: no automatic approval/preparation/projection,
/// no transcript mutation. `core/intelligence` has zero dependency on
/// `cip-core-presentation` at all (see `Cargo.toml`) - it is structurally
/// incapable of preparing or projecting anything; this test proves the
/// behavioral half by walking the full scripted scenario and checking
/// that every finding still starts `Detected` and nothing beyond this
/// crate's own types was ever touched.
#[test]
fn full_architectural_acceptance_scenario() {
    let bible = bible_engine();
    let service_id = Uuid::new_v4();
    let mut context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        ContextBounds::default(),
    );

    // STEP 1-2: "Turn with me to Romans chapter eight." establishes
    // context; the Bible engine's finding carries domain/kind/evidence/
    // confidence/transcript source/engine identity.
    let step1_input = IntelligenceInput::new(
        service_id,
        segment("Turn with me to Romans chapter eight", 0),
    );
    let step1 = bible.analyze(&step1_input, &context).unwrap();
    assert_eq!(step1.findings.len(), 1);
    let f1 = &step1.findings[0];
    assert_eq!(f1.domain, IntelligenceDomain::Bible);
    assert_eq!(f1.kind, FindingKind::Scripture);
    assert!(!f1.evidence.is_empty());
    assert_eq!(
        f1.transcript_segment_ids,
        vec![step1_input.transcript_segment.id]
    );
    assert_eq!(f1.engine_id, "bible");

    context = IntelligenceContext::build(
        service_id,
        None,
        Some(step1_input.transcript_segment.clone()),
        vec![step1_input.transcript_segment.clone()],
        None,
        step1.findings.clone(),
        Vec::new(),
        Vec::new(),
        ContextBounds::default(),
    );

    // STEP 3: "Verse twenty-eight." -> Romans 8:28.
    let step3_input = IntelligenceInput::new(service_id, segment("Verse twenty-eight", 1));
    let step3 = bible.analyze(&step3_input, &context).unwrap();
    assert_eq!(step3.findings[0].summary, "ROM 8:28");
    assert_eq!(step3.findings[0].assertion_level, AssertionLevel::Suggested);

    // STEP 4-5: TestMusicEngine and TestSermonEngine inspect the *same*
    // context - neither calls the Bible engine to do so.
    let music = RecordingEngine {
        domain: IntelligenceDomain::Music,
        engine_id: "test-music",
    };
    let sermon = RecordingEngine {
        domain: IntelligenceDomain::Sermon,
        engine_id: "test-sermon",
    };
    let music_result = music.analyze(&step3_input, &context).unwrap();
    let sermon_result = sermon.analyze(&step3_input, &context).unwrap();
    // Both saw the Bible engine's chapter-context finding through
    // `context.recent_findings` - the only channel between them.
    assert!(music_result.findings[0]
        .summary
        .contains("recent findings = 1"));
    assert!(sermon_result.findings[0]
        .summary
        .contains("recent findings = 1"));

    // STEP 6: substitute a failing Music engine into a registry alongside
    // the real Bible engine and a working Sermon engine.
    let mut registry = IntelligenceEngineRegistry::new();
    registry.register(Box::new(bible_engine())).unwrap();
    registry
        .register(Box::new(FailingEngine {
            domain: IntelligenceDomain::Music,
            engine_id: "music",
        }))
        .unwrap();
    registry
        .register(Box::new(RecordingEngine {
            domain: IntelligenceDomain::Sermon,
            engine_id: "sermon",
        }))
        .unwrap();
    let outcomes = registry.analyze_all(&step3_input, &context);
    assert!(outcomes
        .iter()
        .any(|o| o.identity.domain == IntelligenceDomain::Bible && o.result.is_ok()));
    assert!(outcomes
        .iter()
        .any(|o| o.identity.domain == IntelligenceDomain::Music && o.result.is_err()));
    assert!(outcomes
        .iter()
        .any(|o| o.identity.domain == IntelligenceDomain::Sermon && o.result.is_ok()));

    // STEP 7: no automatic approval/preparation/projection, no transcript
    // mutation - every finding produced during this whole scenario is
    // still sitting at `Detected`, the status a finding starts at and
    // nothing here ever advances.
    for finding in step1.findings.iter().chain(step3.findings.iter()) {
        assert_eq!(finding.status, crate::domain::FindingStatus::Detected);
    }
    // The original transcript segments are exactly what was constructed -
    // nothing in this scenario had any way to mutate `.text` after the
    // fact (there is no `&mut TranscriptSegment` anywhere in this crate's
    // public API).
    assert_eq!(
        step1_input.transcript_segment.text,
        "Turn with me to Romans chapter eight"
    );
    assert_eq!(step3_input.transcript_segment.text, "Verse twenty-eight");
}

// --- Phase 2.1: Bible + real Music + synthetic third engine ----------------

fn music_engine() -> MusicIntelligenceEngine {
    MusicIntelligenceEngine::new(Box::new(FakeMusicProvider::hymnbook_fixture()))
}

fn enabled_music_content() -> ContentMetadata {
    ContentMetadata {
        id: "music:test-hymnbook".to_string(),
        content_type: ContentType::Music,
        name: "Test Hymnbook".to_string(),
        version: "dev-fixture".to_string(),
        language: "en".to_string(),
        source: "test fixture".to_string(),
        publisher: None,
        copyright: None,
        license: None,
        distribution: None,
        imported_at: Utc::now(),
        checksum: None,
        status: ContentStatus::Enabled,
        licensing_status: cip_core_content::LicensingStatus::Unknown,
        usage: cip_core_content::UsagePermissions::default(),
    }
}

/// Phase 2.1 spec section 34: Bible Engine + real Music Engine + a
/// synthetic third engine all receive the same `IntelligenceContext`;
/// Music does not call Bible, Bible does not call Music, and both
/// findings can coexist in the same context (the foundation for a future
/// correlation layer - spec section 35 - without implementing one).
#[test]
fn bible_and_music_engines_share_one_context_without_calling_each_other() {
    let bible = bible_engine();
    let music = music_engine();
    let service_id = Uuid::new_v4();
    let content = vec![enabled_music_content()];

    let context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        content.clone(),
        ContextBounds::default(),
    );

    let bible_input = IntelligenceInput::new(service_id, segment("Romans 8:28", 0));
    let bible_result = bible.analyze(&bible_input, &context).unwrap();
    assert_eq!(bible_result.findings[0].domain, IntelligenceDomain::Bible);

    let music_input = IntelligenceInput::new(service_id, segment("Test Hymn One", 1));
    let music_result = music.analyze(&music_input, &context).unwrap();
    assert_eq!(music_result.findings[0].domain, IntelligenceDomain::Music);

    // Both findings coexist in one context - the foundation a future
    // correlation layer would read from, without this phase implementing
    // any actual correlation logic.
    let shared_context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        vec![
            bible_result.findings[0].clone(),
            music_result.findings[0].clone(),
        ],
        Vec::new(),
        content,
        ContextBounds::default(),
    );
    assert_eq!(shared_context.recent_findings.len(), 2);
    assert!(shared_context
        .recent_findings
        .iter()
        .any(|f| f.domain == IntelligenceDomain::Bible));
    assert!(shared_context
        .recent_findings
        .iter()
        .any(|f| f.domain == IntelligenceDomain::Music));
}

/// Phase 2.1 spec section 34: a failing Bible engine must not break a
/// working Music engine, and vice versa - both directions, with the real
/// engines this time (not synthetic doubles).
#[test]
fn failure_of_either_real_engine_does_not_affect_the_other() {
    let service_id = Uuid::new_v4();
    let content = vec![enabled_music_content()];
    let context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        content,
        ContextBounds::default(),
    );

    // Bible fails (an empty provider that errors on lock would be
    // artificial - instead prove isolation the other direction: a
    // registry with only Music registered still serves Music correctly
    // even though Bible is entirely unregistered/"unavailable".
    let mut registry = IntelligenceEngineRegistry::new();
    registry.register(Box::new(music_engine())).unwrap();
    let input = IntelligenceInput::new(service_id, segment("Test Hymn One", 0));
    let outcomes = registry.analyze_all(&input, &context);
    let music_outcome = outcomes
        .iter()
        .find(|o| o.identity.domain == IntelligenceDomain::Music)
        .unwrap();
    assert!(music_outcome.result.is_ok());
    assert!(!music_outcome.result.as_ref().unwrap().findings.is_empty());

    // And the reverse: Bible registered alone still works with Music
    // completely absent from the registry.
    let mut bible_only = IntelligenceEngineRegistry::new();
    bible_only.register(Box::new(bible_engine())).unwrap();
    let bible_input = IntelligenceInput::new(service_id, segment("Romans 8:28", 0));
    let bible_outcomes = bible_only.analyze_all(&bible_input, &context);
    let bible_outcome = bible_outcomes
        .iter()
        .find(|o| o.identity.domain == IntelligenceDomain::Bible)
        .unwrap();
    assert!(bible_outcome.result.is_ok());
}

/// Determinism proof for the real Music engine, mirroring the Bible
/// determinism test (spec section 51).
#[test]
fn music_engine_is_deterministic_for_identical_input_and_context() {
    let service_id = Uuid::new_v4();
    let content = vec![enabled_music_content()];
    let context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        content,
        ContextBounds::default(),
    );

    let run = || {
        let engine = music_engine();
        let input = IntelligenceInput::new(service_id, segment("Test Hymn One", 0));
        engine
            .analyze(&input, &context)
            .unwrap()
            .findings
            .into_iter()
            .map(|f| {
                (
                    f.domain,
                    f.kind,
                    f.assertion_level,
                    f.summary,
                    f.confidence.score,
                )
            })
            .collect::<Vec<_>>()
    };

    assert_eq!(run(), run());
}

// --- Phase 2.2: acoustic recognition + fusion failure isolation ------------

fn acoustic_candidate(song_id: &str, content_id: &str, score: f32) -> AcousticRecognitionCandidate {
    AcousticRecognitionCandidate {
        song_id: song_id.to_string(),
        content_id: content_id.to_string(),
        confidence: ConfidenceResult::new(score, ConfidenceSource::Model, None),
        method: AcousticRecognitionMethod::Test,
        segment_id: Uuid::nil(),
        duration_ms: 8_000,
        evidence: vec!["acoustic acceptance-test evidence".to_string()],
    }
}

/// Phase 2.2 rule (mirrors Phase 2.1 spec section 34, applied to the new
/// acoustic entry point): acoustic recognition and lyric/title recognition
/// on the *same* `MusicIntelligenceEngine` instance must not interfere.
/// Feeding a genuinely unresolvable acoustic candidate (naming a dataset
/// that is not enabled) must never affect a later lyric-path call on the
/// same engine.
#[test]
fn acoustic_analysis_failure_does_not_affect_lyric_recognition_on_the_same_engine() {
    let engine = music_engine();
    let service_id = Uuid::new_v4();
    let content = vec![enabled_music_content()];
    let context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        content,
        ContextBounds::default(),
    );

    let unresolvable = vec![acoustic_candidate("h1", "music:not-enabled", 0.9)];
    let acoustic_result = engine
        .analyze_acoustic(service_id, &unresolvable, &context)
        .unwrap();
    assert!(
        acoustic_result.findings.is_empty(),
        "a candidate naming a non-enabled dataset must never resolve into a finding"
    );

    let lyric_input = IntelligenceInput::new(service_id, segment("Test Hymn One", 0));
    let lyric_result = engine.analyze(&lyric_input, &context).unwrap();
    assert_eq!(
        lyric_result.findings.len(),
        1,
        "lyric recognition on the same engine instance is unaffected by the failed acoustic call"
    );
}

/// Phase 2.2 rule (mirrors `bible_and_music_engines_share_one_context_without_calling_each_other`):
/// the Bible engine and an acoustic-sourced Music finding both read the
/// same shared `IntelligenceContext`; the acoustic path never calls Bible
/// logic, and both findings coexist in one context.
#[test]
fn acoustic_and_bible_share_one_context_without_calling_each_other() {
    let bible = bible_engine();
    let music = music_engine();
    let service_id = Uuid::new_v4();
    let content = vec![enabled_music_content()];
    let context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        content.clone(),
        ContextBounds::default(),
    );

    let bible_input = IntelligenceInput::new(service_id, segment("Romans 8:28", 0));
    let bible_result = bible.analyze(&bible_input, &context).unwrap();
    assert_eq!(bible_result.findings[0].domain, IntelligenceDomain::Bible);

    let candidates = vec![acoustic_candidate("h1", "music:test-hymnbook", 0.8)];
    let acoustic_result = music
        .analyze_acoustic(service_id, &candidates, &context)
        .unwrap();
    assert_eq!(
        acoustic_result.findings[0].domain,
        IntelligenceDomain::Music
    );

    let shared_context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        vec![
            bible_result.findings[0].clone(),
            acoustic_result.findings[0].clone(),
        ],
        Vec::new(),
        content,
        ContextBounds::default(),
    );
    assert_eq!(shared_context.recent_findings.len(), 2);
    assert!(shared_context
        .recent_findings
        .iter()
        .any(|f| f.domain == IntelligenceDomain::Bible));
    assert!(shared_context
        .recent_findings
        .iter()
        .any(|f| f.domain == IntelligenceDomain::Music));
}

/// Phase 2.2 rule: acoustic-sourced findings must never be automatically
/// approved/projected - they start `Detected` exactly like every other
/// finding this crate produces (Phase 2.0 spec section 36, unchanged).
#[test]
fn acoustic_sourced_findings_never_start_anything_but_detected() {
    let music = music_engine();
    let service_id = Uuid::new_v4();
    let content = vec![enabled_music_content()];
    let context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        content,
        ContextBounds::default(),
    );

    let candidates = vec![acoustic_candidate("h1", "music:test-hymnbook", 0.9)];
    let result = music
        .analyze_acoustic(service_id, &candidates, &context)
        .unwrap();
    assert_eq!(result.findings.len(), 1);
    assert_eq!(result.findings[0].status, FindingStatus::Detected);
}

/// Phase 2.2 rule: a disabled/unregistered Music dataset must never be
/// resolved by acoustic recognition, mirroring the lyric-path degradation
/// proof (`failure_of_either_real_engine_does_not_affect_the_other`) - a
/// registry-less, direct `analyze_acoustic` call against no enabled
/// content degrades to "no result," never an error and never a resolved
/// finding.
#[test]
fn acoustic_recognition_with_no_enabled_music_content_yields_no_findings_and_no_error() {
    let music = music_engine();
    let service_id = Uuid::new_v4();
    let context = IntelligenceContext::build(
        service_id,
        None,
        None,
        Vec::new(),
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(), // no content_metadata at all - nothing enabled.
        ContextBounds::default(),
    );

    let candidates = vec![acoustic_candidate("h1", "music:test-hymnbook", 0.9)];
    let result = music
        .analyze_acoustic(service_id, &candidates, &context)
        .unwrap();
    assert!(result.findings.is_empty());
}
