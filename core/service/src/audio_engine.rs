use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioEngineStatus {
    pub is_capturing: bool,
    pub sample_rate_hz: u32,
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
}

/// The contract for capturing raw audio from an input device during a live
/// service. Implementations live outside `core` (a real cross-platform
/// capture backend is future work); `core/service` only depends on this
/// trait so `ServiceSession` orchestration never couples to a specific
/// audio library.
///
/// `AudioEngine` is deliberately format-agnostic about what happens to the
/// captured audio - it hands frames off (via whatever channel/callback an
/// implementation chooses) for `SpeechEngine` (see `core/ai`) to transcribe.
pub trait AudioEngine: Send + Sync {
    fn list_devices(&self) -> Result<Vec<AudioDevice>, AudioEngineError>;

    fn start(&mut self, device_id: &str) -> Result<(), AudioEngineError>;

    fn stop(&mut self) -> Result<(), AudioEngineError>;

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
        fn start(&mut self, _device_id: &str) -> Result<(), AudioEngineError> {
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
                sample_rate_hz: 16_000,
            }
        }
    }

    #[test]
    fn stub_engine_tracks_capture_state() {
        let mut engine = StubAudioEngine { capturing: false };
        assert!(!engine.status().is_capturing);
        engine.start("default").unwrap();
        assert!(engine.status().is_capturing);
        assert!(matches!(
            engine.start("default"),
            Err(AudioEngineError::AlreadyCapturing)
        ));
        engine.stop().unwrap();
        assert!(!engine.status().is_capturing);
    }
}
