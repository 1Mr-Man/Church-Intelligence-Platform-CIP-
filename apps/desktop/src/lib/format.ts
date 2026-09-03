/** Local `HH:MM:SS` for a stored UTC ISO-8601 timestamp - used by the
 * live transcript and service timeline so the operator sees times in
 * their own clock, not raw ISO strings. */
export function formatClockTime(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleTimeString(undefined, { hour12: false });
}

/** Below this RMS level, `AudioEngineStatus.inputLevel` is already reported
 * as "NO SIGNAL" elsewhere (see `LiveChurchBrain.tsx`) - kept here too so
 * `describeAudioSignal` never mislabels genuine silence as merely "low". */
const NO_SIGNAL_THRESHOLD = 0.01;

/**
 * Phase 14: below this RMS level, a signal is real but quiet enough to
 * produce unreliable transcription - not a lab measurement, a threshold
 * calibrated directly from a real Windows pilot session where 6% input
 * level (Intel Smart Sound Technology array microphone) produced a
 * transcript full of whisper.cpp's own non-speech placeholder captions
 * (see `docs/phase-14-audit.md`). Deliberately a judgment call, not a
 * precise boundary - matches this codebase's own established precedent
 * for heuristic-but-honestly-documented thresholds (e.g.
 * `ai/speech/src/whisper.rs`'s `SILENCE_RMS_THRESHOLD`).
 */
const LOW_SIGNAL_THRESHOLD = 0.15;

/**
 * Phase 18: known Windows loopback/"monitor" capture device name patterns
 * (case-insensitive substring match). A real operator's own report - device
 * "Stereo Mix (Realtek(R) Audio)" selected, input level pinned at 0% for an
 * entire live-service attempt with the video's own audio audibly playing -
 * showed the generic NO SIGNAL/LOW SIGNAL guidance ("move the microphone
 * closer or raise its gain") is actively misleading for a loopback device:
 * it has no physical position or gain control to adjust. A loopback
 * device's real, almost-always-correct failure mode is Windows' own
 * per-device recording level being muted or at 0% (Sound Settings >
 * Recording > that device > Levels), or the target audio not actually
 * routing through the system's default playback output the loopback device
 * mirrors - neither of which this codebase can detect or fix (no
 * cross-platform API exposes a device's own OS-level recording level), so
 * the message points the operator at the right place to check instead of
 * guessing a cause.
 */
const LOOPBACK_DEVICE_NAME_PATTERNS = ["stereo mix", "wave out mix", "what u hear", "loopback", "monitor of"];

function isLoopbackDeviceName(deviceName: string | null): boolean {
  if (!deviceName) return false;
  const lower = deviceName.toLowerCase();
  return LOOPBACK_DEVICE_NAME_PATTERNS.some((pattern) => lower.includes(pattern));
}

/** Turns a raw `AudioEngineStatus.inputLevel` (0..1, or `null` before the
 * first reading arrives) into the exact operator-facing sentence
 * `LiveChurchBrain.tsx`'s audio panel shows - extracted as a pure function
 * so this classification is directly testable without rendering React.
 * See `LOW_SIGNAL_THRESHOLD`'s own docs for why the "LOW SIGNAL" band
 * exists: a bare percentage told a real operator nothing about whether
 * 6% was a problem. `deviceName` (from `AudioEngineStatus.selectedDevice`,
 * the backend's own resolved device name - accurate even when the operator
 * left the picker on "Default device") is optional and defaults to `null`,
 * so every existing caller/test keeps its prior physical-microphone wording
 * unless a loopback device is actually in use - see
 * `LOOPBACK_DEVICE_NAME_PATTERNS`'s own docs for why that distinction
 * matters. */
export function describeAudioSignal(inputLevel: number | null, deviceName: string | null = null): string {
  if (inputLevel == null) return "Capturing — input level not yet reported";
  const isLoopback = isLoopbackDeviceName(deviceName);
  if (inputLevel <= NO_SIGNAL_THRESHOLD) {
    if (isLoopback) {
      return "NO SIGNAL — this loopback device is capturing but receiving no audio (in Windows, open Sound Settings > Recording > this device > Levels and confirm it isn't muted or at 0%, and confirm the audio you want to capture is actually playing through this computer's default output device)";
    }
    return "NO SIGNAL — audio device is capturing but no sound is being detected";
  }
  const percent = Math.round(inputLevel * 100);
  if (inputLevel < LOW_SIGNAL_THRESHOLD) {
    if (isLoopback) {
      return `LOW SIGNAL — input level ${percent}% (raise this computer's system/playback volume; a loopback device mirrors output volume, it has no physical gain to adjust)`;
    }
    return `LOW SIGNAL — input level ${percent}% (move the microphone closer or raise its gain; quiet audio produces unreliable transcription)`;
  }
  return `SIGNAL CAPTURED — input level ${percent}%`;
}
