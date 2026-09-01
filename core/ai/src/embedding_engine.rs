use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmbeddingEngineError {
    #[error("embedding engine not initialized")]
    NotInitialized,
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("embedding failed: {0}")]
    EmbeddingFailed(String),
}

/// The provider/adaptor contract for turning text into a fixed-length
/// numeric embedding, for semantic (meaning-based) Bible verse search -
/// `core` depends only on this trait; `ai/embedding` supplies concrete
/// implementations (a local model first - no cloud dependency is required
/// for CIP to function offline, matching `SpeechEngine`'s own contract).
///
/// Deliberately narrower than `SpeechEngine`: embedding is a single
/// stateless `text -> Vec<f32>` call, not a streaming/buffering process -
/// there is no interim/final distinction and no audio format to negotiate.
pub trait EmbeddingEngine: Send + Sync {
    fn is_ready(&self) -> bool;

    /// A stable identifier for exactly which model produces this engine's
    /// vectors (e.g. `"all-MiniLM-L6-v2"`) - never a display name or a path.
    /// Verse-embedding storage keys stored vectors by this string precisely
    /// so that switching, upgrading, or misconfiguring the embedding model
    /// can never silently mix vectors from two different models into one
    /// similarity comparison; matching `dimensions()` alone isn't enough
    /// since two unrelated models can share a width.
    fn model_id(&self) -> &str;

    /// The fixed length of every vector this engine returns from
    /// [`embed`](Self::embed) - callers (verse-embedding storage, cosine
    /// similarity scoring) need this to validate stored embeddings still
    /// match the currently configured model before comparing against them.
    fn dimensions(&self) -> usize;

    /// Embed one piece of text. Never partial/streaming - always the
    /// complete embedding for the complete input, computed synchronously.
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingEngineError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NullEmbeddingEngine;

    impl EmbeddingEngine for NullEmbeddingEngine {
        fn is_ready(&self) -> bool {
            false
        }
        fn model_id(&self) -> &str {
            "null"
        }
        fn dimensions(&self) -> usize {
            0
        }
        fn embed(&self, _text: &str) -> Result<Vec<f32>, EmbeddingEngineError> {
            Err(EmbeddingEngineError::NotInitialized)
        }
    }

    #[test]
    fn not_ready_engine_reports_not_initialized() {
        let engine = NullEmbeddingEngine;
        assert!(!engine.is_ready());
        assert!(matches!(
            engine.embed("Romans eight twenty eight"),
            Err(EmbeddingEngineError::NotInitialized)
        ));
    }

    #[test]
    fn satisfies_the_trait_object_contract() {
        let engine: Box<dyn EmbeddingEngine> = Box::new(NullEmbeddingEngine);
        assert!(!engine.is_ready());
        assert_eq!(engine.dimensions(), 0);
    }
}
