//! `EmbeddingEngine` implementations, for Phase 4.4's semantic (meaning-
//! based) Bible verse search.
//!
//! - [`NullEmbeddingEngine`] - the safe default: reports itself not ready,
//!   rejects every `embed` call. What the application uses whenever no
//!   real embedding model is configured, so "no semantic search" is never
//!   fatal - mirrors `cip_ai_speech::NullSpeechEngine` exactly.
//! - `CandleEmbeddingEngine` (module `candle_engine`, behind the
//!   `semantic-search` Cargo feature) - a real local backend using
//!   `candle`/`candle-transformers` (HuggingFace's pure-Rust ML framework)
//!   running `sentence-transformers/all-MiniLM-L6-v2`. Off by default:
//!   compiling candle/tokenizers is a genuine build-time cost this crate
//!   shouldn't impose on every default build, and (like Whisper's model)
//!   the model weights themselves are not bundled - the operator supplies
//!   them, matching `cip_ai_speech::WhisperSpeechEngine`'s own model-
//!   provisioning precedent exactly. See `docs/phase-4-4-semantic-bible-search.md`.
//!
//! [`pooling`] holds the pure, always-compiled (never feature-gated)
//! mean-pooling/L2-normalization math both engines' post-processing needs -
//! kept independent of `candle`'s tensor types so it's directly unit-
//! testable without the heavy feature enabled.

#[cfg(feature = "semantic-search")]
mod candle_engine;
pub mod pooling;

#[cfg(feature = "semantic-search")]
pub use candle_engine::CandleEmbeddingEngine;

use cip_core_ai::{EmbeddingEngine, EmbeddingEngineError};

#[derive(Default)]
pub struct NullEmbeddingEngine;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_engine_reports_not_ready_and_rejects_text() {
        let engine = NullEmbeddingEngine;
        assert!(!engine.is_ready());
        assert_eq!(engine.dimensions(), 0);
        assert!(matches!(
            engine.embed("Romans eight twenty eight"),
            Err(EmbeddingEngineError::NotInitialized)
        ));
    }

    #[test]
    fn null_engine_satisfies_the_trait_object_contract() {
        let engine: Box<dyn EmbeddingEngine> = Box::new(NullEmbeddingEngine);
        assert!(!engine.is_ready());
    }
}
