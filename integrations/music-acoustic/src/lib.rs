//! `AcousticMusicRecognizer` implementations (Phase 2.2).
//!
//! - [`NullAcousticMusicRecognizer`] - the safe default: always
//!   `Unavailable`, rejects recognition. Used whenever no real
//!   recognizer is configured, so "no acoustic model" is never fatal.
//! - [`ScriptedAcousticMusicRecognizer`] - a deterministic test/demo
//!   adapter for exercising the acoustic wiring without a microphone or
//!   model.
//! - [`LocalAcousticMusicRecognizer`] - the real local-model integration
//!   boundary: genuine configuration and status resolution, honestly
//!   `Unavailable`/`Error` until a real backend is chosen and wired in a
//!   future phase (see its module docs for exactly why).
//!
//! All three satisfy the same `cip_core_music::AcousticMusicRecognizer`
//! trait - callers never know which one they're holding, mirroring
//! `cip_ai_speech`'s three `SpeechEngine` implementations exactly.

mod local;
mod null;
mod scripted;

pub use local::{LocalAcousticConfig, LocalAcousticMusicRecognizer, MODEL_MANIFEST_FILENAME};
pub use null::NullAcousticMusicRecognizer;
pub use scripted::{ScriptedAcousticMusicRecognizer, ScriptedAcousticStep};
