//! Bible domain: the `BibleProvider` contract and the Scripture Context
//! Manager interface boundary. See `context` module docs for the planned
//! (not yet implemented) context-resolution behavior.

mod context;
mod provider;
mod reference;

pub use context::{ContextResolution, ScriptureContext, ScriptureContextManager};
pub use provider::{
    BibleBook, BibleChapter, BibleProvider, BibleProviderError, BibleTranslation, BibleVerse,
    Testament,
};
pub use reference::{PartialScriptureReference, ScriptureReference};
