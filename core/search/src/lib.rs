//! Search domain: a single source-agnostic contract the UI can query
//! regardless of which domain (Bible, sermon, presentation) backs a given
//! result.

use cip_core_confidence::ConfidenceResult;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Which domain a `SearchResult` came from. Kept as a small closed enum for
/// Phase 1's scope; new sources are added here as their domains land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchResultSource {
    Bible,
    Sermon,
    Presentation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub source: SearchResultSource,
    /// Opaque id meaningful to the originating domain (e.g. a verse
    /// reference string, a presentation item id).
    pub reference_id: String,
    pub title: String,
    pub snippet: String,
    pub relevance: ConfidenceResult,
}

#[derive(Debug, Error)]
pub enum SearchEngineError {
    #[error("query was empty")]
    EmptyQuery,
    #[error("search backend error: {0}")]
    Backend(String),
}

/// The contract the UI searches through. Implementations compose one or
/// more domain-specific lookups (e.g. `BibleProvider::search`) and merge
/// them into a single ranked result set - `core/search` owns ranking policy
/// so no individual domain has to know about the others.
pub trait SearchEngine: Send + Sync {
    fn search(&self, query: &str) -> Result<Vec<SearchResult>, SearchEngineError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptySearchEngine;

    impl SearchEngine for EmptySearchEngine {
        fn search(&self, query: &str) -> Result<Vec<SearchResult>, SearchEngineError> {
            if query.is_empty() {
                return Err(SearchEngineError::EmptyQuery);
            }
            Ok(vec![])
        }
    }

    #[test]
    fn empty_query_is_rejected() {
        let engine = EmptySearchEngine;
        assert!(matches!(
            engine.search(""),
            Err(SearchEngineError::EmptyQuery)
        ));
    }

    #[test]
    fn non_empty_query_returns_a_result_set() {
        let engine = EmptySearchEngine;
        assert!(engine.search("Romans 8").unwrap().is_empty());
    }
}
