//! [`DefaultScriptureContextManager`] - the Phase 1.1 implementation of
//! [`ScriptureContextManager`].
//!
//! ## State
//!
//! - `active`: the current context (book/chapter/last-verse), if any.
//! - `shadow`: the *previous* context, kept for exactly one subsequent
//!   bare-verse resolution after a context replacement, then cleared -
//!   this is what makes ambiguity detection possible (see below) without
//!   letting old context linger indefinitely.
//! - `history`: a bounded, most-recent-first deque of resolved references,
//!   for diagnostics and (like `shadow`) future ambiguity heuristics.
//!
//! ## Validation happens outside this type
//!
//! `resolve` never touches a `BibleProvider` - see the trait's docs. For a
//! book+chapter fragment, the caller (`core/service`'s orchestrator) must
//! validate the chapter exists *before* calling `resolve`, since a
//! successful call unconditionally commits it as the active context. For a
//! bare verse fragment, `resolve` only *proposes* a candidate reference (or
//! several, if ambiguous) - it does not update `last_verse` or `history`
//! itself. The caller validates the candidate(s) against a `BibleProvider`
//! and, for whichever one(s) turn out real, calls
//! [`record_resolved`](DefaultScriptureContextManager::record_resolved) to
//! commit it. This keeps an invalid verse number (e.g. "verse 999") from
//! ever poisoning the context's `last_verse`/history, without requiring
//! `resolve`'s signature (part of the existing trait) to change.
//!
//! ## Ambiguity
//!
//! Immediately after a context replacement, the *next* bare verse fragment
//! is checked against both the new active context and the just-replaced
//! shadow context. If the caller finds both candidates valid against the
//! Bible data, that's a genuine ambiguity (e.g. "verse 16" shortly after
//! switching from John 3 to Romans 8: both John 3:16 and Romans 8:16 are
//! real verses). The current context's candidate is scored higher
//! (evidence: it's what's actively being taught from) than the shadow's
//! (evidence: recency only). The shadow is consumed (cleared) after this
//! one check either way, so ambiguity resolution never grows unbounded
//! state.

use crate::context::{
    AmbiguousCandidate, ContextResolution, ScriptureContext, ScriptureContextManager,
};
use crate::reference::{PartialScriptureReference, ScriptureReference};
use chrono::Utc;
use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use std::collections::VecDeque;

/// Confidence assigned to a newly-established context. The caller has
/// already validated the book+chapter exist before calling `resolve`, so
/// this reflects "explicit reference pattern + valid chapter" evidence.
const CONTEXT_ESTABLISHED_CONFIDENCE: f32 = 0.95;
/// Confidence for a bare verse resolved against the single active context.
const UNAMBIGUOUS_VERSE_CONFIDENCE: f32 = 0.85;
/// Confidence for the current-context candidate when a bare verse is
/// ambiguous between it and a just-replaced shadow context.
const AMBIGUOUS_CURRENT_CONFIDENCE: f32 = 0.71;
/// Confidence for the shadow-context candidate in the same situation -
/// lower, since its only evidence is recency, not active relevance.
const AMBIGUOUS_SHADOW_CONFIDENCE: f32 = 0.64;

const DEFAULT_MAX_HISTORY: usize = 20;

pub struct DefaultScriptureContextManager {
    active: Option<ScriptureContext>,
    shadow: Option<ScriptureContext>,
    history: VecDeque<ScriptureReference>,
    max_history: usize,
    translation_id: String,
}

impl DefaultScriptureContextManager {
    pub fn new(translation_id: impl Into<String>) -> Self {
        Self::with_history_capacity(translation_id, DEFAULT_MAX_HISTORY)
    }

    /// Same as [`new`](Self::new), but with an explicit bound on how many
    /// recent references [`recent_references`](ScriptureContextManager::recent_references)
    /// can ever return - "use a reasonable bounded history size,
    /// configurable if appropriate."
    pub fn with_history_capacity(translation_id: impl Into<String>, max_history: usize) -> Self {
        Self {
            active: None,
            shadow: None,
            history: VecDeque::with_capacity(max_history.min(64)),
            max_history,
            translation_id: translation_id.into(),
        }
    }

    /// Commit a verse the caller has validated against a `BibleProvider`:
    /// updates `last_verse` (if `reference` belongs to the active context)
    /// and pushes it into the bounded history. Called strictly after
    /// validation - see the module docs.
    pub fn record_resolved(&mut self, reference: ScriptureReference) {
        if let Some(active) = self.active.as_mut() {
            if active.book == reference.book && active.chapter == reference.chapter {
                active.last_verse = Some(reference.verse_start);
            }
        }
        self.history.push_front(reference);
        self.history.truncate(self.max_history);
    }
}

impl ScriptureContextManager for DefaultScriptureContextManager {
    fn resolve(&mut self, fragment: PartialScriptureReference) -> ContextResolution {
        match (fragment.book, fragment.chapter, fragment.verse_start) {
            (Some(book), Some(chapter), verse_start) => {
                let new_context = ScriptureContext {
                    translation_id: self.translation_id.clone(),
                    book,
                    chapter,
                    last_verse: verse_start,
                    confidence: ConfidenceResult::new(
                        CONTEXT_ESTABLISHED_CONFIDENCE,
                        ConfidenceSource::Heuristic,
                        Some("explicit book+chapter, validated against Bible data".to_string()),
                    ),
                    established_at: Utc::now(),
                    valid: true,
                };

                let previous = self.active.take();
                self.shadow = previous.clone();
                self.active = Some(new_context.clone());

                if let Some(verse) = verse_start {
                    self.history.push_front(ScriptureReference {
                        translation_id: new_context.translation_id.clone(),
                        book: new_context.book.clone(),
                        chapter,
                        verse_start: verse,
                        verse_end: None,
                    });
                    self.history.truncate(self.max_history);
                }

                match previous {
                    Some(previous) => ContextResolution::Replaced {
                        previous,
                        current: new_context,
                    },
                    None => ContextResolution::Established(new_context),
                }
            }

            (None, None, Some(verse)) => {
                let current_candidate = self.active.as_ref().map(|ctx| AmbiguousCandidate {
                    reference: ScriptureReference {
                        translation_id: self.translation_id.clone(),
                        book: ctx.book.clone(),
                        chapter: ctx.chapter,
                        verse_start: verse,
                        verse_end: None,
                    },
                    confidence: ConfidenceResult::new(
                        if self.shadow.is_some() {
                            AMBIGUOUS_CURRENT_CONFIDENCE
                        } else {
                            UNAMBIGUOUS_VERSE_CONFIDENCE
                        },
                        ConfidenceSource::Heuristic,
                        Some("current active context".to_string()),
                    ),
                });

                let shadow_candidate = self.shadow.take().map(|ctx| AmbiguousCandidate {
                    reference: ScriptureReference {
                        translation_id: self.translation_id.clone(),
                        book: ctx.book.clone(),
                        chapter: ctx.chapter,
                        verse_start: verse,
                        verse_end: None,
                    },
                    confidence: ConfidenceResult::new(
                        AMBIGUOUS_SHADOW_CONFIDENCE,
                        ConfidenceSource::Heuristic,
                        Some("recently active context".to_string()),
                    ),
                });

                let mut candidates: Vec<AmbiguousCandidate> =
                    current_candidate.into_iter().collect();
                candidates.extend(shadow_candidate);

                match candidates.len() {
                    0 => ContextResolution::Unresolved,
                    1 => {
                        let only = candidates.into_iter().next().unwrap();
                        ContextResolution::Resolved(only.reference, only.confidence)
                    }
                    _ => ContextResolution::Ambiguous(candidates),
                }
            }

            _ => ContextResolution::Unresolved,
        }
    }

    fn active_context(&self) -> Option<ScriptureContext> {
        self.active.clone()
    }

    fn recent_references(&self, limit: usize) -> Vec<ScriptureReference> {
        self.history.iter().take(limit).cloned().collect()
    }

    fn confirm_active(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.confidence = ConfidenceResult::new(
                1.0,
                ConfidenceSource::Human,
                Some("confirmed by operator".to_string()),
            );
        }
    }

    fn reject_active(&mut self) {
        self.active = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book_chapter(book: &str, chapter: u32) -> PartialScriptureReference {
        PartialScriptureReference {
            book: Some(book.to_string()),
            chapter: Some(chapter),
            verse_start: None,
            verse_end: None,
        }
    }

    fn bare_verse(verse: u32) -> PartialScriptureReference {
        PartialScriptureReference {
            book: None,
            chapter: None,
            verse_start: Some(verse),
            verse_end: None,
        }
    }

    #[test]
    fn chapter_only_reference_establishes_context_without_inventing_a_verse() {
        let mut manager = DefaultScriptureContextManager::new("KJV");
        let resolution = manager.resolve(book_chapter("ROM", 8));
        let ScriptureContext {
            book,
            chapter,
            last_verse,
            ..
        } = match resolution {
            ContextResolution::Established(ctx) => ctx,
            other => panic!("expected Established, got {other:?}"),
        };
        assert_eq!((book.as_str(), chapter, last_verse), ("ROM", 8, None));
        assert_eq!(manager.active_context().unwrap().last_verse, None);
    }

    #[test]
    fn bare_verse_with_no_active_context_is_unresolved() {
        let mut manager = DefaultScriptureContextManager::new("KJV");
        assert_eq!(
            manager.resolve(bare_verse(28)),
            ContextResolution::Unresolved
        );
    }

    #[test]
    fn bare_verse_resolves_against_a_single_active_context() {
        let mut manager = DefaultScriptureContextManager::new("KJV");
        manager.resolve(book_chapter("ROM", 8));
        match manager.resolve(bare_verse(28)) {
            ContextResolution::Resolved(reference, _) => {
                assert_eq!(
                    (
                        reference.book.as_str(),
                        reference.chapter,
                        reference.verse_start
                    ),
                    ("ROM", 8, 28)
                );
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn context_survives_being_re_peeked_without_a_new_reference_between() {
        let mut manager = DefaultScriptureContextManager::new("KJV");
        manager.resolve(book_chapter("ROM", 8));
        // Several bare-verse resolves in a row, as if intervening prose
        // produced no candidates at all (nothing calls `resolve` for
        // non-reference speech) - context must still be there.
        assert!(manager.active_context().is_some());
        assert!(manager.active_context().is_some());
        match manager.resolve(bare_verse(28)) {
            ContextResolution::Resolved(reference, _) => assert_eq!(reference.chapter, 8),
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn replacing_context_clears_the_prior_last_verse() {
        let mut manager = DefaultScriptureContextManager::new("KJV");
        manager.resolve(book_chapter("ROM", 8));
        manager.record_resolved(ScriptureReference::single("KJV", "ROM", 8, 28));
        assert_eq!(manager.active_context().unwrap().last_verse, Some(28));

        manager.resolve(book_chapter("JHN", 3));
        assert_eq!(manager.active_context().unwrap().book, "JHN");
        assert_eq!(manager.active_context().unwrap().last_verse, None);
    }

    #[test]
    fn ambiguity_window_offers_both_candidates_once_then_closes() {
        let mut manager = DefaultScriptureContextManager::new("KJV");
        manager.resolve(book_chapter("JHN", 3));
        manager.resolve(book_chapter("ROM", 8)); // replaces John 3 -> shadow = John 3

        match manager.resolve(bare_verse(16)) {
            ContextResolution::Ambiguous(candidates) => {
                assert_eq!(candidates.len(), 2);
                assert_eq!(candidates[0].reference.book, "ROM");
                assert_eq!(candidates[1].reference.book, "JHN");
                assert!(candidates[0].confidence.score > candidates[1].confidence.score);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }

        // Shadow was consumed by the check above - the *next* bare verse
        // resolves unambiguously against the still-active Romans 8.
        match manager.resolve(bare_verse(17)) {
            ContextResolution::Resolved(reference, _) => assert_eq!(reference.book, "ROM"),
            other => panic!("expected Resolved (shadow consumed), got {other:?}"),
        }
    }

    #[test]
    fn record_resolved_only_updates_last_verse_for_the_matching_context() {
        let mut manager = DefaultScriptureContextManager::new("KJV");
        manager.resolve(book_chapter("ROM", 8));
        // A verse from a different book/chapter than the active context
        // (e.g. the shadow candidate in an ambiguity) must not overwrite
        // the active context's last_verse.
        manager.record_resolved(ScriptureReference::single("KJV", "JHN", 3, 16));
        assert_eq!(manager.active_context().unwrap().last_verse, None);
    }

    #[test]
    fn history_stays_bounded_across_many_references() {
        let mut manager = DefaultScriptureContextManager::with_history_capacity("KJV", 5);
        manager.resolve(book_chapter("ROM", 8));
        for verse in 1..=50u32 {
            manager.record_resolved(ScriptureReference::single("KJV", "ROM", 8, verse));
        }
        assert_eq!(manager.recent_references(100).len(), 5);
        // Most-recent-first.
        assert_eq!(manager.recent_references(1)[0].verse_start, 50);
    }

    #[test]
    fn reject_active_clears_context_without_touching_history() {
        let mut manager = DefaultScriptureContextManager::new("KJV");
        manager.resolve(book_chapter("ROM", 8));
        manager.record_resolved(ScriptureReference::single("KJV", "ROM", 8, 28));
        manager.reject_active();
        assert!(manager.active_context().is_none());
        assert_eq!(manager.recent_references(10).len(), 1);
    }

    #[test]
    fn confirm_active_raises_confidence_to_human_high() {
        let mut manager = DefaultScriptureContextManager::new("KJV");
        manager.resolve(book_chapter("ROM", 8));
        manager.confirm_active();
        let ctx = manager.active_context().unwrap();
        assert_eq!(ctx.confidence.source, ConfidenceSource::Human);
        assert!(ctx.confidence.is_auto_acceptable());
    }
}
