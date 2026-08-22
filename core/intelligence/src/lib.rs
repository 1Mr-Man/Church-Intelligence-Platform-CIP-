//! The shared intelligence architecture (Phase 2.0): the common contracts
//! future Bible/Music/Sermon/Content/CrossDomain engines are built on.
//!
//! ```text
//! Transcript
//!    |
//! IntelligenceContext  (bounded: recent transcript, active Scripture
//!    |                  context, recent findings, service/content state)
//!    v
//! IntelligenceInput
//!    |
//! independent IntelligenceEngine instances (never call each other)
//!    |
//! IntelligenceResult
//!    |
//! IntelligenceFinding
//!    |
//! operator workflow (review/accept/reject - never automatic)
//! ```
//!
//! This crate does **not** implement Music, Sermon, or Content
//! intelligence - `IntelligenceDomain`/`FindingKind` reserve the shape
//! those engines will occupy, but the only real engine in Phase 2.0 is
//! [`bible_adapter::BibleIntelligenceEngine`], a thin compatibility
//! boundary over the existing, unchanged `core/service::process_transcript_segment`
//! pipeline. See `docs/intelligence-architecture.md`.
//!
//! ## Why this crate depends on other `core/*` crates
//!
//! Every other `core/*` domain crate depends on at most `cip-core-confidence`
//! (see `docs/architecture.md#boundaries`) - `core/service` is the one
//! documented exception, composing `core/bible` + `core/ai` because
//! cross-domain flows are expected to go through exactly one composition
//! point rather than create a cycle between domains. `core/intelligence`
//! is the next composition point up the same stack, for the same reason:
//! `IntelligenceContext` is explicitly required (Phase 2.0 spec section
//! 14) to expose the active Scripture context (`cip_core_bible::ScriptureContext`),
//! the current transcript segment (`cip_core_ai::TranscriptSegment`), the
//! service status (`cip_core_service::ServiceStatus`), and installed
//! content metadata (`cip_core_content::ContentMetadata`) - reusing each of
//! these exactly, rather than duplicating them, is a hard requirement
//! ("do not invent a second X"), not a convenience. This crate still
//! depends on nothing outside `core/*` and `cip-core-confidence`: no
//! Tauri, no React, no SQLite implementation, no `cpal`, no `whisper-rs`,
//! no network client (see `docs/intelligence-architecture.md#offline-operation`).

#[cfg(test)]
mod acceptance_tests;
pub mod bible_adapter;
mod context;
mod correlation;
mod domain;
mod engine;
mod evidence;
mod finding;
#[cfg(test)]
mod fixtures;
pub mod music_adapter;
mod queue;
mod registry;

pub use bible_adapter::{BibleIntelligenceEngine, BIBLE_ENGINE_ID, BIBLE_ENGINE_VERSION};
pub use context::{
    ContextBounds, ContextWindow, ContextWindowView, IntelligenceContext, ServiceEventSummary,
    DEFAULT_MAX_RECENT_FINDINGS, DEFAULT_MAX_RECENT_SERVICE_EVENTS,
    DEFAULT_MAX_RECENT_TRANSCRIPT_SEGMENTS,
};
pub use correlation::{CorrelationKind, IntelligenceCorrelation};
pub use domain::{
    baseline_priority, AssertionLevel, FindingKind, FindingStatus, IntelligenceDomain,
    IntelligencePriority,
};
pub use engine::{
    EngineCapability, EngineIdentity, IntelligenceEngine, IntelligenceError, IntelligenceInput,
    IntelligenceResult, RuntimeCapabilities,
};
pub use evidence::{EvidenceSource, IntelligenceProvenance};
pub use finding::IntelligenceFinding;
pub use music_adapter::{
    acoustic_recognition_available, MusicIntelligenceEngine, MUSIC_ENGINE_ID, MUSIC_ENGINE_VERSION,
};
pub use queue::{FindingQueue, QueueAddOutcome};
pub use registry::{EngineOutcome, IntelligenceEngineRegistry};
