//! Scripture Context Manager - interface boundary only (Phase 1).
//!
//! Planned behavior (not implemented yet):
//!
//! ```text
//! Pastor:  "Romans 8"        -> ACTIVE SCRIPTURE CONTEXT = Romans 8
//! Pastor:  "verse 28"        -> resolves to Romans 8:28
//! Pastor:  "verse 31"        -> resolves to Romans 8:31
//! Pastor:  "go back to verse 18" -> resolves to Romans 8:18
//! ```
//!
//! The manager tracks one *active* context (book + chapter) plus a short
//! history of recently resolved references, so a bare fragment like "verse
//! 28" or "go back to verse 18" can be resolved without the book/chapter
//! being repeated. Every resolution carries a [`ConfidenceResult`] because
//! fragment resolution is inherently a heuristic: "verse 28" is unambiguous
//! only while exactly one context is active.
//!
//! This module defines the trait and the supporting types only. The actual
//! resolution algorithm (fuzzy matching, multi-context ambiguity, timeout
//! decay of the active context) is future work - implementing it now would
//! be coupling a heuristic we haven't validated yet to a public contract.

use crate::reference::{PartialScriptureReference, ScriptureReference};
use chrono::{DateTime, Utc};
use cip_core_confidence::ConfidenceResult;
use serde::{Deserialize, Serialize};

/// The currently active scripture context (e.g. "Romans 8" after the pastor
/// names a chapter but before a specific verse is spoken).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureContext {
    pub reference: ScriptureReference,
    pub confidence: ConfidenceResult,
    pub established_at: DateTime<Utc>,
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
    /// human disambiguation (e.g. no active context yet, or a book name
    /// that matches multiple candidates).
    Ambiguous(Vec<ScriptureReference>),
    /// The fragment could not be resolved at all (e.g. "verse 28" with no
    /// context ever established).
    Unresolved,
}

/// The Scripture Context Manager contract.
///
/// Implementations own the "active context + recent references" state
/// machine described above. `core/service` calls this as scripture
/// fragments arrive from `SpeechEngine`/transcript processing; nothing
/// outside `core/bible` should track scripture context state independently.
pub trait ScriptureContextManager: Send + Sync {
    /// Feed a new fragment (full or partial reference) into the manager.
    fn resolve(&mut self, fragment: PartialScriptureReference) -> ContextResolution;

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

    /// A no-op implementation exists only to prove the trait is
    /// object-safe and wireable; it is not the real resolver.
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
