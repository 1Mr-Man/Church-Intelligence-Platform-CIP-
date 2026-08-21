//! Scripture Context Manager - contract + supporting types.
//!
//! Models how pastors actually speak during services: a chapter (or a
//! chapter+verse) is named once, then referred to by bare `"verse N"`
//! fragments for as long as it stays relevant, even across unrelated
//! intervening speech:
//!
//! ```text
//! Pastor:  "Turn with me to Romans chapter 8." -> ACTIVE CONTEXT = Romans 8
//! Pastor:  "...several unrelated sentences..."
//! Pastor:  "Look at verse 28."                 -> resolves to Romans 8:28
//! Pastor:  "Now verse 31."                     -> resolves to Romans 8:31
//! Pastor:  "Go back to verse 18."               -> resolves to Romans 8:18
//! Pastor:  "Now let's go to John chapter 3."    -> ACTIVE CONTEXT = John 3
//! Pastor:  "Verse 16."                          -> resolves to John 3:16
//! ```
//!
//! See [`crate::DefaultScriptureContextManager`] for the Phase 1.1
//! implementation of this contract.

use crate::reference::ScriptureReference;
use chrono::{DateTime, Utc};
use cip_core_confidence::ConfidenceResult;
use serde::{Deserialize, Serialize};

/// The currently active scripture context: a book and chapter the pastor
/// has named, plus the most recently resolved verse within it (if any).
///
/// Deliberately does not embed a [`ScriptureReference`] (which always
/// requires a specific verse) - a chapter-only reference like "Romans 8" is
/// a valid, common context on its own, and inventing a verse to satisfy a
/// stricter type would violate the "never invent a verse" rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureContext {
    pub translation_id: String,
    /// Canonical book code (e.g. `"ROM"`), never a display name - see
    /// `book_alias`.
    pub book: String,
    pub chapter: u32,
    /// The most recently resolved verse within this context, if any (e.g.
    /// `Some(28)` after "verse 28" was resolved against "Romans 8"). `None`
    /// immediately after a bare chapter reference - no verse is invented.
    pub last_verse: Option<u32>,
    pub confidence: ConfidenceResult,
    pub established_at: DateTime<Utc>,
    /// Whether this context's book+chapter has been confirmed to exist by
    /// a `BibleProvider`. The Scripture Context Manager itself has no
    /// database access (`core/bible` never talks to storage directly) - it
    /// is the caller's responsibility to validate *before* asking the
    /// manager to establish a context, so every `ScriptureContext` the
    /// manager actually holds is `valid: true` in practice. The field
    /// exists so the contract can represent an unvalidated/provisional
    /// context if a future caller ever needs one, without an API change.
    pub valid: bool,
}

/// One candidate reference offered when a fragment is ambiguous, with the
/// confidence CIP has in that specific candidate (higher for the current
/// active context, lower for a recently-replaced one - see
/// `DefaultScriptureContextManager`'s module docs for the exact rule).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbiguousCandidate {
    pub reference: ScriptureReference,
    pub confidence: ConfidenceResult,
}

/// Outcome of feeding a new fragment into the context manager.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContextResolution {
    /// A brand new context was established (e.g. "Romans 8").
    Established(ScriptureContext),
    /// A fragment resolved unambiguously against the active context
    /// (e.g. "verse 28" while Romans 8 is active).
    Resolved(ScriptureReference, ConfidenceResult),
    /// The active context was replaced by a new one.
    Replaced {
        previous: ScriptureContext,
        current: ScriptureContext,
    },
    /// The fragment could resolve to more than one reference and needs
    /// human disambiguation (e.g. a bare verse spoken just after a context
    /// replacement, plausible against either the new or the just-replaced
    /// context). Candidates are provisional - the caller must validate each
    /// one against a `BibleProvider` before treating it as real.
    Ambiguous(Vec<AmbiguousCandidate>),
    /// The fragment could not be resolved at all (e.g. "verse 28" with no
    /// context ever established).
    Unresolved,
}

/// The Scripture Context Manager contract.
///
/// Implementations own the "active context + recent references" state
/// machine described above. `core/service`'s Bible Intelligence Core
/// orchestrator calls this as scripture fragments arrive from transcript
/// processing; nothing outside `core/bible` should track scripture context
/// state independently.
///
/// This trait is intentionally free of any `BibleProvider`/database
/// access: implementations track *what was said*, not *whether it's real*.
/// The orchestrator validates a candidate against a `BibleProvider` before
/// ever calling [`resolve`](ScriptureContextManager::resolve) with a
/// book+chapter fragment, and before treating a `Resolved`/`Ambiguous`
/// verse candidate as final - see `core/service`'s `bible_intelligence`
/// module.
pub trait ScriptureContextManager: Send + Sync {
    /// Feed a new fragment (full or partial reference) into the manager.
    fn resolve(
        &mut self,
        fragment: crate::reference::PartialScriptureReference,
    ) -> ContextResolution;

    /// The currently active context, if any.
    fn active_context(&self) -> Option<ScriptureContext>;

    /// Most-recently-resolved references, newest first, bounded by `limit`.
    fn recent_references(&self, limit: usize) -> Vec<ScriptureReference>;

    /// Human confirmation of the active context (raises its confidence to
    /// `High` and is never overwritten by a later ambiguous match).
    fn confirm_active(&mut self);

    /// Human rejection of the active context (clears it without adding it
    /// to recent references).
    fn reject_active(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::PartialScriptureReference;

    /// A no-op implementation exists only to prove the trait is
    /// object-safe and wireable; it is not the real resolver - see
    /// `context_manager::DefaultScriptureContextManager` for that.
    struct NullContextManager;

    impl ScriptureContextManager for NullContextManager {
        fn resolve(&mut self, _fragment: PartialScriptureReference) -> ContextResolution {
            ContextResolution::Unresolved
        }
        fn active_context(&self) -> Option<ScriptureContext> {
            None
        }
        fn recent_references(&self, _limit: usize) -> Vec<ScriptureReference> {
            vec![]
        }
        fn confirm_active(&mut self) {}
        fn reject_active(&mut self) {}
    }

    #[test]
    fn null_manager_satisfies_the_trait_object_contract() {
        let mut manager: Box<dyn ScriptureContextManager> = Box::new(NullContextManager);
        let resolution = manager.resolve(PartialScriptureReference {
            verse_start: Some(28),
            ..Default::default()
        });
        assert_eq!(resolution, ContextResolution::Unresolved);
        assert!(manager.active_context().is_none());
    }
}
