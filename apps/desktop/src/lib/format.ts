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

/** Turns a raw `AudioEngineStatus.inputLevel` (0..1, or `null` before the
 * first reading arrives) into the exact operator-facing sentence
 * `LiveChurchBrain.tsx`'s audio panel shows - extracted as a pure function
 * so this classification is directly testable without rendering React.
 * See `LOW_SIGNAL_THRESHOLD`'s own docs for why the "LOW SIGNAL" band
 * exists: a bare percentage told a real operator nothing about whether
 * 6% was a problem. */
export function describeAudioSignal(inputLevel: number | null): string {
  if (inputLevel == null) return "Capturing — input level not yet reported";
  if (inputLevel <= NO_SIGNAL_THRESHOLD) {
    return "NO SIGNAL — audio device is capturing but no sound is being detected";
  }
  const percent = Math.round(inputLevel * 100);
  if (inputLevel < LOW_SIGNAL_THRESHOLD) {
    return `LOW SIGNAL — input level ${percent}% (move the microphone closer or raise its gain; quiet audio produces unreliable transcription)`;
  }
  return `SIGNAL CAPTURED — input level ${percent}%`;
}
