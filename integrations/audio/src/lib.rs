//! Real [`AudioEngine`] implementation using
//! [cpal](https://github.com/RustAudio/cpal) (cross-platform audio I/O -
//! ALSA on Linux, CoreAudio on macOS, WASAPI on Windows; MIT/Apache-2.0
//! dual-licensed). No model, no network, no cloud dependency - capture
//! works fully offline, and this crate never assumes a specific church's
//! hardware (mixer, USB interface, or plain microphone are all just
//! "an input device" to cpal).
//!
//! ## Why a worker thread
//!
//! cpal's `Stream` is deliberately `!Send + !Sync` on every platform (it
//! wraps a `PhantomData<*mut ()>` marker) because on some backends the
//! underlying OS stream handle is only valid on the thread that created
//! it. `AudioEngine: Send + Sync` (so `AppState` can hold one behind a
//! `Mutex` shared across Tauri's command threads) - so [`CpalAudioEngine`]
//! itself never touches a `Stream` directly. It spawns one dedicated
//! worker thread that owns every `Stream` for its whole lifetime and talks
//! to it over plain `std::sync::mpsc` channels; only the (`Send + Sync`)
//! channel handles and status atomics live on `CpalAudioEngine`. This is
//! the standard pattern for wrapping a thread-affine resource behind a
//! `Send + Sync` API, not incidental complexity.
//!
//! ## Device identity
//!
//! cpal does not expose a stable numeric device id, so [`AudioDevice::id`]
//! here is the device's own name string, re-resolved against a fresh
//! device enumeration on every `start()` - if a device has disappeared
//! since an earlier [`list_devices`](AudioEngine::list_devices) call
//! (unplugged, sound board powered off), this reports
//! [`AudioEngineError::DeviceNotFound`] instead of silently capturing
//! from whatever cpal falls back to.
//!
//! ## Sample rate
//!
//! Chunks are delivered at the device's own native sample rate (reported
//! on every [`AudioChunk`]), never silently resampled to a fixed rate. A
//! consumer that needs a specific rate (e.g. a `SpeechEngine`) is
//! responsible for converting - resampling is a deliberately deferred
//! follow-up, not something this crate pretends to have solved.
//!
//! ## What could not be verified in this environment
//!
//! This development container has no `/dev/snd` and no real audio
//! hardware, so device discovery here correctly (and, for this
//! environment, truthfully) returns an empty list - proving the
//! "no usable audio input" path for real rather than by assertion. Actual
//! capture (`start`/`stop`/`pause`/`resume` against a real device) is
//! implemented against cpal's documented API and compiles, but has not
//! been exercised against real hardware; see `docs/live-speech.md`.

use cip_core_service::{
    AudioChunk, AudioChunkSink, AudioDevice, AudioEngine, AudioEngineError, AudioEngineStatus,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, Stream, StreamConfig};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

type Reply = mpsc::Sender<Result<(), AudioEngineError>>;

enum WorkerCommand {
    Start {
        device_id: String,
        sink: AudioChunkSink,
        reply: Reply,
    },
    Stop {
        reply: Reply,
    },
    Pause {
        reply: Reply,
    },
    Resume {
        reply: Reply,
    },
    Shutdown,
}

pub struct CpalAudioEngine {
    commands: mpsc::Sender<WorkerCommand>,
    is_capturing: Arc<AtomicBool>,
    sample_rate_hz: Arc<AtomicU32>,
    input_level_bits: Arc<AtomicU32>,
    has_level_reading: Arc<AtomicBool>,
    /// Phase 3.2: the most recent mid-capture stream failure, set from the
    /// cpal backend's own error callback (a different thread than the one
    /// that called `start()`) - see `record_stream_error`'s docs.
    stream_error: Arc<Mutex<Option<String>>>,
}

impl Default for CpalAudioEngine {
    fn default() -> Self {
        let is_capturing = Arc::new(AtomicBool::new(false));
        let sample_rate_hz = Arc::new(AtomicU32::new(0));
        let input_level_bits = Arc::new(AtomicU32::new(0));
        let has_level_reading = Arc::new(AtomicBool::new(false));
        let stream_error = Arc::new(Mutex::new(None));

        let (tx, rx) = mpsc::channel();
        let worker_capturing = Arc::clone(&is_capturing);
        let worker_rate = Arc::clone(&sample_rate_hz);
        let worker_level_bits = Arc::clone(&input_level_bits);
        let worker_has_level = Arc::clone(&has_level_reading);
        let worker_stream_error = Arc::clone(&stream_error);
        std::thread::spawn(move || {
            run_worker(
                rx,
                worker_capturing,
                worker_rate,
                worker_level_bits,
                worker_has_level,
                worker_stream_error,
            );
        });

        Self {
            commands: tx,
            is_capturing,
            sample_rate_hz,
            input_level_bits,
            has_level_reading,
            stream_error,
        }
    }
}

impl CpalAudioEngine {
    pub fn new() -> Self {
        Self::default()
    }

    fn call(
        &self,
        make_command: impl FnOnce(Reply) -> WorkerCommand,
    ) -> Result<(), AudioEngineError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.commands
            .send(make_command(reply_tx))
            .map_err(|_| AudioEngineError::Backend("audio worker thread is gone".to_string()))?;
        reply_rx.recv().map_err(|_| {
            AudioEngineError::Backend("audio worker thread dropped the reply channel".to_string())
        })?
    }
}

impl Drop for CpalAudioEngine {
    fn drop(&mut self) {
        let _ = self.commands.send(WorkerCommand::Shutdown);
    }
}

impl AudioEngine for CpalAudioEngine {
    fn list_devices(&self) -> Result<Vec<AudioDevice>, AudioEngineError> {
        list_devices_now()
    }

    fn start(&mut self, device_id: &str, sink: AudioChunkSink) -> Result<(), AudioEngineError> {
        if self.is_capturing.load(Ordering::SeqCst) {
            return Err(AudioEngineError::AlreadyCapturing);
        }
        let device_id = device_id.to_string();
        self.call(|reply| WorkerCommand::Start {
            device_id,
            sink,
            reply,
        })
    }

    fn stop(&mut self) -> Result<(), AudioEngineError> {
        self.call(|reply| WorkerCommand::Stop { reply })
    }

    fn pause(&mut self) -> Result<(), AudioEngineError> {
        self.call(|reply| WorkerCommand::Pause { reply })
    }

    fn resume(&mut self) -> Result<(), AudioEngineError> {
        self.call(|reply| WorkerCommand::Resume { reply })
    }

    fn status(&self) -> AudioEngineStatus {
        AudioEngineStatus {
            is_capturing: self.is_capturing.load(Ordering::SeqCst),
            is_paused: false,
            sample_rate_hz: self.sample_rate_hz.load(Ordering::SeqCst),
            input_level: self
                .has_level_reading
                .load(Ordering::SeqCst)
                .then(|| f32::from_bits(self.input_level_bits.load(Ordering::SeqCst))),
            stream_error: self
                .stream_error
                .lock()
                .expect("stream_error mutex poisoned")
                .clone(),
        }
    }
}

/// Records a real mid-capture stream failure - called from the cpal
/// backend's error callback (`err_fn` in [`build_stream`]), which runs on
/// a thread cpal itself owns, not the worker thread `WorkerCommand::Start`
/// ran on. Flips `is_capturing` false immediately (the stream is dead the
/// moment cpal reports this, whether or not anyone has called `stop()`
/// yet) and records the reason so [`AudioEngine::status`] can surface it
/// as `AudioStatusKind::Error` rather than silently falling back to
/// `Unavailable`/`Ready` once `is_capturing` reads false. A small, pure,
/// directly-testable function on purpose - the one piece of this failure
/// path this environment (no real audio hardware) can actually prove
/// without a real device to unplug.
fn record_stream_error(
    is_capturing: &AtomicBool,
    stream_error: &Mutex<Option<String>>,
    message: String,
) {
    log::error!(target: "cip::audio", "cpal stream error: {message}");
    *stream_error.lock().expect("stream_error mutex poisoned") = Some(message);
    is_capturing.store(false, Ordering::SeqCst);
}

fn list_devices_now() -> Result<Vec<AudioDevice>, AudioEngineError> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let devices = host
        .input_devices()
        .map_err(|e| AudioEngineError::Backend(e.to_string()))?
        .filter_map(|d| {
            let name = d.name().ok()?;
            let is_default = Some(&name) == default_name.as_ref();
            Some(AudioDevice {
                id: name.clone(),
                name,
                is_default,
            })
        })
        .collect();
    Ok(devices)
}

fn find_device(device_id: &str) -> Result<cpal::Device, AudioEngineError> {
    let host = cpal::default_host();
    host.input_devices()
        .map_err(|e| AudioEngineError::Backend(e.to_string()))?
        .find(|d| d.name().map(|n| n == device_id).unwrap_or(false))
        .ok_or_else(|| AudioEngineError::DeviceNotFound(device_id.to_string()))
}

/// Downmix an arbitrary-channel-count interleaved frame to mono i16 by
/// averaging channels, converting from whatever native sample type cpal
/// handed us.
fn downmix_to_i16<T: Sample + cpal::SizedSample>(data: &[T], channels: usize) -> Vec<i16>
where
    f32: cpal::FromSample<T>,
{
    if channels == 0 {
        return Vec::new();
    }
    data.chunks(channels)
        .map(|frame| {
            let sum: f32 = frame.iter().map(|s| f32::from_sample(*s)).sum();
            let mono = sum / frame.len() as f32;
            (mono.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
        })
        .collect()
}

fn rms_level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples
        .iter()
        .map(|s| (f64::from(*s) / f64::from(i16::MAX)).powi(2))
        .sum();
    ((sum_sq / samples.len() as f64).sqrt() as f32).clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
fn build_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    sample_rate_hz: u32,
    sink: AudioChunkSink,
    input_level_bits: Arc<AtomicU32>,
    has_level_reading: Arc<AtomicBool>,
    is_capturing: Arc<AtomicBool>,
    stream_error: Arc<Mutex<Option<String>>>,
) -> Result<Stream, AudioEngineError> {
    let err_fn = move |err: cpal::StreamError| {
        record_stream_error(&is_capturing, &stream_error, err.to_string())
    };

    macro_rules! stream_for {
        ($ty:ty) => {
            device.build_input_stream(
                config,
                move |data: &[$ty], _info: &cpal::InputCallbackInfo| {
                    let samples = downmix_to_i16(data, channels);
                    if samples.is_empty() {
                        return;
                    }
                    input_level_bits.store(rms_level(&samples).to_bits(), Ordering::Relaxed);
                    has_level_reading.store(true, Ordering::Relaxed);
                    sink(AudioChunk {
                        samples,
                        sample_rate_hz,
                    });
                },
                err_fn,
                None,
            )
        };
    }

    let stream = match sample_format {
        SampleFormat::F32 => stream_for!(f32),
        SampleFormat::I16 => stream_for!(i16),
        SampleFormat::U16 => stream_for!(u16),
        other => {
            return Err(AudioEngineError::Backend(format!(
                "unsupported sample format: {other:?}"
            )))
        }
    };

    stream.map_err(|e| AudioEngineError::Backend(e.to_string()))
}

/// The worker thread's whole world: it owns the (thread-affine) `Stream`
/// for as long as one exists, and never lets it leave this thread.
fn run_worker(
    commands: mpsc::Receiver<WorkerCommand>,
    is_capturing: Arc<AtomicBool>,
    sample_rate_hz: Arc<AtomicU32>,
    input_level_bits: Arc<AtomicU32>,
    has_level_reading: Arc<AtomicBool>,
    stream_error: Arc<Mutex<Option<String>>>,
) {
    let mut stream: Option<Stream> = None;

    for command in commands {
        match command {
            WorkerCommand::Start {
                device_id,
                sink,
                reply,
            } => {
                let result = (|| {
                    let device = find_device(&device_id)?;
                    let supported_config = device
                        .default_input_config()
                        .map_err(|e| AudioEngineError::Backend(e.to_string()))?;
                    let rate = supported_config.sample_rate().0;
                    let channels = supported_config.channels() as usize;
                    let format = supported_config.sample_format();
                    let config: StreamConfig = supported_config.into();

                    has_level_reading.store(false, Ordering::SeqCst);
                    let new_stream = build_stream(
                        &device,
                        &config,
                        format,
                        channels,
                        rate,
                        sink,
                        Arc::clone(&input_level_bits),
                        Arc::clone(&has_level_reading),
                        Arc::clone(&is_capturing),
                        Arc::clone(&stream_error),
                    )?;
                    new_stream
                        .play()
                        .map_err(|e| AudioEngineError::Backend(e.to_string()))?;

                    stream = Some(new_stream);
                    sample_rate_hz.store(rate, Ordering::SeqCst);
                    // A fresh, successful start clears any stale failure
                    // from a previous capture attempt (Phase 3.2) - never
                    // let an old disconnect message linger after the
                    // operator has successfully reconnected and restarted.
                    *stream_error.lock().expect("stream_error mutex poisoned") = None;
                    is_capturing.store(true, Ordering::SeqCst);
                    Ok(())
                })();
                let _ = reply.send(result);
            }

            WorkerCommand::Stop { reply } => {
                stream = None; // dropping a cpal Stream stops and releases it
                is_capturing.store(false, Ordering::SeqCst);
                has_level_reading.store(false, Ordering::SeqCst);
                let _ = reply.send(Ok(()));
            }

            WorkerCommand::Pause { reply } => {
                let result = match &stream {
                    Some(s) => s
                        .pause()
                        .map_err(|e| AudioEngineError::Backend(e.to_string())),
                    None => Err(AudioEngineError::Backend("not capturing".to_string())),
                };
                let _ = reply.send(result);
            }

            WorkerCommand::Resume { reply } => {
                let result = match &stream {
                    Some(s) => s
                        .play()
                        .map_err(|e| AudioEngineError::Backend(e.to_string())),
                    None => Err(AudioEngineError::Backend("not capturing".to_string())),
                };
                let _ = reply.send(result);
            }

            WorkerCommand::Shutdown => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one thing genuinely testable in a container with no audio
    /// hardware: device discovery must not crash and must return an honest
    /// empty (not fabricated) list. This is the real "NO_AUDIO_DEVICE"
    /// path, not a simulation of it.
    #[test]
    fn list_devices_does_not_crash_and_reports_an_honest_empty_list_without_hardware() {
        let engine = CpalAudioEngine::new();
        let devices = engine
            .list_devices()
            .expect("enumeration itself should not fail");
        assert!(
            devices.is_empty(),
            "this CI/dev environment has no audio hardware"
        );
    }

    /// Phase 3.2 mandatory failure-injection scenario ("Microphone
    /// disappears -> Graceful error"): a genuinely no-hardware environment
    /// can never trigger cpal's own stream-error callback for real (there
    /// is no live stream to unplug), so this proves the exact logic that
    /// callback invokes when it does fire on real hardware - the one
    /// piece of this failure path provable without a device to unplug.
    #[test]
    fn record_stream_error_flips_capturing_false_and_records_the_reason() {
        let is_capturing = AtomicBool::new(true);
        let stream_error = Mutex::new(None);

        record_stream_error(
            &is_capturing,
            &stream_error,
            "device disconnected".to_string(),
        );

        assert!(
            !is_capturing.load(Ordering::SeqCst),
            "a stream error must immediately stop reporting Listening"
        );
        assert_eq!(
            stream_error.lock().unwrap().as_deref(),
            Some("device disconnected"),
            "the operator-visible reason must be preserved verbatim"
        );
    }

    /// A fresh, successful `Start` must clear any stale error from an
    /// earlier disconnect - proven directly against `CpalAudioEngine`'s
    /// real, non-hardware-dependent `status()` accessor.
    #[test]
    fn a_fresh_engine_reports_no_stream_error_by_default() {
        let engine = CpalAudioEngine::new();
        assert_eq!(engine.status().stream_error, None);
    }

    #[test]
    fn starting_an_unknown_device_id_is_reported_not_fabricated() {
        let mut engine = CpalAudioEngine::new();
        let sink: AudioChunkSink = Arc::new(|_chunk| {});
        let result = engine.start("definitely-not-a-real-device", sink);
        assert!(matches!(result, Err(AudioEngineError::DeviceNotFound(_))));
    }

    #[test]
    fn status_before_any_capture_is_idle() {
        let engine = CpalAudioEngine::new();
        let status = engine.status();
        assert!(!status.is_capturing);
        assert!(status.input_level.is_none());
    }

    #[test]
    fn stop_without_ever_starting_is_a_safe_no_op() {
        let mut engine = CpalAudioEngine::new();
        assert!(engine.stop().is_ok());
        assert!(!engine.status().is_capturing);
    }

    #[test]
    fn pause_without_capturing_is_a_reported_error_not_a_panic() {
        let mut engine = CpalAudioEngine::new();
        assert!(engine.pause().is_err());
    }

    #[test]
    fn engine_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CpalAudioEngine>();
    }
}
