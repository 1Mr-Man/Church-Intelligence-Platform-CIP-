//! `AcousticMusicRecognizer` implementations (Phase 2.2).
//!
//! - [`NullAcousticMusicRecognizer`] - the safe default: always
//!   `Unavailable`, rejects recognition. Used whenever no real
//!   recognizer is configured, so "no acoustic model" is never fatal.
//! - [`ScriptedAcousticMusicRecognizer`] - a deterministic test/demo
//!   adapter for exercising the acoustic wiring without a microphone or
//!   model.
//! - [`LocalAcousticMusicRecognizer`] - the real local-model integration
//!   boundary: genuine configuration and status resolution. As of Phase
//!   7.1, backed by a real spectral landmark (constellation) hashing
//!   fingerprint algorithm (see [`fingerprint`]) - `Available` once a
//!   manifest naming at least one enrollable reference recording is
//!   configured, honestly `Unavailable`/`Error` otherwise.
//!
//! All three satisfy the same `cip_core_music::AcousticMusicRecognizer`
//! trait - callers never know which one they're holding, mirroring
//! `cip_ai_speech`'s three `SpeechEngine` implementations exactly.
//!
//! Phase 7.2 adds an in-app enrollment path on top of Phase 7.1's engine:
//! [`ManifestSong`], [`read_manifest_entries`], [`write_manifest_entries`],
//! and [`validate_reference_wav`] let a Tauri command list/upsert the
//! manifest and validate a candidate WAV file before ever copying it into
//! the model directory - see `apps/desktop/src-tauri/src/commands.rs`'s
//! `enroll_acoustic_reference`/`list_acoustic_enrollments`.

pub mod fingerprint;
mod local;
mod null;
mod scripted;

pub use local::{
    read_manifest_entries, validate_reference_wav, write_manifest_entries, LocalAcousticConfig,
    LocalAcousticMusicRecognizer, ManifestSong, MODEL_MANIFEST_FILENAME,
};
pub use null::NullAcousticMusicRecognizer;
pub use scripted::{ScriptedAcousticMusicRecognizer, ScriptedAcousticStep};
