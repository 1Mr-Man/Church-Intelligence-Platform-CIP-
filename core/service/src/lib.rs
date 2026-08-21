//! Service domain: the live-service session lifecycle and the audio
//! capture contract it depends on.

mod audio_engine;
mod service_session;

pub use audio_engine::{AudioDevice, AudioEngine, AudioEngineError, AudioEngineStatus};
pub use service_session::{ServiceSession, ServiceStatus};
