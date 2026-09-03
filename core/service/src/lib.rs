//! Service domain: the live-service session lifecycle, the audio capture
//! contract it depends on, and the Bible Intelligence Core pipeline
//! (`bible_intelligence`) that composes `core/bible` and `core/ai` for a
//! live transcript.

mod audio_engine;
mod bible_intelligence;
mod service_session;

pub use audio_engine::{
    AudioChunk, AudioChunkSink, AudioDevice, AudioEngine, AudioEngineError, AudioEngineStatus,
};
pub use bible_intelligence::{
    process_transcript_segment, process_transcript_segment_with_semantic_search,
    retry_paraphrase_or_semantic_with_fuller_context, ProcessedSegment, ScriptureDetection,
    SemanticSearch,
};
pub use service_session::{ServiceSession, ServiceStatus};
