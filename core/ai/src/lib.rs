//! AI domain: the `SpeechEngine` transcription contract, the
//! `EmbeddingEngine` semantic-search contract, and the `Suggestion` model
//! that every AI-produced proposal flows through on its way to human
//! review.

mod embedding_engine;
mod speech_engine;
mod suggestion;

pub use embedding_engine::{EmbeddingEngine, EmbeddingEngineError};
pub use speech_engine::{QualityTranscript, SpeechEngine, SpeechEngineError, TranscriptSegment};
pub use suggestion::{Suggestion, SuggestionKind, SuggestionStatus};
