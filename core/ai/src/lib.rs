//! AI domain: the `SpeechEngine` transcription contract and the
//! `Suggestion` model that every AI-produced proposal flows through on its
//! way to human review.

mod speech_engine;
mod suggestion;

pub use speech_engine::{SpeechEngine, SpeechEngineError, TranscriptSegment};
pub use suggestion::{Suggestion, SuggestionKind, SuggestionStatus};
