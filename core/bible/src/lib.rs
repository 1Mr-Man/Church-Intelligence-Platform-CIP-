//! Bible domain: the `BibleProvider` contract, text normalization,
//! reference detection, and the Scripture Context Manager (interface +
//! the Phase 1.1 `DefaultScriptureContextManager` implementation). See
//! `context` and `context_manager` module docs for the Scripture Context
//! Manager's behavior, and `detection`/`normalize` for how transcript text
//! becomes reference candidates.

pub mod book_alias;
mod context;
mod context_manager;
pub mod detection;
#[cfg(test)]
mod fixtures;
pub mod integrity;
pub mod normalize;
mod provider;
pub mod range;
mod reference;
pub mod search;

pub use book_alias::{canonicalize_book, CanonicalBook};
pub use context::{
    AmbiguousCandidate, ContextResolution, ScriptureContext, ScriptureContextManager,
};
pub use context_manager::DefaultScriptureContextManager;
pub use detection::{detect_candidates, DetectedCandidate, ReferenceKind};
pub use integrity::{check_bible_integrity, IntegrityIssue, IntegrityReport, IntegrityStatus};
pub use provider::{
    BibleBook, BibleChapter, BibleProvider, BibleProviderError, BibleTranslation, BibleVerse,
    Testament,
};
pub use range::{get_verse_range, VerseRangeError};
pub use reference::{PartialScriptureReference, ScriptureReference};
pub use search::{search_bible, BibleSearchError, BibleSearchResult};
