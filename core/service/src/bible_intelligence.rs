//! The Bible Intelligence Core pipeline:
//!
//! ```text
//! TRANSCRIPT TEXT -> TEXT NORMALIZATION -> SCRIPTURE REFERENCE DETECTION
//!   -> SCRIPTURE CONTEXT MANAGEMENT -> REFERENCE RESOLUTION
//!   -> BIBLE VALIDATION -> CONFIDENCE -> SCRIPTURE SUGGESTION
//! ```
//!
//! [`process_transcript_segment`] is the whole pipeline in one call, and is
//! also the deterministic transcript-input test harness Phase 1.1 asks
//! for: it takes plain text and returns detections/suggestions, with no
//! dependency on audio, a real `SpeechEngine`, or Tauri. Whatever eventually
//! feeds it real transcript segments (a typed test fixture today, a real
//! `SpeechEngine` in Phase 1.2) is invisible to it - see the module docs on
//! `cip_core_ai::SpeechEngine` for the seam this is designed to sit behind.
//!
//! This lives in `core/service` rather than `core/bible` because it is
//! fundamentally a composition of two domains that don't depend on each
//! other directly: `core/bible` (detection, context, the `BibleProvider`
//! contract) and `core/ai` (`Suggestion`). `core/service` is the
//! documented composition point for exactly this reason - it already owns
//! `ServiceSession`, the live-service concept a transcript segment belongs
//! to, and cross-domain flows are expected to go through it rather than
//! create a `bible` <-> `ai` dependency in either direction.
//!
//! ## Validation discipline
//!
//! Every candidate is validated against the supplied [`BibleProvider`]
//! *before* it is allowed to affect the active [`ScriptureContext`] or
//! produce a [`Suggestion`] - "do not trust the parser alone." A candidate
//! that fails validation becomes an `Unresolved` detection and never
//! mutates context state; one invalid candidate in a segment never
//! prevents another, independently valid, candidate in the same segment
//! from resolving normally.
//!
//! ## No automatic projection
//!
//! This module only ever constructs [`ScriptureDetection`]s and
//! [`Suggestion`]s (which always start `Pending`). It has no way to
//! construct a `PresentationItem` or move anything to an "active"/projected
//! state - that requires a separate, human-triggered action elsewhere in
//! the system, by design.

use cip_core_ai::{Suggestion, SuggestionKind};
use cip_core_bible::{
    detect_candidates, normalize::normalize_text, paraphrase, AmbiguousCandidate, BibleProvider,
    ContextResolution, DefaultScriptureContextManager, DetectedCandidate, ReferenceKind,
    ScriptureContext, ScriptureContextManager, ScriptureReference,
};
use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A segment must have at least this many *distinct* significant words
/// before the paraphrase fallback even attempts scoring - short utterances
/// ("Praise God", "Let's pray") could otherwise reach a perfect overlap
/// ratio against some verse by sharing just one or two words.
const MIN_PARAPHRASE_SIGNIFICANT_WORDS: usize = 4;

/// How much of a segment's significant vocabulary must be found in a
/// candidate verse before it's trusted as a paraphrase of that verse,
/// rather than a coincidental partial overlap. Deliberately high - see
/// `cip_core_bible::paraphrase`'s module docs for what this scoring can and
/// cannot tell apart.
const MIN_PARAPHRASE_SCORE: f32 = 0.75;

/// How many candidate verses the paraphrase fallback will score per
/// segment - bounds the cost of a pass that runs only when nothing else in
/// the segment already produced a suggestion.
const MAX_PARAPHRASE_CANDIDATES: usize = 25;

/// One reference candidate after context resolution and Bible validation -
/// the pipeline's per-candidate output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureDetection {
    pub kind: ReferenceKind,
    /// The validated, resolved reference - only present for `Direct`,
    /// `Verse`, and `Sequential` detections. Always `None` for `Chapter`
    /// (no verse is ever invented), `Ambiguous`, and `Unresolved`.
    pub reference: Option<ScriptureReference>,
    /// The active context *after* processing this candidate, if a context
    /// exists at that point.
    pub context: Option<ScriptureContext>,
    /// Populated only for `Ambiguous` detections: the validated candidates
    /// a human must choose between.
    pub candidates: Vec<AmbiguousCandidate>,
    pub confidence: ConfidenceResult,
    /// The exact transcript substring this detection came from.
    pub raw_text: String,
}

impl ScriptureDetection {
    fn unresolved(raw_text: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kind: ReferenceKind::Unresolved,
            reference: None,
            context: None,
            candidates: Vec::new(),
            confidence: ConfidenceResult::new(
                0.1,
                ConfidenceSource::Heuristic,
                Some(reason.into()),
            ),
            raw_text: raw_text.into(),
        }
    }
}

/// The result of running one transcript segment through the Bible
/// Intelligence Core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessedSegment {
    pub service_id: Uuid,
    pub detections: Vec<ScriptureDetection>,
    /// Every `Suggestion` produced, always `Pending` - never
    /// auto-approved, never projected.
    pub suggestions: Vec<Suggestion>,
}

/// Run one transcript segment through the full pipeline: normalize,
/// detect, resolve against `context`, validate against `provider`, score
/// confidence, and produce suggestions for whatever validated.
///
/// This is the deterministic test harness Phase 1.2's real `SpeechEngine`
/// will eventually feed: call it once per transcript segment, in order, as
/// they arrive - it has no other state than what `context` carries between
/// calls.
pub fn process_transcript_segment(
    service_id: Uuid,
    segment_text: &str,
    translation_id: &str,
    provider: &dyn BibleProvider,
    context: &mut DefaultScriptureContextManager,
) -> ProcessedSegment {
    let normalized = normalize_text(segment_text);
    let candidates = detect_candidates(&normalized);

    let mut detections = Vec::with_capacity(candidates.len());
    let mut suggestions = Vec::new();

    for candidate in &candidates {
        let detection = match candidate.kind {
            ReferenceKind::Chapter => resolve_chapter(translation_id, provider, context, candidate),
            ReferenceKind::Direct => resolve_direct(translation_id, provider, context, candidate),
            ReferenceKind::Verse => resolve_bare_verse(provider, context, candidate),
            // detect_candidates never emits these - see ReferenceKind docs.
            ReferenceKind::Sequential
            | ReferenceKind::Ambiguous
            | ReferenceKind::Unresolved
            | ReferenceKind::Paraphrase => ScriptureDetection::unresolved(
                candidate.raw_text.clone(),
                "unexpected candidate kind",
            ),
        };

        if let Some(suggestion) = suggestion_for(service_id, &detection) {
            suggestions.push(suggestion);
        }
        detections.push(detection);
    }

    // Fallback: nothing in this segment cited a reference explicitly (or
    // what was cited never validated), but its wording might still
    // paraphrase a specific verse closely enough to be worth surfacing for
    // operator review. Only attempted when the segment produced no
    // suggestion at all through the normal citation-based path, so an
    // explicit "Romans 8:28" is never second-guessed by a lexical-overlap
    // heuristic.
    if suggestions.is_empty() {
        if let Some(detection) =
            try_paraphrase(translation_id, provider, &normalized, segment_text, context)
        {
            if let Some(suggestion) = suggestion_for(service_id, &detection) {
                suggestions.push(suggestion);
            }
            detections.push(detection);
        }
    }

    ProcessedSegment {
        service_id,
        detections,
        suggestions,
    }
}

/// Fallback for a segment with no confirmed suggestion at all: checks
/// whether its wording closely echoes a specific verse's text via
/// lexical/keyword overlap (see `cip_core_bible::paraphrase`'s module docs
/// for exactly what this can and cannot detect - it is not semantic or
/// neural matching). Never mutates `context` - a paraphrase is not an
/// explicit citation, so it must never establish or replace the active
/// Scripture context the way a real `Chapter`/`Direct` reference does.
/// Produces, at most, a `Pending` detection like every other path in this
/// module - never auto-projected.
fn try_paraphrase(
    translation_id: &str,
    provider: &dyn BibleProvider,
    normalized_text: &str,
    raw_text: &str,
    context: &DefaultScriptureContextManager,
) -> Option<ScriptureDetection> {
    if paraphrase::significant_word_count(normalized_text) < MIN_PARAPHRASE_SIGNIFICANT_WORDS {
        return None;
    }

    let candidates = provider
        .find_similar_verses(translation_id, normalized_text, MAX_PARAPHRASE_CANDIDATES)
        .ok()?;

    let mut best: Option<(f32, cip_core_bible::BibleVerse)> = None;
    for verse in candidates {
        let score = paraphrase::score_overlap(normalized_text, &verse.text);
        let is_better = best.as_ref().map(|(s, _)| score > *s).unwrap_or(true);
        if is_better {
            best = Some((score, verse));
        }
    }

    let (score, verse) = best?;
    if score < MIN_PARAPHRASE_SCORE {
        return None;
    }

    let reference_display = verse.reference.to_string();
    Some(ScriptureDetection {
        kind: ReferenceKind::Paraphrase,
        reference: Some(verse.reference),
        context: context.active_context(),
        candidates: Vec::new(),
        confidence: ConfidenceResult::new(
            score,
            ConfidenceSource::Heuristic,
            Some(format!(
                "lexical overlap with {reference_display} ({:.0}% of significant words matched, not a citation)",
                score * 100.0
            )),
        ),
        raw_text: raw_text.to_string(),
    })
}

/// A `Suggestion` is only ever created for a detection that resolved to a
/// concrete, validated verse (`Direct`/`Verse`/`Sequential`) - never for a
/// bare chapter (nothing to suggest yet) or an ambiguous/unresolved one
/// (never guess).
fn suggestion_for(service_id: Uuid, detection: &ScriptureDetection) -> Option<Suggestion> {
    let reference = detection.reference.as_ref()?;
    Some(Suggestion::new(
        service_id,
        SuggestionKind::Scripture {
            reference: reference.to_string(),
        },
        detection.confidence.clone(),
    ))
}

fn confidence_for_kind(kind: ReferenceKind) -> ConfidenceResult {
    let (score, reason) = match kind {
        ReferenceKind::Direct => (0.97, "explicit book, chapter, and verse; validated"),
        ReferenceKind::Chapter => (0.9, "explicit book and chapter; validated"),
        ReferenceKind::Verse => (0.85, "resolved against active context; validated"),
        ReferenceKind::Sequential => (
            0.88,
            "resolved against an established active context; validated",
        ),
        ReferenceKind::Ambiguous | ReferenceKind::Unresolved => (0.1, "unresolved"),
        // Never reached: try_paraphrase builds its own ConfidenceResult
        // from the real overlap score rather than calling this function -
        // a fixed score here would misrepresent it.
        ReferenceKind::Paraphrase => (0.1, "unexpected: paraphrase scored elsewhere"),
    };
    ConfidenceResult::new(score, ConfidenceSource::Heuristic, Some(reason.to_string()))
}

fn context_from_resolution(resolution: ContextResolution) -> Option<ScriptureContext> {
    match resolution {
        ContextResolution::Established(ctx) => Some(ctx),
        ContextResolution::Replaced { current, .. } => Some(current),
        _ => None,
    }
}

/// `"Romans 8"` / `"Romans chapter 8"` - establishes context. Never
/// produces a `Suggestion` (no verse to suggest yet, and "no verse should
/// be invented").
fn resolve_chapter(
    translation_id: &str,
    provider: &dyn BibleProvider,
    context: &mut DefaultScriptureContextManager,
    candidate: &DetectedCandidate,
) -> ScriptureDetection {
    let (Some(book), Some(chapter)) = (candidate.partial.book.clone(), candidate.partial.chapter)
    else {
        return ScriptureDetection::unresolved(
            candidate.raw_text.clone(),
            "incomplete chapter reference",
        );
    };

    match provider.get_chapter(translation_id, &book, chapter) {
        Ok(Some(_)) => {
            let resolution = context.resolve(candidate.partial.clone());
            ScriptureDetection {
                kind: ReferenceKind::Chapter,
                reference: None,
                context: context_from_resolution(resolution),
                candidates: Vec::new(),
                confidence: confidence_for_kind(ReferenceKind::Chapter),
                raw_text: candidate.raw_text.clone(),
            }
        }
        _ => ScriptureDetection::unresolved(
            candidate.raw_text.clone(),
            "chapter not found in Bible data",
        ),
    }
}

/// `"Romans 8:28"` and its spoken variants - fully explicit, validated as
/// one unit before anything is committed to context.
fn resolve_direct(
    translation_id: &str,
    provider: &dyn BibleProvider,
    context: &mut DefaultScriptureContextManager,
    candidate: &DetectedCandidate,
) -> ScriptureDetection {
    let (Some(book), Some(chapter), Some(verse)) = (
        candidate.partial.book.clone(),
        candidate.partial.chapter,
        candidate.partial.verse_start,
    ) else {
        return ScriptureDetection::unresolved(
            candidate.raw_text.clone(),
            "incomplete direct reference",
        );
    };

    let reference = ScriptureReference::single(translation_id, &book, chapter, verse);
    match provider.get_verse(&reference) {
        Ok(Some(_)) => {
            let resolution = context.resolve(candidate.partial.clone());
            let ctx = context_from_resolution(resolution);
            context.record_resolved(reference.clone());
            ScriptureDetection {
                kind: ReferenceKind::Direct,
                reference: Some(reference),
                context: ctx,
                candidates: Vec::new(),
                confidence: confidence_for_kind(ReferenceKind::Direct),
                raw_text: candidate.raw_text.clone(),
            }
        }
        _ => ScriptureDetection::unresolved(
            candidate.raw_text.clone(),
            "verse not found in Bible data",
        ),
    }
}

/// Bare `"verse N"` - resolved against whatever context (and, briefly after
/// a replacement, shadow context) the manager currently holds, then
/// validated. Promotes to `Sequential` when the active context already had
/// a previously-resolved verse.
fn resolve_bare_verse(
    provider: &dyn BibleProvider,
    context: &mut DefaultScriptureContextManager,
    candidate: &DetectedCandidate,
) -> ScriptureDetection {
    if candidate.partial.verse_start.is_none() {
        return ScriptureDetection::unresolved(candidate.raw_text.clone(), "missing verse number");
    }

    let had_prior_verse = context
        .active_context()
        .and_then(|ctx| ctx.last_verse)
        .is_some();

    match context.resolve(candidate.partial.clone()) {
        ContextResolution::Unresolved => ScriptureDetection::unresolved(
            candidate.raw_text.clone(),
            "no active scripture context",
        ),

        ContextResolution::Resolved(reference, _) => match provider.get_verse(&reference) {
            Ok(Some(_)) => {
                context.record_resolved(reference.clone());
                let kind = if had_prior_verse {
                    ReferenceKind::Sequential
                } else {
                    ReferenceKind::Verse
                };
                ScriptureDetection {
                    kind,
                    reference: Some(reference),
                    context: context.active_context(),
                    candidates: Vec::new(),
                    confidence: confidence_for_kind(kind),
                    raw_text: candidate.raw_text.clone(),
                }
            }
            _ => ScriptureDetection::unresolved(
                candidate.raw_text.clone(),
                "verse not found in active chapter",
            ),
        },

        ContextResolution::Ambiguous(proposed) => {
            let validated: Vec<AmbiguousCandidate> = proposed
                .into_iter()
                .filter(|c| matches!(provider.get_verse(&c.reference), Ok(Some(_))))
                .collect();

            match validated.len() {
                0 => ScriptureDetection::unresolved(
                    candidate.raw_text.clone(),
                    "no valid candidates for ambiguous verse",
                ),
                1 => {
                    let only = validated.into_iter().next().unwrap();
                    context.record_resolved(only.reference.clone());
                    ScriptureDetection {
                        kind: ReferenceKind::Verse,
                        reference: Some(only.reference),
                        context: context.active_context(),
                        candidates: Vec::new(),
                        confidence: only.confidence,
                        raw_text: candidate.raw_text.clone(),
                    }
                }
                _ => {
                    let top_confidence = validated[0].confidence.clone();
                    ScriptureDetection {
                        kind: ReferenceKind::Ambiguous,
                        reference: None,
                        context: context.active_context(),
                        candidates: validated,
                        confidence: top_confidence,
                        raw_text: candidate.raw_text.clone(),
                    }
                }
            }
        }

        ContextResolution::Established(_) | ContextResolution::Replaced { .. } => {
            unreachable!("resolve() never returns Established/Replaced for a bare-verse fragment")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_ai::SuggestionStatus;
    use cip_core_bible::{
        BibleBook, BibleChapter, BibleProviderError, BibleTranslation, BibleVerse, Testament,
    };
    use std::collections::HashMap;

    /// An in-memory `BibleProvider` for Bible Intelligence Core tests - "the
    /// core should be testable with a fake/in-memory provider." Chapter
    /// existence is derived from having at least one verse, exactly like
    /// `SqliteBibleProvider` derives it from `bible_verses` rows, so this
    /// fixture's validation behavior matches the real provider's.
    struct FakeBibleProvider {
        verses: HashMap<(String, String, u32, u32), String>,
    }

    impl FakeBibleProvider {
        fn new(entries: &[(&str, u32, u32, &str)]) -> Self {
            let mut verses = HashMap::new();
            for (book, chapter, verse, text) in entries {
                verses.insert(
                    ("KJV".to_string(), book.to_string(), *chapter, *verse),
                    text.to_string(),
                );
            }
            Self { verses }
        }

        /// The standard fixture used by most tests: Romans 8 (18, 28, 29,
        /// 30, 31) and John 3:16. Deliberately does **not** include Romans
        /// 8:16 - a real KJV verse - so the context-replacement test (#8)
        /// resolves cleanly instead of tripping the ambiguity heuristic;
        /// the dedicated ambiguity test builds its own fixture that does
        /// include it, to construct that scenario on purpose.
        fn kjv_fixture() -> Self {
            Self::new(&[
                (
                    "ROM",
                    8,
                    18,
                    "For I reckon that the sufferings of this present time...",
                ),
                (
                    "ROM",
                    8,
                    28,
                    "And we know that all things work together for good...",
                ),
                (
                    "ROM",
                    8,
                    29,
                    "For whom he did foreknow, he also did predestinate...",
                ),
                (
                    "ROM",
                    8,
                    30,
                    "Moreover whom he did predestinate, them he also called...",
                ),
                ("ROM", 8, 31, "What shall we then say to these things?..."),
                ("JHN", 3, 16, "For God so loved the world..."),
            ])
        }
    }

    impl BibleProvider for FakeBibleProvider {
        fn list_translations(&self) -> Result<Vec<BibleTranslation>, BibleProviderError> {
            Ok(vec![])
        }

        fn get_book(
            &self,
            _translation_id: &str,
            book_code: &str,
        ) -> Result<Option<BibleBook>, BibleProviderError> {
            Ok(Some(BibleBook {
                code: book_code.to_string(),
                name: book_code.to_string(),
                testament: Testament::New,
                chapter_count: 999,
                order: 0,
            }))
        }

        fn get_chapter(
            &self,
            translation_id: &str,
            book_code: &str,
            chapter: u32,
        ) -> Result<Option<BibleChapter>, BibleProviderError> {
            let verses: Vec<BibleVerse> = self
                .verses
                .iter()
                .filter(|((t, b, c, _), _)| t == translation_id && b == book_code && *c == chapter)
                .map(|((_, b, c, v), text)| BibleVerse {
                    reference: ScriptureReference::single(translation_id, b, *c, *v),
                    text: text.clone(),
                })
                .collect();
            if verses.is_empty() {
                Ok(None)
            } else {
                Ok(Some(BibleChapter {
                    book: book_code.to_string(),
                    chapter,
                    verses,
                }))
            }
        }

        fn get_verse(
            &self,
            reference: &ScriptureReference,
        ) -> Result<Option<BibleVerse>, BibleProviderError> {
            let key = (
                reference.translation_id.clone(),
                reference.book.clone(),
                reference.chapter,
                reference.verse_start,
            );
            Ok(self.verses.get(&key).map(|text| BibleVerse {
                reference: reference.clone(),
                text: text.clone(),
            }))
        }

        fn search(
            &self,
            query: &str,
            translation_id: &str,
        ) -> Result<Vec<BibleVerse>, BibleProviderError> {
            let needle = query.to_lowercase();
            Ok(self
                .verses
                .iter()
                .filter(|((t, _, _, _), text)| {
                    t == translation_id && text.to_lowercase().contains(&needle)
                })
                .map(|((_, b, c, v), text)| BibleVerse {
                    reference: ScriptureReference::single(translation_id, b, *c, *v),
                    text: text.clone(),
                })
                .collect())
        }

        fn list_chapters(
            &self,
            translation_id: &str,
            book_code: &str,
        ) -> Result<Vec<u32>, BibleProviderError> {
            let mut chapters: Vec<u32> = self
                .verses
                .keys()
                .filter(|(t, b, _, _)| t == translation_id && b == book_code)
                .map(|(_, _, c, _)| *c)
                .collect();
            chapters.sort_unstable();
            chapters.dedup();
            Ok(chapters)
        }
    }

    fn process(
        provider: &dyn BibleProvider,
        context: &mut DefaultScriptureContextManager,
        text: &str,
    ) -> ProcessedSegment {
        process_transcript_segment(Uuid::new_v4(), text, "KJV", provider, context)
    }

    // 1. Explicit reference.
    #[test]
    fn explicit_reference_resolves_directly() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(&provider, &mut context, "Romans 8:28");

        assert_eq!(result.detections.len(), 1);
        let detection = &result.detections[0];
        assert_eq!(detection.kind, ReferenceKind::Direct);
        assert_eq!(
            detection.reference.as_ref().unwrap().to_string(),
            "ROM 8:28"
        );
        assert_eq!(result.suggestions.len(), 1);
    }

    // 2. Chapter establishment.
    #[test]
    fn chapter_only_establishes_context_without_inventing_a_verse() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(&provider, &mut context, "Romans 8");

        let detection = &result.detections[0];
        assert_eq!(detection.kind, ReferenceKind::Chapter);
        assert!(detection.reference.is_none(), "no verse should be invented");
        assert!(
            result.suggestions.is_empty(),
            "a bare chapter must not produce a suggestion"
        );
        assert_eq!(context.active_context().unwrap().last_verse, None);
    }

    // 3. Spoken chapter.
    #[test]
    fn spoken_chapter_reference_establishes_context() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(&provider, &mut context, "Romans chapter eight");
        assert_eq!(result.detections[0].kind, ReferenceKind::Chapter);
        assert_eq!(context.active_context().unwrap().chapter, 8);
    }

    // 4. Spoken full reference.
    #[test]
    fn spoken_full_reference_resolves_directly() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(
            &provider,
            &mut context,
            "Romans chapter eight verse twenty-eight",
        );
        assert_eq!(result.detections[0].kind, ReferenceKind::Direct);
        assert_eq!(
            result.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:28"
        );
    }

    // 5. Verse inheritance.
    #[test]
    fn bare_verse_inherits_the_active_chapter_context() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        process(&provider, &mut context, "Romans 8");
        let result = process(&provider, &mut context, "verse 28");

        assert_eq!(result.detections[0].kind, ReferenceKind::Verse);
        assert_eq!(
            result.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:28"
        );
    }

    // 6. Context survives intervening speech.
    #[test]
    fn context_survives_several_unrelated_intervening_segments() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        process(&provider, &mut context, "Turn with me to Romans chapter 8.");
        process(
            &provider,
            &mut context,
            "Paul is explaining something very important here.",
        );
        process(
            &provider,
            &mut context,
            "We have to understand the work of the Spirit.",
        );
        let result = process(&provider, &mut context, "Look at verse 28.");

        assert_eq!(
            result.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:28"
        );
    }

    // 7. Multiple verse movement.
    #[test]
    fn multiple_verse_movement_within_one_context() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        process(&provider, &mut context, "Romans 8");

        let r1 = process(&provider, &mut context, "Look at verse 28.");
        assert_eq!(r1.detections[0].kind, ReferenceKind::Verse);
        assert_eq!(
            r1.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:28"
        );

        let r2 = process(&provider, &mut context, "Now verse 31.");
        assert_eq!(r2.detections[0].kind, ReferenceKind::Sequential);
        assert_eq!(
            r2.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:31"
        );

        let r3 = process(&provider, &mut context, "Go back to verse 18.");
        assert_eq!(r3.detections[0].kind, ReferenceKind::Sequential);
        assert_eq!(
            r3.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:18"
        );
    }

    // 8. Context replacement.
    #[test]
    fn a_new_chapter_reference_replaces_the_active_context() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        process(&provider, &mut context, "Romans 8");
        process(&provider, &mut context, "Now let's go to John chapter 3.");
        let result = process(&provider, &mut context, "Verse 16.");

        assert_eq!(result.detections[0].kind, ReferenceKind::Verse);
        assert_eq!(
            result.detections[0].reference.as_ref().unwrap().to_string(),
            "JHN 3:16"
        );
    }

    // 9. No context.
    #[test]
    fn bare_verse_with_no_context_is_unresolved() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(&provider, &mut context, "verse 28");

        assert_eq!(result.detections[0].kind, ReferenceKind::Unresolved);
        assert!(result.suggestions.is_empty());
    }

    // 10. Invalid chapter.
    #[test]
    fn nonexistent_chapter_does_not_establish_context() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(&provider, &mut context, "Romans 999");

        assert_eq!(result.detections[0].kind, ReferenceKind::Unresolved);
        assert!(context.active_context().is_none());
    }

    // 11. Invalid verse.
    #[test]
    fn nonexistent_verse_is_unresolved() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(&provider, &mut context, "Romans 8:999");

        assert_eq!(result.detections[0].kind, ReferenceKind::Unresolved);
        assert!(result.suggestions.is_empty());
        assert!(
            context.active_context().is_none(),
            "an invalid direct reference must not commit context"
        );
    }

    // 12 & 13. Abbreviation and punctuation.
    #[test]
    fn abbreviated_and_punctuated_book_names_resolve() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");

        let r1 = process(&provider, &mut context, "Rom 8:28");
        assert_eq!(
            r1.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:28"
        );

        let mut context2 = DefaultScriptureContextManager::new("KJV");
        let r2 = process(&provider, &mut context2, "Rom. 8:28");
        assert_eq!(
            r2.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:28"
        );
    }

    // 14. Multiple references.
    #[test]
    fn multiple_explicit_references_in_one_segment_both_resolve_in_order() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(&provider, &mut context, "Romans 8:28 and John 3:16");

        assert_eq!(result.detections.len(), 2);
        assert_eq!(
            result.detections[0].reference.as_ref().unwrap().to_string(),
            "ROM 8:28"
        );
        assert_eq!(
            result.detections[1].reference.as_ref().unwrap().to_string(),
            "JHN 3:16"
        );
        assert_eq!(result.suggestions.len(), 2);
        // The most recent valid explicit reference updates the active context.
        assert_eq!(context.active_context().unwrap().book, "JHN");
    }

    // 15. Ambiguity.
    #[test]
    fn genuinely_ambiguous_bare_verse_produces_candidates_not_a_guess() {
        // Unlike the standard fixture, this one deliberately includes both
        // Romans 8:16 and John 3:16, constructing the scenario where a bare
        // "verse 16" right after a context replacement is plausible either
        // way.
        let provider = FakeBibleProvider::new(&[
            (
                "ROM",
                8,
                16,
                "The Spirit itself beareth witness with our spirit...",
            ),
            ("JHN", 3, 16, "For God so loved the world..."),
        ]);
        let mut context = DefaultScriptureContextManager::new("KJV");
        process(&provider, &mut context, "John 3");
        process(&provider, &mut context, "Romans 8");
        let result = process(&provider, &mut context, "verse 16");

        let detection = &result.detections[0];
        assert_eq!(detection.kind, ReferenceKind::Ambiguous);
        assert!(detection.reference.is_none(), "must not silently guess");
        assert_eq!(detection.candidates.len(), 2);
        assert_eq!(detection.candidates[0].reference.book, "ROM"); // current context, higher confidence
        assert_eq!(detection.candidates[1].reference.book, "JHN"); // shadow context, lower confidence
        assert!(
            detection.candidates[0].confidence.score > detection.candidates[1].confidence.score
        );
        assert!(
            result.suggestions.is_empty(),
            "an ambiguous detection must not produce a suggestion"
        );
    }

    // 16. Context history bound.
    #[test]
    fn history_remains_bounded_after_processing_many_references() {
        let entries: Vec<(&str, u32, u32, &str)> =
            (1..=50).map(|v| ("ROM", 8, v, "text")).collect();
        let provider = FakeBibleProvider::new(&entries);
        let mut context = DefaultScriptureContextManager::with_history_capacity("KJV", 5);
        process(&provider, &mut context, "Romans 8");
        for verse in 1..=50 {
            process(&provider, &mut context, &format!("verse {verse}"));
        }
        assert_eq!(context.recent_references(1000).len(), 5);
    }

    // 17. Database validation.
    #[test]
    fn parser_candidates_never_become_suggestions_without_provider_confirmation() {
        let empty_provider = FakeBibleProvider::new(&[]);
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(
            &empty_provider,
            &mut context,
            "Turn to Romans 8:28, or Romans chapter 9, or John 3:16.",
        );

        assert!(result.suggestions.is_empty());
        assert!(result
            .detections
            .iter()
            .all(|d| d.kind == ReferenceKind::Unresolved));
        assert!(context.active_context().is_none());
    }

    // 18. Suggestion creation.
    #[test]
    fn a_confidently_resolved_reference_creates_a_pending_suggestion() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(&provider, &mut context, "Romans 8:28");

        assert_eq!(result.suggestions.len(), 1);
        let suggestion = &result.suggestions[0];
        assert_eq!(suggestion.status, SuggestionStatus::Pending);
        assert!(
            matches!(&suggestion.kind, SuggestionKind::Scripture { reference } if reference == "ROM 8:28")
        );
    }

    // 19. No automatic projection.
    #[test]
    fn the_pipeline_never_produces_anything_beyond_a_pending_suggestion() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(&provider, &mut context, "Romans 8:28");

        // `Suggestion` has no state beyond `Pending` this pipeline can set,
        // and `ProcessedSegment` has no field capable of representing a
        // projected/active presentation state at all - the type system
        // guarantees this, not just this assertion.
        assert!(result
            .suggestions
            .iter()
            .all(|s| s.status == SuggestionStatus::Pending));
    }

    // 20. Determinism.
    #[test]
    fn identical_input_produces_identical_detections() {
        let segments = [
            "Turn with me to Romans chapter 8.",
            "Paul is explaining something very important here.",
            "Look at verse 28.",
            "Now verse 31.",
        ];

        let run = || {
            let provider = FakeBibleProvider::kjv_fixture();
            let mut context = DefaultScriptureContextManager::new("KJV");
            segments
                .iter()
                .flat_map(|segment| {
                    process(&provider, &mut context, segment)
                        .detections
                        .into_iter()
                        .map(|d| (d.kind, d.reference, d.confidence.score, d.raw_text))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(run(), run());
    }

    // 21. Paraphrase detection: a segment with no citation shape at all,
    // but wording that closely echoes Romans 8:28, produces a Pending
    // suggestion for that verse.
    #[test]
    fn a_close_paraphrase_with_no_citation_produces_a_paraphrase_suggestion() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(
            &provider,
            &mut context,
            "And we know that all things work together for good.",
        );

        assert_eq!(result.detections.len(), 1);
        let detection = &result.detections[0];
        assert_eq!(detection.kind, ReferenceKind::Paraphrase);
        assert_eq!(
            detection.reference.as_ref().unwrap().to_string(),
            "ROM 8:28"
        );
        assert_eq!(detection.confidence.source, ConfidenceSource::Heuristic);
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].status, SuggestionStatus::Pending);
        assert!(
            matches!(&result.suggestions[0].kind, SuggestionKind::Scripture { reference } if reference == "ROM 8:28")
        );
    }

    // 22. Paraphrase detection never overrides an explicit citation - it
    // only ever runs when nothing else already produced a suggestion.
    #[test]
    fn an_explicit_citation_is_never_second_guessed_by_the_paraphrase_fallback() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        let result = process(&provider, &mut context, "John 3:16");

        assert_eq!(result.detections.len(), 1, "no extra paraphrase detection");
        assert_eq!(result.detections[0].kind, ReferenceKind::Direct);
        assert_eq!(result.suggestions.len(), 1);
    }

    // 23. Paraphrase detection never mutates the active Scripture context -
    // it is not a citation, so it must not change what a later bare
    // "verse N" resolves against.
    #[test]
    fn paraphrase_detection_never_mutates_the_active_context() {
        let provider = FakeBibleProvider::kjv_fixture();
        let mut context = DefaultScriptureContextManager::new("KJV");
        process(&provider, &mut context, "Romans 8");
        let result = process(
            &provider,
            &mut context,
            "And we know that all things work together for good.",
        );

        assert_eq!(result.detections[0].kind, ReferenceKind::Paraphrase);
        assert_eq!(
            context.active_context().unwrap().chapter,
            8,
            "context must survive a paraphrase detection unchanged"
        );
        assert_eq!(
            context.active_context().unwrap().last_verse,
            None,
            "a paraphrase must never be recorded as a resolved verse in context"
        );
    }

    // 24. Short utterances and low-overlap prose never trigger the
    // paraphrase fallback, even when they happen to share one word with a
    // verse in the dataset.
    #[test]
    fn short_or_unrelated_segments_never_trigger_the_paraphrase_fallback() {
        let provider = FakeBibleProvider::kjv_fixture();
        for text in [
            "Chapter eight of our study is important.",
            "Romans is an important book.",
            "John was one of the disciples.",
            "Paul is showing us the work of the Spirit.",
            "Let us pray together this morning.",
        ] {
            let mut context = DefaultScriptureContextManager::new("KJV");
            let result = process(&provider, &mut context, text);
            assert!(
                result.suggestions.is_empty(),
                "{text:?} must not trigger a paraphrase suggestion"
            );
        }
    }
}
