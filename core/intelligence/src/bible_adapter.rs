//! [`BibleIntelligenceEngine`]: the compatibility boundary Phase 2.0 spec
//! section 25 asks for - "create only the adapter/boundary needed to make
//! [Bible Intelligence] compatible with the shared architecture."
//!
//! This does **not** reimplement Bible Intelligence. Every call to
//! [`BibleIntelligenceEngine::analyze`] delegates entirely to
//! `cip_core_service::process_transcript_segment` - the same function
//! Phase 1.1-1.5 already use - and translates its
//! `cip_core_service::ProcessedSegment` output into
//! [`crate::IntelligenceFinding`]s. Detection/context/validation behavior
//! is unchanged; see `core/service::bible_intelligence`'s own test suite
//! (untouched by this crate) for the regression proof, and
//! `docs/intelligence-architecture.md#bible-compatibility` for how the two
//! are kept in sync.
//!
//! Mapping to the observed/inferred/suggested distinction (Phase 2.0 spec
//! section 8):
//! - A `Chapter` detection (context established, no verse) becomes an
//!   `Inferred` finding - "Active Scripture Context: Romans 8" is
//!   something CIP derived, not something anyone suggested projecting.
//! - `Direct`/`Verse`/`Sequential` detections (a concrete, validated
//!   verse) become `Suggested` findings - exactly what a Phase 1
//!   `Suggestion` already represents.
//! - `Ambiguous`/`Unresolved` detections produce no finding at all, for
//!   the same reason they produce no `Suggestion` in Phase 1: never guess.

use std::sync::Mutex;

use cip_core_bible::{BibleProvider, DefaultScriptureContextManager, ReferenceKind};
use cip_core_confidence::ConfidenceResult;
use cip_core_service::process_transcript_segment;
use uuid::Uuid;

use crate::context::IntelligenceContext;
use crate::domain::{AssertionLevel, FindingKind, IntelligenceDomain};
use crate::engine::{
    EngineCapability, EngineIdentity, IntelligenceEngine, IntelligenceError, IntelligenceInput,
    IntelligenceResult,
};
use crate::evidence::{EvidenceSource, IntelligenceProvenance};
use crate::finding::IntelligenceFinding;

pub const BIBLE_ENGINE_ID: &str = "bible";
pub const BIBLE_ENGINE_VERSION: &str = "1.0";

/// Wraps a `BibleProvider` and the same `DefaultScriptureContextManager`
/// state machine Phase 1.1 introduced, behind the shared
/// [`IntelligenceEngine`] contract. The context manager is
/// `Mutex`-wrapped (interior mutability) because `IntelligenceEngine::analyze`
/// takes `&self`, matching how `AppState` already holds its own
/// `context_manager` behind a `Mutex` for the same reason.
pub struct BibleIntelligenceEngine {
    provider: Box<dyn BibleProvider>,
    translation_id: String,
    context: Mutex<DefaultScriptureContextManager>,
}

impl BibleIntelligenceEngine {
    pub fn new(provider: Box<dyn BibleProvider>, translation_id: impl Into<String>) -> Self {
        let translation_id = translation_id.into();
        Self {
            provider,
            context: Mutex::new(DefaultScriptureContextManager::new(&translation_id)),
            translation_id,
        }
    }
}

fn provenance_for(translation_id: &str) -> IntelligenceProvenance {
    IntelligenceProvenance::from_content(format!("bible:{translation_id}"))
}

impl IntelligenceEngine for BibleIntelligenceEngine {
    fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            domain: IntelligenceDomain::Bible,
            engine_id: BIBLE_ENGINE_ID.to_string(),
            engine_version: BIBLE_ENGINE_VERSION.to_string(),
        }
    }

    fn capability(&self) -> EngineCapability {
        EngineCapability::Available
    }

    fn analyze(
        &self,
        input: &IntelligenceInput,
        _context: &IntelligenceContext,
    ) -> Result<IntelligenceResult, IntelligenceError> {
        let mut context = self
            .context
            .lock()
            .map_err(|_| IntelligenceError::EngineFailed {
                engine_id: BIBLE_ENGINE_ID.to_string(),
                reason: "scripture context lock poisoned".to_string(),
            })?;

        let processed = process_transcript_segment(
            input.service_id,
            &input.transcript_segment.text,
            &self.translation_id,
            self.provider.as_ref(),
            &mut context,
        );

        let segment_id = input.transcript_segment.id;
        let findings = processed
            .detections
            .into_iter()
            .filter_map(|detection| {
                finding_for_detection(
                    input.service_id,
                    segment_id,
                    detection,
                    &self.translation_id,
                )
            })
            .collect();

        Ok(IntelligenceResult::new(findings))
    }
}

fn finding_for_detection(
    service_id: Uuid,
    segment_id: Uuid,
    detection: cip_core_service::ScriptureDetection,
    translation_id: &str,
) -> Option<IntelligenceFinding> {
    let (assertion_level, summary) = match detection.kind {
        ReferenceKind::Chapter => {
            let context = detection.context.as_ref()?;
            (
                AssertionLevel::Inferred,
                format!(
                    "Active Scripture Context: {} {}",
                    context.book, context.chapter
                ),
            )
        }
        ReferenceKind::Direct | ReferenceKind::Verse | ReferenceKind::Sequential => {
            let reference = detection.reference.as_ref()?;
            (AssertionLevel::Suggested, reference.to_string())
        }
        // Never guess - matches Phase 1's "no suggestion for
        // ambiguous/unresolved" rule exactly.
        ReferenceKind::Ambiguous | ReferenceKind::Unresolved => return None,
    };

    let confidence: ConfidenceResult = detection.confidence.clone();
    Some(
        IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            assertion_level,
            confidence,
            summary,
            BIBLE_ENGINE_ID,
            BIBLE_ENGINE_VERSION,
        )
        .with_transcript_segments(vec![segment_id])
        .with_evidence(vec![EvidenceSource::Transcript {
            segment_ids: vec![segment_id],
            excerpt: detection.raw_text,
        }])
        .with_provenance(provenance_for(translation_id)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextBounds;
    use crate::domain::FindingStatus;
    use crate::fixtures::FakeBibleProvider;
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult as CR, ConfidenceSource};

    fn segment(text: &str, sequence: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence,
            text: text.to_string(),
            is_final: true,
            confidence: CR::new(0.9, ConfidenceSource::Model, None),
            start_ms: sequence * 1000,
            end_ms: sequence * 1000 + 900,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    fn context(service_id: Uuid) -> IntelligenceContext {
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

    fn engine() -> BibleIntelligenceEngine {
        BibleIntelligenceEngine::new(Box::new(FakeBibleProvider::kjv_fixture()), "KJV")
    }

    /// The exact Phase 1.1 regression sequence (Phase 2.0 spec section 25/48):
    /// Romans 8 -> verse 28 -> verse 31 -> verse 18, and Romans 8 -> John 3 -> verse 16.
    #[test]
    fn preserves_the_romans_8_context_and_sequential_verse_regression() {
        let engine = engine();
        let service_id = Uuid::new_v4();

        let r1 = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("Romans 8", 0)),
                &context(service_id),
            )
            .unwrap();
        assert_eq!(r1.findings.len(), 1);
        assert_eq!(r1.findings[0].assertion_level, AssertionLevel::Inferred);
        assert_eq!(r1.findings[0].summary, "Active Scripture Context: ROM 8");

        let r2 = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("verse 28", 1)),
                &context(service_id),
            )
            .unwrap();
        assert_eq!(r2.findings[0].summary, "ROM 8:28");
        assert_eq!(r2.findings[0].assertion_level, AssertionLevel::Suggested);

        let r3 = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("verse 31", 2)),
                &context(service_id),
            )
            .unwrap();
        assert_eq!(r3.findings[0].summary, "ROM 8:31");

        let r4 = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("verse 18", 3)),
                &context(service_id),
            )
            .unwrap();
        assert_eq!(r4.findings[0].summary, "ROM 8:18");
    }

    #[test]
    fn context_replacement_still_resolves_to_the_new_book() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("Romans 8", 0)),
                &context(service_id),
            )
            .unwrap();
        engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("John chapter 3", 1)),
                &context(service_id),
            )
            .unwrap();
        let r = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("verse 16", 2)),
                &context(service_id),
            )
            .unwrap();
        assert_eq!(
            r.findings[0].summary, "JHN 3:16",
            "must resolve against the replaced context, not the original one"
        );
    }

    #[test]
    fn a_sentence_resembling_scripture_never_becomes_a_finding() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("Romans 8", 0)),
                &context(service_id),
            )
            .unwrap();
        let r = engine
            .analyze(
                &IntelligenceInput::new(
                    service_id,
                    segment("And we know that all things work together for good", 1),
                ),
                &context(service_id),
            )
            .unwrap();
        assert!(
            r.findings.is_empty(),
            "wording resemblance alone must never invent a finding"
        );
    }

    #[test]
    fn an_invalid_reference_produces_no_finding() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let r = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("Romans 8:999", 0)),
                &context(service_id),
            )
            .unwrap();
        assert!(r.findings.is_empty());
    }

    #[test]
    fn findings_carry_provenance_pointing_at_the_content_registry() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let r = engine
            .analyze(
                &IntelligenceInput::new(service_id, segment("Romans 8:28", 0)),
                &context(service_id),
            )
            .unwrap();
        assert_eq!(
            r.findings[0].provenance.content_id.as_deref(),
            Some("bible:KJV")
        );
    }

    #[test]
    fn findings_start_detected_and_carry_transcript_traceability() {
        let engine = engine();
        let service_id = Uuid::new_v4();
        let seg = segment("Romans 8:28", 0);
        let seg_id = seg.id;
        let r = engine
            .analyze(
                &IntelligenceInput::new(service_id, seg),
                &context(service_id),
            )
            .unwrap();
        assert_eq!(r.findings[0].status, FindingStatus::Detected);
        assert_eq!(r.findings[0].transcript_segment_ids, vec![seg_id]);
    }
}
