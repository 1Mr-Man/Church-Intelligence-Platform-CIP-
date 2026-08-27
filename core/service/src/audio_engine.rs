use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioEngineStatus {
    pub is_capturing: bool,
    pub is_paused: bool,
    pub sample_rate_hz: u32,
    /// Coarse RMS input level in `0.0..=1.0`, if the backend can report one
    /// ("reporting input level where practical" - never fabricated when it
    /// can't).
    pub input_level: Option<f32>,
    /// Phase 3.2 hardware-pilot hardening: the most recent mid-capture
    /// stream failure reported by the backend (e.g. a device physically
    /// unplugged while listening) - `None` when capture has never failed,
    /// or has been cleared by a subsequent successful `start()`. This is
    /// distinct from `AudioEngineError` (a synchronous failure returned
    /// from a command call): a real cpal stream error arrives later, on
    /// the audio backend's own callback thread, with no `Result` to
    /// return it through - see `integrations/audio`'s `CpalAudioEngine`
    /// for where this is actually set.
    pub stream_error: Option<String>,
}

#[derive(Debug, Error)]
pub enum AudioEngineError {
    #[error("no audio device available")]
    NoDevice,
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("audio engine already capturing")]
    AlreadyCapturing,
    #[error("audio backend error: {0}")]
    Backend(String),
    #[error("{0} is not supported by this audio engine")]
    Unsupported(&'static str),
}

/// One chunk of captured audio handed from an `AudioEngine` to whatever
/// consumes it - normally a `SpeechEngine`, wired up by `core/service`'s
/// orchestrator. `AudioEngine` itself has no idea a `SpeechEngine` exists;
/// it only knows how to produce normalized chunks. Samples are mono PCM16
/// at `sample_rate_hz` regardless of the underlying device's native
/// format/channel count, so nothing downstream needs device-specific
/// conversion logic.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub samples: Vec<i16>,
    pub sample_rate_hz: u32,
}

/// Where an `AudioEngine` delivers captured chunks. A plain callback
/// (rather than, say, an async channel) keeps the trait executor-agnostic -
/// callers decide their own concurrency model around it. `Arc` (not `Box`)
/// because the same sink is invoked repeatedly from a capture thread the
/// engine owns, for as long as capture runs.
pub type AudioChunkSink = Arc<dyn Fn(AudioChunk) + Send + Sync>;

/// The contract for capturing raw audio from an input device during a live
/// service. Implementations live outside `core` (see `integrations/audio`);
/// `core/service` only depends on this trait so `ServiceSession`
/// orchestration never couples to a specific audio library or device.
///
/// `AudioEngine` is deliberately format-agnostic about what happens to the
/// captured audio beyond normalizing it - it hands chunks to whatever
/// `AudioChunkSink` the caller provides at `start` time; a `SpeechEngine`
/// (see `core/ai`) is the expected consumer, but this trait has no
/// knowledge of that.
pub trait AudioEngine: Send + Sync {
    fn list_devices(&self) -> Result<Vec<AudioDevice>, AudioEngineError>;

    /// Start capturing from `device_id`, delivering chunks to `sink` as
    /// they arrive. Returns once capture has actually started (or
    /// definitively failed to) - it does not block for the lifetime of
    /// capture, which continues on a background thread/task the
    /// implementation owns until [`stop`](Self::stop) is called.
    fn start(&mut self, device_id: &str, sink: AudioChunkSink) -> Result<(), AudioEngineError>;

    fn stop(&mut self) -> Result<(), AudioEngineError>;

    /// Pause capture without releasing the device, if the backend supports
    /// it. Default: unsupported - callers should fall back to
    /// [`stop`](Self::stop) and re-[`start`](Self::start) if a concrete
    /// engine doesn't override this.
    fn pause(&mut self) -> Result<(), AudioEngineError> {
        Err(AudioEngineError::Unsupported("pause"))
    }

    /// Resume a paused capture. Default: unsupported.
    fn resume(&mut self) -> Result<(), AudioEngineError> {
        Err(AudioEngineError::Unsupported("resume"))
    }

    fn status(&self) -> AudioEngineStatus;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubAudioEngine {
        capturing: bool,
    }

    impl AudioEngine for StubAudioEngine {
        fn list_devices(&self) -> Result<Vec<AudioDevice>, AudioEngineError> {
            Ok(vec![AudioDevice {
                id: "default".into(),
                name: "Stub Input".into(),
                is_default: true,
            }])
        }
        fn start(
            &mut self,
            _device_id: &str,
            _sink: AudioChunkSink,
        ) -> Result<(), AudioEngineError> {
            if self.capturing {
                return Err(AudioEngineError::AlreadyCapturing);
            }
            self.capturing = true;
            Ok(())
        }
        fn stop(&mut self) -> Result<(), AudioEngineError> {
            self.capturing = false;
            Ok(())
        }
        fn status(&self) -> AudioEngineStatus {
            AudioEngineStatus {
                is_capturing: self.capturing,
                is_paused: false,
                sample_rate_hz: 16_000,
                input_level: None,
                stream_error: None,
            }
        }
    }

    fn noop_sink() -> AudioChunkSink {
        std::sync::Arc::new(|_chunk| {})
    }

    #[test]
    fn stub_engine_tracks_capture_state() {
        let mut engine = StubAudioEngine { capturing: false };
        assert!(!engine.status().is_capturing);
        engine.start("default", noop_sink()).unwrap();
        assert!(engine.status().is_capturing);
        assert!(matches!(
            engine.start("default", noop_sink()),
            Err(AudioEngineError::AlreadyCapturing)
        ));
        engine.stop().unwrap();
        assert!(!engine.status().is_capturing);
    }

    #[test]
    fn pause_and_resume_default_to_unsupported() {
        let mut engine = StubAudioEngine { capturing: false };
        assert!(matches!(
            engine.pause(),
            Err(AudioEngineError::Unsupported("pause"))
        ));
        assert!(matches!(
            engine.resume(),
            Err(AudioEngineError::Unsupported("resume"))
        ));
    }
}
